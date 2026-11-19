//! 模型市场与下载管理器。
//!
//! 职责：
//! - 内置模型清单（manifest）：名称/版本/文件清单（大小 + SHA-256）/下载地址/许可证；
//! - 下载管理器：reqwest 流式下载到应用数据目录，断点续传（Range 头 + `.part` 文件）、
//!   SHA-256 完整性校验、下载中/已暂停/完成/失败状态、暂停/继续/删除。
//!
//! 下载状态机（对应 UI 模型市场页）：
//! - `start`  → 新建下载任务；跳过已完整安装的文件，`.part` 断点续传
//! - `pause`  → 置中止标志，任务在当前分块处停止并保留 `.part`
//! - `start`（再次）→ 从 `.part` 续传
//! - `delete` → 删除安装目录与状态（进行中的任务需先暂停）
//!
//! 模型安装目录：`<应用数据目录>/models/<model_id>/`，文件布局与 sidecar `ner.py`
//! 期望一致（`model.onnx` + `tokenizer.json` + `config.json` 等）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use reqwest::header::RANGE;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Manager;
use tokio::io::AsyncWriteExt;

/// 单文件下载读取缓冲（也用于哈希分块）。
const CHUNK_SIZE: usize = 64 * 1024;
/// 单次请求（连接 + 响应头）的等待上限。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// 下载过程中两次收到数据之间的最长间隔：超过即判定“卡死”，换一次重试。
/// 不再给整个文件设总超时——400MB 的文件在慢速网络上本来就可能超过 10 分钟，
/// 只要数据仍在流动就应该继续（此前 600 秒总超时会让慢网络下必然失败）。
const STALL_TIMEOUT: Duration = Duration::from_secs(45);
/// 每个下载源对同一文件的最大尝试次数（含首次）。
const MAX_ATTEMPTS_PER_SOURCE: u32 = 3;
/// 重试退避基数（第 n 次失败后等待 base × 2^(n-1)）。
const BACKOFF_BASE: Duration = Duration::from_secs(2);

/// 默认中文 NER 模型的 id（安装目录名）。
pub const NER_MODEL_ID: &str = "ner-zh";

/// 上游模型仓库（HuggingFace，固定到具体 commit，保证文件内容不随 main 变动）。
const NER_REPO_PATH: &str =
    "protectai/gyr66-bert-base-chinese-finetuned-ner-onnx/resolve/5fb2d9494046e1fd2d9d62d1366074a60efae0c1";

/// 主下载源：HuggingFace 官方。
pub const NER_PRIMARY_BASE_URL: &str =
    "https://huggingface.co/protectai/gyr66-bert-base-chinese-finetuned-ner-onnx/resolve/5fb2d9494046e1fd2d9d62d1366074a60efae0c1";

/// 备用下载源（按顺序尝试）：国内可访问的 HuggingFace 镜像。
/// 各源提供**同一 commit 的同一批文件**，完整性始终由清单中的 SHA-256 校验兜底，
/// 因此镜像不需要被信任。
const NER_MIRRORS: [&str; 1] = ["https://hf-mirror.com"];

/// 开发/CI 期 base_url 覆盖：设置 `ANK_MODEL_BASE_URL` 后，模型市场下载只指向该地址
/// （如本地 HTTP 服务），用于在不联网时自测「下载→校验→安装→NER 生效」全链路。
const ENV_MODEL_BASE_URL: &str = "ANK_MODEL_BASE_URL";

/// 清单中的单个文件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelFile {
    pub name: String,
    pub size: u64,
    /// 十六进制小写 SHA-256
    pub sha256: String,
}

/// 模型市场条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    /// 全部文件大小之和（UI 展示）
    pub size_bytes: u64,
    /// 主下载基础 URL，实际文件地址为 `{base_url}/{file.name}`
    pub base_url: String,
    /// 备用下载基础 URL（主源失败时依次尝试；同一批文件，SHA-256 校验兜底）
    #[serde(default)]
    pub mirrors: Vec<String>,
    pub license: String,
    pub files: Vec<ModelFile>,
}

impl ModelEntry {
    /// 按尝试顺序排列的全部下载源（主源 + 备用源，去重、去空、去末尾 `/`）。
    pub fn sources(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for u in std::iter::once(&self.base_url).chain(self.mirrors.iter()) {
            let u = u.trim_end_matches('/').to_string();
            if !u.is_empty() && !out.contains(&u) {
                out.push(u);
            }
        }
        out
    }
}

/// 解析模型下载源：设置了环境变量 `ANK_MODEL_BASE_URL` 时**只用该地址**（自测/CI），
/// 否则 主源 = HuggingFace 官方，备用 = 镜像。纯函数，便于单测（不触碰全局 LazyLock）。
fn resolve_sources(env_value: Result<String, std::env::VarError>) -> (String, Vec<String>) {
    match env_value {
        Ok(u) => (u.trim_end_matches('/').to_string(), Vec::new()),
        Err(_) => (
            NER_PRIMARY_BASE_URL.to_string(),
            NER_MIRRORS
                .iter()
                .map(|m| format!("{m}/{NER_REPO_PATH}"))
                .collect(),
        ),
    }
}

/// 内置模型清单。
///
/// 默认项：中文 NER（`protectai/gyr66-bert-base-chinese-finetuned-ner-onnx`，ONNX FP32，约 388MB）。
/// 文件大小与 SHA-256 取自上游仓库固定 commit（HuggingFace LFS 元数据 + 实际下载校验）。
/// 注意：此前清单指向一个从未发布的 INT8 量化产物（GitHub Release 地址 404），下载必然失败；
/// 现改为直接使用上游发布的文件。
pub static BUILTIN_MODELS: LazyLock<Vec<ModelEntry>> = LazyLock::new(|| {
    let (base_url, mirrors) = resolve_sources(std::env::var(ENV_MODEL_BASE_URL));
    vec![ModelEntry {
        id: NER_MODEL_ID.to_string(),
        name: "中文 NER（人名/机构/地址）".to_string(),
        description: "基于 BERT 的中文命名实体识别（CLUENER 微调，ONNX，约 388MB）。\
                          作为正则与词典检测之后的兜底层，识别未命中规则的姓名/机构/地址。\
                          本地推理、零外传。下载后自动校验完整性；许可证以模型卡为准（训练数据许可待核实）。"
            .to_string(),
        version: "1.0.0".to_string(),
        size_bytes: 407_615_988,
        base_url,
        mirrors,
        license: "Apache-2.0（模型卡标注；训练数据许可待核实）".to_string(),
        files: vec![
            ModelFile {
                name: "model.onnx".into(),
                size: 407_063_645,
                sha256: "2fe6ee9f30730178307f2cec6b2a491e226294975f209812e4781308be2a60e9".into(),
            },
            ModelFile {
                name: "config.json".into(),
                size: 2_121,
                sha256: "7af137fc700873ba4e2ce487749090cc8248f977b9eae073678722d2d32549c6".into(),
            },
            ModelFile {
                name: "tokenizer.json".into(),
                size: 439_223,
                sha256: "a3d5c629d5ebc8652d52bc43b5e65626a46c4a1294dfa2de69de018a31071063".into(),
            },
            ModelFile {
                name: "special_tokens_map.json".into(),
                size: 125,
                sha256: "b6d346be366a7d1d48332dbc9fdf3bf8960b5d879522b7799ddba59e76237ee3".into(),
            },
            ModelFile {
                name: "tokenizer_config.json".into(),
                size: 1_334,
                sha256: "c3adea70ea9f74cc7bd6113b0434cf0099a70b61fbe5350204c2d3e58b9092af".into(),
            },
            ModelFile {
                name: "vocab.txt".into(),
                size: 109_540,
                sha256: "45bbac6b341c319adc98a532532882e91a9cefc0329aa57bac9ae761c27b291c".into(),
            },
        ],
    }]
});

