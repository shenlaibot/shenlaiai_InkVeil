//! 检测器：正则规则 + 校验器 + 上下文置信度提升。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::merge::{arbitrate, default_type_priority};
use crate::rules::{
    ID_ADDRESS, ID_BANK_ACCOUNT, ID_BANK_CARD, ID_CONTRACT_NO, ID_ID_CARD, ID_NAME, is_valid_id_card,
    luhn_valid,
};
use crate::{Hit, HitSource, RuleDef};
use regex::Regex;

const CONTEXT_WINDOW: usize = 20;
const CONTEXT_BOOST: f64 = 0.15;

/// 需要数字边界约束的规则（替代 regex 不支持的 look-around）。
/// date 加入后，「12345-67-89」中的「2345-67-89」子串因左侧是数字被拦截，
/// 避免长数字串内部误报日期。
const DIGIT_BOUNDARY_RULES: [&str; 7] = [
    "mobile",
    "id_card",
    "bank_card",
    "credit_code",
    "ip",
    "phone",
    "date",
];

/// Luhn 不通过的卡号候选置信度（仍上报、交由用户裁决；隐私工具宁严勿漏）
const BANK_CARD_LUHN_FAIL_CONFIDENCE: f64 = 0.75;

/// 已编译正则的全局缓存（按 pattern 复用；批量/分块扫描不再重复编译）。
static REGEX_CACHE: OnceLock<Mutex<HashMap<String, Arc<Regex>>>> = OnceLock::new();

/// 编译（或取缓存的）正则；编译失败返回错误。
fn cached_regex(pattern: &str) -> Result<Arc<Regex>, String> {
    let cache = REGEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(re) = guard.get(pattern) {
        return Ok(re.clone());
    }
    let re = Regex::new(pattern).map_err(|e| format!("正则编译失败: {e}"))?;
    let re = Arc::new(re);
    guard.insert(pattern.to_string(), re.clone());
    Ok(re)
}

/// 命中两侧均不是数字（避免身份证/卡号内部子串误报）。
/// 注：mobile 规则的正则已确保首尾都是数字，分隔符只出现在数字之间，
/// 因此只需检查数字边界即可（空格/- 等是正常的词边界）。
fn digit_boundary_ok(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !matches!(before, Some(c) if c.is_ascii_digit())
        && !matches!(after, Some(c) if c.is_ascii_digit())
}

pub struct Detector;

impl Detector {
    /// 对全文按启用规则做正则匹配 + 格式校验 + 上下文置信度提升。
    ///
    /// 兼容入口：坏规则只被跳过，其余规则照常检测（不再因一条坏规则整体 0 命中）。
    /// 需要知道哪些规则失效的调用方请用 [`Self::detect_isolated`]；
    /// 需要失败即报错的调用方请用 [`Self::try_detect`]。
    pub fn detect(text: &str, rules: &[RuleDef]) -> Vec<Hit> {
        Self::detect_isolated(text, rules).0
    }

    /// 严格版本：任一启用规则的正则编译失败即返回 `Err(规则 id + 错误)`。
    pub fn try_detect(text: &str, rules: &[RuleDef]) -> Result<Vec<Hit>, String> {
        let (hits, errors) = Self::detect_isolated(text, rules);
        if errors.is_empty() {
            Ok(hits)
        } else {
            Err(errors.join("；"))
        }
    }

