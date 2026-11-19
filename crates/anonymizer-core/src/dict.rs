//! 词典匹配：AC 自动机多模式精确匹配。

use crate::{Hit, HitSource};
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};

pub struct DictMatcher {
    label: String,
    ac: AhoCorasick,
    terms: Vec<String>,
}

impl DictMatcher {
    pub fn new(label: impl Into<String>, terms: Vec<String>) -> Self {
        // LeftmostLongest：重叠时取最长（全称优先于简称子串）
        let ac = AhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostLongest)
            .build(&terms)
            .expect("空词典模式不可编译");
        Self {
            label: label.into(),
            ac,
            terms,
        }
    }

    /// 从 JSON 数组字符串加载词典：`["北京降本增笑有限公司", "降本增笑"]`
    pub fn from_json_str(label: impl Into<String>, json: &str) -> Result<Self, String> {
        let terms: Vec<String> =
            serde_json::from_str(json).map_err(|e| format!("词典 JSON 解析失败: {e}"))?;
        Ok(Self::new(label, terms))
    }

    /// 从 CSV（每行一个词）加载
    pub fn from_csv_str(label: impl Into<String>, csv: &str) -> Self {
        let terms: Vec<String> = csv
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Self::new(label, terms)
    }

    pub fn term_count(&self) -> usize {
        self.terms.len()
    }

    /// 返回词典命中（source=Dictionary）
    pub fn match_all(&self, text: &str) -> Vec<Hit> {
        self.ac
            .find_iter(text)
            .map(|m| Hit {
                rule_id: "dict".into(),
                rule_label: self.label.clone(),
                start: m.start(),
                end: m.end(),
                text: text[m.start()..m.end()].to_string(),
                confidence: 1.0,
                source: HitSource::Dictionary,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac_multi_match() {
        let d = DictMatcher::new(
            "公司",
            vec!["北京降本增笑有限公司".into(), "降本增笑".into()],
        );
        let text = r#"甲方：北京降本增笑有限公司（以下简称“降本增笑”）"#;
        let hits = d.match_all(text);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().any(|h| h.text == "北京降本增笑有限公司"));
        assert!(hits.iter().any(|h| h.text == "降本增笑"));
        assert!(hits.iter().all(|h| h.source == HitSource::Dictionary));
    }

    #[test]
    fn load_from_json() {
        let d = DictMatcher::from_json_str("公司", r#"["张三","李四"]"#).unwrap();
        assert_eq!(d.term_count(), 2);
    }
}
