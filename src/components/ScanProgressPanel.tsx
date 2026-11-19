// 扫描进度：当前文件、已完成/总数、预计剩余，可取消。
import { useEffect, useState } from "react";
import type { ProgressEvent } from "../types";
import { estimateRemaining, formatEta } from "./workflow";

export function ScanProgressPanel(props: {
  progress: ProgressEvent;
  startedAt: number;
  currentName: string;
  onCancel: () => void;
}) {
  const { progress, startedAt } = props;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, []);
  const total = Math.max(1, progress.total);
  const pct = Math.round((progress.done / total) * 100);
  const eta = estimateRemaining(startedAt, now, progress.done, progress.total);
  const elapsed = Math.max(0, Math.round((now - startedAt) / 1000));

  return (
    <div className="wf-scan" role="status" aria-live="polite">
      <div className="wf-scan-top">
        <span className="spinner dark" aria-hidden="true" />
        <div className="wf-scan-title">
          正在扫描 <b>{props.currentName || "…"}</b>
        </div>
        <span className="wf-scan-count">
          已完成 {progress.done}/{progress.total}
        </span>
        <button className="btn sm" onClick={props.onCancel}>取消扫描</button>
      </div>
      <div
        className="progress-track wf-scan-track"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        aria-label="扫描进度"
      >
        <div className="progress-fill" style={{ width: `${Math.max(pct, 3)}%` }} />
      </div>
      <div className="wf-scan-meta">
        <span>
          {progress.file ? `上一个：${progress.file}${progress.message ? `（${progress.message}）` : ""}` : "正在解析第一个文档…"}
        </span>
        <span>
          {formatEta(eta)} · 已用 {elapsed} 秒
        </span>
      </div>
    </div>
  );
}