    /// 逐规则隔离版本：返回 (命中, 失效规则的错误列表)。
    /// 一条规则写错只让该规则失效，其余规则照常运行；调用方必须把错误展示给用户，
    /// 绝不能在存在失效规则时报告“未发现敏感信息”。
    pub fn detect_isolated(text: &str, rules: &[RuleDef]) -> (Vec<Hit>, Vec<String>) {
        let mut errors: Vec<String> = Vec::new();
        let mut compiled: Vec<(&RuleDef, Arc<Regex>)> = Vec::new();
        for r in rules.iter().filter(|r| r.enabled) {
            match cached_regex(&r.regex) {
                Ok(re) => compiled.push((r, re)),
                Err(e) => errors.push(format!("规则 [{}] {}", r.id, e)),
            }
        }

        let mut hits: Vec<Hit> = Vec::new();
        for (rule, re) in compiled {
            for caps in re.captures_iter(text) {
                let full = caps.get(0).expect("captures 的 0 号分组恒为整段匹配");
                // 规则正则可带一个「仅命中本体」的捕获组（如公司名/开户银行前面
                // 的「户名：/开户银行：」等上下文前缀），此时优先取捕获组 1 作为命中
                // 文本；其余规则无捕获组或捕获组与整段一致，行为保持不变。
                let (t_start, mut t_end, mut t_text) = match caps.get(1) {
                    Some(g) => (g.start(), g.end(), g.as_str().to_string()),
                    None => (full.start(), full.end(), full.as_str().to_string()),
                };
                if DIGIT_BOUNDARY_RULES.contains(&rule.id.as_str())
                    && !digit_boundary_ok(text, full.start(), full.end())
                {
                    continue;
                }
                // P0 类型附加格式校验
                let mut confidence = rule.confidence;
                let piece = &text[t_start..t_end];
                match rule.id.as_str() {
                    ID_ID_CARD if !is_valid_id_card(&piece.replace(|c: char| c.is_whitespace(), "")) => continue,
                    // Luhn 不通过不作硬过滤：16-19 位数字串在文档中几乎必然是
                    // 卡号类敏感信息，降置信度上报、交由用户裁决（宁严勿漏）。
                    ID_BANK_CARD if !luhn_valid(piece) => {
                        confidence = BANK_CARD_LUHN_FAIL_CONFIDENCE;
                    }
                    // 姓名：命中串若紧跟「公司/银行/集团/事务所/研究院/有限」等组织后缀，
                    // 说明命中的是公司名或行名本体的一部分（如“收款人：成都云图科技有限公司”），
                    // 而非人姓名，此时跳过。
                    ID_NAME => {
                        // 姓名捕获组贪婪取 2–4 个汉字，会把后面的虚词/连词一并吞掉
                        //（“王建国于”“司马懿于”“刘德于今”）。第 3 个字起遇到终止字即截断。
                        const NAME_STOP: [char; 16] = [
                            '于', '与', '和', '及', '诉', '等', '因', '为',
                            '向', '对', '在', '将', '已', '未', '系', '的',
                        ];
                        // 第 3 个字起遇到终止字，或「一案」（“被告王五一案”）即截断
                        let cut = t_text
                            .char_indices()
                            .enumerate()
                            .find(|(ci, (off, c))| {
                                *ci >= 2
                                    && (NAME_STOP.contains(c) || t_text[*off..].starts_with("一案"))
                            })
                            .map(|(_, (off, _))| off);
                        if let Some(off) = cut {
                            t_end = t_start + off;
                            t_text.truncate(off);
                        }
                        // 诉讼角色前缀（原告/被告/审判长…）后易跟普通词组（“原告方面认为”
                        // “被告人民…”）：这类前缀要求名字以常见姓氏开头。其余前缀保持原行为，
                        // 避免对低频姓氏漏报。
                        const LIT_ROLES: [&str; 17] = [
                            "原告", "被告", "第三人", "上诉人", "被上诉人", "被申请人", "申请执行人",
                            "被执行人", "再审申请人", "审判长", "审判员", "人民陪审员", "书记员",
                            "法官助理", "辩护人", "公诉人", "证人",
                        ];
                        let before = text[..t_start].trim_end_matches([':', '：', ' ', '\t']);
                        if LIT_ROLES.iter().any(|r| before.ends_with(r))
                            && !crate::name::starts_with_surname(&t_text)
                        {
                            continue;
                        }
                        if t_text.chars().count() < 2 {
                            continue;
                        }
                        // 用「字符」而非「字节」取窗口，避免多字节字符被截断；
                        // 窗口止于换行（段落/单元格边界）：下一行的「手机/电话」等
                        // 字段名不得误杀上一行的真实姓名；同时覆盖捕获组本体开头，
                        // 拦截「联系人制度…」这类非人名词汇。
                        let seg: String = text[t_start..]
                            .chars()
                            .take_while(|c| *c != '\n')
                            .take(14)
                            .collect();
                        const ORG_SUFFIX: [&str; 7] =
                            ["公司", "银行", "集团", "事务所", "研究院", "有限", "合伙"];
                        if ORG_SUFFIX.iter().any(|k| seg.contains(k)) { continue; }
                        // 命中后紧跟「制度/体系/工作」「身份证/证件/手机号」等常见非姓名短语时跳过，
                        // 含跨单元格场景：表头「姓名\n身份证号」中「身份证号」不得被粘为姓名。
                        const NON_NAME_TAIL: [&str; 16] = [
                            "制度", "体系", "工作", "名单", "电话", "信息", "身份证", "证件",
                            "证件号", "手机", "手机号", "账号", "职务", "单位", "邮箱", "地址",
                        ];
                        if NON_NAME_TAIL.iter().any(|k| seg.contains(k)) { continue; }
                    }
                    // 银行账号：去掉分隔符后需 8–25 位数字
                    ID_BANK_ACCOUNT => {
                        let n = piece.chars().filter(|c| c.is_ascii_digit()).count();
                        if !(8..=25).contains(&n) {
                            continue;
                        }
                    }
                    // 编号必须含数字（否则只是普通英文词/符号）
                    ID_CONTRACT_NO if !piece.chars().any(|c| c.is_ascii_digit()) => continue,
                    // 地址必须含行政区划/道路/门牌类字样，排除“地址：见附件”之类
                    ID_ADDRESS => {
                        const ADDR_HINT: [char; 12] =
                            ['省', '市', '区', '县', '路', '街', '镇', '乡', '村', '号', '巷', '道'];
                        if !piece.chars().any(|c| ADDR_HINT.contains(&c)) {
                            continue;
                        }
                    }
                    _ => {}
                }
                hits.push(Hit {
                    rule_id: rule.id.clone(),
                    rule_label: rule.label.clone(),
                    start: t_start,
                    end: t_end,
                    text: t_text,
                    confidence,
                    source: HitSource::Rule,
                });
            }
        }
        // 上下文关键词提升置信度（来源保持 Rule，置信度封顶 1.0）
        // 窗口排除命中本体本身：公司名「成都云图科技有限公司」自身含「公司」
        // 关键词，若不排除则每次命中必 +0.15，提信机制退化为常数偏移。
        for h in hits.iter_mut() {
            let rule = rules.iter().find(|r| r.id == h.rule_id);
            if let Some(r) = rule
                && !r.context_keywords.is_empty()
            {
                let lo = text.floor_char_boundary(h.start.saturating_sub(CONTEXT_WINDOW));
                let hi = text.ceil_char_boundary((h.end + CONTEXT_WINDOW).min(text.len()));
                let before = &text[lo..h.start];
                let after = &text[h.end..hi];
                if r.context_keywords
                    .iter()
                    .any(|k| before.contains(k.as_str()) || after.contains(k.as_str()))
                {
                    h.confidence = (h.confidence + CONTEXT_BOOST).min(1.0);
                    h.source = HitSource::Context;
                }
            }
        }
        // 姓名启发式补充：姓氏词典 + 名字用字频，不依赖前缀即可召回低频人名
        // （表格单元格内的「王青/张三丰」、变前缀「甲方指定王秀英…为项目对接人」等）。
        // 放在语境提升之后，避免启发式命中的来源被误改成 Context。
        // 仅在「姓名」规则启用时补充--用户关闭姓名规则即代表不需要该类检测。
        if rules.iter().any(|r| r.enabled && r.id == ID_NAME) {
            hits.append(&mut crate::name::detect_names(text));
        }
        // 统一裁决：与 pipeline 的 merge_hits 委托同一 arbitrate
        // （类型优先级 -> 置信度 -> start，贪心去重叠），
        // 保证同一组重叠命中在任何调用点得到相同结果。
        let priority = default_type_priority();
        (arbitrate(hits, &priority), errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::builtin_rules;

    #[test]
    fn mobile_hit() {
        let hits = Detector::detect("请联系张律师：13800138000 谢谢", &builtin_rules());
        // 手机号命中（张律师同时被姓名启发式召回，两者不相干重叠）
        let mobile = hits
            .iter()
            .find(|h| h.rule_id == "mobile")
            .expect("手机号未命中");
        assert_eq!(mobile.text, "13800138000");
        assert!(
            hits.iter().any(|h| h.rule_id == "name" && h.text == "张律师"),
            "张律师应被姓名启发式召回: {hits:?}"
        );
    }

    #[test]
    fn mobile_with_separators_detected() {
        let rules = builtin_rules();
        // 各种分隔符格式的手机号都应被识别
        let cases = [
            ("138 0013 8000", "空格分隔"),
            ("138-0013-8000", "中划线分隔"),
            ("138.0013.8000", "点号分隔"),
            ("138_0013_8000", "下划线分隔"),
            ("138·0013·8000", "间隔号分隔"),
            ("手机：138 0013 8000", "上下文+空格"),
            ("联系电话 138-0013-8000。", "上下文+中划线"),
        ];
        for (text, desc) in cases {
            let hits = Detector::detect(text, &rules);
            let mobile_hits: Vec<_> = hits.iter().filter(|h| h.rule_id == "mobile").collect();
            assert_eq!(mobile_hits.len(), 1, "{} 未命中: {}", desc, text);
            // 命中的数字部分应该是 11 位
            let digit_count: usize = mobile_hits[0]
                .text
                .chars()
                .filter(|c| c.is_ascii_digit())
                .count();
            assert_eq!(
                digit_count, 11,
                "{} 数字位数不对: {}",
                desc, mobile_hits[0].text
            );
        }
    }

    #[test]
    fn mobile_separator_not_matched_inside_digits() {
        // 长数字串内部不应被误判为带分隔符的手机号
        let rules = builtin_rules();
        let text = "6222020200088888884";
        let hits = Detector::detect(text, &rules);
        assert!(
            !hits.iter().any(|h| h.rule_id == "mobile"),
            "长数字串不应误报手机号: {text}"
        );
    }

    #[test]
    fn eight_digits_not_mobile() {
        let hits = Detector::detect("编号 12345678", &builtin_rules());
        assert!(!hits.iter().any(|h| h.rule_id == "mobile"));
    }

    #[test]
    fn id_card_hit_and_reject_invalid() {
        let rules = builtin_rules();
        // 合法身份证（自动构造校验位）
        let valid = {
            let s17 = "11010119840320123";
            let last = crate::rules::id_card_check_code(s17);
            format!("{s17}{last}")
        };
        let hits = Detector::detect(&format!("身份证号:{valid}"), &rules);
        assert!(hits.iter().any(|h| h.rule_id == "id_card"));
        // 17 位数字不命中
        let hits = Detector::detect("12345678901234567", &rules);
        assert!(!hits.iter().any(|h| h.rule_id == "id_card"));
        // 校验位错误不命中
        let bad = "110101198403201233";
        let hits = Detector::detect(bad, &rules);
        assert!(!hits.iter().any(|h| h.rule_id == "id_card"));
    }

    #[test]
    fn bank_card_luhn_filter() {
        let rules = builtin_rules();
        let hits = Detector::detect("卡号 4111111111111111", &rules);
        assert!(hits.iter().any(|h| h.rule_id == "bank_card"));
        // Luhn 不通过：仍上报但降置信度（宁严勿漏，交由用户裁决）
        let hits = Detector::detect("编号 4111111111111112", &rules);
        let h = hits.iter().find(|h| h.rule_id == "bank_card").unwrap();
        assert!(h.confidence < 0.9);
    }

    #[test]
    fn phone_not_matched_inside_long_digits() {
        // 卡号/证件号内部的 0xx-xxxxxxx 子串不得误报为固定电话
        let rules = builtin_rules();
        for text in ["6222020200088888884", "440301197407210509"] {
            let hits = Detector::detect(text, &rules);
            assert!(
                !hits.iter().any(|h| h.rule_id == "phone"),
                "固定电话误报: {text}"
            );
        }
    }

    #[test]
    fn mobile_not_classified_as_bank_card() {
        // 11 位手机号不得命中银行卡号（正则已收窄到 16-19 位）
        let rules = builtin_rules();
        let hits = Detector::detect("联系 13900139000", &rules);
        assert!(hits.iter().any(|h| h.rule_id == "mobile"));
        assert!(!hits.iter().any(|h| h.rule_id == "bank_card"));
    }

    #[test]
    fn bank_card_19_digits_detected() {
        // 19 位 Luhn 合法卡号完整命中
        let rules = builtin_rules();
        let hits = Detector::detect("收款账户 6222020200088888884", &rules);
        let h = hits.iter().find(|h| h.rule_id == "bank_card").unwrap();
        assert_eq!(h.text, "6222020200088888884");
        assert!(h.confidence >= 0.95);
    }

    #[test]
    fn context_boost_and_source() {
        let hits = Detector::detect("手机：13800138000", &builtin_rules());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].confidence, 1.0);
        assert_eq!(hits[0].source, HitSource::Context);
    }

