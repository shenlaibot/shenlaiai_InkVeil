// 工作台“上一批已完成”卡片：导出后工作区已清空，这里给出结果与下一步。
import { CircleCheck, FileText, FilePlus2, FolderOpen, RotateCcw, X } from "lucide-react";
import type { LastBatch } from "../store";

export function BatchDoneCard(props: {
  batch: LastBatch;
  onOpenFolder: () => void;
  onViewReport: () => void;
  onNewFiles: () => void;
  onRetry: () => void;
  onDismiss: () => void;
}) {
  const { batch } = props;
  const time = new Date(batch.time).toLocaleString("zh-CN", { hour12: false });
  const { green, yellow, red } = batch.counts;
  return (
    <section className="card wf-done wf-pop" aria-labelledby="wf-done-title">
      <div className="wf-done-h">
        <CircleCheck size={20} className="wf-done-ic" aria-hidden="true" />
        <div className="wf-done-tt">
          <div id="wf-done-title" className="wf-done-title">上一批已完成</div>
          <div className="wf-muted">{time} · 已导出 {batch.exported} 个脱敏副本，这些文件再次导入时会自动跳过</div>
        </div>
        <button className="icon-x" onClick={props.onDismiss} aria-label="关闭上一批结果">
          <X size={15} />
        </button>
      </div>
      <div className="wf-done-tl" aria-label="导出结论统计">
        <span className="wf-tl g"><i aria-hidden="true" />绿 {green}<em>复检通过</em></span>
        <span className="wf-tl y"><i aria-hidden="true" />黄 {yellow}<em>需人工核对</em></span>
        <span className="wf-tl r"><i aria-hidden="true" />红 {red}<em>不可外发</em></span>
      </div>
      {batch.notExported.length > 0 && (
        <div className="wf-done-retry">
          <span>
            {batch.notExported.length} 个文件未生成脱敏副本（导出失败、复检阻止或未勾选命中），未记为已处理
          </span>
          <button className="btn sm" onClick={props.onRetry}>
            <RotateCcw size={13} /> 重新导入这些文件
          </button>
        </div>
      )}
      <div className="wf-done-actions">
        <button className="btn" onClick={props.onOpenFolder}>
          <FolderOpen size={14} /> 打开输出文件夹
        </button>
        <button className="btn" onClick={props.onViewReport}>
          <FileText size={14} /> 查看结论与报告
        </button>
        <span style={{ flex: 1 }} />
        <button className="btn primary" onClick={props.onNewFiles}>
          <FilePlus2 size={14} /> 处理新文件
        </button>
      </div>
    </section>
  );
}
