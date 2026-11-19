//! 姓名启发式检测：姓氏词典 + 名字用字频，无需前后缀即可召回人名。
//!
//! 背景：大量人名并不紧跟在「姓名/签字人/联系人」等前缀后——表格单元格内的
//! 「王青」「刘伟」「张三丰」「赵敏」、变前缀写法「甲方指定王秀英…为项目对接人」
//! 等，纯前缀正则都无法召回。本模块用「高频姓氏 + 高频名字用字」在 CJK 片段内
//! 枚举候选，配合边界/称谓/停用词精调，作为正则与 NER 之外的补充通道。
//!
//! 设计权衡：NER（本地 BERT）已覆盖绝大多数常用人名，本算法定位为「低频姓名兜底」，
//! 因此偏向高精度（宁缺毋滥）：仅当候选同时满足
//!   1. 首字为高频姓氏；
//!   2. 名字用字全部落在高频名字用字集；
//!   3. 未被停用词命中（内嵌场景）；
//!   4. 存在边界信号（独立成串 / 后续不是名字用字 / 后跟称谓）。
//! 才输出，并按证据强度给出置信度：
//!   - 独立成串（整段 CJK 恰好是候选，且两侧非汉字）→ 0.9（表格单元格等）；
//!   - 姓氏 + 称谓词（先生/女士/经理…）→ 0.8；
//!   - 正文内嵌（后续不是名字用字）→ 0.7。

use crate::{Hit, HitSource};

/// 高频姓氏（覆盖汉族人口绝大多数）。刻意排除「文/成/方/关/公/司/安…」等
/// 亦为高频词首、易误报的姓氏——低频姓氏交给 NER 兜底。
const SURNAMES: &[char] = &[
    '王', '李', '张', '刘', '陈', '杨', '黄', '赵', '吴', '周', '徐', '孙', '马', '朱', '胡',
    '郭', '何', '高', '林', '罗', '郑', '梁', '谢', '宋', '唐', '许', '韩', '冯', '邓', '曹',
    '彭', '曾', '肖', '田', '董', '袁', '潘', '于', '蒋', '蔡', '余', '杜', '叶', '程', '苏',
    '魏', '吕', '丁', '任', '沈', '姚', '卢', '姜', '崔', '钟', '谭', '陆', '汪', '范', '金',
    '石', '廖', '贾', '夏', '韦', '付', '白', '邹', '孟', '熊', '秦', '邱', '江', '尹', '薛',
    '闫', '段', '雷', '侯', '龙', '史', '陶', '黎', '贺', '顾', '毛', '郝', '龚', '邵', '万',
    '钱', '严', '覃', '武', '戴', '莫', '孔', '向', '汤',
];

