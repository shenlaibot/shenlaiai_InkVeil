// 主流程纯函数单测：node --test tests/ui（Node ≥ 22.6，内置 TypeScript 类型擦除）
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  EMPTY_FILTER,
  MAX_RECORDS,
  confirmedStats,
  emptyProcessedStore,
  entityState,
  estimateRemaining,
  excludeListFor,
  filterEntities,
  fingerprint,
  groupByType,
  groupEntities,
  parseProcessedStore,
  partitionImports,
  recordBatch,
  serializeProcessedStore,
  toggleEntity,
} from "../../src/components/workflow.ts";
import type { HitView, ScannedFile } from "../../src/types.ts";

const meta = (path: string, size = 100, mtime_ms = 1000) => ({ path, size, mtime_ms });

test("指纹：路径 + 大小 + 修改时间；Windows 路径大小写/分隔符归一", () => {
  assert.equal(fingerprint(meta("/a/b.docx")), fingerprint(meta("/a/b.docx")));
  assert.notEqual(fingerprint(meta("/a/b.docx", 100)), fingerprint(meta("/a/b.docx", 101)));
  assert.notEqual(fingerprint(meta("/a/b.docx", 100, 1)), fingerprint(meta("/a/b.docx", 100, 2)));
  assert.equal(fingerprint(meta("C:\\Docs\\A.docx")), fingerprint(meta("c:/docs/a.docx")));
  // POSIX 路径区分大小写
  assert.notEqual(fingerprint(meta("/A.docx")), fingerprint(meta("/a.docx")));
});

test("导出后再次导入同一文件：默认跳过；仍要重新处理时导入；内容变化视为新文件", () => {
  let store = emptyProcessedStore();
  store = recordBatch(
    store,
    [{ meta: meta("/in/合同.docx"), output_path: "/out/anonkit-1a2b3c4d_脱敏.docx", overridden: false, level: "green" }],
    5000,
  );
  const again = partitionImports([meta("/in/合同.docx"), meta("/in/新文件.pdf")], store, { reprocess: false, existing: [] });
  assert.equal(again.processed.length, 1);
  assert.equal(again.processed[0].record.output_path, "/out/anonkit-1a2b3c4d_脱敏.docx");
  assert.deepEqual(again.fresh.map((f) => f.path), ["/in/新文件.pdf"]);

  const forced = partitionImports([meta("/in/合同.docx")], store, { reprocess: true, existing: [] });
  assert.equal(forced.processed.length, 0);
  assert.equal(forced.fresh.length, 1);

  const changed = partitionImports([meta("/in/合同.docx", 999, 7777)], store, { reprocess: false, existing: [] });
  assert.equal(changed.processed.length, 0);
  assert.equal(changed.fresh[0].changed, true, "同路径指纹变化应标记为内容已变化");
});

test("直接选中上一次的输出文件 / 已在列表中的文件：分别归为产物与重复", () => {
  const store = recordBatch(
    emptyProcessedStore(),
    [{ meta: meta("/in/a.docx"), output_path: "/out/anonkit-00000000_脱敏.docx", overridden: false, level: "yellow" }],
    1,
  );
  const p = partitionImports(
    [meta("/out/anonkit-00000000_脱敏.docx"), meta("/in/b.txt"), meta("/in/b.txt")],
    store,
    { reprocess: false, existing: ["/in/c.md"] },
  );
  assert.equal(p.outputs.length, 1);
  assert.equal(p.fresh.length, 1);
  assert.equal(p.duplicates, 1);
  const p2 = partitionImports([meta("/in/c.md")], store, { reprocess: false, existing: ["/in/c.md"] });
  assert.equal(p2.duplicates, 1);
  assert.equal(p2.fresh.length, 0);
});

test("排除列表：输出文件与输出目录（交给 collect_files）", () => {
  const store = recordBatch(
    emptyProcessedStore(),
    [
      { meta: meta("/in/a.docx"), output_path: "/out/x/anonkit-11111111_脱敏.docx", overridden: false, level: "green" },
      { meta: meta("/in/b.docx"), output_path: null, overridden: false, level: "red" },
    ],
    1,
  );
  assert.deepEqual(excludeListFor(store).sort(), ["/out/x", "/out/x/anonkit-11111111_脱敏.docx"].sort());
});

