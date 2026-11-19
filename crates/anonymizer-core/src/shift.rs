//! 保结构替换：金额按比例缩放、日期整体平移。
//!
//! 用途：合同审查 / 尽调发给 AI 时既隐藏真实数值，又保持数量关系
//! （大写与小写金额一致、两笔款项的比例不变、两个日期的间隔不变、星期不变）。
//!
//! # 安全定位（务必如实告知用户）
//! 这是**假名化**而不是匿名化：同一案件共用一组系数（每币种一个 k、一个日期偏移 Δd），
//! 攻击者只要掌握任一“真实值 ↔ 替换值”对照，就能反推出全部真实数值；
//! 金额之间的比例、日期之间的间隔也原样保留。HIPAA 等要求去标识化的场景**不适用**。
//!
//! # 设计要点
//! - 系数由系统随机数生成（[`ShiftParams::random`]），**不由原文派生**；
//!   `seed` 只用于“新币种首次出现时派生 k”，同样来自随机数。
//! - 金额解析为「数值（分）+ 格式描述」，按原格式重新渲染：货币符号、千分位、小数位、
//!   万/亿单位、“整”、中文大写（含角分）都保留。
//! - 同一个数值（不论写成大写还是小写）只会算出**一个**新值（按数值记忆化），
//!   所以“人民币壹佰万元整（¥1,000,000.00）”缩放后两处仍一致。
//! - 百分比、费率、“第X条”、法律法规的施行/公布日期不处理（返回 `None`，调用方保留原文）。
//! - 可逆：[`ShiftBook`] 记录“原文 → 替换值”，[`ShiftBook::restore_exact`] 走查表精确还原；
//!   AI 回复里新算出的数没有对照，只给出**建议值**（[`ShiftBook::suggest_unshift`]），不自动替换。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use rand::rngs::{OsRng, StdRng};
use rand::{Rng, RngCore, SeedableRng};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// 缩放系数取值区间（避开 0.93–1.07，防止“几乎没变”）。
pub const K_LOW: (f64, f64) = (0.62, 0.93);
pub const K_HIGH: (f64, f64) = (1.07, 1.6);

// ---------------------------------------------------------------------------
// 参数
// ---------------------------------------------------------------------------

/// 一个案件的保结构参数。随机生成、不由原文派生。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShiftParams {
    /// 随机种子（仅用于新币种首次出现时派生 k）
    pub seed: u64,
    /// 每币种缩放系数（"CNY"/"USD"…）
    pub k: BTreeMap<String, f64>,
    /// 日期平移天数（±30~365；`keep_weekday` 时为 7 的倍数）
    pub delta_days: i64,
    pub keep_weekday: bool,
}

fn fnv64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn sample_k(rng: &mut StdRng) -> f64 {
    let low = K_LOW.1 - K_LOW.0;
    let high = K_HIGH.1 - K_HIGH.0;
    let x = rng.gen_range(0.0..(low + high));
    let k = if x < low { K_LOW.0 + x } else { K_HIGH.0 + (x - low) };
    ((k * 10_000.0).round() / 10_000.0).clamp(K_LOW.0, K_HIGH.1)
}

impl ShiftParams {
    /// 用系统随机数生成（生产路径）。
    pub fn random() -> Self {
        Self::from_seed(OsRng.next_u64(), true)
    }

    /// 由种子生成（测试可复现；生产路径的种子来自 [`OsRng`]）。
    pub fn from_seed(seed: u64, keep_weekday: bool) -> Self {
        let mut rng = StdRng::seed_from_u64(seed ^ 0x9e37_79b9_7f4a_7c15);
        let sign = if rng.gen_bool(0.5) { 1 } else { -1 };
        let delta_days = if keep_weekday {
            sign * 7 * rng.gen_range(5..=52) // 35..=364
        } else {
            sign * rng.gen_range(30..=365)
        };
        let mut p = Self { seed, k: BTreeMap::new(), delta_days, keep_weekday };
        p.k_for("CNY");
        p
    }

    /// 指定参数（单测 / 导入用）。
    pub fn fixed(k_cny: f64, delta_days: i64) -> Self {
        let mut k = BTreeMap::new();
        k.insert("CNY".to_string(), k_cny);
        Self { seed: 0, k, delta_days, keep_weekday: delta_days % 7 == 0 }
    }

    /// 某币种的系数（只读：未登记则由种子确定性派生，不写入）。
    pub fn k_peek(&self, currency: &str) -> f64 {
        if let Some(k) = self.k.get(currency) {
            return *k;
        }
        let mut rng = StdRng::seed_from_u64(self.seed ^ fnv64(currency));
        sample_k(&mut rng)
    }

    /// 某币种的系数（未登记则派生并登记）。
    pub fn k_for(&mut self, currency: &str) -> f64 {
        let k = self.k_peek(currency);
        self.k.entry(currency.to_string()).or_insert(k);
        k
    }

    /// 年月（无日）按月平移的月数：天数折算，至少 ±1。
    pub fn delta_months(&self) -> i64 {
        let m = (self.delta_days as f64 / 30.4375).round() as i64;
        if m == 0 { self.delta_days.signum() } else { m }
    }
}

