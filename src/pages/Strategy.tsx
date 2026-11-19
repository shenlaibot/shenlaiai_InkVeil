// 策略设置：检测规则（开关 + 输出方式 + 人话示例）/ 敏感词典 / 本地识别模型 / 映射文件口令。
// 规则表不展示正则：每行给出“原文 → 输出”的示例，示例随当前输出方式实时变化。
import { useMemo, useState } from "react";
import {
  BookText,
  BrainCircuit,
  Eye,
  EyeOff,
  KeyRound,
  ListChecks,
  RotateCcw,
  ArrowRight,
  Plus,
  X,
} from "lucide-react";
import { useApp, STRUCTURE_PRESERVING, defaultStrategies } from "../store";
import { getBuiltinRules } from "../api";
import type { RuleDef, StrategyDto } from "../types";
import { Badge, Callout, EmptyState, PageHead, Segmented, Switch } from "../components/ui/primitives";
import type { Tone } from "../components/ui/primitives";
import { confirmDialog } from "../components/ui/Dialog";
import { notify } from "../components/ui/Toast";

type Output = StrategyDto["output"];

/** 每种输出方式：短名 + 可还原性标签 + 一句话说明 */
const OUTPUT_META: Record<Output, { label: string; tag: string; tone: Tone; desc: string }> = {
  placeholder: {
    label: "占位符",
    tag: "可还原 · 发给 AI 推荐",
    tone: "success",
    desc: "替换成 [姓名1] 这样的编号；同一对象全文同号，AI 能看懂指代关系，返回后可用映射还原。",
  },
  mask: { label: "掩码", tag: "不可还原", tone: "neutral", desc: "用替换符遮住中间部分，如 138****8000；原文无法从输出恢复。" },
  fake: { label: "假数据", tag: "不可还原", tone: "neutral", desc: "换成同格式的虚构号码（仅手机号/身份证/银行卡）；看起来像真的，阅读时需留意。" },
  aes: { label: "加密", tag: "暂不支持还原", tone: "warn", desc: "原文加密为一段密文，需要口令；本工具的还原功能目前不解密这类内容。" },
  keep: { label: "不处理", tag: "不安全 · 原文保留", tone: "danger", desc: "原文原样保留在输出里。仅适用于确需保留的内容（如金额、日期）。" },
  scale: { label: "保结构", tag: "可还原 · 假名化", tone: "info", desc: "金额按同一系数缩放，数量关系保留；知道任一真实值即可反推全部。" },
  shift: { label: "保结构", tag: "可还原 · 假名化", tone: "info", desc: "日期整体平移同样天数，间隔与星期保留；知道任一真实日期即可反推全部。" },
  base64: { label: "BASE64", tag: "不安全", tone: "danger", desc: "只是编码，任何人可解码。" },
};

const FAKE_OK = new Set(["mobile", "id_card", "bank_card"]);

/** 示例原文均为虚构 */
const EXAMPLES: Record<string, string> = {
  mobile: "138 0013 8000",
  id_card: "110101199003071234",
  bank_card: "6222020200112233445",
  credit_code: "91310115MA1K3YJ12X",
  company_name: "上海星河科技有限公司",
  bank_name: "招商银行上海分行",
  name: "张三",
  phone: "021-6888 1234",
  plate: "沪A12345",
  email: "zhangsan@example.com",
  ip: "192.168.10.24",
  url: "https://example.com/c/123",
  amount: "50,000 元",
  date: "2024年3月1日",
  case_no: "（2024）沪0115民初1234号",
  contract_no: "HT-2024-0088",
  address: "上海市浦东新区世纪大道100号",
  bank_account: "31001234567800001",
  passport: "E12345678",
  id_card_hmt: "A123456(7)",
};

const ILLUSTRATIVE: Partial<Record<Output, string>> = {
  fake: "（示意）",
  aes: "（示意）",
  scale: "（示意）",
  shift: "（示意）",
};

const SEPARATORS = new Set([" ", "-", "·", "_", "　", "."]);

