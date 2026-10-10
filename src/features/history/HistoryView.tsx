import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, asAppError } from "../../api";
import { AudioThumb } from "../../components/AudioThumb";
import { ErrorNote } from "../../components/ErrorNote";
import { ImageThumb } from "../../components/ImageThumb";
import { VideoThumb } from "../../components/VideoThumb";
import { toCurl } from "../../lib/curl";
import { fileName } from "../../lib/schema";
import { type ImageReferenceDraft, useRestore } from "../../store";
import type { HistoryDetail, RawHttp } from "../../types";
import { MODALITY_LABELS } from "../../types";
import { AssetLibraryView } from "./AssetLibraryView";
import { UnifiedSearchView } from "./UnifiedSearchView";

const STATUSES = [
  { id: "", label: "全部状态" },
  { id: "queued", label: "排队" },
  { id: "submitting", label: "提交中" },
  { id: "running", label: "进行中" },
  { id: "downloading", label: "下载中" },
  { id: "completed", label: "完成" },
  { id: "failed", label: "失败" },
  { id: "cancelled", label: "已取消" },
  { id: "interrupted", label: "已中断" },
  { id: "unknown_submission", label: "提交未决" },
];

const STATUS_LABELS: Record<string, string> = {
  queued: "排队",
  submitting: "提交中",
  running: "进行中",
  downloading: "下载中",
  completed: "完成",
  failed: "失败",
  cancelled: "已取消",
  interrupted: "已中断",
  unknown_submission: "提交未决",
};

