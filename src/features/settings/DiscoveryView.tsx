import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, asAppError } from "../../api";
import { ErrorNote } from "../../components/ErrorNote";
import type { AppError, DiscoveredService } from "../../types";

type FilterTab = "all" | "online" | "offline";

export function DiscoveryView() {
  const queryClient = useQueryClient();
  const [filter, setFilter] = useState<FilterTab>("all");
  const [error, setError] = useState<AppError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const discovery = useQuery({
    queryKey: ["discovery"],
    queryFn: api.discoveryScanLocal,
  });

  const addServiceMutation = useMutation({
    mutationFn: (serviceId: string) => api.discoveryAddService(serviceId),
    onSuccess: async (created) => {
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      await queryClient.invalidateQueries({ queryKey: ["models"] });
      await discovery.refetch();
      setNotice(`成功添加 ${created.name} 并同步注册可用模型！`);
      setError(null);
    },
    onError: (err) => {
      setError(asAppError(err));
    },
  });

  const services: DiscoveredService[] = discovery.data ?? [];
  const onlineCount = services.filter((s) => s.online).length;
  const offlineCount = services.length - onlineCount;

  const filteredServices = services.filter((s) => {
    if (filter === "online") return s.online;
    if (filter === "offline") return !s.online;
    return true;
  });

  return (
    <div style={{ maxWidth: "900px", paddingBottom: "32px" }}>
      {/* Title & Control Bar */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "flex-start",
          marginBottom: "16px",
          gap: "16px",
        }}
      >
        <div>
          <h2 style={{ margin: "0 0 6px", fontSize: "17px", fontWeight: 700 }}>
            本地 AI 服务管理器 (Local Service Manager)
          </h2>
          <p
            className="muted"
            style={{ margin: 0, fontSize: "13px", lineHeight: "1.5" }}
          >
            实时探测本机 7 大主流 AI 服务（Ollama、LM
            Studio、llama.cpp、vLLM、ComfyUI、F5-TTS、CosyVoice），
            支持在线心跳、版本提取、已下载模型自动提取、一键打开服务网页及一键注册为
            Provider。
          </p>
        </div>
        <div style={{ display: "flex", gap: "8px", flexShrink: 0 }}>
          <button
            type="button"
            className="primary"
            onClick={() => {
              setNotice(null);
              setError(null);
              void discovery.refetch();
            }}
            disabled={discovery.isFetching}
            style={{ fontSize: "13px", padding: "8px 14px" }}
          >
            {discovery.isFetching ? "正在探测端口…" : " 重新扫描服务"}
          </button>
        </div>
      </div>

      {/* Filter Tabs */}
      <div
        style={{
          display: "flex",
          gap: "6px",
          marginBottom: "16px",
          borderBottom: "1px solid var(--line, #2d3139)",
          paddingBottom: "8px",
        }}
      >
        <button
          type="button"
          onClick={() => setFilter("all")}
          style={{
            padding: "4px 10px",
            borderRadius: "4px",
            border: "none",
            background:
              filter === "all" ? "var(--accent, #3b82f6)" : "transparent",
            color: filter === "all" ? "#fff" : "var(--muted, #8b949e)",
            cursor: "pointer",
            fontWeight: 600,
            fontSize: "12px",
          }}
        >
          全部服务 ({services.length})
        </button>
        <button
          type="button"
          onClick={() => setFilter("online")}
          style={{
            padding: "4px 10px",
            borderRadius: "4px",
            border: "none",
            background:
              filter === "online" ? "var(--ok, #22c55e)" : "transparent",
            color: filter === "online" ? "#fff" : "var(--muted, #8b949e)",
            cursor: "pointer",
            fontWeight: 600,
            fontSize: "12px",
          }}
        >
          ● 在线运行 ({onlineCount})
        </button>
        <button
          type="button"
          onClick={() => setFilter("offline")}
          style={{
            padding: "4px 10px",
            borderRadius: "4px",
            border: "none",
            background:
              filter === "offline" ? "rgba(148, 163, 184, 0.2)" : "transparent",
            color:
              filter === "offline"
                ? "var(--fg, #e1e4ea)"
                : "var(--muted, #8b949e)",
            cursor: "pointer",
            fontWeight: 600,
            fontSize: "12px",
          }}
        >
          ○ 离线未启动 ({offlineCount})
        </button>
      </div>

      {notice && (
        <div
          style={{
            padding: "10px 14px",
            background: "rgba(34, 197, 94, 0.12)",
            border: "1px solid var(--ok, #22c55e)",
            borderRadius: "6px",
            color: "var(--ok, #22c55e)",
            marginBottom: "16px",
            fontSize: "13px",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <span> {notice}</span>
          <button
            onClick={() => setNotice(null)}
            style={{
              background: "none",
              border: "none",
              color: "inherit",
              cursor: "pointer",
            }}
          >
            关闭
          </button>
        </div>
      )}

      {error && <ErrorNote error={error} />}

      {/* Services Grid */}
      <div style={{ display: "grid", gap: "12px" }}>
        {filteredServices.map((service) => (
          <div
            key={service.id}
            style={{
              background: "var(--raised, #161922)",
              border: service.online
                ? "1px solid rgba(34, 197, 94, 0.35)"
                : "1px solid var(--line, #2d3139)",
              borderRadius: "8px",
              padding: "16px",
              display: "flex",
              flexDirection: "column",
              gap: "12px",
            }}
          >
            {/* Top row */}
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
              }}
            >
              <div
                style={{ display: "flex", alignItems: "center", gap: "10px" }}
              >
                <span
                  style={{
                    display: "inline-block",
                    width: "10px",
                    height: "10px",
                    borderRadius: "50%",
                    background: service.online
                      ? "var(--ok, #22c55e)"
                      : "var(--soft, #64748b)",
                    boxShadow: service.online
                      ? "0 0 8px rgba(34, 197, 94, 0.6)"
                      : "none",
                  }}
                  title={service.online ? "在线 (Online)" : "离线 (Offline)"}
                />
                <strong style={{ fontSize: "15px" }}>{service.name}</strong>
                <span
                  style={{
                    fontSize: "11px",
                    padding: "2px 6px",
                    borderRadius: "4px",
                    background: "rgba(255,255,255,0.06)",
                    border: "1px solid var(--line, #2d3139)",
                    fontFamily: "monospace",
                  }}
                >
                  :{service.port}
                </span>

                <span
                  style={{
                    fontSize: "11px",
                    padding: "2px 6px",
                    borderRadius: "4px",
                    background: "rgba(59, 130, 246, 0.12)",
                    color: "#93c5fd",
                    border: "1px solid rgba(59, 130, 246, 0.25)",
                  }}
                >
                  {service.serviceType}
                </span>

                {service.version && (
                  <span
                    style={{
                      fontSize: "11px",
                      padding: "2px 6px",
                      borderRadius: "4px",
                      background: "rgba(168, 85, 247, 0.12)",
                      color: "#c084fc",
                      border: "1px solid rgba(168, 85, 247, 0.25)",
                    }}
                  >
                    {service.version}
                  </span>
                )}

                {service.online ? (
                  <span
                    style={{
                      color: "var(--ok, #22c55e)",
                      fontSize: "12px",
                      fontWeight: 600,
                    }}
                  >
                    ● 运行中{" "}
                    {service.latencyMs != null
                      ? `(${service.latencyMs} ms)`
                      : ""}
                  </span>
                ) : (
                  <span className="muted" style={{ fontSize: "12px" }}>
                    ○ 离线未响应
                  </span>
                )}
              </div>

              {/* Action Buttons */}
              <div
                style={{ display: "flex", gap: "8px", alignItems: "center" }}
              >
                {service.webUiUrl && (
                  <a
                    href={service.webUiUrl}
                    target="_blank"
                    rel="noreferrer"
                    style={{
                      padding: "5px 10px",
                      fontSize: "12px",
                      borderRadius: "4px",
                      background: "var(--surface-2, #212631)",
                      color: "var(--fg, #e1e4ea)",
                      border: "1px solid var(--border, #2d3139)",
                      textDecoration: "none",
                      display: "inline-flex",
                      alignItems: "center",
                      gap: "4px",
                    }}
                  >
                    打开服务
                  </a>
                )}

                {service.alreadyAdded ? (
                  <span
                    style={{
                      padding: "5px 12px",
                      fontSize: "12px",
                      borderRadius: "4px",
                      background: "rgba(34, 197, 94, 0.1)",
                      border: "1px solid rgba(34, 197, 94, 0.3)",
                      color: "var(--ok, #22c55e)",
                      fontWeight: 600,
                    }}
                  >
                    已接入 Provider
                  </span>
                ) : (
                  <button
                    type="button"
                    className={service.online ? "primary" : ""}
                    disabled={!service.online || addServiceMutation.isPending}
                    onClick={() => addServiceMutation.mutate(service.id)}
                    style={{
                      fontSize: "12.5px",
                      padding: "6px 14px",
                      fontWeight: 600,
                    }}
                  >
                    {addServiceMutation.isPending
                      ? "正在添加…"
                      : "＋ 一键添加 Provider 并导入模型"}
                  </button>
                )}
              </div>
            </div>

            {/* Service details & URL */}
            <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
              <div
                className="muted"
                style={{
                  fontSize: "12px",
                  fontFamily: "monospace",
                  background: "rgba(0,0,0,0.15)",
                  padding: "4px 8px",
                  borderRadius: "4px",
                }}
              >
                {service.baseUrl}
              </div>
              {service.details && !service.online && (
                <div
                  style={{ fontSize: "12px", color: "var(--muted, #8b949e)" }}
                >
                  {service.details}
                </div>
              )}
            </div>

            {/* Detected Models chips if any */}
            {service.detectedModels && service.detectedModels.length > 0 && (
              <div
                style={{
                  borderTop: "1px solid var(--line, #2d3139)",
                  paddingTop: "10px",
                  display: "flex",
                  flexDirection: "column",
                  gap: "6px",
                }}
              >
                <div
                  style={{
                    fontSize: "12px",
                    color: "var(--muted, #8b949e)",
                    fontWeight: 600,
                  }}
                >
                  已检测到可用模型 ({service.detectedModels.length} 个):
                </div>
                <div style={{ display: "flex", flexWrap: "wrap", gap: "6px" }}>
                  {service.detectedModels.map((m) => (
                    <span
                      key={m}
                      style={{
                        fontSize: "11px",
                        fontFamily: "monospace",
                        padding: "3px 8px",
                        borderRadius: "4px",
                        background: "rgba(59, 130, 246, 0.1)",
                        border: "1px solid rgba(59, 130, 246, 0.2)",
                        color: "#93c5fd",
                      }}
                    >
                      {m}
                    </span>
                  ))}
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
