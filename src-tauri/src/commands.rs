//! Tauri 命令层：文档导入 / 扫描 / 确认脱敏 / 还原 / 文件选择。
//!
//! 与 UI 工作流一一对应：
//! - 导入：`pick_files` / `pick_folder` / `collect_files`（目录递归展开 + 格式过滤）
//! - 扫描：`scan_files`（解析 → 规则/词典/NER 检测 → 命中列表 + 上下文 + warning）
//! - 确认：`confirm_redact`（按勾选命中生成替换指令 → 写回副本 → 映射表落盘）
//! - 还原：`restore_text`（映射文件 + 可选口令 → 占位符还原为原文）
//! - 辅助：`get_builtin_rules` / `open_in_folder`
//!
//! 偏移约定：本层对外暴露的全部偏移均为「字符偏移」（与 sidecar Python 索引一致），
//! 内部在正则/AC 输出（UTF-8 字节偏移）后立即换算，保证 /redact 与 NER 通道一致。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anonymizer_core::alias::bind_aliases;
use anonymizer_core::crypto::base64_encode;
use anonymizer_core::detect::Detector;
use anonymizer_core::dict::DictMatcher;
use anonymizer_core::mapping::MappingTable;
use anonymizer_core::transform::{fake_for, masked, OutputKind, Strategy};
use anonymizer_core::{Hit, HitSource, RuleDef};
use anonymizer_pipeline::merge::{default_type_priority, merge_hits};
use anonymizer_pipeline::pipeline::ner_label_to_rule;
use anonymizer_pipeline::sidecar_client::{Replacement, SidecarClient, Span};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};
use tauri_plugin_opener::OpenerExt;

/// 支持的文档扩展名（与 sidecar 解析层一致）。
const SUPPORTED_EXTS: [&str; 5] = ["pdf", "docx", "xlsx", "txt", "md"];
/// 命中上下文的字符窗口。
const CTX_BEFORE: usize = 24;
const CTX_AFTER: usize = 40;

// ---------------------------------------------------------------------------
// DTO（前端 ↔ 后端）
// ---------------------------------------------------------------------------

/// 前端可序列化的脱敏策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyDto {
    /// mask / placeholder / fake / base64 / aes
    pub output: String,
    pub mask_char: char,
    pub keep_head: usize,
    pub keep_tail: usize,
}

impl From<&StrategyDto> for Strategy {
    fn from(dto: &StrategyDto) -> Self {
        let output = match dto.output.as_str() {
            "placeholder" => OutputKind::Placeholder,
            "fake" => OutputKind::FakeData,
            "base64" => OutputKind::Base64,
            "aes" => OutputKind::Aes,
            "keep" => OutputKind::Keep,
            "scale" => OutputKind::Scale,
            "shift" => OutputKind::Shift,
            _ => OutputKind::Mask,
        };
        Self {
            output,
            mask_char: dto.mask_char,
            keep_head: dto.keep_head,
            keep_tail: dto.keep_tail,
        }
    }
}

impl From<&Strategy> for StrategyDto {
    fn from(s: &Strategy) -> Self {
        let output = match s.output {
            OutputKind::Mask => "mask",
            OutputKind::Placeholder => "placeholder",
            OutputKind::FakeData => "fake",
            OutputKind::Base64 => "base64",
            OutputKind::Aes => "aes",
            OutputKind::Keep => "keep",
            OutputKind::Scale => "scale",
            OutputKind::Shift => "shift",
        };
        Self {
            output: output.to_string(),
            mask_char: s.mask_char,
            keep_head: s.keep_head,
            keep_tail: s.keep_tail,
        }
    }
}

/// 扫描配置（前端策略页下发）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    /// 检测规则（内置 + 自定义；enabled 过滤生效）
    pub rules: Vec<RuleDef>,
    /// 按规则 id 的脱敏策略（未配置默认掩码）
    pub strategies: HashMap<String, StrategyDto>,
    pub ner_threshold: f64,
    /// NER 模型目录（None = 跳过 NER 通道）
    pub ner_model_path: Option<String>,
    /// 自定义敏感词典词条
    pub dict_terms: Vec<String>,
}

/// 单条命中视图（含上下文与位置）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitView {
    /// 稳定 id：`<文件索引>:<start>`，用于前端勾选/定位
    pub id: String,
    pub rule_id: String,
    pub rule_label: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub confidence: f64,
    /// rule / dictionary / ner / context
    pub source: String,
    /// 页码（TXT/MD 恒为 0）
    pub page: u32,
    pub line: i64,
    /// 块标识（xlsx: sheet 名；docx: 正文/表格/页眉/页脚/文本框；其余为空）
    pub block: String,
    pub context_before: String,
    pub context_after: String,
}

/// 单文件扫描结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannedFile {
    pub path: String,
    pub name: String,
    pub format: String,
    /// 全文（前端渲染高亮）
    pub text: String,
    pub page_count: u32,
    pub hits: Vec<HitView>,
    pub warnings: Vec<String>,
    /// sidecar 解析结构（xlsx 表格网格等，供前端结构化预览）
    pub structure: serde_json::Value,
}

/// 扫描汇总结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub files: Vec<ScannedFile>,
    /// 简称联动（全称, 简称）对，全局去重
    pub aliases: Vec<(String, String)>,
    /// 全局警告（如 NER 模型缺失）
    pub global_warnings: Vec<String>,
}

/// 前端确认勾选后的命中（回传脱敏指令）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmedHit {
    pub rule_id: String,
    pub rule_label: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// 单个文件的确认输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactInput {
    pub path: String,
    pub hits: Vec<ConfirmedHit>,
    /// 用户**未勾选**的命中文本：与已确认命中同文的不参与残留复检（用户有意保留）
    #[serde(default)]
    pub excluded_texts: Vec<String>,
}

/// 确认脱敏配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactOptions {
    pub strategies: HashMap<String, StrategyDto>,
    /// 映射文件口令（Some 时落盘加密 .anonymap.enc）
    pub passphrase: Option<String>,
}

/// 单文件脱敏结果。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RedactFileResult {
    pub input_path: String,
    pub output_path: Option<String>,
    pub error: Option<String>,
    pub confirmed: usize,
    pub is_deep: bool,
    pub warnings: Vec<String>,
    /// 残留复检是否真正运行成功
    #[serde(default)]
    pub verified: bool,
    /// 复检发现的残留（仅部位/类型/字数/次数，不含原文）；非空时输出已被删除
    #[serde(default)]
    pub residual: Vec<ResidualView>,
    /// 未覆盖部位（图片/扫描页/嵌入对象/透视表等），不含原文
    #[serde(default)]
    pub uncovered: Vec<UncoveredView>,
    /// 输入/输出文件 SHA-256（输出被删除时为空）
    #[serde(default)]
    pub input_sha256: Option<String>,
    #[serde(default)]
    pub output_sha256: Option<String>,
    #[serde(default)]
    pub sidecar_version: Option<String>,
}

/// 残留项（给界面/报告）：只含位置、类型名、字数、次数。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResidualView {
    #[serde(rename = "where")]
    pub where_: String,
    pub rule_label: String,
    pub length: usize,
    pub count: usize,
}

pub type UncoveredView = anonymizer_pipeline::sidecar_client::UncoveredItem;

/// 确认脱敏汇总。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactResult {
    pub files: Vec<RedactFileResult>,
    /// 映射表落盘路径（None = 写盘失败）
    pub mapping_path: Option<String>,
    pub mapping_entries: usize,
    /// 映射已保存进当前打开的项目（此时不再向输出目录写映射文件）
    #[serde(default)]
    pub vault_saved: bool,
}