    #[test]
    fn context_boost_ignores_hit_body() {
        // 命中本体自身含关键词不得触发自我提升：公司名「成都云图科技有限公司」
        // 里的「公司」不算上下文证据；必须前后出现「户名/开户」等才算。
        let rules = builtin_rules();
        // 前缀「乙方」命中，但前后 20 字符无 context_keywords（户名/开户/公司…）
        let hits = Detector::detect("乙方向成都云图科技有限公司付款", &rules);
        let h = hits
            .iter()
            .find(|h| h.rule_id == "company_name")
            .expect("应命中公司名");
        assert_eq!(h.confidence, 0.72, "本体自我包含关键词不得提升: {:?}", h);
        assert_eq!(h.source, HitSource::Rule);
        // 上下文真有「户名」时才提升
        let hits = Detector::detect("户名：成都云图科技有限公司", &rules);
        let h = hits
            .iter()
            .find(|h| h.rule_id == "company_name")
            .expect("应命中公司名");
        assert!(h.confidence > 0.72, "有真实上下文应提升: {:?}", h);
        assert_eq!(h.source, HitSource::Context);
    }

    #[test]
    fn email_plate_amount_date() {
        let rules = builtin_rules();
        let text = "邮箱 a@b.com，车牌 京A12345，金额 ¥1,200.50，日期 2026-08-10";
        let hits = Detector::detect(text, &rules);
        let ids: Vec<&str> = hits.iter().map(|h| h.rule_id.as_str()).collect();
        assert!(ids.contains(&"email"));
        assert!(ids.contains(&"plate"));
        assert!(ids.contains(&"amount"));
        assert!(ids.contains(&"date"));
    }

