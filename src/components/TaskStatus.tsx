import type { TaskEvent } from "../types";

const labels: Record<string, string> = {
  queued: "排队中",
  submitting: "提交中",
  running: "生成中",
  started: "生成中",
  progress: "生成中",
  intermediate: "生成中",
  downloading: "下载中",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
  interrupted: "已中断",
  unknown_submission: "提交未决",
};

export function TaskStatus({ task }: { task: TaskEvent | null | undefined }) {
  if (!task) return null;
  const status =
    task.eventType === "progress" || task.eventType === "intermediate"
      ? "running"
      : task.eventType;
  const progress =
    typeof task.progress === "number" && Number.isFinite(task.progress)
      ? Math.round(Math.min(1, Math.max(0, task.progress)) * 100)
      : undefined;
  return (
    <div className={`task-status status-${status}`} role="status">
      <span className="status-dot" aria-hidden="true" />
      <strong>{labels[status] ?? task.status}</strong>
      <span className="task-message">{task.message}</span>
      {progress !== undefined && status === "running" ? (
        <>
          <progress aria-label="生成进度" value={progress} max={100} />
          <span>{progress}%</span>
        </>
      ) : null}
    </div>
  );
}
