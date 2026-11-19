// 全局状态（应用级 context）：导航 / 文件 / 扫描结果 / 勾选 / 配置 / 审计。
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import type {
  AuditEntry,
  CollectResult,
  RedactResult,
  RuleDef,
  ScanResult,
  StrategyDto,
  VaultInfo,
} from "./types";
import {
  auditAdd,
  auditList,
  collectFiles,
  configGet,
  configSet,
  getBuiltinRules,
  vaultInfo,
} from "./api";
import type { ExportSnapshot } from "./components/verdict";
import type { ProcessedStore, VerdictLevel } from "./components/workflow";
import {
  PROCESSED_KEY,
  baseName,
  emptyProcessedStore,
  excludeListFor,
  fingerprint,
  normPath,
  parseProcessedStore,
  partitionImports,
  recordBatch,
  serializeProcessedStore,
} from "./components/workflow";

export type PageId = "workbench" | "preview" | "strategy" | "vault" | "models" | "restore" | "audit" | "about";

const SUPPORTED_FORMATS = ["pdf", "docx", "xlsx", "txt", "md"];

function formatOf(name: string): string {
  const ext = name.includes(".") ? (name.split(".").pop() ?? "").toLowerCase() : "";
  return SUPPORTED_FORMATS.includes(ext) ? ext : "未知";
}

export interface ImportedFile {
  path: string;
  name: string;
  format: string;
  size: number;
  mtime_ms: number;
  /** 同一路径曾处理过，但大小/修改时间变了（视为新文件） */
  changed?: boolean;
  /** 用户选择了“仍要重新处理”（曾处理过、内容未变） */
  reprocess?: boolean;
}

/** 导入时被跳过的文件（只用于展示与“仍要重新处理”，不参与扫描） */
export interface SkippedFile {
  path: string;
  name: string;
  format: string;
  size: number;
  mtime_ms: number;
  /** processed=已处理过；output=上一次导出的脱敏副本；artifact=文件夹中的导出产物；unsupported=格式不支持 */
  reason: "processed" | "output" | "artifact" | "unsupported";
  processedAt?: number;
  outputPath?: string | null;
}

/** 最近一次导入的结果摘要（工作台提示条用） */
export interface ImportOutcome {
  added: number;
  changed: number;
  processed: number;
  outputs: number;
  artifacts: number;
  unsupported: number;
  duplicates: number;
}

/** 上一批导出（工作台“上一批已完成”卡片 + 结论页） */
export interface LastBatch {
  time: number;
  result: RedactResult;
  snapshot: ExportSnapshot;
  counts: Record<VerdictLevel, number>;
  exported: number;
  /** 未生成脱敏副本的文件（失败/复检阻止/未勾选），可一键重新导入 */
  notExported: { path: string; name: string }[];
  outputDir: string | null;
}

export type ExportState = "idle" | "exporting";

export interface AppConfig {
  rules: RuleDef[];
  strategies: Record<string, StrategyDto>;
  maskChar: string;
  dictTerms: string[];
  nerEnabled: boolean;
  nerThreshold: number;
  /** 隐私开关：关闭后模型市场禁止一切下载（含断点续传），实现「唯一网络用途可关闭」 */
  allowModelDownload: boolean;
  passphrase: string | null;
  /**
   * 内置规则模式版本。内置规则正则更新后递增该版本，
   * 启动时据此用最新 builtin 定义刷新对应 id 的规则（保留用户开关），
   * 修复旧版本持久化规则导致「正则修复不生效」的问题。
   */
  rulesSchema: number;
}

/**
 * “保结构”输出（金额按比例缩放 / 日期整体平移）只对这两类规则开放。
 * 仅是可选项：默认策略不变（金额/日期默认仍是“不处理”）。
 */
export const STRUCTURE_PRESERVING: Record<string, "scale" | "shift"> = {
  amount: "scale",
  date: "shift",
};

