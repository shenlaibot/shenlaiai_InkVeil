// 仅开发模式（浏览器直接打开 vite 页面、没有 Tauri 后端时）使用的内存 mock，
// 用来在浏览器里渲染和点击界面做冒烟验证。生产构建不会引入本文件（见 main.tsx 的 import.meta.env.DEV 分支）。
import type { HitView, RedactFileResult, ScannedFile, VaultInfo } from "./types";

/** 演示用扫描文件：text 中的 needle 即命中（文本为虚构，不对应真实个人信息） */
function mkFile(path: string, format: string, text: string, needles: [string, string, string, number][], warnings: string[] = []): ScannedFile {
  const hits: HitView[] = needles.map(([needle, rule_id, rule_label, confidence], i) => {
    const at = text.indexOf(needle);
    return {
      id: `${path}#${i}`, rule_id, rule_label, start: at, end: at + needle.length, text: needle,
      confidence, source: confidence < 1 ? "ner" : "rule", page: 0, line: 0, block: "正文",
      context_before: text.slice(Math.max(0, at - 8), at), context_after: text.slice(at + needle.length, at + needle.length + 8),
    };
  });
  return { path, name: path.split("/").pop()!, format, text, page_count: 3, hits, warnings };
}
/** 演示用长文档：同一原文多次出现（验证“按实体汇总”与长列表分页） */
function mkLong(path: string): ScannedFile {
  const para = [
    "原告张三诉被告李四借款合同纠纷一案，原告张三的联系电话为13800138000。",
    "被告李四辩称，其已于约定日期向张三归还部分款项，证人王五可以证明。",
    "本院认为，张三与李四之间的借贷关系成立，被告李四应当承担还款责任。",
  ];
  const text = Array.from({ length: 30 }, (_, i) => `第${i + 1}段：${para[i % 3]}`).join("\n");
  const needles: [string, string, string, number][] = [["张三", "name", "姓名", 0.95], ["李四", "name", "姓名", 0.92], ["王五", "name", "姓名", 0.62], ["13800138000", "mobile", "手机号", 1]];
  const hits: HitView[] = [];
  for (const [needle, rule_id, rule_label, confidence] of needles) {
    let at = text.indexOf(needle);
    while (at >= 0) {
      hits.push({
        id: `${path}#${hits.length}`, rule_id, rule_label, start: at, end: at + needle.length, text: needle,
        confidence, source: confidence < 1 ? "ner" : "rule", page: Math.floor(at / 400), line: 0, block: "正文",
        context_before: text.slice(Math.max(0, at - 10), at), context_after: text.slice(at + needle.length, at + needle.length + 12),
      });
      at = text.indexOf(needle, at + needle.length);
    }
  }
  return { path, name: path.split("/").pop()!, format: "txt", text, page_count: 2, hits, warnings: [] };
}

const MOCK_FILES: ScannedFile[] = [
  mkFile("/mock/in/甲方合同.docx", "docx", "甲方张三，手机13800138000，签订于上海。", [["张三", "name", "姓名", 0.95], ["13800138000", "mobile", "手机号", 1]]),
  mkFile("/mock/in/证据材料.pdf", "pdf", "乙方李四，身份证110101199001011234。第3页为扫描件。", [["李四", "name", "姓名", 0.9], ["110101199001011234", "id_card", "身份证号", 1]]),
  mkFile("/mock/in/银行流水.xlsx", "xlsx", "户名王五，银行卡6222020200112233445，备注小李。", [["王五", "name", "姓名", 0.9], ["6222020200112233445", "bank_card", "银行卡号", 1], ["小李", "name", "姓名", 0.5]]),
  mkLong("/mock/in/判决书摘录.txt"),
];

