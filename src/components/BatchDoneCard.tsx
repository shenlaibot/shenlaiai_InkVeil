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
          <div className="wf-muted">
            {time} · 已导出 {batch.exported} 个文件
            {batch.exported > 0 && " · 再次导入将自动跳过"}
          </div>
        </div>
        <button className="icon-x" onClick={props.onDismiss} aria-label="关闭上一批结果">
          <X size={15} />
        </button>
      </div>
      {batch.exported > 0 && (
        <div className="wf-output-location wf-done-location">
          <span>导出位置</span>
          <bdi className="wf-output-path">{batch.outputDir}</bdi>
        </div>
      )}
      <div className="wf-done-tl" aria-label="导出结论统计">
        {red > 0 && <span className="wf-tl r"><i aria-hidden="true" />不可外发 {red}</span>}
        {yellow > 0 && <span className="wf-tl y"><i aria-hidden="true" />需核对 {yellow}</span>}
        {green > 0 && <span className="wf-tl g"><i aria-hidden="true" />通过 {green}</span>}
      </div>
      {batch.notExported.length > 0 && (
        <div className="wf-done-retry">
          <span>{batch.notExported.length} 个文件未生成副本</span>
          <button className="btn sm" onClick={props.onRetry}>
            <RotateCcw size={13} /> 重新导入
          </button>
        </div>
      )}
      <div className="wf-done-actions">
        {batch.exported > 0 && (
          <button className="btn" onClick={props.onOpenFolder}>
            <FolderOpen size={14} aria-hidden="true" /> 打开导出文件夹
          </button>
        )}
        <button className="btn" onClick={props.onViewReport}>
          <FileText size={14} /> 查看结论
        </button>
        <span style={{ flex: 1 }} />
        <button className="btn primary" onClick={props.onNewFiles}>
          <FilePlus2 size={14} /> 处理新文件
        </button>
      </div>
    </section>
  );
}
