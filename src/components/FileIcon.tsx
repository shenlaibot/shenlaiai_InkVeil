// 文件格式图标（lucide 开源图标库）；样式沿用 App.css 的 .file-icon / .fi-*
import { FileCode2, FileQuestion, FileSpreadsheet, FileText } from "lucide-react";

export function fmtIcon(format: string, size = 17) {
  const Icon =
    format === "xlsx" ? FileSpreadsheet : format === "md" ? FileCode2 : format === "未知" ? FileQuestion : FileText;
  const cls = format === "未知" ? "unknown" : format;
  return (
    <span className={`file-icon fi-${cls}`} aria-hidden="true">
      <Icon size={size} />
    </span>
  );
}