// ---------------------------------------------------------------------------
// 账本：参数 + 原文→替换值（可持久化，随映射表保存）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShiftBook {
    #[serde(default)]
    pub params: Option<ShiftParams>,
    /// 原文 → 替换值（精确还原用）
    #[serde(default)]
    pub entries: BTreeMap<String, String>,
    /// 金额按数值记忆化：`币种:分` → 新值（分），保证同一数值写法不同也得到同一新值
    #[serde(default)]
    pub amounts: BTreeMap<String, i64>,
}

/// 对 AI 回复中“新算出的数”给出的建议（不自动替换）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnshiftSuggestion {
    /// 在回复文本中的字节区间
    pub start: usize,
    pub end: usize,
    /// 回复里的原写法
    pub text: String,
    /// 建议的真实值（金额 v/k，日期 −Δd）
    pub suggested: String,
    /// "amount" / "date"
    pub kind: String,
}

impl ShiftBook {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn params_mut(&mut self) -> &mut ShiftParams {
        self.params.get_or_insert_with(ShiftParams::random)
    }

    /// 仅在尚无参数时采用（打开旧项目 / 导入时用）。
    pub fn adopt_params(&mut self, p: ShiftParams) {
        if self.params.is_none() {
            self.params = Some(p);
        }
    }

    /// 金额缩放。返回 `None` 表示“不应处理，保留原文”（百分比/费率/条款号/无法解析等）。
    pub fn scale_amount(&mut self, text: &str, before: &str, after: &str) -> Option<String> {
        if amount_excluded(text, before, after) {
            return None;
        }
        let parsed = parse_amount(text, before, after)?;
        if parsed.fen == 0 {
            return None;
        }
        if let Some(r) = self.entries.get(text) {
            return Some(r.clone());
        }
        let key = format!("{}:{}", parsed.currency, parsed.fen);
        let new_fen = match self.amounts.get(&key) {
            Some(v) => *v as i128,
            None => {
                let k = self.params_mut().k_for(parsed.currency);
                let v = scale_fen(parsed.fen, k, parsed.granularity());
                self.amounts.insert(key, v as i64);
                v
            }
        };
        let out = render_amount(&parsed, new_fen)?;
        self.entries.insert(text.to_string(), out.clone());
        Some(out)
    }

    /// 日期平移。返回 `None` 表示不处理（法规施行/公布日期、非法日期等）。
    pub fn shift_date(&mut self, text: &str, before: &str, after: &str) -> Option<String> {
        if law_date_context(before, after) {
            return None;
        }
        if let Some(r) = self.entries.get(text) {
            return Some(r.clone());
        }
        let p = self.params_mut().clone();
        let out = shift_date_text(text, p.delta_days, p.delta_months())?;
        self.entries.insert(text.to_string(), out.clone());
        Some(out)
    }

    /// 走查表精确还原：把文本里出现的“替换值”换回原文（最长优先，数字边界检查）。
    pub fn restore_exact(&self, text: &str) -> String {
        let mut rev: BTreeMap<&str, &str> = BTreeMap::new();
        for (o, r) in &self.entries {
            // 多个原文映射到同一替换值时无法唯一还原：保留字典序最小者（确定性）
            rev.entry(r.as_str()).or_insert(o.as_str());
        }
        if rev.is_empty() {
            return text.to_string();
        }
        let mut cands: Vec<(&str, &str)> = rev.into_iter().collect();
        cands.sort_by_key(|(r, _)| std::cmp::Reverse(r.len()));
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        'outer: while i < text.len() {
            for (r, o) in &cands {
                if text[i..].starts_with(r) && boundary_ok(text, i, i + r.len(), r) {
                    out.push_str(o);
                    i += r.len();
                    continue 'outer;
                }
            }
            let ch = text[i..].chars().next().expect("字符边界");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// 对 AI 回复里**新算出**的金额/日期给出建议的真实值（v/k、−Δd）。
    /// 已在对照表里的替换值不再建议（用 [`Self::restore_exact`] 精确还原）；**不自动替换**。
    pub fn suggest_unshift(&self, reply: &str) -> Vec<UnshiftSuggestion> {
        let Some(p) = &self.params else { return Vec::new() };
        let known: std::collections::HashSet<&str> =
            self.entries.values().map(|s| s.as_str()).collect();
        let mut out = Vec::new();
        for m in rule_re(crate::rules::ID_AMOUNT).find_iter(reply) {
            let t = m.as_str();
            let (b, a) = ctx_around(reply, m.start(), m.end());
            if known.contains(t) || amount_excluded(t, b, a) {
                continue;
            }
            let Some(parsed) = parse_amount(t, b, a) else { continue };
            let k = p.k_peek(parsed.currency);
            let target = (parsed.fen as f64 / k / parsed.granularity() as f64).round() as i128
                * parsed.granularity();
            if let Some(s) = render_amount(&parsed, target.max(parsed.granularity())) {
                out.push(UnshiftSuggestion {
                    start: m.start(),
                    end: m.end(),
                    text: t.to_string(),
                    suggested: s,
                    kind: "amount".into(),
                });
            }
        }
        for m in rule_re(crate::rules::ID_DATE).find_iter(reply) {
            let t = m.as_str();
            let (b, a) = ctx_around(reply, m.start(), m.end());
            if known.contains(t) || law_date_context(b, a) {
                continue;
            }
            if let Some(s) = shift_date_text(t, -p.delta_days, -p.delta_months()) {
                out.push(UnshiftSuggestion {
                    start: m.start(),
                    end: m.end(),
                    text: t.to_string(),
                    suggested: s,
                    kind: "date".into(),
                });
            }
        }
        out.sort_by_key(|s| s.start);
        out
    }
}

/// 数字边界：替换值首/尾是数字时，其外侧不能紧挨数字或千分位（避免 `5万` 命中 `15万`）。
fn boundary_ok(text: &str, start: usize, end: usize, r: &str) -> bool {
    let first = r.chars().next();
    let last = r.chars().last();
    if first.is_some_and(|c| c.is_ascii_digit()) {
        if let Some(p) = text[..start].chars().next_back() {
            if p.is_ascii_digit() || p == ',' || p == '.' {
                return false;
            }
        }
    }
    if last.is_some_and(|c| c.is_ascii_digit()) {
        if let Some(n) = text[end..].chars().next() {
            if n.is_ascii_digit() || n == ',' {
                return false;
            }
        }
    }
    true
}

fn rule_re(id: &str) -> &'static Regex {
    static AMOUNT: OnceLock<Regex> = OnceLock::new();
    static DATE: OnceLock<Regex> = OnceLock::new();
    let cell = if id == crate::rules::ID_AMOUNT { &AMOUNT } else { &DATE };
    cell.get_or_init(|| {
        let r = crate::rules::builtin_rules()
            .into_iter()
            .find(|r| r.id == id)
            .expect("内置规则存在");
        Regex::new(&r.regex).expect("内置规则正则有效")
    })
}

