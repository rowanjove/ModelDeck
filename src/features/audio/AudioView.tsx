import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { api, asAppError } from "../../api";
import { AudioThumb } from "../../components/AudioThumb";
import { ErrorNote } from "../../components/ErrorNote";
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
  supportsAudioTask,
} from "../../lib/schema";
import { useTaskQueueStore, useTaskState } from "../../lib/taskQueue";
import { useAudioDraft, useModelPick, useRestore } from "../../store";
import type {
  AppError,
  AudioResult,
  AudioTask,
  ParameterDefinition,
  Voice,
} from "../../types";
import { SttResultViewer } from "./SttResultViewer";
import { TtsControlsBar } from "./TtsControlsBar";
import { VoiceLibraryModal } from "./VoiceLibraryModal";

const AUDIO_MODES: Array<{ id: AudioTask; capability: string; label: string }> =
  [
    { id: "tts", capability: "textToSpeech", label: "文本转语音" },
    { id: "stt", capability: "speechToText", label: "语音转文本" },
    { id: "sts", capability: "speechToSpeech", label: "语音转语音" },
    { id: "clone", capability: "voiceClone", label: "声音克隆" },
  ];

export function AudioView({
  fresh,
  onOpenModels,
}: {
  fresh: number;
  onOpenModels: () => void;
}) {
  const draft = useAudioDraft();
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
  const voices = useQuery({ queryKey: ["voices"], queryFn: api.voices });
  const [error, setError] = useState<AppError | null>(null);
  const [result, setResult] = useState<AudioResult | null>(null);
  const [showVoiceLibrary, setShowVoiceLibrary] = useState(false);
  const [libraryName, setLibraryName] = useState("");
  const [libraryTranscript, setLibraryTranscript] = useState("");
  const [libraryTags, setLibraryTags] = useState("");
  const [libraryPath, setLibraryPath] = useState<string | null>(null);
  const [libraryError, setLibraryError] = useState<AppError | null>(null);

  const audioModels = (models.data ?? []).filter((model) => {
    if (model.modality !== "audio" || !model.enabled) {
      return false;
    }
    const provider = providers.data?.find(
      (item) => item.id === model.providerId,
    );
    return Boolean(provider?.enabled);
  });
  const model =
    audioModels.find((item) => item.id === draft.modelDefinitionId) ??
    audioModels[0] ??
    null;
  const provider = providers.data?.find(
    (item) => item.id === model?.providerId,
  );
  const scalars = model ? scalarParameters(model.parameters) : [];
  const extra = parseExtra(draft.extraText);
  const issues = model
    ? issuesFor(scalars, model.constraints, draft.values)
    : [];
  const allowReference = model
    ? capabilityEnabled(model.capabilities, "voiceClone")
    : false;
  const available = AUDIO_MODES.filter((mode) =>
    model ? capabilityEnabled(model.capabilities, mode.capability) : false,
  );
  const task = available.some((mode) => mode.id === draft.task)
    ? draft.task
    : (available[0]?.id ?? "tts");
  const showSource = task === "stt" || task === "sts";
  const showReference = task === "clone" || (task === "tts" && allowReference);
  const needsText = task === "tts" || task === "clone";
  const runnable = supportsAudioTask(provider?.type, task);

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
      restore.kind !== "audio" ||
      restore.nonce === seenRestore.current
    ) {
      return;
    }
    seenRestore.current = restore.nonce;
    useAudioDraft.getState().patch({
      modelDefinitionId: restore.modelDefinitionId,
      boundModelId: restore.modelDefinitionId,
      prompt: restore.prompt,
      values: restore.parameters,
      extraText: restore.extraText,
      voiceId: null,
      voiceName: restore.voiceName ?? "",
      task: restore.task ?? "tts",
      sourcePath: restore.sourcePath ?? null,
      referencePath: restore.referencePath ?? null,
      transcript: restore.transcript ?? "",
    });
    useRestore.getState().clear();
  }, [restore]);

  useEffect(() => {
    if (!model) {
      return;
    }
    const current = useAudioDraft.getState();
    const requested = current.modelDefinitionId;
    const requestedExists = (models.data ?? []).some((item) => {
      if (item.id !== requested || item.modality !== "audio" || !item.enabled) {
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
    const clone = capabilityEnabled(model.capabilities, "voiceClone");
    current.patch({
      modelDefinitionId: model.id,
      boundModelId: model.id,
      values: initialParameterValues(model.parameters),
      referencePath: clone ? current.referencePath : null,
      transcript: clone ? current.transcript : "",
    });
  }, [model, models.data, providers.data]);

  useEffect(() => {
    if (!pick || pick.modality !== "audio" || pick.nonce === seenPick.current) {
      return;
    }
    seenPick.current = pick.nonce;
    useAudioDraft.getState().patch({ modelDefinitionId: pick.modelId });
  }, [pick]);

  useEffect(() => {
    if (!model || available.length === 0 || draft.task === task) {
      return;
    }
    draft.patch({ task });
  }, [available.length, draft, model, task]);

  const activeTask = useTaskState(draft.activeTaskId);

  useEffect(() => {
    if (!activeTask) return;
    if (activeTask.eventType === "completed" && activeTask.outputAsset) {
      setResult(activeTask.outputAsset as AudioResult);
      setError(null);
      void queryClient.invalidateQueries({ queryKey: ["history"] });
      useAudioDraft.getState().patch({ activeTaskId: null });
    } else if (activeTask.eventType === "failed") {
      setError({
        code: "task_failed",
        message: activeTask.error || activeTask.message || "音频生成失败",
      });
      void queryClient.invalidateQueries({ queryKey: ["history"] });
      useAudioDraft.getState().patch({ activeTaskId: null });
    } else if (activeTask.eventType === "cancelled") {
      setError({
        code: "cancelled",
        message: "任务已取消",
      });
      useAudioDraft.getState().patch({ activeTaskId: null });
    }
  }, [activeTask, queryClient]);

  const generate = useMutation({
    mutationFn: async (input: Parameters<typeof api.generateAudio>[0]) => {
      const taskId = crypto.randomUUID();
      useAudioDraft.getState().patch({ activeTaskId: taskId });
      setResult(null);
      setError(null);
      await api.tasksSubmitAudio(input, taskId);
      return taskId;
    },
    onError: (reason) => {
      useAudioDraft.getState().patch({ activeTaskId: null });
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
      useAudioDraft.getState().patch({ activeTaskId: null });
    }
  }
  const saveVoice = useMutation({
    mutationFn: api.createVoice,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["voices"] });
      setLibraryName("");
      setLibraryTranscript("");
      setLibraryTags("");
      setLibraryPath(null);
      setLibraryError(null);
    },
    onError: (reason) => setLibraryError(asAppError(reason)),
  });
  const removeVoice = useMutation({
    mutationFn: api.deleteVoice,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["voices"] });
      if (draft.voiceId) {
        draft.patch({ voiceId: null, voiceName: "" });
      }
    },
    onError: (reason) => setLibraryError(asAppError(reason)),
  });

  function applyVoice(voice: Voice | null) {
    if (!voice) {
      draft.patch({ voiceId: null, voiceName: "" });
      return;
    }
    const voiceParameter = model?.parameters.find(
      (item) => item.key === "voice",
    );
    const values = { ...draft.values };
    if (voiceParameter && voiceFits(voiceParameter, voice.name)) {
      values.voice = voice.name;
    }
    if (voice.language) {
      values.language = voice.language;
    }
    const useAsRef = allowReference || task === "clone" || task === "sts";
    draft.patch({
      voiceId: voice.id,
      voiceName: voice.name,
      values,
      referencePath: useAsRef ? voice.filePath : draft.referencePath,
      transcript:
        allowReference || task === "clone"
          ? voice.transcript
          : draft.transcript,
    });
  }

  async function onGenerate() {
    if (
      !model ||
      !extra.ok ||
      issues.length > 0 ||
      !runnable ||
      available.length === 0 ||
      (needsText && !draft.prompt.trim()) ||
      (showSource && !draft.sourcePath) ||
      (task === "clone" && !draft.referencePath)
    ) {
      return;
    }
    const parameterVoice =
      typeof draft.values.voice === "string" ? draft.values.voice : null;
    await generate.mutateAsync({
      modelDefinitionId: model.id,
      task,
      text: draft.prompt,
      voice: draft.voiceName.trim() || parameterVoice,
      sourceAudio:
        showSource && draft.sourcePath ? { path: draft.sourcePath } : null,
      referenceAudio:
        showReference && draft.referencePath
          ? { path: draft.referencePath }
          : null,
      referenceTranscript:
        showReference && draft.transcript.trim() ? draft.transcript : null,
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

  if (providers.isLoading || models.isLoading || voices.isLoading) {
    return <p className="page-message">正在读取模型…</p>;
  }
  if (providers.error || models.error || voices.error) {
    return (
      <ErrorNote
        error={asAppError(providers.error ?? models.error ?? voices.error)}
      />
    );
  }
  if (!model) {
    return (
      <section className="page-message">
        <h1>音频生成</h1>
        <p>
          还没有可用的音频模型。添加 OpenAI、OpenAI Compatible、Generic
          HTTP、Replicate 或 fal.ai，再写模型定义。
        </p>
        <button type="button" className="primary" onClick={onOpenModels}>
          去配置模型
        </button>
      </section>
    );
  }

  const blocked =
    available.length === 0 ||
    (needsText && !draft.prompt.trim()) ||
    (showSource && !draft.sourcePath) ||
    (task === "clone" && !draft.referencePath) ||
    !extra.ok ||
    issues.length > 0 ||
    !runnable ||
    isGenerating;
  const library = voices.data ?? [];

  return (
    <section className="workspace" ref={workspaceRef}>
      <header className="topbar">
        <div>
          <p className="eyebrow">Audio</p>
          <h1>音频生成</h1>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
          <button
            type="button"
            className="button secondary"
            onClick={() => setShowVoiceLibrary(true)}
            title="管理音色库、参考样本与语言标签"
          >
            声音库 ({library.length})
          </button>
          <label className="model-pick">
            <span>模型</span>
            <select
              value={model.id}
              onChange={(event) =>
                draft.patch({ modelDefinitionId: event.target.value })
              }
            >
              {audioModels.map((item) => {
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
        </div>
      </header>
      <div className="split media-split">
        <div className="composer">
          {available.length > 1 ? (
            <div className="inline-actions">
              {available.map((mode) => (
                <button
                  key={mode.id}
                  type="button"
                  className={mode.id === task ? "primary" : undefined}
                  onClick={() => draft.patch({ task: mode.id })}
                >
                  {mode.label}
                </button>
              ))}
            </div>
          ) : null}
          <label className="field grow" htmlFor="audio-text">
            <span>{needsText ? "文本" : "提示词"}</span>
            <textarea
              id="audio-text"
              rows={6}
              value={draft.prompt}
              onChange={(event) => draft.patch({ prompt: event.target.value })}
              placeholder={
                needsText ? "要朗读的文本" : "可选，帮助模型理解这段音频"
              }
            />
          </label>
          {task === "stt" ? (
            <div
              style={{
                display: "flex",
                gap: "14px",
                flexWrap: "wrap",
                padding: "8px 12px",
                background: "var(--bg-secondary, #222)",
                borderRadius: "6px",
                fontSize: "12px",
              }}
            >
              <label
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: "6px",
                  cursor: "pointer",
                }}
              >
                <input
                  type="checkbox"
                  checked={draft.values.timestamp !== false}
                  onChange={(e) =>
                    draft.patch({
                      values: { ...draft.values, timestamp: e.target.checked },
                    })
                  }
                />
                <span>输出时间戳 (Timestamp)</span>
              </label>
              <label
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: "6px",
                  cursor: "pointer",
                }}
              >
                <input
                  type="checkbox"
                  checked={Boolean(draft.values.word_timestamps)}
                  onChange={(e) =>
                    draft.patch({
                      values: {
                        ...draft.values,
                        word_timestamps: e.target.checked,
                      },
                    })
                  }
                />
                <span>词级时间戳 (Word-level)</span>
              </label>
              <label
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: "6px",
                  cursor: "pointer",
                }}
              >
                <input
                  type="checkbox"
                  checked={Boolean(draft.values.speaker)}
                  onChange={(e) =>
                    draft.patch({
                      values: { ...draft.values, speaker: e.target.checked },
                    })
                  }
                />
                <span>说话人区分 (Speaker)</span>
              </label>
            </div>
          ) : (
            <TtsControlsBar
              values={draft.values}
              parameters={scalars}
              prompt={draft.prompt}
              onChangeValues={(next) => draft.patch({ values: next })}
              onChangePrompt={(next) => draft.patch({ prompt: next })}
            />
          )}
          {showSource ? (
            <div className="file-list">
              <div className="file-list-head">
                <span>源音频</span>
                <button
                  type="button"
                  onClick={() => {
                    void chooseFiles("audio", false).then((picked) => {
                      if (picked[0]) {
                        draft.patch({ sourcePath: picked[0] });
                      }
                    });
                  }}
                >
                  选择
                </button>
              </div>
              {draft.sourcePath ? (
                <div className="reference-row">
                  <span title={draft.sourcePath}>
                    {fileName(draft.sourcePath)}
                  </span>
                  <button
                    type="button"
                    onClick={() => draft.patch({ sourcePath: null })}
                  >
                    移除
                  </button>
                </div>
              ) : (
                <p className="muted">未选择</p>
              )}
            </div>
          ) : null}
          <label className="field" htmlFor="audio-voice">
            <span>声音库</span>
            <select
              id="audio-voice"
              value={draft.voiceId ?? ""}
              onChange={(event) => {
                const voice =
                  library.find((item) => item.id === event.target.value) ??
                  null;
                applyVoice(voice);
              }}
            >
              <option value="">不指定</option>
              {library.map((voice) => (
                <option key={voice.id} value={voice.id}>
                  {voice.name}
                  {voice.tags ? ` · ${voice.tags}` : ""}
                </option>
              ))}
            </select>
          </label>
          {showReference ? (
            <>
              <div className="file-list">
                <div className="file-list-head">
                  <span>参考音频</span>
                  <button
                    type="button"
                    onClick={() => {
                      void chooseFiles("audio", false).then((picked) => {
                        if (picked[0]) {
                          draft.patch({ referencePath: picked[0] });
                        }
                      });
                    }}
                  >
                    选择
                  </button>
                </div>
                {draft.referencePath ? (
                  <div className="reference-row">
                    <span title={draft.referencePath}>
                      {fileName(draft.referencePath)}
                    </span>
                    <button
                      type="button"
                      onClick={() => draft.patch({ referencePath: null })}
                    >
                      移除
                    </button>
                  </div>
                ) : (
                  <p className="muted">未选择</p>
                )}
              </div>
              <label className="field" htmlFor="audio-transcript">
                <span>参考文本</span>
                <textarea
                  id="audio-transcript"
                  rows={3}
                  value={draft.transcript}
                  onChange={(event) =>
                    draft.patch({ transcript: event.target.value })
                  }
                  placeholder="参考音频里说的话"
                />
              </label>
            </>
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
            <label className="field" htmlFor="audio-extra">
              <span>Extra JSON</span>
              <textarea
                id="audio-extra"
                rows={3}
                value={draft.extraText}
                onChange={(event) =>
                  draft.patch({ extraText: event.target.value })
                }
                placeholder="{}"
              />
            </label>
          </details>
          {extra.ok ? null : <p className="muted">{extra.message}</p>}
          {available.length === 0 ? (
            <p className="muted">当前模型没有打开音频能力。</p>
          ) : null}
          {runnable ? null : (
            <p className="muted">
              {task === "sts" || task === "clone"
                ? "这个音频任务目前由 Generic HTTP、Replicate 与 fal.ai 执行。"
                : "这个音频任务目前由 OpenAI、OpenAI Compatible、Generic HTTP、Replicate 与 fal.ai 执行。"}
            </p>
          )}
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
        <div className="stage">
          <section>
            <h2>声音库</h2>
            <p className="muted">
              保存名字、参考音频、参考文本和标签。生成时从上面的列表选用。
            </p>
            {library.length === 0 ? (
              <p className="muted">还没有保存的声音。</p>
            ) : null}
            <ul className="file-list">
              {library.map((voice) => (
                <li key={voice.id}>
                  <strong>{voice.name}</strong>
                  {voice.tags ? ` · ${voice.tags}` : ""}
                  <span> {fileName(voice.filePath)}</span>
                  <button
                    type="button"
                    onClick={() => {
                      if (window.confirm(`删除声音「${voice.name}」？`)) {
                        void removeVoice.mutateAsync(voice.id);
                      }
                    }}
                  >
                    删除
                  </button>
                </li>
              ))}
            </ul>
            <label className="field">
              名称
              <input
                value={libraryName}
                onChange={(event) => setLibraryName(event.target.value)}
              />
            </label>
            <div className="file-list">
              <div className="file-list-head">
                <span>参考音频</span>
                <button
                  type="button"
                  onClick={() => {
                    void chooseFiles("audio", false).then((picked) => {
                      if (picked[0]) {
                        setLibraryPath(picked[0]);
                      }
                    });
                  }}
                >
                  选择
                </button>
              </div>
              <p className="muted">
                {libraryPath ? fileName(libraryPath) : "未选择"}
              </p>
            </div>
            <label className="field">
              参考文本
              <textarea
                rows={2}
                value={libraryTranscript}
                onChange={(event) => setLibraryTranscript(event.target.value)}
              />
            </label>
            <label className="field">
              标签
              <input
                value={libraryTags}
                onChange={(event) => setLibraryTags(event.target.value)}
                placeholder="warm, narrator"
              />
            </label>
            <button
              type="button"
              disabled={
                !libraryName.trim() || !libraryPath || saveVoice.isPending
              }
              onClick={() => {
                if (!libraryPath) {
                  return;
                }
                void saveVoice.mutateAsync({
                  name: libraryName.trim(),
                  sourcePath: libraryPath,
                  transcript: libraryTranscript,
                  tags: libraryTags,
                });
              }}
            >
              {saveVoice.isPending ? "保存中…" : "保存到声音库"}
            </button>
            {libraryError ? <ErrorNote error={libraryError} /> : null}
          </section>
          <section>
            <h2>结果</h2>
            {task === "stt" && result ? (
              <SttResultViewer result={result} />
            ) : result?.transcript ? (
              <pre className="history-output">{result.transcript}</pre>
            ) : null}
            {result && result.audios.length > 0 ? (
              <div className="image-gallery">
                {result.audios.map((audio) => (
                  <figure key={audio.path}>
                    <AudioThumb path={audio.path} />
                    <figcaption>{fileName(audio.path)}</figcaption>
                  </figure>
                ))}
              </div>
            ) : (task === "stt" && result) || result?.transcript ? null : (
              <p className="muted">生成后在这里播放。</p>
            )}
          </section>
        </div>
      </div>
      {showVoiceLibrary ? (
        <VoiceLibraryModal
          voices={library}
          activeVoiceId={draft.voiceId}
          onSelectVoice={(v) => applyVoice(v)}
          onClose={() => setShowVoiceLibrary(false)}
        />
      ) : null}
    </section>
  );
}

function voiceFits(parameter: ParameterDefinition, name: string): boolean {
  if (!parameter.options || parameter.options.length === 0) {
    return parameter.type !== "select";
  }
  return parameter.options.some((option) => option.value === name);
}
