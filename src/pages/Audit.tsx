// 审计日志：脱敏操作记录（时间/文件/类型分布/结果），带哈希链防篡改校验；
// 可筛选、分页、导出；保存期限与修剪放在“高级”里（不提供清空）。
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ChevronRight,
  Download,
  RotateCw,
  ScrollText,
  Search,
  ShieldAlert,
  ShieldCheck,
  CircleAlert,
  Scissors,
} from "lucide-react";
import { useApp } from "../store";
import { auditList, auditPrune, auditRetentionGet, auditRetentionSet, auditVerify, saveTextFile } from "../api";
import type { AuditEntry, AuditVerify } from "../types";
import { Badge, EmptyState, PageHead, Segmented, SkeletonRows, fileName } from "../components/ui/primitives";
import type { Tone } from "../components/ui/primitives";
import { confirmDialog } from "../components/ui/Dialog";
import { notify } from "../components/ui/Toast";

const RETENTION_OPTIONS = [
  { days: 365, label: "1 年" },
  { days: 730, label: "2 年" },
  { days: 1095, label: "3 年（默认）" },
  { days: 1825, label: "5 年" },
  { days: 3650, label: "10 年" },
];

const PAGE_SIZE = 20;

type Result = "deep" | "check" | "release" | "blocked" | "system";
const RESULT_META: Record<Result, { label: string; tone: Tone; title: string }> = {
  deep: { label: "已复检", tone: "success", title: "已用导出结果复检：未在输出的可提取内容中发现已确认项的原文（不含图片内文字）" },
  check: { label: "需核对", tone: "warn", title: "存在未处理或无法确认的部位，外发前请人工核对" },
  release: { label: "放行", tone: "danger", title: "存在风险项，由操作者填写理由后放行导出" },
  blocked: { label: "已阻止", tone: "danger", title: "复检发现残留或导出失败，没有生成输出" },
  system: { label: "系统", tone: "neutral", title: "修剪或迁移记录" },
};

function resultOf(r: AuditEntry): Result {
  if (r.kind && r.kind !== "entry") return "system";
  if (r.note?.startsWith("带风险放行")) return "release";
  if (r.output === "失败" || r.note?.includes("阻止")) return "blocked";
  return r.is_deep ? "deep" : "check";
}

function parseTypes(s: string): { label: string; n: string }[] {
  return s
    .split(/\s+/)
    .filter(Boolean)
    .map((t) => {
      const [label, n] = t.split("×");
      return { label, n: n ?? "" };
    });
}

