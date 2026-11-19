//! 脱敏变换：掩码 / 占位符 / 假数据 / BASE64 / AES 加密。

use crate::Hit;
use crate::crypto::{aes_encrypt, base64_encode};
use crate::mapping::MappingTable;
use crate::rules::{ID_BANK_CARD, ID_ID_CARD, ID_MOBILE, MOBILE_SEPARATORS, id_card_check_code};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Mask,
    Placeholder,
    FakeData,
    Base64,
    Aes,
    /// 不处理：保留原文（用于诉讼场景的金额/日期等不应被改动的类型）
    Keep,
    /// 保结构缩放（金额）：按案件系数 k 等比缩放，格式不变；可逆（走对照表）。属假名化，非匿名化
    Scale,
    /// 保结构平移（日期）：整体平移 Δd 天，格式/星期不变；可逆（走对照表）。属假名化，非匿名化
    Shift,
}

/// 每个规则的脱敏策略
#[derive(Debug, Clone)]
pub struct Strategy {
    pub output: OutputKind,
    pub mask_char: char,
    pub keep_head: usize,
    pub keep_tail: usize,
}

impl Default for Strategy {
    fn default() -> Self {
        Self {
            output: OutputKind::Mask,
            mask_char: '*',
            keep_head: 0,
            keep_tail: 0,
        }
    }
}

/// 对命中集合执行脱敏，返回替换后的全文。
pub struct Transformer<'a> {
    mapping: &'a mut MappingTable,
}

impl<'a> Transformer<'a> {
    pub fn new(mapping: &'a mut MappingTable) -> Self {
        Self { mapping }
    }

    /// `key` 为 AES 策略所需（Some 时才允许 Aes）。
    ///
    /// 返回 `Result`：任一命中配置了 AES 策略但未提供密钥时报错上抛，
    /// **不再静默降级为掩码**（用户以为输出是密文、实际是掩码属于安全语义降级）。
    pub fn apply(
        &mut self,
        text: &str,
        hits: &[Hit],
        strategies: &HashMap<String, Strategy>,
        key: Option<&[u8; 32]>,
    ) -> Result<String, String> {
        // 按起始位置排序并过滤重叠（保留更早/更长的命中）
        let mut sorted: Vec<&Hit> = hits.iter().collect();
        sorted.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
        let mut kept: Vec<&Hit> = Vec::new();
        let mut prev_end = 0usize;
        for h in sorted {
            if h.start >= prev_end {
                kept.push(h);
                prev_end = h.end;
            }
        }
        // 从后往前替换，避免偏移漂移
        let mut out = text.to_string();
        for h in kept.iter().rev() {
            let strategy = strategies.get(&h.rule_id).cloned().unwrap_or_default();
            let ctx = crate::shift::ctx_around(text, h.start, h.end);
            let replacement = self.render(h, &strategy, key, ctx)?;
            out.replace_range(h.start..h.end, &replacement);
        }
        Ok(out)
    }

    fn render(
        &mut self,
        hit: &Hit,
        strategy: &Strategy,
        key: Option<&[u8; 32]>,
        ctx: (&str, &str),
    ) -> Result<String, String> {
        match strategy.output {
            OutputKind::Scale | OutputKind::Shift => Ok(render_shift(
                self.mapping,
                strategy.output,
                &hit.text,
                ctx.0,
                ctx.1,
            )),
            OutputKind::Mask => {
                if hit.rule_id == ID_MOBILE
                    && hit.text.chars().any(|c| MOBILE_SEPARATORS.contains(&c))
                {
                    Ok(masked_mobile_preserve_separators(&hit.text, strategy))
                } else {
                    Ok(masked(&hit.text, strategy))
                }
            }
            OutputKind::Placeholder => {
                Ok(self.mapping.placeholder_for(&hit.rule_label, &hit.text))
            }
            OutputKind::FakeData => {
                let key_slice = key.map(|k| k as &[u8]);
                if hit.rule_id == ID_MOBILE
                    && hit.text.chars().any(|c| MOBILE_SEPARATORS.contains(&c))
                {
                    Ok(fake_mobile_preserve_separators(&hit.text, strategy, key_slice))
                } else {
                    Ok(fake_for(&hit.rule_id, &hit.text, strategy, key_slice))
                }
            }
            OutputKind::Keep => Ok(hit.text.clone()),
            OutputKind::Base64 => Ok(base64_encode(&hit.text)),
            OutputKind::Aes => {
                let k = key.ok_or_else(|| {
                    format!(
                        "规则 [{}] 配置了 AES 策略但未提供口令，已拒绝生成密文（不降级为掩码）",
                        hit.rule_id
                    )
                })?;
                aes_encrypt(&hit.text, k)
                    .map_err(|e| format!("规则 [{}] AES 加密失败: {e}", hit.rule_id))
            }
        }
    }
}

