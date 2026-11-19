// 主流程（导入 → 扫描 → 预览 → 导出）的纯函数：无界面、无 Tauri 依赖，可直接用 node --test 测试。
// - 已处理记录：路径 + 大小 + 修改时间 作为指纹；只存元数据，绝不存原文。
// - 导入分流：新文件 / 已处理（默认跳过）/ 内容已变化（视为新文件）。
// - 预览：同一原文按实体汇总、筛选、预计剩余时间。
import type { HitView, ScannedFile } from "../types.ts";

// ---------------------------------------------------------------------------
// 已处理记录
// ---------------------------------------------------------------------------

/** 导入时由后端 collect_files 给出的文件元数据（不含内容） */
export interface FileMeta {
  path: string;
  size: number;
  mtime_ms: number;
}

export type VerdictLevel = "green" | "yellow" | "red";

/** 一条已处理记录。只含路径/指纹/时间/输出路径，不含任何文档内容。 */
export interface ProcessedRecord {
  fp: string;
  path: string;
  size: number;
  mtime_ms: number;
  /** 处理完成时间（毫秒时间戳） */
  processed_at: number;
  output_path: string | null;
  /** 是否为带风险放行导出 */
  overridden: boolean;
  level: VerdictLevel | null;
}

export interface ProcessedStore {
  version: 1;
  records: ProcessedRecord[];
}

export const PROCESSED_KEY = "anonkit.processed.v1";
/** 最多保留的记录数（按处理时间保留最新的） */
export const MAX_RECORDS = 2000;

export function emptyProcessedStore(): ProcessedStore {
  return { version: 1, records: [] };
}

/** Windows 路径大小写不敏感、分隔符可能混用：归一后再比较 */
export function normPath(p: string): string {
  const isWin = /^[a-zA-Z]:[\\/]/.test(p) || p.startsWith("\\\\");
  return isWin ? p.replace(/\//g, "\\").toLowerCase() : p;
}

/** 文件指纹：路径 + 大小 + 修改时间。内容变化（大小或修改时间变）即视为新文件。 */
export function fingerprint(m: FileMeta): string {
  return `${normPath(m.path)}|${m.size}|${m.mtime_ms}`;
}

/** 文件所在目录（兼容 / 与 \） */
export function dirName(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i > 0 ? p.slice(0, i) : p;
}

export function baseName(p: string): string {
  return p.split(/[\\/]/).pop() ?? p;
}

const LEVELS: VerdictLevel[] = ["green", "yellow", "red"];

/**
 * 解析持久化的已处理记录。版本不认识或结构损坏 → 空记录（宁可重新处理也不误跳过）。
 * 只取已知字段，丢弃其它一切（防止历史版本或外部写入夹带内容）。
 */
export function parseProcessedStore(raw: string | null | undefined): ProcessedStore {
  if (!raw) return emptyProcessedStore();
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return emptyProcessedStore();
  }
  if (!parsed || typeof parsed !== "object") return emptyProcessedStore();
  const obj = parsed as { version?: unknown; records?: unknown };
  if (obj.version !== 1 || !Array.isArray(obj.records)) return emptyProcessedStore();
  const records: ProcessedRecord[] = [];
  for (const r of obj.records as Record<string, unknown>[]) {
    if (!r || typeof r !== "object") continue;
    if (typeof r.path !== "string" || typeof r.size !== "number" || typeof r.mtime_ms !== "number") continue;
    const meta = { path: r.path, size: r.size, mtime_ms: r.mtime_ms };
    records.push({
      ...meta,
      fp: fingerprint(meta),
      processed_at: typeof r.processed_at === "number" ? r.processed_at : 0,
      output_path: typeof r.output_path === "string" ? r.output_path : null,
      overridden: r.overridden === true,
      level: LEVELS.includes(r.level as VerdictLevel) ? (r.level as VerdictLevel) : null,
    });
  }
  return { version: 1, records };
}

export function serializeProcessedStore(s: ProcessedStore): string {
  return JSON.stringify({ version: 1, records: s.records });
}

export interface BatchEntry {
  meta: FileMeta;
  output_path: string | null;
  overridden: boolean;
  level: VerdictLevel | null;
}

/** 记录一批导出成功的文件：同一路径只保留最新一条；超过上限丢最旧的。 */
export function recordBatch(store: ProcessedStore, entries: BatchEntry[], now: number): ProcessedStore {
  const touched = new Set(entries.map((e) => normPath(e.meta.path)));
  const kept = store.records.filter((r) => !touched.has(normPath(r.path)));
  const added: ProcessedRecord[] = entries.map((e) => ({
    ...e.meta,
    fp: fingerprint(e.meta),
    processed_at: now,
    output_path: e.output_path,
    overridden: e.overridden,
    level: e.level,
  }));
  const records = [...kept, ...added]
    .sort((a, b) => b.processed_at - a.processed_at)
    .slice(0, MAX_RECORDS);
  return { version: 1, records };
}

