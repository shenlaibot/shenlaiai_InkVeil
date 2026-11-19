//! 全称/简称关联实体识别：解析"（以下简称"XXX"）"模式并建立绑定。

use crate::mapping::MappingTable;
use regex::Regex;

/// 在文本中找出 (全称)（以下简称"简称"）对。
pub fn find_alias_pairs(text: &str) -> Vec<(String, String)> {
    // 兼容：以下简称"X" / 下称"X" / 简称 X（无引号）
    // 全称要求以组织后缀结尾（公司/银行/事务所/研究院/集团等），避免把
    // 「本合同由乙方签署（简称…）」这类非组织短语整体误当全称；
    // 后缀锚定同时让「本合同由北京XX有限公司（以下简称…）」仍能捕获完整公司名。
    let re = Regex::new(
        r#"([\p{Han}A-Za-z0-9]{2,30}?(?:股份有限公司|有限责任公司|集团有限公司|有限公司|合伙企业|事务所|研究院|银行|集团))[（(](?:以下简称|下称|简称)\s*["“「]?([^"”」)）\n]{1,20})["”」]?[)）]"#,
    )
    .expect("别名正则应可编译");
    re.captures_iter(text)
        .map(|c| (trim_company_prefix(&c[1]), c[2].trim().to_string()))
        .filter(|(full, short)| full != short && !full.is_empty() && !short.is_empty())
        .collect()
}

/// 去掉全称前面粘上的角色/套话（“本合同由北京XX有限公司” → “北京XX有限公司”）。
fn trim_company_prefix(full: &str) -> String {
    const PREFIXES: [&str; 8] = ["本合同由", "本协议由", "合同由", "协议由", "甲方", "乙方", "丙方", "丁方"];
    let mut s = full;
    loop {
        let mut changed = false;
        for p in PREFIXES {
            if let Some(rest) = s.strip_prefix(p) {
                // 剥离后仍要保留足够长的名称
                if rest.chars().count() >= 4 {
                    s = rest;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    s.to_string()
}

/// 将别名对绑定到映射表：确保全称与简称使用同一占位符。
/// 需要先为全称调用过 placeholder_for。
pub fn bind_aliases(mapping: &mut MappingTable, text: &str) -> Vec<(String, String)> {
    let pairs = find_alias_pairs(text);
    for (full, short) in &pairs {
        // 若全称尚未注册占位符，先注册
        let placeholder = mapping.placeholder_for("公司", full);
        mapping.bind_alias(short, &placeholder);
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::{bind_aliases, find_alias_pairs};
    use crate::mapping::MappingTable;

    #[test]
    fn extract_pairs_common_forms() {
        let text = r#"甲方：北京降本增笑有限公司（以下简称“降本增笑”）"#;
        let pairs = find_alias_pairs(text);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "北京降本增笑有限公司");
        assert_eq!(pairs[0].1, "降本增笑");
    }

    #[test]
    fn extract_pairs_under_form() {
        let text = r#"乙方 星火信息技术股份有限公司（下称“星火信息”）"#;
        let pairs = find_alias_pairs(text);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].1, "星火信息");
    }

    #[test]
    fn extract_pairs_suffix_anchored() {
        // 带前置杂质的公司名：仍应捕获完整公司名（以组织后缀结尾）
        let text = r#"本合同由北京降本增笑有限公司（以下简称“降本增笑”）签署。"#;
        let pairs = find_alias_pairs(text);
        assert_eq!(pairs.len(), 1);
        // 全称不得带上“本合同由”这类粘连前缀（否则还原时会多出这几个字）
        assert_eq!(pairs[0].0, "北京降本增笑有限公司");
        assert!(pairs[0].0.ends_with("有限公司"), "全称应以组织后缀结尾");

        // 非组织短语（无人名/公司后缀）不得被当作全称
        let text2 = r#"本合同由乙方签署（简称“乙方”），双方各执一份。"#;
        assert!(find_alias_pairs(text2).is_empty(), "非组织短语不应捕获");
    }

    #[test]
    fn bind_makes_full_and_short_same_placeholder() {
        let text = r#"北京降本增笑有限公司（以下简称“降本增笑”）"#;
        let mut mapping = MappingTable::new();
        bind_aliases(&mut mapping, text);
        // 全称与简称应指向同一占位符
        let p_full = mapping
            .placeholder_of("北京降本增笑有限公司")
            .map(|s| s.to_string());
        let p_short = mapping.placeholder_of("降本增笑").map(|s| s.to_string());
        assert!(p_full.is_some() && p_short.is_some());
        assert_eq!(p_full, p_short);
        // 全称出现两次时编号一致
        let second = mapping.placeholder_for("公司", "北京降本增笑有限公司");
        assert_eq!(second, p_full.unwrap());
    }
}
