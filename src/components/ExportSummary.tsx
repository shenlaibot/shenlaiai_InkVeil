// 导出结论：一句话总结 → 按状态筛选的文件列表 → 只对有问题的文件展开原因。
// 全程不显示任何原文：清单只含 文件、部位、类型、字数、次数。
import { useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronDown, CircleCheck, ClipboardCopy, Ellipsis, FilePlus2, FolderOpen, OctagonAlert, TriangleAlert, X,
} from "lucide-react";
import type { RedactResult } from "../types";
import type { ExportSnapshot, FileVerdict, Level } from "./verdict";
import {
  LEVEL_LABEL, LEVEL_ORDER, RULE_SHORT, SCOPE_SHORT, groupUncovered, judgeAll, listText,
} from "./verdict";
import type { FileSnapshot } from "./verdict";
import { buildReportHtml } from "./report";
import { baseName } from "./workflow";
import { saveTextFile } from "../api";

type Filter = "all" | Level;

const LEVELS: Level[] = ["red", "yellow", "green"];
const WHERE_LIMIT = 4;

async function copy(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

function whereText(where: string[]): string {
  if (where.length <= WHERE_LIMIT) return where.join("、");
  return `${where.slice(0, WHERE_LIMIT).join("、")} 等 ${where.length} 处`;
}

/** 文件行的副标题：各类型命中数，最多 3 类 */
function hitSummary(v: FileVerdict): string {
  const counts = [...v.snap.hitCounts].sort((a, b) => b[1] - a[1]);
  if (counts.length === 0) return v.result.confirmed > 0 ? `${v.result.confirmed} 处` : "";
  const head = counts.slice(0, 3).map(([k, n]) => `${k} ${n}`).join(" · ");
  return counts.length > 3 ? `${head} 等` : head;
}

export function ExportSummary(props: {
  result: RedactResult;
  snapshot: ExportSnapshot;
  outputDir: string;
  onClose: () => void;
  onOpenFolder: () => void;
  onAudit: () => void;
  /** 处理新文件（关闭结论页并回到导入） */
  onNewFiles?: () => void;
  toast: (m: string) => void;
}) {
  const { result, snapshot, toast, onClose } = props;
  const [filter, setFilter] = useState<Filter>("all");
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const [menu, setMenu] = useState(false);
  const [rules, setRules] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (menu) setMenu(false);
      else onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, menu]);

  useEffect(() => {
    if (!menu) return;
    const onDown = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenu(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [menu]);

  const verdicts = useMemo<FileVerdict[]>(() => {
    const snaps = new Map<string, FileSnapshot>(snapshot.files.map((f) => [f.path, f]));
    return judgeAll(result.files, snaps)
      .map((v, i) => ({ v, i }))
      .sort((a, b) => LEVEL_ORDER[a.v.level] - LEVEL_ORDER[b.v.level] || a.i - b.i)
      .map((x) => x.v);
  }, [result, snapshot]);

  const count = useMemo(() => {
    const c: Record<Level, number> = { green: 0, yellow: 0, red: 0 };
    verdicts.forEach((v) => c[v.level]++);
    return c;
  }, [verdicts]);

  const worst: Level = count.red > 0 ? "red" : count.yellow > 0 ? "yellow" : "green";
  const title =
    worst === "red"
      ? `${count.red} 个文件不可直接外发`
      : worst === "yellow"
        ? `${count.yellow} 个文件需人工核对`
        : `${count.green} 个文件复检通过`;
  const HeroIcon = worst === "red" ? OctagonAlert : worst === "yellow" ? TriangleAlert : CircleCheck;

  const exported = result.files.filter((f) => f.output_path && !f.error);
  const hits = exported.reduce((a, f) => a + f.confirmed, 0);
  const mapping = result.vault_saved
    ? { text: "映射已存入项目", warn: false }
    : result.mapping_path
      ? { text: "映射已加密保存", warn: false }
      : { text: "未保存映射，无法还原", warn: exported.length > 0 };

  const levelsPresent = LEVELS.filter((l) => count[l] > 0);
  const shown = verdicts.filter((v) => filter === "all" || v.level === filter);
  const issues = verdicts.filter((v) => v.level !== "green");
  const sidecarVersion = result.files.find((f) => f.sidecar_version)?.sidecar_version ?? "";

  const doCopy = async (text: string) =>
    toast((await copy(text)) ? "清单已复制（不含原文）" : "复制失败");

  const exportReport = async (anonymizeNames: boolean) => {
    setMenu(false);
    const html = buildReportHtml(verdicts, snapshot, { anonymizeNames, sidecarVersion });
    try {
      const p = await saveTextFile("脱敏合规报告.html", html);
      if (p) toast(`报告已保存：${p.split(/[\\/]/).pop()}`);
    } catch (e) {
      toast(`保存报告失败: ${e}`);
    }
  };

  return (
    <div className="modal-mask" onClick={onClose}>
      <div
        className="modal xs-modal wf-pop"
        role="dialog"
        aria-modal="true"
        aria-labelledby="export-summary-title"
        onClick={(e) => e.stopPropagation()}
      >
        <header className={`xs-hero ${worst}`}>
          <HeroIcon size={22} className="xs-hero-ic" aria-hidden="true" />
          <div className="xs-hero-tt">
            <h2 id="export-summary-title">{title}</h2>
            <p>
              已导出 {exported.length} 个文件 · 确认 {hits} 处 ·{" "}
              <span className={mapping.warn ? "xs-warn" : undefined}>{mapping.text}</span>
            </p>
          </div>
          <button className="icon-x" onClick={onClose} aria-label="关闭导出结论">
            <X size={15} />
          </button>
        </header>

        <div className="modal-b xs-body">
          {exported.length > 0 && (
            <div className="xs-destination">
              <div className="wf-output-location">
                <span>导出位置</span>
                <bdi className="wf-output-path">{props.outputDir}</bdi>
              </div>
              <button className="btn sm" onClick={props.onOpenFolder}>
                <FolderOpen size={14} aria-hidden="true" /> 打开导出文件夹
              </button>
            </div>
          )}
          {levelsPresent.length > 1 && (
            <div className="xs-tabs" role="group" aria-label="按结论筛选">
              <button className={`xs-tab ${filter === "all" ? "on" : ""}`} aria-pressed={filter === "all"} onClick={() => setFilter("all")}>
                全部 <b>{verdicts.length}</b>
              </button>
              {levelsPresent.map((l) => (
                <button key={l} className={`xs-tab ${l} ${filter === l ? "on" : ""}`} aria-pressed={filter === l} onClick={() => setFilter(l)}>
                  <i aria-hidden="true" />{LEVEL_LABEL[l]} <b>{count[l]}</b>
                </button>
              ))}
            </div>
          )}

          <div className="xs-list">
            {shown.map((v) => {
              const key = v.snap.path || v.result.input_path;
              const expandable = v.level !== "green";
              const expanded = expandable && (open[key] ?? true);
              const residual = v.result.residual ?? [];
              const groups = groupUncovered(v.result.uncovered ?? []);
              const reasons = v.reasons.filter((r) => r.kind !== "uncovered");
              const sub = hitSummary(v);
              const outputPath = v.result.error ? null : v.result.output_path;
              const rowBody = (
                <>
                  <span className={`xs-dot ${v.level}`} aria-hidden="true" />
                  <span className="xs-name">
                    <span className="xs-fn" title={v.snap.name}>{v.snap.name}</span>
                    {sub && <span className="xs-sub">{sub}</span>}
                  </span>
                  <span className={`xs-status ${v.level}`}>{v.headline}</span>
                  {expandable && <ChevronDown size={14} className={`xs-chev ${expanded ? "up" : ""}`} aria-hidden="true" />}
                </>
              );
              return (
                <div className="xs-file" key={key}>
                  <div className="xs-row">
                    {expandable ? (
                      <button className="xs-head" aria-expanded={expanded} onClick={() => setOpen({ ...open, [key]: !expanded })}>
                        {rowBody}
                      </button>
                    ) : (
                      <div className="xs-head">{rowBody}</div>
                    )}
                  </div>
                  {outputPath && (
                    <div className="wf-output-location xs-output">
                      <span>脱敏副本</span>
                      <bdi className="wf-output-path" title={outputPath}>{baseName(outputPath)}</bdi>
                    </div>
                  )}
                  {expanded && (
                    <div className="xs-detail">
                      {reasons.map((r, i) => <div key={i} className={`xs-reason ${r.level}`}>{r.text}</div>)}
                      {(residual.length > 0 || groups.length > 0) && (
                        <dl className="xs-items">
                          {residual.map((r, i) => (
                            <div key={`r${i}`}>
                              <dt>{r.rule_label || "已确认项"}</dt>
                              <dd>{r.where} · {r.length} 字 · {r.count} 处</dd>
                            </div>
                          ))}
                          {groups.map((g) => (
                            <div key={g.kind}>
                              <dt>{g.name}{g.count > 1 ? ` ×${g.count}` : ""}</dt>
                              <dd>{g.where.length > 0 && <>{whereText(g.where)} · </>}{g.advice}</dd>
                            </div>
                          ))}
                        </dl>
                      )}
                      <div className="xs-file-foot">
                        <span>{!v.result.output_path ? "可在工作台重新导入" : ""}</span>
                        <button className="xs-link" onClick={() => void doCopy(listText(v, v.snap.name))}>
                          <ClipboardCopy size={12} aria-hidden="true" /> 复制
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>

          <div className="xs-scope">
            <span>{SCOPE_SHORT}</span>
            <button className="xs-link" aria-expanded={rules} onClick={() => setRules(!rules)}>判定标准</button>
          </div>
          {rules && (
            <dl className="xs-rules">
              {LEVELS.map((l) => (
                <div key={l}>
                  <dt className={l}>{LEVEL_LABEL[l]}</dt>
                  <dd>{RULE_SHORT[l]}</dd>
                </div>
              ))}
            </dl>
          )}
        </div>

        <div className="modal-f xs-foot">
          <div className="xs-more" ref={menuRef}>
            <button className="btn ghost" aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu(!menu)}>
              <Ellipsis size={14} /> 更多
            </button>
            {menu && (
              <div className="xs-menu" role="menu">
                <button role="menuitem" onClick={() => void exportReport(false)}>导出报告</button>
                <button role="menuitem" onClick={() => void exportReport(true)}>导出报告（隐去文件名）</button>
                {issues.length > 0 && (
                  <button
                    role="menuitem"
                    onClick={() => {
                      setMenu(false);
                      void doCopy(issues.map((v) => listText(v, v.snap.name)).join("\n\n"));
                    }}
                  >
                    复制问题清单
                  </button>
                )}
                <button role="menuitem" onClick={() => { setMenu(false); props.onAudit(); }}>查看审计日志</button>
              </div>
            )}
          </div>
          <span style={{ flex: 1 }} />
          {props.onNewFiles ? (
            <button className="btn primary" onClick={props.onNewFiles}>
              <FilePlus2 size={14} /> 处理新文件
            </button>
          ) : (
            <button className="btn primary" onClick={onClose}>完成</button>
          )}
        </div>
      </div>
    </div>
  );
}