// ---- 模拟文件系统（只存元数据，供 collect_files 统计大小/修改时间与排除导出产物）----
const mockFs = new Map<string, { size: number; mtime_ms: number }>();
const T0 = 1_762_000_000_000;
MOCK_FILES.forEach((f, i) => mockFs.set(f.path, { size: 20_000 + i * 13_337, mtime_ms: T0 + i * 1000 }));
mockFs.set("/mock/in/anonkit-1a2b3c4d_脱敏.docx", { size: 18_000, mtime_ms: T0 });
mockFs.set("/mock/in/说明.png", { size: 4_000, mtime_ms: T0 });

const SUPPORTED = ["pdf", "docx", "xlsx", "txt", "md"];
const isSupported = (p: string) => SUPPORTED.includes((p.split(".").pop() ?? "").toLowerCase());
const baseOf = (p: string) => p.split("/").pop() ?? p;
/** 与 commands.rs is_export_artifact_name 一致 */
const isArtifactName = (p: string) => /^\.anonkit-/.test(baseOf(p)) || /^anonkit-[0-9a-fA-F]{8}_脱敏(-\d+)?\.[^.]+$/.test(baseOf(p));
const hasRedactedSuffix = (p: string) => /_脱敏(-\d+)?\.[^.]+$/.test(baseOf(p));

function mockCollect(paths: string[], exclude: string[]) {
  const files: { path: string; size: number; mtime_ms: number }[] = [];
  const skipped: string[] = [];
  const push = (p: string) => {
    if (!files.some((f) => f.path === p)) files.push({ path: p, ...(mockFs.get(p) ?? { size: 0, mtime_ms: 0 }) });
  };
  for (const p of paths) {
    const isDir = !p.includes(".") || [...mockFs.keys()].some((k) => k.startsWith(p + "/"));
    if (isDir) {
      for (const k of mockFs.keys()) {
        if (!k.startsWith(p.replace(/\/$/, "") + "/") || !isSupported(k)) continue;
        const excluded = exclude.some((e) => k === e || (k.startsWith(e + "/") && hasRedactedSuffix(k)));
        if (isArtifactName(k) || excluded) skipped.push(k);
        else push(k);
      }
    } else if (isSupported(p)) {
      push(p);
    }
  }
  return { files, skipped_artifacts: skipped };
}

// ---- 事件（scan-progress / redact-progress / 拖拽）----
const callbacks = new Map<number, (e: unknown) => void>();
const listeners = new Map<string, number[]>();
let cbSeq = 0;
function emit(event: string, payload: unknown) {
  for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
}
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
let scanGen = 0;
let failNextExport = false;

/** 浏览器里模拟拖拽：__mockDrag("enter"|"leave") / __mockDrop(["/mock/in"]) */
(window as any).__mockDrag = (type: "enter" | "leave") =>
  emit(type === "enter" ? "tauri://drag-enter" : "tauri://drag-leave", { paths: [], position: { x: 0, y: 0 } });
