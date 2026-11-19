// 首次启动说明：确认状态带版本号写入本地；说明内容有实质变化时把 ONBOARDING_VERSION +1 即可重新提示一次。
import { useEffect, useState } from "react";
import { CloudDownload, FileExclamationPoint, KeyRound, ScanSearch, ShieldAlert, UserCheck } from "lucide-react";
import { Dialog } from "./Dialog";

const KEY = "anonkit.onboarding.ack";
export const ONBOARDING_VERSION = 1;

function acked(): boolean {
  try {
    const raw = localStorage.getItem(KEY);
    return !!raw && Number(JSON.parse(raw)?.v) >= ONBOARDING_VERSION;
  } catch {
    return false;
  }
}

/** 标题栏“使用说明”按钮可调用，重新打开说明 */
export const OPEN_ONBOARDING_EVENT = "anonkit:open-onboarding";

export function Onboarding() {
  const [open, setOpen] = useState(false);
  const [first, setFirst] = useState(false);

  useEffect(() => {
    const force = new URLSearchParams(location.search).has("onboarding");
    if (force || !acked()) {
      setFirst(true);
      setOpen(true);
    }
    const on = () => {
      setFirst(false);
      setOpen(true);
    };
    window.addEventListener(OPEN_ONBOARDING_EVENT, on);
    return () => window.removeEventListener(OPEN_ONBOARDING_EVENT, on);
  }, []);

  const ack = () => {
    try {
      localStorage.setItem(KEY, JSON.stringify({ v: ONBOARDING_VERSION, at: new Date().toISOString() }));
    } catch {
      /* 写不进本地存储时，下次启动会再提示一次 */
    }
    setOpen(false);
  };

  return (
    <Dialog
      open={open}
      onClose={() => setOpen(false)}
      dismissible={!first}
      className="onb"
      title={first ? "开始之前，请了解这几点" : "使用须知"}
      icon={
        <span className="modal-icon" aria-hidden>
          <ShieldAlert size={18} />
        </span>
      }
      initialFocus="[data-ack]"
      footer={
        <>
          <span className="onb-foot grow">流程：导入 → 扫描 → 逐条确认 → 导出脱敏副本 → 需要时还原</span>
          <button className="btn primary" data-ack onClick={ack}>
            我已阅读并理解
          </button>
        </>
      }
    >
      <ul className="onb-list">
        <li>
          <UserCheck size={16} />
          本工具用于辅助识别和替换敏感信息，最终判断由你作出。
        </li>
        <li className="warn">
          <ScanSearch size={16} />
          自动识别不保证找全，图片、扫描页中的文字等部位可能没有处理。
        </li>
        <li className="warn">
          <FileExclamationPoint size={16} />
          导出的脱敏稿不等于匿名化：上下文、可还原的替换方式仍可能让人识别出当事人。
        </li>
        <li className="warn">
          <KeyRound size={16} />
          映射文件、项目文件可以还原出原文，等同原文，请与脱敏稿分开保存，不要一起发送。
        </li>
        <li>
          <ShieldAlert size={16} />
          发给 AI 或外部协作方之前，请人工复核导出的文件。
        </li>
        <li>
          <CloudDownload size={16} />
          文档在本机处理；联网仅用于下载模型和检查更新，可在模型市场关闭模型下载。
        </li>
      </ul>
    </Dialog>
  );
}