/// 还原结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    pub output_text: String,
    pub restored_count: usize,
    pub mapping_entries: usize,
}

/// 进度事件（`scan-progress` / `redact-progress`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub done: usize,
    pub total: usize,
    pub file: String,
    pub hit_count: usize,
    pub message: String,
}

// ---------------------------------------------------------------------------
// 文件导入
// ---------------------------------------------------------------------------

/// 多选导入文档（过滤 PDF/DOCX/TXT/MD）。
#[tauri::command]
pub async fn pick_files(app: AppHandle) -> Result<Vec<String>, String> {
    Ok(pick_files_async(
        app.dialog()
            .file()
            .set_title("选择要脱敏的文档")
            .add_filter("文档", &SUPPORTED_EXTS),
    )
    .await)
}

/// 选择文件夹（导入目录内全部支持格式）。
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    Ok(pick_one_async(
        app.dialog()
            .file()
            .set_title("选择文件夹（递归导入支持的文档）"),
        DialogKind::Folder,
    )
    .await)
}

/// 选择映射文件（还原页用）。
#[tauri::command]
pub async fn pick_mapping_file(app: AppHandle) -> Result<Option<String>, String> {
    Ok(pick_one_async(
        app.dialog()
            .file()
            .set_title("选择映射文件（.anonymap）")
            .add_filter("映射文件", &["anonymap", "json", "enc"]),
        DialogKind::File,
    )
    .await)
}

/// 导入时收集到的一个文件：路径 + 大小 + 修改时间（毫秒），前端据此算“已处理”指纹。
/// 只含文件元数据，不读取文件内容。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CollectedFile {
    pub path: String,
    pub size: u64,
    /// 修改时间（Unix 毫秒）；文件系统不支持时为 0
    pub mtime_ms: u64,
}

/// `collect_files` 返回：可导入的文件 + 文件夹展开时被排除的本工具导出产物。
#[derive(Debug, Clone, Serialize, Default)]
pub struct CollectResult {
    pub files: Vec<CollectedFile>,
    /// 文件夹展开时识别为导出产物而跳过的路径（产物文件名 / 位于已记录的输出目录 / 已记录的输出路径）
    pub skipped_artifacts: Vec<String>,
}

/// 递归展开目录，过滤出支持的文档格式（文件直接保留），去重保序。
///
/// 文件夹展开时自动排除本工具的导出产物，避免把上一次的脱敏副本又当成输入：
/// - 文件名形如 `anonkit-<8位十六进制>_脱敏[-N].ext`（sidecar 默认命名）或 `.anonkit-*` 临时文件；
/// - `exclude` 中列出的路径（前端记录过的输出文件）；或位于其中某个目录（输出目录）之下、
///   文件名带 `_脱敏` 后缀的文件。输出目录里的其他文件（例如原件与输出同目录）照常导入。
///
/// 用户直接选中/拖入的单个文件不做排除（明确选择的文件按用户意图导入）。
#[tauri::command]
pub fn collect_files(paths: Vec<String>, exclude: Option<Vec<String>>) -> Result<CollectResult, String> {
    let exclude: Vec<PathBuf> = exclude
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    let mut out: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for p in &paths {
        let pb = PathBuf::from(p);
        if pb.is_dir() {
            collect_dir(&pb, &mut out, &mut skipped, &exclude);
        } else if is_supported(&pb) {
            push_unique(&mut out, pb.display().to_string());
        }
    }
    let files = out
        .into_iter()
        .map(|path| {
            let meta = std::fs::metadata(&path).ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime_ms = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            CollectedFile { path, size, mtime_ms }
        })
        .collect();
    Ok(CollectResult { files, skipped_artifacts: skipped })
}

/// 文件名是否为本工具的导出产物：`anonkit-<8位十六进制>_脱敏[-N].ext` 或 `.anonkit-*` 临时文件。
fn is_export_artifact_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with(".anonkit-") {
        return true;
    }
    let Some(rest) = name.strip_prefix("anonkit-") else {
        return false;
    };
    let stem = match rest.rfind('.') {
        Some(i) => &rest[..i],
        None => rest,
    };
    let Some((hash, tail)) = stem.split_once("_脱敏") else {
        return false;
    };
    let hash_ok = hash.len() == 8 && hash.chars().all(|c| c.is_ascii_hexdigit());
    // 重名时 sidecar 追加 -2、-3…
    let tail_ok = tail.is_empty()
        || tail
            .strip_prefix('-')
            .map(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(false);
    hash_ok && tail_ok
}

/// 文件名主干是否以 `_脱敏` 或 `_脱敏-N` 结尾（保留原名导出时的产物命名）。
fn has_redacted_suffix(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|n| n.to_str()) else {
        return false;
    };
    match stem.rfind("_脱敏") {
        Some(i) => {
            let tail = &stem[i + "_脱敏".len()..];
            tail.is_empty()
                || tail
                    .strip_prefix('-')
                    .map(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                    .unwrap_or(false)
        }
        None => false,
    }
}

/// 是否命中前端给出的排除列表：
/// - 等于某个已记录的输出文件路径；
/// - 位于某个已记录的输出目录之下，且文件名带 `_脱敏` 产物后缀。
///   （输出目录可能就是原件所在目录，所以不能把整个目录一刀切排除。）
fn is_excluded(path: &Path, exclude: &[PathBuf]) -> bool {
    exclude.iter().any(|e| {
        path == e.as_path() || (path.starts_with(e) && path != e.as_path() && has_redacted_suffix(path))
    })
}

/// 选择输出目录（脱敏副本落盘位置）。
#[tauri::command]
pub async fn pick_output_dir(app: AppHandle) -> Result<Option<String>, String> {
    pick_folder(app).await
}

/// 已安装的 NER 模型目录（未安装返回 None）。
#[tauri::command]
pub fn ner_model_path(app: AppHandle) -> Option<String> {
    let dir = crate::models::DownloadManager::install_dir(&app, crate::models::NER_MODEL_ID);
    dir.join("model.onnx")
        .is_file()
        .then(|| dir.display().to_string())
}

/// 保存文本到用户选择的位置（导出日志 / 还原文档用）。
#[tauri::command]
pub async fn save_text_file(
    app: AppHandle,
    default_name: String,
    content: String,
) -> Result<Option<String>, String> {
    let Some(path) = save_file_async(
        app.dialog()
            .file()
            .set_title("保存文件")
            .set_file_name(default_name)
            .add_filter("文本", &["txt", "md", "csv", "html"]),
    )
    .await
    else {
        return Ok(None);
    };
    std::fs::write(&path, content).map_err(|e| format!("写入失败: {e}"))?;
    Ok(Some(path.display().to_string()))
}

// ---------------------------------------------------------------------------
// 异步对话框封装
// ---------------------------------------------------------------------------

/// 原生对话框类型（单文件 / 文件夹）。
enum DialogKind {
    File,
    Folder,
}

/// 多选文件对话框（回调版 API 包装为 async）。
///
/// macOS 的原生对话框依赖主线程 run loop 派发事件；同步命令运行在主线程上，
/// 调用 `blocking_*` 会死锁（弹窗卡死、整个应用无响应）。异步命令运行在
/// Tauri 的 async runtime 工作线程，插件内部会把原生面板调度回主线程，
/// 因此这里用 oneshot 等待回调结果，不占用主线程。
async fn pick_files_async(builder: FileDialogBuilder<tauri::Wry>) -> Vec<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    builder.pick_files(move |paths| {
        let out = paths
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| p.into_path().ok())
            .map(|p| p.display().to_string())
            .collect();
        let _ = tx.send(out);
    });
    rx.await.unwrap_or_default()
}

