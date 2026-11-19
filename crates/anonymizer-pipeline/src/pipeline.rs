//! 批量处理编排：文件 → 检测 → 合并 → 映射/变换 → 写回 → 映射表落盘。
//!
//! 单文件处理链：
//! 1. `/parse` 解析出全文（字符偏移基于该全文）；
//! 2. `Detector` 正则/上下文检测 + `DictMatcher` 词典检测；
//! 3. `bind_aliases` 从全文提取"（以下简称"X"）"并绑定全称/简称到同一占位符；
//! 4. （可选）`/ner` 兜底人名/机构/地址，低于 `ner_threshold` 的丢弃；
//! 5. `merge_hits` 按类型优先级合并去重叠；
//! 6. 命中为空则跳过写回；否则按策略渲染生成替换指令（同原文同占位符），
//!    `/redact` 写回脱敏副本；
//! 7. 批处理结束后映射表落盘 `.anonymap.json`（口令存在时加密为 `.anonymap.enc`）。
//!
//! 当前为串行处理 + 进度回调；文件数多时可改用 rayon/std::thread 并行
//! （需将 progress 改为线程安全包装，如 AtomicU32 进度计数）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anonymizer_core::alias::bind_aliases;
use anonymizer_core::crypto::{aes_encrypt, base64_encode, derive_key};
use anonymizer_core::detect::Detector;
use anonymizer_core::dict::DictMatcher;
use anonymizer_core::mapping::MappingTable;
use anonymizer_core::rules::builtin_rules;
use anonymizer_core::transform::{fake_for, masked, OutputKind, Strategy};
use anonymizer_core::{Hit, HitSource, RuleDef};

use crate::merge::{default_type_priority, merge_hits};
use crate::sidecar_client::{Replacement, SidecarClient};

/// NER 统一标签 → 规则 id（供优先级表与策略表查询）。
pub fn ner_label_to_rule(label: &str) -> Option<&'static str> {
    match label {
        "人名" => Some("name"),
        "机构" => Some("org"),
        "地址" => Some("address"),
        _ => None,
    }
}

/// 管线配置（含词典匹配器，`DictMatcher` 不可 Clone/Debug，故不派生）。
pub struct PipelineConfig {
    /// 检测规则，默认 `builtin_rules()`
    pub rules: Vec<RuleDef>,
    /// 类型优先级顺序（id 列表，前高后低），默认 `default_type_priority()`
    pub type_priority: Vec<String>,
    /// NER 置信度阈值（低于则丢弃），默认 0.6
    pub ner_threshold: f64,
    /// 每规则脱敏策略（按规则 id 查询；未配置的规则默认掩码）
    pub strategy_map: HashMap<String, Strategy>,
    /// 映射表加密口令（None 则明文落盘 `.anonymap.json`）
    pub passphrase: Option<String>,
    /// NER 模型目录（None 则跳过 NER 通道）
    pub ner_model_path: Option<String>,
    /// 词典匹配器（默认空；全称/简称联动测试时注入）
    pub dict_matchers: Vec<DictMatcher>,
    /// 初始映射表（项目保险库：跨批次沿用已有占位符编号）；None = 新建
    pub initial_mapping: Option<MappingTable>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            rules: builtin_rules(),
            type_priority: default_type_priority(),
            ner_threshold: 0.6,
            strategy_map: HashMap::new(),
            passphrase: None,
            ner_model_path: None,
            dict_matchers: Vec::new(),
            initial_mapping: None,
        }
    }
}

/// 单文件处理结果。
#[derive(Debug, Clone)]
pub struct FileResult {
    pub input_path: String,
    /// 脱敏副本路径；None = 未产出（失败或未发现敏感信息）
    pub output_path: Option<String>,
    pub error: Option<String>,
    /// 合并后的命中数
    pub hit_count: usize,
    /// 实际替换的命中数（全自动确认，等于 hit_count）
    pub confirmed: usize,
    pub is_deep: bool,
    /// 未能映射到文档内的替换数（原文仍保留在输出文件中），0 = 全部落位
    pub unplaced_count: usize,
    pub warnings: Vec<String>,
}

