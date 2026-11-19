// 主流程步骤提示：导入 → 扫描 → 确认 → 导出
import { Check } from "lucide-react";

const STEPS = ["导入文档", "扫描", "确认命中", "导出"];

export function Stepper({ current }: { current: number }) {
  return (
    <ol className="wf-steps" aria-label="处理步骤">
      {STEPS.map((s, i) => {
        const n = i + 1;
        const state = n < current ? "done" : n === current ? "active" : "todo";
        return (
          <li key={s} className={`wf-step is-${state}`} aria-current={state === "active" ? "step" : undefined}>
            <span className="wf-step-n" aria-hidden="true">{state === "done" ? <Check size={11} strokeWidth={3} /> : n}</span>
            <span className="wf-step-l">{s}</span>
          </li>
        );
      })}
    </ol>
  );
}