// ---------------------------------------------------------------------------
// 上下文窗口
// ---------------------------------------------------------------------------

const CTX_B: usize = 24;
const CTX_A: usize = 40;

/// 取字节区间前后的上下文窗口（按字符数截取，调用方传字节偏移）。
pub fn ctx_around(text: &str, start: usize, end: usize) -> (&str, &str) {
    let start = start.min(text.len());
    let end = end.min(text.len()).max(start);
    let b = &text[..start];
    let bs = b
        .char_indices()
        .rev()
        .nth(CTX_B - 1)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let a = &text[end..];
    let ae = a.char_indices().nth(CTX_A).map(|(i, _)| i).unwrap_or(a.len());
    (&b[bs..], &a[..ae])
}

/// 同上，输入为字符数组与字符偏移（命令层用）。
pub fn ctx_around_chars(chars: &[char], start: usize, end: usize) -> (String, String) {
    let start = start.min(chars.len());
    let end = end.min(chars.len()).max(start);
    let b: String = chars[start.saturating_sub(CTX_B)..start].iter().collect();
    let a: String = chars[end..(end + CTX_A).min(chars.len())].iter().collect();
    (b, a)
}

const CLAUSE_DELIMS: [char; 9] = ['。', '；', ';', '，', ',', '\n', '！', '？', '!'];

/// 命中所在分句：前文取最后一个分隔符之后，后文取第一个分隔符之前。
fn clause_before(before: &str) -> &str {
    match before.rfind(|c| CLAUSE_DELIMS.contains(&c)) {
        Some(i) => &before[i + before[i..].chars().next().map_or(1, |c| c.len_utf8())..],
        None => before,
    }
}
fn clause_after(after: &str) -> &str {
    match after.find(|c| CLAUSE_DELIMS.contains(&c)) {
        Some(i) => &after[..i],
        None => after,
    }
}

// ---------------------------------------------------------------------------
// 日期
// ---------------------------------------------------------------------------

/// 法律法规的施行 / 公布 / 修订日期不平移：它们是公开事实，平移后反而错。
/// - 施行/公布/修正/修订/颁布：同一分句出现即视为法规日期；
/// - “通过”较含糊（董事会决议也常“通过”），要求同句另有立法机关/法规名称线索。
pub fn law_date_context(before: &str, after: &str) -> bool {
    let w = format!("{}{}", clause_before(before), clause_after(after));
    const ALWAYS: [&str; 5] = ["施行", "公布", "修正", "修订", "颁布"];
    if ALWAYS.iter().any(|k| w.contains(k)) {
        return true;
    }
    const LEGIS: [&str; 10] = [
        "人民代表大会", "常务委员会", "国务院", "立法", "审议", "法》", "条例》", "规定》", "办法》", "解释》",
    ];
    w.contains("通过") && LEGIS.iter().any(|k| w.contains(k))
}

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}
fn dim(y: i64, m: i64) -> i64 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// 公历日期 → 自 1970-01-01 起的天数（Howard Hinnant 算法）。
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 星期（0=周日 … 6=周六）。
pub fn weekday(y: i64, m: i64, d: i64) -> i64 {
    (days_from_civil(y, m, d) + 4).rem_euclid(7)
}

fn num(s: &str) -> Option<i64> {
    s.parse().ok()
}

