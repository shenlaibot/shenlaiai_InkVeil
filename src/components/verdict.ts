// 导出结论的判定逻辑（纯函数，无界面依赖）。
// 判定规则同时写在页面“如何判定”折叠说明里（RULE_TEXT），两处必须保持一致。
import type { RedactFileResult, ScannedFile, UncoveredItem } from "../types";

export type Level = "green" | "yellow" | "red";

/** 表示“检测不完整/无法确认”的扫描提醒（检测通道失效 → 红） */
export const RED_WARNING = /无法确认|检测规则失效|未能提取到文字|NER 不可用|NER 请求失败|解析失败/;

/** 低于此置信度的命中视为“低置信度”；未勾选的低置信度命中算“未审阅” */
export const LOW_CONF = 0.7;

/** 导出时刻的快照（之后用户再改勾选，也不影响本次结论与报告） */
export interface FileSnapshot {
  path: string;
  name: string;
  /** 各类型已确认的命中数（只含类型名与计数，不含原文） */
  hitCounts: [string, number][];
  /** 未勾选的低置信度命中数 */
  lowConfUnreviewed: number;
  /** 扫描阶段的检测通道失效提醒（匹配 RED_WARNING 的） */
  channelWarnings: string[];
  /** 带风险放行的理由（无则空） */
  overrideNote: string;
}

export interface ExportSnapshot {
  time: number; // 毫秒时间戳
  files: FileSnapshot[];
  appVersion: string;
  rulesInfo: string;
}

export interface Reason {
  level: "red" | "yellow";
  text: string;
  /** uncovered：界面用分组清单代替这句概述；报告与复制清单仍保留 */
  kind?: "uncovered";
}

export interface FileVerdict {
  level: Level;
  /** 一句话结论 */
  headline: string;
  reasons: Reason[];
  snap: FileSnapshot;
  result: RedactFileResult;
}

export const LEVEL_ORDER: Record<Level, number> = { red: 0, yellow: 1, green: 2 };
export const LEVEL_NAME: Record<Level, string> = { red: "红", yellow: "黄", green: "绿" };
/** 界面上的状态词（比“红/黄/绿”直接） */
export const LEVEL_LABEL: Record<Level, string> = { red: "不可外发", yellow: "需核对", green: "通过" };

/** 未覆盖部位的用户提示文案（不得出现“未发现敏感信息/安全”这类暗示） */
export function uncoveredText(u: UncoveredItem): string {
  switch (u.kind) {
    case "scanned_page":
      return "未检测，请人工核对或涂黑";
    case "image":
      return "其中文字未检测，请人工核对或涂黑";
    case "embedded":
      return "内容未检测也未处理，请人工核对";
    case "pivot":
      return "未处理，可能仍含原文，请人工核对";
    case "external_link":
      return "未处理，可能仍含原文，请人工核对";
    case "custom_xml":
      return "未处理，可能仍含原文，请人工核对";
    default:
      return "未处理，请人工核对";
  }
}

export const KIND_NAME: Record<string, string> = {
  scanned_page: "扫描页",
  image: "图片",
  embedded: "嵌入对象",
  pivot: "透视表",
  external_link: "外部链接缓存",
  custom_xml: "自定义 XML",
  other: "其他",
};

export function judgeFile(fr: RedactFileResult, snap: FileSnapshot): FileVerdict {
  const reasons: Reason[] = [];
  const residual = fr.residual ?? [];
  const uncovered = fr.uncovered ?? [];

  if (residual.length > 0) {
    const n = residual.reduce((a, r) => a + r.count, 0);
    reasons.push({ level: "red", text: `复检发现 ${n} 处原文残留，输出已删除` });
  } else if (fr.error) {
    reasons.push({ level: "red", text: "导出失败，未生成副本" });
  }
  if (snap.channelWarnings.length > 0) {
    reasons.push({
      level: "red",
      text: `无法确认检测完整（${snap.channelWarnings.length} 条提醒）${
        snap.overrideNote ? `，已按“${snap.overrideNote}”放行` : ""
      }`,
    });
  }

  if (!fr.error) {
    if (fr.confirmed === 0 && !fr.output_path) {
      reasons.push({ level: "yellow", text: "未勾选任何命中，未导出" });
    } else {
      if (uncovered.length > 0) {
        reasons.push({ level: "yellow", kind: "uncovered", text: `${uncovered.length} 处内容未检测或未处理` });
      }
      if (fr.verified === false) {
        reasons.push({ level: "yellow", text: "复检未能运行，无法确认没有残留" });
      }
      if (snap.lowConfUnreviewed > 0) {
        reasons.push({ level: "yellow", text: `${snap.lowConfUnreviewed} 处低置信度命中未勾选，仍为原文` });
      }
      if (!fr.is_deep && uncovered.length === 0 && fr.verified !== false) {
        reasons.push({ level: "yellow", text: "部分内容未精确处理，请人工核对" });
      }
    }
  }

  const level: Level = reasons.some((r) => r.level === "red")
    ? "red"
    : reasons.length > 0
      ? "yellow"
      : "green";
  const headline =
    level === "red"
      ? residual.length > 0 || fr.error
        ? "不可外发"
        : "检测不完整"
      : level === "yellow"
        ? "需核对"
        : "复检通过";
  return { level, headline, reasons, snap, result: fr };
}

