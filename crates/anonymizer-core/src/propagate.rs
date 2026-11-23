//! 实体回扫：把已确认的实体文本传播到全文其余出现位置，并把“简称”纳入同一实体。
//!
//! 为什么需要：规则/NER 往往只在“有上下文前缀”的那一次出现命中（如「联系人：陈瑾瑜」），
//! 后文无前缀的「陈瑾瑜负责对接」「云图公司应当付款」不会被命中，等于漏脱敏。
//!
//! 约束（避免传播放大误报）：
//! - 传播姓名、公司名称、开户银行、机构、地址和依赖上下文识别的合同/业务编号；
//! - 只以可靠来源的命中作种子：置信度 ≥ [`SEED_MIN_CONFIDENCE`]，且来源不是“姓名启发式”
//!   （启发式本身已在全文扫描）；
//! - 种子文本至少 2 个字符；
//! - 传播得到的命中不得与任何已有命中重叠（不拆开更长的命中）。
//!
//! 偏移约定：`Hit.start/end` 为**字符偏移**（与应用层一致）。

use std::collections::HashSet;

use crate::rules::ID_CONTRACT_NO;
use crate::{Hit, HitSource};

/// 可传播的实体类型。
const PROPAGATE_TYPES: [&str; 6] = [
    "name", "company_name", "bank_name", "org", "address", ID_CONTRACT_NO,
];
/// 作为传播种子的最低置信度。
pub const SEED_MIN_CONFIDENCE: f64 = 0.6;
/// 传播命中的置信度（略低于种子，交由用户在预览页裁决）。
const PROPAGATED_CONFIDENCE: f64 = 0.7;