/// 保结构渲染（缩放/平移）：命令层、pipeline 层与 `Transformer` 共用。
/// 不应处理的情形（百分比、费率、条款号、法规日期、无法解析）返回原文。
pub fn render_shift(
    mapping: &mut MappingTable,
    output: OutputKind,
    text: &str,
    before: &str,
    after: &str,
) -> String {
    let book = mapping.shift_book_mut();
    let r = match output {
        OutputKind::Scale => book.scale_amount(text, before, after),
        _ => book.shift_date(text, before, after),
    };
    r.unwrap_or_else(|| text.to_string())
}

/// 掩码：保留首/尾 N 字符，中间以替换符填充（头尾不重叠）
pub fn masked(original: &str, strategy: &Strategy) -> String {
    let chars: Vec<char> = original.chars().collect();
    let n = chars.len();
    if n == 0 {
        return String::new();
    }
    let head = strategy.keep_head.min(n);
    let tail = strategy.keep_tail.min(n - head); // 剩余空间内保留
    let mask_count = n - head - tail;
    let mut out = String::with_capacity(n);
    out.extend(&chars[..head]);
    for _ in 0..mask_count {
        out.push(strategy.mask_char);
    }
    out.extend(&chars[n - tail..]);
    out
}

/// 手机号掩码：保留分隔符位置不变，只对数字位做掩码。
/// keep_head / keep_tail 按数字位数计算（与纯数字手机号的策略一致）。
pub fn masked_mobile_preserve_separators(original: &str, strategy: &Strategy) -> String {
    let digits: Vec<(usize, char)> = original
        .chars()
        .enumerate()
        .filter(|(_, c)| c.is_ascii_digit())
        .collect();
    let n = digits.len();
    if n == 0 {
        return original.to_string();
    }
    let head = strategy.keep_head.min(n);
    let tail = strategy.keep_tail.min(n - head);
    // 标记哪些数字位需要掩码
    let mut mask_pos = std::collections::HashSet::new();
    for (idx, _) in digits.iter().take(n - tail).skip(head) {
        mask_pos.insert(*idx);
    }
    original
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if mask_pos.contains(&i) {
                strategy.mask_char
            } else {
                c
            }
        })
        .collect()
}

/// 确定性假数据（同原文同假数据）。
///
/// 格式指纹控制：
/// - 假卡号长度与原卡号一致、首位 BIN 分布到 4/5/6，避免「恒 19 位、恒 622202」
///   这种一眼可识别的伪造；
/// - 假身份证日期覆盖 1-31 日（此前恒 1-27 日，28-31 日出生者在假数据中零出现，
///   是极强的统计区分特征）；
/// - 手机前缀池覆盖 38 个常见号段。
pub fn fake_for(
    rule_id: &str,
    original: &str,
    strategy: &Strategy,
    key: Option<&[u8]>,
) -> String {
    // 有会话密钥时用 HMAC-SHA256 派生种子（同原文同假数据，但离线无法反查）；
    // 无密钥时退回 FNV-1a（保持确定性，向后兼容无口令场景——已知限制：可离线穷举）。
    let seed = match key {
        Some(k) => hmac_seed(k, original),
        None => stable_hash(original),
    };
    match rule_id {
        ID_MOBILE => {
            const PREFIXES: [&str; 38] = [
                "130", "131", "132", "133", "134", "135", "136", "137", "138", "139", "150",
                "151", "152", "153", "155", "156", "157", "158", "159", "166", "171", "173",
                "176", "177", "178", "180", "181", "182", "183", "184", "185", "186", "187",
                "188", "189", "191", "198", "199",
            ];
            let prefix = PREFIXES[seed % PREFIXES.len()];
            format!("{prefix}{:08}", seed % 100_000_000)
        }
        ID_ID_CARD => make_fake_id_card(seed),
        ID_BANK_CARD => {
            let n = original.chars().filter(|c| c.is_ascii_digit()).count();
            make_fake_bank_card(seed, n)
        }
        _ => masked(original, strategy),
    }
}