/** 默认策略：P0 类型占位符可还原，其余掩码；NER 类型假数据/占位符 */
export function defaultStrategies(): Record<string, StrategyDto> {
  return {
    mobile: { output: "mask", mask_char: "*", keep_head: 3, keep_tail: 4 },
    id_card: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    bank_card: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    credit_code: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    company_name: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    bank_name: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    phone: { output: "mask", mask_char: "*", keep_head: 3, keep_tail: 2 },
    plate: { output: "mask", mask_char: "*", keep_head: 1, keep_tail: 1 },
    email: { output: "mask", mask_char: "*", keep_head: 1, keep_tail: 2 },
    ip: { output: "mask", mask_char: "*", keep_head: 0, keep_tail: 0 },
    url: { output: "mask", mask_char: "*", keep_head: 4, keep_tail: 0 },
    // 金额/日期默认“不处理”：合同审查、诉讼中它们是分析对象（时效、违约金、利息都依赖真实值）；
    // 需要隐藏时可在此改为占位符。
    amount: { output: "keep", mask_char: "*", keep_head: 0, keep_tail: 0 },
    date: { output: "keep", mask_char: "*", keep_head: 0, keep_tail: 0 },
    // 姓名默认占位符（可还原）；原“假数据”对姓名实际退化为 ** 掩码，名不副实
    name: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    bank_account: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    id_card_hmt: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    passport: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    case_no: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    contract_no: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    org: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    address: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
    // 词典（含项目案件词典）默认占位符：可还原，且 AI 能看出是同一实体
    dict: { output: "placeholder", mask_char: "*", keep_head: 0, keep_tail: 0 },
  };
}

const CFG_KEY = "anonkit.config.v1";

/**
 * 当前内置规则模式版本。
 * - v0：历史版本（手机号等正则缺失分隔符支持）
 * - v1：引入本字段；启动迁移为 v1 时，用 builtin 定义刷新内置 id 的规则
 * - v2：新增「统一社会信用代码」规则，银行卡/金额/日期正则升级；
 *       迁移不仅刷新已有 id，还会把内置新增的规则补进旧配置。
 * - v3：新增「公司名称 / 开户银行 / 姓名」规则（上下文前缀+捕获组本体），
 *       手机号内部捕获组改为非捕获组（统一“group(1)=本体”约定）。
 * - v4：新增「案号 / 合同编号 / 地址」规则，诉讼角色（原告/被告/审判长…）纳入姓名与公司规则。
 * - v5：新增「银行账号 / 护照与通行证」规则；手机号支持全角数字，身份证支持分组空格，座机支持括号/空格/分机。
 * - v6：新增「港澳台证件号」规则；姓名前缀补繁体写法（聯絡人/負責人…）。
 * 之后内置正则（如 rules.rs）更新就 +1。
 */
const RULES_SCHEMA_VERSION = 6;

/** 序列化用于持久化：口令只存在于内存，绝不写入 localStorage / SQLite */
function serializeConfig(cfg: AppConfig): string {
  return JSON.stringify({ ...cfg, passphrase: null });
}

/** 补全缺失字段，保证任何来源的配置都是完整结构 */
function normalizeConfig(parsed: Partial<AppConfig>): AppConfig {
  return {
    rules: parsed.rules ?? [],
    // 缺失的类型（如新增规则）补默认策略，避免回落到掩码
    strategies: { ...defaultStrategies(), ...(parsed.strategies ?? {}) },
    maskChar: parsed.maskChar ?? "*",
    dictTerms: parsed.dictTerms ?? [],
    nerEnabled: parsed.nerEnabled ?? true,
    nerThreshold: parsed.nerThreshold ?? 0.6,
    // 默认不联网：用户主动开启后才允许下载模型
    allowModelDownload: parsed.allowModelDownload ?? false,
    // 口令是机密：永不从持久化配置恢复（历史版本遗留的明文口令在此被丢弃）
    passphrase: null,
    rulesSchema: parsed.rulesSchema ?? 0,
  };
}

/**
 * 内置规则 schema 迁移：仅当持久化配置规则版本落后时，
 * 用当前 builtin 定义刷新内置 id 的规则（保留用户开关 enabled，
 * 其他字段如 regex/confidence/context 跟随新内置）：
 * - 内置新增的规则（如统一社会信用代码）自动补进旧配置，默认启用；
 * - 用户自定义规则原样保留。
 * 迁移后写入新版本号，之后用户再改内置规则不会被反复覆盖。
 */
