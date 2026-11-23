// 工作台：导入文档（拖拽 / 多选 / 文件夹）→ 文件列表 → 开始扫描；导出后显示“上一批已完成”。
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowRight,
  ChevronDown,
  FolderOpen,
  FolderPlus,
  RotateCcw,
  ScanSearch,
  ShieldCheck,
  Upload,
  X,
} from "lucide-react";
import { useApp } from "../store";
import type { ImportOutcome, SkippedFile } from "../store";
import { SCAN_CANCELLED, cancelScan, nerModelPath, openInFolder, pickFiles, pickFolder, scanFiles } from "../api";
import type { ProgressEvent, ScanOptions, ScanResult } from "../types";
import { fmtIcon } from "../components/FileIcon";
import { Stepper } from "../components/Stepper";
import { BatchDoneCard } from "../components/BatchDoneCard";
import { ScanProgressPanel } from "../components/ScanProgressPanel";
import { ExportSummary } from "../components/ExportSummary";
import { baseName, dirName, formatSize } from "../components/workflow";
import "../styles/workflow.css";

const FORMATS = ["PDF", "DOCX", "XLSX", "TXT", "MD"];

/** 兼容旧引用：图标已移到 components/FileIcon */
export { fmtIcon };

const SKIP_REASON: Record<SkippedFile["reason"], string> = {
  processed: "已处理",
  output: "导出产物",
  artifact: "导出产物",
  unsupported: "不支持",
};

function fmtTime(ms: number | undefined): string {
  if (!ms) return "";
  return new Date(ms).toLocaleString("zh-CN", { hour12: false, month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });
}

/** 一次导入的结果说明（拼成一句克制的话） */
function outcomeLines(o: ImportOutcome): { main: string; extra: string[] } {
  const extra: string[] = [];
  if (o.changed) extra.push(`其中 ${o.changed} 个文件内容有变化，按新文件处理`);
  const outs = o.outputs + o.artifacts;
  if (outs) extra.push(`已排除 ${outs} 个上次导出的脱敏副本`);
  if (o.unsupported) extra.push(`${o.unsupported} 个文件格式不支持，未导入`);
  if (o.duplicates) extra.push(`${o.duplicates} 个文件已在列表中`);
  return { main: o.added ? `已导入 ${o.added} 个文件` : "没有新文件需要导入", extra };
}

