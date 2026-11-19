// 扫描预览：按实体汇总确认 / 按类型分组折叠 / 搜索筛选 / 键盘操作 / 文档预览联动 / 导出。
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { ChevronDown, ChevronRight, Crosshair, Download, Search, TriangleAlert, Undo2, X } from "lucide-react";
import { useApp } from "../store";
import type { HitView, ScannedFile } from "../types";
import { fmtIcon } from "../components/FileIcon";
import { Stepper } from "../components/Stepper";
import { ExportDialog } from "../components/ExportDialog";
import { LOW_CONF } from "../components/verdict";
import type { Entity, HitFilter } from "../components/workflow";
import {
  EMPTY_FILTER,
  confTier,
  confirmedStats,
  entityKey,
  entityState,
  filterEntities,
  groupByType,
  groupEntities,
  isFilterActive,
  sourceName,
  toggleEntity,
} from "../components/workflow";
import "../styles/workflow.css";

/** 类型 → 徽标样式（与 App.css 的 .t-* 对齐） */
const TYPE_CLASS: Record<string, string> = {
  mobile: "t-mobile",
  id_card: "t-idcard",
  bank_card: "t-bankcard",
  phone: "t-phone",
  plate: "t-plate",
  email: "t-email",
  ip: "t-ip",
  url: "t-url",
  amount: "t-amount",
  date: "t-date",
  name: "t-name",
  org: "t-org",
  address: "t-address",
  dict: "t-dict",
};

function typeClass(ruleId: string): string {
  return TYPE_CLASS[ruleId] ?? "t-dict";
}

/** 每组首屏实体数 / 每个实体首屏出现处数（分页，避免长列表卡顿） */
const ENTITY_PAGE = 40;
const OCC_PAGE = 20;
/** 文档预览分块大小（按命中数），只重渲染勾选状态变化的块 */
const DOC_CHUNK = 120;

function prefersReducedMotion(): boolean {
  try {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
}

function locText(file: ScannedFile, hit: HitView): string {
  return file.format === "xlsx" && hit.block
    ? `${hit.block} · 第 ${hit.line + 1} 行`
    : file.format === "docx" && hit.block && hit.block !== "正文"
      ? hit.block
      : `第 ${Math.max(1, hit.page + 1)} 页`;
}

/** 置信度：三格信号 + 文字档位（颜色不是唯一信息） */
function ConfMeter({ value }: { value: number }) {
  const { tier, label } = confTier(value);
  const bars = tier === "high" ? 3 : tier === "mid" ? 2 : 1;
  const pct = Math.round(value * 100);
  return (
    <span className={`wf-conf c-${tier}`} title={`置信度 ${pct}%`} aria-label={`置信度${label}，${pct}%`}>
      <span className="wf-conf-bars" aria-hidden="true">
        {[1, 2, 3].map((i) => <i key={i} className={i <= bars ? "on" : ""} />)}
      </span>
      <span className="wf-conf-l">{label}</span>
    </span>
  );
}

/** 三态复选框（部分确认时显示为半选） */
function TriCheck(props: { state: "all" | "none" | "some"; onChange: () => void; label: string }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = props.state === "some";
  }, [props.state]);
  return (
    <input
      ref={ref}
      type="checkbox"
      className="wf-cb"
      checked={props.state === "all"}
      onChange={props.onChange}
      onClick={(e) => e.stopPropagation()}
      aria-label={props.label}
      tabIndex={-1}
    />
  );
}

type Row = { kind: "entity"; key: string } | { kind: "occ"; key: string; id: string };

