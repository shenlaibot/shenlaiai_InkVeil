//! 映射表：同原文 → 同占位符（跨文件共享实例），可持久化、可口令加密、可还原。

use crate::crypto::{aes_decrypt, aes_encrypt, derive_key};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 加密映射文件的版本化头部：`ANONMAP:v1:{salt_b64}:{ciphertext}`。
///
/// 每文件使用随机盐（防固定盐的预计算字典攻击），PBKDF2 迭代按 OWASP 建议
/// 提升到 600k 轮。v1 之前的旧格式（无头、固定盐 100k 轮）仍可导入。
const ANONMAP_MAGIC: &str = "ANONMAP:v1:";
const ANONMAP_SALT_LEN: usize = 16;
const ANONMAP_ROUNDS: u32 = 600_000;
/// 旧格式（v1 之前的 .anonymap.enc）使用的固定盐与迭代次数，仅用于向后兼容导入。
const ANONMAP_LEGACY_SALT: &[u8] = b"anonkit-anonymap";
const ANONMAP_LEGACY_ROUNDS: u32 = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Persisted {
    version: u32,
    entries: Vec<(String, String)>, // (original, placeholder)
    counters: HashMap<String, usize>,
    /// 保结构替换（金额缩放/日期平移）的参数与“原文→替换值”对照；旧文件没有此字段
    #[serde(default)]
    shift: crate::shift::ShiftBook,
}

#[derive(Clone)]
pub struct MappingTable {
    original_to_placeholder: HashMap<String, String>,
    placeholder_to_original: HashMap<String, String>,
    counters: HashMap<String, usize>,
    shift: crate::shift::ShiftBook,
}

impl Default for MappingTable {
    fn default() -> Self {
        Self::new()
    }
}

impl MappingTable {
    pub fn new() -> Self {
        Self {
            original_to_placeholder: HashMap::new(),
            placeholder_to_original: HashMap::new(),
            counters: HashMap::new(),
            shift: crate::shift::ShiftBook::default(),
        }
    }

    /// 保结构替换账本（只读）。
    pub fn shift_book(&self) -> &crate::shift::ShiftBook {
        &self.shift
    }

    /// 保结构替换账本（可写：首次使用时随机生成参数）。
    pub fn shift_book_mut(&mut self) -> &mut crate::shift::ShiftBook {
        &mut self.shift
    }

    /// 为原文分配占位符：同原文永远返回同一占位符。
    ///
    /// 占位符 `[label{n}]` 以 label 做命名空间；用户自定义 label 可能含数字/`]`
    /// （如「手机号1」的 `[手机号11]` 与「手机号」的第 11 个占位符撞串），
    /// 分配时若发现占位符已被其它原文占用则继续递增编号，杜绝跨 label 碰撞。
    pub fn placeholder_for(&mut self, label: &str, original: &str) -> String {
        if let Some(p) = self.original_to_placeholder.get(original) {
            return p.clone();
        }
        let counter = self.counters.entry(label.to_string()).or_insert(0);
        let placeholder = loop {
            *counter += 1;
            let cand = format!("[{label}{counter}]");
            if !self.placeholder_to_original.contains_key(&cand) {
                break cand;
            }
        };
        self.original_to_placeholder
            .insert(original.to_string(), placeholder.clone());
        self.placeholder_to_original
            .insert(placeholder.clone(), original.to_string());
        placeholder
    }

    /// 绑定别名（简称）到已存在的占位符：全称与简称替换为同一占位符。
    ///
    /// 若别名此前已被独立分配过其它占位符，清理其旧映射（孤儿占位符残留
    /// 会导致已输出文档的旧占位符仍被还原，与新绑定不一致）。
    pub fn bind_alias(&mut self, alias: &str, placeholder: &str) {
        if let Some(original) = self.placeholder_to_original.get(placeholder) {
            let canonical = original.clone();
            // 别名已有旧占位符且与新绑定不同 → 删除旧占位符（宁可不还原，也不还原错）
            if let Some(old) = self.original_to_placeholder.get(alias) {
                if old != placeholder {
                    self.placeholder_to_original.remove(old);
                }
            }
            self.original_to_placeholder
                .insert(alias.to_string(), placeholder.to_string());
            self.placeholder_to_original
                .insert(placeholder.to_string(), canonical);
        }
    }

