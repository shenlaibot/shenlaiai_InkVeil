// 通用小组件：徽标 / 开关 / 分段控件 / 空状态 / 骨架屏 / 提示条。样式见 src/styles/pages.css。
import { useRef } from "react";
import type { KeyboardEvent, ReactNode } from "react";
import { CircleAlert, CircleCheck, Info, TriangleAlert } from "lucide-react";

export type Tone = "neutral" | "info" | "success" | "warn" | "danger";

export function Badge({
  tone = "neutral",
  icon,
  children,
  title,
  className = "",
  size,
}: {
  tone?: Tone;
  icon?: ReactNode;
  children: ReactNode;
  title?: string;
  className?: string;
  size?: "lg";
}) {
  return (
    <span className={`badge ${tone === "neutral" ? "" : tone} ${size ?? ""} ${className}`} title={title}>
      {icon}
      {children}
    </span>
  );
}

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  /** 读屏名称（必填：开关本身没有可见文字） */
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={`toggle ${checked ? "on" : ""}`}
      onClick={() => onChange(!checked)}
    />
  );
}

export interface SegOption<T extends string> {
  value: T;
  label: ReactNode;
  title?: string;
  disabled?: boolean;
}

/** 单选分段控件（role=radiogroup，方向键切换） */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
  size,
  role = "radiogroup",
}: {
  value: T;
  options: SegOption<T>[];
  onChange: (v: T) => void;
  label: string;
  size?: "lg";
  /** tablist：用于页内标签页 */
  role?: "radiogroup" | "tablist";
}) {
  const ref = useRef<HTMLDivElement>(null);
  const enabled = options.filter((o) => !o.disabled);
  const onKey = (e: KeyboardEvent) => {
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(e.key) || !enabled.length) return;
    e.preventDefault();
    const idx = enabled.findIndex((o) => o.value === value);
    let next = idx;
    if (e.key === "Home") next = 0;
    else if (e.key === "End") next = enabled.length - 1;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = (idx - 1 + enabled.length) % enabled.length;
    else next = (idx + 1) % enabled.length;
    const v = enabled[next].value;
    onChange(v);
    window.setTimeout(() => ref.current?.querySelector<HTMLElement>(`[data-v="${v}"]`)?.focus(), 0);
  };
  const itemRole = role === "tablist" ? "tab" : "radio";
  return (
    <div ref={ref} className={`seg ${size ?? ""}`} role={role} aria-label={label} onKeyDown={onKey}>
      {options.map((o) => {
        const on = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            data-v={o.value}
            role={itemRole}
            {...(role === "tablist" ? { "aria-selected": on } : { "aria-checked": on })}
            tabIndex={on ? 0 : -1}
            disabled={o.disabled}
            title={o.title}
            className="seg-btn"
            onClick={() => onChange(o.value)}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  desc,
  action,
  tone,
  compact,
}: {
  icon: ReactNode;
  title: ReactNode;
  desc?: ReactNode;
  action?: ReactNode;
  tone?: "danger";
  compact?: boolean;
}) {
  return (
    <div className={`empty ${tone ?? ""} ${compact ? "compact" : ""}`}>
      <div className="empty-icon" aria-hidden>
        {icon}
      </div>
      <div className="empty-title">{title}</div>
      {desc && <div className="empty-desc">{desc}</div>}
      {action && <div className="empty-action">{action}</div>}
    </div>
  );
}

export function Skeleton({ w = "100%", h = 14, style }: { w?: number | string; h?: number; style?: React.CSSProperties }) {
  return <span className="skeleton" style={{ width: w, height: h, ...style }} aria-hidden />;
}

/** 加载中的卡片骨架（读屏读“正在加载”） */
export function SkeletonRows({ rows = 3, label = "正在加载" }: { rows?: number; label?: string }) {
  return (
    <div role="status" aria-label={label} style={{ display: "grid", gap: 12, padding: 18 }}>
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} style={{ display: "grid", gap: 8 }}>
          <Skeleton w={`${40 + ((i * 17) % 30)}%`} h={14} />
          <Skeleton w={`${70 + ((i * 11) % 25)}%`} h={11} />
        </div>
      ))}
    </div>
  );
}

const CALLOUT_ICON: Record<string, ReactNode> = {
  info: <Info size={16} />,
  success: <CircleCheck size={16} />,
  warn: <TriangleAlert size={16} />,
  danger: <CircleAlert size={16} />,
};

export function Callout({
  tone = "info",
  title,
  children,
  icon,
  compact,
  className = "",
  role,
}: {
  tone?: "info" | "success" | "warn" | "danger" | "neutral";
  title?: ReactNode;
  children?: ReactNode;
  icon?: ReactNode;
  compact?: boolean;
  className?: string;
  role?: string;
}) {
  return (
    <div className={`callout ${tone === "neutral" ? "" : tone} ${compact ? "compact" : ""} ${className}`} role={role}>
      {icon ?? CALLOUT_ICON[tone] ?? <Info size={16} />}
      <div className="callout-body">
        {title && <div className="callout-title">{title}</div>}
        {children}
      </div>
    </div>
  );
}

/** 页头：标题 + 说明 + 右侧操作 */
export function PageHead({ title, sub, actions }: { title: ReactNode; sub?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="page-head">
      <div className="page-head-text">
        <h1 className="page-title">{title}</h1>
        {sub && <div className="page-sub">{sub}</div>}
      </div>
      {actions && <div className="btn-row">{actions}</div>}
    </div>
  );
}

export function fmtBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(bytes >= 100 * 1024 * 1024 ? 0 : 1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

export function fileName(p: string): string {
  return p.split(/[\\/]/).pop() ?? p;
}