export function PreviewPage() {
  const {
    scanResult, confirmed, setConfirmed, selectedFileIdx, setSelectedFileIdx, navigate, scanMs, exportState, page,
  } = useApp();
  const [filter, setFilter] = useState<HitFilter>(EMPTY_FILTER);
  const [collapsedTypes, setCollapsedTypes] = useState<Record<string, boolean>>({});
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [entityLimit, setEntityLimit] = useState<Record<string, number>>({});
  const [occLimit, setOccLimit] = useState<Record<string, number>>({});
  const [active, setActive] = useState<number>(0);
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const [flashId, setFlashId] = useState<string | null>(null);
  const [cursor, setCursor] = useState<Record<string, number>>({});
  const [undo, setUndo] = useState<{ prev: Record<string, boolean>; label: string } | null>(null);
  const [showExport, setShowExport] = useState(false);
  const [showWarn, setShowWarn] = useState(false);
  const flashTimer = useRef<number | undefined>(undefined);
  const undoTimer = useRef<number | undefined>(undefined);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const files = useMemo(() => scanResult?.files ?? [], [scanResult]);
  const entities = useMemo(() => groupEntities(files), [files]);
  const visible = useMemo(() => filterEntities(entities, filter, confirmed, LOW_CONF), [entities, filter, confirmed]);
  const groups = useMemo(() => groupByType(visible), [visible]);
  const allTypes = useMemo(() => groupByType(entities).map((g) => ({ id: g.ruleId, label: g.label, n: g.hitCount })), [entities]);
  const allSources = useMemo(() => [...new Set(entities.flatMap((e) => e.sources))], [entities]);
  const stats = useMemo(() => confirmedStats(files, confirmed), [files, confirmed]);
  const totalHits = useMemo(() => files.reduce((a, f) => a + f.hits.length, 0), [files]);
  const entityByKey = useMemo(() => new Map(entities.map((e) => [e.key, e])), [entities]);

  // 键盘导航用的可见行（类型组折叠/实体展开/分页都计入）
  const rows = useMemo<Row[]>(() => {
    const out: Row[] = [];
    for (const g of groups) {
      if (collapsedTypes[g.ruleId]) continue;
      for (const e of g.entities.slice(0, entityLimit[g.ruleId] ?? ENTITY_PAGE)) {
        out.push({ kind: "entity", key: e.key });
        if (expanded[e.key]) {
          for (const r of e.refs.slice(0, occLimit[e.key] ?? OCC_PAGE)) out.push({ kind: "occ", key: e.key, id: r.hit.id });
        }
      }
    }
    return out;
  }, [groups, collapsedTypes, expanded, entityLimit, occLimit]);
  const rowIndex = useMemo(() => {
    const m = new Map<string, number>();
    rows.forEach((r, i) => m.set(r.kind === "entity" ? `e:${r.key}` : `o:${r.id}`, i));
    return m;
  }, [rows]);
  const activeRow = rows[Math.min(active, rows.length - 1)];

  const pushUndo = useCallback((prev: Record<string, boolean>, label: string) => {
    setUndo({ prev, label });
    window.clearTimeout(undoTimer.current);
    undoTimer.current = window.setTimeout(() => setUndo(null), 8000);
  }, []);

  // 用 ref 读最新勾选，保证 toggleHit 引用稳定（文档分块的 memo 才有效）
  const confirmedRef = useRef(confirmed);
  confirmedRef.current = confirmed;
  const toggleHit = useCallback(
    (id: string) => setConfirmed({ ...confirmedRef.current, [id]: !confirmedRef.current[id] }),
    [setConfirmed],
  );

  const toggleEnt = useCallback(
    (e: Entity) => {
      const next = toggleEntity(e, confirmed);
      if (e.refs.length >= 10) pushUndo(confirmed, `${entityState(e, confirmed) === "all" ? "已取消" : "已确认"}「${e.text}」${e.refs.length} 处`);
      setConfirmed(next);
    },
    [confirmed, setConfirmed, pushUndo],
  );

  const setMany = useCallback(
    (ents: Entity[], on: boolean, label: string) => {
      const next = { ...confirmed };
      for (const e of ents) for (const r of e.refs) next[r.hit.id] = on;
      pushUndo(confirmed, label);
      setConfirmed(next);
    },
    [confirmed, setConfirmed, pushUndo],
  );

  /** 定位到某处命中：切文件 → 平滑滚动 → 高亮闪烁 */
  const locate = useCallback(
    (fileIdx: number, hitId: string) => {
      setSelectedFileIdx(fileIdx);
      window.clearTimeout(flashTimer.current);
      setFlashId(null);
      window.setTimeout(() => {
        const el = document.getElementById(`mark-${hitId}`);
        el?.scrollIntoView({ behavior: prefersReducedMotion() ? "auto" : "smooth", block: "center" });
        setFlashId(hitId);
        flashTimer.current = window.setTimeout(() => setFlashId(null), 1600);
      }, 40);
    },
    [setSelectedFileIdx],
  );

  /** 点击实体：依次定位到它的下一处出现 */
  const locateEntity = useCallback(
    (e: Entity) => {
      const i = ((cursor[e.key] ?? -1) + 1) % e.refs.length;
      setCursor((c) => ({ ...c, [e.key]: i }));
      setFocusKey(e.key);
      locate(e.refs[i].fileIdx, e.refs[i].hit.id);
    },
    [cursor, locate],
  );

  // 当前行滚入可视区
  useEffect(() => {
    if (!activeRow) return;
    const id = activeRow.kind === "entity" ? `row-e-${activeRow.key}` : `row-o-${activeRow.id}`;
    document.getElementById(id)?.scrollIntoView({ block: "nearest" });
  }, [activeRow]);

  // 键盘：↑↓ 切换、空格勾选、回车定位/展开、→← 展开收起、/ 搜索
  useEffect(() => {
    if (page !== "preview") return;
    const onKey = (ev: KeyboardEvent) => {
      if (showExport || document.querySelector(".modal-mask")) return;
      const t = ev.target as HTMLElement | null;
      const typing = !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
      if (ev.key === "/" && !typing) {
        ev.preventDefault();
        searchRef.current?.focus();
        return;
      }
      if (typing) return;
      if (ev.metaKey || ev.ctrlKey || ev.altKey) return;
      if (!rows.length) return;
      const cur = rows[Math.min(active, rows.length - 1)];
      const ent = cur ? entityByKey.get(cur.key) : undefined;
      if (ev.key === "ArrowDown" || ev.key === "j") {
        ev.preventDefault();
        setActive((a) => Math.min(rows.length - 1, a + 1));
      } else if (ev.key === "ArrowUp" || ev.key === "k") {
        ev.preventDefault();
        setActive((a) => Math.max(0, a - 1));
      } else if (ev.key === " ") {
        ev.preventDefault();
        if (cur?.kind === "entity" && ent) toggleEnt(ent);
        else if (cur?.kind === "occ") toggleHit(cur.id);
      } else if (ev.key === "Enter") {
        ev.preventDefault();
        if (cur?.kind === "entity" && ent) locateEntity(ent);
        else if (cur?.kind === "occ" && ent) {
          const r = ent.refs.find((x) => x.hit.id === cur.id);
          if (r) locate(r.fileIdx, r.hit.id);
        }
      } else if (ev.key === "ArrowRight" && cur?.kind === "entity") {
        ev.preventDefault();
        setExpanded((x) => ({ ...x, [cur.key]: true }));
      } else if (ev.key === "ArrowLeft" && cur) {
        ev.preventDefault();
        setExpanded((x) => ({ ...x, [cur.key]: false }));
        if (cur.kind === "occ") setActive(rowIndex.get(`e:${cur.key}`) ?? 0);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [page, rows, active, entityByKey, toggleEnt, toggleHit, locateEntity, locate, rowIndex, showExport]);

  // 筛选变化后，活动行回到顶部
  useEffect(() => setActive(0), [filter]);

  if (!scanResult) {
    return (
      <div className="wf-page">
        <div className="wf-head">
          <div>
            <div className="page-title">扫描预览</div>
            <div className="page-sub">先在工作台导入并扫描文档，这里展示命中与确认。</div>
          </div>
          <Stepper current={1} />
        </div>
        <div className="card wf-blank">
          <div>暂无扫描结果</div>
          <button className="btn primary" onClick={() => navigate("workbench")}>去工作台导入文档</button>
        </div>
      </div>
    );
  }

  // 没有任何文件被成功解析 → 可见提示而非崩溃
  if (files.length === 0) {
    return (
      <div className="wf-page">
        <div className="page-title">扫描预览</div>
        <div className="page-sub">扫描完成，但没有文件被成功解析</div>
        <div className="card" style={{ padding: "28px 20px" }}>
          {scanResult.global_warnings.length > 0 && (
            <div className="warn-banner" style={{ marginBottom: 16 }}>
              <span className="w-icon"><TriangleAlert size={16} /></span>
              <div>
                <div className="w-title">解析提醒（{scanResult.global_warnings.length} 条）</div>
                <ul>{scanResult.global_warnings.map((w, i) => <li key={i}>{w}</li>)}</ul>
              </div>
            </div>
          )}
          <div style={{ textAlign: "center" }}>
            <button className="btn primary" onClick={() => navigate("workbench")}>返回工作台</button>
          </div>
        </div>
      </div>
    );
  }

  const warnings: { text: string; fileIdx: number | null }[] = [
    ...scanResult.global_warnings.map((w) => ({ text: w, fileIdx: null as number | null })),
    ...files.flatMap((f, fi) => f.warnings.map((w) => ({ text: `【${f.name}】${w}`, fileIdx: fi }))),
  ];
  const lowCount = entities.filter((e) => e.minConf < LOW_CONF).length;
  const filtering = isFilterActive(filter);
  const exporting = exportState === "exporting";
  const toggleIn = <T,>(arr: T[], v: T) => (arr.includes(v) ? arr.filter((x) => x !== v) : [...arr, v]);
  const fileIdx = Math.min(selectedFileIdx, files.length - 1);

  return (
    <div className="wf-page wf-preview">
      <div className="wf-head">
        <div>
          <div className="page-title">扫描预览</div>
          <div className="page-sub">
            {files.length} 个文件 · {totalHits} 处命中 · {entities.length} 个不同实体
            {scanMs != null && ` · 检测耗时 ${(scanMs / 1000).toFixed(1)}s`}。同一原文已合并，可整体确认或取消。
          </div>
        </div>
        <Stepper current={3} />
      </div>

      {warnings.length > 0 && (
        <div className="warn-banner wf-warn">
          <span className="w-icon"><TriangleAlert size={16} /></span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <button className="wf-warn-t" onClick={() => setShowWarn((v) => !v)} aria-expanded={showWarn}>
              扫描提醒（{warnings.length} 条）
              <ChevronDown size={13} className={`wf-chev ${showWarn ? "up" : ""}`} />
            </button>
            {showWarn && (
              <ul>
                {warnings.map((w, i) => (
                  <li key={i} onClick={() => w.fileIdx != null && setSelectedFileIdx(w.fileIdx)}>{w.text}</li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}

      <div className="wf-toolbar" role="search">
        <div className="wf-search">
          <Search size={14} aria-hidden="true" />
          <input
            ref={searchRef}
            value={filter.query}
            onChange={(e) => setFilter({ ...filter, query: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setFilter({ ...filter, query: "" });
                (e.target as HTMLInputElement).blur();
              } else if (e.key === "Enter" || e.key === "ArrowDown") {
                (e.target as HTMLInputElement).blur();
                listRef.current?.focus();
              }
            }}
            placeholder="搜索原文或类型"
            aria-label="搜索命中（快捷键 /）"
          />
          <kbd aria-hidden="true">/</kbd>
        </div>
        <div className="wf-chips" role="group" aria-label="按类型筛选">
          {allTypes.map((t) => (
            <button
              key={t.id}
              className={`wf-chip ${filter.ruleIds.includes(t.id) ? "on" : ""}`}
              aria-pressed={filter.ruleIds.includes(t.id)}
              onClick={() => setFilter({ ...filter, ruleIds: toggleIn(filter.ruleIds, t.id) })}
            >
              {t.label}<span>{t.n}</span>
            </button>
          ))}
        </div>
        <div className="wf-chips" role="group" aria-label="其他筛选">
          {allSources.length > 1 && allSources.map((s) => (
            <button
              key={s}
              className={`wf-chip ${filter.sources.includes(s) ? "on" : ""}`}
              aria-pressed={filter.sources.includes(s)}
              onClick={() => setFilter({ ...filter, sources: toggleIn(filter.sources, s) })}
            >
              来源：{sourceName(s)}
            </button>
          ))}
          <button
            className={`wf-chip ${filter.onlyLowConf ? "on" : ""}`}
            aria-pressed={filter.onlyLowConf}
            onClick={() => setFilter({ ...filter, onlyLowConf: !filter.onlyLowConf })}
            disabled={lowCount === 0 && !filter.onlyLowConf}
          >
            仅看低置信度<span>{lowCount}</span>
          </button>
          <button
            className={`wf-chip ${filter.onlyUnconfirmed ? "on" : ""}`}
            aria-pressed={filter.onlyUnconfirmed}
            onClick={() => setFilter({ ...filter, onlyUnconfirmed: !filter.onlyUnconfirmed })}
          >
            仅看未确认
          </button>
          {filtering && (
            <button className="btn sm ghost" onClick={() => setFilter(EMPTY_FILTER)}>
              <X size={13} /> 清除筛选
            </button>
          )}
        </div>
      </div>

      <div className="wf-split">
        {/* 左：实体汇总 */}
        <div
          className="wf-list"
          ref={listRef}
          tabIndex={0}
          role="tree"
          aria-label="命中实体（↑↓ 切换，空格勾选，回车定位，→ 展开）"
          aria-activedescendant={
            activeRow ? (activeRow.kind === "entity" ? `row-e-${activeRow.key}` : `row-o-${activeRow.id}`) : undefined
          }
        >
          <div className="wf-list-h">
            <span>{filtering ? `筛选出 ${visible.length} / ${entities.length} 个实体` : `${entities.length} 个实体`}</span>
            <span className="wf-kbd-hint" aria-hidden="true"><kbd>↑</kbd><kbd>↓</kbd> 切换 <kbd>空格</kbd> 勾选 <kbd>↵</kbd> 定位</span>
          </div>
          {groups.length === 0 && <div className="wf-list-empty">没有符合筛选条件的命中</div>}
          {groups.map((g) => {
            const collapsed = !!collapsedTypes[g.ruleId];
            const on = g.entities.reduce((a, e) => a + e.refs.filter((r) => confirmed[r.hit.id]).length, 0);
            const gState = on === 0 ? "none" : on === g.hitCount ? "all" : "some";
            const limit = entityLimit[g.ruleId] ?? ENTITY_PAGE;
            return (
              <div className="wf-group" key={g.ruleId} role="group" aria-label={g.label}>
                <div className="wf-group-h">
                  <button
                    className="wf-group-tg"
                    onClick={() => setCollapsedTypes({ ...collapsedTypes, [g.ruleId]: !collapsed })}
                    aria-expanded={!collapsed}
                    aria-label={`${collapsed ? "展开" : "折叠"}${g.label}`}
                  >
                    <ChevronRight size={14} className={`wf-rot ${collapsed ? "" : "open"}`} />
                  </button>
                  <span className={`type-badge ${typeClass(g.ruleId)}`}>{g.label}</span>
                  <span className="wf-group-n">{g.entities.length} 个实体 · {on}/{g.hitCount} 处已确认</span>
                  <button
                    className="wf-link"
                    onClick={() => setMany(g.entities, gState !== "all", `${gState !== "all" ? "已确认" : "已取消"}${g.label}共 ${g.hitCount} 处`)}
                  >
                    {gState === "all" ? "取消该组" : "确认该组"}
                  </button>
                </div>
                {!collapsed && (
                  <div className="wf-group-b">
                    {g.entities.slice(0, limit).map((e) => (
                      <EntityRow
                        key={e.key}
                        e={e}
                        files={files}
                        confirmed={confirmed}
                        expanded={!!expanded[e.key]}
                        occLimit={occLimit[e.key] ?? OCC_PAGE}
                        activeId={activeRow ? (activeRow.kind === "entity" ? `e:${activeRow.key}` : `o:${activeRow.id}`) : ""}
                        focused={focusKey === e.key}
                        cursor={cursor[e.key]}
                        onToggle={() => toggleEnt(e)}
                        onToggleHit={toggleHit}
                        onExpand={() => setExpanded({ ...expanded, [e.key]: !expanded[e.key] })}
                        onLocate={() => {
                          setActive(rowIndex.get(`e:${e.key}`) ?? 0);
                          locateEntity(e);
                        }}
                        onLocateHit={(fi, id) => {
                          setActive(rowIndex.get(`o:${id}`) ?? 0);
                          setFocusKey(e.key);
                          locate(fi, id);
                        }}
                        onMore={() => setOccLimit({ ...occLimit, [e.key]: (occLimit[e.key] ?? OCC_PAGE) + OCC_PAGE * 2 })}
                      />
                    ))}
                    {g.entities.length > limit && (
                      <button
                        className="wf-more"
                        onClick={() => setEntityLimit({ ...entityLimit, [g.ruleId]: limit + ENTITY_PAGE * 2 })}
                      >
                        显示更多（还有 {g.entities.length - limit} 个）
                      </button>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {/* 右：文档预览 */}
        <DocPreview
          file={files[fileIdx]}
          fileIdx={fileIdx}
          files={files}
          confirmed={confirmed}
          onSelectFile={setSelectedFileIdx}
          onToggle={toggleHit}
          flashId={flashId}
          focusKey={focusKey}
          aliases={scanResult.aliases}
        />
      </div>

      <div className="wf-footer" role="region" aria-label="确认与导出">
        <span className="wf-footer-stat" aria-live="polite">
          已确认 <b>{stats.hits}</b>/{totalHits} 处 · 预计影响 <b>{stats.files}</b> 个文件
        </span>
        <button className="btn sm ghost" onClick={() => setMany(entities, true, `已全选 ${totalHits} 处`)}>全选</button>
        <button
          className="btn sm ghost"
          onClick={() => setMany(entities, false, `已取消全部 ${stats.hits} 处`)}
          disabled={stats.hits === 0}
        >
          取消全部
        </button>
        {undo && (
          <span className="wf-undo wf-pop" role="status">
            {undo.label}
            <button
              className="wf-link"
              onClick={() => {
                setConfirmed(undo.prev);
                setUndo(null);
              }}
            >
              <Undo2 size={13} /> 撤销
            </button>
          </span>
        )}
        <span style={{ flex: 1 }} />
        <button
          className="btn primary"
          disabled={exporting || stats.hits === 0}
          onClick={() => setShowExport(true)}
          title={stats.hits === 0 ? "请至少确认一处命中" : undefined}
        >
          {exporting ? <span className="spinner" /> : <Download size={15} />}
          导出脱敏副本
        </button>
      </div>

      {showExport && <ExportDialog files={files} onClose={() => setShowExport(false)} />}
    </div>
  );
}

/** 一个实体：一行汇总 + 可展开的出现处 */
const EntityRow = memo(function EntityRow(props: {
  e: Entity;
  files: ScannedFile[];
  confirmed: Record<string, boolean>;
  expanded: boolean;
  occLimit: number;
  activeId: string;
  focused: boolean;
  cursor: number | undefined;
  onToggle: () => void;
  onToggleHit: (id: string) => void;
  onExpand: () => void;
  onLocate: () => void;
  onLocateHit: (fileIdx: number, id: string) => void;
  onMore: () => void;
}) {
  const { e, confirmed } = props;
  const st = entityState(e, confirmed);
  const on = e.refs.filter((r) => confirmed[r.hit.id]).length;
  const isActive = props.activeId === `e:${e.key}`;
  return (
    <div className={`wf-ent ${st !== "none" ? "is-on" : ""} ${isActive ? "is-active" : ""} ${props.focused ? "is-focus" : ""}`}>
      <div
        className="wf-ent-row"
        id={`row-e-${e.key}`}
        role="treeitem"
        aria-selected={isActive}
        aria-expanded={props.expanded}
        aria-checked={st === "all" ? true : st === "none" ? false : "mixed"}
        onClick={props.onLocate}
      >
        <TriCheck state={st} onChange={props.onToggle} label={`${st === "all" ? "取消" : "确认"}「${e.text}」全部 ${e.refs.length} 处`} />
        <div className="wf-ent-main">
          <div className="wf-ent-text">
            <mark>{e.text}</mark>
            <span className="wf-ent-x">×{e.refs.length}</span>
          </div>
          <div className="wf-ent-sub">
            分布在 {e.fileCount} 个文件
            {st === "some" && <> · 已确认 {on}/{e.refs.length}</>}
            {e.sources.some((s) => s !== "rule") && <> · {e.sources.map(sourceName).join("/")}</>}
            {props.cursor != null && e.refs.length > 1 && <> · 第 {props.cursor + 1}/{e.refs.length} 处</>}
          </div>
        </div>
        {e.minConf < 1 && <ConfMeter value={e.minConf} />}
        <button
          className="wf-ent-loc"
          onClick={(ev) => {
            ev.stopPropagation();
            props.onLocate();
          }}
          aria-label={`定位「${e.text}」的下一处`}
          title="定位下一处"
        >
          <Crosshair size={14} />
        </button>
        <button
          className="wf-ent-exp"
          onClick={(ev) => {
            ev.stopPropagation();
            props.onExpand();
          }}
          aria-label={`${props.expanded ? "收起" : "展开"}「${e.text}」的 ${e.refs.length} 处出现`}
        >
          <ChevronDown size={14} className={`wf-chev ${props.expanded ? "up" : ""}`} />
        </button>
      </div>
      {props.expanded && (
        <div className="wf-occ-list" role="group">
          {e.refs.slice(0, props.occLimit).map(({ fileIdx, hit }) => {
            const checked = !!confirmed[hit.id];
            const file = props.files[fileIdx];
            return (
              <div
                key={hit.id}
                id={`row-o-${hit.id}`}
                role="treeitem"
                aria-selected={props.activeId === `o:${hit.id}`}
                aria-checked={checked}
                className={`wf-occ ${checked ? "is-on" : ""} ${props.activeId === `o:${hit.id}` ? "is-active" : ""}`}
                onClick={() => props.onLocateHit(fileIdx, hit.id)}
              >
                <input
                  type="checkbox"
                  className="wf-cb"
                  checked={checked}
                  onChange={() => props.onToggleHit(hit.id)}
                  onClick={(ev) => ev.stopPropagation()}
                  aria-label={`${checked ? "取消" : "确认"}这一处`}
                  tabIndex={-1}
                />
                <div className="wf-occ-body">
                  <div className="wf-occ-ctx">
                    {hit.context_before}
                    <mark>{hit.text}</mark>
                    {hit.context_after}
                  </div>
                  <div className="wf-occ-loc">
                    {file.name} · {locText(file, hit)}
                    {hit.confidence < 1 && ` · 置信 ${Math.round(hit.confidence * 100)}%`}
                  </div>
                </div>
              </div>
            );
          })}
          {e.refs.length > props.occLimit && (
            <button className="wf-more" onClick={props.onMore}>显示更多（还有 {e.refs.length - props.occLimit} 处）</button>
          )}
        </div>
      )}
    </div>
  );
});

/** 右侧文档预览：文件页签 + 全文 mark 高亮（分块渲染，只重绘变化的块） */
function DocPreview(props: {
  file: ScannedFile;
  fileIdx: number;
  files: ScannedFile[];
  confirmed: Record<string, boolean>;
  onSelectFile: (i: number) => void;
  onToggle: (id: string) => void;
  flashId: string | null;
  focusKey: string | null;
  aliases: [string, string][];
}) {
  const { file, confirmed, onToggle, flashId, focusKey } = props;

  // 码点切片：后端偏移是 Unicode 码点，JS String.slice 是 UTF-16 码元
  const chunks = useMemo(() => {
    if (!file) return [];
    const chars = Array.from(file.text);
    const slice = (a: number, b: number) => chars.slice(a, b).join("");
    const hits = [...file.hits].sort((a, b) => a.start - b.start || a.end - b.end);
    const out: { segs: (string | HitView)[]; ids: string[]; keys: string[] }[] = [];
    let cur: { segs: (string | HitView)[]; ids: string[]; keys: string[] } = { segs: [], ids: [], keys: [] };
    let cursor = 0;
    for (const h of hits) {
      const s = Math.min(h.start, chars.length);
      if (s < cursor) continue; // 重叠命中：只渲染前一个
      if (s > cursor) cur.segs.push(slice(cursor, s));
      cur.segs.push(h);
      cur.ids.push(h.id);
      cur.keys.push(entityKey(h));
      cursor = Math.max(cursor, Math.min(h.end, chars.length));
      if (cur.ids.length >= DOC_CHUNK) {
        out.push(cur);
        cur = { segs: [], ids: [], keys: [] };
      }
    }
    if (cursor < chars.length) cur.segs.push(slice(cursor, chars.length));
    out.push(cur);
    return out;
  }, [file]);

  const xlsxSheets = useMemo(
    () => (file?.format === "xlsx" ? (file.structure?.sheets ?? []).filter((s) => s.rows) : []),
    [file],
  );

  if (!file) return null;

  return (
    <div className="doc-preview-card card wf-doc">
      <div className="doc-file-tabs" role="tablist" aria-label="文件">
        {props.files.map((f, i) => (
          <button
            key={f.path}
            role="tab"
            aria-selected={i === props.fileIdx}
            className={`doc-file-tab ${i === props.fileIdx ? "active" : ""}`}
            onClick={() => props.onSelectFile(i)}
            title={f.name}
          >
            {f.name}
            <span className="wf-tab-n">{f.hits.length}</span>
          </button>
        ))}
      </div>
      <div className="doc-preview-head">
        {fmtIcon(file.format)}
        <div className="doc-name">{file.name}</div>
        <div className="doc-meta">{file.page_count} 页 · {file.hits.length} 处命中</div>
      </div>
      <div className="wf-legend" aria-hidden="true">
        <span><mark>已确认</mark></span>
        <span><mark className="unchecked">未确认</mark></span>
        <span className="wf-muted">点击正文高亮可切换</span>
      </div>
      {props.aliases.length > 0 && (
        <div className="alias-callout">
          <b>简称联动：</b>
          检测到 {props.aliases.length} 组全称/简称关联，勾选后将统一替换为同一占位符，保证还原语义一致。
        </div>
      )}
      {xlsxSheets.length > 0 ? (
        <div className="doc-xlsx">
          {xlsxSheets.map((sheet) => (
            <div key={sheet.name} className="xlsx-sheet">
              <div className="doc-page-num">{sheet.name}</div>
              <table className="xlsx-table">
                <tbody>
                  {(sheet.rows ?? []).map((row, ri) => (
                    <tr key={ri}>
                      {row.map((cell, ci) => (
                        <td key={ci}>
                          {cell ? (
                            <CellContent text={cell[0]} s={cell[1]} e={cell[2]} file={file} confirmed={confirmed} onToggle={onToggle} flashId={flashId} focusKey={focusKey} />
                          ) : ""}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ))}
        </div>
      ) : (
        <>
          <div className="doc-page-num">正文（全文预览）</div>
          <div className="doc-text">
            <span className="ln" style={{ whiteSpace: "pre-wrap" }}>
              {chunks.map((c, i) => (
                <DocChunk
                  key={`${file.path}#${i}`}
                  segs={c.segs}
                  sig={
                    c.ids.map((id) => (confirmed[id] ? "1" : "0")).join("") +
                    (flashId && c.ids.includes(flashId) ? `|${flashId}` : "") +
                    (focusKey && c.keys.includes(focusKey) ? `|f:${focusKey}` : "")
                  }
                  confirmed={confirmed}
                  flashId={flashId}
                  focusKey={focusKey}
                  onToggle={onToggle}
                />
              ))}
            </span>
          </div>
        </>
      )}
    </div>
  );
}

function renderMark(h: HitView, checked: boolean, flash: boolean, focus: boolean, onToggle: (id: string) => void): ReactNode {
  return (
    <mark
      key={h.id}
      id={`mark-${h.id}`}
      className={`${checked ? "" : "unchecked"} ${flash ? "flash wf-flash" : ""} ${focus ? "wf-related" : ""}`}
      onClick={(e) => {
        e.stopPropagation();
        onToggle(h.id);
      }}
      title={`${h.rule_label} · 点击${checked ? "取消" : "确认"}`}
    >
      {h.text}
    </mark>
  );
}

/** 一块正文：sig 不变则不重渲染（大文档切换勾选不卡） */
const DocChunk = memo(
  function DocChunk(props: {
    segs: (string | HitView)[];
    sig: string;
    confirmed: Record<string, boolean>;
    flashId: string | null;
    focusKey: string | null;
    onToggle: (id: string) => void;
  }) {
    return (
      <>
        {props.segs.map((s, i) =>
          typeof s === "string"
            ? <span key={i}>{s}</span>
            : renderMark(s, !!props.confirmed[s.id], props.flashId === s.id, props.focusKey === entityKey(s), props.onToggle),
        )}
      </>
    );
  },
  (a, b) => a.sig === b.sig && a.segs === b.segs && a.onToggle === b.onToggle,
);

/** xlsx 单元格内容：在 [s, e) 偏移区间内渲染命中高亮 */
function CellContent(props: {
  text: string;
  s: number;
  e: number;
  file: ScannedFile;
  confirmed: Record<string, boolean>;
  onToggle: (id: string) => void;
  flashId: string | null;
  focusKey: string | null;
}) {
  const hits = props.file.hits.filter((h) => h.start >= props.s && h.end <= props.e).sort((a, b) => a.start - b.start);
  const chars = Array.from(props.text);
  const slice = (a: number, b: number) => chars.slice(a, b).join("");
  const out: ReactNode[] = [];
  let cursor = props.s;
  for (const h of hits) {
    if (h.start < cursor) continue;
    if (h.start > cursor) out.push(slice(cursor - props.s, h.start - props.s));
    out.push(renderMark(h, !!props.confirmed[h.id], props.flashId === h.id, props.focusKey === entityKey(h), props.onToggle));
    cursor = h.end;
  }
  if (cursor < props.e) out.push(slice(cursor - props.s, props.e - props.s));
  return <>{out}</>;
}