/// 带分隔符的手机号假数据：保持原文分隔符位置，只替换数字位。
pub fn fake_mobile_preserve_separators(
    original: &str,
    _strategy: &Strategy,
    key: Option<&[u8]>,
) -> String {
    // 用纯数字原文派生假号码（保证与无分隔符版本一致、可还原）
    let digits_only: String = original.chars().filter(|c| c.is_ascii_digit()).collect();
    let fake_digits = fake_for(ID_MOBILE, &digits_only, _strategy, key);
    let mut fake_chars = fake_digits.chars();
    original
        .chars()
        .map(|c| {
            if c.is_ascii_digit() {
                fake_chars.next().unwrap_or('*')
            } else {
                c
            }
        })
        .collect()
}

/// HMAC-SHA256(session_key, original) 派生确定性种子（有密钥时假数据不可离线反查）。
fn hmac_seed(key: &[u8], s: &str) -> usize {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(s.as_bytes());
    let out = mac.finalize().into_bytes();
    u64::from_be_bytes(out[..8].try_into().expect("SHA-256 输出至少 8 字节")) as usize
}

fn stable_hash(s: &str) -> usize {
    let mut h: usize = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as usize;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 某年月对应的天数（含闰年 2 月 29 日）。
fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn make_fake_id_card(seed: usize) -> String {
    // 区域码覆盖多省市（此前仅 3 个），年份扩展到 1950-2009
    let region = ["110101", "310101", "440101", "510107", "330106", "320105"][seed % 6];
    let year = 1950 + (seed % 60);
    let month = 1 + (seed % 12);
    let day = 1 + (seed % days_in_month(year as u32, month as u32) as usize); // 覆盖 28-31 日
    let seq = seed % 1000;
    let s17 = format!("{region}{year:04}{month:02}{day:02}{seq:03}");
    let check = id_card_check_code(&s17);
    format!("{s17}{check}")
}

fn make_fake_bank_card(seed: usize, original_len: usize) -> String {
    // 长度与原卡号一致（14-19 位），首位 BIN 分布到 4/5/6（银联/ Visa / Mastercard 常见）；
    // 不再固定「622202 + 19 位」的可识别伪造形态。
    let len = original_len.clamp(14, 19);
    let mut digits = String::with_capacity(len);
    digits.push(char::from_digit((4 + seed % 3) as u32, 10).unwrap());
    let mut s = seed / 3;
    for _ in 1..len - 1 {
        digits.push(char::from_digit((s % 10) as u32, 10).unwrap());
        s /= 10;
    }
    // Luhn 补校验位
    let check = luhn_check_digit(&digits);
    digits.push(char::from_digit(check, 10).unwrap());
    digits
}

/// 计算 Luhn 校验位（digits 不含校验位）
fn luhn_check_digit(digits: &str) -> u32 {
    let mut sum = 0u32;
    for (i, c) in digits.chars().rev().enumerate() {
        let mut v = c.to_digit(10).unwrap_or(0);
        if i % 2 == 0 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    (10 - (sum % 10)) % 10
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::Detector;
    use crate::rules::{builtin_rules, luhn_valid};

    #[test]
    fn mask_default() {
        let s = Strategy::default();
        assert_eq!(masked("13800138000", &s), "***********");
        assert_eq!(masked("张三", &s), "**");
    }

    #[test]
    fn mask_keep_head_tail() {
        let s = Strategy {
            mask_char: '*',
            keep_head: 3,
            keep_tail: 4,
            ..Default::default()
        };
        assert_eq!(masked("13800138000", &s), "138****8000");
    }

    #[test]
    fn mask_mobile_with_separators_preserves_format() {
        let s = Strategy {
            mask_char: '*',
            keep_head: 3,
            keep_tail: 4,
            ..Default::default()
        };
        // 分隔符位置保持不变，只掩码数字位
        assert_eq!(
            masked_mobile_preserve_separators("138 0013 8000", &s),
            "138 **** 8000"
        );
        assert_eq!(
            masked_mobile_preserve_separators("138-0013-8000", &s),
            "138-****-8000"
        );
        assert_eq!(
            masked_mobile_preserve_separators("138.0013.8000", &s),
            "138.****.8000"
        );
        // 全掩码
        let s_all = Strategy::default();
        assert_eq!(
            masked_mobile_preserve_separators("138-0013-8000", &s_all),
            "***-****-****"
        );
    }

    #[test]
    fn fake_mobile_with_separators_preserves_format() {
        let s = Strategy::default();
        let original = "138-0013-8000";
        let faked = fake_mobile_preserve_separators(original, &s, None);
        // 分隔符位置不变
        assert_eq!(faked.chars().nth(3), Some('-'));
        assert_eq!(faked.chars().nth(8), Some('-'));
        // 数字位都是数字
        for (i, c) in faked.chars().enumerate() {
            if i != 3 && i != 8 {
                assert!(c.is_ascii_digit(), "位置 {} 应为数字: {}", i, faked);
            }
        }
        // 纯数字版本与带分隔符版本的假号码数字应一致
        let digits_only: String = original.chars().filter(|c| c.is_ascii_digit()).collect();
        let faked_plain = fake_for(ID_MOBILE, &digits_only, &s, None);
        let faked_digits: String = faked.chars().filter(|c| c.is_ascii_digit()).collect();
        assert_eq!(faked_digits, faked_plain);
    }

    #[test]
    fn mask_custom_char() {
        let s = Strategy {
            mask_char: '■',
            ..Default::default()
        };
        assert_eq!(masked("张三", &s), "■■");
    }

    #[test]
    fn mask_keep_more_than_len() {
        let s = Strategy {
            keep_head: 5,
            keep_tail: 5,
            ..Default::default()
        };
        // keep 超过长度时不重复、不越界，整体保留
        assert_eq!(masked("中华人民共和国", &s), "中华人民共和国");
        // 头尾部分保留：3 头 2 尾
        let s = Strategy {
            keep_head: 3,
            keep_tail: 2,
            ..Default::default()
        };
        assert_eq!(masked("中华人民共和国", &s), "中华人**和国");
    }

    #[test]
    fn placeholder_transform_and_restore() {
        let mut mapping = MappingTable::new();
        let mut strategies = HashMap::new();
        strategies.insert(
            "mobile".into(),
            Strategy {
                output: OutputKind::Placeholder,
                ..Default::default()
            },
        );
        let mut t = Transformer::new(&mut mapping);
        let text = "电话 13800138000 与 13800138000";
        let hits = Detector::detect(text, &builtin_rules());
        let out = t.apply(text, &hits, &strategies, None).unwrap();
        assert_eq!(out, "电话 [手机号1] 与 [手机号1]");
        assert_eq!(mapping.restore(&out), text);
    }

    #[test]
    fn mobile_with_separators_mask_and_restore() {
        // 端到端：带分隔符手机号 → 检测 → 掩码（保留格式）→ 占位符还原
        let mut mapping = MappingTable::new();
        let mut strategies = HashMap::new();
        strategies.insert(
            "mobile".into(),
            Strategy {
                output: OutputKind::Mask,
                mask_char: '*',
                keep_head: 3,
                keep_tail: 4,
                ..Default::default()
            },
        );
        let mut t = Transformer::new(&mut mapping);
        let text = "联系电话：138-0013-8000，备用：139 0013 9000";
        let hits = Detector::detect(text, &builtin_rules());
        let mobile_hits: Vec<_> = hits.iter().filter(|h| h.rule_id == "mobile").collect();
        assert_eq!(mobile_hits.len(), 2);
        let out = t.apply(text, &hits, &strategies, None).unwrap();
        // 分隔符保留
        assert!(out.contains("138-****-8000"), "掩码结果: {}", out);
        assert!(out.contains("139 **** 9000"), "掩码结果: {}", out);

        // 占位符策略下可还原
        let mut mapping2 = MappingTable::new();
        let mut strategies2 = HashMap::new();
        strategies2.insert(
            "mobile".into(),
            Strategy {
                output: OutputKind::Placeholder,
                ..Default::default()
            },
        );
        let mut t2 = Transformer::new(&mut mapping2);
        let out2 = t2.apply(text, &hits, &strategies2, None).unwrap();
        assert_eq!(mapping2.restore(&out2), text);
    }

    #[test]
    fn base64_strategy() {
        let mut mapping = MappingTable::new();
        let mut strategies = HashMap::new();
        strategies.insert(
            "mobile".into(),
            Strategy {
                output: OutputKind::Base64,
                ..Default::default()
            },
        );
        let mut t = Transformer::new(&mut mapping);
        let text = "13800138000";
        let hits = Detector::detect(text, &builtin_rules());
        let out = t.apply(text, &hits, &strategies, None).unwrap();
        assert_eq!(crate::crypto::base64_decode(&out).unwrap(), text);
    }

    #[test]
    fn fake_data_same_format() {
        let mut mapping = MappingTable::new();
        let mut strategies = HashMap::new();
        strategies.insert(
            "mobile".into(),
            Strategy {
                output: OutputKind::FakeData,
                ..Default::default()
            },
        );
        strategies.insert(
            "id_card".into(),
            Strategy {
                output: OutputKind::FakeData,
                ..Default::default()
            },
        );
        strategies.insert(
            "bank_card".into(),
            Strategy {
                output: OutputKind::FakeData,
                ..Default::default()
            },
        );
        let mut t = Transformer::new(&mut mapping);
        // 构造合法身份证
        let check = id_card_check_code("11010119840320123");
        let id = format!("11010119840320123{check}");
        let text = format!("手机13800138000 身份证{id} 卡4111111111111111");
        let hits = Detector::detect(&text, &builtin_rules());
        let out = t.apply(&text, &hits, &strategies, None).unwrap();
        assert!(out.contains("手机"));
        assert!(out.contains("身份证"));
        assert!(out.contains("卡"));
        // 假数据保持同格式：11 位数字手机号、18 位身份证、Luhn 合法卡号
        for line in out.split_whitespace() {
            if line.starts_with("手机") {
                let num: String = line.chars().filter(|c| c.is_ascii_digit()).collect();
                assert_eq!(num.len(), 11);
            }
            if line.starts_with("身份证") {
                let num: String = line
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == 'X')
                    .collect();
                assert_eq!(num.len(), 18);
                assert!(crate::rules::is_valid_id_card(&num));
            }
            if line.starts_with("卡") {
                let num: String = line.chars().filter(|c| c.is_ascii_digit()).collect();
                assert!(luhn_valid(&num));
            }
        }
    }

    #[test]
    fn fake_data_deterministic() {
        let mut m1 = MappingTable::new();
        let mut m2 = MappingTable::new();
        let mut s1 = HashMap::new();
        let mut s2 = HashMap::new();
        for m in [&mut s1, &mut s2] {
            m.insert(
                "mobile".into(),
                Strategy {
                    output: OutputKind::FakeData,
                    ..Default::default()
                },
            );
        }
        let text = "13800138000";
        let hits = Detector::detect(text, &builtin_rules());
        let out1 = Transformer::new(&mut m1).apply(text, &hits, &s1, None).unwrap();
        let out2 = Transformer::new(&mut m2).apply(text, &hits, &s2, None).unwrap();
        assert_eq!(out1, out2);
    }

    #[test]
    fn fake_data_no_format_fingerprint() {
        let s = Strategy {
            output: OutputKind::FakeData,
            ..Default::default()
        };
        // 假卡号长度与原卡号一致（16 位原卡 → 16 位假卡），BIN 不再固定 622202
        let fake16 = fake_for(ID_BANK_CARD, "4111111111111111", &s, None);
        let digits16: String = fake16.chars().filter(|c| c.is_ascii_digit()).collect();
        assert_eq!(digits16.len(), 16, "16 位原卡应产出 16 位假卡: {fake16}");
        assert!(!fake16.starts_with("622202"), "不应固定 622202 BIN: {fake16}");
        assert!(luhn_valid(&digits16), "假卡号应 Luhn 合法");
        let fake19 = fake_for(ID_BANK_CARD, "6222020200088888884", &s, None);
        let digits19: String = fake19.chars().filter(|c| c.is_ascii_digit()).collect();
        assert_eq!(digits19.len(), 19, "19 位原卡应产出 19 位假卡: {fake19}");

        // 假身份证日期覆盖 1-31 日：多次采样应出现 >27 的日期（此前恒 1-27）
        let mut has_28_plus = false;
        for i in 0..60 {
            let id = fake_for(ID_ID_CARD, &format!("测试-{i}"), &s, None);
            let day: String = id.chars().skip(12).take(2).collect();
            if day.parse::<u32>().map(|d| d > 27).unwrap_or(false) {
                has_28_plus = true;
            }
            assert!(crate::rules::is_valid_id_card(&id), "假身份证应合法: {id}");
        }
        assert!(has_28_plus, "采样中应出现 28-31 日（消除日期统计指纹）");
    }

    #[test]
    fn aes_requires_key_no_silent_degrade() {
        use crate::crypto::derive_key;

        let mut mapping = MappingTable::new();
        let mut strategies = HashMap::new();
        strategies.insert(
            "mobile".into(),
            Strategy {
                output: OutputKind::Aes,
                ..Default::default()
            },
        );
        let text = "13800138000";
        let hits = Detector::detect(text, &builtin_rules());

        // 无 key：显性报错，不得静默降级为掩码
        let err = Transformer::new(&mut mapping)
            .apply(text, &hits, &strategies, None)
            .unwrap_err();
        assert!(err.contains("AES"), "错误信息应指明 AES 与缺口令: {err}");
        assert!(!err.contains("静默"));

        // 有 key：输出真密文（AES:{iv}:{ct}），不再是掩码
        let key = derive_key("口令", b"test-salt", 1_000);
        let out = Transformer::new(&mut mapping)
            .apply(text, &hits, &strategies, Some(&key))
            .unwrap();
        assert!(out.starts_with("AES:"), "AES 策略应输出密文: {out}");
        assert!(!out.contains("13800138000"), "原文不得残留");
        assert!(!out.contains('*'), "不应降级为掩码: {out}");
    }

    #[test]
    fn fake_data_keyed_seed() {
        let s = Strategy {
            output: OutputKind::FakeData,
            ..Default::default()
        };
        let original = "13800138000";
        // 同 key 同原文 → 同假数据（确定性）
        let a1 = fake_for(ID_MOBILE, original, &s, Some(b"session-key"));
        let a2 = fake_for(ID_MOBILE, original, &s, Some(b"session-key"));
        assert_eq!(a1, a2);
        // 不同 key → 不同假数据
        let b = fake_for(ID_MOBILE, original, &s, Some(b"other-key"));
        assert_ne!(a1, b);
        // 有 key 与无 key（FNV 兜底）也应不同
        let c = fake_for(ID_MOBILE, original, &s, None);
        assert_ne!(a1, c);
    }

    #[test]
    fn keep_output_returns_original() {
        let hit = Hit {
            rule_id: "amount".into(),
            rule_label: "金额".into(),
            start: 0,
            end: 3,
            text: "¥12万".into(),
            confidence: 0.9,
            source: crate::HitSource::Rule,
        };
        let mut mapping = crate::mapping::MappingTable::new();
        let mut t = Transformer::new(&mut mapping);
        let s = Strategy { output: OutputKind::Keep, ..Strategy::default() };
        assert_eq!(t.render(&hit, &s, None, ("", "")).unwrap(), "¥12万");
    }

    #[test]
    fn scale_and_shift_through_transformer() {
        let mut mapping = MappingTable::new();
        mapping.shift_book_mut().params = Some(crate::shift::ShiftParams::fixed(1.5, 28));
        let mut strategies = HashMap::new();
        for (id, out) in [("amount", OutputKind::Scale), ("date", OutputKind::Shift)] {
            strategies.insert(id.to_string(), Strategy { output: out, ..Default::default() });
        }
        let text = "本合同自2024-01-01起生效，价款人民币壹佰万元整（¥1,000,000.00），违约金按日0.05%计；《民法典》自2021年1月1日起施行。";
        let hits = Detector::detect(text, &builtin_rules());
        let out = Transformer::new(&mut mapping).apply(text, &hits, &strategies, None).unwrap();
        assert!(out.contains("2024-01-29"), "{out}");
        assert!(out.contains("人民币壹佰伍拾万元整") && out.contains("¥1,500,000.00"), "{out}");
        assert!(out.contains("0.05%") && out.contains("2021年1月1日起施行"), "{out}");
        assert_eq!(mapping.shift_book().restore_exact(&out), text);
    }
}