function migrateRules(cfg: AppConfig, builtin: RuleDef[]): AppConfig {
  if (cfg.rulesSchema >= RULES_SCHEMA_VERSION) return cfg;
  const current = new Map(cfg.rules.map((r) => [r.id, r]));
  const builtinIds = new Set(builtin.map((r) => r.id));
  const rules = [
    // 内置已覆盖全部最新定义；用户改过的开关（enabled）继续沿用
    ...builtin.map((b) => {
      const old = current.get(b.id);
      return old ? { ...b, enabled: old.enabled } : b;
    }),
    // 用户自定义规则保留
    ...cfg.rules.filter((r) => !builtinIds.has(r.id)),
  ];
  return { ...cfg, rules, rulesSchema: RULES_SCHEMA_VERSION };
}

function loadProcessed(): ProcessedStore {
  try {
    return parseProcessedStore(localStorage.getItem(PROCESSED_KEY));
  } catch {
    return emptyProcessedStore();
  }
}

function loadConfig(): AppConfig {
  try {
    const raw = localStorage.getItem(CFG_KEY);
    if (raw) {
      return normalizeConfig(JSON.parse(raw) as Partial<AppConfig>);
    }
  } catch {
    /* ignore */
  }
  return normalizeConfig({});
}

interface AppStore {
  page: PageId;
  navigate: (p: PageId) => void;
  files: ImportedFile[];
  /** 导入路径（文件或文件夹）：统计元数据 → 排除导出产物 → 已处理的默认跳过 */
  importPaths: (paths: string[], opts?: { reprocess?: boolean }) => Promise<ImportOutcome>;
  removeFile: (path: string) => void;
  clearFiles: () => void;
  /** 本次导入被跳过的文件 */
  skipped: SkippedFile[];
  /** 把跳过的“已处理”文件重新加入列表（不传 = 全部） */
  reprocessSkipped: (paths?: string[]) => void;
  dismissSkipped: () => void;
  importOutcome: ImportOutcome | null;
  /** 已处理记录（会话内 + localStorage 持久化；不含原文） */
  processed: ProcessedStore;
  clearProcessed: () => void;
  lastBatch: LastBatch | null;
  dismissLastBatch: () => void;
  summaryOpen: boolean;
  setSummaryOpen: (b: boolean) => void;
  exportState: ExportState;
  setExportState: (s: ExportState) => void;
  /**
   * 导出成功后收尾：记录已处理 → 生成“上一批”→ 清空工作区（文件/扫描结果/勾选/lastRedact）
   * → 回到工作台并打开结论页。
   */
  completeBatch: (args: {
    result: RedactResult;
    snapshot: ExportSnapshot;
    levels: Map<string, VerdictLevel>;
    overridden: Set<string>;
    outputDir: string;
  }) => void;
  scanResult: ScanResult | null;
  setScanResult: (r: ScanResult | null) => void;
  /** 命中勾选状态：hit.id → 是否确认（跨页面保留） */
  confirmed: Record<string, boolean>;
  setConfirmed: (c: Record<string, boolean>) => void;
  scanState: "idle" | "scanning";
  setScanState: (s: "idle" | "scanning") => void;
  lastRedact: RedactResult | null;
  setLastRedact: (r: RedactResult | null) => void;
  /** 当前打开的项目（保险库）；null = 未打开 */
  vault: VaultInfo | null;
  setVault: (v: VaultInfo | null) => void;
  /** 最近一次扫描耗时（毫秒，预览页统计卡展示） */
  scanMs: number | null;
  setScanMs: (ms: number | null) => void;
  selectedFileIdx: number;
  setSelectedFileIdx: (i: number) => void;
  config: AppConfig;
  updateConfig: (patch: Partial<AppConfig>) => void;
  configReady: boolean;
  audit: AuditEntry[];
  addAudit: (e: AuditEntry) => void;
  toast: (msg: string) => void;
  modelReady: boolean;
  setModelReady: (b: boolean) => void;
}

const Ctx = createContext<AppStore | null>(null);

export function useApp(): AppStore {
  const store = useContext(Ctx);
  if (!store) throw new Error("useApp 必须在 AppStoreProvider 内使用");
  return store;
}

