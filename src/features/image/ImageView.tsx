import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { api, asAppError } from "../../api";
import { CostEstimationHint } from "../../components/CostEstimationHint";
import { ErrorNote } from "../../components/ErrorNote";
import { ImageThumb } from "../../components/ImageThumb";
import { ParameterPanel } from "../../components/ParameterPanel";
import { PresetBar } from "../../components/PresetBar";
import { TaskStatus } from "../../components/TaskStatus";
import { chooseFiles } from "../../lib/files";
import {
  capabilityEnabled,
  collectParameters,
  fileName,
  initialParameterValues,
  issuesFor,
  parseExtra,
  scalarParameters,
  supportsImageGenerate,
} from "../../lib/schema";
import { useTaskQueueStore, useTaskState } from "../../lib/taskQueue";
import { useImageDraft, useModelPick, useRestore } from "../../store";
import type {
  AppError,
  BatchGenerateInput,
  BatchGenerateResult,
  ImageResult,
} from "../../types";
import { ComfyQueuePanel } from "../comfyui/ComfyQueuePanel";
import { MaskEditor } from "./MaskEditor";

const ROLES = [
  { value: "", label: "不指定" },
  { value: "General", label: "通用" },
  { value: "Character", label: "角色" },
  { value: "Style", label: "风格" },
  { value: "Pose", label: "姿态" },
  { value: "Composition", label: "构图" },
  { value: "Structure", label: "结构" },
  { value: "Control", label: "控制" },
];

