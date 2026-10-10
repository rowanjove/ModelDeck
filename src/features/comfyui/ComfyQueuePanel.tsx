import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, asAppError } from "../../api";
import type { AppError, ComfyQueueStatus } from "../../types";

interface ComfyQueuePanelProps {
  providerId: string;
  providerName?: string;
  compact?: boolean;
}

export function ComfyQueuePanel({
  providerId,
  providerName,
  compact,
}: ComfyQueuePanelProps) {
  const [error, setError] = useState<AppError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [autoRefresh, setAutoRefresh] = useState(true);

  const {
    data: queue,
    isLoading,
    refetch,
  } = useQuery<ComfyQueueStatus>({
    queryKey: ["comfyuiQueue", providerId],
    queryFn: () => api.comfyuiGetQueue(providerId),
    refetchInterval: autoRefresh ? 3000 : false,
    enabled: Boolean(providerId),
  });

  const interruptMutation = useMutation({
    mutationFn: () => api.comfyuiInterrupt(providerId),
    onSuccess: async () => {
      setNotice("已发送中断信号");
      setError(null);
      await refetch();
    },
    onError: (err) => setError(asAppError(err)),
  });

  const clearMutation = useMutation({
    mutationFn: () => api.comfyuiClearQueue(providerId),
    onSuccess: async () => {
      setNotice("已清空等待队列");
      setError(null);
      await refetch();
    },
    onError: (err) => setError(asAppError(err)),
  });

  useEffect(() => {
    if (notice) {
      const timer = setTimeout(() => setNotice(null), 3000);
      return () => clearTimeout(timer);
    }
  }, [notice]);

  if (!providerId) return null;

  if (compact) {
    return (
      <div
        className="comfy-queue-compact"
        style={{
          display: "flex",
          alignItems: "center",
          gap: "8px",
          fontSize: "12px",
          background: "var(--bg-secondary, #2a2a2a)",
          padding: "4px 8px",
          borderRadius: "4px",
          border: "1px solid var(--border-color, #444)",
        }}
      >
        <span style={{ fontWeight: 600 }}>ComfyUI 队列:</span>
        <span>
          运行中:{" "}
          <strong
            style={{
              color: (queue?.running.length ?? 0) > 0 ? "#4caf50" : "inherit",
            }}
          >
            {queue?.running.length ?? 0}
          </strong>
        </span>
        <span>
          排队: <strong>{queue?.pending.length ?? 0}</strong>
        </span>
        <button
          type="button"
          className="button tiny"
          style={{ padding: "2px 6px", fontSize: "11px" }}
          onClick={() => void refetch()}
          title="刷新队列"
        >
          🔄
        </button>
        {(queue?.running.length ?? 0) > 0 && (
          <button
            type="button"
            className="button tiny danger"
            style={{ padding: "2px 6px", fontSize: "11px" }}
            onClick={() => void interruptMutation.mutate()}
            disabled={interruptMutation.isPending}
            title="中断当前运行的任务"
          >
            ⏹ 中断
          </button>
        )}
      </div>
    );
  }

  return (
    <div
      className="comfy-queue-panel card"
      style={{
        marginTop: "1rem",
        padding: "1rem",
        border: "1px solid var(--border-color, #444)",
        borderRadius: "8px",
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: "0.75rem",
        }}
      >
        <h4
          style={{
            margin: 0,
            display: "flex",
            alignItems: "center",
            gap: "6px",
          }}
        >
          <span>⚡ ComfyUI 队列状态</span>
          {providerName ? (
            <small
              style={{
                fontWeight: "normal",
                color: "var(--text-muted, #888)",
              }}
            >
              ({providerName})
            </small>
          ) : null}
        </h4>
        <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
          <label
            style={{
              fontSize: "12px",
              display: "flex",
              alignItems: "center",
              gap: "4px",
              cursor: "pointer",
            }}
          >
            <input
              type="checkbox"
              checked={autoRefresh}
              onChange={(e) => setAutoRefresh(e.target.checked)}
            />
            自动刷新 (3s)
          </label>
          <button
            type="button"
            className="button small"
            onClick={() => void refetch()}
            disabled={isLoading}
          >
            {isLoading ? "刷新中…" : "刷新"}
          </button>
        </div>
      </div>

      {notice ? (
        <p className="ok" style={{ margin: "4px 0 8px 0" }}>
          {notice}
        </p>
      ) : null}
      {error ? (
        <p className="error" style={{ margin: "4px 0 8px 0" }}>
          {error.message}
        </p>
      ) : null}

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          gap: "12px",
          marginBottom: "1rem",
        }}
      >
        <div
          style={{
            padding: "8px 12px",
            background: "var(--bg-secondary, #222)",
            borderRadius: "6px",
          }}
        >
          <div style={{ fontSize: "12px", color: "var(--text-muted, #888)" }}>
            当前运行中 (Running)
          </div>
          <div
            style={{
              fontSize: "20px",
              fontWeight: "bold",
              color: (queue?.running.length ?? 0) > 0 ? "#4caf50" : "inherit",
            }}
          >
            {queue?.running.length ?? 0}
          </div>
          {queue?.running && queue.running.length > 0 ? (
            <ul
              style={{
                margin: "6px 0 0 0",
                paddingLeft: "16px",
                fontSize: "11px",
                fontFamily: "monospace",
              }}
            >
              {queue.running.map((item) => (
                <li key={item.promptId}>
                  #{item.number} {item.promptId.slice(0, 8)}...
                </li>
              ))}
            </ul>
          ) : (
            <div
              style={{
                fontSize: "11px",
                color: "var(--text-muted, #888)",
                marginTop: "4px",
              }}
            >
              无执行中的任务
            </div>
          )}
        </div>

        <div
          style={{
            padding: "8px 12px",
            background: "var(--bg-secondary, #222)",
            borderRadius: "6px",
          }}
        >
          <div style={{ fontSize: "12px", color: "var(--text-muted, #888)" }}>
            等待排队 (Pending)
          </div>
          <div style={{ fontSize: "20px", fontWeight: "bold" }}>
            {queue?.pending.length ?? 0}
          </div>
          {queue?.pending && queue.pending.length > 0 ? (
            <ul
              style={{
                margin: "6px 0 0 0",
                paddingLeft: "16px",
                fontSize: "11px",
                fontFamily: "monospace",
              }}
            >
              {queue.pending.map((item) => (
                <li key={item.promptId}>
                  #{item.number} {item.promptId.slice(0, 8)}...
                </li>
              ))}
            </ul>
          ) : (
            <div
              style={{
                fontSize: "11px",
                color: "var(--text-muted, #888)",
                marginTop: "4px",
              }}
            >
              队列空闲
            </div>
          )}
        </div>
      </div>

      <div style={{ display: "flex", gap: "8px" }}>
        <button
          type="button"
          className="button danger small"
          onClick={() => void interruptMutation.mutate()}
          disabled={
            interruptMutation.isPending || (queue?.running.length ?? 0) === 0
          }
        >
          {interruptMutation.isPending ? "中断中…" : "中断当前任务 (Interrupt)"}
        </button>
        <button
          type="button"
          className="button secondary small"
          onClick={() => void clearMutation.mutate()}
          disabled={
            clearMutation.isPending || (queue?.pending.length ?? 0) === 0
          }
        >
          {clearMutation.isPending ? "清空中…" : "清空排队队列 (Clear)"}
        </button>
      </div>
    </div>
  );
}