(window as any).__mockDrop = (paths: string[]) => emit("tauri://drag-drop", { paths, position: { x: 0, y: 0 } });
/** 修改文件（指纹变化）：__mockTouch("/mock/in/甲方合同.docx") */
(window as any).__mockTouch = (p: string) => {
  const m = mockFs.get(p);
  if (m) mockFs.set(p, { size: m.size + 1, mtime_ms: Date.now() });
};
(window as any).__mockFailNextExport = () => {
  failNextExport = true;
};
(window as any).__mockFolder = "/mock/in";
const MOCK_REDACT: Record<string, Partial<RedactFileResult>> = {
  "/mock/in/甲方合同.docx": {
    output_path: "/mock/out/文件-1.docx", is_deep: true, verified: true, uncovered: [],
    input_sha256: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90",
    output_sha256: "0f9e8d7c6b5a49382716f5e4d3c2b1a00f9e8d7c6b5a49382716f5e4d3c2b1a0",
  },
  "/mock/in/证据材料.pdf": {
    output_path: "/mock/out/文件-2.pdf", is_deep: false, verified: true,
    uncovered: [
      { where: "第 3 页", kind: "scanned_page", count: 1 },
      { where: "第 5 页", kind: "scanned_page", count: 1 },
      { where: "第 2 页", kind: "image", count: 1 },
    ],
    warnings: ["以下部位未被处理，其中的原文可能仍保留在输出文件中，外发前请人工核对"],
    input_sha256: "1111c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90",
    output_sha256: "2222d7c6b5a49382716f5e4d3c2b1a00f9e8d7c6b5a49382716f5e4d3c2b1a0f",
  },
  "/mock/in/银行流水.xlsx": {
    output_path: null, is_deep: false, verified: true,
    error: "复检发现已确认的原文仍保留在输出中，已阻止导出（输出已删除）。残留位置: xl/comments1.xml（3字，2处）",
    residual: [
      { where: "xl/comments1.xml", rule_label: "姓名", length: 2, count: 2 },
      { where: "docProps/core.xml（文本）", rule_label: "银行卡号", length: 19, count: 1 },
    ],
    uncovered: [{ where: "数据透视表及其缓存", kind: "pivot", count: 2 }],
    input_sha256: "3333c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90",
  },
};


let vault: VaultInfo | null = null;
const PASS = "123456";
let auditRetention = 1095;
let auditPruned = 0;
const auditTampered = false;
let auditRows: any[] = [
  { seq: 1, kind: "entry", time: "2023-03-01 10:00", file: "旧合同.docx", hit_count: 4, types: "手机×2 姓名×2", output: "旧合同_脱敏.docx", mapping: "已导出", is_deep: true, note: "", prev_hash: "0", hash: "h1" },
  { seq: 2, kind: "entry", time: "2026-08-11 10:00", file: "新合同.pdf", hit_count: 7, types: "身份证×3", output: "新合同_脱敏.pdf", mapping: "已导出", is_deep: true, note: "", prev_hash: "h1", hash: "h2" },
];

function info(): VaultInfo | null {
  return vault ? { ...vault, batches: [...vault.batches] } : null;
}

let mockDl = { tick: 0, started: false, fail: false };
const TOTAL = 407_615_988;
function mockModelStatus() {
  const base = { model_id: "ner-zh", total: TOTAL, installed: false, error: null as string | null, note: null as string | null };
  if (!mockDl.started) return { ...base, state: "idle", received: 0 };
  mockDl.tick += 1;
  const received = Math.min(TOTAL, mockDl.tick * 30_000_000);
  if (mockDl.fail && mockDl.tick > 4) {
    mockDl.started = false;
    return {
      ...base, state: "failed", received,
      error: "下载 model.onnx 失败: 所有下载源都失败：huggingface.co：无法连接（请检查网络或代理）（已重试 3 次）；hf-mirror.com：HTTP 404 Not Found",
    };
  }
  if (received >= TOTAL) return { ...base, state: "completed", received, installed: true };
  return {
    ...base, state: "downloading", received,
    note: mockDl.tick < 3 ? "model.onnx：正在从 huggingface.co 下载…" : "model.onnx：huggingface.co 多次失败，改用 hf-mirror.com…",
  };
}