/** 与后端 transform.rs 的掩码规则一致：保留首/尾 N 位，中间用替换符；手机号保留分隔符位置 */
function maskPreview(ruleId: string, text: string, s: StrategyDto): string {
  const chars = Array.from(text);
  if (ruleId === "mobile" && chars.some((c) => SEPARATORS.has(c))) {
    const digitIdx = chars.map((c, i) => (/\d/.test(c) ? i : -1)).filter((i) => i >= 0);
    const n = digitIdx.length;
    const head = Math.min(s.keep_head, n);
    const tail = Math.min(s.keep_tail, n - head);
    const masked = new Set(digitIdx.slice(head, n - tail));
    return chars.map((c, i) => (masked.has(i) ? s.mask_char : c)).join("");
  }
  const n = chars.length;
  const head = Math.min(s.keep_head, n);
  const tail = Math.min(s.keep_tail, n - head);
  return chars.slice(0, head).join("") + s.mask_char.repeat(n - head - tail) + chars.slice(n - tail).join("");
}

function preview(rule: RuleDef, s: StrategyDto): string {
  const src = EXAMPLES[rule.id];
  if (!src) return "";
  switch (s.output) {
    case "placeholder":
      return `[${rule.label}1]`;
    case "mask":
      return maskPreview(rule.id, src, s);
    case "fake":
      return rule.id === "mobile" ? "139 5521 0738" : rule.id === "id_card" ? "310104198811230418" : "6217003810024471626";
    case "aes":
      return "Xk9fQ2…（密文）";
    case "keep":
      return src;
    case "scale":
      return "63,500 元";
    case "shift":
      return "2024年5月17日";
    default:
      return maskPreview(rule.id, src, s);
  }
}

/** 规则分组（未列出的 id 归入“其他”） */
const GROUPS: { title: string; ids: string[] }[] = [
  { title: "人员与身份", ids: ["name", "id_card", "passport", "id_card_hmt", "mobile", "phone", "email", "address", "plate"] },
  { title: "机构与案件", ids: ["company_name", "credit_code", "case_no", "contract_no"] },
  { title: "金融信息", ids: ["bank_card", "bank_account", "bank_name"] },
  { title: "金额与日期", ids: ["amount", "date"] },
  { title: "网络标识", ids: ["ip", "url"] },
];

function optionsFor(ruleId: string, current: Output): Output[] {
  const opts: Output[] = ["placeholder", "mask"];
  if (FAKE_OK.has(ruleId) || current === "fake") opts.push("fake");
  opts.push("aes", "keep");
  const sp = STRUCTURE_PRESERVING[ruleId];
  if (sp) opts.push(sp);
  return opts;
}

type Tab = "rules" | "dict" | "ner" | "pass";

