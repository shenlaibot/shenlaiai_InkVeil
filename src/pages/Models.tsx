// 模型市场：本地 NER 模型列表 / 下载 / 暂停 / 删除（轮询状态）。
// 四种状态（下载中 / 已暂停 / 失败 / 已安装）用颜色 + 图标 + 文字同时区分；下载中显示速度与剩余时间估算。
import { useCallback, useEffect, useRef, useState } from "react";
import {
  BrainCircuit,
  CircleAlert,
  CircleCheck,
  CloudDownload,
  FolderOpen,
  Pause,
  Play,
  RotateCw,
  Trash2,
  Cpu,
} from "lucide-react";
import {
  deleteModel,
  listModels,
  modelInstallDir,
  modelStatus,
  openInFolder,
  pauseModelDownload,
  startModelDownload,
} from "../api";
import { useApp } from "../store";
import type { DownloadStatus, ModelEntry } from "../types";
import { Badge, Callout, EmptyState, PageHead, SkeletonRows, Switch, fmtBytes } from "../components/ui/primitives";
import type { Tone } from "../components/ui/primitives";
import { confirmDialog } from "../components/ui/Dialog";
import { notify } from "../components/ui/Toast";
import { MODEL_DL_EVENT } from "../components/ui/NetStatus";

type UiState = "idle" | "downloading" | "verifying" | "paused" | "failed" | "installed";

function uiState(st: DownloadStatus | undefined): UiState {
  if (!st) return "idle";
  if (st.installed) return "installed";
  if (st.state === "downloading") return st.total > 0 && st.received >= st.total ? "verifying" : "downloading";
  if (st.state === "paused") return "paused";
  if (st.state === "failed") return "failed";
  return "idle";
}

const STATE_META: Record<UiState, { label: string; tone: Tone; icon: React.ReactNode }> = {
  idle: { label: "未安装", tone: "neutral", icon: <CloudDownload size={12} /> },
  downloading: { label: "下载中", tone: "info", icon: <CloudDownload size={12} /> },
  verifying: { label: "校验中", tone: "info", icon: <CloudDownload size={12} /> },
  paused: { label: "已暂停", tone: "warn", icon: <Pause size={12} /> },
  failed: { label: "下载失败", tone: "danger", icon: <CircleAlert size={12} /> },
  installed: { label: "已安装", tone: "success", icon: <CircleCheck size={12} /> },
};

function fmtEta(sec: number): string {
  if (!isFinite(sec) || sec <= 0) return "";
  if (sec < 60) return `约 ${Math.max(1, Math.round(sec))} 秒`;
  if (sec < 3600) return `约 ${Math.round(sec / 60)} 分钟`;
  return `约 ${(sec / 3600).toFixed(1)} 小时`;
}

/** 把后端的长报错归纳为一句人话原因 */
function failReason(err: string | null | undefined): string {
  const e = err ?? "";
  if (/无法连接|timed? ?out|超时|dns|connect/i.test(e)) return "无法连接到下载源。可能是网络不通、需要代理，或下载源暂时不可用。";
  if (/404|not found/i.test(e)) return "下载源上找不到该文件（可能已移动）。";
  if (/sha|校验|hash|checksum/i.test(e)) return "下载的文件校验不通过，已丢弃，请重试。";
  if (/space|磁盘|空间/i.test(e)) return "磁盘空间不足。";
  if (/permission|权限|denied/i.test(e)) return "没有写入模型目录的权限。";
  return "下载没有完成。";
}

interface Speed {
  t: number;
  received: number;
  bps: number;
}