const handlers: Record<string, (args: any) => any> = {
  config_get: () => null,
  config_set: () => undefined,
  get_builtin_rules: () => [],
  audit_list: () => auditRows.slice().reverse(),
  audit_add: ({ entry }) => {
    auditRows.push({ ...entry, seq: auditRows.length + 1, kind: "entry", prev_hash: "mock", hash: "mock" + (auditRows.length + 1) });
  },
  audit_verify: () => ({ ok: !auditTampered, count: auditRows.length, broken_seq: auditTampered ? 2 : null, reason: auditTampered ? "内容与哈希不符（记录被修改）" : "", pruned_total: auditPruned }),
  audit_prune: ({ retentionDays }) => {
    auditRetention = retentionDays;
    const n = auditRows.filter((r) => r.kind === "entry" && r.time < "2025").length;
    if (n) {
      auditRows = auditRows.filter((r) => !(r.kind === "entry" && r.time < "2025"));
      auditPruned += n;
      auditRows.push({ seq: auditRows.length + 10, kind: "prune", time: "2026-11-08 12:00 UTC", file: "（系统）", hit_count: 0, types: "", output: "", mapping: "", is_deep: false, note: `{"removed":${n},"root_hash":"mockroot"}`, prev_hash: "mock", hash: "mockprune" });
    }
    return n;
  },
  audit_retention_get: () => auditRetention,
  audit_retention_set: ({ days }) => { auditRetention = days; },
  ner_model_path: () => null,
  list_models: () => [
    {
      id: "ner-zh",
      name: "中文 NER（人名/机构/地址）",
      description: "基于 BERT 的中文命名实体识别（演示数据）。",
      version: "1.0.0",
      size_bytes: 407_615_988,
      base_url: "https://huggingface.co/mock",
      mirrors: ["https://hf-mirror.com/mock"],
      license: "Apache-2.0",
      files: [
        { name: "model.onnx", size: 407_063_645, sha256: "x" },
        { name: "config.json", size: 2121, sha256: "x" },
      ],
    },
  ],
  model_install_dir: () => "/mock/app-data/models/ner-zh",
  model_status: () => mockModelStatus(),
  start_model_download: (args: any) => {
    if (args?.consent !== true && !(window as any).__mockAllow) { /* mock 不校验配置 */ }
    mockDl = { tick: 0, started: true, fail: new URLSearchParams(location.search).has("mockfail") };
  },
  pause_model_download: () => {
    mockDl.started = false;
  },
  delete_model: () => {
    mockDl = { tick: 0, started: false, fail: false };
  },
  "plugin:app|version": () => "dev-mock",
  pick_output_dir: () => (window as any).__mockOutDir ?? "/mock/out",
  pick_folder: () => (window as any).__mockFolder ?? "/mock/in",
  pick_mapping_file: () => "/mock/案件.anonymap",
  pick_vault_file: () => "/mock/projects/演示案件.akcase",
  vault_info: () => info(),
  vault_create: ({ dir, name, passphrase }) => {
    if (!passphrase) throw "项目口令不能为空";
    vault = {
      name,
      path: `${dir}/${name}.akcase`,
      project_id: "mockproject0001",
      created: Math.floor(Date.now() / 1000),
      entries: 0,
      dict_terms: [],
      batches: [],
    };
    return info();
  },
  vault_open: ({ path, passphrase }) => {
    if (passphrase !== PASS) throw "口令错误，或项目文件已损坏";
    vault = {
      name: "演示案件",
      path,
      project_id: "mockproject0002",
      created: 1_762_000_000,
      entries: 7,
      dict_terms: ["星辰计划"],
      batches: [
        { seq: 2, time: 1_762_100_000, files: 3, entries_added: 2 },
        { seq: 1, time: 1_762_000_500, files: 5, entries_added: 5 },
      ],
    };
    return info();
  },
  vault_close: () => {
    vault = null;
  },
  vault_set_dict: ({ terms }) => {
    if (!vault) throw "没有打开的项目";
    vault.dict_terms = [...new Set((terms as string[]).map((t) => t.trim()).filter(Boolean))];
    return info();
  },
  pick_files: () => MOCK_FILES.map((f) => f.path),
  collect_files: ({ paths, exclude }) => mockCollect(paths as string[], (exclude as string[]) ?? []),
  scan_files: async ({ paths }) => {
    const my = ++scanGen;
    const list = paths as string[];
    if (!list.length) return { files: [], aliases: [], global_warnings: [] };
    const out: ScannedFile[] = [];
    for (let i = 0; i < list.length; i++) {
      await sleep(Number((window as any).__mockScanMs ?? 650));
      if (my !== scanGen) throw "扫描已取消";
      const f = MOCK_FILES.find((m) => m.path === list[i]) ?? mkFile(list[i], (list[i].split(".").pop() ?? "txt"), "（演示）这是一段没有敏感信息的文字。", []);
      out.push(f);
      emit("scan-progress", { done: i + 1, total: list.length, file: f.name, hit_count: f.hits.length, message: "完成" });
    }
    return { files: out, aliases: [], global_warnings: [] };
  },
  confirm_redact: async ({ inputs, outputDir }) => {
    const list = inputs as { path: string; hits: unknown[] }[];
    const files: RedactFileResult[] = [];
    for (let i = 0; i < list.length; i++) {
      await sleep(400);
      if (failNextExport && i === 1) {
        failNextExport = false;
        throw "写入输出目录失败：权限不足（演示）";
      }
      const inp = list[i];
      const ext = inp.path.split(".").pop();
      const hash = Array.from(inp.path).reduce((a, c) => (a * 31 + c.charCodeAt(0)) >>> 0, 7).toString(16).padStart(8, "0").slice(0, 8);
      const preset = MOCK_REDACT[inp.path] ?? {};
      const empty = inp.hits.length === 0;
      const output_path = empty || preset.output_path === null ? null : `${outputDir}/anonkit-${hash}_脱敏.${ext}`;
      if (output_path) mockFs.set(output_path, { size: 10_000, mtime_ms: Date.now() });
      files.push({
        input_path: inp.path, error: null, confirmed: inp.hits.length, is_deep: true, verified: true, uncovered: [],
        warnings: empty ? ["未勾选命中，跳过"] : [], ...preset, output_path,
      } as RedactFileResult);
      emit("redact-progress", { done: i + 1, total: list.length, file: baseOf(inp.path), hit_count: inp.hits.length, message: empty ? "跳过（未勾选）" : output_path ? "完成" : "复检未通过" });
    }
    return { files, mapping_path: null, mapping_entries: 6, vault_saved: false };
  },
  save_text_file: ({ defaultName, content }) => {
    (window as any).__lastSavedText = content;
    return `/mock/保存/${defaultName}`;
  },
  restore_text: ({ text, mappingPath }) => {
    if (!mappingPath && !vault) throw "未选择映射文件，也没有打开的项目";
    const out = String(text).split("[姓名1]").join("陈瑾瑜");
    return { output_text: out, restored_count: out === text ? 0 : 1, mapping_entries: vault?.entries ?? 0 };
  },
  restore_text_fuzzy: ({ text, mappingPath }) => {
    if (!mappingPath && !vault) throw "未选择映射文件，也没有打开的项目";
    const src = String(text);
    const re = /[\[【［〔]\s*姓名\s*([1-9１-９])\s*[\]】］〕]/g;
    const hits: any[] = [];
    let exact = 0;
    let out = "";
    let last = 0;
    const chars = (s: string) => Array.from(s).length;
    for (const m of src.matchAll(re)) {
      const idx = m.index ?? 0;
      out += src.slice(last, idx);
      const orig = "陈瑾瑜";
      if (m[0] === "[姓名1]") exact++;
      else
        hits.push({
          start: chars(src.slice(0, idx)),
          end: chars(src.slice(0, idx)) + chars(m[0]),
          out_start: chars(out),
          out_end: chars(out) + chars(orig),
          raw: m[0],
          placeholder: "[姓名1]",
          original: orig,
        });
      out += orig;
      last = idx + m[0].length;
    }
    out += src.slice(last);
    const suspects = [...src.matchAll(/[\[【［〔][^\d\s\[\]【】［］〔〕]{1,10}\d+[\]】］〕]/g)]
      .filter((m) => !/姓名\s*[1１]/.test(m[0]))
      .map((m) => ({ start: m.index ?? 0, end: (m.index ?? 0) + m[0].length, text: m[0] }));
    return { output_text: out, exact_count: exact, fuzzy_count: hits.length, fuzzy_hits: hits, suspects };
  },
  pick_restore_doc: () => "/mock/AI返回稿.docx",
  restore_document: ({ inputPath, outputDir }) => ({
    ok: true,
    output_path: `${outputDir}/${String(inputPath).split("/").pop()?.replace(/(\.\w+)$/, "_已还原$1")}`,
    exact_count: 3,
    fuzzy_count: 1,
    fuzzy_hits: [{ raw: "【 姓名 1 】", placeholder: "[姓名1]", original: "陈瑾瑜", where: "正文" }],
    suspects: [{ text: "[公司9]", where: "表格" }],
    mapping_entries: vault?.entries ?? 0,
  }),
};