/// 平移一个日期文本；`delta_months` 用于只有年月的写法。格式（分隔符、空格、补零）原样保留。
pub fn shift_date_text(text: &str, delta_days: i64, delta_months: i64) -> Option<String> {
    static FULL_CN: OnceLock<Regex> = OnceLock::new();
    static YM_CN: OnceLock<Regex> = OnceLock::new();
    static ISO: OnceLock<Regex> = OnceLock::new();
    let full_cn = FULL_CN.get_or_init(|| {
        Regex::new(r"^(\d{4})(\s*)年(\s*)(\d{1,2})(\s*)月(\s*)(\d{1,2})(\s*)日$").unwrap()
    });
    let ym_cn = YM_CN.get_or_init(|| Regex::new(r"^(\d{4})(\s*)年(\s*)(\d{1,2})(\s*)月$").unwrap());
    let iso = ISO.get_or_init(|| Regex::new(r"^(\d{4})([-./])(\d{1,2})([-./])(\d{1,2})$").unwrap());

    if let Some(c) = full_cn.captures(text) {
        let (y, m, d) = (num(&c[1])?, num(&c[4])?, num(&c[7])?);
        let (ny, nm, nd) = shift_ymd(y, m, d, delta_days)?;
        let pad = c[4].starts_with('0') || c[7].starts_with('0');
        let (ms, ds) = (fmt2(nm, pad), fmt2(nd, pad));
        return Some(format!("{ny:04}{}年{}{ms}{}月{}{ds}{}日", &c[2], &c[3], &c[5], &c[6], &c[8]));
    }
    if let Some(c) = ym_cn.captures(text) {
        let (y, m) = (num(&c[1])?, num(&c[4])?);
        if !(1..=12).contains(&m) || !(1900..=2200).contains(&y) {
            return None;
        }
        let idx = y * 12 + (m - 1) + delta_months;
        let (ny, nm) = (idx.div_euclid(12), idx.rem_euclid(12) + 1);
        let ms = fmt2(nm, c[4].starts_with('0'));
        return Some(format!("{ny:04}{}年{}{ms}{}月", &c[2], &c[3], &c[5]));
    }
    if let Some(c) = iso.captures(text) {
        let (y, m, d) = (num(&c[1])?, num(&c[3])?, num(&c[5])?);
        let (ny, nm, nd) = shift_ymd(y, m, d, delta_days)?;
        let pad = c[3].len() == 2 && c[5].len() == 2;
        return Some(format!("{ny:04}{}{}{}{}", &c[2], fmt2(nm, pad), &c[4], fmt2(nd, pad)));
    }
    None
}

fn fmt2(n: i64, pad: bool) -> String {
    if pad { format!("{n:02}") } else { n.to_string() }
}

fn shift_ymd(y: i64, m: i64, d: i64, delta: i64) -> Option<(i64, i64, i64)> {
    if !(1900..=2200).contains(&y) || !(1..=12).contains(&m) || d < 1 || d > dim(y, m) {
        return None;
    }
    let (ny, nm, nd) = civil_from_days(days_from_civil(y, m, d) + delta);
    (1..=9999).contains(&ny).then_some((ny, nm, nd))
}

// ---------------------------------------------------------------------------
// 金额
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    One,
    Wan,
    Yi,
}