/// 单文件 / 文件夹对话框（回调版 API 包装为 async）。
async fn pick_one_async(
    builder: FileDialogBuilder<tauri::Wry>,
    kind: DialogKind,
) -> Option<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    match kind {
        DialogKind::File => builder.pick_file(move |p| {
            let _ = tx.send(p.and_then(|p| p.into_path().ok()));
        }),
        DialogKind::Folder => builder.pick_folder(move |p| {
            let _ = tx.send(p.and_then(|p| p.into_path().ok()));
        }),
    }
    rx.await.unwrap_or(None).map(|p| p.display().to_string())
}

/// 保存文件对话框（回调版 API 包装为 async），返回用户确认的路径。
async fn save_file_async(builder: FileDialogBuilder<tauri::Wry>) -> Option<PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    builder.save_file(move |p| {
        let _ = tx.send(p.and_then(|p| p.into_path().ok()));
    });
    rx.await.unwrap_or(None)
}

/// 在系统文件管理器中显示指定路径。
#[tauri::command]
pub fn open_in_folder(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| format!("无法打开目录: {e}"))
}

fn collect_dir(dir: &Path, out: &mut Vec<String>, skipped: &mut Vec<String>, exclude: &[PathBuf]) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_dir(&path, out, skipped, exclude);
        } else if is_supported(&path) || is_export_artifact_name(&path) {
            if is_export_artifact_name(&path) || is_excluded(&path, exclude) {
                // 临时文件（.tmp.docx 等）也算产物，但只统计支持格式的，避免噪声
                if is_supported(&path) {
                    push_unique(skipped, path.display().to_string());
                }
            } else {
                push_unique(out, path.display().to_string());
            }
        }
    }
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn push_unique(out: &mut Vec<String>, s: String) {
    if !out.contains(&s) {
        out.push(s);
    }
}

// ---------------------------------------------------------------------------
// 内置规则
// ---------------------------------------------------------------------------

/// 返回内置规则集（前端策略页初始化用）。
#[tauri::command]
pub fn get_builtin_rules() -> Vec<RuleDef> {
    anonymizer_core::rules::builtin_rules()
}

// ---------------------------------------------------------------------------
// 扫描
// ---------------------------------------------------------------------------

/// 解析 + 检测单批文件，返回命中列表与全文（供预览页展示/勾选）。
#[tauri::command]
pub async fn scan_files(
    app: AppHandle,
    paths: Vec<String>,
    options: ScanOptions,
) -> Result<ScanResult, String> {
    // 同步命令会在主线程执行并阻塞界面（blocking reqwest 最长等 300s）：
    // 放到阻塞线程池，界面与进度事件保持响应。
    // 每次调用都让代次 +1：正在进行的旧扫描在下一个文件边界发现代次变化即停止。
    // 传入空路径列表即“取消当前扫描”（前端的取消按钮走这里，不需要额外命令）。
    let generation = next_scan_generation();
    if paths.is_empty() {
        return Ok(ScanResult { files: vec![], aliases: vec![], global_warnings: vec![] });
    }
    with_sidecar(app, "扫描任务异常终止", move |app, client| {
        scan_files_blocking(app, client, paths, options, generation)
    })
    .await
}