/// 下载状态（面向 UI 的可序列化视图）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadStatus {
    pub model_id: String,
    /// downloading / paused / completed / failed / idle
    pub state: String,
    pub received: u64,
    pub total: u64,
    pub error: Option<String>,
    /// 过程提示（当前下载源、重试/换源原因等），供界面显示；失败时 `error` 才是最终原因
    #[serde(default)]
    pub note: Option<String>,
    /// 文件是否已全部安装（按大小判断）
    pub installed: bool,
}

impl DownloadStatus {
    fn new(model_id: &str) -> Self {
        Self {
            model_id: model_id.to_string(),
            state: "idle".to_string(),
            received: 0,
            total: 0,
            error: None,
            note: None,
            installed: false,
        }
    }
}

type StatusMap = Arc<Mutex<HashMap<String, DownloadStatus>>>;
type AbortMap = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;

/// 模型下载管理器（Tauri 托管状态）。
pub struct DownloadManager {
    client: reqwest::Client,
    statuses: StatusMap,
    actives: AbortMap,
}

impl Default for DownloadManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadManager {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client 构造失败");
        Self {
            client,
            statuses: Arc::new(Mutex::new(HashMap::new())),
            actives: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 模型存储根目录：`{app_data}/models`。
    pub fn models_root(app: &tauri::AppHandle) -> PathBuf {
        let dir = app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("models");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// 模型安装目录：`{app_data}/models/{id}`。
    pub fn install_dir(app: &tauri::AppHandle, model_id: &str) -> PathBuf {
        let dir = Self::models_root(app).join(model_id);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    pub fn entry(&self, model_id: &str) -> Option<ModelEntry> {
        BUILTIN_MODELS.iter().find(|m| m.id == model_id).cloned()
    }

    /// 当前下载状态；未启动过时返回 `idle`。
    pub fn status(&self, model_id: &str) -> DownloadStatus {
        self.statuses
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(model_id)
            .cloned()
            .unwrap_or_else(|| DownloadStatus::new(model_id))
    }

    /// 模型文件是否全部安装（按大小判断，避免每次读哈希）。
    pub fn is_installed(&self, model_id: &str, install_dir: &Path) -> bool {
        self.entry(model_id)
            .map(|e| files_installed(&e, install_dir))
            .unwrap_or(false)
    }

    /// 开始（或继续）下载模型到 `install_dir`；已有进行中任务时返回错误。
    pub fn start(&self, model_id: &str, install_dir: &Path) -> Result<(), String> {
        let entry = self
            .entry(model_id)
            .ok_or_else(|| format!("未知模型: {model_id}"))?;
        {
            let mut actives = self.actives.lock().unwrap_or_else(|p| p.into_inner());
            if actives.contains_key(model_id) {
                return Err("下载任务已在进行中，请先暂停".to_string());
            }
            actives.insert(model_id.to_string(), Arc::new(AtomicBool::new(false)));
        }
        let abort = {
            let actives = self.actives.lock().unwrap_or_else(|p| p.into_inner());
            actives.get(model_id).expect("刚插入的中止信号").clone()
        };
        {
            let mut statuses = self.statuses.lock().unwrap_or_else(|p| p.into_inner());
            let s = statuses
                .entry(model_id.to_string())
                .or_insert_with(|| DownloadStatus::new(model_id));
            s.state = "downloading".to_string();
            s.total = entry.size_bytes;
            s.received = 0;
            s.error = None;
        }

        let client = self.client.clone();
        let statuses = self.statuses.clone();
        let actives = self.actives.clone();
        let install_dir = install_dir.to_path_buf();
        let model_id = model_id.to_string();

        tauri::async_runtime::spawn(async move {
            let result =
                download_model(&client, &entry, &install_dir, &abort, &statuses, &model_id).await;
            match result {
                Ok(()) => {}
                Err(_) if abort.load(Ordering::Relaxed) => {
                    // 主动暂停：状态置 paused，不记错误
                    let mut map = statuses.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(s) = map.get_mut(&model_id) {
                        s.state = "paused".to_string();
                        s.error = None;
                    }
                }
                Err(e) => {
                    update_progress(&statuses, &model_id, 0, entry.size_bytes, "failed", Some(e));
                }
            }
            actives
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&model_id);
        });
        Ok(())
    }

    /// 暂停进行中的下载（保留 `.part`，可续传）。
    pub fn pause(&self, model_id: &str) -> Result<(), String> {
        let actives = self.actives.lock().unwrap_or_else(|p| p.into_inner());
        match actives.get(model_id) {
            Some(abort) => {
                abort.store(true, Ordering::Relaxed);
                Ok(())
            }
            None => Err("没有进行中的下载任务".to_string()),
        }
    }

    /// 删除模型安装目录并重置状态（进行中的任务需先暂停）。
    pub fn delete(&self, model_id: &str, install_dir: &Path) -> Result<(), String> {
        {
            let actives = self.actives.lock().unwrap_or_else(|p| p.into_inner());
            if actives.contains_key(model_id) {
                return Err("下载任务进行中，请先暂停再删除".to_string());
            }
        }
        if install_dir.exists() {
            std::fs::remove_dir_all(install_dir).map_err(|e| format!("删除失败: {e}"))?;
        }
        let mut map = self.statuses.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = map.get_mut(model_id) {
            *s = DownloadStatus::new(model_id);
        }
        Ok(())
    }
}

/// 模型清单中的文件是否全部安装（按大小判断）。
fn files_installed(entry: &ModelEntry, install_dir: &Path) -> bool {
    entry.files.iter().all(|f| {
        let p = install_dir.join(&f.name);
        p.is_file() && std::fs::metadata(&p).map(|m| m.len()).ok() == Some(f.size)
    })
}

/// 下载整个模型：manifest 内每个文件独立下载 + 校验；已完成文件跳过。
/// 每个文件依次尝试各下载源（主源失败/不可用时自动换备用源），源内带重试与断点续传。
async fn download_model(
    client: &reqwest::Client,
    entry: &ModelEntry,
    install_dir: &Path,
    abort: &AtomicBool,
    statuses: &StatusMap,
    model_id: &str,
) -> Result<(), String> {
    download_model_with(client, entry, install_dir, abort, statuses, model_id, &DlPolicy::default()).await
}

async fn download_model_with(
    client: &reqwest::Client,
    entry: &ModelEntry,
    install_dir: &Path,
    abort: &AtomicBool,
    statuses: &StatusMap,
    model_id: &str,
    policy: &DlPolicy,
) -> Result<(), String> {
    let sources = entry.sources();
    if sources.is_empty() {
        return Err("没有可用的下载地址".to_string());
    }
    let mut received_total = 0u64;
    for f in &entry.files {
        if abort.load(Ordering::Relaxed) {
            return Err("已暂停".to_string());
        }
        let dest = install_dir.join(&f.name);
        let part = install_dir.join(format!("{}.part", f.name));
        if dest.is_file() && std::fs::metadata(&dest).map(|m| m.len()).ok() == Some(f.size) {
            if verify_sha256(&dest, &f.sha256).await? {
                received_total += f.size;
                update_progress(
                    statuses,
                    model_id,
                    received_total,
                    entry.size_bytes,
                    "downloading",
                    None,
                );
                continue;
            }
            let _ = std::fs::remove_file(&dest);
        }
        let urls: Vec<String> = sources.iter().map(|b| format!("{b}/{}", f.name)).collect();
        let target = FileTarget {
            dest,
            part,
            size: f.size,
            sha256: f.sha256.clone(),
        };
        let name = f.name.clone();
        let got = download_file_multi(
            client,
            &urls,
            &target,
            abort,
            policy,
            |recv| {
                update_progress(
                    statuses,
                    model_id,
                    received_total + recv,
                    entry.size_bytes,
                    "downloading",
                    None,
                );
            },
            |note| set_note(statuses, model_id, Some(format!("{name}：{note}"))),
        )
        .await
        .map_err(|e| format!("下载 {} 失败: {e}", f.name))?;
        received_total += got;
    }
    update_progress(
        statuses,
        model_id,
        entry.size_bytes,
        entry.size_bytes,
        "completed",
        None,
    );
    set_note(statuses, model_id, None);
    Ok(())
}

/// 重试/超时策略（生产用默认值；测试里换成毫秒级以免等待）。
#[derive(Clone, Debug)]
struct DlPolicy {
    request_timeout: Duration,
    stall_timeout: Duration,
    attempts_per_source: u32,
    backoff_base: Duration,
}

impl Default for DlPolicy {
    fn default() -> Self {
        Self {
            request_timeout: REQUEST_TIMEOUT,
            stall_timeout: STALL_TIMEOUT,
            attempts_per_source: MAX_ATTEMPTS_PER_SOURCE,
            backoff_base: BACKOFF_BASE,
        }
    }
}

/// 单次下载尝试的失败分类：决定“重试 / 换源 / 终止”。
#[derive(Debug)]
enum DlError {
    /// 用户暂停
    Abort,
    /// 本机问题（磁盘写入失败等）：换源或重试都没用，立即终止
    Fatal(String),
    /// 这个源本身不可用（404/403 等）：不重试，直接换下一个源
    SourceBad(String),
    /// 暂时性问题（网络错误/超时/卡死/5xx/限流/截断/校验失败）：退避后重试
    Transient(String),
}

impl std::fmt::Display for DlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DlError::Abort => write!(f, "已暂停"),
            DlError::Fatal(m) | DlError::SourceBad(m) | DlError::Transient(m) => write!(f, "{m}"),
        }
    }
}