function parseTime(s: string): number {
  const t = new Date(s.replace(/\//g, "-").replace(" UTC", "Z")).getTime();
  return isNaN(t) ? 0 : t;
}

type DateFilter = "all" | "today" | "week" | "month";
type ResultFilter = "all" | Result;

export function AuditPage() {
  const { audit: sessionAudit } = useApp();
  const [audit, setAudit] = useState<AuditEntry[] | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [verify, setVerify] = useState<AuditVerify | null>(null);
  const [verifying, setVerifying] = useState(false);
  const [retention, setRetention] = useState(1095);
  const [dateF, setDateF] = useState<DateFilter>("all");
  const [resF, setResF] = useState<ResultFilter>("all");
  const [q, setQ] = useState("");
  const [page, setPage] = useState(0);

  const reload = useCallback(async () => {
    setVerifying(true);
    try {
      setAudit(await auditList(5000));
      setLoadErr(null);
    } catch (e) {
      setAudit([]);
      setLoadErr(String(e));
    }
    try {
      setVerify(await auditVerify());
    } catch {
      setVerify(null);
    } finally {
      setVerifying(false);
    }
  }, []);

  useEffect(() => {
    // 新记录由后端异步落库，稍等再读，拿到带链哈希的版本
    const t = setTimeout(() => void reload(), 300);
    return () => clearTimeout(t);
  }, [reload, sessionAudit.length]);

  useEffect(() => {
    auditRetentionGet().then(setRetention).catch(() => undefined);
  }, []);

  useEffect(() => setPage(0), [dateF, resF, q]);

  const onRetention = (days: number) => {
    setRetention(days);
    auditRetentionSet(days)
      .then(() => notify("保存期限已更新", { tone: "success" }))
      .catch((e) => notify("保存期限设置失败", { tone: "danger", detail: String(e) }));
  };

  const onPrune = async () => {
    const opt = RETENTION_OPTIONS.find((o) => o.days === retention)?.label ?? `${retention} 天`;
    const ok = await confirmDialog({
      title: "按保存期限修剪记录？",
      message: `将删除早于保存期限（${opt}）的记录，并留下一条不可删除的修剪记录（含删除条数与根哈希），链校验仍可通过。删除的记录无法恢复。`,
      confirmText: "修剪",
      tone: "danger",
    });
    if (!ok) return;
    try {
      const n = await auditPrune(retention);
      await reload();
      notify(n > 0 ? `已修剪 ${n} 条过期记录` : "没有超过保存期限的记录", { tone: n > 0 ? "success" : "info" });
    } catch (e) {
      notify("修剪失败", { tone: "danger", detail: String(e) });
    }
  };

  const rows = useMemo(() => {
    const now = Date.now();
    const span = { all: Infinity, today: 24 * 3600e3, week: 7 * 24 * 3600e3, month: 30 * 24 * 3600e3 }[dateF];
    const kw = q.trim().toLowerCase();
    return (audit ?? []).filter((a) => {
      if (span !== Infinity && now - parseTime(a.time) > span) return false;
      if (resF !== "all" && resultOf(a) !== resF) return false;
      if (kw && !`${a.file} ${a.output} ${a.types}`.toLowerCase().includes(kw)) return false;
      return true;
    });
  }, [audit, dateF, resF, q]);

  const counts = useMemo(() => {
    const c: Record<string, number> = {};
    for (const a of audit ?? []) c[resultOf(a)] = (c[resultOf(a)] ?? 0) + 1;
    return c;
  }, [audit]);

  const pages = Math.max(1, Math.ceil(rows.length / PAGE_SIZE));
  const cur = Math.min(page, pages - 1);
  const pageRows = rows.slice(cur * PAGE_SIZE, cur * PAGE_SIZE + PAGE_SIZE);

  const onExport = async () => {
    if (!rows.length) {
      notify("当前筛选下没有可导出的记录", { tone: "warn" });
      return;
    }
    const header = "序号,类别,时间,文件,命中,类型分布,输出,映射,结果,备注,上一哈希,哈希";
    const kindLabel = { entry: "操作", prune: "修剪", migration: "迁移" } as const;
    const lines = rows.map((r) =>
      [
        r.seq ?? "",
        kindLabel[r.kind ?? "entry"],
        r.time,
        r.file,
        r.hit_count,
        r.types,
        r.output,
        r.mapping,
        RESULT_META[resultOf(r)].label,
        r.note ?? "",
        r.prev_hash ?? "",
        r.hash ?? "",
      ]
        .map((v) => `"${String(v).replace(/"/g, '""')}"`)
        .join(","),
    );
    try {
      const path = await saveTextFile("anonkit-audit.csv", [header, ...lines].join("\n"));
      if (path) notify(`已导出 ${rows.length} 条记录：${fileName(path)}`, { tone: "success" });
    } catch (e) {
      notify("导出失败", { tone: "danger", detail: String(e) });
    }
  };

  const chain = verifying && !verify ? (
    <div className="chain" role="status">
      <span className="chain-icon"><span className="spinner dark" /></span>
      <div className="chain-text"><div className="chain-title">正在校验哈希链…</div></div>
    </div>
  ) : verify ? (
    verify.ok ? (
      <div className="chain ok" data-testid="audit-chain">
        <span className="chain-icon" aria-hidden><ShieldCheck size={20} /></span>
        <div className="chain-text">
          <div className="chain-title">哈希链完整</div>
          <div className="chain-sub">
            已校验 {verify.count} 条记录，未发现被改动或删除的记录
            {verify.pruned_total > 0 ? `；另有 ${verify.pruned_total} 条按保存期限修剪（有修剪记录可查）` : ""}。
          </div>
        </div>
        <button className="btn sm" onClick={() => void reload()} disabled={verifying}>
          {verifying ? <span className="spinner" /> : <RotateCw size={13} />}重新校验
        </button>
      </div>
    ) : (
      <div className="chain bad" role="alert" data-testid="audit-chain">
        <span className="chain-icon" aria-hidden><ShieldAlert size={20} /></span>
        <div className="chain-text">
          <div className="chain-title">哈希链校验未通过：第 {verify.broken_seq} 条</div>
          <div className="chain-sub">
            {verify.reason}。说明日志数据库在本应用之外被修改或损坏，第 {verify.broken_seq} 条及之后的记录不能作为可信记录使用。请保留当前数据库文件，不要再修剪。
          </div>
        </div>
        <button className="btn sm" onClick={() => void reload()} disabled={verifying}>
          {verifying ? <span className="spinner" /> : <RotateCw size={13} />}重新校验
        </button>
      </div>
    )
  ) : (
    <div className="chain" data-testid="audit-chain">
      <span className="chain-icon" aria-hidden><CircleAlert size={20} /></span>
      <div className="chain-text">
        <div className="chain-title">未能校验哈希链</div>
        <div className="chain-sub">读取日志数据库失败，无法确认记录是否完整。</div>
      </div>
      <button className="btn sm" onClick={() => void reload()}><RotateCw size={13} />重试</button>
    </div>
  );

  return (
    <div>
      <PageHead
        title="审计日志"
        sub="记录每次导出的时间、文件、识别到的类型与结果，只保存在本机，不含原文。每条记录与上一条哈希相连，改动或删除任一条都能被校验发现。"
        actions={
          <button className="btn" onClick={() => void onExport()} disabled={!rows.length}>
            <Download size={14} />导出 CSV
          </button>
        }
      />

      {chain}

      <div className="card">
        <div className="filters">
          <div className="search">
            <Search size={14} aria-hidden />
            <input className="input" value={q} onChange={(e) => setQ(e.target.value)} placeholder="搜索文件名、类型" aria-label="搜索记录" />
          </div>
          <select className="select-mini" value={dateF} onChange={(e) => setDateF(e.target.value as DateFilter)} aria-label="时间范围">
            <option value="all">全部时间</option>
            <option value="today">最近 24 小时</option>
            <option value="week">最近 7 天</option>
            <option value="month">最近 30 天</option>
          </select>
          <Segmented<ResultFilter>
            label="按结果筛选"
            value={resF}
            onChange={setResF}
            options={[
              { value: "all", label: "全部" },
              { value: "deep", label: `已复检${counts.deep ? ` ${counts.deep}` : ""}` },
              { value: "check", label: `需核对${counts.check ? ` ${counts.check}` : ""}` },
              { value: "release", label: `放行${counts.release ? ` ${counts.release}` : ""}` },
              { value: "blocked", label: `已阻止${counts.blocked ? ` ${counts.blocked}` : ""}` },
            ]}
          />
          <span className="grow" />
          <span className="hint">{rows.length} 条</span>
        </div>

        {audit === null ? (
          <SkeletonRows rows={4} label="正在读取审计记录" />
        ) : loadErr ? (
          <EmptyState
            tone="danger"
            icon={<CircleAlert size={22} />}
            title="读取审计记录失败"
            desc={<>请稍后重试；如一直失败，请重启应用。<br /><span className="path">{loadErr}</span></>}
            action={<button className="btn" onClick={() => void reload()}><RotateCw size={14} />重试</button>}
          />
        ) : audit.length === 0 ? (
          <EmptyState
            icon={<ScrollText size={22} />}
            title="还没有审计记录"
            desc="完成一次脱敏导出后，记录会出现在这里。"
          />
        ) : rows.length === 0 ? (
          <EmptyState
            compact
            icon={<Search size={20} />}
            title="没有符合筛选条件的记录"
            action={<button className="btn sm" onClick={() => { setQ(""); setDateF("all"); setResF("all"); }}>清除筛选</button>}
          />
        ) : (
          <>
            <div className="table-wrap">
              <table className="table">
                <thead>
                  <tr>
                    <th style={{ width: 150 }}>时间</th>
                    <th>文件</th>
                    <th>识别类型</th>
                    <th style={{ width: 92 }}>结果</th>
                  </tr>
                </thead>
                <tbody>
                  {pageRows.map((r, i) => {
                    const res = resultOf(r);
                    const meta = RESULT_META[res];
                    const sys = res === "system";
                    return (
                      <tr key={r.seq ?? i} title={r.hash ? `#${r.seq} 哈希 ${r.hash}` : undefined}>
                        <td>
                          <div className="audit-time">{r.time}</div>
                          {r.seq != null && <div className="seq">#{r.seq}</div>}
                        </td>
                        <td>
                          <div className="audit-file">{r.file}</div>
                          <div className="audit-sub">
                            {sys
                              ? `${r.kind === "prune" ? "修剪记录" : "迁移点"}：${r.note ?? ""}`
                              : `输出 ${r.output || "—"} · 映射 ${r.mapping || "—"}`}
                          </div>
                          {!sys && r.note && <div className="audit-sub" style={{ color: "var(--warn-text)" }}>{r.note}</div>}
                        </td>
                        <td>
                          {sys ? (
                            <span className="muted">—</span>
                          ) : (
                            <>
                              <div className="type-dist">
                                {parseTypes(r.types).map((t, k) => (
                                  <Badge key={k} className="square">{t.label}{t.n && ` ${t.n}`}</Badge>
                                ))}
                                {!r.types && <span className="muted">—</span>}
                              </div>
                              <div className="audit-sub">共 {r.hit_count} 处</div>
                            </>
                          )}
                        </td>
                        <td>
                          <Badge tone={meta.tone} title={meta.title}>{meta.label}</Badge>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
            {pages > 1 && (
              <nav className="pager" aria-label="分页">
                <span>第 {cur + 1} / {pages} 页</span>
                <span className="grow" />
                <button className="btn sm" disabled={cur === 0} onClick={() => setPage(cur - 1)}>上一页</button>
                <button className="btn sm" disabled={cur >= pages - 1} onClick={() => setPage(cur + 1)}>下一页</button>
              </nav>
            )}
          </>
        )}
      </div>

      <details className="card adv">
        <summary>
          <ChevronRight size={15} className="chev" aria-hidden />
          高级：保存期限与修剪
        </summary>
        <div className="adv-b">
          <div className="strat-row" style={{ padding: 0, border: 0 }}>
            <div className="sr-text">
              <div className="sr-lbl">保存期限</div>
              <div className="sr-desc">超过期限的记录可以手动修剪。修剪会留下一条记录说明删了多少条，链校验仍可通过。</div>
            </div>
            <select className="select-mini" value={retention} onChange={(e) => onRetention(Number(e.target.value))} aria-label="保存期限">
              {RETENTION_OPTIONS.some((o) => o.days === retention) ? null : <option value={retention}>{retention} 天</option>}
              {RETENTION_OPTIONS.map((o) => (
                <option key={o.days} value={o.days}>{o.label}</option>
              ))}
            </select>
          </div>
          <div>
            <button className="btn danger sm" onClick={() => void onPrune()} disabled={verify ? !verify.ok : false}>
              <Scissors size={13} />按期限修剪
            </button>
            {verify && !verify.ok && <span className="hint" style={{ marginLeft: 10 }}>哈希链校验未通过时不建议修剪，已暂时禁用。</span>}
          </div>
        </div>
      </details>
    </div>
  );
}
