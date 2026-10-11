import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../../api";
import type {
  ApiTestPreset,
  DevRequestInput,
  DevResponseOutput,
  Provider,
} from "../../types";

interface HeaderItem {
  id: string;
  key: string;
  value: string;
  enabled: boolean;
}

export function DeveloperView() {
  const queryClient = useQueryClient();

  // State
  const [activeReqSubTab, setActiveReqSubTab] = useState<"body" | "headers">(
    "body",
  );
  const [activeRespTab, setActiveRespTab] = useState<
    "body" | "sse" | "headers" | "curl"
  >("body");

  const [currentPresetId, setCurrentPresetId] = useState<string | null>(null);
  const [presetName, setPresetName] = useState("");
  const [method, setMethod] = useState<string>("POST");
  const [url, setUrl] = useState<string>("/v1/chat/completions");
  const [providerId, setProviderId] = useState<string>("");
  const [timeoutMs, setTimeoutMs] = useState<number>(30000);
  const [headers, setHeaders] = useState<HeaderItem[]>([
    { id: "1", key: "Content-Type", value: "application/json", enabled: true },
  ]);
  const [body, setBody] = useState<string>(
    JSON.stringify(
      {
        model: "gpt-4o",
        messages: [
          { role: "user", content: "Hello! This is a test API call." },
        ],
        stream: false,
      },
      null,
      2,
    ),
  );

  const [response, setResponse] = useState<DevResponseOutput | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copyStatus, setCopyStatus] = useState<string | null>(null);
  const [saveModalOpen, setSaveModalOpen] = useState(false);
  const [showPresets, setShowPresets] = useState(true);

  // Queries
  const providersQuery = useQuery<Provider[]>({
    queryKey: ["providers"],
    queryFn: api.providers,
  });

  const presetsQuery = useQuery({
    queryKey: ["developer_presets"],
    queryFn: api.developerListPresets,
  });

  // Mutations
  const sendMutation = useMutation({
    mutationFn: async () => {
      setError(null);
      const headersMap: Record<string, string> = {};
      for (const h of headers) {
        if (h.enabled && h.key.trim()) {
          headersMap[h.key.trim()] = h.value;
        }
      }

      const input: DevRequestInput = {
        method,
        url,
        providerId: providerId || undefined,
        headers: headersMap,
        body: method !== "GET" && method !== "HEAD" ? body : undefined,
        timeoutMs: timeoutMs > 0 ? timeoutMs : undefined,
      };

      return api.developerSendRequest(input);
    },
    onSuccess: (data) => {
      setResponse(data);
      if (data.isSse && data.sseEvents.length > 0) {
        setActiveRespTab("sse");
      } else {
        setActiveRespTab("body");
      }
    },
    onError: (err: unknown) => {
      setError(err instanceof Error ? err.message : String(err));
    },
  });

  const savePresetMutation = useMutation({
    mutationFn: async (name: string) => {
      const headersMap: Record<string, string> = {};
      for (const h of headers) {
        if (h.enabled && h.key.trim()) {
          headersMap[h.key.trim()] = h.value;
        }
      }

      const preset: ApiTestPreset = {
        id: currentPresetId || "",
        name,
        method,
        path: url,
        providerId: providerId || null,
        headers: headersMap,
        body,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      };

      return api.developerSavePreset(preset);
    },
    onSuccess: (saved) => {
      setCurrentPresetId(saved.id);
      setPresetName(saved.name);
      setSaveModalOpen(false);
      queryClient.invalidateQueries({ queryKey: ["developer_presets"] });
      showNotice("预设已保存");
    },
    onError: (err: unknown) => {
      setError(err instanceof Error ? err.message : "保存预设失败");
    },
  });

  const deletePresetMutation = useMutation({
    mutationFn: (id: string) => api.developerDeletePreset(id),
    onSuccess: () => {
      if (currentPresetId) {
        setCurrentPresetId(null);
        setPresetName("");
      }
      queryClient.invalidateQueries({ queryKey: ["developer_presets"] });
      showNotice("预设已删除");
    },
  });

  const showNotice = (msg: string) => {
    setCopyStatus(msg);
    setTimeout(() => setCopyStatus(null), 2500);
  };

  const copyToClipboard = async (text: string, label = "已复制") => {
    try {
      await navigator.clipboard.writeText(text);
      showNotice(label);
    } catch {
      showNotice("复制失败");
    }
  };

  const handleFormatJson = () => {
    try {
      const parsed = JSON.parse(body);
      setBody(JSON.stringify(parsed, null, 2));
      showNotice("JSON 已格式化");
    } catch (e: unknown) {
      setError(`JSON 格式错误: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const loadPreset = (p: ApiTestPreset) => {
    setCurrentPresetId(p.id);
    setPresetName(p.name);
    setMethod(p.method);
    setUrl(p.path);
    if (p.providerId) {
      setProviderId(p.providerId);
    }
    setBody(p.body);

    const headerItems: HeaderItem[] = Object.entries(p.headers || {}).map(
      ([k, v], idx) => ({
        id: String(idx + 1),
        key: k,
        value: v,
        enabled: true,
      }),
    );
    if (headerItems.length === 0) {
      headerItems.push({
        id: "1",
        key: "Content-Type",
        value: "application/json",
        enabled: true,
      });
    }
    setHeaders(headerItems);
    setError(null);
    showNotice(`已载入预设: ${p.name}`);
  };

  const resetBlank = () => {
    setCurrentPresetId(null);
    setPresetName("");
    setMethod("POST");
    setUrl("/v1/chat/completions");
    setHeaders([
      {
        id: "1",
        key: "Content-Type",
        value: "application/json",
        enabled: true,
      },
    ]);
    setBody(
      JSON.stringify(
        {
          model: "gpt-4o",
          messages: [{ role: "user", content: "Hello" }],
        },
        null,
        2,
      ),
    );
    setResponse(null);
    setError(null);
  };

  const addHeaderRow = () => {
    setHeaders((prev) => [
      ...prev,
      {
        id: Math.random().toString(36).substring(2, 9),
        key: "",
        value: "",
        enabled: true,
      },
    ]);
  };

  const updateHeaderRow = (id: string, updates: Partial<HeaderItem>) => {
    setHeaders((prev) =>
      prev.map((h) => (h.id === id ? { ...h, ...updates } : h)),
    );
  };

  const removeHeaderRow = (id: string) => {
    setHeaders((prev) => prev.filter((h) => h.id !== id));
  };

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  };

  const presets = presetsQuery.data || [];
  const providers = providersQuery.data || [];

  return (
    <div
      style={{
        display: "flex",
        height: "100%",
        width: "100%",
        overflow: "hidden",
        background: "var(--bg, #0f1117)",
        color: "var(--fg, #e1e4ea)",
        fontSize: "13px",
      }}
    >
      {/* 1. Left Presets Sidebar */}
      {showPresets && (
        <div
          style={{
            width: "230px",
            minWidth: "230px",
            borderRight: "1px solid var(--border, #2d3139)",
            display: "flex",
            flexDirection: "column",
            background: "var(--surface-1, #161922)",
            flexShrink: 0,
          }}
        >
          <div
            style={{
              padding: "14px 16px",
              borderBottom: "1px solid var(--border, #2d3139)",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <div style={{ fontWeight: 600, fontSize: "14px" }}>API 预设库</div>
            <button
              onClick={resetBlank}
              title="新建空白请求"
              style={{
                padding: "4px 8px",
                fontSize: "12px",
                background: "var(--accent, #3b82f6)",
                color: "var(--accent-ink)",
                border: "none",
                borderRadius: "4px",
                cursor: "pointer",
              }}
            >
              + 新建
            </button>
          </div>

          <div style={{ flex: 1, overflowY: "auto", padding: "10px 8px" }}>
            {/* Default Templates Section */}
            <div
              style={{
                fontSize: "11px",
                color: "var(--muted, #8b949e)",
                textTransform: "uppercase",
                padding: "6px 8px",
                fontWeight: 600,
              }}
            >
              内置标准模板
            </div>
            {presets
              .filter((p) => p.id.startsWith("preset-"))
              .map((p) => (
                <div
                  key={p.id}
                  onClick={() => loadPreset(p)}
                  style={{
                    padding: "8px 10px",
                    borderRadius: "6px",
                    cursor: "pointer",
                    marginBottom: "4px",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    background:
                      currentPresetId === p.id
                        ? "rgba(59, 130, 246, 0.15)"
                        : "transparent",
                    border:
                      currentPresetId === p.id
                        ? "1px solid rgba(59, 130, 246, 0.4)"
                        : "1px solid transparent",
                  }}
                >
                  <div
                    style={{
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    <span
                      style={{
                        fontSize: "10px",
                        fontWeight: 700,
                        marginRight: "6px",
                        padding: "2px 4px",
                        borderRadius: "3px",
                        background: p.method === "POST" ? "#1e3a8a" : "#065f46",
                        color: p.method === "POST" ? "#93c5fd" : "#6ee7b7",
                      }}
                    >
                      {p.method}
                    </span>
                    <span style={{ fontSize: "12px" }}>{p.name}</span>
                  </div>
                </div>
              ))}

            {/* Custom Presets Section */}
            <div
              style={{
                fontSize: "11px",
                color: "var(--muted, #8b949e)",
                textTransform: "uppercase",
                padding: "12px 8px 6px",
                fontWeight: 600,
              }}
            >
              我的自定义预设 (
              {presets.filter((p) => !p.id.startsWith("preset-")).length})
            </div>
            {presets
              .filter((p) => !p.id.startsWith("preset-"))
              .map((p) => (
                <div
                  key={p.id}
                  onClick={() => loadPreset(p)}
                  style={{
                    padding: "8px 10px",
                    borderRadius: "6px",
                    cursor: "pointer",
                    marginBottom: "4px",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    background:
                      currentPresetId === p.id
                        ? "rgba(59, 130, 246, 0.15)"
                        : "transparent",
                    border:
                      currentPresetId === p.id
                        ? "1px solid rgba(59, 130, 246, 0.4)"
                        : "1px solid transparent",
                  }}
                >
                  <div
                    style={{
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      flex: 1,
                    }}
                  >
                    <span
                      style={{
                        fontSize: "10px",
                        fontWeight: 700,
                        marginRight: "6px",
                        padding: "2px 4px",
                        borderRadius: "3px",
                        background: p.method === "POST" ? "#1e3a8a" : "#065f46",
                        color: p.method === "POST" ? "#93c5fd" : "#6ee7b7",
                      }}
                    >
                      {p.method}
                    </span>
                    <span style={{ fontSize: "12px" }}>{p.name}</span>
                  </div>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      if (confirm(`确定删除预设 "${p.name}" 吗？`)) {
                        deletePresetMutation.mutate(p.id);
                      }
                    }}
                    title="删除预设"
                    style={{
                      background: "none",
                      border: "none",
                      color: "var(--muted, #8b949e)",
                      cursor: "pointer",
                      padding: "2px 4px",
                      fontSize: "12px",
                    }}
                  >
                    ✕
                  </button>
                </div>
              ))}
          </div>
        </div>
      )}

      {/* 2. Main Work Area (Composer + Inspector) */}
      <div
        style={{
          flex: 1,
          display: "flex",
          flexDirection: "column",
          minWidth: 0,
        }}
      >
        {/* Header Bar */}
        <div
          style={{
            padding: "12px 18px",
            borderBottom: "1px solid var(--border, #2d3139)",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            background: "var(--surface-1, #161922)",
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
            <button
              type="button"
              onClick={() => setShowPresets(!showPresets)}
              title={showPresets ? "收起预设列表" : "展开预设列表"}
              style={{
                padding: "4px 8px",
                borderRadius: "4px",
                border: "1px solid var(--border, #2d3139)",
                background: "var(--surface-2, #212631)",
                color: "var(--ink)",
                fontSize: "12px",
                cursor: "pointer",
                display: "flex",
                alignItems: "center",
                gap: "4px",
              }}
            >
              <span>{showPresets ? "◀" : "▶"}</span>
              <span>预设库</span>
            </button>
            <div>
              <p className="eyebrow" style={{ margin: 0 }}>
                API Debugger
              </p>
              <h1 style={{ margin: 0, fontSize: "18px" }}>API 调试器</h1>
            </div>
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
            {copyStatus && (
              <span
                style={{
                  fontSize: "12px",
                  color: "#34d399",
                  background: "rgba(52, 211, 153, 0.15)",
                  padding: "4px 10px",
                  borderRadius: "4px",
                  border: "1px solid rgba(52, 211, 153, 0.3)",
                }}
              >
                {copyStatus}
              </span>
            )}
            <button
              onClick={() => {
                setPresetName(presetName || "新建自定义 API 预设");
                setSaveModalOpen(true);
              }}
              style={{
                padding: "6px 12px",
                background: "var(--surface-2)",
                color: "var(--ink)",
                border: "1px solid var(--border)",
                borderRadius: "4px",
                cursor: "pointer",
              }}
            >
              💾 保存预设
            </button>
          </div>
        </div>

        {/* URL / Method / Provider Action Bar */}
        <div
          style={{
            padding: "10px 18px",
            borderBottom: "1px solid var(--border, #2d3139)",
            display: "flex",
            alignItems: "center",
            gap: "8px",
            background: "var(--surface-2, #1c202c)",
            flexWrap: "wrap",
          }}
        >
          {/* Method selector */}
          <select
            value={method}
            onChange={(e) => setMethod(e.target.value)}
            style={{
              padding: "8px 10px",
              fontWeight: 700,
              background:
                method === "POST"
                  ? "#1e3a8a"
                  : method === "GET"
                    ? "#065f46"
                    : "#4c1d95",
              color: "var(--accent-ink)",
              border: "1px solid var(--border, #2d3139)",
              borderRadius: "4px",
              cursor: "pointer",
              outline: "none",
              flexShrink: 0,
            }}
          >
            <option value="GET">GET</option>
            <option value="POST">POST</option>
            <option value="PUT">PUT</option>
            <option value="DELETE">DELETE</option>
          </select>

          {/* Provider Selector (Auto auth & Base URL) */}
          <select
            value={providerId}
            onChange={(e) => setProviderId(e.target.value)}
            style={{
              padding: "8px 10px",
              background: "var(--surface-1, #161922)",
              color: "var(--ink)",
              border: "1px solid var(--border, #2d3139)",
              borderRadius: "4px",
              outline: "none",
              width: "150px",
              minWidth: "120px",
              flexShrink: 0,
            }}
          >
            <option value="">直连 (完整URL)</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} ({p.type})
              </option>
            ))}
          </select>

          {/* URL / Path input */}
          <input
            type="text"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder={
              providerId
                ? "相对路径 (如 /v1/chat/completions)"
                : "完整 URL (如 https://api.openai.com/v1/chat/completions)"
            }
            style={{
              flex: 1,
              minWidth: "160px",
              padding: "8px 12px",
              background: "var(--surface-1, #161922)",
              color: "var(--ink)",
              border: "1px solid var(--border, #2d3139)",
              borderRadius: "4px",
              outline: "none",
              fontFamily: "monospace",
            }}
          />

          {/* Send Button */}
          <button
            onClick={() => sendMutation.mutate()}
            disabled={sendMutation.isPending || !url.trim()}
            style={{
              padding: "8px 18px",
              background: sendMutation.isPending
                ? "#4b5563"
                : "var(--accent, #0e6b4f)",
              color: "var(--accent-ink)",
              fontWeight: 600,
              border: "none",
              borderRadius: "4px",
              cursor:
                sendMutation.isPending || !url.trim()
                  ? "not-allowed"
                  : "pointer",
              flexShrink: 0,
              whiteSpace: "nowrap",
            }}
          >
            {sendMutation.isPending ? "请求中..." : "🚀 发送"}
          </button>
        </div>

        {/* Error banner if any */}
        {error && (
          <div
            style={{
              padding: "10px 18px",
              background: "rgba(239, 68, 68, 0.15)",
              borderBottom: "1px solid rgba(239, 68, 68, 0.3)",
              color: "#fca5a5",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <span>⚠️ {error}</span>
            <button
              onClick={() => setError(null)}
              style={{
                background: "none",
                border: "none",
                color: "#fca5a5",
                cursor: "pointer",
              }}
            >
              ✕
            </button>
          </div>
        )}

        {/* Split View: Request on Left/Top, Response on Right/Bottom */}
        <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
          {/* Request Composer */}
          <div
            style={{
              flex: 1,
              borderRight: "1px solid var(--border, #2d3139)",
              display: "flex",
              flexDirection: "column",
              minHeight: 0,
            }}
          >
            {/* Request Tabs */}
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "0 14px",
                borderBottom: "1px solid var(--border, #2d3139)",
                background: "var(--surface-1, #161922)",
              }}
            >
              <div style={{ display: "flex", gap: "4px" }}>
                <button
                  onClick={() => setActiveReqSubTab("body")}
                  style={{
                    padding: "8px 14px",
                    background: "none",
                    border: "none",
                    borderBottom:
                      activeReqSubTab === "body"
                        ? "2px solid var(--accent, #0e6b4f)"
                        : "2px solid transparent",
                    color:
                      activeReqSubTab === "body"
                        ? "var(--ink, #18181b)"
                        : "var(--muted, #8b949e)",
                    fontWeight: 600,
                    cursor: "pointer",
                  }}
                >
                  Body (JSON)
                </button>
                <button
                  onClick={() => setActiveReqSubTab("headers")}
                  style={{
                    padding: "8px 14px",
                    background: "none",
                    border: "none",
                    borderBottom:
                      activeReqSubTab === "headers"
                        ? "2px solid var(--accent, #0e6b4f)"
                        : "2px solid transparent",
                    color:
                      activeReqSubTab === "headers"
                        ? "var(--ink, #18181b)"
                        : "var(--muted, #8b949e)",
                    fontWeight: 600,
                    cursor: "pointer",
                  }}
                >
                  Headers ({headers.filter((h) => h.enabled && h.key).length})
                </button>
              </div>

              {activeReqSubTab === "body" && (
                <div style={{ display: "flex", gap: "6px" }}>
                  <button
                    onClick={handleFormatJson}
                    style={{
                      padding: "4px 8px",
                      fontSize: "12px",
                      background: "var(--surface-2)",
                      color: "var(--ink)",
                      border: "1px solid var(--border)",
                      borderRadius: "4px",
                      cursor: "pointer",
                    }}
                  >
                    格式化 JSON
                  </button>
                  <button
                    onClick={() => copyToClipboard(body, "Body 已复制")}
                    style={{
                      padding: "4px 8px",
                      fontSize: "12px",
                      background: "var(--surface-2)",
                      color: "var(--ink)",
                      border: "1px solid var(--border)",
                      borderRadius: "4px",
                      cursor: "pointer",
                    }}
                  >
                    复制 Body
                  </button>
                </div>
              )}

              {activeReqSubTab === "headers" && (
                <button
                  onClick={addHeaderRow}
                  style={{
                    padding: "4px 8px",
                    fontSize: "12px",
                    background: "var(--accent, #0e6b4f)",
                    color: "var(--accent-ink)",
                    border: "none",
                    borderRadius: "4px",
                    cursor: "pointer",
                  }}
                >
                  + 添加 Header
                </button>
              )}
            </div>

            {/* Request Content */}
            <div
              style={{
                flex: 1,
                minHeight: 0,
                padding: "10px",
                display: "flex",
                flexDirection: "column",
              }}
            >
              {activeReqSubTab === "body" && (
                <textarea
                  value={body}
                  onChange={(e) => setBody(e.target.value)}
                  placeholder="输入 JSON 请求体..."
                  style={{
                    flex: 1,
                    width: "100%",
                    boxSizing: "border-box",
                    padding: "10px",
                    background: "var(--surface-2, #1c202c)",
                    color: "var(--ink)",
                    border: "1px solid var(--border, #2d3139)",
                    borderRadius: "4px",
                    fontFamily: "monospace",
                    fontSize: "12px",
                    lineHeight: "1.5",
                    resize: "none",
                    outline: "none",
                  }}
                />
              )}

              {activeReqSubTab === "headers" && (
                <div style={{ flex: 1, overflowY: "auto" }}>
                  <div
                    style={{
                      display: "grid",
                      gridTemplateColumns: "30px 180px 1fr 40px",
                      gap: "6px",
                      padding: "6px 8px",
                      color: "var(--muted, #8b949e)",
                      fontWeight: 600,
                      fontSize: "11px",
                      borderBottom: "1px solid var(--border, #2d3139)",
                    }}
                  >
                    <div />
                    <div>HEADER KEY</div>
                    <div>VALUE</div>
                    <div />
                  </div>
                  {headers.map((h) => (
                    <div
                      key={h.id}
                      style={{
                        display: "grid",
                        gridTemplateColumns: "30px 180px 1fr 40px",
                        gap: "6px",
                        padding: "6px 0",
                        alignItems: "center",
                      }}
                    >
                      <input
                        type="checkbox"
                        checked={h.enabled}
                        onChange={(e) =>
                          updateHeaderRow(h.id, { enabled: e.target.checked })
                        }
                      />
                      <input
                        type="text"
                        value={h.key}
                        onChange={(e) =>
                          updateHeaderRow(h.id, { key: e.target.value })
                        }
                        placeholder="Key (e.g. Authorization)"
                        style={{
                          padding: "6px 8px",
                          background: "var(--surface-2, #1c202c)",
                          color: "var(--ink)",
                          border: "1px solid var(--border, #2d3139)",
                          borderRadius: "4px",
                          outline: "none",
                          fontSize: "12px",
                        }}
                      />
                      <input
                        type="text"
                        value={h.value}
                        onChange={(e) =>
                          updateHeaderRow(h.id, { value: e.target.value })
                        }
                        placeholder="Value (e.g. Bearer sk-...)"
                        style={{
                          padding: "6px 8px",
                          background: "var(--surface-2, #1c202c)",
                          color: "var(--ink)",
                          border: "1px solid var(--border, #2d3139)",
                          borderRadius: "4px",
                          outline: "none",
                          fontSize: "12px",
                        }}
                      />
                      <button
                        onClick={() => removeHeaderRow(h.id)}
                        title="删除行"
                        style={{
                          background: "none",
                          border: "none",
                          color: "var(--muted, #8b949e)",
                          cursor: "pointer",
                          fontSize: "14px",
                        }}
                      >
                        ✕
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>

            {/* Quick Timeout & Options bar */}
            <div
              style={{
                padding: "8px 14px",
                borderTop: "1px solid var(--border, #2d3139)",
                background: "var(--surface-1, #161922)",
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                fontSize: "12px",
              }}
            >
              <div
                style={{ display: "flex", alignItems: "center", gap: "8px" }}
              >
                <span style={{ color: "var(--muted, #8b949e)" }}>
                  超时设置:
                </span>
                <input
                  type="number"
                  value={timeoutMs}
                  onChange={(e) => setTimeoutMs(Number(e.target.value))}
                  style={{
                    width: "70px",
                    padding: "3px 6px",
                    background: "var(--surface-2, #1c202c)",
                    color: "var(--ink)",
                    border: "1px solid var(--border, #2d3139)",
                    borderRadius: "4px",
                    outline: "none",
                  }}
                />
                <span style={{ color: "var(--muted, #8b949e)" }}>ms</span>
              </div>
              <div style={{ color: "var(--muted, #8b949e)", fontSize: "11px" }}>
                {providerId
                  ? "自动注入厂商凭证与 BaseURL"
                  : "使用裸请求直连目标 URL"}
              </div>
            </div>
          </div>

          {/* Response Inspector */}
          <div
            style={{
              flex: 1,
              display: "flex",
              flexDirection: "column",
              minHeight: 0,
            }}
          >
            {/* Response Top Bar (Status, Latency, Size) */}
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "0 14px",
                borderBottom: "1px solid var(--border, #2d3139)",
                background: "var(--surface-1, #161922)",
                height: "37px",
              }}
            >
              <div style={{ display: "flex", gap: "4px" }}>
                <button
                  onClick={() => setActiveRespTab("body")}
                  style={{
                    padding: "8px 12px",
                    background: "none",
                    border: "none",
                    borderBottom:
                      activeRespTab === "body"
                        ? "2px solid var(--accent, #0e6b4f)"
                        : "2px solid transparent",
                    color:
                      activeRespTab === "body"
                        ? "var(--ink, #18181b)"
                        : "var(--muted, #8b949e)",
                    fontWeight: 600,
                    cursor: "pointer",
                  }}
                >
                  响应体 (Body)
                </button>
                <button
                  onClick={() => setActiveRespTab("sse")}
                  style={{
                    padding: "8px 12px",
                    background: "none",
                    border: "none",
                    borderBottom:
                      activeRespTab === "sse"
                        ? "2px solid var(--accent, #0e6b4f)"
                        : "2px solid transparent",
                    color:
                      activeRespTab === "sse"
                        ? "var(--ink, #18181b)"
                        : "var(--muted, #8b949e)",
                    fontWeight: 600,
                    cursor: "pointer",
                    position: "relative",
                  }}
                >
                  SSE 流式事件
                  {response?.isSse && (
                    <span
                      style={{
                        marginLeft: "6px",
                        padding: "1px 5px",
                        borderRadius: "10px",
                        background: "#059669",
                        color: "var(--accent-ink)",
                        fontSize: "10px",
                      }}
                    >
                      {response.sseEvents.length}
                    </span>
                  )}
                </button>
                <button
                  onClick={() => setActiveRespTab("headers")}
                  style={{
                    padding: "8px 12px",
                    background: "none",
                    border: "none",
                    borderBottom:
                      activeRespTab === "headers"
                        ? "2px solid var(--accent, #0e6b4f)"
                        : "2px solid transparent",
                    color:
                      activeRespTab === "headers"
                        ? "var(--ink, #18181b)"
                        : "var(--muted, #8b949e)",
                    fontWeight: 600,
                    cursor: "pointer",
                  }}
                >
                  响应头 ({response ? Object.keys(response.headers).length : 0})
                </button>
                <button
                  onClick={() => setActiveRespTab("curl")}
                  style={{
                    padding: "8px 12px",
                    background: "none",
                    border: "none",
                    borderBottom:
                      activeRespTab === "curl"
                        ? "2px solid var(--accent, #0e6b4f)"
                        : "2px solid transparent",
                    color:
                      activeRespTab === "curl"
                        ? "var(--ink, #18181b)"
                        : "var(--muted, #8b949e)",
                    fontWeight: 600,
                    cursor: "pointer",
                  }}
                >
                  cURL 命令
                </button>
              </div>

              {response && (
                <div
                  style={{ display: "flex", alignItems: "center", gap: "10px" }}
                >
                  <span
                    style={{
                      padding: "2px 6px",
                      borderRadius: "4px",
                      fontWeight: 700,
                      fontSize: "11px",
                      background:
                        response.status >= 200 && response.status < 300
                          ? "#065f46"
                          : response.status >= 400 && response.status < 500
                            ? "#78350f"
                            : "#881337",
                      color:
                        response.status >= 200 && response.status < 300
                          ? "#34d399"
                          : response.status >= 400 && response.status < 500
                            ? "#fde047"
                            : "#fca5a5",
                    }}
                  >
                    {response.status} {response.statusText}
                  </span>
                  <span
                    style={{ color: "var(--muted, #8b949e)", fontSize: "11px" }}
                  >
                    ⏱ {response.latencyMs} ms
                  </span>
                  <span
                    style={{ color: "var(--muted, #8b949e)", fontSize: "11px" }}
                  >
                    📦 {formatSize(response.sizeBytes)}
                  </span>
                </div>
              )}
            </div>

            {/* Response Content Panels */}
            <div
              style={{
                flex: 1,
                minHeight: 0,
                padding: "10px",
                display: "flex",
                flexDirection: "column",
              }}
            >
              {!response && (
                <div
                  style={{
                    flex: 1,
                    display: "flex",
                    flexDirection: "column",
                    alignItems: "center",
                    justifyContent: "center",
                    color: "var(--muted, #8b949e)",
                    gap: "8px",
                  }}
                >
                  <div style={{ fontSize: "28px" }}>📡</div>
                  <div>点击上方 “发送 (Send)” 发起实际请求</div>
                  <div style={{ fontSize: "12px" }}>
                    支持查看完整 JSON、响应头、SSE Chunk 流及生成 cURL
                  </div>
                </div>
              )}

              {response && activeRespTab === "body" && (
                <div
                  style={{
                    flex: 1,
                    display: "flex",
                    flexDirection: "column",
                    minHeight: 0,
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      justifyContent: "flex-end",
                      marginBottom: "6px",
                    }}
                  >
                    <button
                      onClick={() =>
                        copyToClipboard(response.body, "响应体已复制")
                      }
                      style={{
                        padding: "3px 8px",
                        fontSize: "11px",
                        background: "var(--surface-2, #212631)",
                        color: "var(--fg, #e1e4ea)",
                        border: "1px solid var(--border, #2d3139)",
                        borderRadius: "4px",
                        cursor: "pointer",
                      }}
                    >
                      复制响应内容
                    </button>
                  </div>
                  <pre
                    style={{
                      flex: 1,
                      margin: 0,
                      padding: "10px",
                      background: "var(--surface-2, #1c202c)",
                      color: "var(--ink)",
                      border: "1px solid var(--border, #2d3139)",
                      borderRadius: "4px",
                      fontFamily: "monospace",
                      fontSize: "12px",
                      lineHeight: "1.4",
                      overflow: "auto",
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-all",
                    }}
                  >
                    {(() => {
                      try {
                        return JSON.stringify(
                          JSON.parse(response.body),
                          null,
                          2,
                        );
                      } catch {
                        return response.body;
                      }
                    })()}
                  </pre>
                </div>
              )}

              {response && activeRespTab === "sse" && (
                <div style={{ flex: 1, overflowY: "auto" }}>
                  {!response.isSse ? (
                    <div
                      style={{
                        color: "var(--muted, #8b949e)",
                        padding: "20px",
                        textAlign: "center",
                      }}
                    >
                      该响应非 text/event-stream 流式响应。
                    </div>
                  ) : response.sseEvents.length === 0 ? (
                    <div
                      style={{
                        color: "var(--muted, #8b949e)",
                        padding: "20px",
                        textAlign: "center",
                      }}
                    >
                      未解析到任何 SSE 事件数据行。
                    </div>
                  ) : (
                    <div
                      style={{
                        display: "flex",
                        flexDirection: "column",
                        gap: "8px",
                      }}
                    >
                      <div
                        style={{
                          fontSize: "12px",
                          color: "var(--muted, #8b949e)",
                          display: "flex",
                          justifyContent: "space-between",
                          padding: "0 4px",
                        }}
                      >
                        <span>
                          共捕获 {response.sseEvents.length} 个流式事件包
                        </span>
                        <button
                          onClick={() => {
                            const fullStreamText = response.sseEvents
                              .map((e) => e.data)
                              .join("\n");
                            copyToClipboard(
                              fullStreamText,
                              "所有流事件数据已复制",
                            );
                          }}
                          style={{
                            background: "none",
                            border: "none",
                            color: "var(--accent, #3b82f6)",
                            cursor: "pointer",
                            fontSize: "11px",
                          }}
                        >
                          复制全部流数据
                        </button>
                      </div>
                      {response.sseEvents.map((evt) => (
                        <div
                          key={evt.index}
                          style={{
                            padding: "8px 10px",
                            background: "var(--surface-2, #1c202c)",
                            border: "1px solid var(--border, #2d3139)",
                            borderRadius: "4px",
                            fontSize: "12px",
                          }}
                        >
                          <div
                            style={{
                              display: "flex",
                              justifyContent: "space-between",
                              marginBottom: "4px",
                              color: "var(--muted, #8b949e)",
                              fontSize: "11px",
                            }}
                          >
                            <div>
                              <span
                                style={{ fontWeight: 700, color: "#60a5fa" }}
                              >
                                #{evt.index}
                              </span>
                              <span
                                style={{ marginLeft: "8px", color: "#a78bfa" }}
                              >
                                event: {evt.eventType}
                              </span>
                            </div>
                            <button
                              onClick={() =>
                                copyToClipboard(
                                  evt.data,
                                  `Chunk #${evt.index} 已复制`,
                                )
                              }
                              style={{
                                background: "none",
                                border: "none",
                                color: "var(--muted, #8b949e)",
                                cursor: "pointer",
                                fontSize: "11px",
                              }}
                            >
                              复制
                            </button>
                          </div>
                          <pre
                            style={{
                              margin: 0,
                              fontFamily: "monospace",
                              whiteSpace: "pre-wrap",
                              wordBreak: "break-all",
                              color: "var(--ink)",
                              fontSize: "11px",
                            }}
                          >
                            {evt.data}
                          </pre>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {response && activeRespTab === "headers" && (
                <div style={{ flex: 1, overflowY: "auto" }}>
                  <table
                    style={{
                      width: "100%",
                      borderCollapse: "collapse",
                      fontSize: "12px",
                      fontFamily: "monospace",
                    }}
                  >
                    <thead>
                      <tr
                        style={{
                          borderBottom: "1px solid var(--border, #2d3139)",
                          textAlign: "left",
                          color: "var(--muted, #8b949e)",
                        }}
                      >
                        <th style={{ padding: "6px 8px", width: "35%" }}>
                          HEADER
                        </th>
                        <th style={{ padding: "6px 8px" }}>VALUE</th>
                      </tr>
                    </thead>
                    <tbody>
                      {Object.entries(response.headers).map(([k, v]) => (
                        <tr
                          key={k}
                          style={{
                            borderBottom: "1px solid var(--border, #2d3139)",
                          }}
                        >
                          <td
                            style={{
                              padding: "6px 8px",
                              color: "var(--accent, #0e6b4f)",
                              fontWeight: 600,
                            }}
                          >
                            {k}
                          </td>
                          <td
                            style={{
                              padding: "6px 8px",
                              color: "var(--ink)",
                              wordBreak: "break-all",
                            }}
                          >
                            {v}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}

              {response && activeRespTab === "curl" && (
                <div
                  style={{
                    flex: 1,
                    display: "flex",
                    flexDirection: "column",
                    minHeight: 0,
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      justifyContent: "flex-end",
                      marginBottom: "6px",
                    }}
                  >
                    <button
                      onClick={() =>
                        copyToClipboard(response.curlCommand, "cURL 命令已复制")
                      }
                      style={{
                        padding: "3px 8px",
                        fontSize: "11px",
                        background: "var(--accent, #0e6b4f)",
                        color: "var(--accent-ink)",
                        border: "none",
                        borderRadius: "4px",
                        cursor: "pointer",
                      }}
                    >
                      一键复制完整 cURL
                    </button>
                  </div>
                  <pre
                    style={{
                      flex: 1,
                      margin: 0,
                      padding: "10px",
                      background: "var(--surface-2, #1c202c)",
                      color: "var(--ink)",
                      border: "1px solid var(--border, #2d3139)",
                      borderRadius: "4px",
                      fontFamily: "monospace",
                      fontSize: "12px",
                      lineHeight: "1.4",
                      overflow: "auto",
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-all",
                    }}
                  >
                    {response.curlCommand}
                  </pre>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Save Preset Modal */}
      {saveModalOpen && (
        <div
          style={{
            position: "fixed",
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            background: "rgba(0,0,0,0.6)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 1000,
          }}
        >
          <div
            style={{
              width: "420px",
              background: "var(--surface-1, #161922)",
              border: "1px solid var(--border, #2d3139)",
              borderRadius: "8px",
              padding: "20px",
              boxShadow: "0 8px 30px rgba(0,0,0,0.5)",
            }}
          >
            <div
              style={{
                fontSize: "16px",
                fontWeight: 600,
                marginBottom: "12px",
              }}
            >
              💾 保存为 API 预设
            </div>
            <div style={{ marginBottom: "14px" }}>
              <div
                style={{
                  fontSize: "12px",
                  color: "var(--muted, #8b949e)",
                  marginBottom: "4px",
                }}
              >
                预设名称
              </div>
              <input
                type="text"
                value={presetName}
                onChange={(e) => setPresetName(e.target.value)}
                placeholder="例如: DeepSeek 对话调试"
                style={{
                  width: "100%",
                  boxSizing: "border-box",
                  padding: "8px 10px",
                  background: "var(--surface-2, #1c202c)",
                  color: "var(--ink)",
                  border: "1px solid var(--border, #2d3139)",
                  borderRadius: "4px",
                  outline: "none",
                }}
              />
            </div>

            <div
              style={{
                display: "flex",
                justifyContent: "flex-end",
                gap: "8px",
              }}
            >
              <button
                onClick={() => setSaveModalOpen(false)}
                style={{
                  padding: "6px 14px",
                  background: "var(--surface-2)",
                  color: "var(--ink)",
                  border: "1px solid var(--border)",
                  borderRadius: "4px",
                  cursor: "pointer",
                }}
              >
                取消
              </button>
              <button
                onClick={() => {
                  if (presetName.trim()) {
                    savePresetMutation.mutate(presetName.trim());
                  }
                }}
                disabled={!presetName.trim()}
                style={{
                  padding: "6px 16px",
                  background: "var(--accent, #0e6b4f)",
                  color: "var(--accent-ink)",
                  border: "none",
                  borderRadius: "4px",
                  cursor: presetName.trim() ? "pointer" : "not-allowed",
                  fontWeight: 600,
                }}
              >
                确认保存
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