/// URL 的主机名（用于提示“正在从 xxx 下载”）。
fn host_of(url: &str) -> String {
    url.split("//")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or(url)
        .to_string()
}

/// 退避等待，期间每 100ms 检查一次暂停标志。返回 false 表示被暂停。
async fn backoff_sleep(d: Duration, abort: &AtomicBool) -> bool {
    let mut left = d;
    let step = Duration::from_millis(100).min(d.max(Duration::from_millis(1)));
    while left > Duration::ZERO {
        if abort.load(Ordering::Relaxed) {
            return false;
        }
        let t = step.min(left);
        tokio::time::sleep(t).await;
        left = left.saturating_sub(t);
    }
    !abort.load(Ordering::Relaxed)
}

/// 单文件多源下载：依次尝试每个源；源内对暂时性错误做指数退避重试（断点续传沿用同一 `.part`）。
async fn download_file_multi(
    client: &reqwest::Client,
    urls: &[String],
    target: &FileTarget,
    abort: &AtomicBool,
    policy: &DlPolicy,
    on_progress: impl Fn(u64),
    on_note: impl Fn(String),
) -> Result<u64, String> {
    let mut failures: Vec<String> = Vec::new();
    for (si, url) in urls.iter().enumerate() {
        let host = host_of(url);
        let mut attempt = 1u32;
        loop {
            if abort.load(Ordering::Relaxed) {
                return Err("已暂停".to_string());
            }
            if si == 0 && attempt == 1 {
                on_note(format!("正在从 {host} 下载…"));
            }
            match download_attempt(client, url, target, abort, policy, &on_progress).await {
                Ok(n) => return Ok(n),
                Err(DlError::Abort) => return Err("已暂停".to_string()),
                Err(DlError::Fatal(m)) => return Err(m),
                Err(DlError::SourceBad(m)) => {
                    failures.push(format!("{host}：{m}"));
                    if si + 1 < urls.len() {
                        on_note(format!("{host} 不可用（{m}），改用 {}…", host_of(&urls[si + 1])));
                    }
                    break;
                }
                Err(DlError::Transient(m)) => {
                    if attempt >= policy.attempts_per_source {
                        failures.push(format!("{host}：{m}（已重试 {attempt} 次）"));
                        if si + 1 < urls.len() {
                            on_note(format!("{host} 多次失败，改用 {}…", host_of(&urls[si + 1])));
                        }
                        break;
                    }
                    let wait = policy.backoff_base * 2u32.pow(attempt - 1);
                    on_note(format!(
                        "{host}：{m}，{}秒后重试（{attempt}/{}）",
                        wait.as_secs().max(1),
                        policy.attempts_per_source
                    ));
                    if !backoff_sleep(wait, abort).await {
                        return Err("已暂停".to_string());
                    }
                    attempt += 1;
                }
            }
        }
    }
    Err(format!("所有下载源都失败：{}", failures.join("；")))
}