impl Unit {
    fn exp(self) -> u32 {
        match self {
            Unit::One => 0,
            Unit::Wan => 4,
            Unit::Yi => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Style {
    Arabic { prefix: String, thousands: bool, decimals: usize, unit: Unit, suffix: String },
    Upper { prefix: String, yuan: char, zheng: bool, had_fraction: bool },
}

#[derive(Debug, Clone, PartialEq)]
struct Parsed {
    currency: &'static str,
    /// 数值，单位“分”
    fen: i128,
    style: Style,
}

impl Parsed {
    /// 原写法能表达的最小步长（分）。缩放结果取整到此步长，保持原写法精度。
    fn granularity(&self) -> i128 {
        match &self.style {
            Style::Arabic { decimals, unit, .. } => {
                10i128.pow((2 + unit.exp()).saturating_sub(*decimals as u32))
            }
            Style::Upper { had_fraction, .. } => {
                if *had_fraction { 1 } else { 100 }
            }
        }
    }
}

fn scale_fen(fen: i128, k: f64, g: i128) -> i128 {
    let mut v = ((fen as f64 * k) / g as f64).round() as i128 * g;
    if v == fen {
        v += if k > 1.0 { g } else { -g };
    }
    v.max(g)
}

fn currency_of(s: &str) -> Option<&'static str> {
    if s.contains("US$") || s.contains("USD") || s.contains('$') && !s.contains("HK") || s.contains("美元") {
        Some("USD")
    } else if s.contains("HK$") || s.contains("HKD") || s.contains("港元") || s.contains("港币") {
        Some("HKD")
    } else if s.contains('€') || s.contains("EUR") || s.contains("欧元") {
        Some("EUR")
    } else if s.contains('£') || s.contains("英镑") {
        Some("GBP")
    } else if s.contains("日元") {
        Some("JPY")
    } else if s.contains('¥') || s.contains('￥') || s.contains("人民币") || s.contains("RMB") || s.contains("CNY") {
        Some("CNY")
    } else {
        None
    }
}

fn tail_chars(s: &str, n: usize) -> &str {
    s.char_indices().rev().nth(n.saturating_sub(1)).map_or(s, |(i, _)| &s[i..])
}
fn head_chars(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

const UPPER_DIGITS: [char; 10] = ['零', '壹', '贰', '叁', '肆', '伍', '陆', '柒', '捌', '玖'];

fn upper_val(c: char) -> Option<i128> {
    UPPER_DIGITS.iter().position(|&x| x == c).map(|p| p as i128)
}

/// 不应缩放的金额形态：百分比、费率、条款编号、数量。
fn amount_excluded(text: &str, before: &str, after: &str) -> bool {
    let a0 = after.chars().next();
    if matches!(a0, Some('%' | '％' | '‰' | '‱')) || after.starts_with("个百分点") {
        return true;
    }
    if before.trim_end().ends_with('第') {
        return true;
    }
    let has_marker = text.chars().any(|c| "¥￥$€£元圆万亿".contains(c))
        || text.contains("人民币")
        || text.chars().any(|c| UPPER_DIGITS.contains(&c));
    // 条款/章节编号后缀（“第1,200条”这类）
    if matches!(a0, Some('条' | '款' | '项' | '章' | '节' | '编' | '号' | '页' | '期' | '次')) && !has_marker {
        return true;
    }
    if !has_marker {
        // 无货币标记的裸数字：后跟数量单位 → 不是钱；前面是费率类词 → 不缩放
        const QTY: [&str; 12] = ["股", "份", "件", "人", "个", "台", "套", "平方米", "平米", "公里", "吨", "天"];
        if QTY.iter().any(|q| after.starts_with(q)) {
            return true;
        }
        let near = tail_chars(clause_before(before), 6);
        if ["费率", "利率", "税率", "比例", "折扣", "年化", "百分之", "占比", "扣率"].iter().any(|k| near.contains(k)) {
            return true;
        }
    }
    false
}

fn parse_amount(text: &str, before: &str, after: &str) -> Option<Parsed> {
    if text.chars().any(|c| UPPER_DIGITS.contains(&c) || "拾佰仟".contains(c)) {
        return parse_upper(text);
    }
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"^(?P<pre>(?:US|HK)?[¥￥$€£]\s?|(?:人民币|RMB|CNY|USD|EUR|HKD)\s?)?(?P<int>\d{1,3}(?:,\d{3})+|\d+)(?:\.(?P<dec>\d+))?(?P<unit>万|亿)?(?P<suf>元|圆|美元|欧元|港元|港币|英镑|日元)?$",
        )
        .unwrap()
    });
    let c = re.captures(text)?;
    let pre = c.name("pre").map_or("", |m| m.as_str());
    let suf = c.name("suf").map_or("", |m| m.as_str());
    let int_s = c.name("int")?.as_str();
    let thousands = int_s.contains(',');
    let dec_s = c.name("dec").map_or("", |m| m.as_str());
    let unit = match c.name("unit").map(|m| m.as_str()) {
        Some("万") => Unit::Wan,
        Some("亿") => Unit::Yi,
        _ => Unit::One,
    };
    let scale_exp = 2 + unit.exp() as usize;
    if dec_s.len() > scale_exp || int_s.len() > 15 {
        return None;
    }
    let int_v: i128 = int_s.replace(',', "").parse().ok()?;
    let dec_v: i128 = if dec_s.is_empty() { 0 } else { dec_s.parse().ok()? };
    let fen = (int_v * 10i128.pow(dec_s.len() as u32) + dec_v)
        .checked_mul(10i128.pow((scale_exp - dec_s.len()) as u32))?;
    let currency = currency_of(pre)
        .or_else(|| currency_of(suf))
        .or_else(|| currency_of(tail_chars(before, 4)))
        .or_else(|| currency_of(head_chars(after, 3)))
        .unwrap_or("CNY");
    Some(Parsed {
        currency,
        fen,
        style: Style::Arabic {
            prefix: pre.to_string(),
            thousands,
            decimals: dec_s.len(),
            unit,
            suffix: suf.to_string(),
        },
    })
}

/// 解析大写整数部分（零壹贰…、拾佰仟、万亿）。
fn parse_upper_int(s: &str) -> Option<i128> {
    let (mut total, mut grp, mut sec, mut num_) = (0i128, 0i128, 0i128, 0i128);
    let mut seen = false;
    for ch in s.chars() {
        seen = true;
        if let Some(d) = upper_val(ch) {
            num_ = d;
            continue;
        }
        match ch {
            '拾' => {
                sec += if num_ == 0 { 1 } else { num_ } * 10;
                num_ = 0;
            }
            '佰' | '仟' => {
                if num_ == 0 {
                    return None;
                }
                sec += num_ * if ch == '佰' { 100 } else { 1000 };
                num_ = 0;
            }
            '万' => {
                grp = (grp + sec + num_) * 10_000;
                sec = 0;
                num_ = 0;
            }
            '亿' => {
                total = (total + grp + sec + num_) * 100_000_000;
                grp = 0;
                sec = 0;
                num_ = 0;
            }
            _ => return None,
        }
    }
    seen.then_some(total + grp + sec + num_)
}