    /// 已登记的全部原文（项目保险库用它做“已知实体回扫”）。
    pub fn originals(&self) -> Vec<String> {
        self.original_to_placeholder.keys().cloned().collect()
    }

    pub fn has(&self, original: &str) -> bool {
        self.original_to_placeholder.contains_key(original)
    }

    /// 占位符 → 原文 映射（还原页统计用）。
    pub fn placeholder_to_original(&self) -> &HashMap<String, String> {
        &self.placeholder_to_original
    }

    pub fn placeholder_of(&self, original: &str) -> Option<&str> {
        self.original_to_placeholder
            .get(original)
            .map(|s| s.as_str())
    }

    pub fn len(&self) -> usize {
        self.original_to_placeholder.len()
    }

    pub fn is_empty(&self) -> bool {
        self.original_to_placeholder.is_empty()
    }

    /// 将文本中的占位符替换回原文。
    ///
    /// 只要占位符（`[标签n]`）存在于映射表中就还原，**不再要求两侧是边界**：
    /// 中文文书里占位符两侧几乎总是汉字（“甲方[公司1]委托乙方”“由[姓名1]签字”），
    /// 旧的“至少一侧非汉字”规则会让这类最常见的场景全部还原失败。
    /// 按占位符长度降序匹配，避免 `[手机号1]` 与 `[手机号10]` 的子串误替换；
    /// 只在遇到 `[` 时才尝试匹配，其余字符直接拷贝。
    pub fn restore(&self, text: &str) -> String {
        let mut pairs: Vec<(&String, &String)> = self.placeholder_to_original.iter().collect();
        pairs.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        let mut out = String::with_capacity(text.len());
        let bytes = text.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'[' {
                let mut hit = false;
                for (placeholder, original) in &pairs {
                    let p = placeholder.as_bytes();
                    if bytes.len() - i >= p.len() && &bytes[i..i + p.len()] == p {
                        out.push_str(original);
                        i += p.len();
                        hit = true;
                        break;
                    }
                }
                if hit {
                    continue;
                }
            }
            let ch = text[i..].chars().next().expect("i 恒在字符边界");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// 明文序列化映射表（**包含全部 PII 原文，属明文落盘**）。
    ///
    /// 安全警示：仅用于「无口令」的便捷场景；明文文件比单个原始文档更集中，
    /// 一旦泄露即暴露全部 PII。默认加密路径请用 [`Self::export_encrypted`]。
    pub fn to_json(&self) -> String {
        let mut entries: Vec<(String, String)> = self
            .original_to_placeholder
            .iter()
            .map(|(o, p)| (o.clone(), p.clone()))
            .collect();
        entries.sort();
        serde_json::to_string(&Persisted {
            version: 1,
            entries,
            counters: self.counters.clone(),
            shift: self.shift.clone(),
        })
        .expect("序列化不应失败")
    }

    pub fn from_json(&mut self, json: &str) -> Result<(), String> {
        let persisted: Persisted =
            serde_json::from_str(json).map_err(|e| format!("映射表解析失败: {e}"))?;
        self.original_to_placeholder.clear();
        self.placeholder_to_original.clear();
        // 同一占位符可能对应「全称 + 简称」两条 entry（bind_alias 后两者共享占位符）。
        // 还原时优先取更长的原文（全称）：否则 restore 可能把 [公司1] 还原成简称，
        // 与导出前的还原结果不一致（还原语义漂移）。
        for (original, placeholder) in persisted.entries {
            self.original_to_placeholder
                .insert(original.clone(), placeholder.clone());
            match self.placeholder_to_original.get(&placeholder) {
                Some(existing) if existing.len() > original.len() => {}
                _ => {
                    self.placeholder_to_original
                        .insert(placeholder, original);
                }
            }
        }
        self.counters = persisted.counters;
        self.shift = persisted.shift;
        Ok(())
    }

    /// 口令加密导出（用于 .anonymap 落盘）。
    ///
    /// 每文件随机盐 + PBKDF2 600k 轮，输出 `ANONMAP:v1:{salt}:{AES:{iv}:{ct}}`。
    /// 相比旧格式（固定盐 100k 轮）可抵抗针对固定盐的预计算字典攻击。
    pub fn export_encrypted(&self, passphrase: &str) -> Result<String, String> {
        let mut salt = [0u8; ANONMAP_SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let key = derive_key(passphrase, &salt, ANONMAP_ROUNDS);
        let payload = aes_encrypt(&self.to_json(), &key).map_err(|e| e.to_string())?;
        Ok(format!(
            "{ANONMAP_MAGIC}{}{}{}",
            B64.encode(salt),
            ":",
            payload
        ))
    }

    /// 口令解密导入。
    ///
    /// 支持新格式（版本化头部 + 随机盐）；旧格式（无头、固定盐 100k 轮）按
    /// 兼容路径导入，保证历史 .anonymap.enc 仍可还原。
    pub fn import_encrypted(&mut self, payload: &str, passphrase: &str) -> Result<(), String> {
        let json = if let Some(rest) = payload.strip_prefix(ANONMAP_MAGIC) {
            let mut parts = rest.splitn(2, ':');
            let salt_b64 = parts.next().ok_or("映射表密文格式错误：缺少盐")?;
            let cipher = parts.next().ok_or("映射表密文格式错误：缺少密文")?;
            let salt = B64
                .decode(salt_b64)
                .map_err(|e| format!("映射表盐解码失败: {e}"))?;
            let key = derive_key(passphrase, &salt, ANONMAP_ROUNDS);
            aes_decrypt(cipher, &key).map_err(|e| e.to_string())?
        } else {
            // 旧格式兼容：固定盐 + 100k 轮（v1 之前的 .anonymap.enc）
            let key = derive_key(passphrase, ANONMAP_LEGACY_SALT, ANONMAP_LEGACY_ROUNDS);
            aes_decrypt(payload, &key).map_err(|e| e.to_string())?
        };
        self.from_json(&json)
    }
}

// ---------------------------------------------------------------------------
// 模糊还原
// ---------------------------------------------------------------------------

/// 一处模糊命中（AI 把占位符改了写法，但仍能唯一对应到映射表）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FuzzyHit {
    /// 输入文本中的字符偏移 [start, end)
    pub start: usize,
    pub end: usize,
    /// 还原后文本中的字符偏移 [out_start, out_end)（供前端高亮）
    pub out_start: usize,
    pub out_end: usize,
    /// 文本中的原写法，如 `【 姓名 1 】`
    pub raw: String,
    /// 对应的标准占位符，如 `[姓名1]`
    pub placeholder: String,
    /// 还原出的原文
    pub original: String,
}

/// 疑似未还原的占位符：形如 `[xxx数字]` 但不在映射表里。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuspectPlaceholder {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FuzzyRestoreResult {
    pub output_text: String,
    /// 与标准写法完全一致的还原次数
    pub exact_count: usize,
    /// 模糊命中次数（= fuzzy_hits.len()）
    pub fuzzy_count: usize,
    pub fuzzy_hits: Vec<FuzzyHit>,
    pub suspects: Vec<SuspectPlaceholder>,
}

