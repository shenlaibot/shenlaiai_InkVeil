import { Component, useEffect, useState } from "react";
import type { ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { CircleHelp, PanelLeft } from "lucide-react";
import "./App.css";
import "./styles/pages.css";
import { AppStoreProvider, useApp } from "./store";
import { nerModelPath } from "./api";
import { WorkbenchPage } from "./pages/Workbench";
import { PreviewPage } from "./pages/Preview";
import { StrategyPage } from "./pages/Strategy";
import { VaultPage } from "./pages/Vault";
import { ModelsPage } from "./pages/Models";
import { RestorePage } from "./pages/Restore";
import { AuditPage } from "./pages/Audit";
import { AboutPage } from "./pages/About";
import { InkVeilMark } from "./components/InkVeilMark";
import { SideNav } from "./components/ui/SideNav";
import { NetStatus } from "./components/ui/NetStatus";
import { ToastHost } from "./components/ui/Toast";
import { ConfirmHost } from "./components/ui/Dialog";
import { Onboarding, OPEN_ONBOARDING_EVENT } from "./components/ui/Onboarding";

/** 窄窗口（≤1000px）自动收起侧栏；用户仍可手动展开/收起 */
function useNavCollapsed(): [boolean, () => void] {
  const mq = "(max-width: 1000px)";
  const [collapsed, setCollapsed] = useState(() => window.matchMedia(mq).matches);
  useEffect(() => {
    const m = window.matchMedia(mq);
    const on = () => setCollapsed(m.matches);
    m.addEventListener("change", on);
    return () => m.removeEventListener("change", on);
  }, []);
  return [collapsed, () => setCollapsed((v) => !v)];
}

interface BoundState {
  error: Error | null;
}

/** 全局渲染错误边界：任何页面渲染崩溃都显示真实报错而不是白屏 */
class ErrorBoundary extends Component<{ children: ReactNode }, BoundState> {
  state: BoundState = { error: null };

  static getDerivedStateFromError(error: Error): BoundState {
    return { error };
  }

  componentDidCatch(error: Error, info: unknown) {
    console.error("[InkVeil] render error:", error, info);
  }

  render() {
    if (this.state.error) {
      return (
        <div style={{ padding: 32, fontFamily: "system-ui, -apple-system, sans-serif" }}>
          <h2 style={{ margin: "0 0 12px" }}>界面渲染出现异常</h2>
          <pre
            style={{
              whiteSpace: "pre-wrap",
              wordBreak: "break-all",
              background: "#f5f2f0",
              border: "1px solid #e2ddd9",
              padding: 12,
              borderRadius: 8,
              fontSize: 13,
              lineHeight: 1.6,
            }}
          >
            {String(this.state.error?.message ?? this.state.error)}
          </pre>
          <button
            className="btn primary"
            style={{ marginTop: 12 }}
            onClick={() => window.location.reload()}
          >
            重新加载界面
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

function Shell() {
  const { page } = useApp();
  const [version, setVersion] = useState("");
  const [collapsed, toggleNav] = useNavCollapsed();

  useEffect(() => {
    getVersion().then(setVersion).catch(() => undefined);
  }, []);

  // 启动时探测 NER 模型是否已安装（模型市场页与策略页共用）
  useEffect(() => {
    nerModelPath()
      .then((p) => {
        if (p) {
          // 由 Models 页管理 modelReady，此处仅触发状态同步
          window.dispatchEvent(new CustomEvent("anonkit:ner-checked", { detail: p }));
        }
      })
      .catch(() => undefined);
  }, []);

  return (
    <div id="app" className={collapsed ? "nav-collapsed" : ""}>
      <div id="titlebar">
        <button
          className="icon-btn tb-collapse"
          onClick={toggleNav}
          aria-label={collapsed ? "展开侧栏" : "收起侧栏"}
          aria-expanded={!collapsed}
          title={collapsed ? "展开侧栏" : "收起侧栏"}
        >
          <PanelLeft size={17} />
        </button>
        <div className="brand">
          <InkVeilMark size={26} />
          <span className="brand-name">
            <span className="zh">墨隐</span> InkVeil
          </span>
          <span className="tag">开源 · 法律本地脱敏</span>
        </div>
        <div className="spacer" />
        <NetStatus />
        <button
          className="icon-btn"
          title="使用须知"
          aria-label="使用须知"
          onClick={() => window.dispatchEvent(new Event(OPEN_ONBOARDING_EVENT))}
        >
          <CircleHelp size={17} />
        </button>
      </div>

      <SideNav version={version} />

      <main id="content">
        <div className="page">
          {page === "workbench" && <WorkbenchPage />}
          {page === "preview" && <PreviewPage />}
          {page === "strategy" && <StrategyPage />}
          {page === "vault" && <VaultPage />}
          {page === "models" && <ModelsPage />}
          {page === "restore" && <RestorePage />}
          {page === "audit" && <AuditPage />}
          {page === "about" && <AboutPage />}
        </div>
      </main>

      <div id="toast" />
      <ToastHost />
      <ConfirmHost />
      <Onboarding />
    </div>
  );
}

export default function App() {
  return (
    <AppStoreProvider>
      <ErrorBoundary>
        <Shell />
      </ErrorBoundary>
    </AppStoreProvider>
  );
}
