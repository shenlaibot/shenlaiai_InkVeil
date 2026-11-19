//! 命中裁决：跨通道（规则/词典/NER/姓名启发式）命中的统一去重叠。
//!
//! 排序键：类型优先级（前高后低）-> 置信度降序 -> start 升序；
//! 贪心占领：与已占领区间 `[start, end)` 相交的命中丢弃。
//!
//! 本模块是全链路**唯一**的裁决实现：`Detector::detect`（规则+姓名启发式的
//! 内部去重叠）与 pipeline 的 `merge_hits`（规则+词典+NER 三通道合并）均
//! 委托此处，保证同一组重叠命中在任何调用点得到相同结果。

use crate::Hit;

/// 默认类型优先级（id 列表，前高后低）：
/// 身份证 > 银行卡 > 统一社会信用代码 > 手机号 > 邮箱 > 固定电话 > 车牌 > IP >
/// 金额 > 日期 > 词典 > 公司名称 > 开户银行 > 网址 > 人名 > 机构 > 地址
///
/// - 结构化 PII（前 10 项）格式已校验、置信度高，排在一切通道之前；
/// - 词典（用户显式配置的精确词条）排在上下文引导的模糊规则（公司/银行/
///   网址/人名/机构/地址）之前：用户点名要替换的词不应被自动规则反杀
///   （如词典里的公司全称不得被 company_name 正则命中顶掉，否则同一
///   字符串在同一文档里得到两种脱敏形态）；
/// - NER 产出（人名/机构/地址）排在模糊规则之后：模型猜测弱于正则与词典；
/// - 未列出的自定义规则 id 视为最低优先级。
pub fn default_type_priority() -> Vec<String> {
    [
        "id_card",
        "passport",
        "id_card_hmt",
        "bank_card",
        "bank_account",
        "credit_code",
        "case_no",
        "mobile",
        "email",
        "phone",
        "plate",
        "ip",
        "amount",
        "date",
        "contract_no",
        "dict",
        "company_name",
        "bank_name",
        "url",
        "name",
        "org",
        "address",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// 查类型优先级序号（越小越高）；未列入优先级表的 id 视为最低。
pub fn priority_of(id: &str, priority: &[String]) -> usize {
    priority
        .iter()
        .position(|p| p == id)
        .unwrap_or(usize::MAX)
}

/// 统一裁决：按「优先级 asc -> 置信度 desc -> start asc」排序后贪心占领。
///
/// 输入可为任意通道命中的合集（内部已丢弃空区间），输出按 `(start, end)`
/// 升序且互不重叠。
pub fn arbitrate(mut hits: Vec<Hit>, priority: &[String]) -> Vec<Hit> {
    hits.retain(|h| h.end > h.start); // 丢弃空区间

    // 优先级高在前，同优先级按置信度降序（重叠时先到先得即"取高置信度"）
    hits.sort_by(|a, b| {
        priority_of(&a.rule_id, priority)
            .cmp(&priority_of(&b.rule_id, priority))
            .then_with(|| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.start.cmp(&b.start))
    });

    // 贪心占领：与已占领区间重叠的命中丢弃
    let mut occupied: Vec<(usize, usize)> = Vec::new();
    let mut result: Vec<Hit> = Vec::new();
    for h in hits {
        let overlaps = occupied.iter().any(|(s, e)| h.start < *e && *s < h.end);
        if overlaps {
            continue;
        }
        occupied.push((h.start, h.end));
        result.push(h);
    }

    result.sort_by_key(|h| (h.start, h.end));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HitSource;

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
    fn higher_priority_wins_overlap() {
        // Luhn 失败的银行卡（置信度 0.75）与高置信度日期重叠：银行卡优先级更高应保留
        let hits = vec![hit("bank_card", 10, 28, 0.75), hit("date", 10, 20, 0.9)];
        let out = arbitrate(hits, &default_type_priority());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rule_id, "bank_card");
    }

    #[test]
    fn same_priority_higher_confidence_wins() {
        let hits = vec![hit("name", 0, 3, 0.7), hit("name", 0, 3, 0.9)];
        let out = arbitrate(hits, &default_type_priority());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].confidence, 0.9);
    }

    #[test]
    fn unknown_id_lowest_priority() {
        // 自定义规则 id 不在表内 -> 低于词典；不得反杀高置信结构化规则
        let hits = vec![hit("custom_rule", 0, 18, 0.99), hit("id_card", 0, 18, 0.5)];
        let out = arbitrate(hits, &default_type_priority());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rule_id, "id_card");
    }

    #[test]
    fn company_beats_name_inside_company() {
        // 公司名内部的形似人名子串不得反杀公司命中
        let hits = vec![hit("name", 2, 4, 0.9), hit("company_name", 0, 10, 0.72)];
        let out = arbitrate(hits, &default_type_priority());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rule_id, "company_name");
    }

    #[test]
    fn non_overlapping_all_kept_sorted() {
        let hits = vec![hit("name", 15, 17, 0.9), hit("mobile", 0, 11, 0.98)];
        let out = arbitrate(hits, &default_type_priority());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].rule_id, "mobile");
        assert_eq!(out[1].rule_id, "name");
    }
}