/// 传播入口。`aliases` 为 (全称, 简称)：全称若作为种子，则简称也作为同类型种子。
pub fn propagate(text: &str, hits: Vec<Hit>, aliases: &[(String, String)]) -> Vec<Hit> {
    // (文本, rule_id, rule_label)
    let mut seeds: Vec<(String, String, String)> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for h in &hits {
        if !PROPAGATE_TYPES.contains(&h.rule_id.as_str())
            || h.confidence < SEED_MIN_CONFIDENCE
            || matches!(h.source, HitSource::Name)
            || h.text.chars().count() < 2
        {
            continue;
        }
        if seen.insert((h.text.clone(), h.rule_id.clone())) {
            seeds.push((h.text.clone(), h.rule_id.clone(), h.rule_label.clone()));
        }
    }
    // 简称联动：全称是已有的公司类命中 → 简称同类型作为种子
    for (full, short) in aliases {
        if short.chars().count() < 2 {
            continue;
        }
        if let Some(h) = hits.iter().find(|h| &h.text == full) {
            if seen.insert((short.clone(), h.rule_id.clone())) {
                seeds.push((short.clone(), h.rule_id.clone(), h.rule_label.clone()));
            }
        }
    }
    if seeds.is_empty() {
        return hits;
    }

    // 字符偏移 -> 字节偏移 表（仅一次 O(n)）
    let char_starts: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
    let n_chars = char_starts.len();
    let byte_to_char = |b: usize| -> usize { char_starts.partition_point(|&x| x < b).min(n_chars) };

    // 现有命中（字符区间）按 start 排序，用于重叠判断
    let mut occupied: Vec<(usize, usize)> = hits.iter().map(|h| (h.start, h.end)).collect();
    occupied.sort_unstable();
    let overlaps = |s: usize, e: usize, occ: &[(usize, usize)]| -> bool {
        // 找到第一个 end > s 的区间；因区间互不重叠且按 start 排序，检查它与相邻即可
        let i = occ.partition_point(|&(_, oe)| oe <= s);
        occ.get(i).is_some_and(|&(os, _)| os < e)
    };

    // 更长的种子优先，避免短种子占用长种子的子串
    seeds.sort_by_key(|(t, _, _)| std::cmp::Reverse(t.chars().count()));
    let mut out = hits;
    let mut added: Vec<Hit> = Vec::new();
    for (seed, rule_id, rule_label) in &seeds {
        for (b, _) in text.match_indices(seed.as_str()) {
            // 编号须完整匹配，避免把另一个更长编号中的子串替换掉。
            let is_number_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/');
            if rule_id == ID_CONTRACT_NO
                && (text[..b].ends_with(is_number_char)
                    || text[b + seed.len()..].starts_with(is_number_char))
            {
                continue;
            }
            let cs = byte_to_char(b);
            let ce = cs + seed.chars().count();
            if overlaps(cs, ce, &occupied) {
                continue;
            }
            added.push(Hit {
                rule_id: rule_id.clone(),
                rule_label: rule_label.clone(),
                start: cs,
                end: ce,
                text: seed.clone(),
                confidence: PROPAGATED_CONFIDENCE,
                source: HitSource::Context,
            });
            // 新命中也要占位，防止后续（较短的）种子重复覆盖
            let pos = occupied.partition_point(|&(os, _)| os < cs);
            occupied.insert(pos, (cs, ce));
        }
    }
    out.extend(added);
    out.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: &str, label: &str, s: usize, e: usize, t: &str, conf: f64, src: HitSource) -> Hit {
        Hit { rule_id: id.into(), rule_label: label.into(), start: s, end: e, text: t.into(), confidence: conf, source: src }
    }

    #[test]
    fn rare_name_propagates_to_all_occurrences() {
        let text = "联系人：陈瑾瑜。经协商，陈瑾瑜负责对接，陈瑾瑜应于三日内回复。";
        let seeds = vec![hit("name", "姓名", 4, 7, "陈瑾瑜", 0.8, HitSource::Context)];
        let out = propagate(text, seeds, &[]);
        let names: Vec<_> = out.iter().filter(|h| h.text == "陈瑾瑜").collect();
        assert_eq!(names.len(), 3, "{out:?}");
        // 字符偏移与文本一致
        let chars: Vec<char> = text.chars().collect();
        for h in &names {
            assert_eq!(chars[h.start..h.end].iter().collect::<String>(), "陈瑾瑜");
        }
    }

    #[test]
    fn alias_short_name_joins_entity() {
        let text = "甲方：北京云图科技有限公司（以下简称“云图公司”）。云图公司应付款。";
        let full = "北京云图科技有限公司";
        let s = text.chars().position(|c| c == '北').unwrap();
        let seeds = vec![hit("company_name", "公司名称", s, s + full.chars().count(), full, 0.9, HitSource::Rule)];
        let out = propagate(text, seeds, &[(full.into(), "云图公司".into())]);
        let shorts = out.iter().filter(|h| h.text == "云图公司").count();
        assert_eq!(shorts, 2, "定义处与正文处的简称都应命中: {out:?}");
    }

    #[test]
    fn does_not_split_longer_hits_nor_use_heuristic_seeds() {
        let text = "张三丰来了，张三也来了";
        // 启发式来源不作种子
        let seeds = vec![hit("name", "姓名", 0, 3, "张三丰", 0.7, HitSource::Name)];
        assert_eq!(propagate(text, seeds, &[]).len(), 1);
        // 可靠种子“张三”不得拆开已有的更长命中“张三丰”
        let seeds = vec![
            hit("name", "姓名", 0, 3, "张三丰", 0.9, HitSource::Context),
            hit("name", "姓名", 6, 8, "张三", 0.9, HitSource::Rule),
        ];
        let out = propagate(text, seeds, &[]);
        // “张三”只传播到独立出现处（6..8），不得拆开 0..3 的“张三丰”
        let shorts: Vec<_> = out.iter().filter(|h| h.text == "张三").collect();
        assert_eq!(shorts.len(), 1, "{out:?}");
        assert_eq!(shorts[0].start, 6);
    }

    #[test]
    fn numeric_types_are_not_propagated() {
        let text = "手机13800138000，再次13800138000";
        let seeds = vec![hit("mobile", "手机号", 2, 13, "13800138000", 0.9, HitSource::Rule)];
        assert_eq!(propagate(text, seeds, &[]).len(), 1);
    }

    #[test]
    fn contract_number_propagates_only_to_complete_matches() {
        let text = "编号：HT-1234。详见 HT-1234，另有 HT-1234-2、BHT-1234。";
        let seed = hit("contract_no", "合同/业务编号", 3, 10, "HT-1234", 0.9, HitSource::Rule);
        let hits = propagate(text, vec![seed], &[]);
        assert_eq!(hits.len(), 2);
        let chars: Vec<_> = text.chars().collect();
        assert_eq!(chars[hits[1].start..hits[1].end].iter().collect::<String>(), "HT-1234");
        assert_eq!(chars[hits[1].end], '，');
    }
}