export function StrategyPage() {
  const { config, updateConfig, modelReady, navigate, configReady } = useApp();
  const [tab, setTab] = useState<Tab>("rules");
  const [dictInput, setDictInput] = useState("");
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [showPass, setShowPass] = useState(false);
  const [resetting, setResetting] = useState(false);

  const rules: RuleDef[] = config.rules;
  const enabledCount = rules.filter((r) => r.enabled).length;

  const strategyOf = (id: string): StrategyDto =>
    config.strategies[id] ?? { output: "mask", mask_char: config.maskChar, keep_head: 0, keep_tail: 0 };

  const setStrategy = (ruleId: string, patch: Partial<StrategyDto>) => {
    updateConfig({ strategies: { ...config.strategies, [ruleId]: { ...strategyOf(ruleId), ...patch } } });
  };

  const toggleRule = (ruleId: string) => {
    updateConfig({ rules: config.rules.map((r) => (r.id === ruleId ? { ...r, enabled: !r.enabled } : r)) });
  };

  const resetRules = async () => {
    const ok = await confirmDialog({
      title: "恢复默认规则与输出方式？",
      message: "所有检测规则的开关、输出方式和保留位数会恢复为默认值。敏感词典、识别模型和口令设置不受影响。",
      confirmText: "恢复默认",
      tone: "warn",
    });
    if (!ok) return;
    setResetting(true);
    const before = { rules: config.rules, strategies: config.strategies };
    try {
      const builtin = await getBuiltinRules();
      const ch = config.maskChar;
      const strategies = Object.fromEntries(
        Object.entries(defaultStrategies()).map(([k, s]) => [k, { ...s, mask_char: ch }]),
      );
      updateConfig({ rules: builtin, strategies });
      notify("已恢复默认规则与输出方式", {
        tone: "success",
        action: { label: "撤销", onClick: () => updateConfig(before) },
      });
    } catch (e) {
      notify("恢复默认失败", { tone: "danger", detail: String(e) });
    } finally {
      setResetting(false);
    }
  };

  const onMaskChar = (v: string) => {
    const ch = Array.from(v).pop() || "*";
    updateConfig({
      maskChar: ch,
      strategies: Object.fromEntries(Object.entries(config.strategies).map(([k, s]) => [k, { ...s, mask_char: ch }])),
    });
  };

  const addDictTerm = () => {
    const term = dictInput.trim();
    if (!term) return;
    if (config.dictTerms.includes(term)) {
      notify(`词典里已有「${term}」`, { tone: "warn" });
      return;
    }
    updateConfig({ dictTerms: [...config.dictTerms, term] });
    setDictInput("");
  };

  const removeDictTerm = (term: string) => {
    const before = config.dictTerms;
    updateConfig({ dictTerms: before.filter((t) => t !== term) });
    notify(`已移除「${term}」`, { action: { label: "撤销", onClick: () => updateConfig({ dictTerms: before }) } });
  };

  const importDict = () => {
    let terms: string[];
    const raw = importText.trim();
    try {
      const parsed = raw.startsWith("[") ? (JSON.parse(raw) as unknown) : null;
      terms = Array.isArray(parsed) ? parsed.map(String) : raw.split(/[\n,，;；]/);
    } catch {
      terms = raw.split(/[\n,，;；]/);
    }
    terms = terms.map((t) => t.trim()).filter(Boolean);
    if (!terms.length) {
      notify("没有可导入的词条", { tone: "warn" });
      return;
    }
    const merged = [...config.dictTerms];
    let added = 0;
    for (const t of terms) {
      if (!merged.includes(t)) {
        merged.push(t);
        added++;
      }
    }
    const before = config.dictTerms;
    updateConfig({ dictTerms: merged });
    setImportText("");
    setImportOpen(false);
    notify(`导入完成：新增 ${added} 条${terms.length - added ? `，${terms.length - added} 条已存在` : ""}`, {
      tone: "success",
      action: added ? { label: "撤销", onClick: () => updateConfig({ dictTerms: before }) } : undefined,
    });
  };

  const grouped = useMemo(() => {
    const byId = new Map(rules.map((r) => [r.id, r]));
    const used = new Set<string>();
    const out = GROUPS.map((g) => ({
      title: g.title,
      rules: g.ids.map((id) => byId.get(id)).filter((r): r is RuleDef => !!r),
    }));
    out.forEach((g) => g.rules.forEach((r) => used.add(r.id)));
    const rest = rules.filter((r) => !used.has(r.id));
    if (rest.length) out.push({ title: "其他", rules: rest });
    return out.filter((g) => g.rules.length);
  }, [rules]);

  const keepCount = rules.filter((r) => r.enabled && strategyOf(r.id).output === "keep").length;

  return (
    <div>
      <PageHead
        title="策略设置"
        sub="决定识别哪些信息、以及在导出稿里替换成什么。设置只保存在本机。"
      />

      <div className="strat-tabs">
        <Segmented<Tab>
          role="tablist"
          size="lg"
          label="设置分区"
          value={tab}
          onChange={setTab}
          options={[
            { value: "rules", label: <><ListChecks size={15} />检测规则</> },
            { value: "dict", label: <><BookText size={15} />敏感词典{config.dictTerms.length ? ` · ${config.dictTerms.length}` : ""}</> },
            { value: "ner", label: <><BrainCircuit size={15} />识别模型</> },
            { value: "pass", label: <><KeyRound size={15} />映射口令</> },
          ]}
        />
      </div>

      {tab === "rules" && (
        <div className="stack">
          <div className="card">
            <div className="card-h">
              输出方式说明
              <span className="card-h-sub">每类信息可单独选择</span>
            </div>
            <div className="opt-legend">
              {(["placeholder", "mask", "fake", "keep", "scale"] as Output[]).map((o) => (
                <div className="ol-item" key={o}>
                  <div className="ol-name">
                    {OUTPUT_META[o].label}
                    <Badge tone={OUTPUT_META[o].tone}>{OUTPUT_META[o].tag}</Badge>
                  </div>
                  <div className="ol-desc">
                    {o === "scale" ? "仅金额、日期可选。金额按同一系数缩放，日期整体平移；知道任一真实值即可反推全部，属于假名化而非匿名化。" : OUTPUT_META[o].desc}
                  </div>
                </div>
              ))}
            </div>
          </div>

          {keepCount > 0 && (
            <Callout tone="warn" compact>
              有 {keepCount} 类信息设为“不处理”，它们的原文会出现在导出稿里。金额、日期默认不处理，是因为合同审查、诉讼中它们通常是分析对象。
            </Callout>
          )}

          <div className="card">
            <div className="card-h">
              检测规则
              <span className="card-h-sub">
                {rules.length} 类 · 已启用 {enabledCount}
              </span>
              <span className="grow" />
              <label className="keep-ctl" title="掩码使用的填充字符，应用到全部规则">
                掩码替换符
                <input
                  className="num-input"
                  aria-label="掩码替换符"
                  value={config.maskChar}
                  onChange={(e) => onMaskChar(e.target.value)}
                  onFocus={(e) => e.currentTarget.select()}
                />
              </label>
              <button className="btn sm" onClick={() => void resetRules()} disabled={resetting || !rules.length}>
                {resetting ? <span className="spinner" /> : <RotateCcw size={14} />}
                恢复默认
              </button>
            </div>

            {!configReady && rules.length === 0 ? (
              <div style={{ padding: 18 }} role="status">
                <span className="hint">正在载入规则…</span>
              </div>
            ) : rules.length === 0 ? (
              <EmptyState
                icon={<ListChecks size={22} />}
                title="没有可用的检测规则"
                desc="规则列表为空，导出前不会识别任何内置类型。可以点“载入默认规则”重新加载内置规则。"
                action={
                  <button
                    className="btn primary"
                    onClick={() =>
                      void getBuiltinRules()
                        .then((b) => {
                          updateConfig({ rules: b });
                          notify(b.length ? `已载入 ${b.length} 类默认规则` : "内置规则为空", { tone: b.length ? "success" : "warn" });
                        })
                        .catch((e) => notify("载入失败", { tone: "danger", detail: String(e) }))
                    }
                  >
                    载入默认规则
                  </button>
                }
              />
            ) : (
              grouped.map((g) => (
                <div key={g.title} role="group" aria-label={g.title}>
                  <div className="rule-group-h">{g.title}</div>
                  {g.rules.map((r) => {
                    const s = strategyOf(r.id);
                    const meta = OUTPUT_META[s.output] ?? OUTPUT_META.mask;
                    const ex = EXAMPLES[r.id];
                    const out = preview(r, s);
                    const isAmountDate = r.id === "amount" || r.id === "date";
                    return (
                      <div className={`rule-row ${r.enabled ? "" : "off"}`} key={r.id}>
                        <Switch checked={r.enabled} onChange={() => toggleRule(r.id)} label={`识别${r.label}`} />
                        <div className="rr-name">
                          {r.label}
                          <span className="rr-sub">{r.enabled ? "识别" : "不识别"}</span>
                        </div>
                        <div className="rr-example" aria-label={ex ? `示例：${ex} 替换为 ${out}` : undefined}>
                          {ex ? (
                            <>
                              <span className="ex-src">{ex}</span>
                              <ArrowRight size={13} className="ex-arrow" aria-hidden />
                              <span className={`ex-out ${s.output === "keep" ? "keep" : ""}`}>{out}</span>
                              {ILLUSTRATIVE[s.output] && <span className="ex-note">{ILLUSTRATIVE[s.output]}</span>}
                            </>
                          ) : (
                            <span className="ex-note">自定义规则</span>
                          )}
                        </div>
                        <div className="rr-controls">
                          <Segmented<Output>
                            label={`${r.label}的输出方式`}
                            value={s.output}
                            onChange={(v) => setStrategy(r.id, { output: v })}
                            options={optionsFor(r.id, s.output).map((o) => ({
                              value: o,
                              label: OUTPUT_META[o].label,
                              title: `${OUTPUT_META[o].tag}：${OUTPUT_META[o].desc}`,
                              disabled: !r.enabled,
                            }))}
                          />
                          <Badge tone={meta.tone} className="rr-tag">{meta.tag}</Badge>
                        </div>
                        {r.enabled && s.output === "mask" && (
                          <div className="rr-extra">
                            <span className="keep-ctl">
                              保留前
                              <input
                                type="number"
                                min={0}
                                max={10}
                                className="num-input"
                                aria-label={`${r.label}掩码保留开头位数`}
                                value={s.keep_head}
                                onChange={(e) => setStrategy(r.id, { keep_head: Math.max(0, Math.min(10, Number(e.target.value) || 0)) })}
                              />
                              位，后
                              <input
                                type="number"
                                min={0}
                                max={10}
                                className="num-input"
                                aria-label={`${r.label}掩码保留末尾位数`}
                                value={s.keep_tail}
                                onChange={(e) => setStrategy(r.id, { keep_tail: Math.max(0, Math.min(10, Number(e.target.value) || 0)) })}
                              />
                              位
                            </span>
                          </div>
                        )}
                        {r.enabled && isAmountDate && (
                          <div className="rr-extra">
                            {s.output === "keep" ? (
                              <span>
                                默认不处理：{r.id === "amount" ? "违约金、利息、标的额" : "时效、期限、履行日"}通常是审查对象，改动会影响分析结论。需要隐藏时可选“占位符”或“保结构”。
                              </span>
                            ) : s.output === "scale" || s.output === "shift" ? (
                              <Callout tone="warn" compact>
                                仅为假名化，不是匿名化：同一案件共用一组{s.output === "scale" ? "缩放系数（百分比、费率、条款号不变）" : "平移天数（星期不变，法规施行日不动）"}，
                                掌握任一真实值与替换值的对照即可反推全部。映射保存在项目或映射文件里，可精确还原；AI 回复里新算出的数只给建议值，不会自动替换。
                              </Callout>
                            ) : null}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              ))
            )}
          </div>
        </div>
      )}

      {tab === "dict" && (
        <div className="card">
          <div className="card-h">
            敏感词典
            <span className="card-h-sub">{config.dictTerms.length} 条 · 全文精确匹配 · 适用于所有项目</span>
            <span className="grow" />
            <button className="btn sm" onClick={() => setImportOpen((v) => !v)} aria-expanded={importOpen}>
              {importOpen ? "取消批量导入" : "批量导入"}
            </button>
          </div>
          <div className="card-b stack-sm">
            <div className="hint">
              添加内置规则识别不到的词：项目代号、客户简称、内部称呼等。只对某个案件有效的词，请加到“项目”页的案件词典里。
            </div>
            {importOpen ? (
              <form
                className="stack-sm"
                onSubmit={(e) => {
                  e.preventDefault();
                  importDict();
                }}
              >
                <textarea
                  className="textarea"
                  value={importText}
                  onChange={(e) => setImportText(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) importDict();
                  }}
                  placeholder={"每行一个词条，或用逗号分隔；也支持 JSON 数组，如 [\"星辰计划\", \"老王\"]"}
                  rows={5}
                  aria-label="批量导入词条"
                  autoFocus
                />
                <div className="btn-row">
                  <button className="btn primary sm" type="submit">导入</button>
                  <span className="hint">⌘/Ctrl + Enter 也可导入；重复的词条会自动跳过</span>
                </div>
              </form>
            ) : (
              <form
                className="inline-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  addDictTerm();
                }}
              >
                <input
                  className="input"
                  value={dictInput}
                  onChange={(e) => setDictInput(e.target.value)}
                  placeholder="输入词条，回车添加"
                  aria-label="新词条"
                />
                <button className="btn primary" type="submit" disabled={!dictInput.trim()}>
                  <Plus size={15} />
                  添加
                </button>
              </form>
            )}
            {config.dictTerms.length === 0 ? (
              <EmptyState
                compact
                icon={<BookText size={20} />}
                title="词典为空"
                desc="不添加也能正常使用；内置规则与上下文识别仍然生效。"
              />
            ) : (
              <ul className="chip-list" aria-label="已添加的词条" style={{ listStyle: "none", marginTop: 4 }}>
                {config.dictTerms.map((t) => (
                  <li className="chip" key={t}>
                    <span className="chip-t" title={t}>{t}</span>
                    <button className="chip-x" onClick={() => removeDictTerm(t)} aria-label={`移除词条 ${t}`}>
                      <X size={13} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}

      {tab === "ner" && (
        <div className="card">
          <div className="card-h">
            本地识别模型（NER）
            <Badge tone={modelReady ? "success" : "neutral"}>{modelReady ? "模型已安装" : "未安装模型"}</Badge>
          </div>
          <div className="strat-row">
            <div className="sr-text">
              <div className="sr-lbl">用模型补充识别人名、机构、地址</div>
              <div className="sr-desc">
                在规则、词典之外，用本地模型找出没有“联系人：”等前缀的人名、机构名和地址。模型在本机运行，不联网。
                {!modelReady && "当前未安装模型，此开关打开也不会生效。"}
              </div>
            </div>
            <Switch
              checked={config.nerEnabled}
              onChange={(v) => updateConfig({ nerEnabled: v })}
              label="用本地模型补充识别"
            />
          </div>
          {config.nerEnabled && (
            <div className="strat-row">
              <div className="sr-text">
                <div className="sr-lbl">识别灵敏度</div>
                <div className="sr-desc">
                  只保留置信度不低于该值的结果。调低会多找出一些，也会多一些误报，需要在预览页多核对。
                </div>
              </div>
              <div className="range-ctl">
                <span className="hint">多找</span>
                <input
                  type="range"
                  min={0.3}
                  max={0.95}
                  step={0.05}
                  value={config.nerThreshold}
                  onChange={(e) => updateConfig({ nerThreshold: Number(e.target.value) })}
                  aria-label="置信度阈值"
                  aria-valuetext={config.nerThreshold.toFixed(2)}
                />
                <span className="hint">少误报</span>
                <span className="rv">{config.nerThreshold.toFixed(2)}</span>
              </div>
            </div>
          )}
          {!modelReady && (
            <div className="card-f">
              <span className="hint grow" style={{ flex: 1 }}>模型约 400 MB，需在模型市场下载（需要联网，可随时删除）。</span>
              <button className="btn sm" onClick={() => navigate("models")}>
                前往模型市场
              </button>
            </div>
          )}
        </div>
      )}

      {tab === "pass" && (
        <div className="card">
          <div className="card-h">映射文件口令</div>
          <div className="card-b stack-sm">
            <div className="hint" style={{ lineHeight: 1.75 }}>
              不打开项目时，导出会额外生成一个加密的映射文件（.anonymap.enc），用它和这个口令才能把占位符还原为原文。
              口令只保存在内存里，重启应用后需要重新输入；<b>不设口令就不会生成映射文件</b>，导出的脱敏稿将无法还原。
            </div>
            <div className="field" style={{ maxWidth: 420 }}>
              <label className="field-label" htmlFor="map-pass">口令</label>
              <div className="input-group">
                <input
                  id="map-pass"
                  type={showPass ? "text" : "password"}
                  className="input"
                  placeholder="输入用于加密映射文件的口令"
                  autoComplete="new-password"
                  value={config.passphrase ?? ""}
                  onChange={(e) => updateConfig({ passphrase: e.target.value || null })}
                />
                <button
                  className="btn sm ghost icon-only input-addon"
                  onClick={() => setShowPass((v) => !v)}
                  aria-label={showPass ? "隐藏口令" : "显示口令"}
                  aria-pressed={showPass}
                >
                  {showPass ? <EyeOff size={15} /> : <Eye size={15} />}
                </button>
              </div>
            </div>
            {config.passphrase ? (
              <Callout tone="success" compact>已设置口令（仅本次运行有效）。导出时会生成加密映射文件。</Callout>
            ) : (
              <Callout tone="warn" compact>未设置口令：除非打开了项目，否则导出的脱敏稿无法还原。</Callout>
            )}
            <div className="hint">
              处理同一案件的多批文件时，更推荐在“项目”页新建项目：映射自动存进项目，占位符编号跨批次保持一致。{" "}
              <button className="link-btn" onClick={() => navigate("vault")}>去项目页</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
