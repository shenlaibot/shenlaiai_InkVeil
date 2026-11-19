//! 墨隐 InkVeil 本地文档脱敏处理管线。
//!
//! 端到端链路：文件 → sidecar 解析（/parse）→ 规则/词典/上下文检测 →
//! 别名绑定 →（可选）NER 兜底（/ner）→ 多通道命中合并 → 映射/变换 →
//! sidecar 写回（/redact）→ 映射表落盘（.anonymap）。
//!
//! 分层职责：
//! - [`sidecar_client`] Python sidecar 的 HTTP 客户端与响应模型（协议对齐 sidecar/app.py）
//! - [`merge`] 多通道（规则/词典/NER）命中合并：区间去重叠 + 类型优先级裁决
//! - [`pipeline`] 批量处理编排（run_batch）与映射/变换/写回

pub mod merge;
pub mod pipeline;
pub mod sidecar_client;

pub use merge::{default_type_priority, merge_hits};
pub use pipeline::{run_batch, BatchResult, FileResult, PipelineConfig};
pub use sidecar_client::{
    NerEntity, NerResponse, ParseResponse, RedactResponse, Replacement, SidecarClient,
    SidecarError, Span,
};