/** 移除指定路径的记录（“仍要重新处理”后又导出时会被新记录覆盖；这里供清空用） */
export function forgetPaths(store: ProcessedStore, paths: string[]): ProcessedStore {
  const drop = new Set(paths.map(normPath));
  return { version: 1, records: store.records.filter((r) => !drop.has(normPath(r.path))) };
}

/** 给后端 collect_files 的排除列表：已记录的输出文件 + 它们所在的输出目录 */
export function excludeListFor(store: ProcessedStore): string[] {
  const out = new Set<string>();
  for (const r of store.records) {
    if (!r.output_path) continue;
    out.add(r.output_path);
    out.add(dirName(r.output_path));
  }
  return [...out];
}

/** 已记录的输出文件集合（归一路径），用于排除用户直接选中的导出产物 */
export function outputPathSet(store: ProcessedStore): Set<string> {
  return new Set(store.records.flatMap((r) => (r.output_path ? [normPath(r.output_path)] : [])));
}

export interface ImportPartition {
  /** 可以导入的新文件（含内容已变化的） */
  fresh: (FileMeta & { changed: boolean })[];
  /** 指纹一致、已处理过 → 默认跳过 */
  processed: { meta: FileMeta; record: ProcessedRecord }[];
  /** 直接选中的、已记录过的输出文件（上一次的脱敏副本） */
  outputs: FileMeta[];
  /** 已在当前列表中的重复项 */
  duplicates: number;
}

/**
 * 导入分流。
 * - reprocess=true：已处理过的文件也作为新文件导入（“仍要重新处理”）；
 * - 同一路径但指纹变了（大小/修改时间不同）：视为新文件，标记 changed；
 * - 已在当前列表中的路径：计为重复，不再加入。
 */
export function partitionImports(
  metas: FileMeta[],
  store: ProcessedStore,
  opts: { reprocess: boolean; existing: string[] },
): ImportPartition {
  const byFp = new Map(store.records.map((r) => [r.fp, r]));
  const byPath = new Map(store.records.map((r) => [normPath(r.path), r]));
  const outputs = outputPathSet(store);
  const existing = new Set(opts.existing.map(normPath));
  const seen = new Set<string>();
  const res: ImportPartition = { fresh: [], processed: [], outputs: [], duplicates: 0 };
  for (const m of metas) {
    const key = normPath(m.path);
    if (existing.has(key) || seen.has(key)) {
      res.duplicates++;
      continue;
    }
    seen.add(key);
    if (outputs.has(key)) {
      res.outputs.push(m);
      continue;
    }
    const rec = byFp.get(fingerprint(m));
    if (rec && !opts.reprocess) {
      res.processed.push({ meta: m, record: rec });
      continue;
    }
    const prev = byPath.get(key);
    res.fresh.push({ ...m, changed: !!prev && prev.fp !== fingerprint(m) });
  }
  return res;
}

// ---------------------------------------------------------------------------
// 扫描进度
// ---------------------------------------------------------------------------

/** 预计剩余秒数；样本不足（未完成任何文件）返回 null */
export function estimateRemaining(startedAt: number, now: number, done: number, total: number): number | null {
  if (done <= 0 || total <= 0 || now <= startedAt) return null;
  if (done >= total) return 0;
  const per = (now - startedAt) / done;
  return Math.max(0, Math.round((per * (total - done)) / 1000));
}

export function formatEta(sec: number | null): string {
  if (sec == null) return "正在估算剩余时间…";
  if (sec <= 0) return "即将完成";
  if (sec < 60) return `预计还需 ${sec} 秒`;
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return `预计还需 ${m} 分${s ? ` ${s} 秒` : ""}`;
}