export function judgeAll(
  files: RedactFileResult[],
  snaps: Map<string, FileSnapshot>,
): FileVerdict[] {
  return files.map((fr) => {
    const snap =
      snaps.get(fr.input_path) ??
      ({
        path: fr.input_path,
        name: fr.input_path.split(/[\\/]/).pop() ?? fr.input_path,
        hitCounts: [],
        lowConfUnreviewed: 0,
        channelWarnings: [],
        overrideNote: "",
      } satisfies FileSnapshot);
    return judgeFile(fr, snap);
  });
}

/** 在导出时刻为一个扫描文件做快照 */
export function snapshotFile(
  f: ScannedFile,
  confirmed: Record<string, boolean>,
  overrideNote: string,
): FileSnapshot {
  const byType = new Map<string, number>();
  for (const h of f.hits) {
    if (confirmed[h.id]) byType.set(h.rule_label, (byType.get(h.rule_label) ?? 0) + 1);
  }
  const channelWarnings = f.warnings.filter((w) => RED_WARNING.test(w));
  return {
    path: f.path,
    name: f.name,
    hitCounts: [...byType.entries()],
    lowConfUnreviewed: f.hits.filter((h) => !confirmed[h.id] && h.confidence < LOW_CONF).length,
    channelWarnings,
    overrideNote: channelWarnings.length > 0 ? overrideNote : "",
  };
}

/** 清单项文本：哪个文件、哪个部位、类型、字数、出现次数。绝不含原文。 */
export function residualLine(file: string, r: { where: string; rule_label: string; length: number; count: number }): string {
  return `${file} · ${r.where} · ${r.rule_label || "已确认项"} · ${r.length} 字 · ${r.count} 处`;
}

export function uncoveredLine(file: string, u: UncoveredItem): string {
  return `${file} · ${u.where} · ${KIND_NAME[u.kind] ?? "其他"}${u.count > 1 ? ` × ${u.count}` : ""} · ${uncoveredText(u)}`;
}

/** 结论页的分组展示：同类未覆盖部位合并成一行 */
export interface UncoveredGroup {
  kind: string;
  name: string;
  count: number;
  where: string[];
  advice: string;
}

export function groupUncovered(list: UncoveredItem[]): UncoveredGroup[] {
  const map = new Map<string, UncoveredGroup>();
  for (const u of list) {
    let g = map.get(u.kind);
    if (!g) {
      g = { kind: u.kind, name: KIND_NAME[u.kind] ?? "其他", count: 0, where: [], advice: uncoveredText(u) };
      map.set(u.kind, g);
    }
    g.count += u.count;
    if (u.where && !g.where.includes(u.where)) g.where.push(u.where);
  }
  return [...map.values()];
}

/** 单个文件的可复制清单（残留 + 未覆盖 + 其他原因） */
export function listText(v: FileVerdict, displayName: string): string {
  const lines: string[] = [`【${LEVEL_NAME[v.level]}】${displayName}：${v.headline}`];
  for (const r of v.reasons) lines.push(`- ${r.text}`);
  for (const r of v.result.residual ?? []) lines.push(`  残留：${residualLine(displayName, r)}`);
  for (const u of v.result.uncovered ?? []) lines.push(`  未覆盖：${uncoveredLine(displayName, u)}`);
  return lines.join("\n");
}

/** “如何判定”说明（页面折叠区与报告共用） */
export const RULE_TEXT: { level: Level; text: string }[] = [
  {
    level: "green",
    text: "复检通过且没有未覆盖部位：已在输出文件的全部可提取内容（正文、批注、页眉页脚、元数据等）中反查已确认的原文，未发现残留；文件内也没有图片、扫描页、嵌入对象、透视表等本工具无法检测的部位；没有被忽略的低置信度命中。",
  },
  {
    level: "yellow",
    text: "存在以下任一情况：有未覆盖部位（图片、扫描页、嵌入对象、透视表、外部链接缓存、自定义 XML 等，其中的内容未被检测或处理）；残留复检未能运行；有低置信度（低于 70%）命中未勾选；引擎提示存在未精确处理的部位；本文件未勾选任何命中。外发前须人工核对。",
  },
  {
    level: "red",
    text: "复检发现已确认的原文仍在输出中（此时输出文件已自动删除，不会交付）；或检测通道失效/无法确认已完整检测（如规则失效、NER 请求失败、无可提取文字）；或导出失败。红色文件不提供“一键放行”，须回到预览修正后重新导出；检测通道失效的文件只能在导出前带理由放行并记入审计。",
  },
];

/** 结论页的精简判定说明（完整表述见 RULE_TEXT，用于报告；两处含义必须一致） */
export const RULE_SHORT: Record<Level, string> = {
  green: "复检无残留，没有未覆盖部位，低置信度命中已审阅。",
  yellow: "有未覆盖部位（图片、扫描页、嵌入对象等）、复检未运行、低置信度命中未勾选、部分内容未精确处理，或未勾选任何命中。",
  red: "复检发现残留（输出已删除）、无法确认检测完整，或导出失败。",
};

export const SCOPE_SHORT = "复检只验证已确认的内容，不代表没有漏检；图片内文字不在检测范围。";

export const SCOPE_STATEMENT =
  "复检只证明“已确认的已知原文”在输出的可提取内容中没有残留，不证明没有漏检；图片、扫描件内的文字不在检测与复检范围。";