/// 单文件下载目标（目标路径 / 续传临时文件 / 期望大小与校验和）。
struct FileTarget {
    dest: PathBuf,
    part: PathBuf,
    size: u64,
    sha256: String,
}

/// 单文件单次下载（兼容入口，默认策略；错误转成字符串）。
#[cfg(test)]
async fn download_file(
    client: &reqwest::Client,
    url: &str,
    target: &FileTarget,
    abort: &AtomicBool,
    on_progress: impl Fn(u64),
) -> Result<u64, String> {
    download_attempt(client, url, target, abort, &DlPolicy::default(), &on_progress)
        .await
        .map_err(|e| e.to_string())
}

/// 单文件单次下载尝试：断点续传 + 卡死检测 + 大小/SHA-256 校验 + 原子改名。
async fn download_attempt(
    client: &reqwest::Client,
    url: &str,
    target: &FileTarget,
    abort: &AtomicBool,
    policy: &DlPolicy,
    on_progress: &impl Fn(u64),
) -> Result<u64, DlError> {
    let part = &target.part;
    let dest = &target.dest;

    // 1. 断点续传起点（.part 存在且小于目标大小时续传，否则丢弃）
    let mut resume_from = 0u64;
    if part.is_file() {
        let len = std::fs::metadata(part).map(|m| m.len()).ok().unwrap_or(0);
        if len < target.size {
            resume_from = len;
        } else {
            let _ = std::fs::remove_file(part);
        }
    }

    let mut req = client.get(url);
    if resume_from > 0 {
        req = req.header(RANGE, format!("bytes={resume_from}-"));
    }
    // 连接 + 响应头必须在 request_timeout 内到达，否则按暂时性错误重试
    let mut resp = match tokio::time::timeout(policy.request_timeout, req.send()).await {
        Err(_) => return Err(DlError::Transient("连接超时".to_string())),
        Ok(Err(e)) => return Err(DlError::Transient(format!("网络请求失败: {}", short_err(&e)))),
        Ok(Ok(r)) => r,
    };
    let status = resp.status();

    let start = if resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
        resume_from
    } else if status.is_success() {
        0 // 服务器忽略 Range（200）：从头下载，覆盖 .part
    } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // 续传位置不被接受（本地 .part 与远端文件不一致）：丢弃 .part，下一次从头来
        let _ = std::fs::remove_file(part);
        return Err(DlError::Transient("续传位置无效，已清理后重试".to_string()));
    } else if status.is_server_error() || status.as_u16() == 429 || status.as_u16() == 408 {
        return Err(DlError::Transient(format!("HTTP {status}")));
    } else {
        // 404/403/401/410 等：这个源上没有该文件或不允许访问，重试没有意义
        return Err(DlError::SourceBad(format!("HTTP {status}")));
    };

    // 2. 打开目标：续传 → append；从头 → truncate
    let open = if start > 0 {
        tokio::fs::OpenOptions::new().create(true).append(true).open(part).await
    } else {
        tokio::fs::OpenOptions::new().create(true).truncate(true).write(true).open(part).await
    };
    let mut file = open.map_err(|e| DlError::Fatal(format!("打开 .part 失败: {e}")))?;

    // 3. 流式写入（每块检查中止标志；每块读取都有卡死超时）。无论正常 EOF 还是读取中断，
    //    都先 flush + drop 落盘，保证 .part 保留已收到的字节供断点续传。
    let mut received = start;
    let mut read_err: Option<DlError> = None;
    loop {
        match tokio::time::timeout(policy.stall_timeout, resp.chunk()).await {
            Err(_) => {
                read_err = Some(DlError::Transient(format!(
                    "{} 秒没有收到数据（连接卡住）",
                    policy.stall_timeout.as_secs().max(1)
                )));
                break;
            }
            Ok(Ok(Some(chunk))) => {
                if abort.load(Ordering::Relaxed) {
                    file.flush().await.map_err(|e| DlError::Fatal(format!("写入失败: {e}")))?;
                    return Err(DlError::Abort);
                }
                file.write_all(&chunk)
                    .await
                    .map_err(|e| DlError::Fatal(format!("写入失败（磁盘空间不足或无权限）: {e}")))?;
                received += chunk.len() as u64;
                on_progress(received);
            }
            Ok(Ok(None)) => break,
            Ok(Err(e)) => {
                read_err = Some(DlError::Transient(format!("读取响应失败: {}", short_err(&e))));
                break;
            }
        }
    }
    file.flush().await.map_err(|e| DlError::Fatal(format!("写入失败: {e}")))?;
    drop(file);
    if let Some(e) = read_err {
        return Err(e);
    }

    // 4. 大小校验（发现截断即报错，但保留 .part 供断点续传复用；
    //    最终完整性由步骤 5 的 SHA-256 兜底，服务端中途断开不会白传）
    if received != target.size {
        if received > target.size {
            let _ = std::fs::remove_file(part); // 比预期还大：内容不可信，丢弃
        }
        return Err(DlError::Transient(format!(
            "文件大小不符: 期望 {} 字节，实际 {received} 字节",
            target.size
        )));
    }

    // 5. SHA-256 校验（失败删除 .part，下次从头下载）
    let hash = hash_file(part).await.map_err(DlError::Fatal)?;
    if hash != target.sha256 {
        let _ = std::fs::remove_file(part);
        return Err(DlError::Transient("SHA-256 校验失败（已删除残留，重新下载）".to_string()));
    }

    // 6. 原子改名 .part → dest
    tokio::fs::rename(part, dest)
        .await
        .map_err(|e| DlError::Fatal(format!("落盘失败: {e}")))?;
    Ok(received)
}

/// reqwest 错误的简短中文描述（避免把整条 URL/内部细节塞进界面）。
fn short_err(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "超时".to_string()
    } else if e.is_connect() {
        "无法连接（请检查网络或代理）".to_string()
    } else if e.is_body() || e.is_decode() {
        "数据传输中断".to_string()
    } else {
        "网络错误".to_string()
    }
}

