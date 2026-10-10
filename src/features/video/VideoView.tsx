import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { api, asAppError } from "../../api";
import { ErrorNote } from "../../components/ErrorNote";
import { ImageThumb } from "../../components/ImageThumb";
import { ParameterPanel } from "../../components/ParameterPanel";
import { PresetBar } from "../../components/PresetBar";
import { TaskStatus } from "../../components/TaskStatus";
import { VideoThumb } from "../../components/VideoThumb";
import { chooseFiles } from "../../lib/files";
import {
  capabilityEnabled,
  capabilityLimit,
  collectParameters,
  constraintMessages,
  fileName,
  initialParameterValues,
  issuesFor,
  parseExtra,
  scalarParameters,
  supportsVideoGenerate,
} from "../../lib/schema";
import { useTaskQueueStore, useTaskState } from "../../lib/taskQueue";
import { captureVideoFrame } from "../../lib/video";
import { useModelPick, useRestore, useVideoDraft } from "../../store";
import type { AppError, ParameterDefinition, VideoResult } from "../../types";
import { ComfyQueuePanel } from "../comfyui/ComfyQueuePanel";
import { ShotPresetBar } from "./ShotPresetBar";
import { StoryboardView } from "./StoryboardView";

export interface MediaItemDraft {
  path: string;
  role: string;
  weight: string;
  description: string;
  enabled: boolean;
}

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