export function ModelsPage() {
  const { setModelReady, config, updateConfig } = useApp();
  const [models, setModels] = useState<ModelEntry[] | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [installDirs, setInstallDirs] = useState<Record<string, string>>({});
  const [status, setStatus] = useState<Record<string, DownloadStatus>>({});
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const speed = useRef<Record<string, Speed>>({});
  const [, force] = useState(0);

  const refreshStatus = useCallback(
    async (ids: string[]) => {
      try {
        const entries = await Promise.all(ids.map(async (id) => [id, await modelStatus(id)] as const));
        const now = performance.now();
        const next: Record<string, DownloadStatus> = {};
        for (const [id, st] of entries) {
          next[id] = st;
          // 速度：指数平滑，抵消轮询抖动
          const prev = speed.current[id];
          if (st.state === "downloading" && prev && st.received >= prev.received && now > prev.t) {
            const inst = ((st.received - prev.received) * 1000) / (now - prev.t);
            const bps = prev.bps ? prev.bps * 0.8 + inst * 0.2 : inst;
            speed.current[id] = { t: now, received: st.received, bps };
          } else {
            speed.current[id] = { t: now, received: st.received, bps: st.state === "downloading" ? prev?.bps ?? 0 : 0 };
          }
        }
        setStatus((p) => ({ ...p, ...next }));
        const active = entries.find(([, st]) => st.state === "downloading")?.[1];
        window.dispatchEvent(
          new CustomEvent(MODEL_DL_EVENT, {
            detail: active ? { pct: active.total > 0 ? Math.floor((active.received / active.total) * 100) : 0 } : null,
          }),
        );
        setModelReady(entries.some(([, st]) => st.installed));
        force((n) => n + 1);
      } catch {
        /* 轮询失败忽略，下一轮再取 */
      }
    },
    [setModelReady],
  );

  const load = useCallback(() => {
    setLoadErr(null);
    setModels(null);
    listModels()
      .then((list) => {
        setModels(list);
        return refreshStatus(list.map((m) => m.id));
      })
      .catch((e) => {
        setModels([]);
        setLoadErr(String(e));
      });
  }, [refreshStatus]);

  useEffect(load, [load]);

  const anyActive = Object.values(status).some((s) => s.state === "downloading");
  useEffect(() => {
    if (!anyActive || !models) return;
    const ids = models.map((m) => m.id);
    const timer = window.setInterval(() => void refreshStatus(ids), 500);
    return () => window.clearInterval(timer);
  }, [anyActive, models, refreshStatus]);

  useEffect(() => {
    for (const m of models ?? []) {
      if (status[m.id]?.state === "failed" && !installDirs[m.id]) {
        void modelInstallDir(m.id)
          .then((d) => setInstallDirs((x) => ({ ...x, [m.id]: d })))
          .catch(() => undefined);
      }
    }
  }, [models, status, installDirs]);

  const ids = () => (models ?? []).map((x) => x.id);

  const onDownload = async (m: ModelEntry) => {
    // 联网默认关闭：点击“开启联网并下载”即表示本次同意，同时打开开关（之后断点续传不再询问）
    const consent = !config.allowModelDownload;
    if (consent) updateConfig({ allowModelDownload: true });
    setBusy((b) => ({ ...b, [m.id]: true }));
    try {
      await startModelDownload(m.id, consent || undefined);
      await refreshStatus(ids());
    } catch (e) {
      notify("无法开始下载", { tone: "danger", detail: String(e) });
    } finally {
      setBusy((b) => ({ ...b, [m.id]: false }));
    }
  };

  const onPause = async (id: string) => {
    try {
      await pauseModelDownload(id);
      await refreshStatus(ids());
    } catch (e) {
      notify("暂停失败", { tone: "danger", detail: String(e) });
    }
  };

  const onDelete = async (m: ModelEntry) => {
    const ok = await confirmDialog({
      title: `删除「${m.name}」？`,
      message: `将从本机删除约 ${fmtBytes(m.size_bytes)} 的模型文件。删除后扫描不再使用模型补充识别，需要时可重新下载。`,
      confirmText: "删除模型",
      tone: "danger",
    });
    if (!ok) return;
    try {
      await deleteModel(m.id);
      await refreshStatus(ids());
      notify("模型已删除", { tone: "success" });
    } catch (e) {
      notify("删除失败", { tone: "danger", detail: String(e) });
    }
  };

  return (
    <div>
      <PageHead
        title="模型市场"
        sub="本地识别模型是可选增强：用于补充识别没有明显前缀的人名、机构和地址。不安装模型时，规则、词典与上下文识别照常工作。"
      />

      <div className="card" style={{ marginBottom: 16 }}>
        <div className="strat-row">
          <div className="sr-text">
            <div className="sr-lbl">允许下载模型</div>
            <div className="sr-desc">
              {config.allowModelDownload
                ? "已允许：只在你点击下载或继续时连接模型下载源，只下载模型文件，不上传文档内容。"
                : "未允许：模型市场不会发起任何网络请求（包括断点续传）。已安装的模型照常在本地使用。"}
              {" "}检查软件更新是另一项联网行为，只在关于页点击时发生。
            </div>
          </div>
          <Switch
            checked={config.allowModelDownload}
            onChange={(v) => updateConfig({ allowModelDownload: v })}
            label="允许下载模型"
          />
        </div>
      </div>

      {models === null ? (
        <div className="card">
          <SkeletonRows rows={2} label="正在读取模型列表" />
        </div>
      ) : loadErr ? (
        <div className="card">
          <EmptyState
            tone="danger"
            icon={<CircleAlert size={22} />}
            title="读取模型列表失败"
            desc={<>这不影响规则识别。可以重试；如果一直失败，请重启应用。<br /><span className="path">{loadErr}</span></>}
            action={<button className="btn" onClick={load}><RotateCw size={14} />重试</button>}
          />
        </div>
      ) : models.length === 0 ? (
        <div className="card">
          <EmptyState icon={<BrainCircuit size={22} />} title="暂无可用模型" desc="当前版本没有提供可下载的模型。" />
        </div>
      ) : (
        models.map((m) => {
          const st = status[m.id];
          const ui = uiState(st);
          const meta = STATE_META[ui];
          const pct = st && st.total > 0 ? Math.min(100, Math.floor((st.received / st.total) * 100)) : 0;
          const bps = speed.current[m.id]?.bps ?? 0;
          const remain = st ? Math.max(0, st.total - st.received) : 0;
          const showProgress = ui === "downloading" || ui === "verifying" || ui === "paused" || (ui === "failed" && pct > 0);
          const allow = config.allowModelDownload;
          return (
            <section className={`mdl state-${ui === "verifying" ? "downloading" : ui}`} key={m.id} aria-label={m.name}>
              <div className="mdl-main">
                <div className="mdl-icon" aria-hidden>
                  <BrainCircuit size={22} />
                </div>
                <div className="mdl-info">
                  <div className="mdl-title">
                    {m.name}
                    <Badge tone={meta.tone} icon={meta.icon}>
                      {meta.label}
                      {(ui === "downloading" || ui === "paused") && ` ${pct}%`}
                    </Badge>
                  </div>
                  <div className="mdl-meta">
                    <span>v{m.version}</span>
                    <span>{fmtBytes(m.size_bytes)}</span>
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                      <Cpu size={12} aria-hidden />本机 CPU 运行
                    </span>
                    <span>许可：{m.license}</span>
                  </div>
                  <div className="mdl-desc">{m.description}</div>
                  {ui === "idle" && !allow && (
                    <div className="hint" style={{ marginTop: 8 }}>
                      点击下载将连接 huggingface.co（失败时改用 hf-mirror.com），下载约 {fmtBytes(m.size_bytes)}，不上传任何文档内容。
                    </div>
                  )}
                </div>
                <div className="mdl-actions">
                  {ui === "installed" && (
                    <button className="btn danger" onClick={() => void onDelete(m)}>
                      <Trash2 size={14} />删除模型
                    </button>
                  )}
                  {(ui === "downloading" || ui === "verifying") && (
                    <button className="btn" onClick={() => void onPause(m.id)} disabled={ui === "verifying"}>
                      <Pause size={14} />暂停
                    </button>
                  )}
                  {ui === "paused" && (
                    <button className="btn primary" disabled={busy[m.id]} onClick={() => void onDownload(m)}>
                      {busy[m.id] ? <span className="spinner" /> : <Play size={14} />}
                      {allow ? "继续下载" : "开启联网并继续"}
                    </button>
                  )}
                  {ui === "failed" && (
                    <button className="btn primary" disabled={busy[m.id]} onClick={() => void onDownload(m)}>
                      {busy[m.id] ? <span className="spinner" /> : <RotateCw size={14} />}
                      {allow ? "重试下载" : "开启联网并重试"}
                    </button>
                  )}
                  {ui === "idle" && (
                    <button className="btn primary" disabled={busy[m.id]} onClick={() => void onDownload(m)}>
                      {busy[m.id] ? <span className="spinner" /> : <CloudDownload size={14} />}
                      {allow ? "下载" : "开启联网并下载"}
                    </button>
                  )}
                </div>
              </div>

              {showProgress && st && (
                <div className="mdl-progress">
                  <div
                    className={`pbar ${ui === "downloading" ? "active" : ui === "paused" ? "paused" : ui === "failed" ? "failed" : "active"}`}
                    role="progressbar"
                    aria-label={`${m.name} 下载进度`}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={pct}
                  >
                    <i style={{ width: `${pct}%` }} />
                  </div>
                  <div className="pmeta">
                    <span>
                      {fmtBytes(st.received)} / {fmtBytes(st.total)} · {pct}%
                    </span>
                    <span>
                      {ui === "downloading" && bps > 0
                        ? `${fmtBytes(bps)}/s · 剩余${fmtEta(remain / bps)}`
                        : ui === "downloading"
                          ? "正在估算速度…"
                          : ui === "verifying"
                            ? "下载完成，正在校验文件完整性…"
                            : ui === "paused"
                              ? "已暂停，继续时从已下载部分接着下"
                              : "已保留已下载部分，重试时接着下"}
                    </span>
                  </div>
                  {ui === "downloading" && st.note && <div className="pmeta"><span className="pnote">{st.note}</span></div>}
                </div>
              )}

              {ui === "failed" && (
                <div className="mdl-fail" role="alert">
                  <div className="mf-reason">
                    <CircleAlert size={15} />
                    <span>{failReason(st?.error)}</span>
                  </div>
                  {st?.error && <div className="mf-raw">{st.error}</div>}
                  <div style={{ marginTop: 10, fontWeight: 600, color: "var(--text)" }}>接下来可以：</div>
                  <ol className="mf-steps">
                    <li>检查网络或代理后点“重试下载”，会自动换用备用下载源，并从已下载部分继续。</li>
                    <li>仍然失败时，可手动安装：在能联网的电脑上下载下列文件，放进模型目录，再回到本页即可识别（会校验完整性）。</li>
                  </ol>
                  <details className="mf-manual">
                    <summary>手动安装所需信息</summary>
                    <dl className="kv">
                      <dt>文件</dt>
                      <dd className="path">{m.files.map((f) => `${f.name}（${fmtBytes(f.size)}）`).join("、")}</dd>
                      <dt>来源</dt>
                      <dd className="path">huggingface.co 或 hf-mirror.com：protectai/gyr66-bert-base-chinese-finetuned-ner-onnx</dd>
                      <dt>目录</dt>
                      <dd>
                        <span className="path">{installDirs[m.id] ?? "读取中…"}</span>
                        {installDirs[m.id] && (
                          <button
                            className="btn sm ghost"
                            style={{ marginLeft: 6 }}
                            onClick={() => void openInFolder(installDirs[m.id]).catch((e) => notify("无法打开目录", { tone: "danger", detail: String(e) }))}
                          >
                            <FolderOpen size={13} />打开目录
                          </button>
                        )}
                      </dd>
                    </dl>
                  </details>
                </div>
              )}
            </section>
          );
        })
      )}

      <div style={{ marginTop: 16 }}>
      <Callout tone="neutral" icon={<Cpu size={16} />}>
        <span style={{ fontSize: 12.5 }}>
          模型保存在应用数据目录，在本机 CPU 上运行（ONNX Runtime），不需要显卡；扫描时文档内容只交给本机模型处理。可随时关闭或删除。
        </span>
      </Callout>
      </div>
    </div>
  );
}