/// 扫描代次：新扫描（或取消请求）会使旧扫描失效。
static SCAN_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn next_scan_generation() -> u64 {
    SCAN_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

/// 本次扫描是否已被更新的扫描/取消请求取代。
fn scan_superseded(generation: u64) -> bool {
    SCAN_GENERATION.load(std::sync::atomic::Ordering::SeqCst) != generation
}

/// 扫描被取消时返回给前端的错误文本（前端据此区分“取消”与“失败”）。
pub const SCAN_CANCELLED: &str = "扫描已取消";

fn scan_files_blocking(
    app: AppHandle,
    client: SidecarClient,
    paths: Vec<String>,
    options: ScanOptions,
    generation: u64,
) -> Result<ScanResult, String> {
    let rules: Vec<RuleDef> = options
        .rules
        .iter()
        .filter(|r| r.enabled)
        .cloned()
        .collect();
    let type_priority = default_type_priority();
    // 词典 = 全局词典 ∪ 当前打开项目的案件词典
    let mut all_terms: Vec<String> = options.dict_terms.clone();
    {
        let vs = app.state::<VaultState>();
        let g = vs.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = g.as_ref() {
            all_terms.extend(s.vault.dict_terms.iter().cloned());
            // 已知实体回扫：项目映射里登记过的全部原文，在新文件里无需前缀也会被命中（跨批次一致）
            all_terms.extend(s.vault.mapping.originals().into_iter().filter(|t| t.chars().count() >= 2));
        }
    }
    let dict_matcher = all_terms.iter().any(|t| !t.is_empty()).then(|| {
        DictMatcher::new(
            "自定义词典",
            all_terms.iter().filter(|s| !s.is_empty()).cloned().collect(),
        )
    });

    // 别名绑定需要一个共享映射表（扫描期仅收集，不落盘）
    let mut mapping = MappingTable::new();
    let mut aliases: Vec<(String, String)> = Vec::new();
    let mut global_warnings: Vec<String> = Vec::new();
    let mut files: Vec<ScannedFile> = Vec::new();
    let total = paths.len().max(1);

    for (idx, path) in paths.iter().enumerate() {
        if scan_superseded(generation) {
            return Err(SCAN_CANCELLED.into());
        }
        let pb = PathBuf::from(path);
        let name = file_name(&pb);
        let mut warnings: Vec<String> = Vec::new();

        let parse = match client.parse(&pb) {
            Ok(p) if p.ok => p,
            Ok(_) => {
                warn_once(
                    &mut global_warnings,
                    &mut warnings,
                    &name,
                    "sidecar 解析失败",
                );
                emit_progress(&app, "scan-progress", idx + 1, total, &name, 0, "解析失败");
                continue;
            }
            Err(e) => {
                warn_once(
                    &mut global_warnings,
                    &mut warnings,
                    &name,
                    &format!("解析失败: {e}"),
                );
                emit_progress(&app, "scan-progress", idx + 1, total, &name, 0, "解析失败");
                continue;
            }
        };
        let text = parse.text;

        // 1. 规则检测（正则 + 上下文置信度提升），字节偏移 → 字符偏移
        // 逐规则隔离：坏规则只让该规则失效，且必须显性告警，
        // 绝不能因此给出“未发现敏感信息”的虚假结论。
        // 字节→字符偏移表只建一次（O(n)），避免每个命中从头数字符（O(n·命中数)）
        let b2c = ByteCharMap::new(&text);
        let (mut rule_hits, rule_errors) = Detector::detect_isolated(&text, &rules);
        let mut incomplete = !rule_errors.is_empty();
        for err in &rule_errors {
            warn_once(
                &mut global_warnings,
                &mut warnings,
                &name,
                &format!("检测规则失效（已跳过）: {err}"),
            );
        }
        for h in rule_hits.iter_mut() {
            h.start = b2c.char_at(h.start);
            h.end = b2c.char_at(h.end);
        }

        // 2. 词典检测
        let mut dict_hits: Vec<Hit> = Vec::new();
        if let Some(m) = &dict_matcher {
            dict_hits = m.match_all(&text);
            for h in dict_hits.iter_mut() {
                h.start = b2c.char_at(h.start);
                h.end = b2c.char_at(h.end);
            }
        }

        // 3. NER 兜底（可选）
        let mut ner_hits: Vec<Hit> = Vec::new();
        if let Some(model_path) = options.ner_model_path.as_deref() {
            if !Path::new(model_path).join("model.onnx").is_file() {
                // NER 失效必须显性告警：用户不能误以为"没有 NER 可检出的内容"而放行文档
                warn_once(
                    &mut global_warnings,
                    &mut warnings,
                    &name,
                    "NER 模型未安装，已跳过 NER 通道（可在模型市场下载）",
                );
                incomplete = true;
            } else {
                match client.ner(&text, options.ner_threshold, Some(model_path)) {
                    Ok(resp) if resp.ok => {
                        for e in resp.entities {
                            // 统一映射（与 pipeline 共用唯一实现）：未知标签跳过，绝不猜测归类
                            let Some(rule_id) = ner_label_to_rule(&e.label) else {
                                continue;
                            };
                            if e.confidence >= options.ner_threshold {
                                ner_hits.push(Hit {
                                    rule_id: rule_id.to_string(),
                                    rule_label: e.label,
                                    start: e.start,
                                    end: e.end,
                                    text: e.text,
                                    confidence: e.confidence,
                                    source: HitSource::Ner,
                                });
                            }
                        }
                    }
                    Ok(resp) => {
                        incomplete = true;
                        warn_once(
                            &mut global_warnings,
                            &mut warnings,
                            &name,
                            &format!(
                                "NER 不可用: {}",
                                resp.error.as_deref().unwrap_or("未知错误")
                            ),
                        )
                    }
                    Err(e) => {
                        incomplete = true;
                        warn_once(
                            &mut global_warnings,
                            &mut warnings,
                            &name,
                            &format!("NER 请求失败: {e}"),
                        )
                    }
                }
            }
        }

        // 4. 别名绑定（简称联动提示）
        let file_aliases = bind_aliases(&mut mapping, &text);
        for (full, short) in file_aliases.iter().cloned() {
            if !aliases.contains(&(full.clone(), short.clone())) {
                aliases.push((full, short));
            }
        }

        // 5. 合并去重叠
        let hits = merge_hits(rule_hits, dict_hits, ner_hits, &type_priority);
        // 实体回扫：已命中的姓名/公司/机构/地址传播到全文其余出现处，简称并入同一实体
        let hits = anonymizer_core::propagate::propagate(&text, hits, &file_aliases);
        if text.trim().is_empty() {
            // 没有可提取文本（典型：扫描件/纯图片 PDF）≠ 没有敏感信息
            warnings.push(
                "未能提取到文字（可能是扫描件或图片），本工具无法检测其内容，请勿视为安全，外发前请人工核对".into(),
            );
        } else if hits.is_empty() {
            if incomplete {
                warnings.push(
                    "无法确认：部分检测通道失效或被跳过，未命中不代表没有敏感信息，请勿直接外发".into(),
                );
            } else {
                warnings.push("在已检测的文字内容中未发现命中项（图片、批注等未覆盖部位不在检测范围内）".into());
            }
        }

        // 6. 组装视图（上下文 + 位置）
        let mut views = Vec::with_capacity(hits.len());
        let chars: Vec<char> = text.chars().collect(); // 上下文窗口共用，只收集一次
        for h in &hits {
            views.push(hit_view(&format!("{idx}"), h, &chars, &parse.spans));
        }

        emit_progress(
            &app,
            "scan-progress",
            idx + 1,
            total,
            &name,
            hits.len(),
            "扫描完成",
        );
        files.push(ScannedFile {
            path: pb.display().to_string(),
            name,
            format: parse.format,
            text,
            page_count: page_count(&parse.spans),
            hits: views,
            warnings,
            structure: parse.structure,
        });
    }

    Ok(ScanResult {
        files,
        aliases,
        global_warnings,
    })
}

fn hit_view(file_idx: &str, hit: &Hit, chars: &[char], spans: &[Span]) -> HitView {
    let (page, line, block) = locate(hit.start, spans);
    let (before, after) = context_window(chars, hit.start, hit.end);
    HitView {
        id: format!("{file_idx}:{}", hit.start),
        rule_id: hit.rule_id.clone(),
        rule_label: hit.rule_label.clone(),
        start: hit.start,
        end: hit.end,
        text: hit.text.clone(),
        confidence: hit.confidence,
        source: source_str(hit.source),
        page,
        line,
        block,
        context_before: before,
        context_after: after,
    }
}

/// 取命中前/后上下文（字符窗口）。
fn context_window(chars: &[char], start: usize, end: usize) -> (String, String) {
    let start = start.min(chars.len());
    let before_start = start.saturating_sub(CTX_BEFORE);
    let before: String = chars[before_start..start].iter().collect();
    let after_end = (end + CTX_AFTER).min(chars.len());
    let after: String = chars[end.min(chars.len())..after_end].iter().collect();
    (before, after)
}

fn source_str(s: HitSource) -> String {
    match s {
        HitSource::Rule => "rule",
        HitSource::Dictionary => "dictionary",
        HitSource::Context => "context",
        HitSource::Ner => "ner",
        HitSource::Name => "name",
    }
    .to_string()
}

/// 根据字符偏移在 span 流中定位页码/行号。
fn locate(offset: usize, spans: &[Span]) -> (u32, i64, String) {
    // spans 按 span_start 升序：二分定位，避免每个命中线性扫描
    let idx = spans.partition_point(|s| s.span_start <= offset);
    if idx > 0 {
        let s = &spans[idx - 1];
        if offset < s.span_end {
            return (s.page, s.line, s.block.clone());
        }
    }
    for s in spans {
        if s.span_start <= offset && offset < s.span_end {
            return (s.page, s.line, s.block.clone());
        }
    }
    // 兜底：取起点最接近的 span
    spans
        .iter()
        .filter(|s| s.span_start <= offset)
        .max_by_key(|s| s.span_start)
        .map(|s| (s.page, s.line, s.block.clone()))
        .unwrap_or((0, -1, String::new()))
}

fn page_count(spans: &[Span]) -> u32 {
    spans
        .iter()
        .map(|s| s.page)
        .max()
        .map(|p| p + 1)
        .unwrap_or(1)
}

/// 字节偏移 → 字符偏移的预计算表：构建 O(n)，查询 O(1)。
struct ByteCharMap(Vec<u32>);

impl ByteCharMap {
    fn new(text: &str) -> Self {
        let mut v = vec![0u32; text.len() + 1];
        let mut n = 0u32;
        let mut prev = 0usize;
        for (b, _) in text.char_indices().skip(1) {
            for slot in &mut v[prev..b] {
                *slot = n;
            }
            n += 1;
            prev = b;
        }
        // 最后一个字符起点到末尾（含 text.len()）
        if !text.is_empty() {
            for slot in &mut v[prev..text.len()] {
                *slot = n;
            }
            n += 1;
        }
        v[text.len()] = n;
        Self(v)
    }

    fn char_at(&self, byte_offset: usize) -> usize {
        self.0[byte_offset.min(self.0.len() - 1)] as usize
    }
}

// ---------------------------------------------------------------------------
// 确认脱敏
// ---------------------------------------------------------------------------

/// 按前端勾选的命中执行脱敏写回，并落盘映射表。
#[tauri::command]
pub async fn confirm_redact(
    app: AppHandle,
    inputs: Vec<RedactInput>,
    output_dir: String,
    options: RedactOptions,
) -> Result<RedactResult, String> {
    with_sidecar(app, "导出任务异常终止", move |app, client| {
        confirm_redact_blocking(app, client, inputs, output_dir, options)
    })
    .await
}

fn confirm_redact_blocking(
    app: AppHandle,
    client: SidecarClient,
    inputs: Vec<RedactInput>,
    output_dir: String,
    options: RedactOptions,
) -> Result<RedactResult, String> {
    let strategies: HashMap<String, Strategy> = options
        .strategies
        .iter()
        .map(|(k, v)| (k.clone(), Strategy::from(v)))
        .collect();
    let out_dir = PathBuf::from(&output_dir);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("创建输出目录失败: {e}"))?;

    // 跨文件共享映射表：同一原文 → 同一占位符
    // 打开了项目：用项目里的映射表（跨批次编号稳定、不再每批新建/覆盖）；否则沿用旧行为
    let vault_state = app.state::<VaultState>();
    let mut vault_guard = vault_state.0.lock().unwrap_or_else(|p| p.into_inner());
    let vault_active = vault_guard.is_some();
    let mut mapping = match vault_guard.as_ref() {
        Some(s) => s.vault.mapping.clone(),
        None => MappingTable::new(),
    };
    let entries_before = mapping.len();
    // 有效口令：显式口令优先，其次项目口令（仅内存）
    let eff_pass: Option<String> = options
        .passphrase
        .clone()
        .filter(|p| !p.is_empty())
        .or_else(|| vault_guard.as_ref().map(|s| s.passphrase.clone()));
    let mut files: Vec<RedactFileResult> = Vec::new();
    let total = inputs.len().max(1);

    // 会话密钥：整批只派生一次（PBKDF2 60 万轮，之前在逐文件循环里重复计算）。
    // - 有口令：由口令派生（AES / 一致性假数据）；
    // - 无口令且用到“假数据”：用系统随机数生成一次性会话密钥——假数据在本批内保持一致，
    //   但不再由原文哈希派生（旧实现可被离线穷举反推手机号等小取值空间的原文）；
    // - 无口令且用到 AES：保持 None，由渲染阶段显性报错（不降级）。
    let needs_key = strategies
        .values()
        .any(|s| matches!(s.output, OutputKind::Aes | OutputKind::FakeData));
    let uses_aes = strategies.values().any(|s| matches!(s.output, OutputKind::Aes));
    let session_key: Option<[u8; 32]> = if needs_key {
        match eff_pass.as_deref() {
            Some(p) => Some(anonymizer_core::crypto::derive_key(p, b"anonkit-doc-aes", 600_000)),
            None if !uses_aes => {
                use rand::RngCore;
                let mut k = [0u8; 32];
                rand::rngs::OsRng.fill_bytes(&mut k);
                Some(k)
            }
            None => None,
        }
    } else {
        None
    };

    for (i, input) in inputs.iter().enumerate() {
        let pb = PathBuf::from(&input.path);
        let name = file_name(&pb);
        if input.hits.is_empty() {
            files.push(RedactFileResult {
                input_path: input.path.clone(),
                output_path: None,
                error: None,
                confirmed: 0,
                is_deep: false,
                warnings: vec!["未勾选命中，跳过".into()],
                ..Default::default()
            });
            emit_progress(
                &app,
                "redact-progress",
                i + 1,
                total,
                &name,
                0,
                "跳过（未勾选）",
            );
            continue;
        }

        let parse = match client.parse(&pb) {
            Ok(p) if p.ok => p,
            Ok(_) => {
                files.push(err_result(&input.path, "sidecar 解析失败: ok=false"));
                continue;
            }
            Err(e) => {
                files.push(err_result(&input.path, &format!("解析失败: {e}")));
                continue;
            }
        };

        // 原文一致性校验：确认时重新解析的全文必须与扫描期命中文本一致，
        // 否则（文件被外部修改/误选同名文件）按陈旧偏移替换会错位脱敏或写坏文档。
        // 偏移为字符索引，按字符迭代切片（不能直接 &text[a..b]，那是字节索引）。
        let parse_chars: Vec<char> = parse.text.chars().collect();
        let mismatch: Option<String> = input.hits.iter().find_map(|h| {
            let in_bounds = h.start <= h.end && h.end <= parse_chars.len();
            let slice: String = if in_bounds {
                parse_chars[h.start..h.end].iter().collect()
            } else {
                String::new()
            };
            (slice != h.text).then(|| {
                // 错误文本会进入界面/审计：不得拼入命中原文，只给位置
                format!(
                    "文件内容与扫描结果不一致（位置 {}–{} 处的文本已变化），请重新扫描后再确认",
                    h.start, h.end
                )
            })
        });
        if let Some(msg) = mismatch {
            files.push(err_result(&input.path, &msg));
            emit_progress(
                &app,
                "redact-progress",
                i + 1,
                total,
                &name,
                0,
                "已跳过（原文不一致）",
            );
            continue;
        }

        // 先绑定别名，再渲染占位符（保证全称/简称同一编号）
        bind_aliases(&mut mapping, &parse.text);

        let mut replacements = Vec::with_capacity(input.hits.len());
        let mut sorted_hits: Vec<&ConfirmedHit> = input.hits.iter().collect();
        sorted_hits.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
        let mut prev_end = 0usize;
        let mut render_err: Option<String> = None;
        for h in sorted_hits {
            if h.start < prev_end {
                continue; // 重叠防御：跳过被前一个覆盖的
            }
            prev_end = h.end;
            let strategy = strategies.get(&h.rule_id).cloned().unwrap_or_default();
            if matches!(strategy.output, OutputKind::Keep) {
                continue; // “不处理”：保留原文，不生成替换
            }
            // 保结构替换需要上下文（排除百分比/费率/条款号、法规日期）；偏移为字符索引
            let ctx = if matches!(strategy.output, OutputKind::Scale | OutputKind::Shift) {
                anonymizer_core::shift::ctx_around_chars(&parse_chars, h.start, h.end)
            } else {
                (String::new(), String::new())
            };
            match render_confirmed(h, &strategy, &mut mapping, session_key.as_ref(), (&ctx.0, &ctx.1)) {
                Ok(replacement) => replacements.push(Replacement {
                    start: h.start,
                    end: h.end,
                    replacement,
                }),
                Err(e) => {
                    // AES 等策略缺口令：显性报错，不做任何降级
                    render_err = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = render_err {
            files.push(err_result(&input.path, &e));
            emit_progress(
                &app,
                "redact-progress",
                i + 1,
                total,
                &name,
                0,
                "已跳过（策略错误）",
            );
            continue;
        }

        // 复检用原文：已确认命中的文本（去重），剔除用户有意保留（未勾选）的同文本
        let excluded: std::collections::HashSet<&str> =
            input.excluded_texts.iter().map(|s| s.as_str()).collect();
        let mut needles: Vec<String> = Vec::new();
        let mut needle_labels: Vec<String> = Vec::new();
        for h in &input.hits {
            let kept = strategies
                .get(&h.rule_id)
                .is_some_and(|s| matches!(s.output, OutputKind::Keep | OutputKind::Scale | OutputKind::Shift));
            if !kept && !excluded.contains(h.text.as_str()) && !needles.contains(&h.text) {
                needles.push(h.text.clone());
                needle_labels.push(h.rule_label.clone());
            }
        }
        // 先落项目、后写输出：保证“输出里有占位符但映射丢了”不会发生（崩溃/断电时最多多一条未用的映射）
        if let Some(s) = vault_guard.as_mut() {
            s.vault.mapping = mapping.clone();
            if let Err(e) = s.vault.save() {
                files.push(err_result(
                    &input.path,
                    &format!("项目保存失败，已停止写出该文件（避免产生无法还原的脱敏稿）: {e}"),
                ));
                emit_progress(&app, "redact-progress", i + 1, total, &name, 0, "已跳过（项目保存失败）");
                continue;
            }
        }
        match client.redact_verified(&pb, &out_dir, replacements, &needles) {
            Ok(resp) if !resp.residual.is_empty() => {
                // 硬阻断：已确认要脱敏的原文仍在输出里 → 删除输出，不交付
                let _ = std::fs::remove_file(&resp.output_path);
                let places: Vec<String> = resp
                    .residual
                    .iter()
                    .map(|r| format!("{}（{}字，{}处）", r.where_, r.length, r.count))
                    .collect();
                let mut fr = err_result(
                    &input.path,
                    &format!(
                        "复检发现已确认的原文仍保留在输出中，已阻止导出（输出已删除）。残留位置: {}",
                        places.join("；")
                    ),
                );
                fr.verified = true;
                fr.residual = resp
                    .residual
                    .iter()
                    .map(|r| ResidualView {
                        where_: r.where_.clone(),
                        rule_label: needle_labels.get(r.index).cloned().unwrap_or_default(),
                        length: r.length,
                        count: r.count,
                    })
                    .collect();
                fr.uncovered = resp.uncovered.clone();
                fr.input_sha256 = Some(resp.input_sha256.clone()).filter(|s| !s.is_empty());
                fr.sidecar_version = Some(resp.sidecar_version.clone()).filter(|s| !s.is_empty());
                files.push(fr);
            }
            Ok(resp) => {
                let mut warnings = resp.warnings;
                if !resp.verified && !needles.is_empty() {
                    warnings.push("残留复检未能运行，无法确认输出中没有原文，请勿直接外发".into());
                }
                files.push(RedactFileResult {
                    input_path: input.path.clone(),
                    output_path: Some(resp.output_path),
                    error: None,
                    confirmed: input.hits.len(),
                    // 复检未运行时不得宣称“深度”
                    is_deep: resp.is_deep && (resp.verified || needles.is_empty()),
                    warnings,
                    verified: resp.verified,
                    residual: Vec::new(),
                    uncovered: resp.uncovered,
                    input_sha256: Some(resp.input_sha256).filter(|s| !s.is_empty()),
                    output_sha256: Some(resp.output_sha256).filter(|s| !s.is_empty()),
                    sidecar_version: Some(resp.sidecar_version).filter(|s| !s.is_empty()),
                });
            }
            Err(e) => files.push(err_result(&input.path, &format!("脱敏写回失败: {e}"))),
        }
        emit_progress(
            &app,
            "redact-progress",
            i + 1,
            total,
            &name,
            input.hits.len(),
            "已写回",
        );
    }

    // 映射表落盘：打开了项目 → 写入项目（并记录批次）；否则按口令导出加密映射文件
    let mut vault_saved = false;
    let mapping_path = if vault_active {
        if let Some(s) = vault_guard.as_mut() {
            s.vault.mapping = mapping.clone();
            let ok_files = files.iter().filter(|f| f.error.is_none() && f.output_path.is_some()).count();
            s.vault.record_batch(ok_files, entries_before);
            vault_saved = s.vault.save().is_ok();
        }
        None
    } else {
        persist_mapping(&out_dir, &mapping, options.passphrase.as_deref())
    };

    Ok(RedactResult {
        files,
        mapping_path,
        mapping_entries: mapping.len(),
        vault_saved,
    })
}

fn err_result(input_path: &str, msg: &str) -> RedactFileResult {
    RedactFileResult {
        input_path: input_path.to_string(),
        output_path: None,
        error: Some(msg.to_string()),
        confirmed: 0,
        is_deep: false,
        warnings: vec![msg.to_string()],
        ..Default::default()
    }
}

/// 渲染单个确认命中（与 pipeline 语义一致）。
///
/// AES 策略需要口令派生密钥：缺失则显性报错，不静默降级为掩码。
fn render_confirmed(
    hit: &ConfirmedHit,
    strategy: &Strategy,
    mapping: &mut MappingTable,
    session_key: Option<&[u8; 32]>,
    ctx: (&str, &str),
) -> Result<String, String> {
    // 保结构替换（金额缩放 / 日期平移）：走对照表
    if matches!(strategy.output, OutputKind::Scale | OutputKind::Shift) {
        return Ok(anonymizer_core::transform::render_shift(mapping, strategy.output, &hit.text, ctx.0, ctx.1));
    }
    // 已在映射表（项目）里登记过的原文：沿用既有占位符，保证跨批次一致（“不处理”类型除外）
    if !matches!(strategy.output, OutputKind::Keep) {
        if let Some(p) = mapping.placeholder_of(&hit.text) {
            return Ok(p.to_string());
        }
    }
    match strategy.output {
        OutputKind::Mask => Ok(masked(&hit.text, strategy)),
        OutputKind::Placeholder => Ok(mapping.placeholder_for(&hit.rule_label, &hit.text)),
        OutputKind::FakeData => {
            // 有会话密钥时假数据种子用 HMAC（离线不可反查）；无口令时保持确定性
            Ok(fake_for(
                &hit.rule_id,
                &hit.text,
                strategy,
                session_key.as_ref().map(|k| k.as_slice()),
            ))
        },
        OutputKind::Keep | OutputKind::Scale | OutputKind::Shift => Ok(hit.text.clone()),
        OutputKind::Base64 => Ok(base64_encode(&hit.text)),
        OutputKind::Aes => {
            let k = session_key.ok_or_else(|| {
                format!(
                    "规则 [{}] 配置了 AES 策略但未提供口令，已拒绝生成密文（不降级为掩码）",
                    hit.rule_id
                )
            })?;
            anonymizer_core::crypto::aes_encrypt(&hit.text, k)
                .map_err(|e| format!("规则 [{}] AES 加密失败: {e}", hit.rule_id))
        }
    }
}

/// 映射表落盘：仅在设置口令时写出加密文件 `anonkit.anonymap.enc`；无口令不写任何文件。
///
/// 明文映射把全部 PII 原文集中到一个文件，泄露风险高于原始文档，因此不再支持。
fn persist_mapping(
    output_dir: &Path,
    mapping: &MappingTable,
    passphrase: Option<&str>,
) -> Option<String> {
    // 映射表等同于全部原文：绝不以明文写盘（更不能写进要发给第三方的输出目录）。
    // 未设置口令时不生成映射文件，由调用方向用户明示“脱敏稿将无法还原”。
    let p = passphrase.filter(|p| !p.is_empty())?;
    let payload = mapping.export_encrypted(p).ok()?;
    let path = output_dir.join("anonkit.anonymap.enc");
    std::fs::write(&path, payload).ok()?;
    Some(path.display().to_string())
}

// ---------------------------------------------------------------------------
// 还原
// ---------------------------------------------------------------------------

/// 用映射文件把文本中的占位符还原为原文（口令可选）。
#[tauri::command]
pub fn restore_text(
    app: AppHandle,
    text: String,
    mapping_path: String,
    passphrase: Option<String>,
) -> Result<RestoreResult, String> {
    let mut mapping = MappingTable::new();
    if mapping_path.trim().is_empty() {
        // 使用当前打开的项目
        let vs = app.state::<VaultState>();
        let g = vs.0.lock().unwrap_or_else(|p| p.into_inner());
        match g.as_ref() {
            Some(s) => mapping = s.vault.mapping.clone(),
            None => return Err("未选择映射文件，也没有打开的项目".into()),
        }
    } else {
        let payload =
            std::fs::read_to_string(&mapping_path).map_err(|e| format!("读取映射文件失败: {e}"))?;
        match passphrase.filter(|p| !p.is_empty()) {
            Some(p) => mapping.import_encrypted(&payload, &p)?,
            None => mapping.from_json(&payload)?,
        }
    }

    // 统计文本中实际出现的占位符数量
    let restored_count = mapping
        .placeholder_to_original()
        .keys()
        .filter(|p| text.contains(p.as_str()))
        .count();

    Ok(RestoreResult {
        output_text: mapping.restore(&text),
        restored_count,
        mapping_entries: mapping.len(),
    })
}


// ---------------------------------------------------------------------------
// 容错还原 / 文档还原（新增；不改动上面的 restore_text）
// ---------------------------------------------------------------------------

/// 读取还原用映射：mapping_path 为空时取当前打开的项目，否则读文件（可选口令）。
fn load_restore_mapping(
    app: &AppHandle,
    mapping_path: &str,
    passphrase: Option<String>,
) -> Result<MappingTable, String> {
    let mut mapping = MappingTable::new();
    if mapping_path.trim().is_empty() {
        let vs = app.state::<VaultState>();
        let g = vs.0.lock().unwrap_or_else(|p| p.into_inner());
        match g.as_ref() {
            Some(s) => mapping = s.vault.mapping.clone(),
            None => return Err("未选择映射文件，也没有打开的项目".into()),
        }
    } else {
        let payload =
            std::fs::read_to_string(mapping_path).map_err(|e| format!("读取映射文件失败: {e}"))?;
        match passphrase.filter(|p| !p.is_empty()) {
            Some(p) => mapping.import_encrypted(&payload, &p)?,
            None => mapping.from_json(&payload)?,
        }
    }
    Ok(mapping)
}

/// 容错还原文本：精确/模糊分别计数，模糊命中逐条返回，另给疑似未还原占位符清单。
#[tauri::command]
pub fn restore_text_fuzzy(
    app: AppHandle,
    text: String,
    mapping_path: String,
    passphrase: Option<String>,
) -> Result<anonymizer_core::mapping::FuzzyRestoreResult, String> {
    let mapping = load_restore_mapping(&app, &mapping_path, passphrase)?;
    Ok(mapping.restore_fuzzy(&text))
}

/// 选择待还原的 docx / xlsx 文档。
#[tauri::command]
pub async fn pick_restore_doc(app: AppHandle) -> Result<Option<String>, String> {
    Ok(pick_one_async(
        app.dialog()
            .file()
            .set_title("选择待还原的文档（.docx / .xlsx）")
            .add_filter("Word / Excel", &["docx", "xlsx"]),
        DialogKind::File,
    )
    .await)
}

/// 文档还原：映射在内存里经请求体交给 sidecar，另存新文件到输出目录（不覆盖输入）。
#[tauri::command]
pub async fn restore_document(
    app: AppHandle,
    input_path: String,
    output_dir: String,
    mapping_path: String,
    passphrase: Option<String>,
) -> Result<serde_json::Value, String> {
    let mapping = load_restore_mapping(&app, &mapping_path, passphrase)?;
    let table = mapping.placeholder_to_original().clone();
    with_sidecar(app, "文档还原任务异常", move |_, client| {
        client
            .restore_doc(
                std::path::Path::new(&input_path),
                std::path::Path::new(&output_dir),
                &table,
            )
            .map_err(|e| format!("文档还原失败: {e}"))
    })
    .await
}

// ---------------------------------------------------------------------------
// 项目保险库（Vault）
// ---------------------------------------------------------------------------

/// 当前打开的项目：保险库 + 口令（口令只存内存，随关闭/退出丢弃）。
pub struct VaultSession {
    pub vault: anonymizer_core::vault::Vault,
    pub passphrase: String,
}

/// 应用状态：至多一个打开的项目。
pub struct VaultState(pub Mutex<Option<VaultSession>>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultInfo {
    pub name: String,
    pub path: String,
    pub project_id: String,
    pub created: u64,
    pub entries: usize,
    pub dict_terms: Vec<String>,
    /// 最近的批次（新的在前，最多 20 条）
    pub batches: Vec<anonymizer_core::vault::BatchRecord>,
}

fn vault_info_of(s: &VaultSession) -> VaultInfo {
    let mut batches = s.vault.batches.clone();
    batches.reverse();
    batches.truncate(20);
    VaultInfo {
        name: s.vault.name.clone(),
        path: s.vault.path.display().to_string(),
        project_id: s.vault.project_id.clone(),
        created: s.vault.created,
        entries: s.vault.mapping.len(),
        dict_terms: s.vault.dict_terms.clone(),
        batches,
    }
}

/// 文件名安全化：去掉路径分隔符与控制字符。
fn safe_file_stem(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let s = s.trim_matches('.').trim().to_string();
    if s.is_empty() { "项目".to_string() } else { s }
}

#[tauri::command]
pub async fn pick_vault_file(app: AppHandle) -> Result<Option<String>, String> {
    Ok(pick_one_async(
        app.dialog()
            .file()
            .set_title("打开项目文件（.akcase）")
            .add_filter("墨隐 InkVeil 项目", &["akcase"]),
        DialogKind::File,
    )
    .await)
}

/// 新建项目：在目录 `dir` 下创建 `<名称>.akcase`（重名自动加序号，不覆盖已有项目），并打开它。
#[tauri::command]
pub async fn vault_create(
    app: AppHandle,
    dir: String,
    name: String,
    passphrase: String,
) -> Result<VaultInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let stem = safe_file_stem(&name);
        let mut path = PathBuf::from(&dir).join(format!("{stem}.akcase"));
        let mut n = 2;
        while path.exists() {
            path = PathBuf::from(&dir).join(format!("{stem}-{n}.akcase"));
            n += 1;
        }
        let vault = anonymizer_core::vault::Vault::create(&path, &name, &passphrase)?;
        let session = VaultSession { vault, passphrase };
        let info = vault_info_of(&session);
        let vs = app.state::<VaultState>();
        *vs.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(session);
        Ok(info)
    })
    .await
    .map_err(|e| format!("创建项目任务异常终止: {e}"))?
}

#[tauri::command]
pub async fn vault_open(app: AppHandle, path: String, passphrase: String) -> Result<VaultInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let vault = anonymizer_core::vault::Vault::open(Path::new(&path), &passphrase)?;
        let session = VaultSession { vault, passphrase };
        let info = vault_info_of(&session);
        let vs = app.state::<VaultState>();
        *vs.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(session);
        Ok(info)
    })
    .await
    .map_err(|e| format!("打开项目任务异常终止: {e}"))?
}