export function VideoView({
  fresh,
  onOpenModels,
}: {
  fresh: number;
  onOpenModels: () => void;
}) {
  const draft = useVideoDraft();
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
  const [result, setResult] = useState<VideoResult | null>(null);
  const [mode, setMode] = useState<"single" | "storyboard">("single");
  const [firstEnabled, setFirstEnabled] = useState(true);
  const [lastEnabled, setLastEnabled] = useState(true);
  const [clipItems, setClipItems] = useState<MediaItemDraft[]>([]);
  const [audioItems, setAudioItems] = useState<MediaItemDraft[]>([]);

  const videoModels = (models.data ?? []).filter((model) => {
    if (model.modality !== "video" || !model.enabled) {
      return false;
    }
    const provider = providers.data?.find(
      (item) => item.id === model.providerId,
    );
    return Boolean(provider?.enabled);
  });
  const model =
    videoModels.find((item) => item.id === draft.modelDefinitionId) ??
    videoModels[0] ??
    null;
  const provider = providers.data?.find(
    (item) => item.id === model?.providerId,
  );
  const scalars = model ? scalarParameters(model.parameters) : [];
  const extra = parseExtra(draft.extraText);
  const allowClips = model
    ? capabilityEnabled(model.capabilities, "referenceVideos")
    : false;
  const clipLimit = model
    ? capabilityLimit(model.capabilities, "referenceVideos", 4)
    : 4;
  const allowAudio = model
    ? capabilityEnabled(model.capabilities, "referenceAudio")
    : false;
  const audioLimit = model
    ? capabilityLimit(model.capabilities, "referenceAudio", 4)
    : 4;
  const mediaLabels: ParameterDefinition[] = [
    ...scalars,
    { key: "firstFrame", label: "首帧", type: "image" },
    { key: "lastFrame", label: "尾帧", type: "image" },
    { key: "referenceImages", label: "参考图", type: "images" },
    { key: "referenceVideos", label: "参考视频", type: "videos" },
    { key: "referenceAudio", label: "参考音频", type: "audios" },
  ];
  const mediaValues: Record<string, unknown> = { ...draft.values };
  if (draft.firstPath) {
    mediaValues.firstFrame = draft.firstPath;
  }
  if (draft.lastPath) {
    mediaValues.lastFrame = draft.lastPath;
  }
  if (draft.references.length > 0) {
    mediaValues.referenceImages = draft.references;
  }
  if (draft.videoPaths.length > 0) {
    mediaValues.referenceVideos = draft.videoPaths;
  }
  if (draft.audioPaths.length > 0) {
    mediaValues.referenceAudio = draft.audioPaths;
  }
  const issues = model
    ? [
        ...issuesFor(scalars, model.constraints, draft.values).filter(
          (issue) => !issue.includes("不能同时设置"),
        ),
        ...constraintMessages(model.constraints, mediaValues, mediaLabels),
      ]
    : [];
  const runnable = supportsVideoGenerate(provider?.type);
  const allowFirst = model
    ? capabilityEnabled(model.capabilities, "firstFrame") ||
      capabilityEnabled(model.capabilities, "imageToVideo")
    : false;
  const allowLast = model
    ? capabilityEnabled(model.capabilities, "lastFrame")
    : false;
  const allowReferences = model
    ? capabilityEnabled(model.capabilities, "referenceImages")
    : false;
  const referenceLimit = model
    ? capabilityLimit(model.capabilities, "referenceImages", 8)
    : 8;
  const promptOptional =
    provider?.type === "comfyui" || provider?.type === "generic-http";

  useEffect(() => {
    if (fresh < 0) {
      return;
    }
    setResult(null);
    setError(null);
  }, [fresh]);

  useEffect(() => {
    if (
      !restore ||
      restore.kind !== "video" ||
      restore.nonce === seenRestore.current
    ) {
      return;
    }
    seenRestore.current = restore.nonce;
    useVideoDraft.getState().patch({
      modelDefinitionId: restore.modelDefinitionId,
      boundModelId: restore.modelDefinitionId,
      prompt: restore.prompt,
      negativePrompt: restore.negativePrompt ?? "",
      values: restore.parameters,
      extraText: restore.extraText,
      firstPath: restore.firstPath ?? null,
      lastPath: restore.lastPath ?? null,
      references: restore.references ?? [],
      videoPaths: restore.videoPaths ?? [],
      audioPaths: restore.audioPaths ?? [],
    });
    useRestore.getState().clear();
  }, [restore]);

  useEffect(() => {
    if (!model) {
      return;
    }
    const current = useVideoDraft.getState();
    const requested = current.modelDefinitionId;
    const requestedExists = (models.data ?? []).some((item) => {
      if (item.id !== requested || item.modality !== "video" || !item.enabled) {
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
    const firstOk =
      capabilityEnabled(model.capabilities, "firstFrame") ||
      capabilityEnabled(model.capabilities, "imageToVideo");
    current.patch({
      modelDefinitionId: model.id,
      boundModelId: model.id,
      values: initialParameterValues(model.parameters),
      firstPath: firstOk ? current.firstPath : null,
      lastPath: capabilityEnabled(model.capabilities, "lastFrame")
        ? current.lastPath
        : null,
      references: capabilityEnabled(model.capabilities, "referenceImages")
        ? current.references
        : [],
      videoPaths: capabilityEnabled(model.capabilities, "referenceVideos")
        ? current.videoPaths
        : [],
      audioPaths: capabilityEnabled(model.capabilities, "referenceAudio")
        ? current.audioPaths
        : [],
    });
  }, [model, models.data, providers.data]);

  useEffect(() => {
    if (!pick || pick.modality !== "video" || pick.nonce === seenPick.current) {
      return;
    }
    seenPick.current = pick.nonce;
    useVideoDraft.getState().patch({ modelDefinitionId: pick.modelId });
  }, [pick]);

  useEffect(() => {
    setClipItems((prev) => {
      const map = new Map(prev.map((i) => [i.path, i]));
      return draft.videoPaths.map(
        (path) =>
          map.get(path) ?? {
            path,
            role: "",
            weight: "1.0",
            description: "",
            enabled: true,
          },
      );
    });
  }, [draft.videoPaths]);

  useEffect(() => {
    setAudioItems((prev) => {
      const map = new Map(prev.map((i) => [i.path, i]));
      return draft.audioPaths.map(
        (path) =>
          map.get(path) ?? {
            path,
            role: "",
            weight: "1.0",
            description: "",
            enabled: true,
          },
      );
    });
  }, [draft.audioPaths]);

  const activeTask = useTaskState(draft.activeTaskId);

  useEffect(() => {
    if (!activeTask) return;
    if (activeTask.eventType === "completed" && activeTask.outputAsset) {
      setResult(activeTask.outputAsset as VideoResult);
      setError(null);
      void queryClient.invalidateQueries({ queryKey: ["history"] });
      useVideoDraft.getState().patch({ activeTaskId: null });
    } else if (activeTask.eventType === "failed") {
      setError({
        code: "task_failed",
        message: activeTask.error || activeTask.message || "视频生成失败",
      });
      void queryClient.invalidateQueries({ queryKey: ["history"] });
      useVideoDraft.getState().patch({ activeTaskId: null });
    } else if (activeTask.eventType === "cancelled") {
      setError({
        code: "cancelled",
        message: "任务已取消",
      });
      useVideoDraft.getState().patch({ activeTaskId: null });
    }
  }, [activeTask, queryClient]);

  const generate = useMutation({
    mutationFn: async (input: Parameters<typeof api.generateVideo>[0]) => {
      const taskId = crypto.randomUUID();
      useVideoDraft.getState().patch({ activeTaskId: taskId });
      setResult(null);
      setError(null);
      await api.tasksSubmitVideo(input, taskId);
      return taskId;
    },
    onError: (reason) => {
      useVideoDraft.getState().patch({ activeTaskId: null });
      setError(asAppError(reason));
      void queryClient.invalidateQueries({ queryKey: ["history"] });
    },
  });

  const isGenerating = Boolean(
    generate.isPending ||
      (draft.activeTaskId &&
        activeTask &&
        !["completed", "failed", "cancelled"].includes(activeTask.status)),
  );

  async function onCancel() {
    if (draft.activeTaskId) {
      await useTaskQueueStore.getState().cancelTask(draft.activeTaskId);
      useVideoDraft.getState().patch({ activeTaskId: null });
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
      firstFrame:
        allowFirst && draft.firstPath && firstEnabled
          ? { path: draft.firstPath, enabled: true }
          : null,
      lastFrame:
        allowLast && draft.lastPath && lastEnabled
          ? { path: draft.lastPath, enabled: true }
          : null,
      referenceImages: allowReferences
        ? draft.references.slice(0, referenceLimit).map((item) => ({
            path: item.path,
            role: item.role || null,
            weight: item.weight.trim() ? Number(item.weight) : null,
            description: item.description.trim() || null,
            enabled: item.enabled ?? true,
          }))
        : [],
      referenceVideos: allowClips
        ? clipItems.slice(0, clipLimit).map((item) => ({
            path: item.path,
            role: item.role || null,
            weight: item.weight.trim() ? Number(item.weight) : null,
            description: item.description.trim() || null,
            enabled: item.enabled,
          }))
        : [],
      referenceAudios: allowAudio
        ? audioItems.slice(0, audioLimit).map((item) => ({
            path: item.path,
            role: item.role || null,
            weight: item.weight.trim() ? Number(item.weight) : null,
            description: item.description.trim() || null,
            enabled: item.enabled,
          }))
        : [],
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
        <h1>视频生成</h1>
        <p>
          还没有可用的视频模型。添加 ComfyUI、Generic HTTP、Replicate 或
          fal.ai，再写模型定义。
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
          <p className="eyebrow">Video</p>
          <h1>视频生成</h1>
        </div>
        <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
          <button
            type="button"
            className={mode === "single" ? "primary" : ""}
            onClick={() => setMode("single")}
          >
            单镜头生成
          </button>
          <button
            type="button"
            className={mode === "storyboard" ? "primary" : ""}
            onClick={() => setMode("storyboard")}
          >
            分镜故事板 (Storyboard)
          </button>
        </div>
        <label className="model-pick">
          <span>模型</span>
          <select
            value={model.id}
            onChange={(event) =>
              draft.patch({ modelDefinitionId: event.target.value })
            }
          >
            {videoModels.map((item) => {
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
      {mode === "storyboard" ? (
        <StoryboardView
          model={model}
          onSelectShotForEditor={(shot) => {
            setMode("single");
            draft.patch({
              prompt: shot.prompt,
              firstPath: shot.firstFrame?.path ?? null,
              lastPath: shot.lastFrame?.path ?? null,
            });
            setFirstEnabled(Boolean(shot.firstFrame?.path));
            setLastEnabled(Boolean(shot.lastFrame?.path));
          }}
        />
      ) : (
        <div className="split media-split">
          <div className="composer">
            <ShotPresetBar
              modelParameters={model.parameters}
              currentValues={draft.values}
              onApplyParameters={(patch) =>
                draft.patch({ values: { ...draft.values, ...patch } })
              }
              onAppendPrompt={(tag) =>
                draft.patch({ prompt: `${draft.prompt}${tag}` })
              }
            />
            <label className="field grow" htmlFor="video-prompt">
              <span>提示词</span>
              <textarea
                id="video-prompt"
                rows={6}
                value={draft.prompt}
                onChange={(event) =>
                  draft.patch({ prompt: event.target.value })
                }
                placeholder="留空则使用工作流或 HTTP 模板里的提示词"
              />
            </label>
            <label className="field" htmlFor="video-negative">
              <span>反向提示词</span>
              <textarea
                id="video-negative"
                rows={3}
                value={draft.negativePrompt}
                onChange={(event) =>
                  draft.patch({ negativePrompt: event.target.value })
                }
                placeholder="不希望出现的内容"
              />
            </label>
            {allowFirst ? (
              <FrameField
                label="首帧"
                path={draft.firstPath}
                enabled={firstEnabled}
                onToggleEnabled={setFirstEnabled}
                onPick={(path) => {
                  draft.patch({ firstPath: path });
                  setFirstEnabled(true);
                }}
                onClear={() => draft.patch({ firstPath: null, lastPath: null })}
              />
            ) : null}
            {allowLast ? (
              <FrameField
                label="尾帧"
                path={draft.lastPath}
                enabled={lastEnabled}
                onToggleEnabled={setLastEnabled}
                onPick={(path) => {
                  draft.patch({ lastPath: path });
                  setLastEnabled(true);
                }}
                onClear={() => draft.patch({ lastPath: null })}
              />
            ) : null}
            {allowReferences ? (
              <div className="file-list">
                <div className="file-list-head">
                  <span>参考图</span>
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
                          if (next.length >= referenceLimit) {
                            break;
                          }
                          next.push({
                            path,
                            role: "",
                            weight: "",
                            description: "",
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
                  <p className="muted">未选择</p>
                ) : (
                  draft.references.slice(0, referenceLimit).map((item) => (
                    <div key={item.path} className="reference-row">
                      <span title={item.path}>{fileName(item.path)}</span>
                      <select
                        value={item.role}
                        aria-label="参考图角色"
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
                          <option key={role.label} value={role.value}>
                            {role.label}
                          </option>
                        ))}
                      </select>
                      <input
                        value={item.weight}
                        aria-label="参考图权重"
                        placeholder="权重"
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
                      <button
                        type="button"
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
            {allowClips ? (
              <PathList
                label="参考视频"
                kind="video"
                items={clipItems}
                limit={clipLimit}
                onChange={(next) => {
                  setClipItems(next);
                  draft.patch({ videoPaths: next.map((i) => i.path) });
                }}
              />
            ) : null}
            {allowAudio ? (
              <PathList
                label="参考音频"
                kind="audio"
                items={audioItems}
                limit={audioLimit}
                onChange={(next) => {
                  setAudioItems(next);
                  draft.patch({ audioPaths: next.map((i) => i.path) });
                }}
              />
            ) : null}
            <PresetBar
              modelId={model.id}
              values={draft.values}
              onApply={(next) => draft.patch({ values: next })}
            />
            <ParameterPanel
              parameters={scalars}
              constraints={model.constraints}
              values={draft.values}
              onChange={(values) => draft.patch({ values })}
              chooseFiles={chooseFiles}
            />
            <details className="fold">
              <summary>请求扩展 · Extra JSON</summary>
              <label className="field" htmlFor="video-extra">
                <span>Extra JSON</span>
                <textarea
                  id="video-extra"
                  rows={4}
                  value={draft.extraText}
                  onChange={(event) =>
                    draft.patch({ extraText: event.target.value })
                  }
                  placeholder="{}"
                />
              </label>
            </details>
            {!extra.ok ? <p className="muted">{extra.message}</p> : null}
            {!runnable ? (
              <p className="muted">
                视频生成目前由 ComfyUI、Generic HTTP、Replicate 与 fal.ai 执行。
              </p>
            ) : null}
            <div className="inline-actions">
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
            {error ? <ErrorNote error={error} /> : null}
          </div>
          <div className="stage result-stage">
            <h2>视频结果</h2>
            {!result?.videos.length ? (
              <p className="muted">
                生成的视频会显示在这里，可预览并继续编辑镜头。
              </p>
            ) : null}
            <div className="image-gallery">
              {(result?.videos ?? []).map((video) => (
                <figure key={video.path}>
                  <VideoThumb path={video.path} />
                  <figcaption>{fileName(video.path)}</figcaption>
                  <div
                    className="inline-actions"
                    style={{
                      marginTop: "8px",
                      display: "flex",
                      gap: "6px",
                      flexWrap: "wrap",
                    }}
                  >
                    <button
                      type="button"
                      className="button small primary"
                      onClick={async () => {
                        try {
                          const src = convertFileSrc(video.path);
                          const lastFrame = await captureVideoFrame(src, 0.98);
                          draft.patch({ firstPath: lastFrame, lastPath: null });
                          setFirstEnabled(true);
                        } catch (e) {
                          setError(asAppError(e));
                        }
                      }}
                      title="抓取当前视频最后一帧作为下一视频首帧"
                    >
                      ⏩ 续写下个镜头
                    </button>
                    <button
                      type="button"
                      className="button small secondary"
                      onClick={() => {
                        draft.patch({
                          firstPath: null,
                          lastPath: null,
                          prompt: "",
                        });
                      }}
                    >
                      新建镜头
                    </button>
                  </div>
                </figure>
              ))}
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function PathList({
  label,
  kind,
  items,
  limit,
  onChange,
}: {
  label: string;
  kind: "video" | "audio";
  items: MediaItemDraft[];
  limit: number;
  onChange: (items: MediaItemDraft[]) => void;
}) {
  return (
    <div className="file-list">
      <div className="file-list-head">
        <span>{label}</span>
        <button
          type="button"
          onClick={() => {
            void chooseFiles(kind, true).then((picked) => {
              if (picked.length === 0) {
                return;
              }
              const next = [...items];
              for (const path of picked) {
                if (next.some((i) => i.path === path) || next.length >= limit) {
                  continue;
                }
                next.push({
                  path,
                  role: "",
                  weight: "1.0",
                  description: "",
                  enabled: true,
                });
              }
              onChange(next);
            });
          }}
        >
          添加
        </button>
      </div>
      {items.length === 0 ? (
        <p className="muted">未选择</p>
      ) : (
        items.map((item) => (
          <div
            key={item.path}
            className="reference-row"
            style={{
              display: "flex",
              alignItems: "center",
              gap: "6px",
              flexWrap: "wrap",
              padding: "4px 0",
              borderBottom: "1px dashed var(--border-color, #333)",
            }}
          >
            <input
              type="checkbox"
              checked={item.enabled}
              onChange={(e) =>
                onChange(
                  items.map((i) =>
                    i.path === item.path
                      ? { ...i, enabled: e.target.checked }
                      : i,
                  ),
                )
              }
              title="启用/禁用"
            />
            <span
              title={item.path}
              style={{
                fontSize: "12px",
                maxWidth: "140px",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                textDecoration: item.enabled ? "none" : "line-through",
                color: item.enabled ? "inherit" : "var(--text-muted, #777)",
              }}
            >
              {fileName(item.path)}
            </span>
            <select
              value={item.role}
              aria-label={`${label}角色`}
              onChange={(e) =>
                onChange(
                  items.map((i) =>
                    i.path === item.path ? { ...i, role: e.target.value } : i,
                  ),
                )
              }
              style={{ fontSize: "11px", padding: "2px 4px" }}
            >
              {ROLES.map((role) => (
                <option key={role.value} value={role.value}>
                  {role.label}
                </option>
              ))}
            </select>
            <input
              type="number"
              min="0"
              max="10"
              step="0.1"
              value={item.weight}
              placeholder="权重"
              onChange={(e) =>
                onChange(
                  items.map((i) =>
                    i.path === item.path ? { ...i, weight: e.target.value } : i,
                  ),
                )
              }
              style={{ width: "48px", fontSize: "11px", padding: "2px 4px" }}
              title="权重"
            />
            <input
              type="text"
              value={item.description}
              placeholder="说明/提示"
              onChange={(e) =>
                onChange(
                  items.map((i) =>
                    i.path === item.path
                      ? { ...i, description: e.target.value }
                      : i,
                  ),
                )
              }
              style={{
                flex: 1,
                minWidth: "60px",
                fontSize: "11px",
                padding: "2px 4px",
              }}
            />
            <button
              type="button"
              className="button tiny danger"
              onClick={() =>
                onChange(items.filter((i) => i.path !== item.path))
              }
            >
              移除
            </button>
          </div>
        ))
      )}
    </div>
  );
}

function FrameField({
  label,
  path,
  enabled,
  onToggleEnabled,
  onPick,
  onClear,
}: {
  label: string;
  path: string | null;
  enabled: boolean;
  onToggleEnabled: (enabled: boolean) => void;
  onPick: (path: string) => void;
  onClear: () => void;
}) {
  return (
    <div className="file-list">
      <div className="file-list-head">
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: "6px",
            margin: 0,
            cursor: "pointer",
          }}
        >
          {path ? (
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => onToggleEnabled(e.target.checked)}
              title="启用/禁用"
            />
          ) : null}
          <span>{label}</span>
        </label>
        <button
          type="button"
          onClick={() => {
            void chooseFiles("image", false).then((picked) => {
              if (picked[0]) {
                onPick(picked[0]);
              }
            });
          }}
        >
          {path ? "更换" : "选择文件"}
        </button>
      </div>
      {path ? (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "10px",
            padding: "5px 8px",
            background: "var(--raised)",
            border: "1px solid var(--line)",
            borderRadius: "var(--radius-sm)",
          }}
        >
          <div
            style={{
              width: "36px",
              height: "36px",
              borderRadius: "var(--radius-xs)",
              overflow: "hidden",
              flexShrink: 0,
              background: "var(--sunken)",
            }}
          >
            <ImageThumb path={path} alt={label} />
          </div>
          <span
            style={{
              flex: 1,
              fontSize: "12px",
              textDecoration: enabled ? "none" : "line-through",
              color: enabled ? "var(--ink)" : "var(--muted)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
            title={path}
          >
            {fileName(path)}
          </span>
          <button type="button" onClick={onClear}>
            移除
          </button>
        </div>
      ) : (
        <p className="muted">未选择</p>
      )}
    </div>
  );
}