export function ImageView({
  fresh,
  onOpenModels,
}: {
  fresh: number;
  onOpenModels: () => void;
}) {
  const draft = useImageDraft();
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
  const [result, setResult] = useState<ImageResult | null>(null);
  const [maskBase64, setMaskBase64] = useState<string | null>(null);

  // Batch Generation State
  const [isBatch, setIsBatch] = useState(false);
  const [batchPrompts, setBatchPrompts] = useState("");
  const [batchSeeds, setBatchSeeds] = useState("");
  const [batchResult, setBatchResult] = useState<BatchGenerateResult | null>(
    null,
  );

  const imageModels = (models.data ?? []).filter((model) => {
    if (model.modality !== "image" || !model.enabled) {
      return false;
    }
    const provider = providers.data?.find(
      (item) => item.id === model.providerId,
    );
    return Boolean(provider?.enabled);
  });
  const model =
    imageModels.find((item) => item.id === draft.modelDefinitionId) ??
    imageModels[0] ??
    null;
  const provider = providers.data?.find(
    (item) => item.id === model?.providerId,
  );
  const scalars = model ? scalarParameters(model.parameters) : [];
  const extra = parseExtra(draft.extraText);
  const issues = model
    ? issuesFor(scalars, model.constraints, draft.values)
    : [];
  const runnable = supportsImageGenerate(provider?.type);
  const allowSource = model
    ? capabilityEnabled(model.capabilities, "imageToImage") ||
      capabilityEnabled(model.capabilities, "edit") ||
      capabilityEnabled(model.capabilities, "mask")
    : false;
  const allowReferences = model
    ? capabilityEnabled(model.capabilities, "referenceImages")
    : false;
  const allowMask = model
    ? capabilityEnabled(model.capabilities, "mask")
    : false;
  const promptOptional = provider?.type === "comfyui";

  useEffect(() => {
    if (fresh < 0) {
      return;
    }
    setResult(null);
    setError(null);
    setMaskBase64(null);
  }, [fresh]);

  useEffect(() => {
    if (
      !restore ||
      restore.kind !== "image" ||
      restore.nonce === seenRestore.current
    ) {
      return;
    }
    seenRestore.current = restore.nonce;
    setMaskBase64(null);
    useImageDraft.getState().patch({
      modelDefinitionId: restore.modelDefinitionId,
      boundModelId: restore.modelDefinitionId,
      prompt: restore.prompt,
      negativePrompt: restore.negativePrompt ?? "",
      values: restore.parameters,
      extraText: restore.extraText,
      sourcePath: restore.sourcePath ?? null,
      references: restore.references ?? [],
      maskPath: restore.maskPath ?? null,
    });
    useRestore.getState().clear();
  }, [restore]);

  useEffect(() => {
    if (!model) {
      return;
    }
    const current = useImageDraft.getState();
    const requested = current.modelDefinitionId;
    const requestedExists = (models.data ?? []).some((item) => {
      if (item.id !== requested || item.modality !== "image" || !item.enabled) {
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
    const sourceOk =
      capabilityEnabled(model.capabilities, "imageToImage") ||
      capabilityEnabled(model.capabilities, "edit") ||
      capabilityEnabled(model.capabilities, "mask");
    current.patch({
      modelDefinitionId: model.id,
      boundModelId: model.id,
      values: initialParameterValues(model.parameters),
      sourcePath: sourceOk ? current.sourcePath : null,
      references: capabilityEnabled(model.capabilities, "referenceImages")
        ? current.references
        : [],
      maskPath: capabilityEnabled(model.capabilities, "mask")
        ? current.maskPath
        : null,
    });
  }, [model, models.data, providers.data]);

  useEffect(() => {
    if (!pick || pick.modality !== "image" || pick.nonce === seenPick.current) {
      return;
    }
    seenPick.current = pick.nonce;
    useImageDraft.getState().patch({ modelDefinitionId: pick.modelId });
  }, [pick]);

  const activeTask = useTaskState(draft.activeTaskId);

  useEffect(() => {
    if (!activeTask) return;
    if (activeTask.eventType === "completed" && activeTask.outputAsset) {
      if (isBatch) {
        setBatchResult(activeTask.outputAsset as BatchGenerateResult);
      } else {
        setResult(activeTask.outputAsset as ImageResult);
      }
      setError(null);
      void queryClient.invalidateQueries({ queryKey: ["history"] });
      useImageDraft.getState().patch({ activeTaskId: null });
    } else if (activeTask.eventType === "failed") {
      setError({
        code: "task_failed",
        message: activeTask.error || activeTask.message || "图像生成失败",
      });
      void queryClient.invalidateQueries({ queryKey: ["history"] });
      useImageDraft.getState().patch({ activeTaskId: null });
    } else if (activeTask.eventType === "cancelled") {
      setError({
        code: "cancelled",
        message: "任务已取消",
      });
      useImageDraft.getState().patch({ activeTaskId: null });
    }
  }, [activeTask, isBatch, queryClient]);

  const generate = useMutation({
    mutationFn: async (input: Parameters<typeof api.generateImage>[0]) => {
      const taskId = crypto.randomUUID();
      useImageDraft.getState().patch({ activeTaskId: taskId });
      setResult(null);
      setError(null);
      await api.tasksSubmitImage(input, taskId);
      return taskId;
    },
    onError: (reason) => {
      useImageDraft.getState().patch({ activeTaskId: null });
      setError(asAppError(reason));
      void queryClient.invalidateQueries({ queryKey: ["history"] });
    },
  });

  const batchMutation = useMutation({
    mutationFn: async (input: BatchGenerateInput) => {
      const taskId = crypto.randomUUID();
      useImageDraft.getState().patch({ activeTaskId: taskId });
      setBatchResult(null);
      setError(null);
      await api.tasksSubmitBatch(input, taskId);
      return taskId;
    },
    onError: (err) => {
      useImageDraft.getState().patch({ activeTaskId: null });
      setError(asAppError(err));
      void queryClient.invalidateQueries({ queryKey: ["history"] });
    },
  });

  const isGenerating = Boolean(
    generate.isPending ||
      batchMutation.isPending ||
      (draft.activeTaskId &&
        activeTask &&
        !["completed", "failed", "cancelled"].includes(activeTask.status)),
  );

  async function onBatchGenerate() {
    if (!model || isGenerating) return;
    const prompts = batchPrompts
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
    if (prompts.length === 0) {
      setError({
        code: "ValidationError",
        message: "批量生成至少需要填写一行提示词",
      });
      return;
    }
    const seeds = batchSeeds
      .split(/[,，\s]+/)
      .map((s) => s.trim())
      .filter(Boolean)
      .map(Number)
      .filter((n) => Number.isFinite(n));

    setError(null);
    setBatchResult(null);
    await batchMutation.mutateAsync({
      modality: "image",
      modelId: model.id,
      prompts,
      seeds: seeds.length > 0 ? seeds : null,
      parameters: collectParameters(scalars, draft.values),
    });
  }

  async function onCancel() {
    if (draft.activeTaskId) {
      await useTaskQueueStore.getState().cancelTask(draft.activeTaskId);
      useImageDraft.getState().patch({ activeTaskId: null });
    }
  }

  const weightsOk = draft.references.every((item) => {
    const text = item.weight.trim();
    return text === "" || Number.isFinite(Number(text));
  });

  async function onGenerate() {
    if (
      !model ||
      !extra.ok ||
      issues.length > 0 ||
      !weightsOk ||
      !runnable ||
      (!promptOptional && !draft.prompt.trim())
    ) {
      return;
    }
    await generate.mutateAsync({
      modelDefinitionId: model.id,
      prompt: draft.prompt,
      negativePrompt: draft.negativePrompt.trim() ? draft.negativePrompt : null,
      sourceImage:
        allowSource && draft.sourcePath ? { path: draft.sourcePath } : null,
      referenceImages: allowReferences
        ? draft.references
            .filter((item) => item.enabled !== false)
            .map((item) => ({
              path: item.path,
              role: item.role || null,
              weight: item.weight.trim() ? Number(item.weight) : null,
              description: item.description.trim() || null,
              enabled: true,
            }))
        : [],
      maskBase64: allowMask ? maskBase64 : null,
      maskPath: allowMask && !maskBase64 ? draft.maskPath : null,
      parameters: collectParameters(scalars, draft.values),
      extra: extra.value,
    });
  }

  const workspaceRef = useRef<HTMLElement>(null);
  const generateRef = useRef(onGenerate);
  generateRef.current = onGenerate;
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
        void generateRef.current();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (providers.isLoading || models.isLoading) {
    return <p className="page-message">正在读取模型…</p>;
  }
  if (providers.error || models.error) {
    return <ErrorNote error={asAppError(providers.error ?? models.error)} />;
  }
  if (!model) {
    return (
      <section className="page-message">
        <h1>图像生成</h1>
        <p>
          还没有可用的图像模型。添加 Provider 和 Model Definition 后即可生成。
        </p>
        <button type="button" className="primary" onClick={onOpenModels}>
          去配置模型
        </button>
      </section>
    );
  }

  const blocked =
    (!promptOptional && !draft.prompt.trim()) ||
    !extra.ok ||
    issues.length > 0 ||
    !weightsOk ||
    !runnable ||
    isGenerating;

  return (
    <section className="workspace" ref={workspaceRef}>
      <header className="topbar">
        <div>
          <p className="eyebrow">Image</p>
          <h1>图像生成</h1>
        </div>
        <label className="model-pick">
          <span>模型</span>
          <select
            value={model.id}
            onChange={(event) =>
              draft.patch({ modelDefinitionId: event.target.value })
            }
          >
            {imageModels.map((item) => {
              const itemProvider = providers.data?.find(
                (entry) => entry.id === item.providerId,
              );
              return (
                <option key={item.id} value={item.id}>
                  {item.name} · {itemProvider?.name ?? item.modelId}
                </option>
              );
            })}
          </select>
        </label>
        {provider && provider.type === "comfyui" ? (
          <ComfyQueuePanel providerId={provider.id} compact />
        ) : null}
      </header>
      <div className="split">
        {isBatch ? (
          <div className="composer">
            <div style={{ display: "flex", gap: "8px", marginBottom: "12px" }}>
              <button
                type="button"
                aria-pressed={false}
                onClick={() => setIsBatch(false)}
              >
                单张生成
              </button>
              <button
                type="button"
                className="primary"
                aria-pressed={true}
                onClick={() => setIsBatch(true)}
              >
                批量生成 (Prompts × Seeds)
              </button>
            </div>
            <label className="field grow" htmlFor="image-batch-prompts">
              <span>批量提示词（每行一条）</span>
              <textarea
                id="image-batch-prompts"
                rows={8}
                value={batchPrompts}
                onChange={(event) => setBatchPrompts(event.target.value)}
                placeholder="每行一个 Prompt，例如：&#10;cyberpunk city street at night, neon lights&#10;ancient ruined castle on a cliff, cinematic lighting&#10;fantasy warrior portrait, detailed armor"
              />
            </label>
            <label className="field" htmlFor="image-batch-seeds">
              <span>随机种子列表（可选，逗号分隔）</span>
              <input
                id="image-batch-seeds"
                value={batchSeeds}
                onChange={(event) => setBatchSeeds(event.target.value)}
                placeholder="例如：42, 100, 2024"
              />
            </label>
            {error ? <ErrorNote error={error} /> : null}
            <div className="inline-actions">
              <button
                type="button"
                className="primary"
                disabled={isGenerating || !batchPrompts.trim()}
                onClick={() => void onBatchGenerate()}
              >
                {isGenerating ? "批量生成中…" : "开始批量生成"}
              </button>
              {isGenerating ? (
                <button
                  type="button"
                  className="danger"
                  onClick={() => void onCancel()}
                >
                  取消
                </button>
              ) : null}
            </div>
            <TaskStatus task={activeTask} />
            {batchResult && (
              <article className="result" style={{ marginTop: "16px" }}>
                <p className="eyebrow">
                  批量生成完成：成功 {batchResult.succeeded} / 总共{" "}
                  {batchResult.total}
                </p>
                <div
                  style={{
                    display: "grid",
                    gridTemplateColumns:
                      "repeat(auto-fill, minmax(200px, 1fr))",
                    gap: "12px",
                    marginTop: "10px",
                  }}
                >
                  {batchResult.items.map((item) => (
                    <div
                      key={item.index}
                      style={{
                        background: "var(--raised)",
                        border: "1px solid var(--line)",
                        borderRadius: "6px",
                        padding: "10px",
                        display: "flex",
                        flexDirection: "column",
                        gap: "6px",
                      }}
                    >
                      {item.output ? (
                        <ImageThumb path={item.output} alt={item.prompt} />
                      ) : (
                        <div
                          style={{
                            height: "120px",
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "center",
                            background: "var(--line)",
                          }}
                        >
                          <span
                            className="constraint"
                            style={{ fontSize: "12px" }}
                          >
                            {item.error || "失败"}
                          </span>
                        </div>
                      )}
                      <p
                        style={{
                          fontSize: "12px",
                          margin: 0,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                        title={item.prompt}
                      >
                        {item.prompt}
                      </p>
                      <div
                        style={{
                          display: "flex",
                          justifyContent: "space-between",
                          fontSize: "11px",
                          color: "var(--muted)",
                        }}
                      >
                        <span>种子: {item.seed ?? "—"}</span>
                        <span>{item.latencyMs} ms</span>
                      </div>
                      {item.output && (
                        <div
                          style={{
                            display: "flex",
                            gap: "6px",
                            marginTop: "4px",
                          }}
                        >
                          <button
                            type="button"
                            className="linkish"
                            style={{ fontSize: "11px" }}
                            onClick={() => {
                              draft.patch({ sourcePath: item.output });
                              setIsBatch(false);
                            }}
                          >
                            设为主图
                          </button>
                          <button
                            type="button"
                            className="linkish"
                            style={{ fontSize: "11px" }}
                            onClick={() => {
                              if (
                                item.output &&
                                !draft.references.some(
                                  (r) => r.path === item.output,
                                )
                              ) {
                                draft.patch({
                                  references: [
                                    ...draft.references,
                                    {
                                      path: item.output,
                                      role: "General",
                                      weight: "1.0",
                                      description: "批量生成参考",
                                      enabled: true,
                                    },
                                  ],
                                });
                              }
                              setIsBatch(false);
                            }}
                          >
                            加为参考
                          </button>
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              </article>
            )}
          </div>
        ) : (
          <div className="composer">
            <div style={{ display: "flex", gap: "8px", marginBottom: "12px" }}>
              <button
                type="button"
                className="primary"
                aria-pressed={true}
                onClick={() => setIsBatch(false)}
              >
                单张生成
              </button>
              <button
                type="button"
                aria-pressed={false}
                onClick={() => setIsBatch(true)}
              >
                批量生成 (Prompts × Seeds)
              </button>
            </div>
            <label className="field grow" htmlFor="image-prompt">
              <span>提示词</span>
              <textarea
                id="image-prompt"
                rows={6}
                value={draft.prompt}
                onChange={(event) =>
                  draft.patch({ prompt: event.target.value })
                }
                placeholder={
                  promptOptional
                    ? "留空则使用工作流里的提示词"
                    : "描述要生成的图像"
                }
              />
            </label>
            <label className="field" htmlFor="negative-prompt">
              <span>反向提示词</span>
              <textarea
                id="negative-prompt"
                rows={3}
                value={draft.negativePrompt}
                onChange={(event) =>
                  draft.patch({ negativePrompt: event.target.value })
                }
                placeholder="不希望出现的内容。OpenAI 会忽略这一项"
              />
            </label>
            {allowSource ? (
              <div className="file-list">
                <div className="file-list-head">
                  <span>原图</span>
                  <button
                    type="button"
                    onClick={() => {
                      void chooseFiles("image", false).then((picked) => {
                        if (picked.length === 0) {
                          return;
                        }
                        draft.patch({
                          sourcePath: picked[0],
                          maskPath: null,
                        });
                        setMaskBase64(null);
                      });
                    }}
                  >
                    选择文件
                  </button>
                </div>
                {draft.sourcePath ? (
                  <ul>
                    <li>
                      <span title={draft.sourcePath}>
                        {fileName(draft.sourcePath)}
                      </span>
                      <button
                        type="button"
                        onClick={() => {
                          draft.patch({ sourcePath: null, maskPath: null });
                          setMaskBase64(null);
                        }}
                      >
                        移除
                      </button>
                    </li>
                  </ul>
                ) : (
                  <p className="muted">未选择</p>
                )}
              </div>
            ) : null}
            {allowReferences ? (
              <div className="file-list">
                <div className="file-list-head">
                  <span>参考图管理器</span>
                  <button
                    type="button"
                    onClick={() => {
                      void chooseFiles("image", true).then((picked) => {
                        if (picked.length === 0) {
                          return;
                        }
                        const next = [...draft.references];
                        for (const path of picked) {
                          if (next.some((item) => item.path === path)) {
                            continue;
                          }
                          if (next.length >= 8) {
                            break;
                          }
                          next.push({
                            path,
                            role: "",
                            weight: "",
                            description: "",
                            enabled: true,
                          });
                        }
                        draft.patch({ references: next });
                      });
                    }}
                  >
                    添加
                  </button>
                </div>
                {draft.references.length === 0 ? (
                  <p className="muted">未选择参考图</p>
                ) : (
                  draft.references.map((item) => (
                    <div
                      key={item.path}
                      className="reference-row"
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: "6px",
                        padding: "4px 8px",
                      }}
                    >
                      <input
                        type="checkbox"
                        checked={item.enabled !== false}
                        title={item.enabled === false ? "点击启用" : "点击禁用"}
                        onChange={(e) =>
                          draft.patch({
                            references: draft.references.map((entry) =>
                              entry.path === item.path
                                ? { ...entry, enabled: e.target.checked }
                                : entry,
                            ),
                          })
                        }
                      />
                      <div
                        style={{
                          width: 28,
                          height: 28,
                          borderRadius: "var(--radius-xs)",
                          overflow: "hidden",
                          flexShrink: 0,
                          background: "var(--sunken)",
                        }}
                      >
                        <ImageThumb path={item.path} alt="" />
                      </div>
                      <span
                        title={item.path}
                        style={{
                          opacity: item.enabled === false ? 0.45 : 1,
                          textDecoration:
                            item.enabled === false ? "line-through" : undefined,
                          maxWidth: "90px",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          fontSize: "12px",
                        }}
                      >
                        {fileName(item.path)}
                      </span>
                      <select
                        value={item.role}
                        aria-label="参考图角色"
                        style={{ padding: "3px 6px", fontSize: "11.5px" }}
                        onChange={(event) =>
                          draft.patch({
                            references: draft.references.map((entry) =>
                              entry.path === item.path
                                ? { ...entry, role: event.target.value }
                                : entry,
                            ),
                          })
                        }
                      >
                        {ROLES.map((role) => (
                          <option key={role.value || "none"} value={role.value}>
                            {role.label}
                          </option>
                        ))}
                      </select>
                      <input
                        value={item.weight}
                        aria-label="参考图权重"
                        placeholder="权重"
                        style={{
                          width: "48px",
                          padding: "3px 6px",
                          fontSize: "11.5px",
                        }}
                        onChange={(event) =>
                          draft.patch({
                            references: draft.references.map((entry) =>
                              entry.path === item.path
                                ? { ...entry, weight: event.target.value }
                                : entry,
                            ),
                          })
                        }
                      />
                      <input
                        value={item.description}
                        aria-label="参考图说明"
                        placeholder="说明(如角色/风格)"
                        style={{
                          flex: 1,
                          minWidth: "80px",
                          padding: "3px 6px",
                          fontSize: "11.5px",
                        }}
                        onChange={(event) =>
                          draft.patch({
                            references: draft.references.map((entry) =>
                              entry.path === item.path
                                ? { ...entry, description: event.target.value }
                                : entry,
                            ),
                          })
                        }
                      />
                      <button
                        type="button"
                        style={{ padding: "3px 7px", fontSize: "11.5px" }}
                        onClick={() =>
                          draft.patch({
                            references: draft.references.filter(
                              (entry) => entry.path !== item.path,
                            ),
                          })
                        }
                      >
                        移除
                      </button>
                    </div>
                  ))
                )}
              </div>
            ) : null}
            {allowMask && draft.sourcePath ? (
              <div>
                <h2>蒙版</h2>
                {draft.maskPath && !maskBase64 ? (
                  <p className="muted">
                    将使用已保存的蒙版。
                    <button
                      type="button"
                      onClick={() => draft.patch({ maskPath: null })}
                    >
                      不用这张蒙版
                    </button>
                  </p>
                ) : null}
                <MaskEditor
                  sourcePath={draft.sourcePath}
                  onPaint={(value) => {
                    setMaskBase64(value);
                    if (value) {
                      draft.patch({ maskPath: null });
                    }
                  }}
                />
              </div>
            ) : null}
            <details className="fold">
              <summary>请求扩展 · Extra JSON</summary>
              <label className="field" htmlFor="image-extra">
                <span>Extra JSON</span>
                <textarea
                  id="image-extra"
                  rows={3}
                  value={draft.extraText}
                  onChange={(event) =>
                    draft.patch({ extraText: event.target.value })
                  }
                  placeholder="{}"
                />
              </label>
            </details>
            {!extra.ok ? <p className="muted">{extra.message}</p> : null}
            {!weightsOk ? <p className="muted">参考图权重无效</p> : null}
            {!runnable ? (
              <p className="muted">
                这个 Provider 还不能执行图像生成。可用的是 OpenAI、OpenAI
                Compatible 和 ComfyUI。
              </p>
            ) : null}
            {error ? <ErrorNote error={error} /> : null}
            <div className="inline-actions" style={{ alignItems: "center" }}>
              <CostEstimationHint
                modality="image"
                modelId={model.modelId}
                providerId={provider?.id}
                mediaCount={1}
              />
              <button
                type="button"
                className="primary"
                disabled={blocked}
                onClick={() => void onGenerate()}
              >
                {isGenerating ? "生成中…" : "生成"}
              </button>
              {isGenerating ? (
                <button
                  type="button"
                  className="danger"
                  onClick={() => void onCancel()}
                >
                  取消
                </button>
              ) : null}
            </div>
            <TaskStatus task={activeTask} />
            {result ? (
              <article className="result">
                <div className="image-gallery">
                  {result.images.map((image) => (
                    <figure key={image.path}>
                      <ImageThumb path={image.path} alt="生成结果" />
                      <figcaption>{fileName(image.path)}</figcaption>
                      <div
                        style={{
                          display: "flex",
                          flexWrap: "wrap",
                          gap: "6px",
                          marginTop: "8px",
                        }}
                      >
                        <button
                          type="button"
                          className="linkish"
                          onClick={() => {
                            draft.patch({
                              sourcePath: image.path,
                              maskPath: null,
                            });
                            setMaskBase64(null);
                          }}
                        >
                          设为主图
                        </button>
                        <button
                          type="button"
                          className="linkish"
                          onClick={() => {
                            if (
                              draft.references.some(
                                (r) => r.path === image.path,
                              )
                            ) {
                              return;
                            }
                            draft.patch({
                              references: [
                                ...draft.references,
                                {
                                  path: image.path,
                                  role: "General",
                                  weight: "1.0",
                                  description: "前次生成结果",
                                  enabled: true,
                                },
                              ],
                            });
                          }}
                        >
                          加为参考图
                        </button>
                        {allowMask && (
                          <button
                            type="button"
                            className="linkish"
                            onClick={() => {
                              draft.patch({
                                sourcePath: image.path,
                                maskPath: null,
                              });
                              setMaskBase64(null);
                            }}
                          >
                            局部重绘
                          </button>
                        )}
                        <button
                          type="button"
                          className="linkish"
                          onClick={() => {
                            draft.patch({
                              sourcePath: image.path,
                              maskPath: null,
                            });
                            setMaskBase64(null);
                            if (draft.values.seed != null) {
                              draft.patch({
                                values: {
                                  ...draft.values,
                                  seed: Number(draft.values.seed) + 1,
                                },
                              });
                            }
                            void onGenerate();
                          }}
                        >
                          微调 (Variation)
                        </button>
                        <button
                          type="button"
                          className="linkish"
                          onClick={() => {
                            const w = Number(draft.values.width || 1024);
                            const h = Number(draft.values.height || 1024);
                            draft.patch({
                              sourcePath: image.path,
                              values: {
                                ...draft.values,
                                width: Math.min(2048, Math.round(w * 1.5)),
                                height: Math.min(2048, Math.round(h * 1.5)),
                              },
                            });
                          }}
                        >
                          放大 (Upscale)
                        </button>
                      </div>
                    </figure>
                  ))}
                </div>
                <dl className="meta">
                  <div>
                    <dt>耗时</dt>
                    <dd>{result.latencyMs} ms</dd>
                  </div>
                  <div>
                    <dt>状态</dt>
                    <dd>{result.status}</dd>
                  </div>
                </dl>
              </article>
            ) : null}
          </div>
        )}
        <aside className="parameters" aria-label="图像参数">
          <PresetBar
            modelId={model.id}
            values={draft.values}
            userTemplate={draft.prompt}
            onApply={(next) => draft.patch({ values: next })}
            onApplyPreset={(p) => {
              if (p.userTemplate) {
                draft.patch({ prompt: p.userTemplate });
              }
            }}
          />
          <ParameterPanel
            parameters={scalars}
            constraints={model.constraints}
            values={draft.values}
            onChange={(values) => draft.patch({ values })}
            chooseFiles={chooseFiles}
          />
        </aside>
      </div>
    </section>
  );
}