    #[test]
    fn credit_code_detected() {
        let rules = builtin_rules();
        for code in [
            "91510100MA6C2KX88Q",
            "91310000MA1K3P7R9T",
            "91110108MA01XXXX1A", // X 字符在允许集内
        ] {
            let hits = Detector::detect(&format!("统一社会信用代码：{code}"), &rules);
            assert!(
                hits.iter().any(|h| h.rule_id == "credit_code" && h.text == code),
                "{code} 未命中"
            );
        }
    }

    #[test]
    fn credit_code_rejects_bad_chars_and_substrings() {
        let rules = builtin_rules();
        // 含禁用字符 S/V/I/O/Z 的代码不命中
        for bad in ["90110145MA6C2KX88S", "91110145MA6C2KX88V"] {
            let hits = Detector::detect(bad, &rules);
            assert!(!hits.iter().any(|h| h.rule_id == "credit_code"), "误报: {bad}");
        }
        // 17 位不应命中
        let hits = Detector::detect("91510108MA6C2KX88", &rules);
        assert!(!hits.iter().any(|h| h.rule_id == "credit_code"));
    }

    #[test]
    fn bank_card_with_space_separators() {
        let rules = builtin_rules();
        let hits = Detector::detect("银行账号：5100 1406 8350 5150 6675", &rules);
        let h = hits
            .iter()
            .find(|h| h.rule_id == "bank_card")
            .expect("带空格银行账号应命中");
        assert_eq!(h.text, "5100 1406 8350 5150 6675");
    }

