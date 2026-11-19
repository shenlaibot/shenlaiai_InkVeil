// 侧栏：当前项目胶囊 + 主流程 / 案件与设置两组导航 + 版本号。收起时只显示图标（带 title 与读屏名称）。
import {
  Briefcase,
  BrainCircuit,
  FolderClosed,
  FolderOpen,
  Info,
  LayoutGrid,
  RotateCcw,
  ScanEye,
  ScrollText,
  Settings2,
} from "lucide-react";
import type { ReactNode } from "react";
import { useApp } from "../../store";
import type { PageId } from "../../store";

interface NavDef {
  id: PageId;
  label: string;
  icon: ReactNode;
}

const GROUPS: { title: string; items: NavDef[] }[] = [
  {
    title: "主流程",
    items: [
      { id: "workbench", label: "工作台", icon: <LayoutGrid size={17} /> },
      { id: "preview", label: "扫描预览", icon: <ScanEye size={17} /> },
      { id: "restore", label: "还原", icon: <RotateCcw size={17} /> },
    ],
  },
  {
    title: "案件与设置",
    items: [
      { id: "vault", label: "项目", icon: <Briefcase size={17} /> },
      { id: "strategy", label: "策略设置", icon: <Settings2 size={17} /> },
      { id: "models", label: "模型市场", icon: <BrainCircuit size={17} /> },
      { id: "audit", label: "审计日志", icon: <ScrollText size={17} /> },
    ],
  },
];

export function SideNav({ version }: { version: string }) {
  const { page, navigate, scanResult, vault } = useApp();
  const hitCount = scanResult ? scanResult.files.reduce((a, f) => a + f.hits.length, 0) : 0;

  const item = (n: NavDef) => (
    <button
      key={n.id}
      className={`nav-item ${page === n.id ? "active" : ""}`}
      onClick={() => navigate(n.id)}
      aria-current={page === n.id ? "page" : undefined}
      title={n.label}
      aria-label={n.id === "preview" && hitCount ? `${n.label}（${hitCount} 处命中）` : n.label}
    >
      {n.icon}
      <span className="nav-label">{n.label}</span>
      {n.id === "preview" && scanResult && <span className="badge">{hitCount}</span>}
    </button>
  );

  return (
    <nav id="sidebar" aria-label="主导航">
      <button
        className={`proj-capsule ${vault ? "open" : ""}`}
        onClick={() => navigate("vault")}
        title={vault ? `当前项目：${vault.name}` : "未打开项目"}
        aria-label={vault ? `当前项目：${vault.name}，${vault.entries} 条映射` : "未打开项目，点击新建或打开"}
      >
        <span className="pc-icon" aria-hidden>
          {vault ? <FolderOpen size={16} /> : <FolderClosed size={16} />}
        </span>
        <span className="pc-text">
          <span className="pc-k">当前项目</span>
          <span className="pc-v" style={{ display: "block" }}>
            {vault ? vault.name : "未打开"}
          </span>
        </span>
      </button>

      {GROUPS.map((g, i) => (
        <div key={g.title} role="group" aria-label={g.title}>
          {i > 0 && <div className="nav-sep" aria-hidden />}
          <div className="nav-group-title">{g.title}</div>
          {g.items.map(item)}
        </div>
      ))}

      <div style={{ marginTop: "auto" }}>
        <div className="nav-sep" aria-hidden />
        {item({ id: "about", label: "关于", icon: <Info size={17} /> })}
        <div className="footer-note" style={{ marginTop: 6, borderTop: "1px solid var(--border)" }}>
          {version && <div className="ver">v{version}</div>}
          文档在本机处理；联网仅用于下载模型和检查更新。
        </div>
      </div>
    </nav>
  );
}
