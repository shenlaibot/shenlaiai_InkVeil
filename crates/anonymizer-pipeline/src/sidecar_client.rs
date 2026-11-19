//! Python sidecar 的 HTTP 客户端（reqwest::blocking）与响应模型。
//!
//! 协议与 `sidecar/app.py`、`sidecar/parsers.py`、`sidecar/ner.py` 保持一致：
//! - `/health`   GET  → `{"status":"ok","version":...}`
//! - `/parse`    POST → `{"ok":true,"doc_id","format","text","spans","structure"}`
//! - `/redact`   POST → `{"ok":true,"files":[{"output_path","is_deep","replaced_count","warnings"}]}`
//! - `/ner`      POST → `{"ok":true,"entities":[...],"model_loaded":bool,"error":null}`
//!
//! 偏移约定：spans 的 `span_start/span_end` 与 NER 的 `start/end` 均为基于全文的
//! **字符偏移**（Unicode 码点数，与 sidecar Python `len(str)` 语义一致；注意
//! 不是 UTF-8 字节偏移，中文每字 3 字节）。Rust 侧正则/Aho-Corasick 输出字节
//! 偏移，命令层与 pipeline 通过 `byte_to_char` 统一换算后再与本客户端交互。
//! PDF span 的 `bbox` 可为 `null`。

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 一次替换指令（与 sidecar Replacement 协议一致）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replacement {
    /// 全文字符起始偏移（含）
    pub start: usize,
    /// 全文字符结束偏移（不含）
    pub end: usize,
    /// 替换文本（掩码/占位符/假数据等）
    pub replacement: String,
}

/// `/parse` 响应。
#[derive(Debug, Clone, Deserialize)]
pub struct ParseResponse {
    pub ok: bool,
    #[serde(default)]
    pub doc_id: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub spans: Vec<Span>,
    /// 页面/块级结构（PDF/DOCX 有内容，TXT/MD 为简单 dict）
    #[serde(default)]
    pub structure: serde_json::Value,
}

/// span 流（基于全文字符偏移）。
#[derive(Debug, Clone, Deserialize)]
pub struct Span {
    #[serde(default)]
    pub doc_id: String,
    #[serde(default)]
    pub page: u32,
    #[serde(default)]
    pub block: String,
    #[serde(default)]
    pub line: i64,
    pub span_start: usize,
    pub span_end: usize,
    #[serde(default)]
    pub text: String,
    /// PDF 命中区域 [x0,y0,x1,y1]（左下原点）；其余格式为 null
    pub bbox: Option<Vec<f64>>,
}

/// `/redact` 响应中的单个文件结果。
#[derive(Debug, Clone, Deserialize)]
pub struct RedactResponse {
    pub output_path: String,
    pub is_deep: bool,
    pub replaced_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// 未能映射到文档内的替换数（原文仍保留在输出文件中），0 = 全部落位
    #[serde(default)]
    pub unplaced_count: usize,
    /// 导出后复检发现的残留（只含位置/长度，不含原文）；空 = 未发现
    #[serde(default)]
    pub residual: Vec<ResidualItem>,
    /// 复检是否真正运行成功（false 时 `residual` 为空也不代表安全）
    #[serde(default)]
    pub verified: bool,
    /// 本工具未处理/无法检测的部位（图片、扫描页、嵌入对象、透视表…）；只含部位与类型，不含原文
    #[serde(default)]
    pub uncovered: Vec<UncoveredItem>,
    #[serde(default)]
    pub input_sha256: String,
    #[serde(default)]
    pub output_sha256: String,
    #[serde(default)]
    pub sidecar_version: String,
}

/// 未覆盖部位：`where` 为部位说明，`kind` 为类型（scanned_page/image/embedded/pivot/…）。
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct UncoveredItem {
    #[serde(rename = "where")]
    pub where_: String,
    pub kind: String,
    #[serde(default)]
    pub count: usize,
}