export function WorkbenchPage() {
  const {
    files, importPaths, removeFile, clearFiles, skipped, reprocessSkipped, dismissSkipped, importOutcome,
    setScanResult, setConfirmed, setScanState, scanState, navigate, config, toast, setScanMs, setSelectedFileIdx,
    lastBatch, dismissLastBatch, summaryOpen, setSummaryOpen, processed,
  } = useApp();
  const [dragging, setDragging] = useState(false);
  const [importing, setImporting] = useState(false);
  const [reprocess, setReprocess] = useState(false);
  const [showSkipped, setShowSkipped] = useState(false);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [scanStarted, setScanStarted] = useState(0);
  const [scanError, setScanError] = useState<string | null>(null);
  /** 扫描代次：取消后旧扫描的结果到达也不再生效 */
  const scanGen = useRef(0);
  const optionsRef = useRef<ScanOptions | null>(null);
  const reprocessRef = useRef(reprocess);
  reprocessRef.current = reprocess;
  const dropRef = useRef<HTMLDivElement>(null);

  const doImport = useCallback(
    async (paths: string[]) => {
      if (!paths.length) return;
      setImporting(true);
      setScanError(null);
      try {
        const o = await importPaths(paths, { reprocess: reprocessRef.current });
        const { main, extra } = outcomeLines(o);
        if (o.processed) {
          toast(`${main}；${o.processed} 个文件已处理过，已跳过`);
        } else {
          toast([main, ...extra].join("；"));
        }
      } catch (e) {
        toast(`导入失败: ${e}`);
      } finally {
        setImporting(false);
      }
    },
    [importPaths, toast],
  );

  // Tauri 原生文件拖拽（拖拽高亮 + 释放导入）
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") setDragging(true);
        else if (p.type === "leave") setDragging(false);
        else if (p.type === "drop") {
          setDragging(false);
          if (p.paths.length) void doImport(p.paths);
        }
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [doImport]);

  // 扫描进度事件（组件卸载时确实取消监听）
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    listen<ProgressEvent>("scan-progress", (e) => setProgress(e.payload))
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const onPickFiles = useCallback(async () => {
    try {
      const paths = await pickFiles();
      if (paths.length) await doImport(paths);
    } catch (e) {
      toast(`选择文件失败: ${e}`);
    }
  }, [doImport, toast]);

  const onPickFolder = useCallback(async () => {
    try {
      const dir = await pickFolder();
      if (dir) await doImport([dir]);
    } catch (e) {
      toast(`选择文件夹失败: ${e}`);
    }
  }, [doImport, toast]);

  const onScan = useCallback(async () => {
    if (files.length === 0) {
      toast("请先导入文档");
      return;
    }
    const my = ++scanGen.current;
    setScanError(null);
    setScanState("scanning");
    setProgress({ done: 0, total: files.length, file: "", hit_count: 0, message: "" });
    const started = Date.now();
    setScanStarted(started);
    try {
      const modelPath = config.nerEnabled ? await nerModelPath().catch(() => null) : null;
      const options: ScanOptions = {
        rules: config.rules,
        strategies: config.strategies,
        ner_threshold: config.nerThreshold,
        ner_model_path: modelPath,
        dict_terms: config.dictTerms,
      };
      optionsRef.current = options;
      const result: ScanResult = await scanFiles(files.map((f) => f.path), options);
      if (my !== scanGen.current) return; // 已取消
      setScanResult(result);
      // 新扫描结果默认全部确认（用户可在预览页取消）
      const init: Record<string, boolean> = {};
      for (const f of result.files) for (const h of f.hits) init[h.id] = true;
      setConfirmed(init);
      setSelectedFileIdx(0);
      const ms = Date.now() - started;
      setScanMs(ms);
      const total = result.files.reduce((a, f) => a + f.hits.length, 0);
      toast(
        result.files.length === 0
          ? "没有文件被成功解析（见预览页提醒）"
          : `扫描完成：${result.files.length} 个文件 · ${total} 处命中 · ${(ms / 1000).toFixed(1)}s`,
      );
      navigate("preview");
    } catch (e) {
      if (my !== scanGen.current) return;
      const msg = String(e);
      if (msg.includes(SCAN_CANCELLED)) toast("已取消扫描，文件列表已保留");
      else setScanError(msg);
    } finally {
      if (my === scanGen.current) {
        setScanState("idle");
        setProgress(null);
      }
    }
  }, [files, config, setScanResult, setConfirmed, setScanState, setScanMs, setSelectedFileIdx, navigate, toast]);

  const onCancelScan = useCallback(() => {
    scanGen.current++;
    setScanState("idle");
    setProgress(null);
    const opts = optionsRef.current;
    if (opts) void cancelScan(opts).catch(() => undefined);
    toast("已取消扫描，文件列表已保留");
  }, [setScanState, toast]);

  const processedSkipped = useMemo(() => skipped.filter((s) => s.reason === "processed"), [skipped]);
  const scanning = scanState === "scanning";
  const step = files.length ? 2 : 1;
  const totalSize = files.reduce((a, f) => a + (f.size || 0), 0);

  const openExportFolder = () => {
    const path = lastBatch?.result.files.find((f) => f.output_path && !f.error)?.output_path;
    if (path) void openInFolder(path).catch((e) => toast(`无法打开导出文件夹: ${e}`));
  };

  return (
    <div className="wf-page">
      <div className="wf-head">
        <div>
          <div className="page-title">工作台</div>
          <div className="page-sub">导入需要脱敏的文档，支持拖拽、多选或整文件夹导入。文档在本机解析与处理。</div>
        </div>
        <Stepper current={step} />
      </div>

      {lastBatch && !files.length && (
        <BatchDoneCard
          batch={lastBatch}
          onOpenFolder={openExportFolder}
          onViewReport={() => setSummaryOpen(true)}
          onNewFiles={() => void onPickFiles()}
          onRetry={() => void doImport(lastBatch.notExported.map((f) => f.path))}
          onDismiss={dismissLastBatch}
        />
      )}

      <div
        ref={dropRef}
        className={`wf-drop ${dragging ? "is-drag" : ""} ${files.length ? "is-compact" : ""} ${importing ? "is-busy" : ""}`}
        onClick={() => !importing && void onPickFiles()}
        onKeyDown={(e) => {
          if ((e.key === "Enter" || e.key === " ") && e.target === e.currentTarget) {
            e.preventDefault();
            void onPickFiles();
          }
        }}
        role="button"
        tabIndex={0}
        aria-label="导入文档：拖拽文档到此处，或按回车选择文件"
        aria-busy={importing}
      >
        <div className="wf-drop-icon" aria-hidden="true">
          {importing ? <span className="spinner dark" /> : <Upload size={files.length ? 22 : 34} strokeWidth={1.6} />}
        </div>
        <div className="wf-drop-text">
          <div className="wf-drop-title">
            {dragging ? "松开即可导入" : importing ? "正在读取文件列表…" : files.length ? "继续添加文档" : "拖拽文档或文件夹到此处"}
          </div>
          {!files.length && (
            <div className="wf-drop-sub">
              文件夹会递归导入；上一次导出的脱敏副本和已处理过的文件会自动跳过
            </div>
          )}
          <div className="wf-drop-formats" aria-label="支持的格式">
            {FORMATS.map((f) => (
              <span key={f} className="fmt-chip">{f}</span>
            ))}
          </div>
        </div>
        <div className="wf-drop-actions" onClick={(e) => e.stopPropagation()}>
          <button className="btn sm" onClick={() => void onPickFiles()} disabled={importing}>
            <FolderPlus size={14} /> 选择文件
          </button>
          <button className="btn sm" onClick={() => void onPickFolder()} disabled={importing}>
            <FolderOpen size={14} /> 选择文件夹
          </button>
        </div>
      </div>

      <label className="wf-switch" title="打开后，已处理过且内容未变的文件也会导入">
        <input type="checkbox" checked={reprocess} onChange={(e) => setReprocess(e.target.checked)} />
        <span className="wf-switch-track" aria-hidden="true"><span /></span>
        导入时包含已处理过的文件
        {processed.records.length > 0 && <span className="wf-muted">（已记录 {processed.records.length} 个）</span>}
      </label>

      {importOutcome && (processedSkipped.length > 0 || skipped.length > 0 || outcomeLines(importOutcome).extra.length > 0) && (
        <div className={`wf-notice ${processedSkipped.length ? "is-info" : ""}`} role="status">
          <div className="wf-notice-body">
            <div>
              <b>{outcomeLines(importOutcome).main}</b>
              {processedSkipped.length > 0 && <>；<b>{processedSkipped.length} 个文件已处理过，已跳过</b></>}
            </div>
            {outcomeLines(importOutcome).extra.length > 0 && (
              <div className="wf-notice-extra">{outcomeLines(importOutcome).extra.join(" · ")}</div>
            )}
          </div>
          <div className="wf-notice-actions">
            {skipped.length > 0 && (
              <button className="btn sm ghost" onClick={() => setShowSkipped((v) => !v)} aria-expanded={showSkipped}>
                {showSkipped ? "收起" : "查看"}
                <ChevronDown size={13} className={`wf-chev ${showSkipped ? "up" : ""}`} />
              </button>
            )}
            {processedSkipped.length > 0 && (
              <button className="btn sm" onClick={() => reprocessSkipped()}>
                <RotateCcw size={13} /> 仍要重新处理
              </button>
            )}
            <button className="icon-x" onClick={dismissSkipped} aria-label="关闭导入提示">
              <X size={14} />
            </button>
          </div>
        </div>
      )}

      {showSkipped && skipped.length > 0 && (
        <div className="card wf-skipped">
          <div className="card-h">已跳过 <span className="hint">{skipped.length} 个文件，不参与本次扫描</span></div>
          <ul className="wf-file-list">
            {skipped.map((f) => (
              <li key={f.path} className="wf-file is-skipped">
                {fmtIcon(f.format)}
                <div className="wf-file-main">
                  <div className="wf-file-name" title={f.path}>{f.name}</div>
                  <div className="wf-file-sub">
                    {f.reason === "processed"
                      ? `${fmtTime(f.processedAt)} 处理过${f.outputPath ? ` · 输出 ${baseName(f.outputPath)}` : ""}`
                      : f.reason === "unsupported"
                        ? "仅支持 PDF / DOCX / XLSX / TXT / MD"
                        : `本工具导出的脱敏副本 · ${dirName(f.path)}`}
                  </div>
                </div>
                <span className={`wf-badge s-${f.reason}`}>{SKIP_REASON[f.reason]}</span>
                {f.reason === "processed" ? (
                  <button className="btn sm ghost" onClick={() => reprocessSkipped([f.path])}>仍要处理</button>
                ) : (
                  <span className="wf-file-act" />
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      {files.length > 0 ? (
        <div className="card wf-files">
          <div className="card-h">
            待处理文档
            <span className="hint">{files.length} 个文件 · {formatSize(totalSize)}</span>
            <span style={{ flex: 1 }} />
            <button className="btn sm ghost" onClick={clearFiles} disabled={scanning}>清空列表</button>
          </div>
          <ul className="wf-file-list">
            {files.map((f, i) => {
              const state = scanning && progress
                ? i < progress.done ? "done" : i === progress.done ? "current" : "queued"
                : null;
              return (
                <li key={f.path} className={`wf-file ${state ? `is-${state}` : ""}`} style={{ animationDelay: `${Math.min(i, 12) * 18}ms` }}>
                  {fmtIcon(f.format)}
                  <div className="wf-file-main">
                    <div className="wf-file-name" title={f.path}>{f.name}</div>
                    <div className="wf-file-sub" title={f.path}>
                      {formatSize(f.size)} · {dirName(f.path)}
                    </div>
                  </div>
                  <span
                    className={`wf-badge ${
                      state === "done" ? "s-done" : state === "current" ? "s-current" : f.format === "未知" ? "s-unsupported" : f.changed ? "s-changed" : f.reprocess ? "s-reprocess" : "s-pending"
                    }`}
                  >
                    {state === "done"
                      ? "已扫描"
                      : state === "current"
                        ? "扫描中"
                        : f.format === "未知"
                          ? "不支持"
                          : f.changed
                            ? "内容已变化"
                            : f.reprocess
                              ? "重新处理"
                              : "待处理"}
                  </span>
                  <button
                    className="btn sm ghost wf-file-act"
                    onClick={() => removeFile(f.path)}
                    aria-label={`移除 ${f.name}`}
                    title="移除"
                    disabled={scanning}
                  >
                    <X size={14} />
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      ) : (
        !lastBatch && (
          <div className="wf-empty">
            <div className="wf-empty-row">
              <span className="wf-empty-n">1</span>导入文档，文件夹会递归展开
            </div>
            <div className="wf-empty-row">
              <span className="wf-empty-n">2</span>扫描并在预览中逐项确认命中
            </div>
            <div className="wf-empty-row">
              <span className="wf-empty-n">3</span>导出脱敏副本，查看红 / 黄 / 绿结论
            </div>
          </div>
        )
      )}

      {scanError && (
        <div className="wf-error" role="alert">
          <div>
            <b>扫描失败</b>：{scanError}
            <div className="wf-muted">文件列表已保留，可调整后重试。</div>
          </div>
          <button className="btn sm" onClick={() => void onScan()}>
            <RotateCcw size={13} /> 重试
          </button>
          <button className="icon-x" onClick={() => setScanError(null)} aria-label="关闭错误提示">
            <X size={14} />
          </button>
        </div>
      )}

      {scanning && progress && (
        <ScanProgressPanel
          progress={progress}
          startedAt={scanStarted}
          currentName={files[Math.min(progress.done, files.length - 1)]?.name ?? ""}
          onCancel={onCancelScan}
        />
      )}

      <div className="wf-actionbar">
        <button
          className="btn primary wf-cta"
          disabled={scanning || files.length === 0 || importing}
          onClick={() => void onScan()}
        >
          {scanning ? <span className="spinner" /> : <ScanSearch size={15} />}
          {files.length ? `开始扫描 ${files.length} 个文件` : "开始扫描"}
        </button>
        <span className="note">
          <ShieldCheck size={14} aria-hidden="true" />
          文档内容仅在本机处理，扫描与导出过程不会把文档发送到网络
        </span>
        <span style={{ flex: 1 }} />
        <button className="btn ghost" onClick={() => navigate("strategy")} title="调整检测规则与输出方式">
          调整策略
          <ArrowRight size={14} />
        </button>
      </div>

      {summaryOpen && lastBatch && (
        <ExportSummary
          result={lastBatch.result}
          snapshot={lastBatch.snapshot}
          outputDir={lastBatch.outputDir}
          toast={toast}
          onClose={() => setSummaryOpen(false)}
          onOpenFolder={openExportFolder}
          onAudit={() => {
            setSummaryOpen(false);
            navigate("audit");
          }}
          onNewFiles={() => {
            setSummaryOpen(false);
            void onPickFiles();
          }}
        />
      )}
    </div>
  );
}
