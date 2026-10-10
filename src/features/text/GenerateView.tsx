import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { api, asAppError } from "../../api";
import { ErrorNote } from "../../components/ErrorNote";
import { FileList } from "../../components/FileList";
import { ParameterPanel } from "../../components/ParameterPanel";
import { PresetBar } from "../../components/PresetBar";
import { toCurl } from "../../lib/curl";
import { chooseFiles } from "../../lib/files";
import {
  capabilityEnabled,
  collectParameters,
  initialParameterValues,
  issuesFor,
  parseExtra,
  supportsTextGenerate,
} from "../../lib/schema";
import { useModelPick, useRestore, useTextDraft } from "../../store";
import type {
  AppError,
  BatchGenerateInput,
  BatchGenerateResult,
  TextResult,
} from "../../types";

export function GenerateView({
  onOpenModels,
  onShowChat,
  onShowCompare,
}: {
  onOpenModels: () => void;
  onShowChat: () => void;
  onShowCompare?: () => void;
}) {
  const draft = useTextDraft();
  const queryClient = useQueryClient();
  const restore = useRestore((state) => state.request);
  const seenRestore = useRef(0);
  const seenPick = useRef(0);
  const pick = useModelPick((state) => state.pick);
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: api.providers,
  });
  const models = useQuery({ queryKey: ["models"], queryFn: api.models });
  const [error, setError] = useState<AppError | null>(null);
  const [result, setResult] = useState<TextResult | null>(null);
  const [inspector, setInspector] = useState<"request" | "response" | null>(
    null,
  );
  const [notice, setNotice] = useState<string | null>(null);

  // Batch Generation State
  const [isBatch, setIsBatch] = useState(false);
  const [batchPrompts, setBatchPrompts] = useState("");
  const [batchSeeds, setBatchSeeds] = useState("");
  const [batchResult, setBatchResult] = useState<BatchGenerateResult | null>(
    null,
  );

  const textModels = (models.data ?? []).filter((model) => {
    if (model.modality !== "text" || !model.enabled) {
      return false;
    }
    const provider = providers.data?.find(
      (item) => item.id === model.providerId,
    );
    return Boolean(provider?.enabled);
  });
  const model =
    textModels.find((item) => item.id === draft.modelDefinitionId) ??
    textModels[0] ??
    null;
  const provider = providers.data?.find(
    (item) => item.id === model?.providerId,
  );
  const extra = parseExtra(draft.extraText);
  const issues = model
    ? issuesFor(model.parameters, model.constraints, draft.values)
    : [];
  const runnable = supportsTextGenerate(provider?.type);
  const vision = model
    ? capabilityEnabled(model.capabilities, "vision")
    : false;

  useEffect(() => {
    if (
      !restore ||
      restore.kind !== "generate" ||
      restore.nonce === seenRestore.current
    ) {
      return;
    }
    seenRestore.current = restore.nonce;
    useTextDraft.getState().patch({
      modelDefinitionId: restore.modelDefinitionId,
      boundModelId: restore.modelDefinitionId,
      systemPrompt: restore.systemPrompt,
      prompt: restore.prompt,
      values: restore.parameters,
      extraText: restore.extraText,
      images: restore.images,
    });
    useRestore.getState().clear();
  }, [restore]);

  useEffect(() => {
    if (!model) {
      return;
    }
    const current = useTextDraft.getState();
    const requested = current.modelDefinitionId;
    const requestedExists = (models.data ?? []).some((item) => {
      if (item.id !== requested || item.modality !== "text" || !item.enabled) {
        return false;
      }
      const owner = providers.data?.find(
        (entry) => entry.id === item.providerId,
      );
      return Boolean(owner?.enabled);
    });
    if (requested && requested !== model.id && requestedExists) {
      return;
    }
    if (current.boundModelId === model.id) {
      if (current.modelDefinitionId !== model.id) {
        current.patch({ modelDefinitionId: model.id });
      }
      return;
    }
    current.patch({
      modelDefinitionId: model.id,
      boundModelId: model.id,
      values: initialParameterValues(model.parameters),
      images: capabilityEnabled(model.capabilities, "vision")
        ? current.images
        : [],
    });
  }, [model, models.data, providers.data]);

  useEffect(() => {
    if (!pick || pick.modality !== "text" || pick.nonce === seenPick.current) {
      return;
    }
    seenPick.current = pick.nonce;
    useTextDraft.getState().patch({ modelDefinitionId: pick.modelId });
  }, [pick]);

  const generate = useMutation({
    mutationFn: api.generateText,
    onSuccess: (next) => {
      setResult(next);
      setError(null);
      setInspector(null);
      void queryClient.invalidateQueries({ queryKey: ["history"] });
    },
    onError: (reason) => {
      setError(asAppError(reason));
      void queryClient.invalidateQueries({ queryKey: ["history"] });
    },
  });

  const batchMutation = useMutation({
    mutationFn: (input: BatchGenerateInput) => api.batchGenerate(input),
    onSuccess: (data) => {
      setBatchResult(data);
      setError(null);
      void queryClient.invalidateQueries({ queryKey: ["history"] });
    },
    onError: (err) => {
      setError(asAppError(err));
    },
  });

  const parsedPrompts = batchPrompts
    .split("\n")
    .map((p) => p.trim())
    .filter(Boolean);

  const parsedSeeds = batchSeeds
    .split(/[,，\s]+/)
    .map((s) => s.trim())
    .filter(Boolean)
    .map((s) => Number.parseInt(s, 10))
    .filter((n) => !Number.isNaN(n));

  const batchTaskCount =
    parsedSeeds.length > 0
      ? parsedPrompts.length * parsedSeeds.length
      : parsedPrompts.length;

  async function onGenerate() {
    if (
      !model ||
      !extra.ok ||
      issues.length > 0 ||
      !draft.prompt.trim() ||
      !runnable
    ) {
      return;
    }
    setNotice(null);
    await generate.mutateAsync({
      modelDefinitionId: model.id,
      systemPrompt: draft.systemPrompt.trim() ? draft.systemPrompt : null,
      prompt: draft.prompt,
      parameters: collectParameters(model.parameters, draft.values),
      extra: extra.value,
      images: draft.images.map((path) => ({ path })),
    });
  }

  async function onBatchGenerate() {
    if (!model || !runnable || parsedPrompts.length === 0) {
      return;
    }
    setNotice(null);
    setError(null);
    await batchMutation.mutateAsync({
      modality: "text",
      modelId: model.id,
      prompts: parsedPrompts,
      systemPrompt: draft.systemPrompt.trim() ? draft.systemPrompt : null,
      seeds: parsedSeeds.length > 0 ? parsedSeeds : null,
      parameters: collectParameters(model.parameters, draft.values),
    });
  }

  const workspaceRef = useRef<HTMLElement>(null);
  const generateRef = useRef(onGenerate);
  generateRef.current = onGenerate;
  const batchGenerateRef = useRef(onBatchGenerate);
  batchGenerateRef.current = onBatchGenerate;
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (
        !workspaceRef.current?.getClientRects().length ||
        event.isComposing ||
        event.altKey
      )
        return;
      if (event.ctrlKey && event.key === "Enter") {
        event.preventDefault();
        if (isBatch) {
          void batchGenerateRef.current();
        } else {
          void generateRef.current();
        }
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [isBatch]);

  async function copy(value: string, label: string) {
    await navigator.clipboard.writeText(value);
    setNotice(label);
  }

  async function onSave() {
    if (!result) {
      return;
    }
    const path = await save({
      defaultPath: `${result.modelName}.txt`,
      filters: [{ name: "Text", extensions: ["txt"] }],
    });
    if (!path) {
      return;
    }
    try {
      await api.saveTextFile(path, result.text);
      setNotice("已保存");
      setError(null);
    } catch (reason) {
      setError(asAppError(reason));
    }
  }

  if (providers.isLoading || models.isLoading) {
    return <p className="page-message">正在读取模型…</p>;
  }
  if (providers.error || models.error) {
    return <ErrorNote error={asAppError(providers.error ?? models.error)} />;
  }
  if (!model) {
    return (
      <section className="page-message">
        <ModeSwitch onShowChat={onShowChat} onShowCompare={onShowCompare} />
        <h1>文本生成</h1>
        <p>还没有可用的文本模型。先接入服务，再添加要使用的模型。</p>
        <button type="button" className="primary" onClick={onOpenModels}>
          去配置模型
        </button>
      </section>
    );
  }

  const blocked =
    !draft.prompt.trim() ||
    !extra.ok ||
    issues.length > 0 ||
    !runnable ||
    generate.isPending;

  const batchBlocked =
    parsedPrompts.length === 0 ||
    batchTaskCount > 30 ||
    !extra.ok ||
    issues.length > 0 ||
    !runnable ||
    batchMutation.isPending;

  return (
    <section className="workspace" ref={workspaceRef}>
      <header className="topbar">
        <div>
          <ModeSwitch onShowChat={onShowChat} onShowCompare={onShowCompare} />
          <p className="eyebrow">Text / Generate</p>
          <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
            <h1 style={{ margin: 0 }}>文本生成</h1>
            <button
              type="button"
              className={isBatch ? "primary" : ""}
              style={{
                fontSize: "12px",
                padding: "3px 10px",
                borderRadius: "var(--radius-sm)",
              }}
              onClick={() => setIsBatch(!isBatch)}
            >
              {isBatch ? " 批量模式已开启" : "切换为批量模式"}
            </button>
          </div>
        </div>
        <label className="model-pick">
          <span>模型</span>
          <select
            value={model.id}
            onChange={(event) =>
              draft.patch({ modelDefinitionId: event.target.value })
            }
          >
            {textModels.map((item) => {
              const itemProvider = providers.data?.find(
                (provider) => provider.id === item.providerId,
              );
              return (
                <option key={item.id} value={item.id}>
                  {item.name} · {itemProvider?.name ?? item.modelId}
                </option>
              );
            })}
          </select>
        </label>
      </header>
      <div className="split">
        {isBatch ? (
          /* Batch Mode Composer */
          <div className="composer">
            <label className="field" htmlFor="system-prompt">
              <span>System Prompt（可选）</span>
              <textarea
                id="system-prompt"
                rows={2}
                value={draft.systemPrompt}
                onChange={(event) =>
                  draft.patch({ systemPrompt: event.target.value })
                }
                placeholder="统一注入的系统提示词"
              />
            </label>

            <label className="field grow" htmlFor="batch-prompts">
              <span>批量 Prompts（每行一条）*</span>
              <textarea
                id="batch-prompts"
                rows={6}
                value={batchPrompts}
                onChange={(e) => setBatchPrompts(e.target.value)}
                placeholder={
                  "例如：\n请用一句话赞美春天\n请用一句话赞美夏天\n请用一句话赞美秋天"
                }
              />
            </label>

            <label className="field" htmlFor="batch-seeds">
              <span>随机种子 Seeds（可选，英文逗号分隔）</span>
              <input
                id="batch-seeds"
                value={batchSeeds}
                onChange={(e) => setBatchSeeds(e.target.value)}
                placeholder="例如: 42, 100, 2026"
              />
            </label>

            <div style={{ fontSize: "12.5px", color: "var(--soft)" }}>
              任务矩阵: Prompts {parsedPrompts.length} 行 × Seeds{" "}
              {parsedSeeds.length || 1} 个 = {batchTaskCount} 条任务（单次上限
              30 条，并发数 2）
              {batchTaskCount > 30 && (
                <span style={{ color: "var(--accent)", marginLeft: "8px" }}>
                  超过 30 条上限，请减少 Prompt 行数或 Seed 数量
                </span>
              )}
            </div>

            {error ? <ErrorNote error={error} /> : null}

            {batchResult && (
              <article className="result" style={{ marginTop: "16px" }}>
                <div
                  style={{
                    display: "flex",
                    gap: "16px",
                    marginBottom: "12px",
                    alignItems: "center",
                  }}
                >
                  <strong>批量生成结果概览:</strong>
                  <span>总计 {batchResult.total} 条</span>
                  <span style={{ color: "var(--ok)" }}>
                    成功 {batchResult.succeeded} 条
                  </span>
                  <span
                    style={{
                      color:
                        batchResult.failed > 0 ? "var(--accent)" : "inherit",
                    }}
                  >
                    失败 {batchResult.failed} 条
                  </span>
                </div>

                <div style={{ overflowX: "auto" }}>
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
                          borderBottom: "1px solid var(--line)",
                          textAlign: "left",
                        }}
                      >
                        <th style={{ padding: "6px" }}>#</th>
                        <th style={{ padding: "6px" }}>Prompt</th>
                        <th style={{ padding: "6px" }}>Seed</th>
                        <th style={{ padding: "6px" }}>耗时</th>
                        <th style={{ padding: "6px" }}>状态</th>
                        <th style={{ padding: "6px" }}>输出</th>
                        <th style={{ padding: "6px" }}>操作</th>
                      </tr>
                    </thead>
                    <tbody>
                      {batchResult.items.map((item) => (
                        <tr
                          key={item.index}
                          style={{ borderBottom: "1px solid var(--line)" }}
                        >
                          <td style={{ padding: "8px 6px" }}>
                            {item.index + 1}
                          </td>
                          <td
                            style={{
                              padding: "8px 6px",
                              maxWidth: "180px",
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                            title={item.prompt}
                          >
                            {item.prompt}
                          </td>
                          <td style={{ padding: "8px 6px" }}>
                            {item.seed ?? "—"}
                          </td>
                          <td style={{ padding: "8px 6px" }}>
                            {item.latencyMs} ms
                          </td>
                          <td
                            style={{
                              padding: "8px 6px",
                              color: item.error ? "var(--accent)" : "var(--ok)",
                            }}
                          >
                            {item.error ? "失败" : "成功"}
                          </td>
                          <td
                            style={{
                              padding: "8px 6px",
                              maxWidth: "260px",
                            }}
                          >
                            {item.error ? (
                              <span
                                style={{
                                  color: "var(--accent)",
                                  fontSize: "12px",
                                }}
                              >
                                {item.error}
                              </span>
                            ) : (
                              <pre
                                style={{
                                  margin: 0,
                                  maxHeight: "60px",
                                  overflowY: "auto",
                                  fontSize: "12px",
                                  whiteSpace: "pre-wrap",
                                }}
                              >
                                {item.output}
                              </pre>
                            )}
                          </td>
                          <td style={{ padding: "8px 6px" }}>
                            {item.output ? (
                              <button
                                type="button"
                                className="linkish"
                                onClick={() => {
                                  if (item.output) {
                                    void copy(item.output, "已复制条目输出");
                                  }
                                }}
                              >
                                复制
                              </button>
                            ) : null}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </article>
            )}
          </div>
        ) : (
          /* Single Mode Composer */
          <div className="composer">
            <label className="field" htmlFor="system-prompt">
              <span>System Prompt</span>
              <textarea
                id="system-prompt"
                rows={3}
                value={draft.systemPrompt}
                onChange={(event) =>
                  draft.patch({ systemPrompt: event.target.value })
                }
              />
            </label>
            <label className="field grow" htmlFor="prompt">
              <span>Prompt</span>
              <textarea
                id="prompt"
                rows={8}
                value={draft.prompt}
                onChange={(event) =>
                  draft.patch({ prompt: event.target.value })
                }
                placeholder="写给模型的内容"
              />
            </label>
            {vision ? (
              <FileList
                label="图像输入"
                paths={draft.images}
                multiple
                onChoose={() => chooseFiles("image", true)}
                onChange={(images) => draft.patch({ images })}
              />
            ) : null}
            {error ? <ErrorNote error={error} /> : null}
            {result ? (
              <article className="result">
                <pre>{result.text || "（空响应）"}</pre>
                <dl className="meta">
                  <div>
                    <dt>Tokens</dt>
                    <dd>{formatUsage(result)}</dd>
                  </div>
                  <div>
                    <dt>耗时</dt>
                    <dd>{result.latencyMs} ms</dd>
                  </div>
                  <div>
                    <dt>Provider</dt>
                    <dd>{result.providerName}</dd>
                  </div>
                  <div>
                    <dt>Model</dt>
                    <dd>{result.modelName}</dd>
                  </div>
                </dl>
                <div className="actions">
                  <button
                    type="button"
                    onClick={() => void copy(result.text, "已复制结果")}
                  >
                    复制
                  </button>
                  <button
                    type="button"
                    onClick={() => void onGenerate()}
                    disabled={blocked}
                  >
                    重新生成
                  </button>
                  <button type="button" onClick={() => void onSave()}>
                    保存
                  </button>
                  <button
                    type="button"
                    onClick={() =>
                      setInspector(inspector === "request" ? null : "request")
                    }
                  >
                    原始请求
                  </button>
                  <button
                    type="button"
                    onClick={() =>
                      setInspector(inspector === "response" ? null : "response")
                    }
                  >
                    原始响应
                  </button>
                  {notice ? <span className="muted">{notice}</span> : null}
                </div>
                {inspector === "request" ? (
                  <div className="inspector">
                    <div className="inline-actions">
                      <button
                        type="button"
                        onClick={() =>
                          void copy(
                            JSON.stringify(result.rawRequest, null, 2),
                            "已复制 JSON",
                          )
                        }
                      >
                        复制 JSON
                      </button>
                      <button
                        type="button"
                        onClick={() =>
                          void copy(
                            toCurl(result.rawRequest),
                            "已复制 cURL（密钥已脱敏）",
                          )
                        }
                      >
                        复制 cURL
                      </button>
                    </div>
                    <pre>{JSON.stringify(result.rawRequest, null, 2)}</pre>
                  </div>
                ) : null}
                {inspector === "response" ? (
                  <div className="inspector">
                    <button
                      type="button"
                      onClick={() =>
                        void copy(
                          JSON.stringify(result.rawResponse, null, 2),
                          "已复制响应",
                        )
                      }
                    >
                      复制响应
                    </button>
                    <pre>{JSON.stringify(result.rawResponse, null, 2)}</pre>
                  </div>
                ) : null}
              </article>
            ) : null}
          </div>
        )}

        <aside className="parameters">
          <PresetBar
            modelId={model.id}
            values={draft.values}
            systemPrompt={draft.systemPrompt}
            userTemplate={draft.prompt}
            onApply={(values) => draft.patch({ values })}
            onApplyPreset={(p) => {
              const updates: Record<string, unknown> = {};
              if (p.systemPrompt !== undefined && p.systemPrompt !== null) {
                updates.systemPrompt = p.systemPrompt;
              }
              if (p.userTemplate !== undefined && p.userTemplate !== null) {
                updates.prompt = p.userTemplate;
              }
              draft.patch(updates);
            }}
          />
          <ParameterPanel
            parameters={model.parameters}
            constraints={model.constraints}
            values={draft.values}
            onChange={(values) => draft.patch({ values })}
            chooseFiles={chooseFiles}
          />
          <details className="fold">
            <summary>Extra JSON</summary>
            <p className="muted">
              同名键会覆盖标准参数。密钥会被脱敏，不能覆盖 model 和 messages。
            </p>
            <textarea
              rows={6}
              value={draft.extraText}
              onChange={(event) =>
                draft.patch({ extraText: event.target.value })
              }
              placeholder='{"custom_flag": true}'
              spellCheck={false}
            />
            {extra.ok ? null : <p className="constraint">{extra.message}</p>}
          </details>
          {issues
            .filter((issue) => !issue.includes("不能同时设置"))
            .map((issue) => (
              <p key={issue} className="constraint">
                {issue}
              </p>
            ))}
          {runnable ? null : (
            <p className="constraint">
              这个 Provider 类型还不能执行文本生成。当前支持 OpenAI、OpenAI
              Compatible 与 Ollama。
            </p>
          )}
          {isBatch ? (
            <button
              type="button"
              className="primary generate"
              disabled={batchBlocked}
              onClick={() => void onBatchGenerate()}
            >
              {batchMutation.isPending ? "批量生成中…" : "开始批量生成"}
              <kbd>Ctrl+Enter</kbd>
            </button>
          ) : (
            <button
              type="button"
              className="primary generate"
              disabled={blocked}
              onClick={() => void onGenerate()}
            >
              {generate.isPending ? "生成中…" : "生成"}
              <kbd>Ctrl+Enter</kbd>
            </button>
          )}
        </aside>
      </div>
    </section>
  );
}

function ModeSwitch({
  onShowChat,
  onShowCompare,
}: {
  onShowChat: () => void;
  onShowCompare?: () => void;
}) {
  return (
    <div className="mode-switch">
      <button type="button" aria-pressed={true}>
        生成
      </button>
      <button type="button" aria-pressed={false} onClick={onShowChat}>
        对话
      </button>
      {onShowCompare && (
        <button type="button" aria-pressed={false} onClick={onShowCompare}>
          多模型对比
        </button>
      )}
    </div>
  );
}

function formatUsage(result: TextResult): string {
  const usage = result.usage;
  if (!usage) {
    return "—";
  }
  const total = usage.totalTokens ?? "—";
  const input = usage.inputTokens ?? "—";
  const output = usage.outputTokens ?? "—";
  const reasoning = usage.reasoningTokens
    ? ` / 思考 ${usage.reasoningTokens}`
    : "";
  return `${total}（输入 ${input} / 输出 ${output}${reasoning}）`;
}