/// 复检发现的单处残留。`index` 是调用方传入 `verify_needles` 的下标。
#[derive(Debug, Clone, Deserialize)]
pub struct ResidualItem {
    pub index: usize,
    pub length: usize,
    #[serde(rename = "where")]
    pub where_: String,
    pub count: usize,
}

/// `/redact` 响应外壳：`{"ok":true,"files":[...]}`。
#[derive(Debug, Clone, Deserialize)]
struct RedactEnvelope {
    #[allow(dead_code)]
    ok: bool,
    files: Vec<RedactResponse>,
}

/// NER 实体（与 sidecar ner.py predict 返回一致）。
#[derive(Debug, Clone, Deserialize)]
pub struct NerEntity {
    /// 统一标签：人名 / 机构 / 地址
    pub label: String,
    #[serde(default)]
    pub model_label: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub confidence: f64,
}

/// `/ner` 响应。
#[derive(Debug, Clone, Deserialize)]
pub struct NerResponse {
    pub ok: bool,
    #[serde(default)]
    pub entities: Vec<NerEntity>,
    #[serde(default)]
    pub model_loaded: bool,
    #[serde(default)]
    pub error: Option<String>,
}

/// sidecar 通信错误。
#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("HTTP 请求失败: {0}")]
    Http(#[from] reqwest::Error),
    #[error("响应解析失败: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("服务端错误 {status}: {body}")]
    Server { status: u16, body: String },
    #[error("/redact 响应缺少 files 结果")]
    MissingFileResult,
}

/// Python sidecar HTTP 客户端（blocking）。
#[derive(Debug, Clone)]
pub struct SidecarClient {
    base_url: String,
    /// 认证 token（Some 时所有请求携带 `Authorization: Bearer <token>`）
    token: Option<String>,
    client: reqwest::blocking::Client,
}

impl SidecarClient {
    /// base_url 示例：`http://127.0.0.1:54321`
    ///
    /// 隐私约束：base_url 必须指向本机回环（127.0.0.1 / localhost / ::1），
    /// 处理路径绝不与外部地址通信；违反视为程序不变量被破坏（panic）。
    ///
    /// 无 token 构造（测试/直接调 sidecar 的脚本用）。生产链路请用
    /// [`Self::with_token`]，携带 Rust 主进程注入的认证 token。
    pub fn new(base_url: String) -> Self {
        Self::with_token(base_url, None)
    }

    /// 携带认证 token 构造（Rust 主进程注入，防本机其它进程冒名调用）。
    pub fn with_token(base_url: String, token: Option<String>) -> Self {
        assert!(
            is_loopback_base_url(&base_url),
            "sidecar base_url 必须是本机回环地址（127.0.0.1/localhost/::1），收到: {base_url}"
        );
        let client = reqwest::blocking::Client::builder()
            // 本地大文件解析 / NER 首次加载可能较慢，放宽整体超时
            .timeout(Duration::from_secs(300))
            .build()
            .expect("blocking client 构造失败");
        Self {
            base_url,
            token,
            client,
        }
    }

    /// `/health` 就绪探测（短超时，返回是否健康）。
    pub fn health(&self) -> bool {
        let mut req = self
            .client
            .get(format!("{}/health", self.base_url))
            .timeout(Duration::from_millis(800));
        if let Some(t) = &self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        req.send()
            .map(|resp| resp.status().is_success())
            .unwrap_or(false)
    }

    /// `/parse`：解析文档 → 全文 + span 流 + 结构。
    pub fn parse(&self, path: &Path) -> Result<ParseResponse, SidecarError> {
        let body = serde_json::json!({ "path": path.display().to_string(), "doc_id": null });
        self.post_json("/parse", body)
    }

    /// `/redact`：按全文字符偏移写回脱敏副本到 output_dir。
    pub fn redact(
        &self,
        input_path: &Path,
        output_dir: &Path,
        replacements: Vec<Replacement>,
    ) -> Result<RedactResponse, SidecarError> {
        self.redact_verified(input_path, output_dir, replacements, &[])
    }

