// 一页纸合规报告（HTML，可在浏览器中打印为 PDF）。
// 只含：文件名（可选序号）、哈希、时间、版本、各类型命中计数、复检结论与范围、未处理项、放行记录。
// 绝不含任何文档原文或命中原文。
import type { ExportSnapshot, FileVerdict } from "./verdict";
import {
  KIND_NAME, LEVEL_NAME, RULE_TEXT, SCOPE_STATEMENT, residualLine, uncoveredLine,
} from "./verdict";

const esc = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

const COLOR = { red: "#dc2626", yellow: "#d97706", green: "#059669" } as const;

export function reportName(v: FileVerdict, idx: number, anonymize: boolean): string {
  return anonymize ? `文件 ${idx + 1}` : v.snap.name;
}

export function buildReportHtml(
  verdicts: FileVerdict[],
  snap: ExportSnapshot,
  opts: { anonymizeNames: boolean; sidecarVersion: string },
): string {
  const count = { green: 0, yellow: 0, red: 0 };
  verdicts.forEach((v) => count[v.level]++);
  const when = new Date(snap.time).toLocaleString("zh-CN", { hour12: false });

  const rows = verdicts
    .map((v, i) => {
      const name = reportName(v, i, opts.anonymizeNames);
      const r = v.result;
      const hits = v.snap.hitCounts.length
        ? v.snap.hitCounts.map(([k, n]) => `${esc(k)}×${n}`).join("、")
        : "—";
      const scope =
        r.verified === false
          ? "复检未能运行"
          : (r.residual ?? []).length > 0
            ? "复检发现残留，输出已删除"
            : r.error
              ? "未生成输出"
              : r.confirmed === 0
                ? "未导出"
                : "复检已运行，未发现已确认项的残留";
      return `<tr>
  <td>${i + 1}</td>
  <td>${esc(name)}</td>
  <td><span class="lv" style="background:${COLOR[v.level]}">${LEVEL_NAME[v.level]}</span> ${esc(v.headline)}</td>
  <td class="mono">${esc(r.input_sha256 ?? "—")}</td>
  <td class="mono">${esc(r.output_sha256 ?? (r.output_path ? "—" : "无输出"))}</td>
  <td>${hits}</td>
  <td>${esc(scope)}</td>
</tr>`;
    })
    .join("\n");

  const items: string[] = [];
  verdicts.forEach((v, i) => {
    const name = reportName(v, i, opts.anonymizeNames);
    for (const x of v.result.residual ?? []) items.push(`<li class="red">残留：${esc(residualLine(name, x))}</li>`);
    for (const u of v.result.uncovered ?? []) items.push(`<li class="yellow">未处理：${esc(uncoveredLine(name, u))}</li>`);
    for (const rs of v.reasons) {
      if (rs.level === "yellow" && !/未覆盖部位/.test(rs.text))
        items.push(`<li class="yellow">${esc(name)}：${esc(rs.text)}</li>`);
      if (rs.level === "red" && !/复检发现/.test(rs.text))
        items.push(`<li class="red">${esc(name)}：${esc(rs.text)}</li>`);
    }
  });

  const overrides = verdicts
    .map((v, i) => ({ v, i }))
    .filter(({ v }) => v.snap.overrideNote)
    .map(
      ({ v, i }) =>
        `<li>${esc(reportName(v, i, opts.anonymizeNames))}：带风险放行，理由“${esc(v.snap.overrideNote)}”</li>`,
    );

  return `<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<title>脱敏合规报告</title>
<style>
  body{font:13px/1.6 -apple-system,"PingFang SC","Microsoft YaHei",sans-serif;color:#111827;max-width:900px;margin:24px auto;padding:0 20px}
  h1{font-size:20px;margin-bottom:4px} h2{font-size:14px;margin:18px 0 6px;border-bottom:1px solid #e5e7eb;padding-bottom:3px}
  .meta{color:#4b5563;font-size:12px} .sum span{display:inline-block;margin-right:14px;font-weight:700}
  table{width:100%;border-collapse:collapse;font-size:12px} th,td{border:1px solid #d1d5db;padding:4px 6px;vertical-align:top;text-align:left}
  th{background:#f3f4f6} .mono{font-family:ui-monospace,Menlo,monospace;font-size:10px;word-break:break-all}
  .lv{color:#fff;border-radius:4px;padding:0 6px;font-weight:700} li.red{color:#b91c1c} li.yellow{color:#92400e}
  .scope{border:1px solid #d1d5db;background:#f9fafb;padding:8px 10px;border-radius:6px}
  @media print{body{margin:0} h2{break-after:avoid} tr{break-inside:avoid}}
</style></head><body>
<h1>脱敏合规报告</h1>
<div class="meta">处理时间：${esc(when)} · 软件版本：墨隐 InkVeil ${esc(snap.appVersion)}（sidecar ${esc(opts.sidecarVersion || "—")}） · ${esc(snap.rulesInfo)}</div>
<div class="meta">本报告不含任何文档原文或命中原文${opts.anonymizeNames ? "；文件名已用序号代替" : ""}。</div>
<h2>结论汇总</h2>
<div class="sum"><span style="color:${COLOR.green}">绿 ${count.green}</span><span style="color:${COLOR.yellow}">黄 ${count.yellow}</span><span style="color:${COLOR.red}">红 ${count.red}</span></div>
<h2>文件明细</h2>
<table><thead><tr><th>#</th><th>文件</th><th>结论</th><th>输入 SHA-256</th><th>输出 SHA-256</th><th>各类型命中（已确认）</th><th>复检</th></tr></thead>
<tbody>
${rows}
</tbody></table>
<h2>未处理项与残留</h2>
${items.length ? `<ul>${items.join("\n")}</ul>` : "<div>无。</div>"}
<h2>例外放行记录</h2>
${overrides.length ? `<ul>${overrides.join("\n")}</ul>` : "<div>无。</div>"}
<h2>复检范围声明</h2>
<div class="scope">${esc(SCOPE_STATEMENT)}</div>
<h2>判定规则</h2>
<ul>${RULE_TEXT.map((r) => `<li><b style="color:${COLOR[r.level]}">${LEVEL_NAME[r.level]}</b>：${esc(r.text)}</li>`).join("")}</ul>
<div class="meta" style="margin-top:14px">未覆盖部位类型：${Object.values(KIND_NAME).join("、")}。</div>
</body></html>`;
}
