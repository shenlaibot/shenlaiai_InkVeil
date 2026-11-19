// 统一弹窗：焦点进入/循环/归还，Esc 关闭，点击遮罩关闭（可关），aria-modal。
// confirmDialog() 返回 Promise<boolean>，替代 window.confirm。
import { useEffect, useId, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { TriangleAlert, Info, X } from "lucide-react";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  icon?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  /** 不允许 Esc / 遮罩关闭（如必须确认的首启说明） */
  dismissible?: boolean;
  size?: "sm" | "md";
  className?: string;
  /** 打开后优先聚焦的元素选择器 */
  initialFocus?: string;
}

export function Dialog({
  open,
  onClose,
  title,
  icon,
  children,
  footer,
  dismissible = true,
  size = "md",
  className = "",
  initialFocus,
}: DialogProps) {
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    const node = ref.current;
    const t = window.setTimeout(() => {
      const target =
        (initialFocus && node?.querySelector<HTMLElement>(initialFocus)) ||
        node?.querySelector<HTMLElement>("[data-autofocus]") ||
        node?.querySelector<HTMLElement>(FOCUSABLE);
      target?.focus();
    }, 20);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && dismissible) {
        e.stopPropagation();
        closeRef.current();
      } else if (e.key === "Tab" && node) {
        const els = Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => el.offsetParent !== null);
        if (!els.length) return;
        const first = els[0];
        const last = els[els.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      window.clearTimeout(t);
      document.removeEventListener("keydown", onKey, true);
      prev?.focus?.();
    };
  }, [open, dismissible, initialFocus]);

  if (!open) return null;
  return createPortal(
    <div
      className="modal-mask"
      onMouseDown={(e) => {
        if (dismissible && e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={ref}
        className={`modal ${size === "sm" ? "sm" : ""} ${className}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <div className="modal-h">
          {icon}
          <span id={titleId}>{title}</span>
          {dismissible && (
            <button className="btn sm ghost icon-only modal-x" onClick={onClose} aria-label="关闭">
              <X size={16} />
            </button>
          )}
        </div>
        <div className="modal-b">{children}</div>
        {footer && <div className="modal-f">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}

// ---------- confirmDialog ----------
export interface ConfirmOptions {
  title: string;
  message: ReactNode;
  confirmText?: string;
  cancelText?: string;
  tone?: "danger" | "warn" | "info";
}

type Req = ConfirmOptions & { resolve: (ok: boolean) => void };
let push: ((r: Req) => void) | null = null;

export function confirmDialog(opts: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    if (!push) {
      // Host 未挂载时退回原生确认框
      resolve(window.confirm(typeof opts.message === "string" ? opts.message : opts.title));
      return;
    }
    push({ ...opts, resolve });
  });
}

export function ConfirmHost() {
  const [req, setReq] = useState<Req | null>(null);
  useEffect(() => {
    push = (r) => setReq(r);
    return () => {
      push = null;
    };
  }, []);
  const done = (ok: boolean) => {
    req?.resolve(ok);
    setReq(null);
  };
  const tone = req?.tone ?? "info";
  return (
    <Dialog
      open={!!req}
      onClose={() => done(false)}
      size="sm"
      title={req?.title ?? ""}
      icon={
        <span className={`modal-icon ${tone}`} aria-hidden>
          {tone === "info" ? <Info size={18} /> : <TriangleAlert size={18} />}
        </span>
      }
      initialFocus="[data-cancel]"
      footer={
        <>
          <button className="btn" data-cancel onClick={() => done(false)}>
            {req?.cancelText ?? "取消"}
          </button>
          <button className={`btn ${tone === "danger" ? "danger solid" : "primary"}`} onClick={() => done(true)}>
            {req?.confirmText ?? "确定"}
          </button>
        </>
      }
    >
      {req?.message}
    </Dialog>
  );
}
