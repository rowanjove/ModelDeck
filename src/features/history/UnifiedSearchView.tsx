import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../../api";
import type { UnifiedSearchItem } from "../../types";

export function UnifiedSearchView({
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
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("all");
  const [modality, setModality] = useState("all");
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const searchQuery = useQuery({
    queryKey: ["unified_search", query, category, modality],
    queryFn: () =>
      api.searchUnified(
        query.trim(),
        category === "all" ? undefined : category,
        modality === "all" ? undefined : modality,
        50,
      ),
    enabled: Boolean(query.trim()),
  });

  const items: UnifiedSearchItem[] = searchQuery.data?.items ?? [];

  const copyText = (text: string, id: string) => {
    navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const getCategoryBadge = (cat: string) => {
    switch (cat) {
      case "conversation":
        return {
          label: " 对话",
          bg: "rgba(59, 130, 246, 0.15)",
          color: "#93c5fd",
        };
      case "asset":
        return {
          label: " 媒体素材",
          bg: "rgba(168, 85, 247, 0.15)",
          color: "#c084fc",
        };
      case "task":
        return {
          label: " 生成任务",
          bg: "rgba(245, 158, 11, 0.15)",
          color: "#fcd34d",
        };
      case "model":
        return {
          label: " AI 模型",
          bg: "rgba(16, 185, 129, 0.15)",
          color: "#6ee7b7",
        };
      case "pipeline":
        return {
          label: " 工作流",
          bg: "rgba(236, 72, 153, 0.15)",
          color: "#f472b6",
        };
      default:
        return {
          label: cat,
          bg: "rgba(255, 255, 255, 0.1)",
          color: "var(--ink)",
        };
    }
  };

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        padding: "20px 24px",
        overflowY: "auto",
        background: "var(--bg)",
      }}
    >
      {/* Header bar */}
      <div style={{ marginBottom: "18px" }}>
        <p className="eyebrow" style={{ margin: 0 }}>
          History & Search
        </p>
        <h1 style={{ margin: "0 0 4px", fontSize: "18px", fontWeight: 700 }}>
          全局聚合检索
        </h1>
        <p className="muted" style={{ margin: 0, fontSize: "12.5px" }}>
          跨越对话记录、媒体素材、生成任务、模型定义与工作流，全文字段毫秒级多维搜索。
        </p>
      </div>

      {/* Main Search Input */}
      <div style={{ display: "flex", gap: "10px", marginBottom: "16px" }}>
        <input
          type="text"
          placeholder="输入搜索关键词（如：量子计算、Cyberpunk、Flux、Ollama...）"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          autoFocus
          style={{
            flex: 1,
            padding: "10px 14px",
            background: "var(--surface-1)",
            color: "var(--ink)",
            border: "1px solid var(--border)",
            borderRadius: "6px",
            fontSize: "14px",
            outline: "none",
          }}
        />
        {query && (
          <button
            type="button"
            onClick={() => setQuery("")}
            style={{
              padding: "8px 14px",
              background: "var(--surface-2)",
              color: "var(--ink)",
              border: "1px solid var(--border)",
              borderRadius: "6px",
              cursor: "pointer",
            }}
          >
            清空
          </button>
        )}
      </div>

      {/* Filters (Category + Modality) */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: "16px",
          borderBottom: "1px solid var(--border)",
          paddingBottom: "10px",
          gap: "12px",
          flexWrap: "wrap",
        }}
      >
        {/* Category Pills */}
        <div style={{ display: "flex", gap: "6px", flexWrap: "wrap" }}>
          {[
            { id: "all", label: "全部类别" },
            { id: "conversation", label: " 对话" },
            { id: "asset", label: " 素材资产" },
            { id: "model", label: " 模型" },
            { id: "pipeline", label: " 工作流" },
          ].map((cat) => (
            <button
              key={cat.id}
              type="button"
              onClick={() => setCategory(cat.id)}
              style={{
                padding: "4px 10px",
                borderRadius: "4px",
                border: "none",
                background:
                  category === cat.id
                    ? "var(--accent, #0e6b4f)"
                    : "transparent",
                color:
                  category === cat.id
                    ? "var(--accent-ink)"
                    : "var(--muted, #71717a)",
                cursor: "pointer",
                fontWeight: 600,
                fontSize: "12px",
              }}
            >
              {cat.label}
            </button>
          ))}
        </div>

        {/* Modality Pills */}
        <div style={{ display: "flex", gap: "6px" }}>
          {[
            { id: "all", label: "全部模态" },
            { id: "text", label: "文本" },
            { id: "image", label: "图像" },
            { id: "video", label: "视频" },
            { id: "audio", label: "音频" },
          ].map((mod) => (
            <button
              key={mod.id}
              type="button"
              onClick={() => setModality(mod.id)}
              style={{
                padding: "3px 8px",
                borderRadius: "4px",
                border: "1px solid var(--border, #2d3139)",
                background:
                  modality === mod.id ? "var(--sunken)" : "transparent",
                color:
                  modality === mod.id ? "var(--ink)" : "var(--muted, #8b949e)",
                cursor: "pointer",
                fontSize: "11px",
              }}
            >
              {mod.label}
            </button>
          ))}
        </div>
      </div>

      {/* Results List */}
      {!query.trim() ? (
        <div
          style={{
            flex: 1,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--muted, #8b949e)",
            padding: "48px 0",
          }}
        >
          <div style={{ fontSize: "14px" }}>输入关键词开始检索</div>
          <div style={{ fontSize: "12px", marginTop: "4px" }}>
            支持对历史对话、生成素材、任务、模型与工作流进行全文跨库检索
          </div>
        </div>
      ) : searchQuery.isFetching ? (
        <div
          style={{
            padding: "30px",
            textAlign: "center",
            color: "var(--muted, #8b949e)",
          }}
        >
          正在检索...
        </div>
      ) : items.length === 0 ? (
        <div
          style={{
            flex: 1,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--muted, #8b949e)",
            padding: "48px 0",
          }}
        >
          <div style={{ fontSize: "14px" }}>未找到与 "{query}" 匹配的结果</div>
          <div style={{ fontSize: "12px", marginTop: "4px" }}>
            请尝试更换关键词或放宽筛选条件
          </div>
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
          <div
            style={{
              fontSize: "12px",
              color: "var(--muted, #8b949e)",
              marginBottom: "4px",
            }}
          >
            共找到 {items.length} 条相关匹配结果：
          </div>
          {items.map((item) => {
            const badge = getCategoryBadge(item.category);
            return (
              <div
                key={`${item.category}-${item.id}`}
                style={{
                  background: "var(--surface-1, #161922)",
                  border: "1px solid var(--border, #2d3139)",
                  borderRadius: "8px",
                  padding: "14px 16px",
                  display: "flex",
                  flexDirection: "column",
                  gap: "6px",
                }}
              >
                <div
                  style={{
                    display: "flex",
                    justifyContent: "space-between",
                    alignItems: "center",
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: "8px",
                    }}
                  >
                    <span
                      style={{
                        padding: "2px 8px",
                        borderRadius: "4px",
                        fontSize: "11px",
                        fontWeight: 700,
                        background: badge.bg,
                        color: badge.color,
                      }}
                    >
                      {badge.label}
                    </span>
                    <strong
                      style={{ fontSize: "14px", color: "var(--fg, #e1e4ea)" }}
                    >
                      {item.title}
                    </strong>
                    {item.modality && (
                      <span
                        style={{
                          fontSize: "10px",
                          padding: "1px 5px",
                          borderRadius: "3px",
                          background: "rgba(255,255,255,0.06)",
                          color: "var(--muted, #8b949e)",
                        }}
                      >
                        {item.modality}
                      </span>
                    )}
                  </div>

                  <span
                    style={{ fontSize: "11px", color: "var(--muted, #8b949e)" }}
                  >
                    {new Date(item.createdAt).toLocaleDateString()}
                  </span>
                </div>

                <div
                  style={{
                    fontSize: "12.5px",
                    color: "var(--soft, #94a3b8)",
                    lineHeight: "1.4",
                    background: "rgba(0,0,0,0.15)",
                    padding: "8px 10px",
                    borderRadius: "4px",
                    wordBreak: "break-all",
                  }}
                >
                  {item.snippet}
                </div>

                <div
                  style={{
                    display: "flex",
                    justifyContent: "flex-end",
                    gap: "8px",
                    marginTop: "4px",
                  }}
                >
                  <button
                    type="button"
                    onClick={() => copyText(item.path || item.snippet, item.id)}
                    style={{
                      padding: "4px 10px",
                      fontSize: "11px",
                      background: "var(--surface-2, #212631)",
                      color:
                        copiedId === item.id ? "#34d399" : "var(--fg, #e1e4ea)",
                      border: "1px solid var(--border, #2d3139)",
                      borderRadius: "4px",
                      cursor: "pointer",
                    }}
                  >
                    {copiedId === item.id ? "已复制内容 " : "复制文本/路径"}
                  </button>

                  {item.category === "conversation" && (
                    <button
                      type="button"
                      onClick={onOpenText}
                      style={{
                        padding: "4px 12px",
                        fontSize: "11px",
                        background: "var(--accent, #3b82f6)",
                        color: "var(--accent-ink)",
                        border: "none",
                        borderRadius: "4px",
                        cursor: "pointer",
                        fontWeight: 600,
                      }}
                    >
                      打开对话
                    </button>
                  )}

                  {item.category === "asset" && item.modality === "image" && (
                    <button
                      type="button"
                      onClick={onOpenImage}
                      style={{
                        padding: "4px 12px",
                        fontSize: "11px",
                        background: "var(--accent, #3b82f6)",
                        color: "var(--accent-ink)",
                        border: "none",
                        borderRadius: "4px",
                        cursor: "pointer",
                        fontWeight: 600,
                      }}
                    >
                      前往图像工作台
                    </button>
                  )}
                  {item.category === "asset" && item.modality === "video" && (
                    <button
                      type="button"
                      onClick={onOpenVideo}
                      style={{
                        padding: "4px 12px",
                        fontSize: "11px",
                        background: "var(--accent, #3b82f6)",
                        color: "var(--accent-ink)",
                        border: "none",
                        borderRadius: "4px",
                        cursor: "pointer",
                        fontWeight: 600,
                      }}
                    >
                      前往视频工作台
                    </button>
                  )}
                  {item.category === "asset" && item.modality === "audio" && (
                    <button
                      type="button"
                      onClick={onOpenAudio}
                      style={{
                        padding: "4px 12px",
                        fontSize: "11px",
                        background: "var(--accent, #3b82f6)",
                        color: "var(--accent-ink)",
                        border: "none",
                        borderRadius: "4px",
                        cursor: "pointer",
                        fontWeight: 600,
                      }}
                    >
                      前往音频工作台
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