    #[test]
    fn bank_card_not_match_id_card() {
        // 18 位身份证不得作为银行卡号命中
        let rules = builtin_rules();
        let hits = Detector::detect("身份证号 510104198506123428", &rules);
        assert!(hits.iter().any(|h| h.rule_id == "id_card"));
        assert!(!hits.iter().any(|h| h.rule_id == "bank_card"));
    }

    #[test]
    fn amount_with_thousand_separator() {
        let rules = builtin_rules();
        for text in ["480,000.00", "411,600.00 元", "合计 1,372,000.00", "¥1,372,000.00"] {
            let hits = Detector::detect(text, &rules);
            assert!(hits.iter().any(|h| h.rule_id == "amount"), "金额未命中: {text}");
        }
        // 裸数字不带千分位/单位不判为金额
        for text in ["编号 480000", "占比 0.05%", "共 15000 字"] {
            let hits = Detector::detect(text, &rules);
            assert!(!hits.iter().any(|h| h.rule_id == "amount"), "误报金额: {text}");
        }
    }

    #[test]
    fn amount_chinese_uppercase() {
        let rules = builtin_rules();
        for text in [
            "人民币壹佰叁拾柒万贰仟元整",
            "肆拾万零捌佰元整",
            "金额：贰仟万元",
            "合计 壹拾贰万元整",
        ] {
            let hits = Detector::detect(text, &rules);
            assert!(hits.iter().any(|h| h.rule_id == "amount"), "中文大写金额未命中: {text}");
        }
    }

    #[test]
    fn date_with_spaces() {
        let hits = Detector::detect("自 2026 年 8 月 15 日起", &builtin_rules());
        assert!(hits.iter().any(|h| h.rule_id == "date" && h.text == "2026 年 8 月 15 日"));
    }

    #[test]
    fn date_not_matched_inside_long_digits() {
        // 「12345-67-89」中的「2345-67-89」子串不得误报日期（数字边界约束）
        let rules = builtin_rules();
        let hits = Detector::detect("编号 12345-67-89", &rules);
        assert!(
            !hits.iter().any(|h| h.rule_id == "date"),
            "长数字串内部子串不应误报日期: {hits:?}"
        );
        // 正常日期仍命中
        let hits = Detector::detect("签署于 2024-06-30", &rules);
        assert!(hits.iter().any(|h| h.rule_id == "date"));
    }

