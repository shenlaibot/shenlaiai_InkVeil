// 与 Rust 命令层（src-tauri/src/commands.rs / models.rs）对应的类型定义。

/** collect_files 返回的单个文件元数据（不含内容） */
export interface CollectedFile {
  path: string;
  size: number;
  mtime_ms: number;
}

/** collect_files 返回：可导入文件 + 文件夹展开时排除的导出产物 */
export interface CollectResult {
  files: CollectedFile[];
  skipped_artifacts: string[];
}

/** 内置检测规则 */
export interface RuleDef {
  id: string;
  label: string;
  regex: string;
  enabled: boolean;
  confidence: number;
  context_keywords: string[];
}

/** 每规则的脱敏策略 */
export interface StrategyDto {
  output: "mask" | "placeholder" | "fake" | "base64" | "aes" | "keep" | "scale" | "shift";
  mask_char: string;
  keep_head: number;
  keep_tail: number;
}

/** 扫描配置 */
export interface ScanOptions {
  rules: RuleDef[];
  strategies: Record<string, StrategyDto>;
  ner_threshold: number;
  ner_model_path: string | null;
  dict_terms: string[];
}

/** 单条命中视图 */
export interface HitView {
  id: string;
  rule_id: string;
  rule_label: string;
  start: number;
  end: number;
  text: string;
  confidence: number;
  source: string; // rule / dictionary / ner / context
  page: number;
  line: number;
  block: string; // xlsx: sheet 名；docx: 正文/表格/页眉等
  context_before: string;
  context_after: string;
}

/** 单文件扫描结果 */
export interface ScannedFile {
  path: string;
  name: string;
  format: string;
  text: string;
  page_count: number;
  hits: HitView[];
  warnings: string[];
  structure?: {
    type?: string;
    sheets?: { name: string; max_row: number; max_col: number; rows?: ([string, number, number] | null)[][] }[];
    [k: string]: unknown;
  };
}

/** 扫描汇总 */
export interface ScanResult {
  files: ScannedFile[];
  aliases: [string, string][];
  global_warnings: string[];
}

/** 勾选确认后的命中 */
export interface ConfirmedHit {
  rule_id: string;
  rule_label: string;
  start: number;
  end: number;
  text: string;
}

export interface RedactInput {
  path: string;
  hits: ConfirmedHit[];
  /** 用户未勾选的命中文本（残留复检时视为有意保留） */
  excluded_texts: string[];
}

/** 确认脱敏配置 */
export interface RedactOptions {
  strategies: Record<string, StrategyDto>;
  /** 映射文件口令（Some 时落盘加密 .anonymap.enc） */
  passphrase: string | null;
}

export interface RedactFileResult {
  input_path: string;
  output_path: string | null;
  error: string | null;
  confirmed: number;
  is_deep: boolean;
  warnings: string[];
  /** 残留复检是否真正运行成功 */
  verified?: boolean;
  /** 复检发现的残留（只含部位/类型/字数/次数，绝不含原文）；非空时输出已被删除 */
  residual?: ResidualView[];
  /** 本工具未处理/无法检测的部位（图片、扫描页、嵌入对象、透视表…） */
  uncovered?: UncoveredItem[];
  input_sha256?: string | null;
  output_sha256?: string | null;
  sidecar_version?: string | null;
}

export interface ResidualView {
  where: string;
  rule_label: string;
  length: number;
  count: number;
}

export type UncoveredKind =
  | "scanned_page"
  | "image"
  | "embedded"
  | "pivot"
  | "external_link"
  | "custom_xml"
  | "other";

export interface UncoveredItem {
  where: string;
  kind: UncoveredKind | string;
  count: number;
}

export interface RedactResult {
  files: RedactFileResult[];
  mapping_path: string | null;
  mapping_entries: number;
  /** 映射已保存进当前打开的项目（此时不再向输出目录写映射文件） */
  vault_saved?: boolean;
}

/** 项目批次记录（不含原文） */
export interface BatchRecord {
  seq: number;
  time: number; // Unix 秒
  files: number;
  entries_added: number;
}

/** 当前打开的项目（保险库）摘要 */
export interface VaultInfo {
  name: string;
  path: string;
  project_id: string;
  created: number;
  entries: number;
  dict_terms: string[];
  batches: BatchRecord[];
}

export interface RestoreResult {
  output_text: string;
  restored_count: number;
  mapping_entries: number;
}

/** 一处模糊命中：AI 改了占位符写法，但仍唯一对应映射表。偏移均为字符（码点）偏移。 */
export interface FuzzyHit {
  start: number;
  end: number;
  out_start: number;
  out_end: number;
  raw: string;
  placeholder: string;
  original: string;
}

/** 疑似未还原的占位符（形如 [xxx数字] 但不在映射表里）。 */
export interface SuspectPlaceholder {
  start: number;
  end: number;
  text: string;
}

export interface FuzzyRestoreResult {
  output_text: string;
  exact_count: number;
  fuzzy_count: number;
  fuzzy_hits: FuzzyHit[];
  suspects: SuspectPlaceholder[];
}

/** 文档（docx/xlsx）还原结果；where 为命中所在位置（段落标签/单元格等）。 */
export interface DocRestoreResult {
  ok: boolean;
  output_path: string;
  exact_count: number;
  fuzzy_count: number;
  fuzzy_hits: { raw: string; placeholder: string; original: string; where: string }[];
  suspects: { text: string; where: string }[];
  mapping_entries: number;
}

export interface ProgressEvent {
  done: number;
  total: number;
  file: string;
  hit_count: number;
  message: string;
}

/** 模型市场条目 */
export interface ModelFile {
  name: string;
  size: number;
  sha256: string;
}

export interface ModelEntry {
  id: string;
  name: string;
  description: string;
  version: string;
  size_bytes: number;
  base_url: string;
  /** 备用下载源（主源失败时自动改用） */
  mirrors?: string[];
  license: string;
  files: ModelFile[];
}

export interface DownloadStatus {
  model_id: string;
  state: string; // downloading / paused / completed / failed / idle
  received: number;
  total: number;
  error: string | null;
  /** 过程提示：当前下载源、重试/换源原因（失败时 error 才是最终原因） */
  note?: string | null;
  installed: boolean;
}

/** 审计日志条目（会话级，Task 8 落库） */
export interface AuditEntry {
  time: string;
  file: string;
  hit_count: number;
  types: string;
  output: string;
  mapping: string;
  is_deep: boolean;
  /** 例外放行理由等备注（不含原文） */
  note?: string;
  /** 以下链字段由后端写入时计算，前端只读 */
  seq?: number;
  /** entry 脱敏操作 / prune 修剪记录 / migration 旧库补链标记 */
  kind?: "entry" | "prune" | "migration";
  prev_hash?: string;
  hash?: string;
}

/** 审计哈希链校验结果 */
export interface AuditVerify {
  ok: boolean;
  count: number;
  broken_seq: number | null;
  reason: string;
  pruned_total: number;
}