const OPENERS: &str = "[［【〔「『(（{｛<＜";
const CLOSERS: &str = "]］】〕」』)）}｝>＞";
/// 疑似占位符只认方括号一族（圆括号/尖括号太容易误报“（图1）”之类）
const SUSPECT_OPENERS: &str = "[［【〔";
/// 括号内最长允许的字符数（占位符标签很短，超过即不是占位符）
const MAX_INNER_CHARS: usize = 40;

/// 归一化括号内文本：去掉空白/零宽字符与反斜杠转义，全角 ASCII 转半角，字母转小写。
fn normalize_inner(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_whitespace() && c != '\n' && c != '\t' || matches!(c, '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\\') {
            continue;
        }
        let c = match c {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        };
        for l in c.to_lowercase() {
            out.push(l);
        }
    }
    out
}

/// 是否含不可编辑的分隔符（制表/换行）：这类跨度不做模糊匹配。
fn has_hard_break(inner: &[char]) -> bool {
    inner.iter().any(|c| *c == '\n' || *c == '\t' || *c == '\r')
}

/// `[姓名1]` 形：非数字、非空白前缀（1~12 字）+ 阿拉伯数字尾。
fn looks_like_placeholder(norm: &str) -> bool {
    let chars: Vec<char> = norm.chars().collect();
    let digits = chars.iter().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 6 {
        return false;
    }
    let prefix = &chars[..chars.len() - digits];
    !prefix.is_empty()
        && prefix.len() <= 12
        && prefix.iter().all(|c| !c.is_ascii_digit() && !c.is_whitespace())
}