    #[test]
    fn company_name_detected() {
        let rules = builtin_rules();
        let cases = [
            ("户名：成都云图科技有限公司", "成都云图科技有限公司"),
            ("收款方：北京智汇数据有限责任公司", "北京智汇数据有限责任公司"),
            ("甲方：上海星辰信息技术股份有限公司", "上海星辰信息技术股份有限公司"),
            ("开户名 四川锦江商务咨询有限公司", "四川锦江商务咨询有限公司"),
        ];
        for (text, expect) in cases {
            let hits = Detector::detect(text, &rules);
            assert!(
                hits.iter().any(|h| h.rule_id == "company_name" && h.text == expect),
                "公司名称未命中或命中文本非本体: {text}"
            );
        }
    }

    #[test]
    fn company_name_avoids_prose_matches() {
        let rules = builtin_rules();
        // 正文里零散的「有限公司」不应整体诱惑误报为具体公司名
        let hits = Detector::detect("该合同中提到的有限公司不构成违约", &rules);
        assert!(!hits.iter().any(|h| h.rule_id == "company_name"));
    }

    #[test]
    fn bank_name_detected() {
        let rules = builtin_rules();
        let cases = [
            ("开户银行：中国建设银行成都高新支行", "中国建设银行成都高新支行"),
            ("开户行：成都农村商业银行", "成都农村商业银行"),
            ("收款账号开户行 招商银行北京分行营业部", "招商银行北京分行营业部"),
        ];
        for (text, expect) in cases {
            let hits = Detector::detect(text, &rules);
            assert!(
                hits.iter().any(|h| h.rule_id == "bank_name" && h.text == expect),
                "开户银行未命中或命中文本非本体: {text}"
            );
        }
    }

    #[test]
    fn name_detected() {
        let rules = builtin_rules();
        let cases = [
            ("签字人：李明辉", "李明辉"),
            ("签字人：陈志强", "陈志强"),
            ("姓名 张伟", "张伟"),
            ("联系人 刘洋", "刘洋"),
            ("法定代表人：王志强", "王志强"),
            ("经办人：孙丽娟", "孙丽娟"),
            ("申请人：赵明", "赵明"),
        ];
        for (text, expect) in cases {
            let hits = Detector::detect(text, &rules);
            assert!(
                hits.iter().any(|h| h.rule_id == "name" && h.text == expect),
                "姓名未命中或命中文本非本体: {text}"
            );
        }
    }

    #[test]
    fn name_avoids_company_false_positive() {
        // 前缀「收款人/申请人」后跟的是公司名而非人名时不得误报；
        // name 规则的防御：命中串尾部若出现组织后缀则跳过。
        let rules = builtin_rules();
        for text in [
            "收款人：成都云图科技有限公司",
            "申请人：上海星辰信息技术股份有限公司",
            "付款人：四川锦江商务咨询有限公司",
            "委托人：北京智汇数据有限责任公司",
        ] {
            let hits = Detector::detect(text, &rules);
            assert!(
                !hits.iter().any(|h| h.rule_id == "name"),
                "公司名被误报为姓名: {text}"
            );
        }
    }

    #[test]
    fn name_avoids_prose_matches() {
        // 正文中零散出现的两字词/无上下文前缀时不当作姓名
        let rules = builtin_rules();
        for text in [
            "本协议由乙方盖章后生效",
            "相关事项已经确认完毕",
            "根据相关规定执行",
            "联系人制度已经建立",
        ] {
            let hits = Detector::detect(text, &rules);
            assert!(
                !hits.iter().any(|h| h.rule_id == "name"),
                "正文误报姓名: {text}"
            );
        }
    }

    #[test]
    fn name_detected_in_table_cells() {
        // 表格单元格内无前缀姓名（此前全部漏检）：姓氏+用字频启发式召回
        let hits = Detector::detect("王青\n刘伟\n张三丰\n赵敏", &builtin_rules());
        let names: Vec<String> = hits
            .iter()
            .filter(|h| h.rule_id == "name")
            .map(|h| h.text.clone())
            .collect();
        for expect in ["王青", "刘伟", "张三丰", "赵敏"] {
            assert!(names.contains(&expect.to_string()), "表内姓名漏检: {expect} in {names:?}");
        }
        // 独立单元格置信度最高，来源为启发式
        let h = hits.iter().find(|h| h.rule_id == "name" && h.text == "张三丰").unwrap();
        assert_eq!(h.confidence, 0.9);
        assert_eq!(h.source, HitSource::Name);
    }