#[tauri::command]
pub fn vault_info(app: AppHandle) -> Option<VaultInfo> {
    let vs = app.state::<VaultState>();
    let g = vs.0.lock().unwrap_or_else(|p| p.into_inner());
    g.as_ref().map(vault_info_of)
}

/// 关闭项目：口令与映射从内存丢弃。
#[tauri::command]
pub fn vault_close(app: AppHandle) {
    let vs = app.state::<VaultState>();
    *vs.0.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// 保存案件词典（去空、去重、保序）。
#[tauri::command]
pub fn vault_set_dict(app: AppHandle, terms: Vec<String>) -> Result<VaultInfo, String> {
    let vs = app.state::<VaultState>();
    let mut g = vs.0.lock().unwrap_or_else(|p| p.into_inner());
    let s = g.as_mut().ok_or("没有打开的项目")?;
    let mut seen = std::collections::HashSet::new();
    s.vault.dict_terms = terms
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty() && seen.insert(t.clone()))
        .collect();
    s.vault.save()?;
    Ok(vault_info_of(s))
}

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// 异步等待引擎就绪，在阻塞线程中构建、使用和销毁 HTTP 客户端。
async fn with_sidecar<T: Send + 'static>(
    app: AppHandle,
    task_error: &str,
    run: impl FnOnce(AppHandle, SidecarClient) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    use crate::sidecar::{StartupStatus, StartupWatch};

    let mut startup = app.state::<StartupWatch>().subscribe();
    let status = startup
        .wait_for(|s| *s != StartupStatus::Starting)
        .await
        .map_err(|_| "处理引擎启动状态丢失，请重启应用".to_string())?
        .clone();
    if let StartupStatus::Failed(reason) = status {
        return Err(format!(
            "处理引擎启动失败，请重启应用；仍失败请把这段信息反馈给开发者：{reason}"
        ));
    }

    let (base_url, token) = {
        let state = app.state::<Mutex<Option<crate::sidecar::SidecarManager>>>();
        let guard = state.lock().unwrap_or_else(|p| p.into_inner());
        let manager = guard.as_ref().ok_or("处理引擎已停止，请重启应用")?;
        (manager.base_url().to_owned(), manager.token().to_owned())
    };

    // reqwest::blocking 在构建客户端时也会阻塞，必须在阻塞线程中调用。
    tauri::async_runtime::spawn_blocking(move || {
        let client = SidecarClient::with_token(base_url, Some(token));
        run(app, client)
    })
    .await
    .map_err(|e| format!("{task_error}: {e}"))?
}

