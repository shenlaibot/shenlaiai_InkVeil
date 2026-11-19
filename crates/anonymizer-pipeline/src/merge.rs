//! 多通道命中合并：规则 / 词典 / NER 命中按区间去重叠。
//!
//! 裁决算法与默认优先级表已下沉到 `anonymizer-core::merge`（`Detector`
//! 内部与本文档的 `merge_hits` 委托同一 `arbitrate` 实现），保证同一组
//! 重叠命中在任何调用点得到相同结果；此处仅保留三通道入口与 re-export。

pub use anonymizer_core::merge::default_type_priority;

use anonymizer_core::merge::arbitrate;
use anonymizer_core::Hit;

/// 合并规则/词典/NER 命中。
///
/// - NER 命中请先按阈值过滤后传入（pipeline 中完成），本函数只做区间与优先级裁决；
/// - 重叠判定：区间 `[start, end)` 相交即重叠；重叠时保留优先级更高的命中，
///   同优先级保留置信度更高的命中；
/// - 输出按 `(start, end)` 升序，互不重叠，`source` 保留原通道来源。
pub fn merge_hits(
    rule_hits: Vec<Hit>,
    dict_hits: Vec<Hit>,
    ner_hits: Vec<Hit>,
    type_priority: &[String],
) -> Vec<Hit> {
    let all: Vec<Hit> = rule_hits
        .into_iter()
        .chain(dict_hits)
        .chain(ner_hits)
        .collect();
    arbitrate(all, type_priority)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anonymizer_core::HitSource;

    fn hit(id: &str, start: usize, end: usize, confidence: f64) -> Hit {
        Hit {
            rule_id: id.into(),
            rule_label: id.into(),
            start,
            end,
            text: String::new(),
            confidence,
            source: HitSource::Rule,
        }
    }

    #[test]
    fn rule_beats_dict_on_overlap() {
        // 词典词条与统一社会信用代码重叠：结构化规则必须赢（词典兜底最低）
        let rules = vec![hit("credit_code", 0, 18, 0.98)];
        let dict = vec![hit("dict", 0, 18, 1.0)];
        let out = merge_hits(rules, dict, vec![], &default_type_priority());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rule_id, "credit_code");
    }

    #[test]
    fn empty_ranges_dropped() {
        let out = merge_hits(
            vec![hit("mobile", 5, 5, 0.98)],
            vec![],
            vec![],
            &default_type_priority(),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn three_channels_non_overlap_all_kept() {
        let out = merge_hits(
            vec![hit("mobile", 0, 11, 0.98)],
            vec![hit("dict", 20, 24, 1.0)],
            vec![hit("name", 30, 33, 0.9)],
            &default_type_priority(),
        );
        assert_eq!(out.len(), 3);
    }
}
