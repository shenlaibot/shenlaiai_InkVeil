// 墨隐 InkVeil 品牌标识（与 docs/brand/inkveil-mark.svg 同源）。
// 宣纸底 · 一笔墨横（遮去文书中的一行）· 朱砂小印。
// size < 40 时省略文书行与飞白，保证小尺寸下依然清晰。
import { useId } from "react";

export function InkVeilMark({ size = 26, className }: { size?: number; className?: string }) {
  const uid = useId();
  const paperId = `iv-paper-${uid}`;
  const inkId = `iv-ink-${uid}`;
  const detailed = size >= 40;
  const stroke =
    "M214 462C238 436 300 446 372 464C500 490 640 484 746 448C792 432 830 426 838 452C848 488 826 556 786 588C720 604 560 586 428 598C340 606 266 612 222 590C190 574 190 492 214 462Z";
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 1024 1024"
      className={className}
      role="img"
      aria-label="墨隐 InkVeil"
      style={{ flexShrink: 0 }}
    >
      <defs>
        <linearGradient id={paperId} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#fbf7ef" />
          <stop offset="1" stopColor="#ece5d6" />
        </linearGradient>
        <linearGradient id={inkId} x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#1a1d2b" />
          <stop offset="1" stopColor="#0b0d14" />
        </linearGradient>
      </defs>
      <rect width="1024" height="1024" rx="228" fill={`url(#${paperId})`} />
      <rect x="1.5" y="1.5" width="1021" height="1021" rx="227" fill="none" stroke="#000" strokeOpacity="0.08" strokeWidth="3" />
      {detailed && (
        <g fill="#bfb6a8">
          <rect x="232" y="262" width="560" height="48" rx="24" />
          <rect x="232" y="366" width="400" height="48" rx="24" />
          <rect x="232" y="664" width="380" height="48" rx="24" />
          <rect x="232" y="768" width="300" height="48" rx="24" />
        </g>
      )}
      <path d={stroke} fill={`url(#${inkId})`} />
      {detailed && (
        <path
          d="M596 534C676 522 742 514 802 498"
          fill="none"
          stroke="#fbf7ef"
          strokeOpacity="0.55"
          strokeWidth="7"
          strokeLinecap="round"
        />
      )}
      <rect x="676" y="676" width="116" height="116" rx="16" fill="#c4392c" />
      {detailed && <rect x="700" y="700" width="68" height="68" rx="6" fill="none" stroke="#fbf7ef" strokeWidth="8" />}
    </svg>
  );
}