fn emit_progress(
    app: &AppHandle,
    event: &str,
    done: usize,
    total: usize,
    file: &str,
    hit_count: usize,
    message: &str,
) {
    let _ = app.emit(
        event,
        ProgressEvent {
            done,
            total,
            file: file.to_string(),
            hit_count,
            message: message.to_string(),
        },
    );
}

fn file_name(pb: &Path) -> String {
    pb.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| pb.display().to_string())
}

/// 全局警告只在首见时记录一次，避免批量重复。
fn warn_once(global: &mut Vec<String>, local: &mut Vec<String>, file: &str, msg: &str) {
    let full = format!("[{file}] {msg}");
    if !global.contains(&full) {
        global.push(full.clone());
    }
    local.push(full);
}

#[cfg(test)]
mod offset_tests {
    use super::*;

    #[test]
    fn byte_char_map_matches_naive() {
        let text = "ab张三c😀d";
        let m = ByteCharMap::new(text);
        for (b, _) in text.char_indices() {
            assert_eq!(m.char_at(b), text[..b].chars().count(), "byte {b}");
        }
        assert_eq!(m.char_at(text.len()), text.chars().count());
        assert_eq!(ByteCharMap::new("").char_at(0), 0);
    }
}

#[cfg(test)]
mod persist_tests {
    use super::*;