fn parse_upper(text: &str) -> Option<Parsed> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"^(?P<pre>人民币)?(?P<n>[零壹贰叁肆伍陆柒捌玖拾佰仟万亿]+)(?P<y>元|圆)(?:(?P<z>整)|(?:(?P<j>[零壹贰叁肆伍陆柒捌玖])角)?(?:零?(?P<f>[零壹贰叁肆伍陆柒捌玖])分)?)$",
        )
        .unwrap()
    });
    let c = re.captures(text)?;
    let yuan = parse_upper_int(c.name("n")?.as_str())?;
    let jiao = c.name("j").and_then(|m| upper_val(m.as_str().chars().next()?)).unwrap_or(0);
    let fen_d = c.name("f").and_then(|m| upper_val(m.as_str().chars().next()?)).unwrap_or(0);
    if yuan > 10i128.pow(15) {
        return None;
    }
    Some(Parsed {
        currency: "CNY",
        fen: yuan * 100 + jiao * 10 + fen_d,
        style: Style::Upper {
            prefix: c.name("pre").map_or("", |m| m.as_str()).to_string(),
            yuan: c.name("y")?.as_str().chars().next()?,
            zheng: c.name("z").is_some(),
            had_fraction: c.name("j").is_some() || c.name("f").is_some(),
        },
    })
}

fn upper_group(g: u32) -> String {
    let ds = [g / 1000, g / 100 % 10, g / 10 % 10, g % 10];
    let us = ["仟", "佰", "拾", ""];
    let (mut s, mut zero, mut emitted) = (String::new(), false, false);
    for (d, u) in ds.iter().zip(us) {
        if *d == 0 {
            zero |= emitted;
        } else {
            if zero {
                s.push('零');
                zero = false;
            }
            s.push(UPPER_DIGITS[*d as usize]);
            s.push_str(u);
            emitted = true;
        }
    }
    s
}

/// 整数 → 中文大写（零的处理遵循财务写法：壹万零伍佰、壹亿零伍仟）。
pub fn upper_int(n: i128) -> Option<String> {
    if n == 0 {
        return Some("零".into());
    }
    if !(0..10i128.pow(16)).contains(&n) {
        return None;
    }
    let mut groups = Vec::new();
    let mut x = n;
    while x > 0 {
        groups.push((x % 10_000) as u32);
        x /= 10_000;
    }
    const UNITS: [&str; 4] = ["", "万", "亿", "万亿"];
    let (mut s, mut pending) = (String::new(), false);
    for (i, &g) in groups.iter().enumerate().rev() {
        if g == 0 {
            pending |= !s.is_empty();
            continue;
        }
        if !s.is_empty() && (pending || g < 1000) {
            s.push('零');
        }
        pending = false;
        s.push_str(&upper_group(g));
        s.push_str(UNITS[i]);
    }
    Some(s)
}