/// 高频名字用字（常见名用字频表）。刻意排除「法/式/面/处/时/到/说/上/下/重/事/名/为」
/// 等高频词用字，避免「方法/方面/及时」等词被误判为名字；姓氏字（如 金/龙/江/华/林）
/// 同时可作名用字时保留在集合内。
const GIVEN: &[char] = &[
    '伟', '芳', '娜', '敏', '静', '丽', '强', '磊', '军', '洋', '勇', '艳', '杰', '娟', '涛',
    '明', '超', '秀', '霞', '平', '刚', '桂', '英', '华', '鹏', '斌', '玲', '志', '鑫', '海',
    '波', '峰', '雪', '燕', '飞', '建', '彬', '晨', '阳', '俊', '凯', '龙', '宇', '浩', '博',
    '欣', '妍', '思', '涵', '泽', '睿', '轩', '子', '怡', '佳', '琳', '彤', '琪', '楠', '蕾',
    '然', '谦', '亦', '辰', '铭', '昊', '天', '瑞', '远', '立', '学', '国', '家', '忠', '孝',
    '仁', '义', '礼', '智', '信', '德', '才', '贤', '良', '善', '美', '爱', '惠', '梅', '兰',
    '竹', '菊', '珍', '珠', '玉', '香', '花', '红', '紫', '翠', '丹', '琴', '书', '画', '诗',
    '词', '歌', '赋', '松', '柏', '森', '淼', '焱', '垚', '众', '晶', '品', '河', '湖', '山',
    '岩', '云', '雨', '风', '霜', '露', '曦', '晖', '辉', '煌', '荣', '耀', '富', '贵', '升',
    '达', '财', '源', '广', '进', '运', '程', '鹏', '万', '里', '青', '春', '永', '驻', '正',
    '茂', '多', '双', '全', '帆', '顺', '事', '业', '成', '和', '兴', '健', '康', '长', '福',
    '快', '乐', '吉', '祥', '如', '意', '昭', '皓', '朗', '洲', '航', '楷', '哲', '沐', '宸',
    '墨', '诺', '汐', '萱', '梦', '颖', '淑', '慧', '琼', '凤', '兰', '莲', '彩', '秋', '实',
    '冬', '晓', '东', '芬', '芸', '月', '星', '晴', '璐', '璇', '玥', '珂', '珊', '薇', '蕊',
    '菲', '萌', '瑶', '雯', '洁', '莉', '亮', '翔', '民', '生', '宏', '威', '奎', '江', '尧',
    '舜', '禹', '津', '瀚', '泓', '溶', '溪', '澜', '婷', '婧', '媛', '姝', '嫣', '琦', '婉',
    '靖', '馨', '曼', '恬', '恋', '悠', '悦', '聪', '伶', '俐', '捷', '泰', '安', '宁', '畅',
    '观', '朗', '活', '泼', '可', '爱', '温', '柔', '贤', '端', '庄', '潇', '洒', '豪', '迈',
    '坚', '毅', '果', '断', '沉', '稳', '儒', '雅', '闻', '识', '广', '卓', '群', '润', '霖',
    '沛', '泉', '林', '金', '石', '田', '白', '雷', '汪', '王', '高', '黎', '陶', '夏', '文',
    '三', '丰',
];

/// 常见双字词（以姓氏开头、且第二字落在名字用字集），内嵌场景需排除，
/// 避免「高兴/黄金/森林/万一」等词被误判为姓名。独立成串（表格单元格）仍会被
/// 当作姓名召回（如真有人叫「高峰/金星」），此处只管内嵌场景。
const STOPWORDS: &[&str] = &[
    "黄金", "黄河", "黄山", "黄海", "高兴", "高明", "高峰", "高飞", "高山", "高原", "高远",
    "高升", "马达", "胡子", "森林", "树林", "竹林", "林海", "金星", "金山", "石英", "白云",
    "白天", "白金", "江山", "江湖", "江海", "雷雨", "万一", "万元", "钱财", "武汉", "方正",
    "方才", "方程", "方可", "杨梅", "叶子", "黎明", "贺词", "顾全", "孔子", "王国", "王子",
    "李子", "刘海", "陈词", "宋词", "唐诗", "许可", "许多", "生肖", "陆军", "夏天", "丁香",
    "汪洋",
];

/// 称谓后缀：姓氏 + 称谓即视为人名引用（如「王经理」「张先生」「李主任」）。
const TITLES: &[&str] = &[
    "先生", "女士", "小姐", "同志", "老师", "经理", "主任", "医生", "律师", "教授", "局长",
    "处长", "科长", "厂长", "校长", "部长", "董事长", "总经理", "总监", "工程师", "专员",
    "师傅", "阿姨", "叔叔",
];

/// 复姓表（首两字）。
const COMPOUND_SURNAMES: &[&str] = &[
    "欧阳", "司马", "上官", "诸葛", "东方", "皇甫", "尉迟", "公孙", "慕容", "司徒", "夏侯",
    "轩辕", "令狐", "宇文", "长孙", "澹台", "南宫", "独孤", "端木", "百里", "呼延", "赫连",
];