export function AppStoreProvider({ children }: { children: ReactNode }) {
  const [page, setPage] = useState<PageId>("workbench");
  const [files, setFiles] = useState<ImportedFile[]>([]);
  const filesRef = useRef<ImportedFile[]>([]);
  filesRef.current = files;
  const [skipped, setSkipped] = useState<SkippedFile[]>([]);
  const skippedRef = useRef<SkippedFile[]>([]);
  skippedRef.current = skipped;
  const [importOutcome, setImportOutcome] = useState<ImportOutcome | null>(null);
  const [processed, setProcessed] = useState<ProcessedStore>(loadProcessed);
  const processedRef = useRef(processed);
  processedRef.current = processed;
  const [lastBatch, setLastBatch] = useState<LastBatch | null>(null);
  const [summaryOpen, setSummaryOpen] = useState(false);
  const [exportState, setExportState] = useState<ExportState>("idle");
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [confirmed, setConfirmed] = useState<Record<string, boolean>>({});
  const [scanState, setScanState] = useState<"idle" | "scanning">("idle");
  const [lastRedact, setLastRedact] = useState<RedactResult | null>(null);
  const [vault, setVault] = useState<VaultInfo | null>(null);
  const [scanMs, setScanMs] = useState<number | null>(null);
  const [selectedFileIdx, setSelectedFileIdx] = useState(0);
  const [config, setConfig] = useState<AppConfig>(loadConfig);
  const [configReady, setConfigReady] = useState(false);
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [modelReady, setModelReady] = useState(false);
  const toastTimer = useRef<number | undefined>(undefined);

  // 启动初始化：取权威配置（SQLite > localStorage）→ 内置规则 schema 迁移 →
  // 回写持久化；同时恢复审计历史。合并到单次异步流程避免两个 effect 互相覆盖。
  useEffect(() => {
    let cancelled = false;
    (async () => {
      let cfg = loadConfig(); // localStorage 兜底（可能来自历史版本）
      try {
        const json = await configGet();
        if (json) {
          cfg = normalizeConfig(JSON.parse(json) as Partial<AppConfig>);
          try {
            localStorage.setItem(CFG_KEY, json);
          } catch {
            /* ignore */
          }
        }
      } catch {
        /* ignore */
      }

      // 无条件回写一次已去除口令的配置：清掉历史版本遗留在 localStorage / SQLite 中的明文口令
      try {
        localStorage.setItem(CFG_KEY, serializeConfig(cfg));
      } catch {
        /* ignore */
      }
      configSet(serializeConfig(cfg)).catch(() => undefined);

      let next = cfg;
      try {
        const builtin = await getBuiltinRules();
        next = migrateRules(cfg, builtin);
        if (next !== cfg) {
          // 规则 schema 落后：回写最新内置定义，避免旧正则继续生效
          try {
            localStorage.setItem(CFG_KEY, serializeConfig(next));
          } catch {
            /* ignore */
          }
          configSet(serializeConfig(next)).catch(() => undefined);
        }
      } catch {
        /* ignore */
      }

      // 应用内已打开的项目（如界面刷新）：从后端恢复，口令始终只在后端内存里
      try {
        const v = await vaultInfo();
        if (!cancelled) setVault(v);
      } catch {
        /* ignore */
      }

      if (!cancelled) {
        setConfig(next);
        setConfigReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // 启动时恢复审计历史
  useEffect(() => {
    auditList()
      .then((entries) => {
        if (entries.length) setAudit(entries);
      })
      .catch(() => undefined);
  }, []);

  const persistProcessed = useCallback((next: ProcessedStore) => {
    processedRef.current = next;
    setProcessed(next);
    try {
      localStorage.setItem(PROCESSED_KEY, serializeProcessedStore(next));
    } catch {
      /* ignore */
    }
  }, []);

  const importPaths = useCallback(
    async (paths: string[], opts?: { reprocess?: boolean }): Promise<ImportOutcome> => {
      const store = processedRef.current;
      const res: CollectResult = await collectFiles(paths, excludeListFor(store));
      const got = new Set(res.files.map((f) => normPath(f.path)));
      // 明确给出的、带扩展名但格式不支持的文件（文件夹没有扩展名，会被 collect_files 展开）
      const unsupported = paths.filter((p) => {
        const name = baseName(p);
        return !got.has(normPath(p)) && /\.[^.]+$/.test(name) && formatOf(name) === "未知";
      });
      const part = partitionImports(res.files, store, {
        reprocess: !!opts?.reprocess,
        existing: filesRef.current.map((f) => f.path),
      });
      const toFile = (m: { path: string; size: number; mtime_ms: number }) => {
        const name = baseName(m.path);
        return { path: m.path, name, format: formatOf(name), size: m.size, mtime_ms: m.mtime_ms };
      };
      const knownFps = new Set(store.records.map((r) => r.fp));
      const added: ImportedFile[] = part.fresh.map((m) => ({
        ...toFile(m),
        changed: m.changed,
        reprocess: knownFps.has(fingerprint(m)),
      }));
      setFiles((prev) => {
        const have = new Set(prev.map((f) => normPath(f.path)));
        return [...prev, ...added.filter((f) => !have.has(normPath(f.path)))];
      });
      const newSkipped: SkippedFile[] = [
        ...part.processed.map(({ meta, record }) => ({
          ...toFile(meta),
          reason: "processed" as const,
          processedAt: record.processed_at,
          outputPath: record.output_path,
        })),
        ...part.outputs.map((m) => ({ ...toFile(m), reason: "output" as const })),
        ...res.skipped_artifacts.map((p) => ({ ...toFile({ path: p, size: 0, mtime_ms: 0 }), reason: "artifact" as const })),
        ...unsupported.map((p) => ({ ...toFile({ path: p, size: 0, mtime_ms: 0 }), reason: "unsupported" as const })),
      ];
      // 同一路径以最新一次导入为准；已加入列表的不再显示为跳过
      const addedSet = new Set(added.map((f) => normPath(f.path)));
      setSkipped((prev) => {
        const fresh = new Set(newSkipped.map((f) => normPath(f.path)));
        return [
          ...prev.filter((f) => !fresh.has(normPath(f.path)) && !addedSet.has(normPath(f.path))),
          ...newSkipped,
        ];
      });
      const outcome: ImportOutcome = {
        added: added.length,
        changed: added.filter((f) => f.changed).length,
        processed: part.processed.length,
        outputs: part.outputs.length,
        artifacts: res.skipped_artifacts.length,
        unsupported: unsupported.length,
        duplicates: part.duplicates,
      };
      setImportOutcome(outcome);
      return outcome;
    },
    [],
  );

  const reprocessSkipped = useCallback((paths?: string[]) => {
    const want = paths ? new Set(paths.map(normPath)) : null;
    const pick = skippedRef.current.filter(
      (f) => f.reason === "processed" && (!want || want.has(normPath(f.path))),
    );
    if (!pick.length) return;
    const pickSet = new Set(pick.map((f) => normPath(f.path)));
    setFiles((fs) => {
      const have = new Set(fs.map((f) => normPath(f.path)));
      return [
        ...fs,
        ...pick
          .filter((f) => !have.has(normPath(f.path)))
          .map<ImportedFile>((f) => ({
            path: f.path, name: f.name, format: f.format, size: f.size, mtime_ms: f.mtime_ms, reprocess: true,
          })),
      ];
    });
    setSkipped((prev) => prev.filter((f) => !pickSet.has(normPath(f.path))));
    setImportOutcome((o) => (o ? { ...o, processed: Math.max(0, o.processed - pick.length), added: o.added + pick.length } : o));
  }, []);

  const dismissSkipped = useCallback(() => {
    setSkipped([]);
    setImportOutcome(null);
  }, []);

  const removeFile = useCallback((path: string) => {
    setFiles((prev) => prev.filter((f) => f.path !== path));
  }, []);

  const clearFiles = useCallback(() => {
    setFiles([]);
    setSkipped([]);
    setImportOutcome(null);
  }, []);

  const clearProcessed = useCallback(() => persistProcessed(emptyProcessedStore()), [persistProcessed]);

  const completeBatch = useCallback<AppStore["completeBatch"]>(
    ({ result, snapshot, levels, overridden, outputDir }) => {
      const metas = new Map(filesRef.current.map((f) => [normPath(f.path), f]));
      // 只有真正生成了脱敏副本的文件才记为“已处理”；失败/被复检阻止的保持可再次处理
      const entries = result.files
        .filter((fr) => !!fr.output_path && !fr.error)
        .map((fr) => {
          const m = metas.get(normPath(fr.input_path));
          return {
            meta: { path: fr.input_path, size: m?.size ?? 0, mtime_ms: m?.mtime_ms ?? 0 },
            output_path: fr.output_path,
            overridden: overridden.has(fr.input_path),
            level: levels.get(fr.input_path) ?? null,
          };
        });
      persistProcessed(recordBatch(processedRef.current, entries, Date.now()));
      const counts: Record<VerdictLevel, number> = { green: 0, yellow: 0, red: 0 };
      for (const fr of result.files) {
        const l = levels.get(fr.input_path);
        if (l) counts[l]++;
      }
      setLastBatch({
        time: snapshot.time,
        result,
        snapshot,
        counts,
        exported: entries.length,
        notExported: result.files
          .filter((fr) => !fr.output_path || !!fr.error)
          .map((fr) => ({ path: fr.input_path, name: baseName(fr.input_path) })),
        outputDir,
      });
      // 清空工作区：下一批从干净状态开始，不残留旧结果/勾选
      setFiles([]);
      setSkipped([]);
      setImportOutcome(null);
      setScanResult(null);
      setConfirmed({});
      setLastRedact(null);
      setScanMs(null);
      setSelectedFileIdx(0);
      setScanState("idle");
      setExportState("idle");
      setSummaryOpen(true);
      setPage("workbench");
      document.getElementById("content")?.scrollTo({ top: 0 });
    },
    [persistProcessed],
  );

  const dismissLastBatch = useCallback(() => {
    setLastBatch(null);
    setSummaryOpen(false);
  }, []);

  const updateConfig = useCallback((patch: Partial<AppConfig>) => {
    setConfig((c) => {
      const next = { ...c, ...patch };
      try {
        localStorage.setItem(CFG_KEY, serializeConfig(next));
      } catch {
        /* ignore */
      }
      // 权威源：SQLite 落库（本地毫秒级写，fire-and-forget）
      configSet(serializeConfig(next)).catch(() => undefined);
      return next;
    });
  }, []);

  const addAudit = useCallback((e: AuditEntry) => {
    setAudit((prev) => [e, ...prev]);
    auditAdd(e).catch(() => undefined); // 落库失败不阻断 UI
  }, []);


  const toast = useCallback((msg: string) => {
    const el = document.getElementById("toast");
    if (!el) return;
    el.textContent = msg;
    el.classList.add("show");
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => el.classList.remove("show"), 2600);
  }, []);

  const navigate = useCallback((p: PageId) => {
    setPage(p);
    document.getElementById("content")?.scrollTo({ top: 0 });
  }, []);

  const value = useMemo<AppStore>(
    () => ({
      page,
      navigate,
      files,
      importPaths,
      removeFile,
      clearFiles,
      skipped,
      reprocessSkipped,
      dismissSkipped,
      importOutcome,
      processed,
      clearProcessed,
      lastBatch,
      dismissLastBatch,
      summaryOpen,
      setSummaryOpen,
      exportState,
      setExportState,
      completeBatch,
      scanResult,
      setScanResult,
      confirmed,
      setConfirmed,
      scanState,
      setScanState,
      lastRedact,
      setLastRedact,
      vault,
      setVault,
      scanMs,
      setScanMs,
      selectedFileIdx,
      setSelectedFileIdx,
      config,
      updateConfig,
      configReady,
      audit,
      addAudit,
      toast,
      modelReady,
      setModelReady,
    }),
    [
      page, navigate, files, importPaths, removeFile, clearFiles, skipped, reprocessSkipped, dismissSkipped,
      importOutcome, processed, clearProcessed, lastBatch, dismissLastBatch, summaryOpen, exportState,
      completeBatch, scanResult, setScanResult,
      confirmed, setConfirmed, scanState,
      lastRedact, vault, setVault, scanMs, selectedFileIdx, config, updateConfig, configReady, audit, addAudit,
      toast, modelReady,
    ],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
