// 项目（保险库）：按案件持久保存映射表与案件词典，跨批次占位符编号稳定。
import { useState } from "react";
import type { FormEvent } from "react";
import {
  Briefcase,
  Copy,
  Eye,
  EyeOff,
  FolderOpen,
  FolderPlus,
  History,
  Lock,
  BookText,
  ArrowLeft,
} from "lucide-react";
import { useApp } from "../store";
import { pickOutputDir, pickVaultFile, vaultClose, vaultCreate, vaultOpen, vaultSetDict } from "../api";
import { Badge, Callout, EmptyState, PageHead, fileName } from "../components/ui/primitives";
import { confirmDialog } from "../components/ui/Dialog";
import { notify } from "../components/ui/Toast";

function fmtTime(sec: number): string {
  return new Date(sec * 1000).toLocaleString("zh-CN", { hour12: false });
}

/** 口令强度：只做提示，不阻止（最低要求仍是 6 位） */
function strength(p: string): { score: 0 | 1 | 2 | 3 | 4; label: string } {
  if (!p) return { score: 0, label: "" };
  let s = 0;
  if (p.length >= 8) s++;
  if (p.length >= 12) s++;
  const kinds = [/[a-z]/, /[A-Z]/, /\d/, /[^A-Za-z0-9]/].filter((r) => r.test(p)).length + (/[^\x00-\x7f]/.test(p) ? 1 : 0);
  if (kinds >= 2) s++;
  if (kinds >= 3 && p.length >= 10) s++;
  if (p.length < 6) s = 0;
  const score = Math.max(1, Math.min(4, s)) as 1 | 2 | 3 | 4;
  return { score, label: p.length < 6 ? "太短（至少 6 位）" : ["", "弱", "一般", "较强", "强"][score] };
}

function PassInput({
  id,
  value,
  onChange,
  placeholder,
  autoFocus,
  autoComplete,
}: {
  id: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
  autoComplete: string;
}) {
  const [show, setShow] = useState(false);
  return (
    <div className="input-group">
      <input
        id={id}
        type={show ? "text" : "password"}
        className="input"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoFocus={autoFocus}
        autoComplete={autoComplete}
      />
      <button
        type="button"
        className="btn sm ghost icon-only input-addon"
        onClick={() => setShow((v) => !v)}
        aria-label={show ? "隐藏口令" : "显示口令"}
        aria-pressed={show}
      >
        {show ? <EyeOff size={15} /> : <Eye size={15} />}
      </button>
    </div>
  );
}