    #[test]
    fn name_variable_prefix_detected() {
        // 变前缀写法：姓名前面是「甲方指定…为项目对接人」，不在前缀后也不在表格里
        let hits = Detector::detect("甲方指定王秀英（乙方）为项目对接人。", &builtin_rules());
        assert!(
            hits.iter().any(|h| h.rule_id == "name" && h.text == "王秀英"),
            "变前缀姓名漏检: {hits:?}"
        );
    }

    #[test]
    fn name_not_cross_cell_header() {
        // 「姓名\n身份证号」相邻表头不得跨单元格误粘为姓名（ID_NAME 正则已排除换行，
        // 且姓名启发式对「姓名/身份证号」这类字段名不产出命中）
        let rules = builtin_rules();
        for text in ["姓名\n身份证号", "姓名\n证件号码", "姓名\n手机号"] {
            let hits = Detector::detect(text, &rules);
            assert!(
                !hits.iter().any(|h| h.rule_id == "name"),
                "跨单元格表头误报姓名: {text} -> {hits:?}"
            );
        }
    }

    #[test]
    fn bad_rule_does_not_disable_other_rules() {
        let mut rules = builtin_rules();
        rules.push(RuleDef {
            id: "broken".into(),
            label: "坏规则".into(),
            regex: "(unclosed".into(),
            enabled: true,
            confidence: 0.9,
            context_keywords: vec![],
        });
        let (hits, errors) = Detector::detect_isolated("手机13800138000", &rules);
        assert!(hits.iter().any(|h| h.rule_id == "mobile"), "其余规则应照常工作");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("broken"));
        assert!(Detector::try_detect("手机13800138000", &rules).is_err());
        assert!(Detector::detect("手机13800138000", &rules).iter().any(|h| h.rule_id == "mobile"));
    }

    #[test]
    fn amount_not_truncated() {
        let hits = Detector::detect("价款为¥1234567.89元，另付1234567.89元", &builtin_rules());
        let amounts: Vec<&str> = hits.iter().filter(|h| h.rule_id == "amount").map(|h| h.text.as_str()).collect();
        assert!(amounts.contains(&"¥1234567.89元"), "{amounts:?}");
        assert!(amounts.contains(&"1234567.89元"), "{amounts:?}");
    }

    #[test]
    fn uppercase_amount_includes_jiao_fen() {
        let hits = Detector::detect("人民币壹佰贰拾叁万肆仟伍佰陆拾柒元捌角玖分", &builtin_rules());
        let a = hits.iter().find(|h| h.rule_id == "amount").expect("大写金额未命中");
        assert!(a.text.ends_with("捌角玖分"), "{}", a.text);
    }

    #[test]
    fn name_not_swallowing_following_chars() {
        for (text, want) in [
            ("申请人司马懿于2024年", "司马懿"),
            ("签字人：王建国于今日", "王建国"),
            ("委托代理人吴彦祖于", "吴彦祖"),
            ("联系人刘德于今日", "刘德"),
        ] {
            let hits = Detector::detect(text, &builtin_rules());
            assert!(hits.iter().any(|h| h.rule_id == "name" && h.text == want), "{text} -> {hits:?}");
            assert!(!hits.iter().any(|h| h.rule_id == "name" && h.text.contains('于')), "{text} -> {hits:?}");
        }
    }

    #[test]
    fn litigation_parties_and_case_no() {
        let rules = builtin_rules();
        let hits = Detector::detect(
            "原告李四诉被告王五一案，审判长赵六，书记员钱七。案号：(2024)京0105民初12345号",
            &rules,
        );
        let names: Vec<&str> = hits.iter().filter(|h| h.rule_id == "name").map(|h| h.text.as_str()).collect();
        for want in ["李四", "王五", "赵六", "钱七"] {
            assert!(names.contains(&want), "缺少 {want}: {names:?}");
        }
        assert!(hits.iter().any(|h| h.rule_id == "case_no" && h.text == "(2024)京0105民初12345号"), "{hits:?}");
        let h2 = Detector::detect("〔2024〕京仲案字第0123号；（2023）最高法民终123号", &rules);
        let cases: Vec<&str> = h2.iter().filter(|h| h.rule_id == "case_no").map(|h| h.text.as_str()).collect();
        assert_eq!(cases.len(), 2, "{cases:?}");
    }

    #[test]
    fn litigation_role_false_positives_avoided() {
        let hits = Detector::detect("原告方面认为合同无效，被告人民法院审理", &builtin_rules());
        assert!(!hits.iter().any(|h| h.rule_id == "name"), "{hits:?}");
    }

    #[test]
    fn company_in_litigation_roles() {
        let hits = Detector::detect("原告：北京字节跳动科技有限公司，被告：张三", &builtin_rules());
        assert!(hits.iter().any(|h| h.rule_id == "company_name" && h.text == "北京字节跳动科技有限公司"), "{hits:?}");
        assert!(hits.iter().any(|h| h.rule_id == "name" && h.text == "张三"), "{hits:?}");
    }

    #[test]
    fn contract_no_and_address() {
        let rules = builtin_rules();
        let hits = Detector::detect("合同编号：HT-2024-0088；员工编号 E-0001；编号：无", &rules);
        let nos: Vec<&str> = hits.iter().filter(|h| h.rule_id == "contract_no").map(|h| h.text.as_str()).collect();
        assert!(nos.contains(&"HT-2024-0088") && nos.contains(&"E-0001"), "{nos:?}");
        assert!(!nos.iter().any(|n| n.contains('无')));
        let h2 = Detector::detect("住址：北京市朝阳区建国路88号院3号楼2单元501室。另见上海市浦东新区世纪大道100号", &rules);
        let addrs: Vec<&str> = h2.iter().filter(|h| h.rule_id == "address").map(|h| h.text.as_str()).collect();
        assert!(addrs.iter().any(|a| a.starts_with("北京市朝阳区建国路88号")), "{addrs:?}");
        assert!(addrs.iter().any(|a| a.starts_with("上海市浦东新区世纪大道100号")), "{addrs:?}");
        let h3 = Detector::detect("地址：见附件", &rules);
        assert!(!h3.iter().any(|h| h.rule_id == "address"), "{h3:?}");
    }

    #[test]
    fn number_format_variants() {
        let rules = builtin_rules();
        let find = |text: &str, id: &str| -> Vec<String> {
            Detector::detect(text, &rules).into_iter().filter(|h| h.rule_id == id).map(|h| h.text).collect()
        };
        // 全角数字手机号
        assert_eq!(find("手机：１３８００１３８０００", "mobile"), vec!["１３８００１３８０００"]);
        // 座机：括号区号 / 空格 / 分机
        assert!(find("座机：(010)62345678", "phone").contains(&"(010)62345678".to_string()));
        assert!(find("座机：010 62345679", "phone").contains(&"010 62345679".to_string()));
        // 带分机的座机：不论被归为座机还是（因四位一组而）被卡号规则抢先，整串必须被某条命中完整覆盖
        let ext: Vec<String> = Detector::detect("座机：0755-82345670-8001", &rules).into_iter().map(|h| h.text).collect();
        assert!(ext.contains(&"0755-82345670-8001".to_string()), "{ext:?}");
        // 身份证：分组空格、小写 x
        assert_eq!(find("身份证：440301 19900715 1983", "id_card").len(), 1);
        assert_eq!(find("身份证：44030119900101123x", "id_card").len(), 0, "校验位错误的不应命中（该号为构造值）");
        // 对公账号（17 位）：需账号上下文
        assert_eq!(find("对公账号：11001234567890123", "bank_account"), vec!["11001234567890123"]);
        assert!(find("编号 11001234567890123", "bank_account").is_empty());
        // 护照/港澳通行证：需上下文
        assert_eq!(find("护照号：E12345610", "passport"), vec!["E12345610"]);
        assert_eq!(find("港澳通行证：C23456711", "passport"), vec!["C23456711"]);
        assert!(find("型号 E12345610", "passport").is_empty());
    }

    #[test]
    fn hk_tw_ids_and_traditional_prefixes() {
        let rules = builtin_rules();
        let hits = Detector::detect("香港身份證：A123456(7)；台灣身份證字號：A123456789；聯絡人：張明輝，負責人：李國強", &rules);
        let by = |id: &str| -> Vec<&str> { hits.iter().filter(|h| h.rule_id == id).map(|h| h.text.as_str()).collect() };
        assert_eq!(by("id_card_hmt"), vec!["A123456(7)", "A123456789"], "{hits:?}");
        let names = by("name");
        assert!(names.contains(&"張明輝") && names.contains(&"李國強"), "{names:?}");
        // 无“身份证”上下文的普通编号不应命中台湾身份证
        assert!(Detector::detect("型号 A123456789", &rules).iter().all(|h| h.rule_id != "id_card_hmt"));
    }
}