test("持久化：只存元数据；版本不认识/损坏时回到空记录；未知字段被丢弃", () => {
  const store = recordBatch(
    emptyProcessedStore(),
    [{ meta: meta("/in/a.docx"), output_path: "/out/o.docx", overridden: true, level: "red" }],
    42,
  );
  const raw = serializeProcessedStore(store);
  const back = parseProcessedStore(raw);
  assert.deepEqual(back, store);
  assert.deepEqual(parseProcessedStore(null), emptyProcessedStore());
  assert.deepEqual(parseProcessedStore("{oops"), emptyProcessedStore());
  assert.deepEqual(parseProcessedStore(JSON.stringify({ version: 2, records: [] })), emptyProcessedStore());
  const injected = parseProcessedStore(
    JSON.stringify({ version: 1, records: [{ path: "/a", size: 1, mtime_ms: 2, text: "张三 13800138000", level: "bogus" }, { bad: 1 }] }),
  );
  assert.equal(injected.records.length, 1);
  assert.equal("text" in injected.records[0], false, "不得保留任何原文字段");
  assert.equal(injected.records[0].level, null);
});

test("记录同一路径只保留最新一条，且有上限", () => {
  let s = recordBatch(emptyProcessedStore(), [{ meta: meta("/a", 1, 1), output_path: "/o1", overridden: false, level: "green" }], 1);
  s = recordBatch(s, [{ meta: meta("/a", 2, 2), output_path: "/o2", overridden: false, level: "green" }], 2);
  assert.equal(s.records.length, 1);
  assert.equal(s.records[0].output_path, "/o2");
  const many = Array.from({ length: MAX_RECORDS + 5 }, (_, i) => ({
    meta: meta(`/f${i}`), output_path: null, overridden: false, level: null,
  }));
  assert.equal(recordBatch(emptyProcessedStore(), many, 3).records.length, MAX_RECORDS);
});

test("预计剩余时间", () => {
  assert.equal(estimateRemaining(0, 1000, 0, 10), null);
  assert.equal(estimateRemaining(0, 2000, 2, 10), 8);
  assert.equal(estimateRemaining(0, 2000, 10, 10), 0);
});

// ---- 实体汇总 ----
let n = 0;
const hit = (text: string, rule_id = "name", confidence = 1, source = "rule"): HitView => ({
  id: `h${n++}`, rule_id, rule_label: rule_id === "name" ? "姓名" : "手机号", start: 0, end: text.length, text,
  confidence, source, page: 0, line: 0, block: "", context_before: "", context_after: "",
});
const file = (path: string, hits: HitView[]): ScannedFile => ({ path, name: path, format: "txt", text: "", page_count: 1, hits, warnings: [] });

test("同一原文合并为一个实体，统计次数与文件数；一键确认/取消整个实体", () => {
  const files = [
    file("/1", [hit("张三"), hit("张三"), hit("13800138000", "mobile")]),
    file("/2", [hit("张三", "name", 0.5, "ner"), hit("李四")]),
  ];
  const ents = groupEntities(files);
  const zs = ents.find((e) => e.text === "张三")!;
  assert.equal(zs.refs.length, 3);
  assert.equal(zs.fileCount, 2);
  assert.equal(zs.minConf, 0.5);
  assert.deepEqual(zs.sources.sort(), ["ner", "rule"]);

  let c: Record<string, boolean> = { [zs.refs[0].hit.id]: true };
  assert.equal(entityState(zs, c), "some");
  c = toggleEntity(zs, c);
  assert.equal(entityState(zs, c), "all");
  c = toggleEntity(zs, c);
  assert.equal(entityState(zs, c), "none");

  const groups = groupByType(ents);
  assert.deepEqual(groups.map((g) => g.ruleId), ["name", "mobile"]);
  assert.equal(groups[0].entities[0].text, "张三", "组内按次数降序");
  assert.equal(groups[0].hitCount, 4);

  const all: Record<string, boolean> = {};
  for (const f of files) for (const h of f.hits) all[h.id] = true;
  assert.deepEqual(confirmedStats(files, all), { hits: 5, files: 2 });
  assert.deepEqual(confirmedStats(files, {}), { hits: 0, files: 0 });
});

test("筛选：搜索 / 类型 / 来源 / 仅低置信度 / 仅未确认", () => {
  const files = [file("/1", [hit("张三"), hit("李四", "name", 0.5, "ner"), hit("13800138000", "mobile")])];
  const ents = groupEntities(files);
  const names = (f: typeof EMPTY_FILTER, c: Record<string, boolean> = {}) =>
    filterEntities(ents, f, c, 0.7).map((e) => e.text);
  assert.deepEqual(names({ ...EMPTY_FILTER, query: "李" }), ["李四"]);
  assert.deepEqual(names({ ...EMPTY_FILTER, query: "手机" }), ["13800138000"]);
  assert.deepEqual(names({ ...EMPTY_FILTER, ruleIds: ["mobile"] }), ["13800138000"]);
  assert.deepEqual(names({ ...EMPTY_FILTER, sources: ["ner"] }), ["李四"]);
  assert.deepEqual(names({ ...EMPTY_FILTER, onlyLowConf: true }), ["李四"]);
  const c = { [files[0].hits[0].id]: true };
  assert.deepEqual(names({ ...EMPTY_FILTER, onlyUnconfirmed: true }, c), ["李四", "13800138000"]);
});