(window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };
(window as any).__TAURI_INTERNALS__ = {
  metadata: {
    currentWindow: { label: "main" },
    currentWebview: { label: "main", windowLabel: "main" },
    windows: [{ label: "main" }],
    webviews: [{ label: "main", windowLabel: "main" }],
  },
  invoke: async (cmd: string, args: any) => {
    const h = handlers[cmd];
    if (h) return h(args ?? {});
    if (cmd === "plugin:event|listen") {
      const arr = listeners.get(args.event) ?? [];
      arr.push(args.handler);
      listeners.set(args.event, arr);
      return args.handler;
    }
    if (cmd === "plugin:event|unlisten") {
      listeners.set(args.event, (listeners.get(args.event) ?? []).filter((x) => x !== args.eventId));
      callbacks.delete(args.eventId);
      return null;
    }
    if (cmd.startsWith("plugin:")) return null; // 窗口/事件等插件命令：开发 mock 下忽略
    throw `dev-mock: 未实现的命令 ${cmd}`;
  },
  transformCallback: (cb: (e: unknown) => void) => {
    const id = ++cbSeq;
    callbacks.set(id, cb);
    return id;
  },
  convertFileSrc: (p: string) => p,
};

// ===== 以下为设计验证用的可切换状态（只追加；不带 URL 参数时行为与上面一致，内置规则除外） =====
// ?mockrules=empty                                 内置规则为空（策略页空状态）
// ?mockmodel=downloading|paused|failed|installed   模型市场各状态
// ?mockvault=open                                  启动即打开演示项目
// ?mockaudit=tampered|empty|many|error             审计链被篡改 / 无记录 / 多页记录 / 读取失败
// ?mockrestore=error                               还原报错
// ?onboarding=1                                    强制显示首启说明（见 components/ui/Onboarding.tsx）
{
  const q = new URLSearchParams(location.search);
  const L = [
    ["mobile", "手机号"], ["id_card", "身份证号"], ["bank_card", "银行卡号"], ["credit_code", "统一社会信用代码"],
    ["company_name", "公司名称"], ["bank_name", "开户银行"], ["name", "姓名"], ["phone", "固定电话"],
    ["plate", "车牌号"], ["email", "电子邮箱"], ["ip", "IP地址"], ["url", "网址"], ["amount", "金额"],
    ["case_no", "案号"], ["contract_no", "合同/业务编号"], ["address", "地址"], ["bank_account", "银行账号"],
    ["passport", "护照/通行证"], ["id_card_hmt", "港澳台证件号"], ["date", "日期"],
  ];
  const builtin = L.map(([id, label]) => ({ id, label, regex: "(mock)", enabled: true, confidence: 0.9, context_keywords: [] }));
  handlers.get_builtin_rules = () => (q.get("mockrules") === "empty" ? [] : builtin);

  const mm = q.get("mockmodel");
  if (mm) {
    let received = mm === "installed" ? TOTAL : mm === "paused" ? 152_000_000 : mm === "failed" ? 98_000_000 : 40_000_000;
    let state = mm === "installed" ? "completed" : mm;
    handlers.model_status = () => {
      if (state === "downloading") {
        received = Math.min(TOTAL, received + 1_800_000 + Math.round(Math.random() * 600_000));
        if (received >= TOTAL) state = "completed";
      }
      return {
        model_id: "ner-zh", total: TOTAL, received, state, installed: state === "completed",
        error: state === "failed" ? "下载 model.onnx 失败: 所有下载源都失败：huggingface.co：无法连接（请检查网络或代理）（已重试 3 次）；hf-mirror.com：HTTP 404 Not Found" : null,
        note: state === "downloading" ? "model.onnx：正在从 huggingface.co 下载…" : null,
      };
    };
    handlers.start_model_download = () => { state = "downloading"; };
    handlers.pause_model_download = () => { state = "paused"; };
    handlers.delete_model = () => { state = "idle"; received = 0; };
    handlers.ner_model_path = () => (state === "completed" ? "/mock/app-data/models/ner-zh/model.onnx" : null);
  }

  if (q.get("mockvault") === "open") {
    vault = {
      name: "甲公司并购案", path: "/mock/projects/甲公司并购案.akcase", project_id: "mockproject0003",
      created: 1_760_000_000, entries: 42, dict_terms: ["星辰计划", "甲公司", "老王"],
      batches: [
        { seq: 3, time: 1_762_300_000, files: 2, entries_added: 4 },
        { seq: 2, time: 1_762_100_000, files: 6, entries_added: 13 },
        { seq: 1, time: 1_761_000_500, files: 9, entries_added: 25 },
      ],
    };
  }

  const ma = q.get("mockaudit");
  if (ma === "tampered") {
    handlers.audit_verify = () => ({ ok: false, count: auditRows.length, broken_seq: 2, reason: "内容与哈希不符（记录被修改）", pruned_total: 0 });
  }
  if (ma === "empty") auditRows = [];
  if (ma === "error") {
    handlers.audit_list = () => { throw "无法打开审计数据库：database is locked"; };
    handlers.audit_verify = () => { throw "无法打开审计数据库"; };
  }
  if (ma === "many" || ma === "tampered") {
    const files = ["股权转让协议.docx", "尽调报告.pdf", "银行流水.xlsx", "起诉状.docx", "证据目录.xlsx", "会议纪要.md"];
    const types = ["姓名×4 手机号×2", "身份证号×3 姓名×1", "银行卡号×6 金额×3", "公司名称×5 案号×1", "地址×2 姓名×2 日期×4"];
    for (let i = 0; i < 46; i++) {
      const d = new Date(Date.now() - (46 - i) * 9 * 3600 * 1000);
      const blocked = i % 13 === 7;
      auditRows.push({
        seq: auditRows.length + 1, kind: "entry", time: d.toLocaleString("zh-CN", { hour12: false }),
        file: files[i % files.length], hit_count: 3 + ((i * 7) % 19), types: types[i % types.length],
        output: blocked ? "失败" : `文件-${i + 1}.${files[i % files.length].split(".").pop()}`,
        mapping: blocked ? "导出失败" : i % 3 ? "已存入项目" : "已导出（加密）", is_deep: i % 4 !== 1,
        note: i % 11 === 5 ? "带风险放行：扫描页已人工核对" : blocked ? "复检发现残留，已阻止导出" : "",
        prev_hash: "h", hash: "h" + i,
      });
    }
    auditRows.push({ seq: auditRows.length + 1, kind: "prune", time: new Date().toLocaleString("zh-CN", { hour12: false }), file: "（系统）", hit_count: 0, types: "", output: "", mapping: "", is_deep: false, note: '{"removed":1,"root_hash":"mockroot"}', prev_hash: "h", hash: "hp" });
  }

  if (q.get("mockrestore") === "error") {
    handlers.restore_text_fuzzy = () => { throw "映射文件口令错误，或文件已损坏"; };
    handlers.restore_document = () => { throw "映射文件口令错误，或文件已损坏"; };
  }
}