export function formatSize(bytes: number): string {
  if (!bytes || bytes < 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(bytes < 10 * 1024 ? 1 : 0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

// ---------------------------------------------------------------------------
// 预览：实体汇总与筛选
// ---------------------------------------------------------------------------

export interface HitRef {
  fileIdx: number;
  hit: HitView;
}

/** 同一类型 + 同一原文合并成一个实体 */
export interface Entity {
  key: string;
  text: string;
  ruleId: string;
  ruleLabel: string;
  refs: HitRef[];
  fileCount: number;
  minConf: number;
  sources: string[];
}

export function entityKey(h: HitView): string {
  return `${h.rule_id}\u0000${h.text.trim()}`;
}

/** 按实体汇总（保持首次出现顺序；同组内按出现次数降序） */
export function groupEntities(files: ScannedFile[]): Entity[] {
  const map = new Map<string, Entity>();
  const fileSets = new Map<string, Set<number>>();
  files.forEach((f, fi) => {
    for (const h of f.hits) {
      const key = entityKey(h);
      let e = map.get(key);
      if (!e) {
        e = { key, text: h.text.trim(), ruleId: h.rule_id, ruleLabel: h.rule_label, refs: [], fileCount: 0, minConf: 1, sources: [] };
        map.set(key, e);
        fileSets.set(key, new Set());
      }
      e.refs.push({ fileIdx: fi, hit: h });
      fileSets.get(key)!.add(fi);
      e.minConf = Math.min(e.minConf, h.confidence);
      if (!e.sources.includes(h.source)) e.sources.push(h.source);
    }
  });
  for (const [k, e] of map) e.fileCount = fileSets.get(k)!.size;
  return [...map.values()];
}

export interface TypeGroup {
  ruleId: string;
  label: string;
  entities: Entity[];
  hitCount: number;
}

/** 实体再按类型分组（类型按首次出现顺序，组内实体按出现次数降序） */
export function groupByType(entities: Entity[]): TypeGroup[] {
  const order: string[] = [];
  const map = new Map<string, TypeGroup>();
  for (const e of entities) {
    let g = map.get(e.ruleId);
    if (!g) {
      g = { ruleId: e.ruleId, label: e.ruleLabel, entities: [], hitCount: 0 };
      map.set(e.ruleId, g);
      order.push(e.ruleId);
    }
    g.entities.push(e);
    g.hitCount += e.refs.length;
  }
  return order.map((id) => {
    const g = map.get(id)!;
    g.entities.sort((a, b) => b.refs.length - a.refs.length);
    return g;
  });
}

export type ConfState = "all" | "none" | "some";

export function entityState(e: Entity, confirmed: Record<string, boolean>): ConfState {
  let on = 0;
  for (const r of e.refs) if (confirmed[r.hit.id]) on++;
  return on === 0 ? "none" : on === e.refs.length ? "all" : "some";
}

/** 整个实体一键确认/取消：部分或全未确认 → 全部确认；全部已确认 → 全部取消 */
export function toggleEntity(e: Entity, confirmed: Record<string, boolean>): Record<string, boolean> {
  const turnOn = entityState(e, confirmed) !== "all";
  const next = { ...confirmed };
  for (const r of e.refs) next[r.hit.id] = turnOn;
  return next;
}

export interface HitFilter {
  query: string;
  ruleIds: string[]; // 空 = 全部类型
  sources: string[]; // 空 = 全部来源
  onlyLowConf: boolean;
  onlyUnconfirmed: boolean;
}

export const EMPTY_FILTER: HitFilter = { query: "", ruleIds: [], sources: [], onlyLowConf: false, onlyUnconfirmed: false };

export function isFilterActive(f: HitFilter): boolean {
  return !!f.query.trim() || f.ruleIds.length > 0 || f.sources.length > 0 || f.onlyLowConf || f.onlyUnconfirmed;
}

/** 筛选实体（搜索匹配原文或类型名；低置信度阈值 lowConf） */
export function filterEntities(
  entities: Entity[],
  f: HitFilter,
  confirmed: Record<string, boolean>,
  lowConf: number,
): Entity[] {
  const q = f.query.trim().toLowerCase();
  return entities.filter((e) => {
    if (q && !e.text.toLowerCase().includes(q) && !e.ruleLabel.toLowerCase().includes(q)) return false;
    if (f.ruleIds.length && !f.ruleIds.includes(e.ruleId)) return false;
    if (f.sources.length && !e.sources.some((s) => f.sources.includes(s))) return false;
    if (f.onlyLowConf && e.minConf >= lowConf) return false;
    if (f.onlyUnconfirmed && entityState(e, confirmed) === "all") return false;
    return true;
  });
}

/** 已确认命中数 + 受影响（至少一处已确认）的文件数 */
export function confirmedStats(files: ScannedFile[], confirmed: Record<string, boolean>): { hits: number; files: number } {
  let hits = 0;
  let nf = 0;
  for (const f of files) {
    let any = false;
    for (const h of f.hits) {
      if (confirmed[h.id]) {
        hits++;
        any = true;
      }
    }
    if (any) nf++;
  }
  return { hits, files: nf };
}

/** 置信度分档（颜色之外同时给文字，颜色不是唯一信息） */
export function confTier(c: number): { tier: "high" | "mid" | "low"; label: string } {
  if (c >= 0.9) return { tier: "high", label: "高" };
  if (c >= 0.7) return { tier: "mid", label: "中" };
  return { tier: "low", label: "低" };
}

export const SOURCE_NAME: Record<string, string> = {
  rule: "规则",
  dictionary: "词典",
  dict: "词典",
  ner: "模型",
  context: "上下文",
};

export function sourceName(s: string): string {
  return SOURCE_NAME[s] ?? s;
}
