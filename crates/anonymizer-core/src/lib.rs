//! 墨隐 InkVeil 检测 / 脱敏核心库。
//!
//! 分层职责：
//! - [`rules`] 内置规则定义（正则 + 校验器）
//! - [`detect`] 检测器（正则 + 上下文置信度提升）
//! - [`dict`] 词典匹配（AC 自动机）
//! - [`merge`] 跨通道命中统一裁决（优先级 + 贪心去重叠）
//! - [`transform`] 脱敏变换（掩码 / 占位符 / 假数据 / 加密）
//! - [`mapping`] 映射表（同原文同占位符、可持久化、可还原）
//! - [`crypto`] AES-256-GCM / BASE64 可逆加密
//! - [`shift`] 保结构替换（金额缩放 / 日期平移）
//! - [`alias`] 全称/简称关联实体识别

pub mod alias;
pub mod crypto;
pub mod detect;
pub mod dict;
pub mod merge;
pub mod mapping;
pub mod name;
pub mod propagate;
pub mod rules;
pub mod shift;
pub mod transform;
pub mod vault;

use serde::{Deserialize, Serialize};

/// 命中来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitSource {
    Rule,
    Dictionary,
    Context,
    Ner,
    Name,
}

/// 一次检测命中。
///
/// 偏移约定：本结构体内为 **UTF-8 字节偏移**（regex / Aho-Corasick 的原生输出）；
/// 命令层与 pipeline 在与 sidecar（Python 字符索引）交互前统一换算为字符偏移。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    pub rule_id: String,
    pub rule_label: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub confidence: f64,
    pub source: HitSource,
}

/// 规则定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleDef {
    pub id: String,
    pub label: String,
    pub regex: String,
    pub enabled: bool,
    pub confidence: f64,
    pub context_keywords: Vec<String>,
}