/// 批处理结果。
#[derive(Clone)]
pub struct BatchResult {
    /// 批处理结束后的完整映射表（含初始映射 + 本批新增），供调用方存入项目保险库
    pub mapping: MappingTable,
    pub files: Vec<FileResult>,
    /// 映射表落盘路径（None = 写盘失败）
    pub anonymap_path: Option<String>,
}

/// 手写 Debug：映射表含全部原文，绝不能被 `{:?}` 打进日志/断言信息。
impl std::fmt::Debug for BatchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchResult")
            .field("files", &self.files)
            .field("anonymap_path", &self.anonymap_path)
            .field("mapping_entries", &self.mapping.len())
            .finish()
    }
}

/// 批量处理入口。
///
/// 所有文件共享同一个 [`MappingTable`]：跨文件同一原文 → 同一占位符。
/// `progress(frac, msg)` 在每处理完一个文件后回调（`frac` ∈ (0, 1]）。
pub fn run_batch(
    client: &SidecarClient,
    input_paths: &[PathBuf],
    output_dir: &Path,
    config: &PipelineConfig,
    progress: impl Fn(f32, &str),
) -> BatchResult {
    let mut mapping = config.initial_mapping.clone().unwrap_or_default();
    // 已知实体回扫：初始映射里的全部原文作为词典，新文件里无需前缀也会命中
    let known: Option<DictMatcher> = config.initial_mapping.as_ref().and_then(|m| {
        let terms: Vec<String> = m.originals().into_iter().filter(|t| t.chars().count() >= 2).collect();
        (!terms.is_empty()).then(|| DictMatcher::new("项目已知实体", terms))
    });
    let total = input_paths.len().max(1);
    let mut files = Vec::with_capacity(input_paths.len());

    for (i, path) in input_paths.iter().enumerate() {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let result = process_one(client, path, output_dir, config, known.as_ref(), &mut mapping);
        progress((i + 1) as f32 / total as f32, &name);
        files.push(result);
    }

    let anonymap_path = persist_mapping(output_dir, &mapping, config.passphrase.as_deref());
    BatchResult {
        mapping,
        files,
        anonymap_path,
    }
}

