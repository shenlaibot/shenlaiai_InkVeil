import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

async function boot() {
  // 浏览器里直接打开 vite 页面（没有 Tauri 后端）时，加载内存 mock 以便界面冒烟验证；生产构建不会走到这里
  if (import.meta.env.DEV && !(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) {
    await import("./dev-mock");
  }
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void boot();
