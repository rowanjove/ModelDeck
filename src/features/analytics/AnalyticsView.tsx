import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../../api";
import type { ModelPricing } from "../../types";

type TimeRange = "today" | "7d" | "30d" | "all";

export function AnalyticsView() {
  const queryClient = useQueryClient();
  const [range, setRange] = useState<TimeRange>("7d");
  const [showPricingModal, setShowPricingModal] = useState(false);
  const [editingModelId, setEditingModelId] = useState("");
  const [inputPrice, setInputPrice] = useState("1.00");
  const [outputPrice, setOutputPrice] = useState("2.00");

  const summaryQuery = useQuery({
    queryKey: ["analytics_summary", range],
    queryFn: () => api.analyticsGetSummary(range),
  });

  const pricingsQuery = useQuery({
    queryKey: ["analytics_pricing"],
    queryFn: api.analyticsListPricing,
  });

  const savePricingMutation = useMutation({
    mutationFn: ({
      modelId,
      inputPerM,
      outputPerM,
    }: {
      modelId: string;
      inputPerM: number;
      outputPerM: number;
    }) => api.analyticsSetPricing(modelId, inputPerM, outputPerM),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["analytics_pricing"] });
      await queryClient.invalidateQueries({ queryKey: ["analytics_summary"] });
      setShowPricingModal(false);
    },
  });

  const openPricingEditor = (
    modelId: string,
    currentIn?: number,
    currentOut?: number,
  ) => {
    setEditingModelId(modelId);
    setInputPrice(currentIn !== undefined ? currentIn.toString() : "1.00");
    setOutputPrice(currentOut !== undefined ? currentOut.toString() : "2.00");
    setShowPricingModal(true);
  };

  const data = summaryQuery.data;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
        background: "var(--bg)",
      }}
    >
      {/* Top Header */}
      <header
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "16px 24px",
          borderBottom: "1px solid var(--line)",
          background: "var(--bg-card)",
          flexShrink: 0,
        }}
      >
        <div>
          <p className="eyebrow" style={{ margin: 0 }}>
            Phase 11 · Local Usage & Cost
          </p>
          <h1 style={{ margin: 0, fontSize: "18px" }}>📊 本地用量与成本分析</h1>
        </div>

        <div style={{ display: "flex", gap: "10px", alignItems: "center" }}>
          {/* Time Range Filter Buttons */}
          <div
            style={{
              display: "flex",
              background: "var(--bg)",
              border: "1px solid var(--line)",
              borderRadius: "6px",
              padding: "2px",
            }}
          >
            {(
              [
                { id: "today", label: "今天 (Today)" },
                { id: "7d", label: "近 7 天" },
                { id: "30d", label: "近 30 天" },
                { id: "all", label: "全部 (All)" },
              ] as const
            ).map((item) => (
              <button
                key={item.id}
                type="button"
                onClick={() => setRange(item.id)}
                style={{
                  padding: "5px 12px",
                  fontSize: "12px",
                  border: "none",
                  borderRadius: "4px",
                  background:
                    range === item.id ? "var(--accent)" : "transparent",
                  color: range === item.id ? "#fff" : "var(--muted)",
                  fontWeight: range === item.id ? 600 : 400,
                  cursor: "pointer",
                }}
              >
                {item.label}
              </button>
            ))}
          </div>

          <button
            type="button"
            className="btn"
            style={{ fontSize: "13px", padding: "6px 12px" }}
            onClick={() => openPricingEditor("")}
          >
            ⚙️ 模型单价配置
          </button>
        </div>
      </header>

      {/* Main Content Area */}
      <div
        style={{
          flex: 1,
          overflowY: "auto",
          padding: "20px 24px",
          display: "flex",
          flexDirection: "column",
          gap: "20px",
        }}
      >
        {/* KPI Top Cards */}
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(210px, 1fr))",
            gap: "14px",
          }}
        >
          {/* Total Cost */}
          <div
            style={{
              padding: "16px",
              background: "var(--bg-card)",
              borderRadius: "8px",
              border: "1px solid var(--line)",
            }}
          >
            <div
              style={{
                fontSize: "12px",
                color: "var(--muted)",
                marginBottom: "6px",
              }}
            >
              💰 估算总花费 (USD)
            </div>
            <div
              style={{
                fontSize: "24px",
                fontWeight: 700,
                color: "var(--accent)",
              }}
            >
              ${data ? data.estimatedCostUsd.toFixed(4) : "0.0000"}
            </div>
            <div
              style={{
                fontSize: "11px",
                color: "var(--muted)",
                marginTop: "4px",
              }}
            >
              基于输入/输出 Token 真实阶梯定价
            </div>
          </div>

          {/* Total Tokens */}
          <div
            style={{
              padding: "16px",
              background: "var(--bg-card)",
              borderRadius: "8px",
              border: "1px solid var(--line)",
            }}
          >
            <div
              style={{
                fontSize: "12px",
                color: "var(--muted)",
                marginBottom: "6px",
              }}
            >
              🔤 总 Token 消耗
            </div>
            <div style={{ fontSize: "24px", fontWeight: 700 }}>
              {data ? data.totalTokens.toLocaleString() : "0"}
            </div>
            <div
              style={{
                fontSize: "11px",
                color: "var(--muted)",
                marginTop: "4px",
              }}
            >
              Prompt: {data ? data.totalPromptTokens.toLocaleString() : 0} ·
              Completion:{" "}
              {data ? data.totalCompletionTokens.toLocaleString() : 0}
            </div>
          </div>

          {/* Total Requests */}
          <div
            style={{
              padding: "16px",
              background: "var(--bg-card)",
              borderRadius: "8px",
              border: "1px solid var(--line)",
            }}
          >
            <div
              style={{
                fontSize: "12px",
                color: "var(--muted)",
                marginBottom: "6px",
              }}
            >
              ⚡ 请求生成总数
            </div>
            <div style={{ fontSize: "24px", fontWeight: 700 }}>
              {data ? data.totalTasks.toLocaleString() : "0"}
            </div>
            <div
              style={{
                fontSize: "11px",
                color: "var(--muted)",
                marginTop: "4px",
              }}
            >
              成功:{" "}
              <span style={{ color: "#10b981" }}>
                {data?.completedTasks ?? 0}
              </span>{" "}
              · 失败:{" "}
              <span style={{ color: "#ef4444" }}>{data?.failedTasks ?? 0}</span>
            </div>
          </div>

          {/* Average Latency */}
          <div
            style={{
              padding: "16px",
              background: "var(--bg-card)",
              borderRadius: "8px",
              border: "1px solid var(--line)",
            }}
          >
            <div
              style={{
                fontSize: "12px",
                color: "var(--muted)",
                marginBottom: "6px",
              }}
            >
              ⏱️ 平均响应延迟
            </div>
            <div style={{ fontSize: "24px", fontWeight: 700 }}>
              {data && data.avgLatencyMs > 0
                ? `${data.avgLatencyMs.toLocaleString()}ms`
                : "-"}
            </div>
            <div
              style={{
                fontSize: "11px",
                color: "var(--muted)",
                marginTop: "4px",
              }}
            >
              端到端平均调用耗时
            </div>
          </div>
        </div>

        {/* Modality Breakdown */}
        <div
          style={{
            padding: "16px",
            background: "var(--bg-card)",
            borderRadius: "8px",
            border: "1px solid var(--line)",
          }}
        >
          <h3
            style={{ margin: "0 0 12px 0", fontSize: "14px", fontWeight: 600 }}
          >
            🎨 多模态调用类型分布
          </h3>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(4, 1fr)",
              gap: "10px",
            }}
          >
            {[
              { key: "text", label: "💬 文本 (Text)", icon: "💬" },
              { key: "image", label: "🖼️ 图像 (Image)", icon: "🖼️" },
              { key: "video", label: "🎬 视频 (Video)", icon: "🎬" },
              { key: "audio", label: "🎵 音频 (Audio)", icon: "🎵" },
            ].map((m) => {
              const count = data?.modalityCounts[m.key] ?? 0;
              const percent =
                data && data.totalTasks > 0
                  ? Math.round((count / data.totalTasks) * 100)
                  : 0;
              return (
                <div
                  key={m.key}
                  style={{
                    padding: "12px",
                    background: "var(--bg)",
                    borderRadius: "6px",
                    border: "1px solid var(--line)",
                  }}
                >
                  <div style={{ fontSize: "12px", color: "var(--muted)" }}>
                    {m.label}
                  </div>
                  <div
                    style={{
                      fontSize: "18px",
                      fontWeight: 700,
                      margin: "4px 0",
                    }}
                  >
                    {count} 次
                  </div>
                  <div
                    style={{
                      height: "4px",
                      width: "100%",
                      background: "var(--line)",
                      borderRadius: "2px",
                      overflow: "hidden",
                    }}
                  >
                    <div
                      style={{
                        height: "100%",
                        width: `${percent}%`,
                        background: "var(--accent)",
                      }}
                    />
                  </div>
                  <div
                    style={{
                      fontSize: "11px",
                      color: "var(--muted)",
                      marginTop: "4px",
                    }}
                  >
                    占比 {percent}%
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        {/* Two Columns: Model Ranking & Provider Ranking */}
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1.2fr 1fr",
            gap: "16px",
          }}
        >
          {/* Top Models */}
          <div
            style={{
              padding: "16px",
              background: "var(--bg-card)",
              borderRadius: "8px",
              border: "1px solid var(--line)",
              display: "flex",
              flexDirection: "column",
            }}
          >
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                marginBottom: "12px",
              }}
            >
              <h3 style={{ margin: 0, fontSize: "14px", fontWeight: 600 }}>
                🤖 模型调用与成本排行 (Top Models)
              </h3>
              <span style={{ fontSize: "12px", color: "var(--muted)" }}>
                共 {data?.topModels.length ?? 0} 个模型
              </span>
            </div>

            {data && data.topModels.length > 0 ? (
              <div style={{ overflowX: "auto" }}>
                <table
                  style={{
                    width: "100%",
                    borderCollapse: "collapse",
                    fontSize: "12px",
                    textAlign: "left",
                  }}
                >
                  <thead>
                    <tr
                      style={{
                        borderBottom: "1px solid var(--line)",
                        color: "var(--muted)",
                      }}
                    >
                      <th style={{ padding: "8px 6px" }}>模型</th>
                      <th style={{ padding: "8px 6px" }}>调用数</th>
                      <th style={{ padding: "8px 6px" }}>Tokens</th>
                      <th style={{ padding: "8px 6px" }}>平均耗时</th>
                      <th style={{ padding: "8px 6px" }}>估算费用</th>
                      <th style={{ padding: "8px 6px" }}>操作</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.topModels.map((m) => (
                      <tr
                        key={m.modelId}
                        style={{ borderBottom: "1px solid var(--line)" }}
                      >
                        <td style={{ padding: "8px 6px" }}>
                          <div style={{ fontWeight: 600 }}>{m.modelName}</div>
                          <div
                            style={{
                              fontSize: "11px",
                              color: "var(--muted)",
                              fontFamily: "monospace",
                            }}
                          >
                            {m.modelId}
                          </div>
                        </td>
                        <td style={{ padding: "8px 6px" }}>{m.taskCount}</td>
                        <td style={{ padding: "8px 6px" }}>
                          {m.totalTokens.toLocaleString()}
                        </td>
                        <td style={{ padding: "8px 6px" }}>
                          {m.avgLatencyMs > 0 ? `${m.avgLatencyMs}ms` : "-"}
                        </td>
                        <td
                          style={{
                            padding: "8px 6px",
                            fontWeight: 600,
                            color: "var(--accent)",
                          }}
                        >
                          ${m.costUsd.toFixed(4)}
                        </td>
                        <td style={{ padding: "8px 6px" }}>
                          <button
                            type="button"
                            onClick={() => openPricingEditor(m.modelId)}
                            style={{
                              background: "none",
                              border: "none",
                              color: "var(--accent)",
                              cursor: "pointer",
                              fontSize: "11px",
                              textDecoration: "underline",
                            }}
                          >
                            配置定价
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <div
                style={{
                  padding: "20px",
                  textAlign: "center",
                  color: "var(--muted)",
                  fontSize: "12px",
                }}
              >
                当前时间段内暂无模型调用数据
              </div>
            )}
          </div>

          {/* Top Providers */}
          <div
            style={{
              padding: "16px",
              background: "var(--bg-card)",
              borderRadius: "8px",
              border: "1px solid var(--line)",
              display: "flex",
              flexDirection: "column",
            }}
          >
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                marginBottom: "12px",
              }}
            >
              <h3 style={{ margin: 0, fontSize: "14px", fontWeight: 600 }}>
                🏢 Provider 渠道分布
              </h3>
              <span style={{ fontSize: "12px", color: "var(--muted)" }}>
                共 {data?.topProviders.length ?? 0} 个渠道
              </span>
            </div>

            {data && data.topProviders.length > 0 ? (
              <div style={{ overflowX: "auto" }}>
                <table
                  style={{
                    width: "100%",
                    borderCollapse: "collapse",
                    fontSize: "12px",
                    textAlign: "left",
                  }}
                >
                  <thead>
                    <tr
                      style={{
                        borderBottom: "1px solid var(--line)",
                        color: "var(--muted)",
                      }}
                    >
                      <th style={{ padding: "8px 6px" }}>渠道 Provider</th>
                      <th style={{ padding: "8px 6px" }}>任务数</th>
                      <th style={{ padding: "8px 6px" }}>Tokens</th>
                      <th style={{ padding: "8px 6px" }}>费用</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.topProviders.map((p) => (
                      <tr
                        key={p.providerId}
                        style={{ borderBottom: "1px solid var(--line)" }}
                      >
                        <td style={{ padding: "8px 6px", fontWeight: 600 }}>
                          {p.providerName}
                        </td>
                        <td style={{ padding: "8px 6px" }}>{p.taskCount}</td>
                        <td style={{ padding: "8px 6px" }}>
                          {p.totalTokens.toLocaleString()}
                        </td>
                        <td
                          style={{
                            padding: "8px 6px",
                            fontWeight: 600,
                            color: "var(--accent)",
                          }}
                        >
                          ${p.costUsd.toFixed(4)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <div
                style={{
                  padding: "20px",
                  textAlign: "center",
                  color: "var(--muted)",
                  fontSize: "12px",
                }}
              >
                暂无渠道调用记录
              </div>
            )}
          </div>
        </div>

        {/* Daily Stat Table */}
        <div
          style={{
            padding: "16px",
            background: "var(--bg-card)",
            borderRadius: "8px",
            border: "1px solid var(--line)",
          }}
        >
          <h3
            style={{ margin: "0 0 12px 0", fontSize: "14px", fontWeight: 600 }}
          >
            📅 每日用量趋势 (Daily Activity)
          </h3>
          {data && data.recentDays.length > 0 ? (
            <div style={{ overflowX: "auto" }}>
              <table
                style={{
                  width: "100%",
                  borderCollapse: "collapse",
                  fontSize: "12px",
                  textAlign: "left",
                }}
              >
                <thead>
                  <tr
                    style={{
                      borderBottom: "1px solid var(--line)",
                      color: "var(--muted)",
                    }}
                  >
                    <th style={{ padding: "8px" }}>日期 (Date)</th>
                    <th style={{ padding: "8px" }}>请求任务数</th>
                    <th style={{ padding: "8px" }}>总消耗 Tokens</th>
                    <th style={{ padding: "8px" }}>估算花费 (USD)</th>
                  </tr>
                </thead>
                <tbody>
                  {data.recentDays.map((d) => (
                    <tr
                      key={d.date}
                      style={{ borderBottom: "1px solid var(--line)" }}
                    >
                      <td style={{ padding: "8px", fontWeight: 600 }}>
                        {d.date}
                      </td>
                      <td style={{ padding: "8px" }}>{d.taskCount} 次</td>
                      <td style={{ padding: "8px" }}>
                        {d.totalTokens.toLocaleString()}
                      </td>
                      <td
                        style={{
                          padding: "8px",
                          fontWeight: 600,
                          color: "var(--accent)",
                        }}
                      >
                        ${d.costUsd.toFixed(4)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <div
              style={{
                padding: "16px",
                textAlign: "center",
                color: "var(--muted)",
                fontSize: "12px",
              }}
            >
              当前时间范围内无历史记录
            </div>
          )}
        </div>
      </div>

      {/* Pricing Modal */}
      {showPricingModal && (
        <div
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.6)",
            backdropFilter: "blur(4px)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 1000,
          }}
          onClick={(e) => {
            if (e.target === e.currentTarget) setShowPricingModal(false);
          }}
        >
          <div
            style={{
              width: "480px",
              background: "var(--bg-card)",
              borderRadius: "10px",
              border: "1px solid var(--line)",
              padding: "20px",
              display: "flex",
              flexDirection: "column",
              gap: "14px",
            }}
          >
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
              }}
            >
              <h3 style={{ margin: 0, fontSize: "15px" }}>
                ⚙️ 自定义模型定价 (Pricing)
              </h3>
              <button
                type="button"
                onClick={() => setShowPricingModal(false)}
                style={{
                  background: "none",
                  border: "none",
                  cursor: "pointer",
                  color: "var(--muted)",
                }}
              >
                ✕
              </button>
            </div>

            <div
              style={{ display: "flex", flexDirection: "column", gap: "6px" }}
            >
              <label style={{ fontSize: "12px", color: "var(--muted)" }}>
                模型标识符 (Model ID / Model API ID):
              </label>
              <input
                type="text"
                value={editingModelId}
                onChange={(e) => setEditingModelId(e.target.value)}
                placeholder="例如: gpt-4o, claude-3-5-sonnet, deepseek-chat"
                style={{
                  padding: "8px",
                  fontSize: "13px",
                  borderRadius: "6px",
                  border: "1px solid var(--line)",
                  background: "var(--bg)",
                }}
              />
            </div>

            <div
              style={{
                display: "grid",
                gridTemplateColumns: "1fr 1fr",
                gap: "10px",
              }}
            >
              <div
                style={{ display: "flex", flexDirection: "column", gap: "4px" }}
              >
                <label style={{ fontSize: "12px", color: "var(--muted)" }}>
                  输入价格 ($ / 1M Tokens):
                </label>
                <input
                  type="number"
                  step="0.01"
                  value={inputPrice}
                  onChange={(e) => setInputPrice(e.target.value)}
                  style={{
                    padding: "8px",
                    fontSize: "13px",
                    borderRadius: "6px",
                    border: "1px solid var(--line)",
                    background: "var(--bg)",
                  }}
                />
              </div>

              <div
                style={{ display: "flex", flexDirection: "column", gap: "4px" }}
              >
                <label style={{ fontSize: "12px", color: "var(--muted)" }}>
                  输出价格 ($ / 1M Tokens):
                </label>
                <input
                  type="number"
                  step="0.01"
                  value={outputPrice}
                  onChange={(e) => setOutputPrice(e.target.value)}
                  style={{
                    padding: "8px",
                    fontSize: "13px",
                    borderRadius: "6px",
                    border: "1px solid var(--line)",
                    background: "var(--bg)",
                  }}
                />
              </div>
            </div>

            {/* List Existing Custom Pricing */}
            {pricingsQuery.data && pricingsQuery.data.length > 0 && (
              <div style={{ marginTop: "6px" }}>
                <div
                  style={{
                    fontSize: "11px",
                    color: "var(--muted)",
                    marginBottom: "4px",
                  }}
                >
                  已配置的自定义单价:
                </div>
                <div
                  style={{
                    maxHeight: "120px",
                    overflowY: "auto",
                    display: "flex",
                    flexDirection: "column",
                    gap: "4px",
                  }}
                >
                  {pricingsQuery.data.map((pr: ModelPricing) => (
                    <div
                      key={pr.modelId}
                      onClick={() =>
                        openPricingEditor(
                          pr.modelId,
                          pr.inputPerM,
                          pr.outputPerM,
                        )
                      }
                      style={{
                        padding: "4px 8px",
                        borderRadius: "4px",
                        background: "var(--bg)",
                        border: "1px solid var(--line)",
                        fontSize: "11px",
                        display: "flex",
                        justifyContent: "space-between",
                        cursor: "pointer",
                      }}
                    >
                      <span style={{ fontWeight: 600 }}>{pr.modelId}</span>
                      <span>
                        In: ${pr.inputPerM}/M · Out: ${pr.outputPerM}/M
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            <div
              style={{
                display: "flex",
                justifyContent: "flex-end",
                gap: "8px",
                marginTop: "10px",
              }}
            >
              <button
                type="button"
                className="btn"
                onClick={() => setShowPricingModal(false)}
              >
                取消
              </button>
              <button
                type="button"
                className="btn"
                style={{
                  background: "var(--accent)",
                  color: "var(--accent-ink)",
                  fontWeight: 600,
                }}
                disabled={
                  !editingModelId.trim() || savePricingMutation.isPending
                }
                onClick={() => {
                  savePricingMutation.mutate({
                    modelId: editingModelId.trim(),
                    inputPerM: Number.parseFloat(inputPrice) || 0,
                    outputPerM: Number.parseFloat(outputPrice) || 0,
                  });
                }}
              >
                {savePricingMutation.isPending ? "保存中..." : "保存定价"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
