import { useQuery } from "@tanstack/react-query";
import { api } from "../api";

interface CostEstimationHintProps {
  modality: string;
  modelId: string;
  providerId?: string;
  promptChars?: number;
  mediaCount?: number;
  durationSeconds?: number;
  compact?: boolean;
}

export function CostEstimationHint({
  modality,
  modelId,
  providerId,
  promptChars,
  mediaCount,
  durationSeconds,
  compact = false,
}: CostEstimationHintProps) {
  const query = useQuery({
    queryKey: [
      "estimate_cost",
      modality,
      modelId,
      providerId,
      promptChars,
      mediaCount,
      durationSeconds,
    ],
    queryFn: () =>
      api.analyticsEstimateCost({
        modality,
        modelId,
        providerId,
        promptChars,
        mediaCount,
        durationSeconds,
      }),
    enabled: Boolean(modelId),
    staleTime: 30_000,
  });

  if (!modelId || query.isLoading) {
    return null;
  }

  const est = query.data;
  if (!est) {
    return null;
  }

  if (!est.hasPricing) {
    return (
      <div
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: "6px",
          fontSize: compact ? "11px" : "12px",
          color: "var(--muted)",
          padding: compact ? "2px 6px" : "4px 8px",
          background: "var(--raised)",
          border: "1px dashed var(--line)",
          borderRadius: "4px",
        }}
        title="该模型未配置官方或自定义定价单价，本次生成将不会计入财务账单统计"
      >
        <span>⚠️ 暂无可靠报价 (将标记为无计量)</span>
      </div>
    );
  }

  const isRange = est.estimatedCostMin !== est.estimatedCostMax;
  const costText = isRange
    ? `$${est.estimatedCostMin.toFixed(4)} ~ $${est.estimatedCostMax.toFixed(4)}`
    : `$${est.estimatedCostMin.toFixed(4)}`;

  return (
    <div
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: "6px",
        fontSize: compact ? "11px" : "12px",
        color: "var(--ink)",
        padding: compact ? "2px 8px" : "4px 10px",
        background: "var(--bg-card)",
        border: "1px solid var(--line)",
        borderRadius: "4px",
      }}
      title={est.note}
    >
      <span style={{ color: "var(--accent)", fontWeight: 600 }}>
        💰 预估: ~{costText}
      </span>
      <span style={{ fontSize: "11px", color: "var(--muted)" }}>
        (
        {est.pricingUnit === "per_image"
          ? "按张计费"
          : est.pricingUnit === "per_video_second"
            ? "按秒计费"
            : est.pricingUnit === "per_call"
              ? "按次计费"
              : "Token 阶梯"}
        )
      </span>
      {est.isOutdated ? (
        <span
          style={{
            fontSize: "10px",
            color: "#f59e0b",
            background: "#f59e0b15",
            padding: "1px 4px",
            borderRadius: "3px",
          }}
          title="报价已生效超过30天，实际扣费以服务商账单为准"
        >
          单价超30天
        </span>
      ) : null}
    </div>
  );
}