    /// 同 [`Self::redact`]，并让 sidecar 在导出后用 `verify_needles` 复检输出文件。
    pub fn redact_verified(
        &self,
        input_path: &Path,
        output_dir: &Path,
        replacements: Vec<Replacement>,
        verify_needles: &[String],
    ) -> Result<RedactResponse, SidecarError> {
        let body = serde_json::json!({
            "input_path": input_path.display().to_string(),
            "output_dir": output_dir.display().to_string(),
            "replacements": replacements,
            "verify_needles": verify_needles,
        });
        let envelope: RedactEnvelope = self.post_json("/redact", body)?;
        envelope
            .files
            .into_iter()
            .next()
            .ok_or(SidecarError::MissingFileResult)
    }

    /// `/ner`：中文 NER（人名/机构/地址），threshold 为置信度阈值，model_path 缺省用默认模型。
    pub fn ner(
        &self,
        text: &str,
        threshold: f64,
        model_path: Option<&str>,
    ) -> Result<NerResponse, SidecarError> {
        let body = serde_json::json!({
            "text": text,
            "threshold": threshold,
            "model_path": model_path,
        });
        self.post_json("/ner", body)
    }

    /// `/restore`：docx/xlsx 文档还原（映射经请求体传入内存，不落盘），返回原始 JSON。
    pub fn restore_doc(
        &self,
        input_path: &Path,
        output_dir: &Path,
        mapping: &std::collections::HashMap<String, String>,
    ) -> Result<serde_json::Value, SidecarError> {
        let body = serde_json::json!({
            "input_path": input_path.display().to_string(),
            "output_dir": output_dir.display().to_string(),
            "mapping": mapping,
        });
        self.post_json("/restore", body)
    }

    fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        body: serde_json::Value,
    ) -> Result<T, SidecarError> {
        let mut req = self
            .client
            .post(format!("{}{}", self.base_url, endpoint))
            .json(&body);
        if let Some(t) = &self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let resp = req.send()?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().unwrap_or_default();
            return Err(SidecarError::Server {
                status: status.as_u16(),
                body: body_text,
            });
        }
        Ok(resp.json::<T>()?)
    }
}

/// 校验 base_url 的主机是否为本机回环地址（127.0.0.1 / localhost / ::1）。
///
/// 隐私约束：处理路径只允许与本地 sidecar 通信，杜绝任何外部地址。
fn is_loopback_base_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"));
    match rest {
        Some(r) => {
            // 去掉路径与端口，取主机部分（支持 [::1]:8080 形式）
            let host_port = r.split('/').next().unwrap_or("");
            let host = host_port
                .rsplit_once(':')
                .map(|(h, _)| h)
                .unwrap_or(host_port);
            let host = host.trim_start_matches('[').trim_end_matches(']');
            matches!(host, "127.0.0.1" | "localhost" | "::1")
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_loopback_accepts_local_hosts() {
        for url in [
            "http://127.0.0.1:54321",
            "http://localhost:1234",
            "http://127.0.0.1",
            "http://[::1]:8080",
            "http://localhost",
        ] {
            assert!(is_loopback_base_url(url), "应接受回环地址: {url}");
        }
    }

    #[test]
    fn is_loopback_rejects_external_hosts() {
        for url in [
            "http://example.com:8080",
            "https://github.com",
            "http://10.0.0.1:1234",
            "http://192.168.1.5",
            "http://[::ffff:10.0.0.1]",
            "ftp://127.0.0.1:21",
            "not a url",
        ] {
            assert!(!is_loopback_base_url(url), "应拒绝外部地址: {url}");
        }
    }

    #[test]
    fn new_accepts_loopback() {
        // 构造客户端不触发网络（reqwest blocking client 惰性建连）。
        let _ = SidecarClient::new("http://127.0.0.1:54321".to_string());
    }

    #[test]
    #[should_panic(expected = "回环")]
    fn new_panics_on_external_host() {
        let _ = SidecarClient::new("http://example.com:8080".to_string());
    }
}