export function HistoryView({
  onOpenText,
  onOpenImage,
  onOpenVideo,
  onOpenAudio,
}: {
  onOpenText: () => void;
  onOpenImage: () => void;
  onOpenVideo: () => void;
  onOpenAudio: () => void;
}) {
  const [modality, setModality] = useState("");
  const [providerId, setProviderId] = useState("");
  const [modelId, setModelId] = useState("");
  const [status, setStatus] = useState("");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [offset, setOffset] = useState(0);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<
    "tasks" | "assets" | "search" | "analytics"
  >("tasks");
  const [pricingModelId, setPricingModelId] = useState("");
  const [pricingInputRate, setPricingInputRate] = useState("2.5");
  const [pricingOutputRate, setPricingOutputRate] = useState("10.0");
  const [priceNotice, setPriceNotice] = useState<string | null>(null);

  const queryClient = useQueryClient();
  const analytics = useQuery({
    queryKey: ["analytics"],
    queryFn: () => api.analyticsGetSummary(),
    enabled: viewMode === "analytics",
  });
  const pricings = useQuery({
    queryKey: ["analytics-pricing"],
    queryFn: api.analyticsListPricing,
    enabled: viewMode === "analytics",
  });
  const setPricingMutation = useMutation({
    mutationFn: (args: {
      modelId: string;
      inputPerM: number;
      outputPerM: number;
    }) =>
      api.analyticsSetPricing(args.modelId, args.inputPerM, args.outputPerM),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["analytics-pricing"] });
      await queryClient.invalidateQueries({ queryKey: ["analytics"] });
      setPriceNotice("价格已更新");
      setPricingModelId("");
    },
  });

  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: api.providers,
  });
  const models = useQuery({ queryKey: ["models"], queryFn: api.models });
  const filters = {
    modality: modality || null,
    providerId: providerId || null,
    modelId: modelId || null,
    status: status || null,
    dateFrom: dateFrom || null,
    dateTo: dateTo || null,
    query: searchQuery.trim() || null,
    limit: 30,
    offset,
  };
  const page = useQuery({
    queryKey: ["history", filters],
    queryFn: () => api.historyList(filters),
  });
  const detail = useQuery({
    queryKey: ["history", "detail", selectedId],
    queryFn: () => api.historyGet(selectedId ?? ""),
    enabled: Boolean(selectedId),
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.historyDelete(id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["history"] });
      setSelectedId(null);
    },
  });

  const deleteAllMutation = useMutation({
    mutationFn: () => api.historyDeleteAll(modality || undefined),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["history"] });
      setSelectedId(null);
    },
  });

  useEffect(() => {
    if (page.data && page.data.total > 0 && offset >= page.data.total) {
      setOffset(0);
    }
  }, [page.data, offset]);

  useEffect(() => {
    const items = page.data?.items ?? [];
    if (!page.data) {
      return;
    }
    if (items.length === 0) {
      if (selectedId) {
        setSelectedId(null);
      }
      return;
    }
    if (!selectedId || !items.some((item) => item.id === selectedId)) {
      setSelectedId(items[0].id);
    }
  }, [page.data, selectedId]);

  const modelChoices = (models.data ?? []).filter(
    (model) => !providerId || model.providerId === providerId,
  );
  const filtering = Boolean(
    modality ||
      providerId ||
      modelId ||
      status ||
      dateFrom ||
      dateTo ||
      searchQuery.trim(),
  );
  const total = page.data?.total ?? 0;
  const shownFrom = total === 0 ? 0 : offset + 1;
  const shownTo = Math.min(offset + (page.data?.items.length ?? 0), total);

  function resetOffset<T>(set: (value: T) => void) {
    return (value: T) => {
      set(value);
      setOffset(0);
    };
  }

  const summary = analytics.data;
  const pricingList = pricings.data ?? [];

  const renderAnalytics = () => (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        overflowY: "auto",
        padding: "20px 24px",
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: "20px",
        }}
      >
        <div>
          <h2 style={{ margin: "0 0 4px", fontSize: "16px", fontWeight: 700 }}>
            模型用量与成本统计 (Token Usage & Cost)
          </h2>
          <p className="muted" style={{ margin: 0, fontSize: "12.5px" }}>
            基于全模态任务历史的 Token
            消耗与成本换算，配置自定义模型费率可实时重新估算。
          </p>
        </div>
      </div>

      {/* 1. KPI Cards */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))",
          gap: "16px",
          marginBottom: "28px",
        }}
      >
        <div
          style={{
            background: "var(--raised)",
            border: "1px solid var(--line)",
            borderRadius: "8px",
            padding: "16px",
          }}
        >
          <span className="muted" style={{ fontSize: "12px" }}>
            总生成任务
          </span>
          <div
            style={{
              fontSize: "24px",
              fontWeight: "bold",
              marginTop: "4px",
            }}
          >
            {summary?.totalTasks ?? 0}
          </div>
          <span className="muted" style={{ fontSize: "12px" }}>
            {summary?.completedTasks ?? 0} 成功 / {summary?.failedTasks ?? 0}{" "}
            失败
          </span>
        </div>

        <div
          style={{
            background: "var(--raised)",
            border: "1px solid var(--line)",
            borderRadius: "8px",
            padding: "16px",
          }}
        >
          <span className="muted" style={{ fontSize: "12px" }}>
            估算总成本 (USD)
          </span>
          <div
            style={{
              fontSize: "24px",
              fontWeight: "bold",
              marginTop: "4px",
              color: "var(--accent)",
            }}
          >
            ${(summary?.estimatedCostUsd ?? 0).toFixed(4)}
          </div>
          <span className="muted" style={{ fontSize: "12px" }}>
            根据自定义与默认模型费率换算
          </span>
        </div>

        <div
          style={{
            background: "var(--raised)",
            border: "1px solid var(--line)",
            borderRadius: "8px",
            padding: "16px",
          }}
        >
          <span className="muted" style={{ fontSize: "12px" }}>
            总消耗 Tokens
          </span>
          <div
            style={{
              fontSize: "24px",
              fontWeight: "bold",
              marginTop: "4px",
            }}
          >
            {(summary?.totalTokens ?? 0).toLocaleString()}
          </div>
          <span className="muted" style={{ fontSize: "12px" }}>
            全模态任务综合统计
          </span>
        </div>

        <div
          style={{
            background: "var(--raised)",
            border: "1px solid var(--line)",
            borderRadius: "8px",
            padding: "16px",
          }}
        >
          <span className="muted" style={{ fontSize: "12px" }}>
            输入 / Prompt Tokens
          </span>
          <div
            style={{
              fontSize: "24px",
              fontWeight: "bold",
              marginTop: "4px",
            }}
          >
            {(summary?.totalPromptTokens ?? 0).toLocaleString()}
          </div>
          <span className="muted" style={{ fontSize: "12px" }}>
            占总 Token 的{" "}
            {summary?.totalTokens
              ? Math.round(
                  (summary.totalPromptTokens / summary.totalTokens) * 100,
                )
              : 0}
            %
          </span>
        </div>

        <div
          style={{
            background: "var(--raised)",
            border: "1px solid var(--line)",
            borderRadius: "8px",
            padding: "16px",
          }}
        >
          <span className="muted" style={{ fontSize: "12px" }}>
            输出 / Completion Tokens
          </span>
          <div
            style={{
              fontSize: "24px",
              fontWeight: "bold",
              marginTop: "4px",
            }}
          >
            {(summary?.totalCompletionTokens ?? 0).toLocaleString()}
          </div>
          <span className="muted" style={{ fontSize: "12px" }}>
            占总 Token 的{" "}
            {summary?.totalTokens
              ? Math.round(
                  (summary.totalCompletionTokens / summary.totalTokens) * 100,
                )
              : 0}
            %
          </span>
        </div>
      </div>

      {/* 2. Top Models Table */}
      <div
        style={{
          background: "var(--raised)",
          border: "1px solid var(--line)",
          borderRadius: "8px",
          padding: "20px",
          marginBottom: "28px",
        }}
      >
        <h2 style={{ margin: "0 0 14px", fontSize: "16px" }}>
          高频模型消耗排行 (Top Models)
        </h2>
        {(summary?.topModels ?? []).length === 0 ? (
          <p className="muted" style={{ margin: 0 }}>
            暂无模型生成记录。
          </p>
        ) : (
          <table
            style={{
              width: "100%",
              borderCollapse: "collapse",
              fontSize: "13.5px",
            }}
          >
            <thead>
              <tr
                style={{
                  textAlign: "left",
                  borderBottom: "1px solid var(--line)",
                }}
              >
                <th style={{ padding: "8px 6px" }}>模型</th>
                <th style={{ padding: "8px 6px" }}>任务数</th>
                <th style={{ padding: "8px 6px" }}>总消耗 Tokens</th>
                <th style={{ padding: "8px 6px" }}>估算花费 (USD)</th>
              </tr>
            </thead>
            <tbody>
              {(summary?.topModels ?? []).map((m) => (
                <tr
                  key={m.modelId}
                  style={{ borderBottom: "1px solid var(--line)" }}
                >
                  <td style={{ padding: "10px 6px" }}>
                    <strong>{m.modelName}</strong>
                    <span
                      className="muted"
                      style={{ marginLeft: "8px", fontSize: "12px" }}
                    >
                      {m.modelId}
                    </span>
                  </td>
                  <td style={{ padding: "10px 6px" }}>{m.taskCount}</td>
                  <td style={{ padding: "10px 6px" }}>
                    {m.totalTokens.toLocaleString()}
                  </td>
                  <td
                    style={{
                      padding: "10px 6px",
                      fontWeight: 600,
                      color: "var(--accent)",
                    }}
                  >
                    ${m.costUsd.toFixed(4)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* 3. Custom Pricing Override Manager */}
      <div
        style={{
          background: "var(--raised)",
          border: "1px solid var(--line)",
          borderRadius: "8px",
          padding: "20px",
        }}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: "14px",
          }}
        >
          <div>
            <h2 style={{ margin: "0 0 4px", fontSize: "16px" }}>
              自定义模型费率设置 (Pricing Configuration)
            </h2>
            <p className="muted" style={{ margin: 0, fontSize: "12.5px" }}>
              配置或覆盖指定模型的每百万 Token 价格 ($ / 1M
              tokens)，统计将实时应用新费率。
            </p>
          </div>
          {priceNotice && (
            <span style={{ color: "var(--ok)", fontSize: "13px" }}>
              {priceNotice}
            </span>
          )}
        </div>

        {pricingList.length > 0 && (
          <div style={{ marginBottom: "16px" }}>
            <table
              style={{
                width: "100%",
                borderCollapse: "collapse",
                fontSize: "13px",
              }}
            >
              <thead>
                <tr
                  style={{
                    textAlign: "left",
                    borderBottom: "1px solid var(--line)",
                  }}
                >
                  <th style={{ padding: "6px" }}>模型标识</th>
                  <th style={{ padding: "6px" }}>输入价格 (每 1M Tokens)</th>
                  <th style={{ padding: "6px" }}>输出价格 (每 1M Tokens)</th>
                </tr>
              </thead>
              <tbody>
                {pricingList.map((p) => (
                  <tr
                    key={p.modelId}
                    style={{ borderBottom: "1px solid var(--line)" }}
                  >
                    <td style={{ padding: "8px 6px", fontWeight: 600 }}>
                      {p.modelId}
                    </td>
                    <td style={{ padding: "8px 6px" }}>${p.inputPerM} / 1M</td>
                    <td style={{ padding: "8px 6px" }}>${p.outputPerM} / 1M</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 140px 140px auto",
            gap: "10px",
            alignItems: "flex-end",
          }}
        >
          <label>
            <span style={{ fontSize: "12px" }}>
              模型 ID (如 gpt-4o, claude-3-7-sonnet)
            </span>
            <input
              placeholder="例如 gpt-4o"
              value={pricingModelId}
              onChange={(e) => setPricingModelId(e.target.value)}
            />
          </label>
          <label>
            <span style={{ fontSize: "12px" }}>输入 $/1M tokens</span>
            <input
              type="number"
              step="0.01"
              value={pricingInputRate}
              onChange={(e) => setPricingInputRate(e.target.value)}
            />
          </label>
          <label>
            <span style={{ fontSize: "12px" }}>输出 $/1M tokens</span>
            <input
              type="number"
              step="0.01"
              value={pricingOutputRate}
              onChange={(e) => setPricingOutputRate(e.target.value)}
            />
          </label>
          <button
            type="button"
            className="primary"
            disabled={!pricingModelId.trim() || setPricingMutation.isPending}
            onClick={() => {
              setPriceNotice(null);
              void setPricingMutation.mutateAsync({
                modelId: pricingModelId.trim(),
                inputPerM: Number.parseFloat(pricingInputRate) || 0,
                outputPerM: Number.parseFloat(pricingOutputRate) || 0,
              });
            }}
          >
            保存费率
          </button>
        </div>
      </div>
    </div>
  );

  const renderTasks = () => (
    <section className="history" style={{ height: "100%" }}>
      <div className="history-side">
        <header
          className="history-head"
          style={{
            display: "flex",
            flexDirection: "column",
            gap: "10px",
            padding: "14px 16px 10px",
          }}
        >
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
            }}
          >
            <span
              style={{
                fontSize: "13px",
                fontWeight: 600,
                color: "var(--ink)",
              }}
            >
              任务流水列表 ({total})
            </span>
            <span className="muted" style={{ fontSize: "11.5px" }}>
              {shownFrom}–{shownTo} / 共 {total} 条
            </span>
          </div>
          <div>
            <input
              type="text"
              placeholder="搜索 Prompt / 结果 / 模型..."
              value={searchQuery}
              onChange={(e) => {
                setSearchQuery(e.target.value);
                setOffset(0);
              }}
              style={{
                width: "100%",
                boxSizing: "border-box",
                padding: "7px 10px",
                background: "var(--surface-1)",
                color: "var(--ink)",
                border: "1px solid var(--border)",
                borderRadius: "4px",
                fontSize: "12px",
                outline: "none",
              }}
            />
          </div>
        </header>
        <div className="history-filters">
          <label>
            模态
            <select
              value={modality}
              onChange={(event) => resetOffset(setModality)(event.target.value)}
            >
              <option value="">全部</option>
              {Object.entries(MODALITY_LABELS).map(([id, label]) => (
                <option key={id} value={id}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label>
            状态
            <select
              value={status}
              onChange={(event) => resetOffset(setStatus)(event.target.value)}
            >
              {STATUSES.map((item) => (
                <option key={item.id || "all"} value={item.id}>
                  {item.label}
                </option>
              ))}
            </select>
          </label>
          <details className="history-advanced">
            <summary>
              更多筛选
              {providerId || modelId || dateFrom || dateTo ? " · 已启用" : ""}
            </summary>
            <div className="history-filter-grid">
              <label>
                Provider
                <select
                  value={providerId}
                  onChange={(event) => {
                    setProviderId(event.target.value);
                    setModelId("");
                    setOffset(0);
                  }}
                >
                  <option value="">全部</option>
                  {(providers.data ?? []).map((provider) => (
                    <option key={provider.id} value={provider.id}>
                      {provider.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                模型
                <select
                  value={modelId}
                  onChange={(event) =>
                    resetOffset(setModelId)(event.target.value)
                  }
                >
                  <option value="">全部</option>
                  {modelChoices.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                开始日期
                <input
                  type="date"
                  value={dateFrom}
                  onChange={(event) =>
                    resetOffset(setDateFrom)(event.target.value)
                  }
                />
              </label>
              <label>
                结束日期
                <input
                  type="date"
                  value={dateTo}
                  onChange={(event) =>
                    resetOffset(setDateTo)(event.target.value)
                  }
                />
              </label>
            </div>
          </details>
        </div>
        {page.isLoading ? <p className="page-message">正在读取历史…</p> : null}
        {page.error ? <ErrorNote error={asAppError(page.error)} /> : null}
        {page.data && page.data.items.length === 0 ? (
          <p className="page-message">
            {filtering ? "没有符合条件的记录。" : "还没有生成记录。"}
          </p>
        ) : null}
        <ul>
          {(page.data?.items ?? []).map((item) => {
            const modalityBadge =
              item.modality === "text"
                ? "TXT"
                : item.modality === "image"
                  ? "IMG"
                  : item.modality === "video"
                    ? "VID"
                    : item.modality === "audio"
                      ? "AUD"
                      : "AI";
            const statusDotColor =
              item.status === "completed"
                ? "var(--ok)"
                : item.status === "failed"
                  ? "var(--danger)"
                  : item.status === "queued" || item.status === "running"
                    ? "var(--warn)"
                    : "var(--muted)";
            return (
              <li key={item.id}>
                <button
                  type="button"
                  aria-current={item.id === selectedId ? "true" : undefined}
                  onClick={() => setSelectedId(item.id)}
                  style={{ gap: "4px", padding: "7px 10px" }}
                >
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: "8px",
                      width: "100%",
                    }}
                  >
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: "6px",
                        minWidth: 0,
                        flex: 1,
                      }}
                    >
                      <span
                        style={{
                          fontSize: "10px",
                          fontFamily: "var(--mono)",
                          fontWeight: 700,
                          padding: "0 4px",
                          borderRadius: "var(--radius-xs)",
                          background: "var(--surface-3)",
                          color: "var(--ink-secondary)",
                          flexShrink: 0,
                        }}
                      >
                        {modalityBadge}
                      </span>
                      <strong
                        style={{
                          fontSize: "12.5px",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          fontWeight: 600,
                        }}
                      >
                        {item.promptPreview || "（无提示词）"}
                      </strong>
                    </div>
                    <small
                      style={{
                        fontSize: "11px",
                        color: "var(--muted)",
                        flexShrink: 0,
                      }}
                    >
                      {stamp(item.createdAt)}
                    </small>
                  </div>
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: "6px",
                      fontSize: "11.5px",
                      color: "var(--soft)",
                    }}
                  >
                    <span
                      style={{
                        width: "6px",
                        height: "6px",
                        borderRadius: "50%",
                        background: statusDotColor,
                        display: "inline-block",
                        flexShrink: 0,
                      }}
                    />
                    <span className={statusClass(item.status)}>
                      {STATUS_LABELS[item.status] ?? item.status}
                    </span>
                    <span>·</span>
                    <span
                      style={{
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {item.providerName} / {item.modelName}
                    </span>
                  </div>
                </button>
              </li>
            );
          })}
        </ul>
        {page.data && total > 0 ? (
          <div className="pager">
            <button
              type="button"
              disabled={offset === 0}
              onClick={() => setOffset(Math.max(0, offset - 30))}
            >
              上一页
            </button>
            <span>
              {shownFrom}–{shownTo} / {total}
            </span>
            <button
              type="button"
              disabled={offset + 30 >= total}
              onClick={() => setOffset(offset + 30)}
            >
              下一页
            </button>
          </div>
        ) : null}
      </div>
      <HistoryDetailPanel
        deleting={deleteMutation.isPending}
        deleteError={deleteMutation.error ?? deleteAllMutation.error}
        detail={selectedId ? detail.data : undefined}
        loading={Boolean(selectedId) && detail.isLoading}
        error={selectedId ? detail.error : null}
        onOpenText={onOpenText}
        onOpenImage={onOpenImage}
        onOpenVideo={onOpenVideo}
        onOpenAudio={onOpenAudio}
        onDelete={(id) => {
          if (window.confirm("确定要删除这条历史记录及关联文件吗？")) {
            deleteMutation.mutate(id);
          }
        }}
      />
    </section>
  );

  return (
    <div
      className="history-workspace"
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
      }}
    >
      {/* 统一顶部子导航栏 */}
      <header
        className="settings-header"
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          flexShrink: 0,
          flexWrap: "wrap",
          gap: "12px",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "16px",
            flexWrap: "wrap",
          }}
        >
          <div>
            <p className="eyebrow" style={{ margin: 0 }}>
              History & Library
            </p>
            <h1 style={{ margin: 0, fontSize: "18px" }}>历史与素材库</h1>
          </div>

          <div className="mode-switch" style={{ margin: 0 }}>
            <button
              type="button"
              aria-pressed={viewMode === "tasks"}
              onClick={() => setViewMode("tasks")}
            >
              任务历史
            </button>
            <button
              type="button"
              aria-pressed={viewMode === "assets"}
              onClick={() => setViewMode("assets")}
            >
              媒体素材库
            </button>
            <button
              type="button"
              aria-pressed={viewMode === "search"}
              onClick={() => setViewMode("search")}
            >
              全局聚合检索
            </button>
            <button
              type="button"
              aria-pressed={viewMode === "analytics"}
              onClick={() => setViewMode("analytics")}
            >
              成本统计
            </button>
          </div>
        </div>

        {/* 右侧上下文快捷操作 */}
        {viewMode === "tasks" && (
          <div className="inline-actions" style={{ margin: 0 }}>
            <button
              type="button"
              className="linkish danger"
              disabled={deleteAllMutation.isPending || total === 0}
              onClick={() => {
                if (
                  window.confirm(
                    `确定要清空全部${modality ? `【${MODALITY_LABELS[modality as keyof typeof MODALITY_LABELS] || modality}】` : ""}历史记录吗？关联文件也将被清理。`,
                  )
                ) {
                  deleteAllMutation.mutate();
                }
              }}
              style={{ fontSize: "12px", padding: "4px 10px" }}
            >
              清空任务历史
            </button>
          </div>
        )}
        {viewMode === "analytics" && (
          <div className="inline-actions" style={{ margin: 0 }}>
            <button
              type="button"
              className="primary"
              onClick={() => void analytics.refetch()}
              disabled={analytics.isFetching}
              style={{ fontSize: "12px", padding: "4px 12px" }}
            >
              {analytics.isFetching ? "刷新中…" : "刷新数据"}
            </button>
          </div>
        )}
      </header>

      {/* 主体视图区域 */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflow: "hidden",
          display: "flex",
          flexDirection: "column",
        }}
      >
        {viewMode === "tasks" && renderTasks()}
        {viewMode === "assets" && (
          <div style={{ flex: 1, minHeight: 0 }}>
            <AssetLibraryView
              onOpenImage={onOpenImage}
              onOpenVideo={onOpenVideo}
              onOpenAudio={onOpenAudio}
            />
          </div>
        )}
        {viewMode === "search" && (
          <div style={{ flex: 1, minHeight: 0 }}>
            <UnifiedSearchView
              onOpenText={onOpenText}
              onOpenImage={onOpenImage}
              onOpenVideo={onOpenVideo}
              onOpenAudio={onOpenAudio}
            />
          </div>
        )}
        {viewMode === "analytics" && renderAnalytics()}
      </div>
    </div>
  );
}

function HistoryDetailPanel({
  deleting,
  deleteError,
  detail,
  loading,
  error,
  onOpenText,
  onOpenImage,
  onOpenVideo,
  onOpenAudio,
  onDelete,
}: {
  deleting: boolean;
  deleteError: unknown;
  detail: HistoryDetail | undefined;
  loading: boolean;
  error: unknown;
  onOpenText: () => void;
  onOpenImage: () => void;
  onOpenVideo: () => void;
  onOpenAudio: () => void;
  onDelete: (id: string) => void;
}) {
  if (!detail && loading) {
    return <p className="page-message">正在读取记录…</p>;
  }
  if (error) {
    return <ErrorNote error={asAppError(error)} />;
  }
  if (!detail) {
    return <p className="page-message">选择一条记录查看详情。</p>;
  }
  const parameters = Object.keys(detail.parameters);
  return (
    <article className="history-detail">
      {deleteError ? <ErrorNote error={asAppError(deleteError)} /> : null}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          flexWrap: "wrap",
          borderBottom: "1px solid var(--line)",
          paddingBottom: "10px",
          gap: "12px",
        }}
      >
        <div style={{ minWidth: 0, flex: 1 }}>
          <p className="eyebrow" style={{ margin: 0 }}>
            {detail.kind === "chat"
              ? "Chat"
              : detail.kind === "image"
                ? "图像"
                : detail.kind === "video"
                  ? "视频"
                  : detail.kind === "audio"
                    ? "音频"
                    : "Generate"}
          </p>
          <h1
            style={{
              margin: 0,
              fontSize: "15px",
              fontWeight: 650,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {detail.prompt.split("\n")[0] || "记录"}
          </h1>
        </div>
        <div style={{ display: "flex", gap: "8px", flexShrink: 0 }}>
          <button
            type="button"
            className="primary"
            onClick={() =>
              runAgain(
                detail,
                onOpenText,
                onOpenImage,
                onOpenVideo,
                onOpenAudio,
              )
            }
          >
            再次生成
          </button>
          <button
            type="button"
            className="danger"
            disabled={deleting}
            onClick={() => onDelete(detail.id)}
          >
            {deleting ? "删除中…" : "删除"}
          </button>
        </div>
      </div>
      <dl className="meta">
        <div>
          <dt>状态</dt>
          <dd className={statusClass(detail.status)}>
            {STATUS_LABELS[detail.status] ?? detail.status}
          </dd>
        </div>
        <div>
          <dt>Provider</dt>
          <dd>{detail.providerName}</dd>
        </div>
        <div>
          <dt>模型</dt>
          <dd>{detail.modelName}</dd>
        </div>
        <div>
          <dt>时间</dt>
          <dd>{stamp(detail.createdAt)}</dd>
        </div>
        <div>
          <dt>任务</dt>
          <dd>{detail.id}</dd>
        </div>
      </dl>
      <CopyActions detail={detail} />
      {detail.error ? (
        <ErrorNote error={{ code: "ProviderError", message: detail.error }} />
      ) : null}
      <section>
        <h2>提示词</h2>
        <pre>{detail.prompt || "无"}</pre>
      </section>
      {detail.systemPrompt ? (
        <section>
          <h2>系统提示</h2>
          <pre>{detail.systemPrompt}</pre>
        </section>
      ) : null}
      <section>
        <h2>输入</h2>
        <InputSummary detail={detail} />
      </section>
      <section>
        <h2>参数</h2>
        {parameters.length === 0 ? (
          <p>无</p>
        ) : (
          <pre>{JSON.stringify(detail.parameters, null, 2)}</pre>
        )}
      </section>
      <section>
        <h2>输出</h2>
        <OutputSummary detail={detail} />
      </section>
      <details>
        <summary>原始请求</summary>
        <pre>{pretty(detail.rawRequest)}</pre>
      </details>
      <details>
        <summary>原始响应</summary>
        <pre>{pretty(detail.rawResponse)}</pre>
      </details>
    </article>
  );
}

function InputSummary({ detail }: { detail: HistoryDetail }) {
  if (detail.kind === "image") {
    const references = detail.referenceImages ?? [];
    const empty =
      !detail.sourceImage && references.length === 0 && !detail.mask;
    if (empty) {
      return <p>无</p>;
    }
    return (
      <ul className="file-list">
        {detail.sourceImage ? (
          <li>原图 {fileName(detail.sourceImage.path)}</li>
        ) : null}
        {references.map((image) => (
          <li key={image.path}>
            参考图 {fileName(image.path)}
            {image.role ? ` · ${image.role}` : ""}
          </li>
        ))}
        {detail.mask ? <li>蒙版 {fileName(detail.mask.path)}</li> : null}
      </ul>
    );
  }
  if (detail.kind === "video") {
    const references = detail.referenceImages ?? [];
    const clips = detail.referenceVideos ?? [];
    const sounds = detail.referenceAudios ?? [];
    const empty =
      !detail.firstFrame &&
      !detail.lastFrame &&
      references.length === 0 &&
      clips.length === 0 &&
      sounds.length === 0;
    if (empty) {
      return <p>无</p>;
    }
    return (
      <ul className="file-list">
        {detail.firstFrame ? (
          <li>首帧 {fileName(detail.firstFrame.path)}</li>
        ) : null}
        {detail.lastFrame ? (
          <li>尾帧 {fileName(detail.lastFrame.path)}</li>
        ) : null}
        {references.map((image) => (
          <li key={image.path}>
            参考图 {fileName(image.path)}
            {image.role ? ` · ${image.role}` : ""}
          </li>
        ))}
        {clips.map((item) => (
          <li key={item.path}>参考视频 {fileName(item.path)}</li>
        ))}
        {sounds.map((item) => (
          <li key={item.path}>参考音频 {fileName(item.path)}</li>
        ))}
      </ul>
    );
  }
  if (detail.kind === "audio") {
    const empty =
      !detail.voice &&
      !detail.referenceAudio &&
      !detail.referenceTranscript &&
      !detail.sourceAudio &&
      !detail.audioTask;
    if (empty) {
      return <p>无</p>;
    }
    return (
      <ul className="file-list">
        {detail.audioTask ? (
          <li>任务 {audioTaskLabel(detail.audioTask)}</li>
        ) : null}
        {detail.voice ? <li>声音 {detail.voice}</li> : null}
        {detail.sourceAudio ? (
          <li>源音频 {fileName(detail.sourceAudio.path)}</li>
        ) : null}
        {detail.referenceAudio ? (
          <li>参考音频 {fileName(detail.referenceAudio.path)}</li>
        ) : null}
        {detail.referenceTranscript ? (
          <li>参考文本 {detail.referenceTranscript}</li>
        ) : null}
      </ul>
    );
  }
  if (detail.images.length === 0) {
    return <p>无</p>;
  }
  return (
    <ul className="file-list">
      {detail.images.map((image) => (
        <li key={image.path}>{fileName(image.path)}</li>
      ))}
    </ul>
  );
}

function OutputSummary({ detail }: { detail: HistoryDetail }) {
  const images = detail.assets.filter(
    (asset) =>
      asset.type === "image" && asset.metadata.role === "output" && asset.path,
  );
  if (detail.modality === "image") {
    if (images.length === 0) {
      return <p>没有图像输出</p>;
    }
    return (
      <div className="history-gallery">
        {images.map((asset) => (
          <figure key={asset.id}>
            <ImageThumb path={asset.path ?? ""} alt="历史图像" />
          </figure>
        ))}
      </div>
    );
  }
  if (detail.modality === "video") {
    const videos = detail.assets.filter(
      (asset) =>
        asset.type === "video" &&
        asset.metadata.role === "output" &&
        asset.path,
    );
    if (videos.length === 0) {
      return <p>没有视频输出</p>;
    }
    return (
      <div className="history-gallery">
        {videos.map((asset) => (
          <figure key={asset.id}>
            <VideoThumb path={asset.path ?? ""} />
            <figcaption>{fileName(asset.path ?? "")}</figcaption>
          </figure>
        ))}
      </div>
    );
  }
  if (detail.modality === "audio") {
    const audios = detail.assets.filter(
      (asset) =>
        asset.type === "audio" &&
        asset.metadata.role === "output" &&
        asset.path,
    );
    if (audios.length === 0) {
      return (
        <pre className="history-output">{detail.output || "没有音频输出"}</pre>
      );
    }
    return (
      <div className="history-gallery">
        {detail.output ? (
          <pre className="history-output">{detail.output}</pre>
        ) : null}
        {audios.map((asset) => (
          <figure key={asset.id}>
            <AudioThumb path={asset.path ?? ""} />
            <figcaption>{fileName(asset.path ?? "")}</figcaption>
          </figure>
        ))}
      </div>
    );
  }
  return (
    <pre className="history-output">{detail.output || "没有文本输出"}</pre>
  );
}

function runAgain(
  detail: HistoryDetail,
  onOpenText: () => void,
  onOpenImage: () => void,
  onOpenVideo: () => void,
  onOpenAudio: () => void,
) {
  const parameters =
    detail.parameters && typeof detail.parameters === "object"
      ? detail.parameters
      : {};
  const extraText =
    detail.extra && typeof detail.extra === "object"
      ? JSON.stringify(detail.extra, null, 2)
      : "";
  if (detail.modality === "image" || detail.kind === "image") {
    const references: ImageReferenceDraft[] = (
      detail.referenceImages ?? []
    ).map((image) => ({
      path: image.path,
      role: image.role ?? "",
      weight: image.weight == null ? "" : String(image.weight),
      description: image.description ?? "",
    }));
    useRestore.getState().publish({
      kind: "image",
      modelDefinitionId: detail.modelId,
      systemPrompt: "",
      prompt: detail.prompt,
      parameters,
      extraText,
      images: [],
      conversationId: null,
      negativePrompt: detail.negativePrompt ?? "",
      sourcePath: detail.sourceImage?.path ?? null,
      references,
      maskPath: detail.mask?.path ?? null,
    });
    onOpenImage();
    return;
  }
  if (detail.modality === "video" || detail.kind === "video") {
    const references: ImageReferenceDraft[] = (
      detail.referenceImages ?? []
    ).map((image) => ({
      path: image.path,
      role: image.role ?? "",
      weight: image.weight == null ? "" : String(image.weight),
      description: image.description ?? "",
    }));
    useRestore.getState().publish({
      kind: "video",
      modelDefinitionId: detail.modelId,
      systemPrompt: "",
      prompt: detail.prompt,
      parameters,
      extraText,
      images: [],
      conversationId: null,
      negativePrompt: detail.negativePrompt ?? "",
      references,
      firstPath: detail.firstFrame?.path ?? null,
      lastPath: detail.lastFrame?.path ?? null,
      videoPaths: (detail.referenceVideos ?? []).map((item) => item.path),
      audioPaths: (detail.referenceAudios ?? []).map((item) => item.path),
    });
    onOpenVideo();
    return;
  }
  if (detail.modality === "audio" || detail.kind === "audio") {
    useRestore.getState().publish({
      kind: "audio",
      modelDefinitionId: detail.modelId,
      systemPrompt: "",
      prompt: detail.prompt,
      parameters,
      extraText,
      images: [],
      conversationId: null,
      voiceName: detail.voice ?? "",
      task: audioTaskOf(detail.audioTask),
      sourcePath: detail.sourceAudio?.path ?? null,
      referencePath: detail.referenceAudio?.path ?? null,
      transcript: detail.referenceTranscript ?? "",
    });
    onOpenAudio();
    return;
  }
  useRestore.getState().publish({
    kind: detail.kind === "chat" ? "chat" : "generate",
    modelDefinitionId: detail.modelId,
    systemPrompt: detail.systemPrompt ?? "",
    prompt: detail.prompt,
    parameters,
    extraText,
    images: detail.images.map((image) => image.path),
    conversationId: detail.conversationId ?? null,
  });
  onOpenText();
}

function audioTaskLabel(task: string): string {
  if (task === "stt") {
    return "语音转文本";
  }
  if (task === "sts") {
    return "语音转语音";
  }
  if (task === "clone") {
    return "声音克隆";
  }
  return "文本转语音";
}

function audioTaskOf(task: string | null | undefined) {
  if (task === "stt" || task === "sts" || task === "clone" || task === "tts") {
    return task;
  }
  return "tts" as const;
}

function CopyActions({ detail }: { detail: HistoryDetail }) {
  const [notice, setNotice] = useState<string | null>(null);
  const raw = asRaw(detail.rawRequest);
  async function copy(text: string, message: string) {
    await navigator.clipboard.writeText(text);
    setNotice(message);
  }
  return (
    <div className="actions">
      <button
        type="button"
        onClick={() =>
          void copy(JSON.stringify(detail.rawRequest, null, 2), "已复制 JSON")
        }
      >
        复制 JSON
      </button>
      {raw ? (
        <button
          type="button"
          onClick={() => void copy(toCurl(raw), "已复制 cURL（密钥已脱敏）")}
        >
          复制 cURL
        </button>
      ) : null}
      {notice ? <p className="muted">{notice}</p> : null}
    </div>
  );
}

function asRaw(value: unknown): RawHttp | null {
  if (!value || typeof value !== "object") {
    return null;
  }
  const raw = value as RawHttp;
  if (
    typeof raw.method !== "string" ||
    typeof raw.url !== "string" ||
    !raw.headers ||
    typeof raw.headers !== "object"
  ) {
    return null;
  }
  return raw;
}

function pretty(value: unknown): string {
  if (value == null) {
    return "无";
  }
  return JSON.stringify(value, null, 2);
}

function stamp(value: string): string {
  return value.replace("T", " ").replace(/Z$/, "").slice(0, 19);
}

function statusClass(status: string): string | undefined {
  if (status === "failed") {
    return "status-failed";
  }
  if (status === "completed") {
    return "status-completed";
  }
  return undefined;
}