export function VaultPage() {
  const { vault, setVault, navigate } = useApp();
  const [mode, setMode] = useState<"none" | "create" | "open">("none");
  const [name, setName] = useState("");
  const [pass, setPass] = useState("");
  const [pass2, setPass2] = useState("");
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [dictText, setDictText] = useState<string | null>(null);
  const [savingDict, setSavingDict] = useState(false);

  const reset = () => {
    setMode("none");
    setName("");
    setPass("");
    setPass2("");
    setOpenPath(null);
    setErr(null);
  };

  const st = strength(pass);

  const onCreate = async (e: FormEvent) => {
    e.preventDefault();
    setErr(null);
    if (!name.trim()) return setErr("请填写项目名称");
    if (pass.length < 6) return setErr("口令至少 6 位");
    if (pass !== pass2) return setErr("两次输入的口令不一致");
    const dir = await pickOutputDir();
    if (!dir) return;
    setBusy(true);
    try {
      const info = await vaultCreate(dir, name.trim(), pass);
      setVault(info);
      notify(`项目已创建：${fileName(info.path)}`, { tone: "success" });
      reset();
    } catch (e) {
      setErr(`创建失败：${e}`);
    } finally {
      setBusy(false);
    }
  };

  const onOpen = async (e: FormEvent) => {
    e.preventDefault();
    setErr(null);
    if (!openPath) return setErr("请先选择项目文件");
    if (!pass) return setErr("请输入项目口令");
    setBusy(true);
    try {
      const info = await vaultOpen(openPath, pass);
      setVault(info);
      notify(`已打开项目：${info.name}`, { tone: "success" });
      reset();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onClose = async () => {
    if (dictText !== null) {
      const ok = await confirmDialog({
        title: "案件词典有未保存的修改",
        message: "关闭项目会丢弃这些修改。",
        confirmText: "丢弃并关闭",
        tone: "warn",
      });
      if (!ok) return;
    } else {
      const ok = await confirmDialog({
        title: `关闭项目「${vault?.name}」？`,
        message: "口令会从内存中清除。之后导出的文件不会再存入这个项目，要继续使用需重新输入口令打开。",
        confirmText: "关闭项目",
      });
      if (!ok) return;
    }
    try {
      await vaultClose();
      setVault(null);
      setDictText(null);
      notify("项目已关闭，口令已从内存清除", { tone: "success" });
    } catch (e) {
      notify("关闭失败", { tone: "danger", detail: String(e) });
    }
  };

  const onSaveDict = async () => {
    if (dictText === null) return;
    setSavingDict(true);
    try {
      const info = await vaultSetDict(dictText.split(/\r?\n/));
      setVault(info);
      setDictText(null);
      notify(`案件词典已保存（${info.dict_terms.length} 条）`, { tone: "success" });
    } catch (e) {
      notify("保存词典失败", { tone: "danger", detail: String(e) });
    } finally {
      setSavingDict(false);
    }
  };

  const copyPath = (p: string) => {
    navigator.clipboard
      .writeText(p)
      .then(() => notify("已复制路径", { tone: "success" }))
      .catch(() => notify("无法写入剪贴板", { tone: "danger" }));
  };

  // ---------- 未打开项目 ----------
  if (!vault) {
    return (
      <div>
        <PageHead
          title="项目"
          sub="按案件保存映射表与案件词典：同一个人、同一家公司在所有批次里使用同一个占位符，之后任何一批脱敏稿都能用这个项目还原。"
        />

        {mode === "none" && (
          <div className="stack">
            <Callout tone="info" title="当前没有打开的项目">
              不打开项目也能导出，但每次都会新建映射表，占位符编号从 1 重新开始：不同批次里同一个人的占位符可能不同，用错映射表会还原成别人的原文。
              处理同一案件的多批文件前，建议先新建或打开项目。
            </Callout>
            <div className="vault-choices">
              <button className="choice" onClick={() => setMode("create")}>
                <span className="ch-icon" aria-hidden>
                  <FolderPlus size={20} />
                </span>
                <span>
                  <span className="ch-title" style={{ display: "block" }}>新建项目</span>
                  <span className="ch-desc" style={{ display: "block" }}>为一个新案件建立加密的项目文件（.akcase），设置项目口令。</span>
                </span>
              </button>
              <button className="choice" onClick={() => setMode("open")}>
                <span className="ch-icon" aria-hidden>
                  <FolderOpen size={20} />
                </span>
                <span>
                  <span className="ch-title" style={{ display: "block" }}>打开已有项目</span>
                  <span className="ch-desc" style={{ display: "block" }}>选择之前保存的 .akcase 文件，输入口令后继续使用同一套占位符。</span>
                </span>
              </button>
            </div>
          </div>
        )}

        {mode === "create" && (
          <form className="card form-card" onSubmit={(e) => void onCreate(e)} noValidate>
            <div className="card-h">
              <button type="button" className="btn sm ghost icon-only" onClick={reset} aria-label="返回">
                <ArrowLeft size={16} />
              </button>
              新建项目
            </div>
            <div className="card-b">
              <div className="field">
                <label className="field-label" htmlFor="v-name">项目名称</label>
                <input id="v-name" className="input" autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="如：甲公司并购案" />
                <span className="field-hint">会作为项目文件名；不要使用当事人真实姓名。</span>
              </div>
              <div className="field">
                <label className="field-label" htmlFor="v-pass">项目口令</label>
                <PassInput id="v-pass" value={pass} onChange={setPass} placeholder="至少 6 位" autoComplete="new-password" />
                {pass && (
                  <div className={`strength s${st.score}`} aria-live="polite">
                    <span className="bars" aria-hidden><i /><i /><i /><i /></span>
                    口令强度：{st.label}
                  </div>
                )}
              </div>
              <div className="field">
                <label className="field-label" htmlFor="v-pass2">再次输入口令</label>
                <PassInput id="v-pass2" value={pass2} onChange={setPass2} autoComplete="new-password" />
                {pass2 && pass !== pass2 && <span className="field-hint" style={{ color: "var(--danger-text)" }}>两次输入不一致</span>}
              </div>
              <Callout tone="warn" compact>
                口令无法找回：忘记口令将无法打开项目，也就无法还原已导出的脱敏稿。项目文件可还原出原文，请与脱敏稿分开保存。
              </Callout>
              {err && <Callout tone="danger" compact role="alert">{err}</Callout>}
            </div>
            <div className="card-f">
              <button type="submit" className="btn primary" disabled={busy}>
                {busy && <span className="spinner" />}选择保存位置并创建
              </button>
              <button type="button" className="btn ghost" onClick={reset}>取消</button>
            </div>
          </form>
        )}

        {mode === "open" && (
          <form className="card form-card" onSubmit={(e) => void onOpen(e)} noValidate>
            <div className="card-h">
              <button type="button" className="btn sm ghost icon-only" onClick={reset} aria-label="返回">
                <ArrowLeft size={16} />
              </button>
              打开已有项目
            </div>
            <div className="card-b">
              <div className="field">
                <span className="field-label">项目文件</span>
                <div className="file-pick">
                  <span className={`fp-name ${openPath ? "" : "empty-v"}`}>{openPath ?? "尚未选择 .akcase 文件"}</span>
                  <button
                    type="button"
                    className="btn sm"
                    onClick={() =>
                      void pickVaultFile()
                        .then((p) => p && setOpenPath(p))
                        .catch((e) => notify("无法打开文件选择", { tone: "danger", detail: String(e) }))
                    }
                  >
                    {openPath ? "更换" : "选择文件"}
                  </button>
                </div>
              </div>
              <div className="field">
                <label className="field-label" htmlFor="v-open-pass">项目口令</label>
                <PassInput id="v-open-pass" value={pass} onChange={setPass} autoComplete="current-password" />
              </div>
              {err && <Callout tone="danger" compact role="alert">{err}</Callout>}
            </div>
            <div className="card-f">
              <button type="submit" className="btn primary" disabled={busy}>
                {busy && <span className="spinner" />}打开项目
              </button>
              <button type="button" className="btn ghost" onClick={reset}>取消</button>
            </div>
          </form>
        )}
      </div>
    );
  }

  // ---------- 已打开 ----------
  const dictValue = dictText ?? vault.dict_terms.join("\n");
  const dictCount = dictValue.split(/\r?\n/).map((t) => t.trim()).filter(Boolean).length;

  return (
    <div>
      <PageHead
        title="项目"
        sub="导出时映射表自动保存进这个项目，不再向输出目录写映射文件。"
      />

      <div className="card">
        <div className="vault-head">
          <div className="vh-icon" aria-hidden>
            <Briefcase size={22} />
          </div>
          <div className="vh-text">
            <div className="vh-name">
              {vault.name}
              <Badge tone="success" icon={<Lock size={11} />}>已打开 · 口令仅在内存</Badge>
            </div>
            <div className="vh-meta">
              <span style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                <span className="path">{vault.path}</span>
                <button className="btn sm ghost icon-only" onClick={() => copyPath(vault.path)} aria-label="复制项目文件路径" title="复制路径">
                  <Copy size={13} />
                </button>
              </span>
              <span>创建于 {fmtTime(vault.created)}</span>
            </div>
          </div>
          <div className="btn-row">
            <button className="btn" onClick={() => navigate("restore")}>用此项目还原</button>
            <button className="btn" onClick={() => void onClose()}>关闭项目</button>
          </div>
        </div>
        <div style={{ padding: "0 18px 18px" }}>
          <div className="tiles">
            <div className="tile"><div className="tile-num">{vault.entries}</div><div className="tile-lbl">映射条目</div></div>
            <div className="tile"><div className="tile-num">{vault.batches.length}</div><div className="tile-lbl">已导出批次</div></div>
            <div className="tile"><div className="tile-num">{vault.dict_terms.length}</div><div className="tile-lbl">案件词典</div></div>
          </div>
        </div>
        <div className="card-f">
          <span className="hint">项目文件可以还原出原文，请与脱敏稿分开保存，不要一起发送。</span>
        </div>
      </div>

      <div className="vault-grid">
        <div className="card">
          <div className="card-h">
            <BookText size={16} aria-hidden />
            案件词典
            {dictText !== null && <Badge tone="warn">未保存</Badge>}
          </div>
          <div className="card-b">
            <p className="hint" style={{ marginBottom: 10 }}>
              每行一个：项目代号、客户简称、内部称呼等。扫描时与“策略设置”里的全局词典合并。项目里登记过的人名、公司名，在新文件里即使没有“联系人：”之类的前缀也会被识别。
            </p>
            <textarea
              className="textarea mono"
              style={{ minHeight: 180 }}
              value={dictValue}
              onChange={(e) => setDictText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void onSaveDict();
              }}
              placeholder={"例如：\n星辰计划\n老王"}
              aria-label="案件词典，每行一个词条"
            />
            <div className="dict-meta">
              <span>{dictCount} 条</span>
              <span style={{ flex: 1 }} />
              {dictText !== null && (
                <button className="btn sm ghost" onClick={() => setDictText(null)}>放弃修改</button>
              )}
              <button className="btn sm primary" disabled={dictText === null || savingDict} onClick={() => void onSaveDict()}>
                {savingDict && <span className="spinner" />}保存词典
              </button>
            </div>
          </div>
        </div>

        <div className="card">
          <div className="card-h">
            <History size={16} aria-hidden />
            批次记录
            <span className="card-h-sub">只记录时间与数量，不含原文</span>
          </div>
          {vault.batches.length === 0 ? (
            <EmptyState
              compact
              icon={<History size={20} />}
              title="还没有导出过"
              desc="在工作台扫描并导出后，每一批会出现在这里。"
              action={<button className="btn sm" onClick={() => navigate("workbench")}>去工作台</button>}
            />
          ) : (
            <div className="card-b">
              <ol className="timeline">
                {vault.batches.map((b) => (
                  <li key={b.seq}>
                    <div className="tl-t">第 {b.seq} 批</div>
                    <div className="tl-m">
                      <span>{fmtTime(b.time)}</span>
                      <Badge>{b.files} 个文件</Badge>
                      <Badge tone="info">新增 {b.entries_added} 条映射</Badge>
                    </div>
                  </li>
                ))}
              </ol>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