/// 文本中的一个“括号包裹的候选”：[open, close] 为含括号的字符区间。
pub struct BracketCand {
    pub start: usize,
    pub end: usize,
    pub open: char,
    pub close: char,
    pub inner: String,
}

/// 扫描所有 `开括号 … 闭括号` 候选（中间不含其它括号、不超过 MAX_INNER_CHARS）。
pub fn scan_brackets(chars: &[char]) -> Vec<BracketCand> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if OPENERS.contains(chars[i]) {
            let mut j = i + 1;
            let mut found = None;
            while j < chars.len() && j - i - 1 <= MAX_INNER_CHARS {
                let c = chars[j];
                if CLOSERS.contains(c) {
                    found = Some(j);
                    break;
                }
                if OPENERS.contains(c) {
                    break;
                }
                j += 1;
            }
            if let Some(j) = found {
                let inner_chars = &chars[i + 1..j];
                if !has_hard_break(inner_chars) {
                    // Markdown 转义 `\[姓名1\]`：把前导反斜杠并入命中区间
                    let start = if i > 0 && chars[i - 1] == '\\' { i - 1 } else { i };
                    out.push(BracketCand {
                        start,
                        end: j + 1,
                        open: chars[i],
                        close: chars[j],
                        inner: inner_chars.iter().collect(),
                    });
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

impl MappingTable {
    /// 归一化占位符内文 → 标准占位符（归一化后有歧义的键不收录，宁漏勿错）。
    fn fuzzy_index(&self) -> HashMap<String, String> {
        let mut idx: HashMap<String, Option<String>> = HashMap::new();
        for p in self.placeholder_to_original.keys() {
            let inner = p.trim_start_matches('[').trim_end_matches(']');
            let key = normalize_inner(inner);
            match idx.get(&key) {
                Some(_) => {
                    idx.insert(key, None);
                }
                None => {
                    idx.insert(key, Some(p.clone()));
                }
            }
        }
        idx.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))).collect()
    }

    /// 容错还原：在 [`Self::restore`] 之上，容忍 AI 回复改写占位符的写法——
    /// 全角/六角括号（【】［］〔〕「」（）等）、括号内插空格（`[ 姓名 1 ]`）、
    /// 全角数字、字母大小写、Markdown 转义等。
    ///
    /// 精确匹配与模糊匹配分别计数；模糊命中逐条返回（位置 + 原写法）供人工确认，
    /// 并给出“疑似未还原占位符”清单（形如 `[xxx数字]` 但不在映射表中）。
    /// 无法识别的改写（`PER-7`、`该人员`）不在其列。
    pub fn restore_fuzzy(&self, text: &str) -> FuzzyRestoreResult {
        let chars: Vec<char> = text.chars().collect();
        let index = self.fuzzy_index();
        let mut out = String::with_capacity(text.len());
        let mut out_len = 0usize; // 已输出字符数
        let mut res = FuzzyRestoreResult::default();
        let mut cursor = 0usize;
        for cand in scan_brackets(&chars) {
            if cand.start < cursor {
                continue;
            }
            let norm = normalize_inner(&cand.inner);
            let std_ph = format!("[{}]", cand.inner);
            let hit: Option<&String> = if cand.open == '[' && cand.close == ']'
                && cand.start == cand.end - cand.inner.chars().count() - 2
                && self.placeholder_to_original.contains_key(&std_ph)
            {
                self.placeholder_to_original.get_key_value(&std_ph).map(|(k, _)| k)
            } else {
                index.get(&norm)
            };
            let Some(ph) = hit else {
                if SUSPECT_OPENERS.contains(cand.open)
                    && CLOSERS.contains(cand.close)
                    && looks_like_placeholder(&norm)
                {
                    res.suspects.push(SuspectPlaceholder {
                        start: cand.start,
                        end: cand.end,
                        text: chars[cand.start..cand.end].iter().collect(),
                    });
                }
                continue;
            };
            let original = &self.placeholder_to_original[ph];
            let before: String = chars[cursor..cand.start].iter().collect();
            out_len += before.chars().count();
            out.push_str(&before);
            let raw: String = chars[cand.start..cand.end].iter().collect();
            if raw == *ph {
                res.exact_count += 1;
            } else {
                res.fuzzy_hits.push(FuzzyHit {
                    start: cand.start,
                    end: cand.end,
                    out_start: out_len,
                    out_end: out_len + original.chars().count(),
                    raw,
                    placeholder: ph.clone(),
                    original: original.clone(),
                });
            }
            out.push_str(original);
            out_len += original.chars().count();
            cursor = cand.end;
        }
        out.extend(chars[cursor..].iter());
        res.fuzzy_count = res.fuzzy_hits.len();
        res.output_text = out;
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_original_same_placeholder() {
        let mut m = MappingTable::new();
        let p1 = m.placeholder_for("人名", "张三");
        let p2 = m.placeholder_for("人名", "张三");
        assert_eq!(p1, p2);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn counter_per_label() {
        let mut m = MappingTable::new();
        assert_eq!(m.placeholder_for("人名", "张三"), "[人名1]");
        assert_eq!(m.placeholder_for("公司", "北京降本增笑有限公司"), "[公司1]");
        assert_eq!(m.placeholder_for("人名", "李四"), "[人名2]");
    }

    #[test]
    fn restore_roundtrip() {
        let mut m = MappingTable::new();
        let text = "张三将股份转让给李四";
        let p1 = m.placeholder_for("人名", "张三");
        let p2 = m.placeholder_for("人名", "李四");
        let masked = text.replace("张三", &p1).replace("李四", &p2);
        assert_eq!(m.restore(&masked), text);
    }

    #[test]
    fn alias_binding_same_placeholder() {
        let mut m = MappingTable::new();
        let p = m.placeholder_for("公司", "北京降本增笑有限公司");
        m.bind_alias("降本增笑", &p);
        assert_eq!(m.placeholder_of("降本增笑"), Some(p.as_str()));
        // 简称绑定不消耗新编号：下一家公司从 [公司2] 开始
        assert_eq!(m.placeholder_for("公司", "另一家公司"), "[公司2]");
    }

    #[test]
    fn alias_restore_after_persist_uses_full_name() {
        // 全称+简称共享占位符并持久化后，restore 必须还原为全称（此前可能被
        // 简称覆盖，导致 [公司1] 还原成简称——还原语义漂移）
        let mut m = MappingTable::new();
        let full = "北京降本增笑有限公司";
        let p = m.placeholder_for("公司", full);
        m.bind_alias("降本增笑", &p);
        let json = m.to_json();
        let mut m2 = MappingTable::new();
        m2.from_json(&json).unwrap();
        assert_eq!(m2.restore("[公司1]"), full);
    }

    #[test]
    fn placeholder_label_collision_avoided() {
        // label「手机号1」生成的 [手机号11] 不得覆盖 label「手机号」的第 11 个占位符
        let mut m = MappingTable::new();
        let p1 = m.placeholder_for("手机号", "13800000001");
        // 手动推进「手机号」计数到 11
        for i in 2..=11 {
            let _ = m.placeholder_for("手机号", &format!("138000000{i:02}"));
        }
        assert_eq!(p1, "[手机号1]");
        let p11 = m.placeholder_for("手机号", "13800000011");
        assert_eq!(p11, "[手机号11]");
        // label「手机号1」再为其它原文分配：不得撞 [手机号11]
        let px = m.placeholder_for("手机号1", "内容A");
        assert_ne!(px, "[手机号11]", "跨 label 撞串必须规避: {px}");
        // 两个映射都保留
        assert_eq!(m.placeholder_of("13800000011"), Some("[手机号11]"));
        assert_eq!(m.placeholder_of("内容A"), Some(px.as_str()));
    }

    #[test]
    fn bind_alias_removes_orphan_placeholder() {
        // 别名先被独立分配过占位符，再绑到全称：旧占位符应被清理，
        // 避免已输出文档中的旧占位符被错误还原（新旧映射不一致）
        let mut m = MappingTable::new();
        let old_p = m.placeholder_for("公司", "降本增笑"); // 独立分配
        assert_eq!(old_p, "[公司1]");
        let full_p = m.placeholder_for("公司", "北京降本增笑有限公司");
        m.bind_alias("降本增笑", &full_p);
        assert_eq!(m.placeholder_of("降本增笑"), Some(full_p.as_str()));
        // 旧占位符 [公司1] 不再可还原（宁可不还原，也不还原错）
        assert!(
            m.placeholder_to_original().get("[公司1]").is_none(),
            "孤儿占位符应被清理"
        );
    }

    #[test]
    fn persist_roundtrip() {
        let mut m = MappingTable::new();
        m.placeholder_for("人名", "张三");
        m.placeholder_for("公司", "北京降本增笑有限公司");
        let json = m.to_json();
        let mut m2 = MappingTable::new();
        m2.from_json(&json).unwrap();
        assert_eq!(m2.placeholder_of("张三"), m.placeholder_of("张三"));
        assert_eq!(m2.restore("[人名1]"), "张三");
    }

    #[test]
    fn encrypted_export_import() {
        let mut m = MappingTable::new();
        m.placeholder_for("人名", "张三");
        let payload = m.export_encrypted("passphrase").unwrap();
        // 新格式带版本化头部（随机盐 + 600k 轮）
        assert!(payload.starts_with("ANONMAP:v1:"), "应带版本化头部: {payload}");
        let mut m2 = MappingTable::new();
        m2.import_encrypted(&payload, "passphrase").unwrap();
        assert_eq!(m2.placeholder_of("张三"), m.placeholder_of("张三"));
        // 错误口令失败
        let mut m3 = MappingTable::new();
        assert!(m3.import_encrypted(&payload, "wrong").is_err());
        // 同一口令两次导出盐不同（随机盐，抗预计算字典）
        let p2 = m.export_encrypted("passphrase").unwrap();
        assert_ne!(payload, p2, "随机盐应使两次导出不同");
    }

    #[test]
    fn encrypted_import_legacy_format() {
        // 旧格式（无头、固定盐 100k 轮）仍可导入，保证历史 .anonymap.enc 可还原
        let mut m = MappingTable::new();
        m.placeholder_for("人名", "张三");
        let key = derive_key("passphrase", ANONMAP_LEGACY_SALT, ANONMAP_LEGACY_ROUNDS);
        let legacy = aes_encrypt(&m.to_json(), &key).unwrap();
        assert!(!legacy.starts_with("ANONMAP:v1:"));
        let mut m2 = MappingTable::new();
        m2.import_encrypted(&legacy, "passphrase").unwrap();
        assert_eq!(m2.placeholder_of("张三"), m.placeholder_of("张三"));
        // 旧格式错误口令仍失败
        let mut m3 = MappingTable::new();
        assert!(m3.import_encrypted(&legacy, "wrong").is_err());
    }

    #[test]
    fn restore_no_partial_substitution() {
        let mut m = MappingTable::new();
        m.placeholder_for("人名", "张三");
        // 占位符为 [人名1]，应整体替换而非部分匹配
        assert_eq!(m.restore("[人名1]和[人名10]"), "张三和[人名10]");
    }

    #[test]
    fn restore_placeholder_adjacent_to_chinese_text() {
        // 中文文书里占位符两侧通常紧贴汉字，必须还原（旧的“至少一侧非汉字”规则会让这类场景全部失败）
        let mut m = MappingTable::new();
        m.placeholder_for("手机号", "13800138000");
        assert_eq!(m.restore("示例格式如[手机号1]所示"), "示例格式如13800138000所示");
        assert_eq!(m.restore("电话：[手机号1]"), "电话：13800138000");
        assert_eq!(m.restore("[手机号1]待确认"), "13800138000待确认");
        // 映射表里没有的占位符保持原样
        assert_eq!(m.restore("见[手机号9]"), "见[手机号9]");
    }

    #[test]
    fn restore_adjacent_to_chinese() {
        let mut m = MappingTable::new();
        let c = m.placeholder_for("公司", "北京云图科技有限公司");
        let n = m.placeholder_for("姓名", "陈瑾瑜");
        let text = format!("甲方{c}委托乙方，由{n}签字，{n}。");
        let back = m.restore(&text);
        assert_eq!(back, "甲方北京云图科技有限公司委托乙方，由陈瑾瑜签字，陈瑾瑜。", "{back}");
    }

    // ---- restore_fuzzy ----

    fn fz() -> MappingTable {
        let mut m = MappingTable::new();
        m.placeholder_for("姓名", "陈瑾瑜");
        m.placeholder_for("姓名", "李四");
        m.placeholder_for("公司", "北京云图科技有限公司");
        m.placeholder_for("PER", "王五");
        m
    }

    #[test]
    fn fuzzy_exact_only_counts_exact() {
        let r = fz().restore_fuzzy("甲方[公司1]由[姓名1]签字，[姓名2]见证。");
        assert_eq!(r.output_text, "甲方北京云图科技有限公司由陈瑾瑜签字，李四见证。");
        assert_eq!((r.exact_count, r.fuzzy_count), (3, 0));
        assert!(r.suspects.is_empty());
    }

    #[test]
    fn fuzzy_bracket_variants_and_spaces() {
        let m = fz();
        for raw in [
            "【姓名1】", "［姓名1］", "〔姓名1〕", "「姓名1」", "（姓名1）", "(姓名1)",
            "[ 姓名1 ]", "[姓名 1]", "[ 姓名 1 ]", "【 姓 名 1 】", "[姓名１]", "［姓名１］",
            "\\[姓名1\\]", "[姓名1】", "[\u{3000}姓名1\u{3000}]",
        ] {
            let r = m.restore_fuzzy(&format!("由{raw}签字"));
            assert_eq!(r.output_text, "由陈瑾瑜签字", "写法 {raw}");
            assert_eq!((r.exact_count, r.fuzzy_count), (0, 1), "写法 {raw}");
            assert_eq!(r.fuzzy_hits[0].raw, raw);
            assert_eq!(r.fuzzy_hits[0].placeholder, "[姓名1]");
        }
    }

    #[test]
    fn fuzzy_case_insensitive_label() {
        let m = fz();
        let r = m.restore_fuzzy("见[per1]与【Per 1】");
        assert_eq!(r.output_text, "见王五与王五");
        assert_eq!(r.fuzzy_count, 2);
    }

    #[test]
    fn fuzzy_inside_chinese_quotes() {
        let r = fz().restore_fuzzy("“【姓名1】”和‘[ 姓名2 ]’");
        assert_eq!(r.output_text, "“陈瑾瑜”和‘李四’");
        assert_eq!(r.fuzzy_count, 2);
    }

    #[test]
    fn fuzzy_positions_cover_input_and_output() {
        let r = fz().restore_fuzzy("甲：[姓名1]，乙：【姓名2】。");
        assert_eq!(r.exact_count, 1);
        let h = &r.fuzzy_hits[0];
        let in_chars: Vec<char> = "甲：[姓名1]，乙：【姓名2】。".chars().collect();
        assert_eq!(in_chars[h.start..h.end].iter().collect::<String>(), "【姓名2】");
        let out_chars: Vec<char> = r.output_text.chars().collect();
        assert_eq!(out_chars[h.out_start..h.out_end].iter().collect::<String>(), "李四");
    }

    #[test]
    fn fuzzy_suspects_listed_but_unrecognizable_not() {
        let r = fz().restore_fuzzy("[姓名9] 与【公司7】，还有 PER-7、该人员、[1]、（图1）、[备注]。");
        assert_eq!(r.output_text, "[姓名9] 与【公司7】，还有 PER-7、该人员、[1]、（图1）、[备注]。");
        let texts: Vec<_> = r.suspects.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["[姓名9]", "【公司7】"]);
    }

    #[test]
    fn fuzzy_does_not_confuse_prefix_numbers() {
        let mut m = MappingTable::new();
        for i in 1..=10 {
            m.placeholder_for("手机号", &format!("1380000{i:04}"));
        }
        let r = m.restore_fuzzy("[手机号1] 与 [ 手机号 10 ]");
        assert_eq!(r.output_text, "13800000001 与 13800000010");
        assert_eq!((r.exact_count, r.fuzzy_count), (1, 1));
    }

    #[test]
    fn fuzzy_agrees_with_restore_on_exact_input() {
        let m = fz();
        let t = "甲方[公司1]委托乙方，由[姓名1]签字，[姓名2]、[姓名1]。[姓名8]";
        assert_eq!(m.restore_fuzzy(t).output_text, m.restore(t));
    }

    #[test]
    fn fuzzy_multibyte_offsets() {
        let r = fz().restore_fuzzy("😀【姓名1】😀");
        assert_eq!(r.output_text, "😀陈瑾瑜😀");
        let h = &r.fuzzy_hits[0];
        assert_eq!((h.start, h.end, h.out_start, h.out_end), (1, 6, 1, 4));
    }
}