/// 单文件处理链。
fn process_one(
    client: &SidecarClient,
    input_path: &Path,
    output_dir: &Path,
    config: &PipelineConfig,
    known: Option<&DictMatcher>,
    mapping: &mut MappingTable,
) -> FileResult {
    let input_str = input_path.display().to_string();
    let mut warnings: Vec<String> = Vec::new();

    // 1. 解析
    let parse = match client.parse(input_path) {
        Ok(p) if p.ok => p,
        Ok(_) => {
            return FileResult {
                input_path: input_str,
                output_path: None,
                error: Some("sidecar 解析失败: ok=false".to_string()),
                hit_count: 0,
                confirmed: 0,
                is_deep: false,
                unplaced_count: 0,
                warnings,
            };
        }
        Err(e) => {
            return FileResult {
                input_path: input_str,
                output_path: None,
                error: Some(format!("解析失败: {e}")),
                hit_count: 0,
                confirmed: 0,
                is_deep: false,
                unplaced_count: 0,
                warnings,
            };
        }
    };
    let text = parse.text;

    // 2. 规则检测（正则 + 格式校验 + 上下文置信度提升）
    // try_detect：规则正则编译失败显性报错（宁严勿漏，不静默跳过）
    let mut rule_hits = match Detector::try_detect(&text, &config.rules) {
        Ok(h) => h,
        Err(e) => {
            warnings.push(e.clone());
            return FileResult {
                input_path: input_str,
                output_path: None,
                error: Some(e),
                hit_count: 0,
                confirmed: 0,
                is_deep: false,
                unplaced_count: 0,
                warnings,
            };
        }
    };

    // 3. 词典检测
    let mut dict_hits = Vec::new();
    for matcher in config.dict_matchers.iter().chain(known) {
        dict_hits.extend(matcher.match_all(&text));
    }

    // 统一为「字符偏移」：sidecar /redact 与 NER 均使用 Python 字符索引
    // （Rust 侧 regex / Aho-Corasick 返回的是 UTF-8 字节偏移，含中文时二者不一致）
    for h in rule_hits.iter_mut().chain(dict_hits.iter_mut()) {
        h.start = byte_to_char(&text, h.start);
        h.end = byte_to_char(&text, h.end);
    }

    // 4. 别名绑定："（以下简称"X"）" → 全称/简称同一占位符
    let file_aliases = bind_aliases(mapping, &text);

    // 5. NER 兜底（可选，模型路径不存在则跳过并告警）
    let mut ner_hits = Vec::new();
    if let Some(model_path) = config.ner_model_path.as_deref() {
        if !Path::new(model_path).join("model.onnx").is_file() {
            warnings.push(format!("NER 模型路径不存在，已跳过 NER 通道: {model_path}"));
        } else {
            match client.ner(&text, config.ner_threshold, Some(model_path)) {
                Ok(resp) if resp.ok => {
                    for e in resp.entities {
                        let Some(rule_id) = ner_label_to_rule(&e.label) else {
                            continue;
                        };
                        if e.confidence >= config.ner_threshold {
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
                Ok(resp) => warnings.push(format!(
                    "NER 不可用: {}",
                    resp.error.as_deref().unwrap_or("未知错误")
                )),
                Err(e) => warnings.push(format!("NER 请求失败: {e}")),
            }
        }
    }

    // 6. 合并（区间去重叠 + 类型优先级裁决）
    let hits = merge_hits(rule_hits, dict_hits, ner_hits, &config.type_priority);
    // 实体回扫：传播到全文其余出现处，简称并入同一实体（与应用层一致）
    let hits = anonymizer_core::propagate::propagate(&text, hits, &file_aliases);
    let hit_count = hits.len();
    if hits.is_empty() {
        warnings.push("未发现敏感信息".into());
        return FileResult {
            input_path: input_str,
            output_path: None,
            error: None,
            hit_count: 0,
            confirmed: 0,
            is_deep: false,
            unplaced_count: 0,
            warnings,
        };
    }

    // 7. 渲染替换指令（同步分配占位符 → 跨文件一致性）
    //    AES 策略需要口令派生密钥：缺失则显性报错，不静默降级为掩码。
    let session_key = session_key_for(config);
    let replacements = match build_replacements(&text, &hits, &config.strategy_map, mapping, session_key) {
        Ok(r) => r,
        Err(e) => {
            warnings.push(e.clone());
            return FileResult {
                input_path: input_str,
                output_path: None,
                error: Some(e),
                hit_count,
                confirmed: 0,
                is_deep: false,
                unplaced_count: 0,
                warnings,
            };
        }
    };

    // 8. /redact 写回
    match client.redact(input_path, output_dir, replacements) {
        Ok(resp) => {
            let mut ws = warnings;
            ws.extend(resp.warnings);
            FileResult {
                input_path: input_str,
                output_path: Some(resp.output_path),
                error: None,
                hit_count,
                confirmed: hit_count,
                is_deep: resp.is_deep,
                unplaced_count: resp.unplaced_count,
                warnings: ws,
            }
        }
        Err(e) => {
            warnings.push(format!("写回失败: {e}"));
            FileResult {
                input_path: input_str,
                output_path: None,
                error: Some(format!("脱敏写回失败: {e}")),
                hit_count,
                confirmed: 0,
                is_deep: false,
                unplaced_count: 0,
                warnings,
            }
        }
    }
}

/// 将合并后的命中渲染为替换指令（与 anonymizer-core `Transformer::apply`
/// 的过滤与渲染语义一致，额外保留偏移）。
fn build_replacements(
    text: &str,
    hits: &[Hit],
    strategies: &HashMap<String, Strategy>,
    mapping: &mut MappingTable,
    aes_key: Option<[u8; 32]>,
) -> Result<Vec<Replacement>, String> {
    // 与 Transformer::apply 相同的重叠过滤（merge 已保证无重叠，此处防御）
    let mut sorted: Vec<&Hit> = hits.iter().collect();
    sorted.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
    let mut kept: Vec<&Hit> = Vec::new();
    let mut prev_end = 0usize;
    for h in sorted {
        if h.start >= prev_end {
            kept.push(h);
            prev_end = h.end;
        }
    }

    // 命中偏移此时已是字符偏移；保结构替换需要上下文，仅在用到时才展开字符数组
    let needs_ctx = strategies
        .values()
        .any(|s| matches!(s.output, OutputKind::Scale | OutputKind::Shift));
    let chars: Vec<char> = if needs_ctx { text.chars().collect() } else { Vec::new() };

    kept.into_iter()
        .map(|h| {
            let strategy = strategies.get(&h.rule_id).cloned().unwrap_or_default();
            let (b, a) = if needs_ctx {
                anonymizer_core::shift::ctx_around_chars(&chars, h.start, h.end)
            } else {
                (String::new(), String::new())
            };
            let replacement = render_hit(h, &strategy, mapping, aes_key, (&b, &a))?;
            Ok(Replacement {
                start: h.start,
                end: h.end,
                replacement,
            })
        })
        .collect()
}

/// 渲染单个命中（与 anonymizer-core transform::Transformer::render 一致）。
fn render_hit(
    hit: &Hit,
    strategy: &Strategy,
    mapping: &mut MappingTable,
    aes_key: Option<[u8; 32]>,
    ctx: (&str, &str),
) -> Result<String, String> {
    // 保结构替换（金额缩放 / 日期平移）：走对照表，不参与占位符沿用
    if matches!(strategy.output, OutputKind::Scale | OutputKind::Shift) {
        return Ok(anonymizer_core::transform::render_shift(mapping, strategy.output, &hit.text, ctx.0, ctx.1));
    }
    // 已登记的原文沿用既有占位符（项目保险库跨批次一致；“不处理”除外）
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
            Ok(fake_for(&hit.rule_id, &hit.text, strategy, aes_key.as_ref().map(|k| k.as_slice())))
        }
        OutputKind::Keep | OutputKind::Scale | OutputKind::Shift => Ok(hit.text.clone()),
        OutputKind::Base64 => Ok(base64_encode(&hit.text)),
        OutputKind::Aes => {
            let k = aes_key.ok_or_else(|| {
                format!(
                    "规则 [{}] 配置了 AES 策略但未提供口令，已拒绝生成密文（不降级为掩码）",
                    hit.rule_id
                )
            })?;
            aes_encrypt(&hit.text, &k).map_err(|e| format!("规则 [{}] AES 加密失败: {e}", hit.rule_id))
        }
    }
}

/// 派生文档级会话密钥：策略表包含 AES 或 FakeData 且配置了口令时派生
/// （避免无谓的 PBKDF2 开销）。AES 用于加密命中文本；FakeData 用于假数据种子
/// （HMAC 化后离线无法从假数据反查原文）。
fn session_key_for(config: &PipelineConfig) -> Option<[u8; 32]> {
    let needs_key = config
        .strategy_map
        .values()
        .any(|s| matches!(s.output, OutputKind::Aes | OutputKind::FakeData));
    if !needs_key {
        return None;
    }
    config
        .passphrase
        .as_deref()
        .map(|p| derive_key(p, b"anonkit-doc-aes", 600_000))
}

// ---------------------------------------------------------------------------
// 偏移换算
// ---------------------------------------------------------------------------

/// UTF-8 字节偏移 → 字符偏移（要求 byte_offset 落在字符边界上）。
fn byte_to_char(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset].chars().count()
}

// ---------------------------------------------------------------------------
// 映射表落盘
// ---------------------------------------------------------------------------

/// 批处理结束后将映射表落盘到 `output_dir`：
/// 无口令 → 明文 `anonkit.anonymap.json`；有口令 → 加密为 `anonkit.anonymap.enc`。
///
/// 安全警示：明文路径把全部 PII 原文集中写入一个文件，泄露风险高于原始文档，
/// 仅用于便捷场景，生产建议始终设置口令走加密分支。
fn persist_mapping(
    output_dir: &Path,
    mapping: &MappingTable,
    passphrase: Option<&str>,
) -> Option<String> {
    let (filename, payload) = match passphrase {
        Some(p) => {
            let payload = mapping.export_encrypted(p).ok()?;
            ("anonkit.anonymap.enc", payload)
        }
        None => ("anonkit.anonymap.json", mapping.to_json()),
    };
    let path = output_dir.join(filename);
    std::fs::write(&path, payload).ok()?;
    Some(path.display().to_string())
}
