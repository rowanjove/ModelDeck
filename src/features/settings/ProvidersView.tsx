import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, asAppError } from "../../api";
import { ErrorNote } from "../../components/ErrorNote";
import { McpPanel } from "../../components/McpPanel";
import {
  PROVIDER_DEFAULTS,
  PROVIDER_LABELS,
  type Provider,
  type ProviderType,
} from "../../types";
import { ComfyQueuePanel } from "../comfyui/ComfyQueuePanel";
import { PluginSdkModal } from "./PluginSdkModal";

interface HeaderRow {
  id: string;
  key: string;
  value: string;
}

interface EditorState {
  id: string | null;
  name: string;
  type: ProviderType;
  baseUrl: string;
  apiKey: string;
  clearApiKey: boolean;
  hasCredential: boolean;
  headers: HeaderRow[];
  timeoutSec: number;
  enabled: boolean;
}

export function ProvidersView() {
  const queryClient = useQueryClient();
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: api.providers,
  });
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [error, setError] = useState<ReturnType<typeof asAppError> | null>(
    null,
  );
  const [report, setReport] = useState<string | null>(null);
  const [showPlugins, setShowPlugins] = useState(false);
  const [pluginNotice, setPluginNotice] = useState<string | null>(null);
  const [customPluginJson, setCustomPluginJson] = useState("");
  const [showCustomInstall, setShowCustomInstall] = useState(false);
  const [showSdkModal, setShowSdkModal] = useState(false);
  const [instantiateApiKey, setInstantiateApiKey] = useState<
    Record<string, string>
  >({});

  const pluginsQuery = useQuery({
    queryKey: ["plugins"],
    queryFn: api.pluginsList,
    enabled: showPlugins,
  });

  const installPlugin = useMutation({
    mutationFn: (manifest: string) => api.pluginsInstall(manifest),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["plugins"] });
      setCustomPluginJson("");
      setShowCustomInstall(false);
      setPluginNotice("自定义插件安装成功！");
    },
    onError: (err) => setError(asAppError(err)),
  });

  const uninstallPlugin = useMutation({
    mutationFn: (id: string) => api.pluginsUninstall(id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["plugins"] });
      setPluginNotice("插件已删除");
    },
    onError: (err) => setError(asAppError(err)),
  });

  const instantiatePlugin = useMutation({
    mutationFn: ({ id, key }: { id: string; key?: string }) =>
      api.pluginsInstantiate(id, key),
    onSuccess: async (createdProvider) => {
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      await queryClient.invalidateQueries({ queryKey: ["models"] });
      setEditor(fromProvider(createdProvider));
      setShowPlugins(false);
      setPluginNotice(null);
    },
    onError: (err) => setError(asAppError(err)),
  });

  const save = useMutation({
    mutationFn: async (state: EditorState) => {
      const input = {
        name: state.name,
        type: state.type,
        baseUrl: state.baseUrl,
        apiKey: state.apiKey.trim() ? state.apiKey.trim() : null,
        clearApiKey: state.clearApiKey,
        headers: Object.fromEntries(
          state.headers
            .filter((header) => header.key.trim())
            .map((header) => [header.key.trim(), header.value]),
        ),
        timeoutMs: Math.round(state.timeoutSec * 1000),
        enabled: state.enabled,
      };
      const saved = state.id
        ? await api.updateProvider(state.id, input)
        : await api.createProvider(input);
      return saved;
    },
    onSuccess: async (provider) => {
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      setEditor(fromProvider(provider));
      setError(null);
    },
    onError: (reason) => setError(asAppError(reason)),
  });
  const remove = useMutation({
    mutationFn: api.deleteProvider,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      await queryClient.invalidateQueries({ queryKey: ["models"] });
      setEditor(null);
    },
    onError: (reason) => setError(asAppError(reason)),
  });
  const test = useMutation({
    mutationFn: async () => {
      if (!editor?.id) {
        throw new Error("先保存，再测试连接");
      }
      return api.testProvider(editor.id);
    },
    onSuccess: (result) => {
      setReport(`${result.message} · ${result.latencyMs} ms`);
      setError(null);
    },
    onError: (reason) => {
      setReport(null);
      setError(asAppError(reason));
    },
  });

  if (providers.isLoading) {
    return <p className="page-message">正在读取 Provider…</p>;
  }
  if (providers.error) {
    return <ErrorNote error={asAppError(providers.error)} />;
  }

  return (
    <section className="catalog">
      <div className="catalog-list">
        <header>
          <h1>服务接入</h1>
          <div className="inline-actions">
            <button
              type="button"
              onClick={() => {
                setShowPlugins(!showPlugins);
                setPluginNotice(null);
              }}
            >
              {showPlugins ? "返回配置" : "插件接入"}
            </button>
            <button
              type="button"
              className="primary"
              onClick={() => {
                setEditor(blank());
                setReport(null);
                setError(null);
                setShowPlugins(false);
              }}
            >
              新建
            </button>
          </div>
        </header>
        <ul>
          {(providers.data ?? []).map((provider) => {
            const isSelected = editor?.id === provider.id && !showPlugins;
            return (
              <li key={provider.id}>
                <button
                  type="button"
                  aria-current={isSelected ? "page" : undefined}
                  onClick={() => {
                    setEditor(fromProvider(provider));
                    setReport(null);
                    setError(null);
                    setShowPlugins(false);
                  }}
                  style={{
                    opacity: provider.enabled ? 1 : 0.6,
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: "6px",
                      width: "100%",
                    }}
                  >
                    <strong
                      style={{
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        fontSize: "13px",
                      }}
                    >
                      {provider.name}
                    </strong>
                    <span
                      style={{
                        width: "6px",
                        height: "6px",
                        borderRadius: "50%",
                        background: provider.enabled
                          ? "var(--ok)"
                          : "var(--muted)",
                        display: "inline-block",
                        flexShrink: 0,
                      }}
                    />
                  </div>
                  <span
                    style={{
                      fontSize: "11.5px",
                      color: "var(--soft)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {PROVIDER_LABELS[provider.type]} ·{" "}
                    {provider.enabled ? "已启用" : "已停用"}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </div>
      <div className="catalog-detail">
        {showPlugins ? (
          <div>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                marginBottom: "16px",
              }}
            >
              <div>
                <h2 style={{ margin: "0 0 4px" }}>第三方 Provider 插件接入</h2>
                <p className="muted" style={{ margin: 0 }}>
                  一键接入第三方推理云，自动注册推荐模型与参数模板。
                </p>
              </div>
              <div className="inline-actions">
                <button
                  type="button"
                  style={{
                    background: "var(--sunken)",
                    fontWeight: 600,
                  }}
                  onClick={() => setShowSdkModal(true)}
                >
                  Plugin & Adapter SDK
                </button>
                <button
                  type="button"
                  onClick={() => setShowCustomInstall(!showCustomInstall)}
                >
                  {showCustomInstall ? "取消导入" : "导入插件 JSON"}
                </button>
                <button type="button" onClick={() => setShowPlugins(false)}>
                  关闭
                </button>
              </div>
            </div>

            {pluginNotice && (
              <div
                style={{
                  padding: "8px 12px",
                  background: "var(--sunken)",
                  borderRadius: "6px",
                  marginBottom: "16px",
                }}
              >
                {pluginNotice}
              </div>
            )}

            {showCustomInstall && (
              <div
                style={{
                  border: "1px dashed var(--line)",
                  padding: "14px",
                  borderRadius: "8px",
                  marginBottom: "16px",
                }}
              >
                <h3>导入插件清单 (Manifest JSON)</h3>
                <textarea
                  rows={6}
                  value={customPluginJson}
                  onChange={(e) => setCustomPluginJson(e.target.value)}
                  placeholder='{"id":"my-plugin","name":"My Provider","version":"1.0.0","protocol":"openai-compatible","defaultBaseUrl":"https://api.my.ai/v1","recommendedModels":[]}'
                />
                <div style={{ marginTop: "8px" }} className="inline-actions">
                  <button
                    type="button"
                    className="primary"
                    disabled={
                      !customPluginJson.trim() || installPlugin.isPending
                    }
                    onClick={() =>
                      void installPlugin.mutateAsync(customPluginJson)
                    }
                  >
                    确认安装插件
                  </button>
                </div>
              </div>
            )}

            <div style={{ display: "grid", gap: "14px" }}>
              {(pluginsQuery.data ?? []).map((plugin) => (
                <div
                  key={plugin.id}
                  style={{
                    border: "1px solid var(--line)",
                    background: "var(--raised)",
                    borderRadius: "8px",
                    padding: "16px",
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      justifyContent: "space-between",
                      alignItems: "flex-start",
                    }}
                  >
                    <div>
                      <strong>{plugin.name}</strong>{" "}
                      <span className="muted" style={{ marginLeft: "8px" }}>
                        v{plugin.version} · {plugin.protocol}
                      </span>
                      <p style={{ margin: "6px 0", color: "var(--soft)" }}>
                        {plugin.description}
                      </p>
                      <p
                        className="muted"
                        style={{ fontSize: "12px", margin: "4px 0" }}
                      >
                        Base URL: {plugin.defaultBaseUrl}
                      </p>
                    </div>
                    {plugin.id !== "groq" && (
                      <button
                        type="button"
                        className="danger"
                        style={{ fontSize: "12px", padding: "4px 8px" }}
                        onClick={() =>
                          void uninstallPlugin.mutateAsync(plugin.id)
                        }
                      >
                        卸载
                      </button>
                    )}
                  </div>

                  <div style={{ marginTop: "10px", fontSize: "13px" }}>
                    <span className="muted">推荐模型: </span>
                    {plugin.recommendedModels.map((m) => m.name).join("、")}
                  </div>

                  <div
                    style={{
                      marginTop: "12px",
                      display: "flex",
                      gap: "8px",
                      alignItems: "center",
                    }}
                  >
                    <input
                      type="password"
                      placeholder="输入 API Key（可选）"
                      value={instantiateApiKey[plugin.id] ?? ""}
                      onChange={(e) =>
                        setInstantiateApiKey({
                          ...instantiateApiKey,
                          [plugin.id]: e.target.value,
                        })
                      }
                      style={{ maxWidth: "260px" }}
                    />
                    <button
                      type="button"
                      className="primary"
                      disabled={instantiatePlugin.isPending}
                      onClick={() =>
                        void instantiatePlugin.mutateAsync({
                          id: plugin.id,
                          key: instantiateApiKey[plugin.id] || undefined,
                        })
                      }
                    >
                      接入此 Provider
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        ) : editor ? (
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void save.mutateAsync(editor);
            }}
          >
            <p className="eyebrow">Provider</p>
            <p className="muted">
              管理服务地址与凭据。模型页面中的模型通过此服务发送请求。
            </p>
            <div className="form-grid">
              <label>
                名称
                <input
                  value={editor.name}
                  onChange={(event) =>
                    setEditor({ ...editor, name: event.target.value })
                  }
                />
              </label>
              <label>
                类型
                <select
                  value={editor.type}
                  onChange={(event) => {
                    const type = event.target.value as ProviderType;
                    const previousDefault = Object.values(
                      PROVIDER_DEFAULTS,
                    ).includes(editor.baseUrl);
                    setEditor({
                      ...editor,
                      type,
                      baseUrl:
                        editor.baseUrl.trim() === "" || previousDefault
                          ? PROVIDER_DEFAULTS[type]
                          : editor.baseUrl,
                    });
                  }}
                >
                  {(Object.keys(PROVIDER_LABELS) as ProviderType[]).map(
                    (type) => (
                      <option key={type} value={type}>
                        {PROVIDER_LABELS[type]}
                      </option>
                    ),
                  )}
                </select>
              </label>
              <label className="wide">
                Base URL
                <input
                  value={editor.baseUrl}
                  onChange={(event) =>
                    setEditor({ ...editor, baseUrl: event.target.value })
                  }
                />
              </label>
              <label>
                API Key
                <input
                  type="password"
                  autoComplete="off"
                  value={editor.apiKey}
                  placeholder={
                    editor.hasCredential && !editor.clearApiKey
                      ? "已保存在系统凭据库"
                      : ""
                  }
                  onChange={(event) =>
                    setEditor({
                      ...editor,
                      apiKey: event.target.value,
                      clearApiKey: false,
                    })
                  }
                />
              </label>
              <label>
                超时（秒）
                <input
                  type="number"
                  min={1}
                  max={600}
                  value={editor.timeoutSec}
                  onChange={(event) =>
                    setEditor({
                      ...editor,
                      timeoutSec: Number(event.target.value),
                    })
                  }
                />
              </label>
            </div>
            {editor.hasCredential ? (
              <p className="muted">
                修改 Base URL 后请重新输入 API Key；未输入时会移除旧密钥绑定。
              </p>
            ) : null}
            {editor.hasCredential ? (
              <button
                type="button"
                onClick={() =>
                  setEditor({
                    ...editor,
                    clearApiKey: !editor.clearApiKey,
                    apiKey: "",
                  })
                }
              >
                {editor.clearApiKey ? "取消移除密钥" : "移除密钥"}
              </button>
            ) : null}
            <fieldset>
              <legend>自定义 Headers</legend>
              {editor.headers.map((header, index) => (
                <div key={header.id} className="form-grid">
                  <input
                    aria-label="Header 名"
                    value={header.key}
                    onChange={(event) => {
                      const headers = editor.headers.slice();
                      headers[index] = { ...header, key: event.target.value };
                      setEditor({ ...editor, headers });
                    }}
                  />
                  <input
                    aria-label="Header 值"
                    value={header.value}
                    onChange={(event) => {
                      const headers = editor.headers.slice();
                      headers[index] = { ...header, value: event.target.value };
                      setEditor({ ...editor, headers });
                    }}
                  />
                  <button
                    type="button"
                    onClick={() =>
                      setEditor({
                        ...editor,
                        headers: editor.headers.filter(
                          (_, item) => item !== index,
                        ),
                      })
                    }
                  >
                    删除
                  </button>
                </div>
              ))}
              <button
                type="button"
                onClick={() =>
                  setEditor({
                    ...editor,
                    headers: [
                      ...editor.headers,
                      { id: crypto.randomUUID(), key: "", value: "" },
                    ],
                  })
                }
              >
                添加 Header
              </button>
            </fieldset>
            <label className="check">
              <input
                type="checkbox"
                checked={editor.enabled}
                onChange={(event) =>
                  setEditor({ ...editor, enabled: event.target.checked })
                }
              />
              启用
            </label>
            {report ? <p className="ok">{report}</p> : null}
            {error ? <ErrorNote error={error} /> : null}
            <div className="inline-actions">
              <button
                type="submit"
                className="primary"
                disabled={save.isPending}
              >
                {save.isPending ? "保存中…" : "保存"}
              </button>
              <button
                type="button"
                onClick={() => void test.mutateAsync()}
                disabled={!editor.id || test.isPending}
              >
                {test.isPending ? "测试中…" : "测试连接"}
              </button>
              {editor.id ? (
                <button
                  type="button"
                  className="danger"
                  onClick={() => {
                    if (
                      editor.id &&
                      window.confirm(
                        `删除 Provider「${editor.name}」及其模型？`,
                      )
                    ) {
                      void remove.mutateAsync(editor.id);
                    }
                  }}
                >
                  删除
                </button>
              ) : null}
            </div>
            {editor.id && editor.type === "comfyui" ? (
              <ComfyQueuePanel
                providerId={editor.id}
                providerName={editor.name}
              />
            ) : null}
          </form>
        ) : (
          <p className="muted">
            添加 OpenAI、OpenAI Compatible、Ollama、ComfyUI、Generic
            HTTP、Anthropic、Gemini、Replicate 或 fal.ai。
          </p>
        )}
        <McpPanel />
        {showSdkModal ? (
          <PluginSdkModal
            onClose={() => setShowSdkModal(false)}
            onInstalled={async () => {
              await queryClient.invalidateQueries({ queryKey: ["plugins"] });
              setShowSdkModal(false);
            }}
          />
        ) : null}
      </div>
    </section>
  );
}

function blank(): EditorState {
  return {
    id: null,
    name: "",
    type: "openai-compatible",
    baseUrl: PROVIDER_DEFAULTS["openai-compatible"],
    apiKey: "",
    clearApiKey: false,
    hasCredential: false,
    headers: [],
    timeoutSec: 120,
    enabled: true,
  };
}

function fromProvider(provider: Provider): EditorState {
  return {
    id: provider.id,
    name: provider.name,
    type: provider.type,
    baseUrl: provider.baseUrl,
    apiKey: "",
    clearApiKey: false,
    hasCredential: provider.hasCredential,
    headers: Object.entries(provider.headers).map(([key, value]) => ({
      id: crypto.randomUUID(),
      key,
      value,
    })),
    timeoutSec: provider.timeoutMs / 1000,
    enabled: provider.enabled,
  };
}