/// 分块读取并计算文件 SHA-256。
async fn hash_file(path: &Path) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("打开失败: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| format!("读取失败: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 校验文件 SHA-256 是否与期望一致。
async fn verify_sha256(path: &Path, expected: &str) -> Result<bool, String> {
    Ok(hash_file(path).await? == expected)
}

fn update_progress(
    statuses: &StatusMap,
    model_id: &str,
    received: u64,
    total: u64,
    state: &str,
    error: Option<String>,
) {
    let mut map = statuses.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = map.get_mut(model_id) {
        s.received = received;
        s.total = total;
        s.state = state.to_string();
        s.error = error;
    }
}

fn set_note(statuses: &StatusMap, model_id: &str, note: Option<String>) {
    let mut map = statuses.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = map.get_mut(model_id) {
        s.note = note;
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// 模型市场清单。
#[tauri::command]
pub fn list_models() -> Vec<ModelEntry> {
    BUILTIN_MODELS.clone()
}

/// 模型安装目录（界面提示“也可手动把文件放入此目录”用）。
#[tauri::command]
pub fn model_install_dir(app: tauri::AppHandle, model_id: String) -> String {
    DownloadManager::install_dir(&app, &model_id).display().to_string()
}

/// 查询模型下载/安装状态。
#[tauri::command]
pub fn model_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, DownloadManager>,
    model_id: String,
) -> DownloadStatus {
    let mut st = state.status(&model_id);
    if st.state != "downloading" && st.state != "paused" {
        let dir = DownloadManager::install_dir(&app, &model_id);
        st.installed = state.is_installed(&model_id, &dir);
        if st.installed && st.state != "failed" {
            st.state = "completed".to_string();
            st.total = st.total.max(st.received);
        }
    }
    st
}

/// 开始（或继续）下载模型。
#[tauri::command]
pub fn start_model_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, DownloadManager>,
    model_id: String,
    consent: Option<bool>,
) -> Result<(), String> {
    // 隐私开关：用户关闭「允许网络下载」后，禁止一切模型下载（含断点续传），
    // 应用唯一网络用途被禁用，实现 spec「唯一网络用途可关闭」。
    // `consent = true`：用户刚在界面上点了“开启联网并下载”，视为本次明确同意（此时配置可能还没落库，
    // 不能再按旧配置拒绝）。
    if consent != Some(true) && !network_download_allowed(&app) {
        return Err("模型下载已在「模型市场」关闭（允许网络下载 = 关），请开启后再试".to_string());
    }
    let dir = DownloadManager::install_dir(&app, &model_id);
    state.start(&model_id, &dir)
}

/// 读取配置库中的 `allowModelDownload`；配置缺失、解析失败或 SQLite 不可用时视为允许。
fn network_download_allowed(app: &tauri::AppHandle) -> bool {
    let Some(db) = app.try_state::<crate::audit::AuditDb>() else {
        return true;
    };
    config_allows_download(db.config_get().ok().flatten())
}

/// 从配置 JSON 判断是否允许模型下载（纯函数，便于单测）。缺失/非法一律视为允许。
fn config_allows_download(json: Option<String>) -> bool {
    let Some(json) = json else {
        return true;
    };
    match serde_json::from_str::<serde_json::Value>(&json) {
        Ok(v) => v
            .get("allowModelDownload")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        Err(_) => true,
    }
}

/// 暂停下载。
#[tauri::command]
pub fn pause_model_download(
    state: tauri::State<'_, DownloadManager>,
    model_id: String,
) -> Result<(), String> {
    state.pause(&model_id)
}

/// 删除模型。
#[tauri::command]
pub fn delete_model(
    app: tauri::AppHandle,
    state: tauri::State<'_, DownloadManager>,
    model_id: String,
) -> Result<(), String> {
    let dir = DownloadManager::install_dir(&app, &model_id);
    state.delete(&model_id, &dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicU32, AtomicUsize};

    fn sha_hex(data: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(data);
        format!("{:x}", h.finalize())
    }

    fn temp_dir() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "anonkit-models-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 极简 HTTP 文件服务器：支持多文件与单区间 Range；`interrupt_after` 使
    /// 第一个连接只发送 N 字节后断开（模拟下载中断）。
    struct MockServer {
        addr: String,
        join: Option<std::thread::JoinHandle<()>>,
        stop: Arc<AtomicBool>,
    }

    impl MockServer {
        /// 单文件服务器（既有用例的便捷入口）。
        fn start(content: Vec<u8>, interrupt_after: Option<usize>) -> Self {
            Self::start_files(vec![("model.onnx".to_string(), content)], interrupt_after)
        }

        /// 多文件服务器：按请求路径返回对应文件内容。
        fn start_files(files: Vec<(String, Vec<u8>)>, interrupt_after: Option<usize>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let addr = listener.local_addr().unwrap();
            let files: HashMap<String, Vec<u8>> = files.into_iter().collect();
            let conns = Arc::new(AtomicUsize::new(0));
            let conns2 = Arc::clone(&conns);
            let stop = Arc::new(AtomicBool::new(false));
            let stop2 = Arc::clone(&stop);
            let join = std::thread::spawn(move || {
                while !stop2.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let conn_no = conns2.fetch_add(1, Ordering::SeqCst) + 1;
                            serve(&mut stream, &files, conn_no, interrupt_after);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                addr: format!("http://{addr}"),
                join: Some(join),
                stop,
            }
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut buf = [0u8; 4096];
        let mut n = 0usize;
        loop {
            match stream.read(&mut buf[n..]) {
                Ok(0) => break,
                Ok(k) => {
                    n += k;
                    if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // 请求尚未到达：短暂让步后重试，避免提前关闭连接导致客户端"请求失败"。
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&buf[..n]).to_string()
    }

    fn serve(
        stream: &mut TcpStream,
        files: &HashMap<String, Vec<u8>>,
        conn_no: usize,
        interrupt_after: Option<usize>,
    ) {
        let request = read_request(stream);
        if request.is_empty() {
            return;
        }
        // 解析请求行 "GET /model.onnx HTTP/1.1" 的文件路径
        let path = request
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|p| p.strip_prefix('/'))
            .unwrap_or("");
        let content = match files.get(path) {
            Some(c) => c.clone(),
            None => {
                write_all_nb(
                    stream,
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                return;
            }
        };
        let range_from = request.lines().find_map(|l| {
            let l = l.trim();
            l.strip_prefix("Range: bytes=").and_then(|v| {
                let end = v.find('-').unwrap_or(v.len());
                v[..end].parse::<usize>().ok()
            })
        });
        let offset = range_from.unwrap_or(0).min(content.len());
        let body = &content[offset..];
        let status = if range_from.is_some() {
            "206 Partial Content"
        } else {
            "200 OK"
        };
        let mut header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        if let Some(from) = range_from {
            let last = content.len().saturating_sub(1);
            header.push_str(&format!(
                "Content-Range: bytes {from}-{last}/{}\r\n",
                content.len()
            ));
        }
        header.push_str("\r\n");
        write_all_nb(stream, header.as_bytes());

        let to_write = if conn_no == 1 {
            interrupt_after.unwrap_or(body.len()).min(body.len())
        } else {
            body.len()
        };
        write_all_nb(stream, &body[..to_write]);
        // 模拟中断：只写部分字节后关闭写端（shutdown 保证 FIN 排在已发送数据之后）。
        let _ = stream.shutdown(std::net::Shutdown::Write);
    }

    /// non-blocking socket 上循环写入，直到写完或连接失败（WouldBlock 时短暂让步）。
    fn write_all_nb(stream: &mut TcpStream, buf: &[u8]) {
        let mut written = 0usize;
        while written < buf.len() {
            match stream.write(&buf[written..]) {
                Ok(n) if n > 0 => written += n,
                Ok(_) => return,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(_) => return,
            }
        }
    }

    #[test]
    fn manifest_is_sane() {
        let mut ids: Vec<&str> = BUILTIN_MODELS.iter().map(|m| m.id.as_str()).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "模型 id 应唯一");

        for m in BUILTIN_MODELS.iter() {
            assert!(!m.files.is_empty());
            let sum: u64 = m.files.iter().map(|f| f.size).sum();
            assert_eq!(m.size_bytes, sum, "size_bytes 应与文件大小之和一致");
            for f in &m.files {
                assert_eq!(f.sha256.len(), 64, "SHA-256 应为 64 位十六进制");
                assert!(f.sha256.chars().all(|c| c.is_ascii_hexdigit()));
                assert!(!f.name.is_empty());
            }
        }
    }

    #[test]
    fn files_installed_checks_sizes() {
        let entry = ModelEntry {
            id: "test".into(),
            name: "test".into(),
            description: String::new(),
            version: "0".into(),
            size_bytes: 8,
            base_url: "http://127.0.0.1:1".into(),
            mirrors: vec![],
            license: "MIT".into(),
            files: vec![
                ModelFile {
                    name: "model.onnx".into(),
                    size: 4,
                    sha256: "a".into(),
                },
                ModelFile {
                    name: "config.json".into(),
                    size: 4,
                    sha256: "b".into(),
                },
            ],
        };
        let dir = temp_dir();
        assert!(!files_installed(&entry, &dir));

        std::fs::write(dir.join("model.onnx"), b"aaaa").unwrap();
        std::fs::write(dir.join("config.json"), b"bbbb").unwrap();
        assert!(files_installed(&entry, &dir));

        // 大小不符（被截断）不算已安装
        std::fs::write(dir.join("config.json"), b"b").unwrap();
        assert!(!files_installed(&entry, &dir));
    }

    #[tokio::test]
    async fn download_file_completes_and_verifies_sha() {
        let content: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let sha = sha_hex(&content);
        let server = MockServer::start(content, None);
        let dir = temp_dir();
        let dest = dir.join("model.onnx");
        let part = dir.join("model.onnx.part");
        let client = reqwest::Client::new();
        let abort = Arc::new(AtomicBool::new(false));

        let target = FileTarget {
            dest: dest.clone(),
            part: part.clone(),
            size: 200_000,
            sha256: sha.clone(),
        };
        let got = download_file(
            &client,
            &format!("{}/model.onnx", server.addr),
            &target,
            &abort,
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(got, 200_000);
        assert!(dest.is_file());
        assert!(!part.exists(), ".part 应已被改名");
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), 200_000);
    }

    #[tokio::test]
    async fn download_file_resumes_after_interrupt() {
        let content: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let sha = sha_hex(&content);
        let server = MockServer::start(content, Some(100_000));
        let dir = temp_dir();
        let dest = dir.join("model.onnx");
        let part = dir.join("model.onnx.part");
        let client = reqwest::Client::new();
        let abort = Arc::new(AtomicBool::new(false));
        let url = format!("{}/model.onnx", server.addr);

        // 第一次：服务器发送 100KB 后断开 → 失败，.part 保留 100_000 字节
        let target = FileTarget {
            dest: dest.clone(),
            part: part.clone(),
            size: 300_000,
            sha256: sha.clone(),
        };
        let err = download_file(&client, &url, &target, &abort, |_| {})
            .await
            .unwrap_err();
        // 服务器中断后的错误路径依连接关闭方式而定（EOF→大小不符 / RST→读取失败），
        // 关键不变量是 .part 保留 100_000 字节供续传。
        assert!(
            err.contains("文件大小不符") || err.contains("读取响应失败"),
            "err = {err}"
        );
        assert_eq!(std::fs::metadata(&part).unwrap().len(), 100_000);

        // 第二次：从 .part 续传（Range: bytes=100000-）→ 完成且校验通过
        download_file(&client, &url, &target, &abort, |_| {})
            .await
            .unwrap();
        assert!(dest.is_file());
        assert!(!part.exists());
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), 300_000);
    }

    #[tokio::test]
    async fn download_file_rejects_checksum_mismatch() {
        let content: Vec<u8> = vec![7u8; 50_000];
        let wrong_sha = sha_hex(b"different content");
        let server = MockServer::start(content, None);
        let dir = temp_dir();
        let dest = dir.join("model.onnx");
        let part = dir.join("model.onnx.part");
        let client = reqwest::Client::new();
        let abort = Arc::new(AtomicBool::new(false));

        let target = FileTarget {
            dest: dest.clone(),
            part: part.clone(),
            size: 50_000,
            sha256: wrong_sha,
        };
        let err = download_file(
            &client,
            &format!("{}/model.onnx", server.addr),
            &target,
            &abort,
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(err.contains("SHA-256"), "err = {err}");
        assert!(!part.exists(), "校验失败后 .part 应被清理");
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn download_model_installs_all_files_and_marks_completed() {
        // 多文件完整下载 e2e：逐文件下载 → SHA-256 校验 → 安装 → 状态机 completed。
        let files: Vec<(String, Vec<u8>)> = vec![
            (
                "model.onnx".into(),
                (0..100_000u32).map(|i| (i % 251) as u8).collect(),
            ),
            ("config.json".into(), b"{\"name\":\"test\"}".to_vec()),
            ("tokenizer.json".into(), vec![7u8; 4096]),
        ];
        let server = MockServer::start_files(files.clone(), None);
        let entry = ModelEntry {
            id: "test".into(),
            name: "test".into(),
            description: String::new(),
            version: "0".into(),
            size_bytes: files.iter().map(|(_, c)| c.len() as u64).sum(),
            base_url: server.addr.clone(),
            mirrors: vec![],
            license: "MIT".into(),
            files: files
                .iter()
                .map(|(name, content)| ModelFile {
                    name: name.clone(),
                    size: content.len() as u64,
                    sha256: sha_hex(content),
                })
                .collect(),
        };
        let dir = temp_dir();
        let client = reqwest::Client::new();
        let abort = Arc::new(AtomicBool::new(false));
        let statuses: StatusMap = Arc::new(Mutex::new(HashMap::new()));
        // 与生产路径 DownloadManager::start() 一致：先插入初始状态，update_progress 才可更新
        statuses
            .lock()
            .unwrap()
            .insert("test".to_string(), DownloadStatus::new("test"));

        download_model(&client, &entry, &dir, &abort, &statuses, "test")
            .await
            .unwrap();

        // 全部文件已安装，且内容与 SHA-256 逐字节一致
        assert!(files_installed(&entry, &dir));
        for (name, content) in &files {
            assert_eq!(
                std::fs::read(dir.join(name)).unwrap(),
                *content,
                "文件 {name} 内容不一致"
            );
        }
        // 状态机推进到 completed，received == total == size_bytes
        let st = statuses.lock().unwrap().get("test").cloned().unwrap();
        assert_eq!(st.state, "completed");
        assert_eq!(st.received, st.total);
        assert_eq!(st.received, entry.size_bytes);
    }

    #[test]
    fn manager_delete_removes_files_and_resets_status() {
        let manager = DownloadManager::new();
        let dir = temp_dir();
        std::fs::write(dir.join("model.onnx"), b"test").unwrap();

        manager.delete(NER_MODEL_ID, &dir).unwrap();
        assert!(!dir.exists());
        let st = manager.status(NER_MODEL_ID);
        assert_eq!(st.state, "idle");
    }

    #[test]
    fn config_allows_download_flag_semantics() {
        // 缺失配置 → 允许（默认开启，兼容旧配置）
        assert!(config_allows_download(None));
        // 配置无该字段 → 允许
        assert!(config_allows_download(Some(
            r#"{"nerEnabled":true}"#.into()
        )));
        // 显式 false → 禁止（「唯一网络用途可关闭」）
        assert!(!config_allows_download(Some(
            r#"{"allowModelDownload":false}"#.into()
        )));
        // 显式 true → 允许
        assert!(config_allows_download(Some(
            r#"{"allowModelDownload":true}"#.into()
        )));
        // 非法 JSON → 允许（不因脏配置锁死功能）
        assert!(config_allows_download(Some("not json".into())));
    }

    #[test]
    fn base_url_env_override_semantics() {
        // 未设置环境变量 → 回退内置默认地址
        assert_eq!(
            resolve_sources(Err(std::env::VarError::NotPresent)).0,
            NER_PRIMARY_BASE_URL
        );
        // 设置后 → 使用覆盖地址，且去除末尾 "/"
        assert_eq!(
            resolve_sources(Ok("http://127.0.0.1:8765/".into())).0,
            "http://127.0.0.1:8765"
        );
        assert_eq!(
            resolve_sources(Ok("http://127.0.0.1:8765".into())).0,
            "http://127.0.0.1:8765"
        );
        // 空值视为覆盖为空串（下载 URL 将不可用，但语义保持「显式覆盖」）
        assert_eq!(resolve_sources(Ok(String::new())).0, "");
        // 覆盖时不带镜像（只用指定地址）；默认时带镜像
        assert!(resolve_sources(Ok("http://x".into())).1.is_empty());
        assert!(!resolve_sources(Err(std::env::VarError::NotPresent)).1.is_empty());
    }

    // ------------------------- 稳定性：重试 / 换源 / 卡死 / 暂停 -------------------------

    fn fast_policy() -> DlPolicy {
        DlPolicy {
            request_timeout: Duration::from_millis(500),
            stall_timeout: Duration::from_millis(300),
            attempts_per_source: 3,
            backoff_base: Duration::from_millis(10),
        }
    }

    fn target_for(dir: &Path, content: &[u8]) -> FileTarget {
        FileTarget {
            dest: dir.join("model.onnx"),
            part: dir.join("model.onnx.part"),
            size: content.len() as u64,
            sha256: sha_hex(content),
        }
    }

    /// 发完响应头后一直不发数据的“卡死”服务器（模拟网络黑洞/限速到 0）。
    fn start_stalling_server(content_len: usize) -> (String, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut held: Vec<TcpStream> = Vec::new();
            while !stop2.load(Ordering::SeqCst) {
                if let Ok((mut st, _)) = listener.accept() {
                    let _ = read_request(&mut st);
                    let h = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {content_len}\r\nConnection: close\r\n\r\n"
                    );
                    write_all_nb(&mut st, h.as_bytes());
                    held.push(st); // 持有连接不再写数据
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        (addr, stop)
    }

    #[tokio::test]
    async fn failover_to_mirror_when_primary_returns_404() {
        // 复现真实故障：主源（占位的 GitHub Release 地址）404，镜像有文件 → 必须自动换源成功
        let content: Vec<u8> = (0..80_000u32).map(|i| (i % 251) as u8).collect();
        let good = MockServer::start(content.clone(), None);
        let bad = MockServer::start_files(vec![], None); // 任何路径都 404
        let dir = temp_dir();
        let target = target_for(&dir, &content);
        let abort = AtomicBool::new(false);
        let notes = Mutex::new(Vec::<String>::new());
        let got = download_file_multi(
            &reqwest::Client::new(),
            &[format!("{}/model.onnx", bad.addr), format!("{}/model.onnx", good.addr)],
            &target,
            &abort,
            &fast_policy(),
            |_| {},
            |n| notes.lock().unwrap().push(n),
        )
        .await
        .unwrap();
        assert_eq!(got, content.len() as u64);
        assert!(target.dest.is_file());
        let notes = notes.lock().unwrap().join(" | ");
        assert!(notes.contains("不可用") && notes.contains("改用"), "应提示已换源: {notes}");
    }

    #[tokio::test]
    async fn auto_resume_after_interrupt_without_user_retry() {
        // 中途断开 → 自动退避重试并从 .part 续传，用户无需再点一次
        let content: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let server = MockServer::start(content.clone(), Some(100_000));
        let dir = temp_dir();
        let target = target_for(&dir, &content);
        let abort = AtomicBool::new(false);
        let got = download_file_multi(
            &reqwest::Client::new(),
            &[format!("{}/model.onnx", server.addr)],
            &target,
            &abort,
            &fast_policy(),
            |_| {},
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(got, 300_000);
        assert_eq!(std::fs::read(&target.dest).unwrap(), content);
        assert!(!target.part.exists());
    }

    #[tokio::test]
    async fn stalled_connection_times_out_then_fails_over() {
        // 卡死的源必须在 stall 超时后放弃（此前没有读超时，会永远显示“下载中”）
        let content: Vec<u8> = vec![9u8; 40_000];
        let (stall_addr, stop) = start_stalling_server(content.len());
        let good = MockServer::start(content.clone(), None);
        let dir = temp_dir();
        let target = target_for(&dir, &content);
        let abort = AtomicBool::new(false);
        let started = std::time::Instant::now();
        let got = download_file_multi(
            &reqwest::Client::new(),
            &[format!("{stall_addr}/model.onnx"), format!("{}/model.onnx", good.addr)],
            &target,
            &abort,
            &fast_policy(),
            |_| {},
            |_| {},
        )
        .await
        .unwrap();
        stop.store(true, Ordering::SeqCst);
        assert_eq!(got, 40_000);
        // 3 次尝试 × 300ms 卡死超时 + 退避，远小于“永远卡住”
        assert!(started.elapsed() < Duration::from_secs(10), "耗时 {:?}", started.elapsed());
    }

    #[tokio::test]
    async fn all_sources_failing_reports_each_reason() {
        let a = MockServer::start_files(vec![], None);
        let b = MockServer::start_files(vec![], None);
        let dir = temp_dir();
        let target = target_for(&dir, b"x");
        let abort = AtomicBool::new(false);
        let err = download_file_multi(
            &reqwest::Client::new(),
            &[format!("{}/model.onnx", a.addr), format!("{}/model.onnx", b.addr)],
            &target,
            &abort,
            &fast_policy(),
            |_| {},
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(err.contains("所有下载源都失败"), "{err}");
        assert!(err.contains("HTTP 404"), "应带上每个源的真实原因: {err}");
        assert_eq!(err.matches("HTTP 404").count(), 2, "两个源各一条: {err}");
    }

    #[tokio::test]
    async fn unreachable_source_is_transient_and_retried_then_next_source() {
        // 端口上没有服务（连接被拒绝）→ 视为暂时性错误重试，最终换到可用源
        let content: Vec<u8> = vec![3u8; 10_000];
        let good = MockServer::start(content.clone(), None);
        let dir = temp_dir();
        let target = target_for(&dir, &content);
        let abort = AtomicBool::new(false);
        let notes = Mutex::new(Vec::<String>::new());
        download_file_multi(
            &reqwest::Client::new(),
            &["http://127.0.0.1:1/model.onnx".to_string(), format!("{}/model.onnx", good.addr)],
            &target,
            &abort,
            &fast_policy(),
            |_| {},
            |n| notes.lock().unwrap().push(n),
        )
        .await
        .unwrap();
        let notes = notes.lock().unwrap().join(" | ");
        assert!(notes.contains("后重试") && notes.contains("多次失败"), "{notes}");
    }

    #[tokio::test]
    async fn pause_interrupts_backoff_promptly() {
        let dir = temp_dir();
        let target = target_for(&dir, b"x");
        let abort = Arc::new(AtomicBool::new(false));
        let a2 = Arc::clone(&abort);
        let slow = DlPolicy { backoff_base: Duration::from_secs(30), ..fast_policy() };
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            a2.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        let err = download_file_multi(
            &reqwest::Client::new(),
            &["http://127.0.0.1:1/model.onnx".to_string()],
            &target,
            &abort,
            &slow,
            |_| {},
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(err, "已暂停");
        assert!(started.elapsed() < Duration::from_secs(5), "暂停应立即生效，耗时 {:?}", started.elapsed());
    }

    #[tokio::test]
    async fn download_model_with_fails_over_across_files() {
        // 整个模型多文件：主源 404、镜像完整 → 全部文件装好，状态 completed
        let files: Vec<(String, Vec<u8>)> = vec![
            ("model.onnx".into(), (0..60_000u32).map(|i| (i % 251) as u8).collect()),
            ("config.json".into(), b"{\"a\":1}".to_vec()),
        ];
        let good = MockServer::start_files(files.clone(), None);
        let bad = MockServer::start_files(vec![], None);
        let entry = ModelEntry {
            id: "t".into(),
            name: "t".into(),
            description: String::new(),
            version: "0".into(),
            size_bytes: files.iter().map(|(_, c)| c.len() as u64).sum(),
            base_url: bad.addr.clone(),
            mirrors: vec![good.addr.clone()],
            license: "MIT".into(),
            files: files
                .iter()
                .map(|(n, c)| ModelFile { name: n.clone(), size: c.len() as u64, sha256: sha_hex(c) })
                .collect(),
        };
        let dir = temp_dir();
        let abort = AtomicBool::new(false);
        let statuses: StatusMap = Arc::new(Mutex::new(HashMap::new()));
        statuses.lock().unwrap().insert("t".into(), DownloadStatus::new("t"));
        download_model_with(&reqwest::Client::new(), &entry, &dir, &abort, &statuses, "t", &fast_policy())
            .await
            .unwrap();
        assert!(files_installed(&entry, &dir));
        let st = statuses.lock().unwrap().get("t").cloned().unwrap();
        assert_eq!(st.state, "completed");
        assert!(st.note.is_none(), "完成后不应残留过程提示");
    }

    #[test]
    fn builtin_manifest_points_to_real_upstream_with_mirror() {
        let m = &BUILTIN_MODELS[0];
        assert_eq!(m.id, NER_MODEL_ID);
        let srcs = m.sources();
        if std::env::var("ANK_MODEL_BASE_URL").is_err() {
            assert!(srcs[0].starts_with("https://huggingface.co/"), "{srcs:?}");
            assert!(srcs.iter().any(|u| u.starts_with("https://hf-mirror.com/")), "应带备用镜像: {srcs:?}");
            assert!(!srcs.iter().any(|u| u.contains("anonkit-ai")), "不得再指向从未发布的占位地址");
            // 固定到具体 commit，内容不随 main 变化
            assert!(srcs.iter().all(|u| u.contains("5fb2d9494046e1fd2d9d62d1366074a60efae0c1")), "{srcs:?}");
        }
        // model.onnx 是上游 FP32 文件
        let onnx = m.files.iter().find(|f| f.name == "model.onnx").unwrap();
        assert_eq!(onnx.size, 407_063_645);
    }

    /// 联网真实下载（默认忽略）：用内置清单从真实上游/镜像下载并校验 SHA-256。
    /// 运行：`ANK_LIVE_DIR=/some/dir cargo test -p anonkit live_download -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_download_real_model_matches_manifest() {
        let dir = std::env::var("ANK_LIVE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| temp_dir());
        std::fs::create_dir_all(&dir).unwrap();
        let entry = BUILTIN_MODELS[0].clone();
        println!("下载源: {:?}", entry.sources());
        let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).build().unwrap();
        let abort = AtomicBool::new(false);
        let statuses: StatusMap = Arc::new(Mutex::new(HashMap::new()));
        statuses.lock().unwrap().insert(entry.id.clone(), DownloadStatus::new(&entry.id));
        let t0 = std::time::Instant::now();
        let st2 = Arc::clone(&statuses);
        let id = entry.id.clone();
        let reporter = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let s = st2.lock().unwrap().get(&id).cloned();
                if let Some(s) = s {
                    println!("  {} {:.1}/{:.1} MB 提示={:?}", s.state, s.received as f64 / 1e6, s.total as f64 / 1e6, s.note);
                    if s.state == "completed" || s.state == "failed" {
                        break;
                    }
                }
            }
        });
        let r = download_model(&client, &entry, &dir, &abort, &statuses, &entry.id).await;
        reporter.abort();
        println!("耗时 {:?}，结果 {:?}", t0.elapsed(), r);
        r.unwrap();
        assert!(files_installed(&entry, &dir));
        for f in &entry.files {
            assert!(verify_sha256(&dir.join(&f.name), &f.sha256).await.unwrap(), "{} SHA-256 不符", f.name);
        }
    }
}