fn group3(int: i128) -> String {
    let s = int.to_string();
    let n = s.len();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 按原写法渲染新值（分）。原写法放不下新值的精度时，自动多给小数位 / 角分（宁可多一位也不丢精度，
/// 保证“同一数值的大小写两处一致”）。
fn render_amount(p: &Parsed, fen: i128) -> Option<String> {
    if fen <= 0 {
        return None;
    }
    match &p.style {
        Style::Arabic { prefix, thousands, decimals, unit, suffix } => {
            let scale_exp = (2 + unit.exp()) as usize;
            let mut d = *decimals;
            while d < scale_exp && fen % 10i128.pow((scale_exp - d) as u32) != 0 {
                d += 1;
            }
            let div = 10i128.pow(scale_exp as u32);
            let (int, frac) = (fen / div, fen % div);
            let int_s = if *thousands { group3(int) } else { int.to_string() };
            let mut s = format!("{prefix}{int_s}");
            if d > 0 {
                let f = frac / 10i128.pow((scale_exp - d) as u32);
                s.push_str(&format!(".{f:0width$}", width = d));
            }
            s.push_str(match unit {
                Unit::One => "",
                Unit::Wan => "万",
                Unit::Yi => "亿",
            });
            s.push_str(suffix);
            Some(s)
        }
        Style::Upper { prefix, yuan, zheng, .. } => {
            let (y, jf) = (fen / 100, fen % 100);
            let (jiao, f) = (jf / 10, jf % 10);
            let mut s = format!("{prefix}{}{yuan}", upper_int(y)?);
            if jf == 0 {
                if *zheng {
                    s.push('整');
                }
            } else {
                if jiao > 0 {
                    s.push(UPPER_DIGITS[jiao as usize]);
                    s.push('角');
                } else {
                    s.push('零');
                }
                if f > 0 {
                    s.push(UPPER_DIGITS[f as usize]);
                    s.push('分');
                }
            }
            Some(s)
        }
    }
}

// ---------------------------------------------------------------------------
// 单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn book(k: f64, d: i64) -> ShiftBook {
        ShiftBook { params: Some(ShiftParams::fixed(k, d)), ..Default::default() }
    }

    fn fen_of(s: &str) -> i128 {
        parse_amount(s, "", "").unwrap_or_else(|| panic!("解析失败: {s}")).fen
    }

    #[test]
    fn upper_roundtrip() {
        for n in [1, 10, 100, 105, 1000, 1005, 10_500, 100_000, 100_005_000, 123_456_789, 1_000_000_000_000] {
            let u = upper_int(n).unwrap();
            assert_eq!(parse_upper_int(&u), Some(n), "{n} -> {u}");
        }
        assert_eq!(upper_int(10_500).unwrap(), "壹万零伍佰");
        assert_eq!(upper_int(100_005_000).unwrap(), "壹亿零伍仟");
        assert_eq!(upper_int(1_234_567).unwrap(), "壹佰贰拾叁万肆仟伍佰陆拾柒");
        assert_eq!(parse_upper_int("拾万"), Some(100_000));
    }

    #[test]
    fn upper_and_lower_amount_stay_consistent() {
        let mut b = book(1.37, 49);
        let up = b.scale_amount("人民币壹佰贰拾叁万肆仟伍佰陆拾柒元整", "合同价款为", "（").unwrap();
        let lo = b.scale_amount("¥1,234,567.00", "（小写：", "）").unwrap();
        assert_eq!(fen_of(&up), fen_of(&lo), "{up} vs {lo}");
        // 大写先出现（整元精度），小写沿用同一新值
        assert!((fen_of(&lo) - (123_456_700f64 * 1.37).round() as i128).abs() <= 50, "{lo}");
        assert!(up.starts_with("人民币") && up.ends_with("元整"), "{up}");
        assert!(lo.starts_with('¥') && lo.contains(','), "{lo}");
        // 先出现小写、后出现大写，结果同样一致
        let mut b2 = book(0.8, 49);
        let lo2 = b2.scale_amount("1,000,000元", "", "").unwrap();
        let up2 = b2.scale_amount("壹佰万元整", "", "").unwrap();
        assert_eq!(fen_of(&lo2), fen_of(&up2));
        assert_eq!(lo2, "800,000元");
        assert_eq!(up2, "捌拾万元整");
    }

    #[test]
    fn upper_with_jiao_fen() {
        let mut b = book(1.5, 49);
        let up = b.scale_amount("人民币壹仟元伍角陆分", "", "").unwrap();
        assert_eq!(fen_of(&up), (100_056f64 * 1.5).round() as i128); // 150084 分
        assert_eq!(up, "人民币壹仟伍佰元捌角肆分");
    }

    #[test]
    fn wan_and_yi_units() {
        let mut b = book(1.25, 49);
        assert_eq!(b.scale_amount("12.5万元", "", "").unwrap(), "15.6万元");
        assert_eq!(b.scale_amount("300万元", "", "").unwrap(), "375万元");
        assert_eq!(b.scale_amount("3.2亿元", "", "").unwrap(), "4.0亿元");
        assert_eq!(b.scale_amount("480,000.00元", "", "").unwrap(), "600,000.00元");
        assert_eq!(b.scale_amount("¥ 5000", "", "").unwrap(), "¥ 6250");
    }

    #[test]
    fn same_value_same_result_across_units() {
        let mut b = book(1.3, 49);
        let a = b.scale_amount("100万元", "", "").unwrap();
        let c = b.scale_amount("1,000,000.00元", "", "").unwrap();
        assert_eq!(fen_of(&a), fen_of(&c));
    }

    #[test]
    fn excluded_percentages_rates_and_article_numbers() {
        let mut b = book(1.3, 49);
        assert_eq!(b.scale_amount("1,000", "年利率", "%"), None);
        assert_eq!(b.scale_amount("1,200", "依据第", "条约定"), None);
        assert_eq!(b.scale_amount("1,200", "违约金费率为", "，"), None);
        assert_eq!(b.scale_amount("5,000", "共计", "股"), None);
        // “第X条”与普通金额同在一句
        assert!(b.scale_amount("5,000元", "违约金", "。").is_some());
        // 带货币标记时即使后跟“号”等字也照常缩放
        assert!(b.scale_amount("5000元", "付", "款").is_some());
    }

    #[test]
    fn currencies_get_independent_k() {
        let mut b = ShiftBook { params: Some(ShiftParams::from_seed(42, true)), ..Default::default() };
        let cny = b.scale_amount("100,000元", "", "").unwrap();
        let usd = b.scale_amount("100,000", "US$", "").unwrap();
        let p = b.params.as_ref().unwrap();
        assert!(p.k.contains_key("CNY") && p.k.contains_key("USD"));
        assert_ne!(fen_of(&cny), fen_of(&usd));
    }

    #[test]
    fn k_always_in_allowed_ranges() {
        for seed in 0..2000u64 {
            let p = ShiftParams::from_seed(seed, true);
            let k = p.k["CNY"];
            assert!((0.62..=0.93).contains(&k) || (1.07..=1.6).contains(&k), "k={k}");
            assert!((35..=364).contains(&p.delta_days.abs()), "Δ={}", p.delta_days);
            let k2 = p.k_peek("USD");
            assert!((0.62..=0.93).contains(&k2) || (1.07..=1.6).contains(&k2));
        }
        let p = ShiftParams::from_seed(7, false);
        assert!((30..=365).contains(&p.delta_days.abs()));
    }

    #[test]
    fn params_are_random_not_derived_from_text() {
        let a = ShiftParams::random();
        let b = ShiftParams::random();
        assert_ne!(a.seed, b.seed);
        // 同一原文在不同案件（不同参数）里得到不同结果
        let mut x = ShiftBook { params: Some(a), ..Default::default() };
        let mut y = ShiftBook { params: Some(b), ..Default::default() };
        let (rx, ry) = (x.scale_amount("123,456元", "", ""), y.scale_amount("123,456元", "", ""));
        assert_ne!(rx, ry);
    }

    #[test]
    fn dates_cross_month_and_year() {
        let mut b = book(1.3, 45);
        assert_eq!(b.shift_date("2024-12-20", "", "").unwrap(), "2025-02-03");
        assert_eq!(b.shift_date("2024年12月20日", "", "").unwrap(), "2025年2月3日");
        assert_eq!(b.shift_date("2024年 3 月 5 日", "", "").unwrap(), "2024年 4 月 19 日");
        assert_eq!(b.shift_date("2024-1-5", "", "").unwrap(), "2024-2-19");
        let mut n = book(1.3, -365);
        // 闰日
        assert_eq!(n.shift_date("2025-02-28", "", "").unwrap(), "2024-02-29");
        let mut f = book(1.3, 365);
        assert_eq!(f.shift_date("2024-02-29", "", "").unwrap(), "2025-02-28");
        // 非法日期不动
        assert_eq!(b.shift_date("2023-02-30", "", ""), None);
    }

    #[test]
    fn intervals_between_dates_preserved() {
        let mut b = ShiftBook { params: Some(ShiftParams::from_seed(99, true)), ..Default::default() };
        let d1 = b.shift_date("2023-11-30", "", "").unwrap();
        let d2 = b.shift_date("2024-03-01", "", "").unwrap();
        let days = |s: &str| {
            let (y, m, d) = (num(&s[0..4]).unwrap(), num(&s[5..7]).unwrap(), num(&s[8..10]).unwrap());
            days_from_civil(y, m, d)
        };
        assert_eq!(days(&d2) - days(&d1), days("2024-03-01") - days("2023-11-30"));
    }

    #[test]
    fn year_month_shift_by_months() {
        assert_eq!(shift_date_text("2024年11月", 0, 3).unwrap(), "2025年2月");
        assert_eq!(shift_date_text("2024年2月", 0, -3).unwrap(), "2023年11月");
    }

    #[test]
    fn weekday_preserved() {
        for seed in 0..300u64 {
            let mut b = ShiftBook { params: Some(ShiftParams::from_seed(seed, true)), ..Default::default() };
            let out = b.shift_date("2024-05-17", "", "").unwrap();
            let (y, m, d) = (num(&out[0..4]).unwrap(), num(&out[5..7]).unwrap(), num(&out[8..10]).unwrap());
            assert_eq!(weekday(y, m, d), weekday(2024, 5, 17), "seed {seed}: {out}");
        }
        assert_eq!(weekday(2024, 5, 17), 5); // 周五
    }

    #[test]
    fn law_dates_not_shifted_but_contract_dates_are() {
        let mut b = book(1.3, 49);
        assert_eq!(b.shift_date("2021年1月1日", "《中华人民共和国民法典》自", "起施行。"), None);
        assert_eq!(b.shift_date("2020年5月28日", "", "第十三届全国人民代表大会第三次会议通过"), None);
        assert_eq!(b.shift_date("2023年12月29日", "《公司法》（", "修订）"), None);
        assert!(b.shift_date("2021年1月1日", "本合同自", "起生效。").is_some());
        // 董事会“通过”不是立法线索，照常平移
        assert!(b.shift_date("2024年3月1日", "公司董事会于", "通过了决议").is_some());
        // 分句隔离：前一句的“施行”不影响后一句
        assert!(b.shift_date("2024年3月1日", "该办法已施行。双方约定自", "起履行").is_some());
    }

    #[test]
    fn exact_restore_roundtrip_and_boundary() {
        let mut b = book(1.3, 49);
        let src = "价款100万元，另付15万元；2024-03-01签订。";
        let a1 = b.scale_amount("100万元", "价款", "，").unwrap();
        let a2 = b.scale_amount("15万元", "另付", "；").unwrap();
        let d = b.shift_date("2024-03-01", "；", "签订。").unwrap();
        let masked = src.replace("100万元", &a1).replace("15万元", &a2).replace("2024-03-01", &d);
        assert!(!masked.contains("100万") && !masked.contains("2024-03-01"), "{masked}");
        assert_eq!(b.restore_exact(&masked), src);
        // “5万元”不能误命中“15万元”内部
        b.entries.insert("1万元".into(), "5万元".into());
        assert_eq!(b.restore_exact("15万元"), "15万元");
        // 序列化往返
        let j = serde_json::to_string(&b).unwrap();
        let b2: ShiftBook = serde_json::from_str(&j).unwrap();
        assert_eq!(b2, b);
    }

    #[test]
    fn suggest_unshift_for_new_numbers() {
        let mut b = book(1.25, 49);
        let a = b.scale_amount("100万元", "", "").unwrap(); // 125万元
        assert_eq!(a, "125万元");
        let reply = format!("已知 {a}；两期合计 250万元，并于 2025-03-01 前支付。");
        let s = b.suggest_unshift(&reply);
        // 已在对照表里的 125万元 不再建议
        assert!(s.iter().all(|x| x.text != a));
        let amt = s.iter().find(|x| x.kind == "amount").unwrap();
        assert_eq!(amt.text, "250万元");
        assert_eq!(amt.suggested, "200万元");
        let dt = s.iter().find(|x| x.kind == "date").unwrap();
        assert_eq!(dt.suggested, "2025-01-11"); // 2025-03-01 − 49 天
        assert_eq!(&reply[amt.start..amt.end], "250万元");
    }

    #[test]
    fn tiny_values_still_change() {
        let mut b = book(1.07, 49);
        let r = b.scale_amount("1元", "", "").unwrap();
        assert_ne!(r, "1元");
    }
}
