// 统一通知：notify() 可带语气、操作按钮（撤销等）与“查看详情”。
// 旧的 store.toast(msg) 仍写入隐藏的 #toast 节点；ToastHost 监听该节点，把内容镜像进同一个通知栈，
// 这样全应用只有一种通知样式，也不用改动 store。
import { useEffect, useRef, useState } from "react";
import { CircleAlert, CircleCheck, Info, TriangleAlert, X } from "lucide-react";

export type ToastTone = "info" | "success" | "warn" | "danger";

export interface ToastOptions {
  tone?: ToastTone;
  /** 次要信息：折叠在“查看详情”后面（如完整报错） */
  detail?: string;
  action?: { label: string; onClick: () => void };
  /** 毫秒；0 = 不自动关闭 */
  duration?: number;
}

interface ToastItem extends ToastOptions {
  id: number;
  message: string;
}

type Listener = (t: ToastItem) => void;
const listeners = new Set<Listener>();
let seq = 0;

export function notify(message: string, opts: ToastOptions = {}) {
  const item: ToastItem = { id: ++seq, message, ...opts };
  listeners.forEach((l) => l(item));
}

/** 按措辞推断语气（用于镜像 store.toast 的纯文本消息） */
export function inferTone(msg: string): ToastTone {
  if (/失败|错误|无法|不能|异常|阻止|篡改/.test(msg)) return "danger";
  if (/请|注意|未生成|未能|没有|暂无|为空/.test(msg)) return "warn";
  if (/已|完成|成功/.test(msg)) return "success";
  return "info";
}

/** 把“xxx失败: 长报错”拆成短消息 + 详情 */
function splitDetail(msg: string): { message: string; detail?: string } {
  const m = msg.match(/^(.{2,24}?(?:失败|错误))[:：]\s*([\s\S]{40,})$/);
  if (m) return { message: m[1], detail: m[2] };
  return { message: msg };
}

const ICON: Record<ToastTone, React.ReactNode> = {
  info: <Info size={16} />,
  success: <CircleCheck size={16} />,
  warn: <TriangleAlert size={16} />,
  danger: <CircleAlert size={16} />,
};

function ToastView({ t, onClose }: { t: ToastItem; onClose: () => void }) {
  const [open, setOpen] = useState(false);
  const [hover, setHover] = useState(false);
  const tone = t.tone ?? "info";
  const duration = t.duration ?? (t.action || t.detail ? 6500 : tone === "danger" ? 5500 : 3200);

  useEffect(() => {
    if (!duration || hover || open) return;
    const timer = window.setTimeout(onClose, duration);
    return () => window.clearTimeout(timer);
  }, [duration, hover, open, onClose]);

  return (
    <div
      className={`toast ${tone}`}
      role={tone === "danger" ? "alert" : "status"}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
    >
      <span className="toast-icon" aria-hidden>
        {ICON[tone]}
      </span>
      <div className="toast-msg">
        {t.message}
        {open && t.detail && <div className="toast-detail">{t.detail}</div>}
      </div>
      {t.detail && (
        <button className="toast-act" onClick={() => setOpen((v) => !v)} aria-expanded={open}>
          {open ? "收起" : "查看详情"}
        </button>
      )}
      {t.action && (
        <button
          className="toast-act"
          onClick={() => {
            t.action?.onClick();
            onClose();
          }}
        >
          {t.action.label}
        </button>
      )}
      <button className="toast-x" onClick={onClose} aria-label="关闭通知">
        <X size={15} />
      </button>
    </div>
  );
}

export function ToastHost() {
  const [items, setItems] = useState<ToastItem[]>([]);
  const lastMirror = useRef<{ msg: string; at: number }>({ msg: "", at: 0 });

  useEffect(() => {
    const l: Listener = (t) => setItems((prev) => [...prev.filter((p) => p.message !== t.message), t].slice(-4));
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);

  // 镜像 store.toast() 写入 #toast 的内容
  useEffect(() => {
    const el = document.getElementById("toast");
    if (!el) return;
    const obs = new MutationObserver(() => {
      const raw = (el.textContent ?? "").trim();
      if (!raw) return;
      const now = Date.now();
      // React StrictMode / 同一消息连发去抖
      if (raw === lastMirror.current.msg && now - lastMirror.current.at < 250) return;
      lastMirror.current = { msg: raw, at: now };
      const { message, detail } = splitDetail(raw);
      notify(message, { tone: inferTone(raw), detail });
    });
    obs.observe(el, { childList: true, characterData: true, subtree: true });
    return () => obs.disconnect();
  }, []);

  const close = (id: number) => setItems((prev) => prev.filter((p) => p.id !== id));

  return (
    <div className="toast-stack" aria-live="polite">
      {items.map((t) => (
        <ToastView key={t.id} t={t} onClose={() => close(t.id)} />
      ))}
    </div>
  );
}
