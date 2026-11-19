// 还原向导：① 选映射来源（当前项目 / 映射文件）→ ② 粘贴文本或选择 docx/xlsx → ③ 结果。
// 精确 / 模糊 / 疑似未还原用颜色 + 文字同时区分；模糊命中可逐条核对（确认或改回 AI 原写法）。
import { useMemo, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Briefcase,
  Check,
  ClipboardPaste,
  Copy,
  Download,
  FileText,
  FolderOpen,
  KeyRound,
  RotateCcw,
  Undo2,
} from "lucide-react";
import { useApp } from "../store";
import {
  openInFolder,
  pickMappingFile,
  pickOutputDir,
  pickRestoreDoc,
  restoreDocument,
  restoreTextFuzzy,
  saveTextFile,
} from "../api";
import type { DocRestoreResult, FuzzyRestoreResult } from "../types";
import { Badge, Callout, PageHead, Segmented, fileName } from "../components/ui/primitives";
import { notify } from "../components/ui/Toast";

type Decision = "pending" | "ok" | "reverted";
type Step = 1 | 2 | 3;

interface Seg {
  text: string;
  hit?: number;
  sus?: boolean;
}

/** 按字符（码点）偏移切出普通 / 模糊命中片段；被“改回”的命中显示 AI 原写法；再在普通片段里标出疑似未还原 */
function segmentOutput(res: FuzzyRestoreResult, dec: Decision[]): Seg[] {
  const chars = Array.from(res.output_text);
  const segs: Seg[] = [];
  let cur = 0;
  res.fuzzy_hits.forEach((h, i) => {
    if (h.out_start > cur) segs.push({ text: chars.slice(cur, h.out_start).join("") });
    segs.push({ text: dec[i] === "reverted" ? h.raw : chars.slice(h.out_start, h.out_end).join(""), hit: i });
    cur = h.out_end;
  });
  if (cur < chars.length) segs.push({ text: chars.slice(cur).join("") });
  const sus = [...new Set(res.suspects.map((s) => s.text))].filter(Boolean);
  if (!sus.length) return segs;
  const re = new RegExp(sus.map((s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|"), "g");
  const out: Seg[] = [];
  for (const s of segs) {
    if (s.hit !== undefined) {
      out.push(s);
      continue;
    }
    let last = 0;
    for (const m of s.text.matchAll(re)) {
      const at = m.index ?? 0;
      if (at > last) out.push({ text: s.text.slice(last, at) });
      out.push({ text: m[0], sus: true });
      last = at + m[0].length;
    }
    if (last < s.text.length) out.push({ text: s.text.slice(last) });
  }
  return out;
}

function composed(res: FuzzyRestoreResult, dec: Decision[]): string {
  return segmentOutput(res, dec)
    .map((s) => s.text)
    .join("");
}

export function RestorePage() {
  const { vault, navigate } = useApp();
  const [step, setStep] = useState<Step>(1);
  const [source, setSource] = useState<"vault" | "file">(vault ? "vault" : "file");
  const [mappingPath, setMappingPath] = useState<string | null>(null);
  const [passphrase, setPassphrase] = useState("");
  const [mode, setMode] = useState<"text" | "doc">("text");
  const [input, setInput] = useState("");
  const [docPath, setDocPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<FuzzyRestoreResult | null>(null);
  const [docResult, setDocResult] = useState<DocRestoreResult | null>(null);
  const [dec, setDec] = useState<Decision[]>([]);

  const useVault = source === "vault" && !!vault;
  const sourceOk = useVault || !!mappingPath;
  const inputOk = mode === "text" ? !!input.trim() : !!docPath;

  const mapping = () => (useVault ? { path: "", pass: null } : { path: mappingPath as string, pass: passphrase || null });

  const pasteClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText();
      if (text) setInput(text);
      else notify("剪贴板是空的", { tone: "warn" });
    } catch {
      notify("无法读取剪贴板，请直接在输入框里粘贴（⌘/Ctrl + V）", { tone: "warn" });
    }
  };

  const run = async () => {
    setErr(null);
    setBusy(true);
    const m = mapping();
    try {
      if (mode === "text") {
        const res = await restoreTextFuzzy(input, m.path, m.pass);
        setResult(res);
        setDocResult(null);
        setDec(res.fuzzy_hits.map(() => "pending"));
      } else {
        const dir = await pickOutputDir();
        if (!dir) {
          setBusy(false);
          return;
        }
        const res = await restoreDocument(docPath as string, dir, m.path, m.pass);
        setDocResult(res);
        setResult(null);
        setDec(res.fuzzy_hits.map(() => "pending"));
      }
      setStep(3);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const restart = () => {
    setStep(2);
    setResult(null);
    setDocResult(null);
    setDec([]);
    setErr(null);
  };

  const setDecision = (i: number, d: Decision) => {
    const prev = dec[i];
    setDec((x) => x.map((v, k) => (k === i ? d : v)));
    if (d === "reverted")
      notify(`#${i + 1} 已改回 AI 原写法`, { action: { label: "撤销", onClick: () => setDec((x) => x.map((v, k) => (k === i ? prev : v))) } });
  };

  const finalText = useMemo(() => (result ? composed(result, dec) : ""), [result, dec]);
  const pending = dec.filter((d) => d === "pending").length;

  const copyResult = () => {
    navigator.clipboard
      .writeText(finalText)
      .then(() => notify(pending ? `已复制（还有 ${pending} 处模糊还原未核对）` : "已复制还原结果", { tone: pending ? "warn" : "success" }))
      .catch(() => notify("无法写入剪贴板", { tone: "danger" }));
  };

  const exportResult = async () => {
    try {
      const path = await saveTextFile("还原结果.txt", finalText);
      if (path) notify(`已导出：${fileName(path)}${pending ? `（还有 ${pending} 处未核对）` : ""}`, { tone: pending ? "warn" : "success" });
    } catch (e) {
      notify("导出失败", { tone: "danger", detail: String(e) });
    }
  };

  const flash = (i: number) => {
    const el = document.getElementById(`fz-${i}`);
    if (!el) return;
    el.scrollIntoView({ block: "center", behavior: "smooth" });
    el.classList.add("flash");
    window.setTimeout(() => el.classList.remove("flash"), 1200);
  };

  const STEPS: { n: Step; t: string; d: string }[] = [
    { n: 1, t: "映射来源", d: sourceOk ? (useVault ? `项目「${vault?.name}」` : fileName(mappingPath ?? "")) : "项目或映射文件" },
    { n: 2, t: "待还原内容", d: inputOk ? (mode === "text" ? `${Array.from(input).length} 字` : fileName(docPath ?? "")) : "粘贴文本或选文件" },
    { n: 3, t: "结果与核对", d: result || docResult ? (pending ? `${pending} 处待核对` : "已核对") : "—" },
  ];
  const canGo = (n: Step) => n === 1 || (n === 2 && sourceOk) || (n === 3 && !!(result || docResult));

  const exact = result?.exact_count ?? docResult?.exact_count ?? 0;
  const fuzzy = result?.fuzzy_count ?? docResult?.fuzzy_count ?? 0;
  const suspects = result?.suspects.length ?? docResult?.suspects.length ?? 0;

  return (
    <div>
      <PageHead
        title="还原"
        sub="AI 或协作方返回带占位符的内容后，用同一个项目或映射文件把占位符换回原文。全程在本机完成。"
      />

      <ol className="stepper" aria-label="还原步骤">
        {STEPS.map((s, i) => (
          <li key={s.n} className={step === s.n ? "current" : step > s.n ? "done" : ""}>
            <button className="st-btn" disabled={!canGo(s.n)} onClick={() => setStep(s.n)} aria-current={step === s.n ? "step" : undefined}>
              <span className="st-n">{step > s.n ? <Check size={14} /> : s.n}</span>
              <span>
                <span className="st-t" style={{ display: "block" }}>{s.t}</span>
                <span className="st-d" style={{ display: "block" }}>{s.d}</span>
              </span>
            </button>
            {i < STEPS.length - 1 && <span className="st-line" aria-hidden />}
          </li>
        ))}
      </ol>

      {step === 1 && (
        <div className="card wizard-card">
          <div className="card-h">第 1 步：用哪份映射还原？</div>
          <div className="card-b">
            <div className="src-choices" role="radiogroup" aria-label="映射来源">
              <button
                className={`choice ${useVault ? "selected" : ""}`}
                role="radio"
                aria-checked={useVault}
                disabled={!vault}
                onClick={() => setSource("vault")}
              >
                <span className="ch-icon" aria-hidden><Briefcase size={18} /></span>
                <span>
                  <span className="ch-title" style={{ display: "block" }}>当前项目</span>
                  <span className="ch-desc" style={{ display: "block" }}>
                    {vault ? `「${vault.name}」· ${vault.entries} 条映射，覆盖该项目所有批次` : "没有打开的项目"}
                  </span>
                </span>
              </button>
              <button
                className={`choice ${!useVault ? "selected" : ""}`}
                role="radio"
                aria-checked={!useVault}
                onClick={() => setSource("file")}
              >
                <span className="ch-icon" aria-hidden><KeyRound size={18} /></span>
                <span>
                  <span className="ch-title" style={{ display: "block" }}>映射文件</span>
                  <span className="ch-desc" style={{ display: "block" }}>导出时生成的 .anonymap / .anonymap.enc 文件</span>
                </span>
              </button>
            </div>
            {!vault && (
              <div className="hint">
                处理同一案件的多批文件时，用项目还原更可靠。{" "}
                <button className="link-btn" onClick={() => navigate("vault")}>打开或新建项目</button>
              </div>
            )}
            {!useVault && (
              <div className="stack-sm">
                <div className="file-pick">
                  <span className={`fp-name ${mappingPath ? "" : "empty-v"}`}>{mappingPath ?? "尚未选择映射文件"}</span>
                  <button
                    className="btn sm"
                    onClick={() =>
                      void pickMappingFile()
                        .then((p) => p && setMappingPath(p))
                        .catch((e) => notify("无法打开文件选择", { tone: "danger", detail: String(e) }))
                    }
                  >
                    <FolderOpen size={13} />
                    {mappingPath ? "更换" : "选择文件"}
                  </button>
                </div>
                <div className="field" style={{ maxWidth: 360 }}>
                  <label className="field-label" htmlFor="rs-pass">映射文件口令</label>
                  <input
                    id="rs-pass"
                    type="password"
                    className="input"
                    value={passphrase}
                    onChange={(e) => setPassphrase(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && sourceOk) setStep(2);
                    }}
                    placeholder="加密映射文件需要；未加密可留空"
                    autoComplete="off"
                  />
                </div>
              </div>
            )}
          </div>
          <div className="card-f wizard-nav">
            <span className="grow" />
            <button className="btn primary" disabled={!sourceOk} onClick={() => setStep(2)}>
              下一步<ArrowRight size={14} />
            </button>
          </div>
        </div>
      )}

      {step === 2 && (
        <div className="card wizard-card">
          <div className="card-h">
            第 2 步：要还原什么？
            <span className="grow" />
            <Segmented<"text" | "doc">
              label="还原内容类型"
              value={mode}
              onChange={(v) => {
                setMode(v);
                setErr(null);
              }}
              options={[
                { value: "text", label: <><FileText size={13} />粘贴文本</> },
                { value: "doc", label: <><FolderOpen size={13} />Word / Excel 文件</> },
              ]}
            />
          </div>
          <div className="card-b">
            {mode === "text" ? (
              <div className="stack-sm">
                <textarea
                  className="textarea"
                  style={{ minHeight: 240 }}
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && inputOk && !busy) void run();
                  }}
                  placeholder="粘贴 AI 的审阅意见、协作方的批注等含占位符的文本，如：[姓名1] 应于 [日期2] 前付款…"
                  aria-label="待还原文本"
                  autoFocus
                />
                <div className="btn-row">
                  <button className="btn sm" onClick={() => void pasteClipboard()}>
                    <ClipboardPaste size={13} />从剪贴板粘贴
                  </button>
                  {input && (
                    <button className="btn sm ghost" onClick={() => setInput("")}>清空</button>
                  )}
                  <span className="hint">⌘/Ctrl + Enter 开始还原</span>
                </div>
              </div>
            ) : (
              <div className="stack-sm">
                <div className="file-pick">
                  <span className={`fp-name ${docPath ? "" : "empty-v"}`}>{docPath ?? "尚未选择 .docx / .xlsx 文件"}</span>
                  <button
                    className="btn sm"
                    onClick={() =>
                      void pickRestoreDoc()
                        .then((p) => {
                          if (p) {
                            setDocPath(p);
                            setDocResult(null);
                          }
                        })
                        .catch((e) => notify("无法打开文件选择", { tone: "danger", detail: String(e) }))
                    }
                  >
                    {docPath ? "更换" : "选择文件"}
                  </button>
                </div>
                <div className="hint">
                  占位符被 Word 拆成多段文字也能识别，格式保持不变。原文件不会被修改，结果另存为「原名_已还原」，下一步会让你选择保存位置。
                </div>
              </div>
            )}
            {err && (
              <Callout tone="danger" title="还原失败" role="alert">
                {err}
                <div style={{ marginTop: 4 }}>请确认映射来源与口令是否对应这批脱敏稿。</div>
              </Callout>
            )}
          </div>
          <div className="card-f wizard-nav">
            <button className="btn ghost" onClick={() => setStep(1)}>
              <ArrowLeft size={14} />上一步
            </button>
            <span className="grow" />
            <button className="btn primary" disabled={!inputOk || busy} onClick={() => void run()}>
              {busy && <span className="spinner" />}
              {mode === "text" ? "开始还原" : "还原并另存为…"}
            </button>
          </div>
        </div>
      )}

      {step === 3 && (result || docResult) && (
        <div className="stack">
          <div className="card">
            <div className="card-h">
              第 3 步：结果
              <span className="grow" />
              <div className="rs-summary">
                <Badge tone="success" size="lg">精确还原 {exact}</Badge>
                <Badge tone={fuzzy ? "warn" : "neutral"} size="lg">模糊还原 {fuzzy}{fuzzy ? ` · 待核对 ${pending}` : ""}</Badge>
                <Badge tone={suspects ? "danger" : "neutral"} size="lg">疑似未还原 {suspects}</Badge>
              </div>
            </div>

            {exact + fuzzy === 0 && (
              <div style={{ padding: "14px 18px 0" }}>
                <Callout tone="warn" compact>
                  没有找到可还原的占位符。请确认内容来自这份映射对应的脱敏稿，或 AI 是否把占位符改写得面目全非。
                </Callout>
              </div>
            )}

            {result && (
              <>
                <div style={{ padding: "12px 18px 0" }} className="legend" aria-hidden>
                  <span><i className="fz" />模糊还原（待核对）</span>
                  <span><i className="ok" />已核对</span>
                  <span><i className="sus" />疑似未还原</span>
                </div>
                <div className="rs-out" aria-label="还原结果" tabIndex={0}>
                  {segmentOutput(result, dec).map((g, i) =>
                    g.hit !== undefined ? (
                      <mark
                        key={i}
                        id={`fz-${g.hit}`}
                        className={`fz ${dec[g.hit] === "ok" ? "ok" : dec[g.hit] === "reverted" ? "reverted" : ""}`}
                        title={`#${g.hit + 1} 模糊还原：AI 写作 ${result.fuzzy_hits[g.hit].raw}`}
                      >
                        {g.text}
                      </mark>
                    ) : g.sus ? (
                      <mark key={i} className="sus" title="形如占位符，但映射里没有">{g.text}</mark>
                    ) : (
                      <span key={i}>{g.text}</span>
                    ),
                  )}
                </div>
                <div className="card-f">
                  <button className="btn primary" onClick={copyResult}><Copy size={14} />复制结果</button>
                  <button className="btn" onClick={() => void exportResult()}><Download size={14} />导出为文本</button>
                  <span className="grow" style={{ flex: 1 }} />
                  <button className="btn ghost" onClick={restart}><RotateCcw size={14} />还原另一段</button>
                </div>
              </>
            )}

            {docResult && (
              <div className="card-b stack-sm">
                <div className="file-pick">
                  <span className="fp-name">{docResult.output_path}</span>
                  <button className="btn sm" onClick={() => void openInFolder(docResult.output_path).catch((e) => notify("无法打开文件夹", { tone: "danger", detail: String(e) }))}>
                    <FolderOpen size={13} />在文件夹中显示
                  </button>
                </div>
                <div className="btn-row">
                  <span className="hint" style={{ flex: 1 }}>已另存为新文件，原文件未改动。模糊还原已写入文件，请打开文件按下方位置逐条核对。</span>
                  <button className="btn ghost" onClick={restart}><RotateCcw size={14} />还原另一个文件</button>
                </div>
              </div>
            )}
          </div>

          {((result && result.fuzzy_hits.length > 0) || (docResult && docResult.fuzzy_hits.length > 0)) && (
            <div className="card">
              <div className="card-h">
                模糊还原核对
                <span className="card-h-sub">AI 改了占位符写法，但仍能唯一对应。请逐条确认是否还原正确。</span>
                <span className="grow" />
                {pending > 0 && (
                  <button className="btn sm" onClick={() => setDec((x) => x.map((d) => (d === "pending" ? "ok" : d)))}>
                    <Check size={13} />全部确认
                  </button>
                )}
              </div>
              <ul className="review-list">
                {(result ? result.fuzzy_hits : docResult!.fuzzy_hits).map((h, i) => (
                  <li className="review-item" key={i}>
                    <button
                      className="ri-n"
                      onClick={() => result && flash(i)}
                      aria-label={result ? `在结果中定位第 ${i + 1} 处` : `第 ${i + 1} 处`}
                      disabled={!result}
                    >
                      #{i + 1}
                    </button>
                    <div style={{ minWidth: 0 }}>
                      <div className="ri-flow">
                        <span className="muted">AI 写作</span>
                        <code>{h.raw}</code>
                        <ArrowRight size={12} aria-hidden className="muted" />
                        <code>{h.placeholder}</code>
                        <ArrowRight size={12} aria-hidden className="muted" />
                        <span className="ri-orig">{h.original}</span>
                      </div>
                      {"where" in h && <div className="ri-where">位置：{(h as { where: string }).where}</div>}
                    </div>
                    <div className="ri-act">
                      {dec[i] === "ok" ? (
                        <Badge tone="success" icon={<Check size={11} />}>已确认</Badge>
                      ) : dec[i] === "reverted" ? (
                        <Badge>已改回原写法</Badge>
                      ) : (
                        <Badge tone="warn">待核对</Badge>
                      )}
                      {dec[i] !== "ok" && (
                        <button className="btn sm" onClick={() => setDecision(i, "ok")} aria-label={`确认第 ${i + 1} 处还原正确`}>
                          <Check size={13} />正确
                        </button>
                      )}
                      {result && dec[i] !== "reverted" && (
                        <button className="btn sm ghost" onClick={() => setDecision(i, "reverted")} aria-label={`第 ${i + 1} 处不还原，改回 AI 原写法`}>
                          <Undo2 size={13} />不还原
                        </button>
                      )}
                      {dec[i] !== "pending" && (
                        <button className="btn sm ghost" onClick={() => setDec((x) => x.map((v, k) => (k === i ? "pending" : v)))}>
                          重置
                        </button>
                      )}
                    </div>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {suspects > 0 && (
            <Callout tone="danger" title={`${suspects} 处疑似未还原的占位符`}>
              这些内容形如占位符，但映射里没有对应项，可能来自另一份映射、或被 AI 编造。请人工判断：
              <div className="chip-list" style={{ marginTop: 8 }}>
                {(result ? result.suspects.map((s) => ({ text: s.text, where: "" })) : docResult!.suspects).map((s, i) => (
                  <span className="chip" key={i} style={{ paddingRight: 10 }}>
                    <code>{s.text}</code>
                    {s.where && <span className="muted" style={{ fontSize: 12 }}>· {s.where}</span>}
                  </span>
                ))}
              </div>
            </Callout>
          )}
        </div>
      )}
    </div>
  );
}
