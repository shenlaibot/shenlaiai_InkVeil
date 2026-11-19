// 标题栏联网状态：如实反映三种情况——未开启联网 / 已允许下载模型（当前无任务）/ 正在下载模型 / 正在检查更新。
// 文档内容只在本机处理；本应用的联网用途只有“下载模型”和“检查更新”（后者仅在用户点击时发生）。
import { useEffect, useRef, useState } from "react";
import { listModels, modelStatus } from "../../api";
import { useApp } from "../../store";

/** 关于页检查更新时广播，标题栏据此显示“正在检查更新” */
export const NET_UPDATE_EVENT = "anonkit:net-update";
export function announceUpdateCheck(active: boolean) {
  window.dispatchEvent(new CustomEvent(NET_UPDATE_EVENT, { detail: active }));
}

/** 模型页轮询到下载状态时广播（null = 没有进行中的下载） */
export const MODEL_DL_EVENT = "anonkit:model-dl";

export function NetStatus() {
  const { config, navigate, page } = useApp();
  const [dl, setDl] = useState<{ pct: number } | null>(null);
  const [checking, setChecking] = useState(false);
  const [open, setOpen] = useState(false);
  const ids = useRef<string[] | null>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const onDl = (e: Event) => setDl((e as CustomEvent<{ pct: number } | null>).detail);
    window.addEventListener(MODEL_DL_EVENT, onDl);
    return () => window.removeEventListener(MODEL_DL_EVENT, onDl);
  }, []);

  useEffect(() => {
    const on = (e: Event) => setChecking(!!(e as CustomEvent).detail);
    window.addEventListener(NET_UPDATE_EVENT, on);
    return () => window.removeEventListener(NET_UPDATE_EVENT, on);
  }, []);

  // 轮询下载状态（本地 IPC，本身不联网）；模型页自己有更快的轮询，这里放慢。
  // 不以“是否允许下载”为前提：以后台实际状态为准，避免显示与事实不符。
  useEffect(() => {
    let stop = false;
    let timer = 0;
    const tick = async () => {
      try {
        if (!ids.current) ids.current = (await listModels()).map((m) => m.id);
        const sts = await Promise.all(ids.current.map((id) => modelStatus(id)));
        const active = sts.find((s) => s.state === "downloading");
        if (!stop) setDl(active ? { pct: active.total > 0 ? Math.round((active.received / active.total) * 100) : 0 } : null);
        if (!stop) timer = window.setTimeout(tick, active ? 1500 : config.allowModelDownload ? 6000 : 15000);
      } catch {
        if (!stop) timer = window.setTimeout(tick, 10000);
      }
    };
    // 模型页打开时由该页轮询，避免双倍请求（该页通过事件把状态告诉这里）
    if (page !== "models") void tick();
    return () => {
      stop = true;
      window.clearTimeout(timer);
    };
  }, [config.allowModelDownload, page]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!popRef.current?.contains(e.target as Node) && !btnRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
        btnRef.current?.focus();
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  let cls = "offline";
  let text = "未开启联网";
  if (checking) {
    cls = "active";
    text = "正在检查更新";
  } else if (dl) {
    cls = "active";
    text = `正在下载模型 ${dl.pct}%`;
  } else if (config.allowModelDownload) {
    cls = "allowed";
    text = "已允许下载模型";
  }

  return (
    <>
      <button
        ref={btnRef}
        className={`net-pill ${cls}`}
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label={`联网状态：${text}。文档内容只在本机处理`}
      >
        <span className="net-dot" aria-hidden />
        <span className="net-long">文档仅本机处理</span>
        <span className="net-long" aria-hidden style={{ color: "var(--text-3)" }}>·</span>
        <span>{text}</span>
      </button>
      {open && (
        <div className="net-pop" ref={popRef} role="dialog" aria-label="联网说明">
          <h4>联网说明</h4>
          文档的解析、检测、替换都在本机完成，文档内容不会被发送出去。本应用只在以下两种情况联网：
          <ul>
            <li>
              <b>下载模型</b>（模型市场）：当前{config.allowModelDownload ? "已允许" : "未允许"}
              {dl ? `，正在下载（${dl.pct}%）` : ""}。
            </li>
            <li>
              <b>检查更新</b>（关于页）：只在你点击“检查更新”时发生{checking ? "，正在进行" : ""}。
            </li>
          </ul>
          <div className="btn-row">
            <button
              className="btn sm"
              onClick={() => {
                setOpen(false);
                navigate("models");
              }}
            >
              管理模型下载
            </button>
            <button
              className="btn sm ghost"
              onClick={() => {
                setOpen(false);
                navigate("about");
              }}
            >
              数据流说明
            </button>
          </div>
        </div>
      )}
    </>
  );
}
