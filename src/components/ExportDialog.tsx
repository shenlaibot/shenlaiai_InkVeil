// 导出对话框：一处完成“确认 → 逐文件进度 → 失败可重试”。
// 确认信息：输出目录、映射保存位置、导出前预检（红/黄风险摘要）；检测不完整的文件需选理由才能放行。
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { FolderOpen, KeyRound, Lock, RotateCcw, TriangleAlert, X } from "lucide-react";
import { useApp } from "../store";
import { confirmRedact, pickOutputDir, vaultInfo } from "../api";
import type { ConfirmedHit, ProgressEvent, RedactInput, RedactResult, ScannedFile } from "../types";
import { LOW_CONF, RED_WARNING, judgeAll, snapshotFile } from "./verdict";
import type { ExportSnapshot, FileSnapshot } from "./verdict";
import type { VerdictLevel } from "./workflow";
import { fmtIcon } from "./FileIcon";

const OUT_DIR_KEY = "anonkit.lastOutputDir.v1";

function loadOutDir(): string | null {
  try {
    return localStorage.getItem(OUT_DIR_KEY);
  } catch {
    return null;
  }
}

type Phase = "confirm" | "running" | "error";

export function ExportDialog(props: { files: ScannedFile[]; onClose: () => void }) {
  const { files, onClose } = props;
  const {
    confirmed, config, vault, setVault, addAudit, toast, setLastRedact, completeBatch, setExportState, navigate,
  } = useApp();
  const [phase, setPhase] = useState<Phase>("confirm");
  const [outDir, setOutDir] = useState<string | null>(loadOutDir);
  const [note, setNote] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  /** 每个文件的处理结果提示（来自 redact-progress，按文件名） */
  const [msgs, setMsgs] = useState<Record<string, string>>({});
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  // 预检：只用扫描结果与勾选状态，不读文件
  const pre = useMemo(() => {
    const risky = files.filter((f) => f.warnings.some((w) => RED_WARNING.test(w)));
    const lowConf = files.reduce((a, f) => a + f.hits.filter((h) => !confirmed[h.id] && h.confidence < LOW_CONF).length, 0);
    const noHits = files.filter((f) => !f.hits.some((h) => confirmed[h.id]));
    const willExport = files.length - noHits.length;
    const hits = files.reduce((a, f) => a + f.hits.filter((h) => confirmed[h.id]).length, 0);
    return { risky, lowConf, noHits, willExport, hits };
  }, [files, confirmed]);

  const running = phase === "running";

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !running) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, running]);

  // 逐文件进度
  useEffect(() => {
    let disposed = false;
    let un: (() => void) | null = null;
    listen<ProgressEvent>("redact-progress", (e) => {
      setProgress(e.payload);
      setMsgs((m) => ({ ...m, [e.payload.file]: e.payload.message }));
    })
      .then((fn) => {
        if (disposed) fn();
        else un = fn;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      un?.();
    };
  }, []);

  const chooseDir = useCallback(async () => {
    try {
      const d = await pickOutputDir();
      if (d) {
        setOutDir(d);
        try {
          localStorage.setItem(OUT_DIR_KEY, d);
        } catch {
          /* ignore */
        }
      }
      return d;
    } catch (e) {
      toast(`选择输出目录失败: ${e}`);
      return null;
    }
  }, [toast]);

  const start = useCallback(async () => {
    if (pre.risky.length > 0 && !note) return;
    let dir = outDir;
    if (!dir) dir = await chooseDir();
    if (!dir) return;
    // 未勾选任何命中的文件也传给后端：后端标记“未勾选，跳过”，结论页列为黄色并可重新导入
    const inputs: RedactInput[] = files.map((f) => ({
      path: f.path,
      hits: f.hits
        .filter((h) => confirmed[h.id])
        .map<ConfirmedHit>((h) => ({ rule_id: h.rule_id, rule_label: h.rule_label, start: h.start, end: h.end, text: h.text })),
      // 用户未勾选的命中文本：同文本在残留复检中视为“有意保留”
      excluded_texts: f.hits.filter((h) => !confirmed[h.id]).map((h) => h.text),
    }));
    setPhase("running");
    setError(null);
    setProgress({ done: 0, total: inputs.length, file: "", hit_count: 0, message: "" });
    setMsgs({});
    setExportState("exporting");
    const riskyNames = new Set(pre.risky.map((f) => f.path));
    try {
      const result: RedactResult = await confirmRedact(inputs, dir, {
        strategies: config.strategies,
        passphrase: config.passphrase,
      });
      setLastRedact(result);
      const appVersion = await getVersion().catch(() => "未知");
      const enabled = config.rules.filter((r) => r.enabled).length;
      // 导出时刻快照：结论与报告以此为准
      const snapshot: ExportSnapshot = {
        time: Date.now(),
        appVersion,
        rulesInfo: `启用检测规则 ${enabled} 条，自定义词典 ${config.dictTerms?.length ?? 0} 条`,
        files: files.map((f) => snapshotFile(f, confirmed, note)),
      };
      // 会话审计（不含原文）
      const now = new Date().toLocaleString("zh-CN", { hour12: false });
      for (const fr of result.files) {
        const file = files.find((f) => f.path === fr.input_path);
        if (!file) continue;
        const byType = new Map<string, number>();
        for (const h of file.hits.filter((x) => confirmed[x.id])) byType.set(h.rule_label, (byType.get(h.rule_label) ?? 0) + 1);
        addAudit({
          time: now,
          file: file.name,
          hit_count: fr.confirmed,
          types: [...byType.entries()].map(([k, v]) => `${k}×${v}`).join(" "),
          output: fr.output_path ? fr.output_path.split(/[\\/]/).pop() ?? "" : "失败",
          // 审计不得携带后端错误文本（可能含原文片段）；只记录状态
          mapping: fr.error ? "导出失败" : result.vault_saved ? "已存入项目" : result.mapping_path ? "已导出（加密）" : "未生成",
          is_deep: fr.is_deep,
          note:
            note && riskyNames.has(file.path)
              ? `带风险放行：${note}`
              : fr.error?.includes("复检")
                ? "复检发现残留，已阻止导出"
                : "",
        });
      }
      const snaps = new Map<string, FileSnapshot>(snapshot.files.map((f) => [f.path, f]));
      const levels = new Map<string, VerdictLevel>(judgeAll(result.files, snaps).map((v) => [v.result.input_path, v.level]));
      const okCount = result.files.filter((f) => f.output_path && !f.error).length;
      if (result.vault_saved) {
        toast(`已导出 ${okCount} 个脱敏副本；映射已保存到项目（共 ${result.mapping_entries} 条）`);
        void vaultInfo().then(setVault).catch(() => undefined);
      } else if (vault) {
        toast(`已导出 ${okCount} 个脱敏副本，但映射未能保存到项目，请立即检查项目文件的写入权限，否则这批脱敏稿无法还原`);
      } else {
        toast(
          result.mapping_path
            ? `已导出 ${okCount} 个脱敏副本 + 加密映射文件（${result.mapping_entries} 条映射）`
            : `已导出 ${okCount} 个脱敏副本。未打开项目也未设置口令，未生成映射文件，脱敏稿无法还原`,
        );
      }
      // 收尾：记录已处理、清空工作区、回到工作台并打开结论页（本对话框随预览页一起卸载）
      completeBatch({ result, snapshot, levels, overridden: note ? riskyNames : new Set(), outputDir: dir });
    } catch (e) {
      if (!alive.current) return;
      setExportState("idle");
      setError(String(e));
      setPhase("error");
    }
  }, [pre, note, outDir, chooseDir, files, confirmed, config, setExportState, setLastRedact, addAudit, toast, vault, setVault, completeBatch]);

  // 映射保存位置
  const mapping = vault
    ? { icon: <Lock size={14} />, text: `保存到项目「${vault.name}」`, sub: "可在还原页用该项目还原 AI 返回的文稿", warn: false }
    : config.passphrase
      ? { icon: <KeyRound size={14} />, text: "加密映射文件（anonkit.anonymap.enc）", sub: "写入输出目录，还原时需要口令", warn: false }
      : { icon: <TriangleAlert size={14} />, text: "不保存映射", sub: "未打开项目也未设置口令：导出后无法还原占位符", warn: true };

  const total = Math.max(1, progress?.total ?? files.length);
  const pct = Math.round(((progress?.done ?? 0) / total) * 100);

  return (
    <div className="modal-mask" onClick={() => !running && onClose()}>
      <div
        className="modal wf-export wf-pop"
        role="dialog"
        aria-modal="true"
        aria-labelledby="wf-export-title"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-h" id="wf-export-title">
          {phase === "running" ? "正在导出脱敏副本…" : phase === "error" ? "导出未完成" : "导出脱敏副本"}
          <span style={{ flex: 1 }} />
          {!running && (
            <button className="icon-x" onClick={onClose} aria-label="关闭导出对话框">
              <X size={15} />
            </button>
          )}
        </div>
        <div className="modal-b">
          {phase !== "running" && (
            <>
              <dl className="wf-kv">
                <dt>导出内容</dt>
                <dd>
                  {pre.willExport} 个文件 · 已确认 {pre.hits} 处命中
                  {pre.noHits.length > 0 && <span className="wf-muted">（{pre.noHits.length} 个文件未勾选命中，将不导出）</span>}
                </dd>
                <dt>输出目录</dt>
                <dd className="wf-kv-row">
                  <span className={`wf-path ${outDir ? "" : "wf-muted"}`} title={outDir ?? ""}>{outDir ?? "尚未选择"}</span>
                  <button className="btn sm" onClick={() => void chooseDir()}>
                    <FolderOpen size={13} /> {outDir ? "更改" : "选择"}
                  </button>
                </dd>
                <dt>映射保存</dt>
                <dd className={`wf-kv-row ${mapping.warn ? "is-warn" : ""}`}>
                  {mapping.icon}
                  <span>
                    {mapping.text}
                    <span className="wf-muted"> · {mapping.sub}</span>
                  </span>
                  {mapping.warn && (
                    <button className="btn sm ghost" onClick={() => { onClose(); navigate("vault"); }}>去打开项目</button>
                  )}
                </dd>
              </dl>

              <div className="wf-precheck" aria-label="导出前预检">
                <div className="wf-precheck-h">导出前预检</div>
                {pre.risky.length === 0 && pre.lowConf === 0 && pre.noHits.length === 0 ? (
                  <div className="wf-pc-row g"><i aria-hidden="true" />未发现需要额外处理的项。导出后会自动复检，并给出每个文件的红 / 黄 / 绿结论。</div>
                ) : (
                  <>
                    {pre.risky.length > 0 && (
                      <div className="wf-pc-row r">
                        <i aria-hidden="true" />
                        <span><b>红 · {pre.risky.length} 个文件无法确认已完整检测</b>（检测通道失效、被跳过或没有可提取文字）：
                          {pre.risky.map((f) => f.name).join("、")}</span>
                      </div>
                    )}
                    {pre.lowConf > 0 && (
                      <div className="wf-pc-row y"><i aria-hidden="true" /><span><b>黄 · {pre.lowConf} 处低置信度命中未勾选</b>，导出后会保留原文</span></div>
                    )}
                    {pre.noHits.length > 0 && (
                      <div className="wf-pc-row y"><i aria-hidden="true" /><span><b>黄 · {pre.noHits.length} 个文件未勾选任何命中</b>，不会生成脱敏副本</span></div>
                    )}
                  </>
                )}
                {pre.risky.length > 0 && (
                  <label className="wf-reason">
                    <span>“没有命中”不代表没有敏感信息。如确需继续，请选择理由（记入审计，不含文档内容）：</span>
                    <select value={note} onChange={(e) => setNote(e.target.value)}>
                      <option value="">请选择理由…</option>
                      <option value="已人工核对原文">已人工核对原文</option>
                      <option value="仅内部使用，不对外发送">仅内部使用，不对外发送</option>
                      <option value="扫描页已人工涂黑或不含敏感信息">扫描页已人工涂黑或不含敏感信息</option>
                      <option value="其他（已自行评估风险）">其他（已自行评估风险）</option>
                    </select>
                  </label>
                )}
              </div>

              {phase === "error" && (
                <div className="wf-error" role="alert">
                  <div>
                    <b>导出失败</b>：{error}
                    <div className="wf-muted">勾选状态已保留，可重试或返回修改。</div>
                  </div>
                </div>
              )}
            </>
          )}

          {phase === "running" && (
            <div className="wf-export-run" role="status" aria-live="polite">
              <div className="progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-label="导出进度">
                <div className="progress-fill" style={{ width: `${Math.max(pct, 3)}%` }} />
              </div>
              <div className="wf-scan-meta">
                <span>已完成 {progress?.done ?? 0}/{progress?.total ?? files.length}</span>
                <span>导出后自动复检残留</span>
              </div>
              <ul className="wf-file-list wf-run-list">
                {files.map((f, i) => {
                  const done = (progress?.done ?? 0) > i;
                  const cur = (progress?.done ?? 0) === i;
                  return (
                    <li key={f.path} className={`wf-file ${done ? "is-done" : cur ? "is-current" : "is-queued"}`}>
                      {fmtIcon(f.format)}
                      <div className="wf-file-main"><div className="wf-file-name">{f.name}</div></div>
                      <span className={`wf-badge ${done ? "s-done" : cur ? "s-current" : "s-pending"}`}>
                        {done ? msgs[f.name] || "已完成" : cur ? "处理中" : "排队中"}
                      </span>
                    </li>
                  );
                })}
              </ul>
            </div>
          )}
        </div>
        <div className="modal-f">
          {phase === "running" ? (
            <span className="wf-muted">导出进行中，请勿关闭应用…</span>
          ) : (
            <>
              <button className="btn ghost" onClick={onClose}>返回检查</button>
              <button
                className="btn primary"
                disabled={(pre.risky.length > 0 && !note) || pre.hits === 0}
                onClick={() => void start()}
              >
                {phase === "error" ? <><RotateCcw size={14} /> 重试导出</> : pre.risky.length > 0 ? "带风险继续导出" : outDir ? "开始导出" : "选择目录并导出"}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
