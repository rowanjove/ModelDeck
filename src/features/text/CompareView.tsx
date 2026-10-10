import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { api, asAppError } from "../../api";
import { ErrorNote } from "../../components/ErrorNote";
import type { AppError, CompareItemResult, CompareResult } from "../../types";

export function CompareView({
  onOpenModels,
  onShowGenerate,
  onShowChat,
}: {
  onOpenModels: () => void;
  onShowGenerate: () => void;
  onShowChat: () => void;
}) {
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: api.providers,
  });
  const models = useQuery({ queryKey: ["models"], queryFn: api.models });

  const textModels = (models.data ?? []).filter((model) => {
    if (model.modality !== "text" || !model.enabled) {
      return false;
    }
    const provider = providers.data?.find(
      (item) => item.id === model.providerId,
    );
    return Boolean(provider?.enabled);
  });

  const [selectedModelIds, setSelectedModelIds] = useState<string[]>(() => {
    return textModels.slice(0, 2).map((m) => m.id);
  });
  const [systemPrompt, setSystemPrompt] = useState("");
  const [prompt, setPrompt] = useState("");
  const [temperature, setTemperature] = useState<number>(0.7);
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);
  const [compareResult, setCompareResult] = useState<CompareResult | null>(
    null,
  );
  const [error, setError] = useState<AppError | null>(null);

  const compareMutation = useMutation({
    mutationFn: (input: {
      prompt: string;
      systemPrompt?: string;
      modelIds: string[];
      parameters: Record<string, unknown>;
    }) => api.compareText(input),
    onSuccess: (data) => {
      setCompareResult(data);
      setError(null);
    },
    onError: (err) => {
      setError(asAppError(err));
    },
  });

  function toggleModel(id: string) {
    if (selectedModelIds.includes(id)) {
      if (selectedModelIds.length > 2) {
        setSelectedModelIds(selectedModelIds.filter((m) => m !== id));
      }
    } else {
      if (selectedModelIds.length < 4) {
        setSelectedModelIds([...selectedModelIds, id]);
      }
    }
  }

  async function handleCopy(text: string, index: number) {
    await navigator.clipboard.writeText(text);
    setCopiedIndex(index);
    setTimeout(() => setCopiedIndex(null), 2000);
  }

  function handleCompare() {
    if (!prompt.trim() || selectedModelIds.length < 2) {
      return;
    }
    setError(null);
    compareMutation.mutate({
      prompt: prompt.trim(),
      systemPrompt: systemPrompt.trim() || undefined,
      modelIds: selectedModelIds,
      parameters: { temperature },
    });
  }

  if (textModels.length === 0) {
    return (
      <section className="page-message">
        <div className="mode-switch">
          <button type="button" aria-pressed={false} onClick={onShowGenerate}>
            生成
          </button>
          <button type="button" aria-pressed={false} onClick={onShowChat}>
            对话
          </button>
          <button type="button" aria-pressed={true}>
            多模型对比
          </button>
        </div>
        <h1>多模型对比 (Compare)</h1>
        <p>
          目前没有已启用的文本模型。请先在模型管理中配置并启用至少 2 个模型。
        </p>
        <button type="button" className="primary" onClick={onOpenModels}>
          去配置模型
        </button>
      </section>
    );
  }

  return (
    <section className="generate" style={{ overflowY: "auto" }}>
      <header className="topbar">
        <div>
          <div className="mode-switch">
            <button type="button" aria-pressed={false} onClick={onShowGenerate}>
              生成
            </button>
            <button type="button" aria-pressed={false} onClick={onShowChat}>
              对话
            </button>
            <button type="button" aria-pressed={true}>
              多模型对比
            </button>
          </div>
          <p className="eyebrow">Text / Compare</p>
          <h1>多模型并发对比</h1>
        </div>
      </header>

      <div style={{ padding: "0 24px 32px" }}>
        {/* Model Selection area */}
        <div
          style={{
            background: "var(--raised)",
            border: "1px solid var(--line)",
            borderRadius: "8px",
            padding: "16px",
            marginBottom: "20px",
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
            <div>
              <strong style={{ fontSize: "14px" }}>选择对比模型</strong>
              <span
                className="muted"
                style={{ marginLeft: "10px", fontSize: "12.5px" }}
              >
                请选择 2 至 4 个文本模型进行并发结果对比（已选{" "}
                {selectedModelIds.length} / 4）
              </span>
            </div>
            {selectedModelIds.length < 2 && (
              <span style={{ color: "var(--accent)", fontSize: "12px" }}>
                至少需要勾选 2 个模型
              </span>
            )}
          </div>

          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))",
              gap: "10px",
            }}
          >
            {textModels.map((m) => {
              const checked = selectedModelIds.includes(m.id);
              const prov = providers.data?.find((p) => p.id === m.providerId);
              return (
                <label
                  key={m.id}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: "8px",
                    padding: "8px 12px",
                    border: checked
                      ? "1px solid var(--ink)"
                      : "1px solid var(--line)",
                    borderRadius: "6px",
                    background: checked ? "rgba(0,0,0,0.03)" : "transparent",
                    cursor: "pointer",
                    userSelect: "none",
                  }}
                >
                  <input
                    type="checkbox"
                    checked={checked}
                    disabled={!checked && selectedModelIds.length >= 4}
                    onChange={() => toggleModel(m.id)}
                  />
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div
                      style={{
                        fontWeight: 600,
                        fontSize: "13px",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                      title={m.name}
                    >
                      {m.name}
                    </div>
                    <div
                      className="muted"
                      style={{
                        fontSize: "11px",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {prov?.name ?? m.modelId}
                    </div>
                  </div>
                </label>
              );
            })}
          </div>
        </div>

        {/* Prompt Inputs */}
        <div style={{ display: "grid", gap: "12px", marginBottom: "20px" }}>
          <label className="field">
            <span>System Prompt（可选）</span>
            <textarea
              rows={2}
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              placeholder="设定全局人设或规则..."
            />
          </label>

          <label className="field">
            <span>Prompt（提示词）*</span>
            <textarea
              rows={4}
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="输入需要让各个模型进行对比回答的问题或指令..."
            />
          </label>

          <div style={{ display: "flex", gap: "20px", alignItems: "center" }}>
            <label
              style={{
                display: "flex",
                alignItems: "center",
                gap: "8px",
                fontSize: "13px",
              }}
            >
              <span>Temperature: {temperature}</span>
              <input
                type="range"
                min="0"
                max="1.5"
                step="0.1"
                value={temperature}
                onChange={(e) =>
                  setTemperature(Number.parseFloat(e.target.value))
                }
                style={{ width: "120px" }}
              />
            </label>

            <button
              type="button"
              className="primary"
              disabled={
                !prompt.trim() ||
                selectedModelIds.length < 2 ||
                compareMutation.isPending
              }
              onClick={handleCompare}
              style={{ minWidth: "120px" }}
            >
              {compareMutation.isPending ? "并发对比中…" : "开始对比 "}
            </button>
          </div>
        </div>

        {error && <ErrorNote error={error} />}

        {/* Results Side-by-Side View */}
        {compareResult && (
          <div style={{ marginTop: "24px" }}>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                marginBottom: "14px",
              }}
            >
              <h2 style={{ margin: 0, fontSize: "16px" }}>对比结果</h2>
              <span className="muted" style={{ fontSize: "13px" }}>
                共对比 {compareResult.items.length} 个模型
              </span>
            </div>

            <div
              style={{
                display: "grid",
                gridTemplateColumns: `repeat(${Math.max(1, compareResult.items.length)}, minmax(280px, 1fr))`,
                gap: "16px",
                alignItems: "start",
              }}
            >
              {compareResult.items.map(
                (item: CompareItemResult, idx: number) => (
                  <div
                    key={item.modelId}
                    style={{
                      background: "var(--raised)",
                      border: "1px solid var(--line)",
                      borderRadius: "8px",
                      display: "flex",
                      flexDirection: "column",
                      minHeight: "360px",
                    }}
                  >
                    {/* Card Header */}
                    <div
                      style={{
                        padding: "12px 14px",
                        borderBottom: "1px solid var(--line)",
                        background: "rgba(0,0,0,0.02)",
                      }}
                    >
                      <div
                        style={{
                          display: "flex",
                          justifyContent: "space-between",
                          alignItems: "baseline",
                          marginBottom: "4px",
                        }}
                      >
                        <strong style={{ fontSize: "14px" }}>
                          {item.modelName}
                        </strong>
                        <span className="muted" style={{ fontSize: "12px" }}>
                          {item.providerName}
                        </span>
                      </div>

                      {/* Metrics Badges */}
                      <div
                        style={{
                          display: "flex",
                          gap: "10px",
                          fontSize: "11.5px",
                          marginTop: "6px",
                        }}
                      >
                        <span
                          style={{
                            background: "var(--raised)",
                            padding: "2px 6px",
                            borderRadius: "4px",
                            border: "1px solid var(--line)",
                          }}
                        >
                          ⏱ {item.latencyMs} ms
                        </span>
                        <span
                          style={{
                            background: "var(--raised)",
                            padding: "2px 6px",
                            borderRadius: "4px",
                            border: "1px solid var(--line)",
                          }}
                        >
                          {item.tokens} tokens
                        </span>
                        {item.reasoningTokens ? (
                          <span
                            style={{
                              background: "var(--raised)",
                              padding: "2px 6px",
                              borderRadius: "4px",
                              border: "1px solid var(--line)",
                            }}
                          >
                            {item.reasoningTokens}
                          </span>
                        ) : null}
                        <span
                          style={{
                            background: "var(--raised)",
                            padding: "2px 6px",
                            borderRadius: "4px",
                            border: "1px solid var(--line)",
                            color: "var(--accent)",
                          }}
                        >
                          ${item.costUsd.toFixed(4)}
                        </span>
                      </div>
                    </div>

                    {/* Card Body */}
                    <div
                      style={{ padding: "14px", flex: 1, position: "relative" }}
                    >
                      {item.error ? (
                        <div
                          style={{ color: "var(--accent)", fontSize: "13px" }}
                        >
                          <strong>生成失败:</strong>
                          <p style={{ margin: "4px 0" }}>{item.error}</p>
                        </div>
                      ) : (
                        <div>
                          {item.reasoning && (
                            <details
                              style={{
                                marginBottom: "10px",
                                fontSize: "12px",
                                color: "var(--muted)",
                                borderBottom: "1px solid var(--line)",
                                paddingBottom: "8px",
                              }}
                            >
                              <summary
                                style={{
                                  cursor: "pointer",
                                  fontWeight: 500,
                                }}
                              >
                                深度思考过程
                              </summary>
                              <p
                                className="reasoning"
                                style={{
                                  margin: "6px 0 0",
                                  whiteSpace: "pre-wrap",
                                }}
                              >
                                {item.reasoning}
                              </p>
                            </details>
                          )}
                          <div
                            style={{
                              fontSize: "13.5px",
                              lineHeight: "1.6",
                              whiteSpace: "pre-wrap",
                              wordBreak: "break-word",
                            }}
                          >
                            {item.text || (
                              <span className="muted">（空输出）</span>
                            )}
                          </div>
                        </div>
                      )}
                    </div>

                    {/* Card Footer Actions */}
                    {!item.error && item.text && (
                      <div
                        style={{
                          padding: "8px 14px",
                          borderTop: "1px solid var(--line)",
                          display: "flex",
                          justifyContent: "flex-end",
                        }}
                      >
                        <button
                          type="button"
                          className="linkish"
                          style={{ fontSize: "12px" }}
                          onClick={() => void handleCopy(item.text, idx)}
                        >
                          {copiedIndex === idx ? "已复制 " : "复制回答"}
                        </button>
                      </div>
                    )}
                  </div>
                ),
              )}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