/// 文本是否以常见姓氏（单姓高频表或复姓表）开头，供带角色前缀的姓名规则校验。
pub fn starts_with_surname(s: &str) -> bool {
    let mut it = s.chars();
    let Some(first) = it.next() else { return false };
    if SURNAMES.contains(&first) {
        return true;
    }
    COMPOUND_SURNAMES.iter().any(|c| s.starts_with(c))
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fa5}').contains(&c)
}

/// 称谓匹配：返回 `after` 开头能匹配到的称谓词（若有）。
fn title_match(after: &[(usize, char)]) -> Option<&'static str> {
    let first = after.first()?.1;
    TITLES.iter().find(|t| t.starts_with(first)).and_then(|&t| {
        // 字符数而非字节数：称谓为 CJK，「律师」len()==6 但字符数为 2
        let n = t.chars().count();
        let s: String = after.iter().take(n).map(|(_, ch)| *ch).collect();
        (s == t).then_some(t)
    })
}

/// 全文姓名启发检测，返回字节偏移命中（与 Detector::detect 的字节偏移约定一致）。
pub fn detect_names(text: &str) -> Vec<Hit> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let mut hits: Vec<Hit> = Vec::new();
    let mut i = 0usize;
    while i < n {
        if !is_cjk(chars[i].1) {
            i += 1;
            continue;
        }
        // 找到一段连续 CJK 片段 [i, j)
        let mut j = i;
        while j < n && is_cjk(chars[j].1) {
            j += 1;
        }
        // 「独立成串」：片段两侧都不是汉字（行首/行尾/标点/空格等）
        let isolated = (i == 0 || !is_cjk(chars[i - 1].1)) && (j == n || !is_cjk(chars[j].1));
        let mut occupied = vec![false; j - i];
        for k in i..j {
            if occupied[k - i] {
                continue;
            }
            let c = chars[k].1;
            if !SURNAMES.contains(&c) {
                continue;
            }
            // “任何/如何/为何/无何…”里的「何」是疑问/泛指词而不是姓氏（“任何敏感信息”曾被误判为姓名“何敏”）
            if c == '何' && k > i && ['任', '如', '为', '无', '若', '几', '奈'].contains(&chars[k - 1].1) {
                continue;
            }
            let after = &chars[k + 1..j];
            // 1) 称谓规则：姓氏 + 称谓（王经理 / 张先生 / 李主任）
            if let Some(t) = title_match(after) {
                let end_k = k + 1 + t.chars().count();
                if end_k <= j {
                    let start = chars[k].0;
                    let end = chars[end_k - 1].0 + 3; // CJK 字符恒为 3 字节
                    let cand: String = chars[k..end_k].iter().map(|(_, ch)| *ch).collect();
                    hits.push(Hit {
                        rule_id: "name".into(),
                        rule_label: "姓名".into(),
                        start,
                        end,
                        text: cand,
                        confidence: 0.8,
                        source: HitSource::Name,
                    });
                    for o in k..end_k {
                        occupied[o - i] = true;
                    }
                    continue;
                }
            }
            // 2) 姓名字规则：姓氏 + 1~2 个名字用字（先长后短）
            for len in (2..=3).rev() {
                if k + len > j {
                    continue;
                }
                let given: Vec<char> = chars[k + 1..k + len].iter().map(|(_, ch)| *ch).collect();
                if given.iter().any(|g| !GIVEN.contains(g)) {
                    continue;
                }
                // 后一个仍是名字用字 → 用更长的候选（如「张伟强」优先整段而非「张伟」）
                if k + len < j && GIVEN.contains(&chars[k + len].1) {
                    continue;
                }
                let cand: String = chars[k..k + len].iter().map(|(_, ch)| *ch).collect();
                if STOPWORDS.contains(&cand.as_str()) {
                    continue;
                }
                let whole = k == i && k + len == j;
                let confidence = if whole && isolated { 0.9 } else { 0.7 };
                let start = chars[k].0;
                let end = chars[k + len - 1].0 + 3;
                hits.push(Hit {
                    rule_id: "name".into(),
                    rule_label: "姓名".into(),
                    start,
                    end,
                    text: cand,
                    confidence,
                    source: HitSource::Name,
                });
                for o in k..k + len {
                    occupied[o - i] = true;
                }
                break; // 只保留该姓氏处最长可接受候选
            }
        }
        i = j;
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.text.as_str()).collect()
    }

    #[test]
    fn detects_table_cell_names() {
        // 表格单元格内无前缀姓名（此前全部漏检）
        let hits = detect_names("王青\n刘伟\n张三丰\n赵敏");
        let got = texts(&hits);
        for expect in ["王青", "刘伟", "张三丰", "赵敏"] {
            assert!(got.contains(&expect), "漏检表内姓名: {expect} in {got:?}");
        }
        // 独立单元格置信度最高
        let h = hits.iter().find(|h| h.text == "张三丰").unwrap();
        assert_eq!(h.confidence, 0.9);
    }

    #[test]
    fn detects_variable_prefix_name() {
        // 变前缀：姓名既不跟在前缀后，也不在表格里
        let hits = detect_names("甲方指定王秀英（乙方）为项目对接人。");
        let got = texts(&hits);
        assert!(got.contains(&"王秀英"), "变前缀姓名漏检: {got:?}");
        let h = hits.iter().find(|h| h.text == "王秀英").unwrap();
        assert_eq!(h.confidence, 0.7);
    }

    #[test]
    fn detects_prose_name_with_title() {
        // 称谓后缀
        let hits = detect_names("请联系张律师，王经理稍后到，李主任在办公室。");
        let got = texts(&hits);
        assert!(got.contains(&"张律师"), "称谓姓名漏检: {got:?}");
        assert!(got.contains(&"王经理"), "称谓姓名漏检: {got:?}");
        assert!(got.contains(&"李主任"), "称谓姓名漏检: {got:?}");
    }

    #[test]
    fn no_header_cross_cell_false_positive() {
        // 「姓名\n身份证号」相邻表头不得误报
        let hits = detect_names("姓名\n身份证号");
        assert!(hits.is_empty(), "表头误报为姓名: {hits:?}");
    }

    #[test]
    fn avoids_common_words() {
        // 高频词不得误判为姓名
        for text in [
            "使用此方法处理",       // 方法
            "达到高峰后回落",       // 高峰
            "合同金额五十万元",     // 万元
            "森林覆盖率达九成",     // 森林
            "很高兴见到您",         // 高兴
            "根据相关规定执行",     // 相关规定
            "本协议由乙方盖章后生效", // 乙方/盖章/生效
        ] {
            let hits = detect_names(text);
            assert!(hits.is_empty(), "误报姓名: {text} -> {hits:?}");
        }
    }

    #[test]
    fn byte_offsets_are_valid() {
        // 命中区间必须是合法字节边界，且 text 与原文一致
        let text = "表格：王秀英、刘伟。";
        for h in detect_names(text) {
            assert!(h.start < h.end && h.end <= text.len());
            assert_eq!(&text[h.start..h.end], h.text);
        }
    }

    #[test]
    fn prefers_longer_name() {
        // 「张伟强」应整段命中，而非拆成「张伟」
        let hits = detect_names("张伟强");
        let got = texts(&hits);
        assert!(got.contains(&"张伟强"), "长名未整段命中: {got:?}");
        assert!(!got.contains(&"张伟"), "长名被错误拆分: {got:?}");
    }

    #[test]
    fn no_false_name_in_common_phrases() {
        for t in ["没有任何敏感信息", "如何处理", "为何失败", "无何不可"] {
            assert!(detect_names(t).is_empty(), "{t} 不应识别出姓名: {:?}", detect_names(t));
        }
        // 真姓氏“何”不受影响
        assert!(!detect_names("联系人何敏").is_empty() || !detect_names("何敏").is_empty());
    }
}
