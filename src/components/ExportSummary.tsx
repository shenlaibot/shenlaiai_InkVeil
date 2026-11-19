// 导出结论弹窗：每个文件一个红/黄/绿结论 + 残留/未覆盖清单 + 一页纸合规报告。
// 全程不显示任何原文：清单只含 文件、部位、类型、字数、次数。
import { useEffect, useMemo, useState } from "react";
import { ChevronDown, ClipboardCopy, FilePlus2, FileText, ShieldAlert } from "lucide-react";
import type { RedactResult } from "../types";
import type { ExportSnapshot, FileVerdict, Level } from "./verdict";
import {
  LEVEL_NAME, LEVEL_ORDER, RULE_TEXT, SCOPE_STATEMENT, judgeAll, listText,
  residualLine, uncoveredLine,
} from "./verdict";
import type { FileSnapshot } from "./verdict";
import { buildReportHtml } from "./report";
import { saveTextFile } from "../api";

type Filter = "all" | Level;

async function copy(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

export function ExportSummary(props: {
  result: RedactResult;
  snapshot: ExportSnapshot;
  onClose: () => void;
  onOpenFolder: () => void;
  onAudit: () => void;
  /** 处理新文件（关闭结论页并回到导入） */
  onNewFiles?: () => void;
  toast: (m: string) => void;
}) {
  const { result, snapshot, toast, onClose } = props;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const [filter, setFilter] = useState<Filter>("all");
  const [anonNames, setAnonNames] = useState(false);
  const [open, setOpen] = useState<Record<string, boolean>>({});

  const verdicts = useMemo<FileVerdict[]>(() => {
    const snaps = new Map<string, FileSnapshot>(snapshot.files.map((f) => [f.path, f]));
    return judgeAll(result.files, snaps)
      .map((v, i) => ({ v, i }))
      .sort((a, b) => LEVEL_ORDER[a.v.level] - LEVEL_ORDER[b.v.level] || a.i - b.i)
      .map((x) => x.v);
  }, [result, snapshot]);

  const count = useMemo(() => {
    const c = { green: 0, yellow: 0, red: 0 };
    verdicts.forEach((v) => c[v.level]++);
    return c;
  }, [verdicts]);

  const shown = verdicts.filter((v) => filter === "all" || v.level === filter);
  const okFiles = result.files.filter((f) => !f.error);
  const sidecarVersion = result.files.find((f) => f.sidecar_version)?.sidecar_version ?? "";

  const doCopy = async (text: string) =>
    toast((await copy(text)) ? "清单已复制（不含原文）" : "复制失败，请手动选择文本");

  const exportReport = async () => {
    const html = buildReportHtml(verdicts, snapshot, { anonymizeNames: anonNames, sidecarVersion });
    try {
      const p = await saveTextFile("脱敏合规报告.html", html);
      if (p) toast(`报告已保存：${p.split(/[\\/]/).pop()}（用浏览器打开后可打印为 PDF）`);
    } catch (e) {
      toast(`保存报告失败: ${e}`);
    }
  };

  return (
    <div className="modal-mask" onClick={props.onClose}>
      <div
        className="modal modal-wide wf-pop"
        role="dialog"
        aria-modal="true"
        aria-labelledby="export-summary-title"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-h" id="export-summary-title"><ShieldAlert size={16} /> 导出结论</div>
        <div className="modal-b">
          <div className="tl-bar" role="group" aria-label="按结论筛选">
            <span className="tl-count" aria-live="polite">
              <b className="g">绿 {count.green}</b> / <b className="y">黄 {count.yellow}</b> / <b className="r">红 {count.red}</b>
            </span>
            <span className="tl-filters">
              {(["all", "red", "yellow", "green"] as Filter[]).map((f) => (
                <button
                  key={f}
                  className={`tl-chip ${f} ${filter === f ? "on" : ""}`}
                  onClick={() => setFilter(f)}
                >
                  {f === "all" ? "全部" : LEVEL_NAME[f]}
                </button>
              ))}
            </span>
            <button
              className="btn sm ghost"
              onClick={() => void doCopy(verdicts.map((v, i) => listText(v, v.snap.name || `文件 ${i + 1}`)).join("\n\n"))}
            >
              <ClipboardCopy size={13} /> 复制全部清单
            </button>
          </div>
          <div className="tl-sub">
            已导出 {okFiles.length} 个文件 · 脱敏命中 {okFiles.reduce((a, f) => a + f.confirmed, 0)} 处 ·
            映射 {result.mapping_entries} 条 · 失败 {result.files.length - okFiles.length} 个。
            绿色只表示复检通过且无未覆盖部位，不代表无漏检。
          </div>

          <details className="tl-how">
            <summary>如何判定红 / 黄 / 绿</summary>
            <ul>
              {RULE_TEXT.map((r) => (
                <li key={r.level}><b className={`lv-${r.level}`}>{LEVEL_NAME[r.level]}</b>：{r.text}</li>
              ))}
            </ul>
            <div className="tl-scope">{SCOPE_STATEMENT}</div>
          </details>

          <div className="tl-list">
            {shown.length === 0 && <div className="tl-empty">该分类下没有文件</div>}
            {shown.map((v) => {
              const key = v.snap.path || v.result.input_path;
              const res = v.result.residual ?? [];
              const unc = v.result.uncovered ?? [];
              const expanded = open[key] ?? v.level !== "green";
              return (
                <div className={`tl-file ${v.level}`} key={key}>
                  <div className="tl-head" onClick={() => setOpen({ ...open, [key]: !expanded })}>
                    <span className={`tl-dot ${v.level}`} aria-hidden="true" />
                    <span className="tl-name" title={v.snap.name}>{v.snap.name}</span>
                    <span className={`tl-badge ${v.level}`}>{LEVEL_NAME[v.level]} · {v.headline}</span>
                    <ChevronDown size={14} className={`tl-chev ${expanded ? "up" : ""}`} />
                  </div>
                  {expanded && (
                    <div className="tl-body">
                      {v.reasons.length === 0 ? (
                        <div className="tl-ok">
                          已在输出的全部可提取内容中反查已确认项，未发现残留；无未覆盖部位。
                          图片内文字不在检测范围，复检不证明没有漏检。
                        </div>
                      ) : (
                        <ul className="tl-reasons">
                          {v.reasons.map((r, i) => <li key={i} className={r.level}>{r.text}</li>)}
                        </ul>
                      )}
                      {res.length > 0 && (
                        <Section title="残留清单" onCopy={() => void doCopy(res.map((r) => residualLine(v.snap.name, r)).join("\n"))}>
                          {res.map((r, i) => (
                            <li key={i} className="red">{residualLine(v.snap.name, r)}</li>
                          ))}
                        </Section>
                      )}
                      {unc.length > 0 && (
                        <Section title="未覆盖清单" onCopy={() => void doCopy(unc.map((u) => uncoveredLine(v.snap.name, u)).join("\n"))}>
                          {unc.map((u, i) => (
                            <li key={i} className="yellow">{uncoveredLine(v.snap.name, u)}</li>
                          ))}
                        </Section>
                      )}
                      {v.level === "red" && (
                        <div className="tl-norelease">
                          红色文件不提供放行。该文件未记为“已处理”，可在工作台重新导入，修正后再导出。
                        </div>
                      )}
                      <div className="tl-actions">
                        <button className="btn sm ghost" onClick={() => void doCopy(listText(v, v.snap.name))}>
                          <ClipboardCopy size={13} /> 复制本文件清单
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
        <div className="modal-f tl-foot">
          <button className="btn" onClick={props.onClose}>关闭</button>
          <button className="btn" onClick={props.onOpenFolder}>在文件夹中显示</button>
          <label className="tl-anon">
            <input type="checkbox" checked={anonNames} onChange={(e) => setAnonNames(e.target.checked)} />
            报告中文件名用序号
          </label>
          <span style={{ flex: 1 }} />
          <button className="btn" onClick={() => void exportReport()}>
            <FileText size={14} /> 导出报告
          </button>
          <button className="btn" onClick={props.onAudit}>查看审计日志</button>
          {props.onNewFiles && (
            <button className="btn primary" onClick={props.onNewFiles}>
              <FilePlus2 size={14} /> 处理新文件
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function Section(props: { title: string; onCopy: () => void; children: React.ReactNode }) {
  return (
    <div className="tl-sec">
      <div className="tl-sec-h">
        <span>{props.title}</span>
        <button className="tl-copy" onClick={props.onCopy} title="复制该清单">复制</button>
      </div>
      <ul className="tl-items">{props.children}</ul>
    </div>
  );
}
