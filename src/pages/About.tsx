// 关于：版本 / 数据流说明 / 已知限制 / 开源许可 / 检查更新（tauri-plugin-updater，仅在用户点击时联网）。
import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  ArrowRight,
  CloudDownload,
  Code2,
  Cpu,
  FileInput,
  FolderOutput,
  RefreshCw,
  TriangleAlert,
  Scale,
  ShieldAlert,
} from "lucide-react";
import { useApp } from "../store";
import { InkVeilMark } from "../components/InkVeilMark";
import { Callout } from "../components/ui/primitives";
import { notify } from "../components/ui/Toast";
import { announceUpdateCheck } from "../components/ui/NetStatus";
import { OPEN_ONBOARDING_EVENT } from "../components/ui/Onboarding";

const LIMITS = [
  ["识别不保证完整", "规则、词典和本地模型都可能漏掉或误判，导出前请在预览页逐条确认，导出后再人工复核。"],
  ["部分部位不处理", "图片和扫描页中的文字、嵌入对象、透视表等不会被识别和替换；导出后会列出这些部位，请人工核对。"],
  ["不等于匿名化", "占位符、保结构等可还原方式属于假名化；结合上下文，读者仍可能推断出当事人。"],
  ["映射文件等同原文", "映射文件与项目文件可以还原全部原文，请与脱敏稿分开保存、分开传递。"],
  ["版式可能变化", "为替换文字，部分文档的排版、字体可能与原件略有差异。"],
];

export function AboutPage() {
  const { config, navigate } = useApp();
  const [version, setVersion] = useState("");
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [update, setUpdate] = useState<Update | null>(null);
  const [updateMsg, setUpdateMsg] = useState<{ tone: "info" | "success" | "warn"; text: string } | null>(null);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => undefined);
  }, []);

  const onCheck = async () => {
    setChecking(true);
    announceUpdateCheck(true);
    setUpdate(null);
    setUpdateMsg(null);
    try {
      const u = await check();
      if (u) {
        setUpdate(u);
        setUpdateMsg({ tone: "info", text: `发现新版本 v${u.version}。` });
      } else {
        setUpdateMsg({ tone: "success", text: `当前 v${version} 已是最新版本。` });
      }
    } catch (e) {
      setUpdateMsg({ tone: "warn", text: `检查更新失败：${e}。开发版本或无法连接更新服务器时会出现这种情况。` });
    } finally {
      setChecking(false);
      announceUpdateCheck(false);
    }
  };

  const onInstall = async () => {
    if (!update) return;
    setInstalling(true);
    announceUpdateCheck(true);
    try {
      await update.downloadAndInstall();
      notify("更新已安装，应用即将重启", { tone: "success" });
      await relaunch();
    } catch (e) {
      notify("安装更新失败", { tone: "danger", detail: String(e) });
      setInstalling(false);
    } finally {
      announceUpdateCheck(false);
    }
  };

  return (
    <div>
      <div className="about-hero card">
        <InkVeilMark size={56} />
        <div>
          <div className="about-name">
            <span className="zh">墨隐</span> InkVeil
            <span className="ver-badge">v{version || "0.0.0"}</span>
          </div>
          <div className="about-tag">开源 AI 法律本地脱敏工具 · 本地处理 · 开源</div>
        </div>
      </div>

      <div className="card" style={{ marginTop: 16 }}>
        <div className="card-h">数据怎么流动</div>
        <div className="flow">
          <div className="fl-node">
            <b><FileInput size={15} aria-hidden />你选择的文档</b>
            只读取，不改动原文件。
          </div>
          <ArrowRight size={16} className="fl-arrow" aria-hidden />
          <div className="fl-node">
            <b><Cpu size={15} aria-hidden />本机处理</b>
            解析、识别、替换都在这台电脑上完成，文档内容不会被发送出去。
          </div>
          <ArrowRight size={16} className="fl-arrow" aria-hidden />
          <div className="fl-node">
            <b><FolderOutput size={15} aria-hidden />你选择的目录</b>
            脱敏副本、映射文件、项目文件只写到你指定的位置。
          </div>
        </div>
        <div className="card-f" style={{ alignItems: "flex-start" }}>
          <CloudDownload size={16} aria-hidden style={{ color: "var(--text-3)", marginTop: 2, flex: "none" }} />
          <span className="hint" style={{ flex: 1, minWidth: 220 }}>
            联网只在两种情况发生：在模型市场下载模型（当前{config.allowModelDownload ? "已允许" : "未允许"}，可随时关闭），以及在本页点击“检查更新”。
          </span>
          <button className="btn sm" onClick={() => navigate("models")}>模型下载设置</button>
        </div>
      </div>

      <div className="about-grid">
        <div className="card">
          <div className="card-h">
            <TriangleAlert size={16} aria-hidden style={{ color: "var(--warn)" }} />
            已知限制
          </div>
          <ul className="plain-list">
            {LIMITS.map(([t, d]) => (
              <li key={t}>
                <ShieldAlert size={15} aria-hidden />
                <span><b>{t}：</b>{d}</span>
              </li>
            ))}
          </ul>
          <div className="card-f">
            <button className="btn sm ghost" onClick={() => window.dispatchEvent(new Event(OPEN_ONBOARDING_EVENT))}>
              查看使用须知
            </button>
          </div>
        </div>

        <div className="stack">
          <div className="card">
            <div className="card-h">
              <RefreshCw size={16} aria-hidden />
              软件更新
            </div>
            <div className="card-b stack-sm">
              <div className="hint">
                当前版本 v{version || "—"}。只有点击“检查更新”时才会连接更新服务器；更新包经签名校验后才会安装。
              </div>
              {updateMsg && <Callout tone={updateMsg.tone} compact>{updateMsg.text}</Callout>}
              {update && <div className="hint" style={{ whiteSpace: "pre-wrap" }}>更新说明：{update.body ?? update.date ?? "无"}</div>}
              <div className="btn-row">
                <button className="btn" disabled={checking || installing} onClick={() => void onCheck()}>
                  {checking && <span className="spinner" />}
                  检查更新
                </button>
                {update && (
                  <button className="btn primary" disabled={installing} onClick={() => void onInstall()}>
                    {installing && <span className="spinner" />}
                    下载并安装 v{update.version}
                  </button>
                )}
              </div>
            </div>
          </div>

          <div className="card">
            <div className="card-h">
              <Scale size={16} aria-hidden />
              开源许可
            </div>
            <div className="card-b stack-sm">
              <div className="hint" style={{ lineHeight: 1.75 }}>
                本软件以 <b style={{ color: "var(--text)" }}>Apache License 2.0</b> 开源。所用第三方组件及其许可证见源码与安装包附带的
                <code> LICENSE</code>、<code>NOTICE</code>、<code>THIRD-PARTY-NOTICES.md</code>。本地识别模型另有各自的许可证，见模型市场。
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="about-foot">
        <span className="gh"><Code2 size={14} aria-hidden /> 墨隐 InkVeil 开源项目</span>
        <span>本工具不提供法律意见；处理结果请由使用者复核。</span>
      </div>
    </div>
  );
}