    #[test]
    fn mapping_never_written_in_plaintext() {
        let dir = std::env::temp_dir().join(format!("anonkit_persist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = MappingTable::new();
        m.placeholder_for("姓名", "陈瑾瑜");
        // 无口令（含空口令）：不生成任何文件
        assert!(persist_mapping(&dir, &m, None).is_none());
        assert!(persist_mapping(&dir, &m, Some("")).is_none());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "无口令时输出目录必须为空");
        // 有口令：写出加密文件，内容不含原文
        let p = persist_mapping(&dir, &m, Some("pw")).expect("应写出加密映射");
        assert!(p.ends_with(".enc"));
        let bytes = std::fs::read(&p).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("陈瑾瑜"), "密文中不得出现原文");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod collect_tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("anonkit_collect_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn artifact_name_pattern() {
        let yes = [
            "anonkit-1a2b3c4d_脱敏.docx",
            "anonkit-ABCDEF01_脱敏-2.pdf",
            ".anonkit-0123456789abcdef.tmp.docx",
        ];
        for n in yes {
            assert!(is_export_artifact_name(Path::new(n)), "{n} 应识别为产物");
        }
        let no = [
            "合同.docx",
            "anonkit-说明.docx",
            "anonkit-1a2b3c4_脱敏.docx",   // 7 位
            "anonkit-1a2b3c4z_脱敏.docx",  // 非十六进制
            "anonkit-1a2b3c4d_脱敏-x.docx",
            "anonkit-1a2b3c4d_草稿.docx",
            "张三_脱敏.docx",              // keep_name 产物：靠输出目录排除，不按名字猜
        ];
        for n in no {
            assert!(!is_export_artifact_name(Path::new(n)), "{n} 不应识别为产物");
        }
        assert!(has_redacted_suffix(Path::new("张三_脱敏.docx")));
        assert!(has_redacted_suffix(Path::new("张三_脱敏-3.docx")));
        assert!(!has_redacted_suffix(Path::new("脱敏说明.docx")));
        assert!(!has_redacted_suffix(Path::new("张三_脱敏版.docx")));
    }

    #[test]
    fn folder_import_skips_artifacts_and_output_dirs() {
        let d = tmpdir("skip");
        std::fs::write(d.join("合同.docx"), b"x").unwrap();
        std::fs::write(d.join("说明.txt"), b"hello").unwrap();
        std::fs::write(d.join("anonkit-1a2b3c4d_脱敏.docx"), b"x").unwrap();
        std::fs::write(d.join(".anonkit-ff00.tmp.pdf"), b"x").unwrap();
        std::fs::write(d.join("图片.png"), b"x").unwrap();
        let out = d.join("输出");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("张三_脱敏.docx"), b"x").unwrap();
        std::fs::write(out.join("原件.docx"), b"x").unwrap();
        std::fs::write(d.join("单独记录的输出.md"), b"x").unwrap();

        let r = collect_files(
            vec![d.display().to_string()],
            Some(vec![out.display().to_string(), d.join("单独记录的输出.md").display().to_string()]),
        )
        .unwrap();
        let mut names: Vec<String> = r
            .files
            .iter()
            .map(|f| Path::new(&f.path).file_name().unwrap().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["原件.docx".to_string(), "合同.docx".to_string(), "说明.txt".to_string()],
            "输出目录里的非产物文件照常导入"
        );
        assert_eq!(r.skipped_artifacts.len(), 4, "{:?}", r.skipped_artifacts);
        let txt = r.files.iter().find(|f| f.path.ends_with("说明.txt")).unwrap();
        assert_eq!(txt.size, 5);
        assert!(txt.mtime_ms > 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn explicit_files_are_kept_and_deduped() {
        let d = tmpdir("explicit");
        let a = d.join("anonkit-1a2b3c4d_脱敏.docx");
        std::fs::write(&a, b"x").unwrap();
        let p = a.display().to_string();
        let r = collect_files(vec![p.clone(), p.clone()], None).unwrap();
        assert_eq!(r.files.len(), 1, "用户明确选择的文件不排除，且去重");
        assert!(r.skipped_artifacts.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod scan_generation_tests {
    use super::*;

    #[test]
    fn newer_generation_supersedes_older() {
        let a = next_scan_generation();
        assert!(!scan_superseded(a));
        let b = next_scan_generation(); // 新扫描或取消请求
        assert!(scan_superseded(a), "旧扫描应被取代");
        assert!(!scan_superseded(b));
    }
}
