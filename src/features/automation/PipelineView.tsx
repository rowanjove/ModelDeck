import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api } from "../../api";
import { useTaskQueueStore, useTaskState } from "../../lib/taskQueue";
import type {
  PipelineDefinition,
  PipelineExecutionResult,
  PipelineStep,
  PipelineStepResult,
} from "../../types";

export function PipelineView({
  onOpenModels,
}: {
  onOpenModels?: () => void;
}) {
  const modelsQuery = useQuery({
    queryKey: ["models"],
    queryFn: api.models,
  });

  const pipelinesQuery = useQuery({
    queryKey: ["pipelines"],
    queryFn: api.pipelinesList,
  });

  const presetsQuery = useQuery({
    queryKey: ["pipelines_presets"],
    queryFn: api.pipelinesPresets,
  });

  // Current editing pipeline
  const [currentPipeline, setCurrentPipeline] = useState<PipelineDefinition>({
    id: "",
    name: "未命名工作流",
    description: "",
    steps: [
      {
        type: "input",
        id: "step_input",
        label: "初始文本输入",
        defaultValue: "赛博朋克都市夜晚的雨中漫步，霓虹灯倒映在积水里。",
      },
      {
        type: "model",
        id: "step_summarize",
        label: "提示词提炼模型",
        modelDefinitionId: "",
        promptTemplate:
          "请为以下描述提炼一段高度生动的英文画面描写，突出细节与质感：\n\n{{input}}",
        systemPrompt: "你是一个专业的 AI 视觉艺术家与提示词工程专家。",
        parameters: {},
      },
      {
        type: "transform",
        id: "step_enhancer",
        label: "提示词增强包装",
        kind: "template",
        rule: "{{prev}}, masterpiece, highly detailed, 8k resolution, cinematic lighting",
      },
    ],
    createdAt: "",
    updatedAt: "",
  });

  // Run execution state
  const [initialInput, setInitialInput] = useState(
    "赛博朋克都市夜晚的雨中漫步，霓虹灯倒映在积水里。",
  );
  const [isRunning, setIsRunning] = useState(false);
  const [runResult, setRunResult] = useState<PipelineExecutionResult | null>(
    null,
  );
  const [runError, setRunError] = useState<string | null>(null);
  const [selectedPresetId, setSelectedPresetId] = useState<string>("");

  // Auto assign default model to steps if empty
  useEffect(() => {
    if (modelsQuery.data && modelsQuery.data.length > 0) {
      setCurrentPipeline((prev) => {
        let changed = false;
        const newSteps = prev.steps.map((s) => {
          if (s.type === "model" && !s.modelDefinitionId) {
            changed = true;
            // Match modality if possible
            const defaultModel =
              modelsQuery.data?.find((m) => m.enabled) || modelsQuery.data?.[0];
            return {
              ...s,
              modelDefinitionId: defaultModel ? defaultModel.id : "",
            };
          }
          return s;
        });
        return changed ? { ...prev, steps: newSteps } : prev;
      });
    }
  }, [modelsQuery.data]);

  const loadPreset = (preset: PipelineDefinition) => {
    setSelectedPresetId(preset.id);
    // clone preset with adapted model IDs
    const clonedSteps: PipelineStep[] = preset.steps.map((st) => {
      if (st.type === "model") {
        let matchedId = st.modelDefinitionId;
        if (!matchedId || !modelsQuery.data?.some((m) => m.id === matchedId)) {
          // If step label says image/video/tts, find suitable model
          const label = st.label.toLowerCase();
          let targetModality = "text";
          if (
            label.includes("图像") ||
            label.includes("绘图") ||
            label.includes("image")
          ) {
            targetModality = "image";
          } else if (label.includes("视频") || label.includes("video")) {
            targetModality = "video";
          } else if (
            label.includes("语音") ||
            label.includes("tts") ||
            label.includes("audio")
          ) {
            targetModality = "audio";
          }
          const suitable =
            modelsQuery.data?.find(
              (m) => m.enabled && m.modality === targetModality,
            ) ||
            modelsQuery.data?.find((m) => m.modality === targetModality) ||
            modelsQuery.data?.find((m) => m.enabled) ||
            modelsQuery.data?.[0];
          matchedId = suitable ? suitable.id : "";
        }
        return { ...st, modelDefinitionId: matchedId };
      }
      return { ...st };
    });

    const firstInput = clonedSteps.find((s) => s.type === "input");
    if (firstInput && firstInput.type === "input" && firstInput.defaultValue) {
      setInitialInput(firstInput.defaultValue);
    }

    setCurrentPipeline({
      ...preset,
      id: "", // clear ID so saving creates or saves as custom
      steps: clonedSteps,
    });
    setRunResult(null);
    setRunError(null);
  };

  const [importFeedback, setImportFeedback] = useState<string | null>(null);

  const handleExport = async () => {
    try {
      const json = await api.pipelinesExport(currentPipeline);
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${
        currentPipeline.name.replace(/\s+/g, "_") || "pipeline"
      }.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e: unknown) {
      setRunError(`导出失败: ${String(e)}`);
    }
  };

  const handleImportFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = async (evt) => {
      try {
        const text = evt.target?.result as string;
        const imported = await api.pipelinesImport(text);
        setCurrentPipeline(imported);
        setImportFeedback(`成功导入工作流:「${imported.name}」`);
        setTimeout(() => setImportFeedback(null), 3000);
      } catch (err: unknown) {
        setRunError(`导入工作流失败: ${String(err)}`);
      }
    };
    reader.readAsText(file);
    e.target.value = "";
  };

  const handleSave = async () => {
    try {
      setRunError(null);
      const saved = await api.pipelinesSave(currentPipeline);
      setCurrentPipeline(saved);
      pipelinesQuery.refetch();
    } catch (e: unknown) {
      setRunError(String(e));
    }
  };

  const handleDelete = async (id: string) => {
    if (!confirm("确定要删除此工作流吗？")) return;
    try {
      await api.pipelinesDelete(id);
      if (currentPipeline.id === id) {
        setCurrentPipeline({
          id: "",
          name: "新工作流",
          description: "",
          steps: [
            {
              type: "input",
              id: "step_input",
              label: "初始文本输入",
              defaultValue: "",
            },
          ],
          createdAt: "",
          updatedAt: "",
        });
      }
      pipelinesQuery.refetch();
    } catch (e: unknown) {
      setRunError(String(e));
    }
  };

  const [activeTaskId, setActiveTaskId] = useState<string | null>(null);
  const activeTask = useTaskState(activeTaskId);
  const [intermediateSteps, setIntermediateSteps] = useState<
    PipelineStepResult[]
  >([]);

  useEffect(() => {
    if (!activeTask) return;
    if (activeTask.eventType === "intermediate" && activeTask.outputAsset) {
      const stepResult = activeTask.outputAsset as PipelineStepResult;
      setIntermediateSteps((prev) => {
        const idx = prev.findIndex((s) => s.stepId === stepResult.stepId);
        if (idx >= 0) {
          const next = [...prev];
          next[idx] = stepResult;
          return next;
        }
        return [...prev, stepResult];
      });
    } else if (activeTask.eventType === "completed" && activeTask.outputAsset) {
      setRunResult(activeTask.outputAsset as PipelineExecutionResult);
      setIsRunning(false);
      setActiveTaskId(null);
    } else if (activeTask.eventType === "failed") {
      setRunError(activeTask.error || activeTask.message || "工作流执行失败");
      setIsRunning(false);
      setActiveTaskId(null);
    } else if (activeTask.eventType === "cancelled") {
      setRunError("工作流已取消");
      setIsRunning(false);
      setActiveTaskId(null);
    }
  }, [activeTask]);

  const handleRun = async () => {
    setIsRunning(true);
    setRunResult(null);
    setRunError(null);
    setIntermediateSteps([]);
    const taskId = crypto.randomUUID();
    setActiveTaskId(taskId);
    try {
      await api.tasksSubmitPipeline(
        {
          pipelineId: currentPipeline.id || undefined,
          steps: currentPipeline.steps,
          initialInput,
        },
        taskId,
      );
    } catch (e: unknown) {
      setRunError(String(e));
      setIsRunning(false);
      setActiveTaskId(null);
    }
  };

  const handleResume = async (resumeIndex: number) => {
    setIsRunning(true);
    setRunError(null);
    const taskId = crypto.randomUUID();
    setActiveTaskId(taskId);
    try {
      await api.tasksSubmitPipeline(
        {
          pipelineId: currentPipeline.id || undefined,
          steps: currentPipeline.steps,
          initialInput,
          resumeFromStepIndex: resumeIndex,
          cachedStepResults: intermediateSteps,
        },
        taskId,
      );
    } catch (e: unknown) {
      setRunError(String(e));
      setIsRunning(false);
      setActiveTaskId(null);
    }
  };

  const handleCancel = async () => {
    if (activeTaskId) {
      try {
        await useTaskQueueStore.getState().cancelTask(activeTaskId);
      } catch (e: unknown) {
        console.error("Cancel failed:", e);
      }
      setActiveTaskId(null);
      setIsRunning(false);
    }
  };

  const addStep = (type: "input" | "model" | "transform") => {
    const id = `step_${Date.now().toString(36)}`;
    let newStep: PipelineStep;
    if (type === "input") {
      newStep = {
        type: "input",
        id,
        label: `输入节点 ${currentPipeline.steps.length + 1}`,
        defaultValue: "",
      };
    } else if (type === "model") {
      const defaultModel =
        modelsQuery.data?.find((m) => m.enabled) || modelsQuery.data?.[0];
      newStep = {
        type: "model",
        id,
        label: `AI 模型处理 ${currentPipeline.steps.length + 1}`,
        modelDefinitionId: defaultModel ? defaultModel.id : "",
        promptTemplate: "{{prev}}",
        systemPrompt: "",
        parameters: {},
      };
    } else {
      newStep = {
        type: "transform",
        id,
        label: `规则转换 ${currentPipeline.steps.length + 1}`,
        kind: "template",
        rule: "{{prev}}",
      };
    }
    setCurrentPipeline((prev) => ({
      ...prev,
      steps: [...prev.steps, newStep],
    }));
  };

  const removeStep = (index: number) => {
    setCurrentPipeline((prev) => ({
      ...prev,
      steps: prev.steps.filter((_, i) => i !== index),
    }));
  };

  const moveStep = (index: number, direction: "up" | "down") => {
    const targetIdx = direction === "up" ? index - 1 : index + 1;
    if (targetIdx < 0 || targetIdx >= currentPipeline.steps.length) return;
    const newSteps = [...currentPipeline.steps];
    const temp = newSteps[index];
    newSteps[index] = newSteps[targetIdx];
    newSteps[targetIdx] = temp;
    setCurrentPipeline((prev) => ({ ...prev, steps: newSteps }));
  };

  const updateStep = (index: number, updated: PipelineStep) => {
    const newSteps = [...currentPipeline.steps];
    newSteps[index] = updated;
    setCurrentPipeline((prev) => ({ ...prev, steps: newSteps }));
  };

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
        background: "var(--bg)",
      }}
    >
      {/* Header */}
      <header
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "16px 24px",
          borderBottom: "1px solid var(--line)",
          background: "var(--bg-card)",
          flexShrink: 0,
        }}
      >
        <div>
          <p className="eyebrow" style={{ margin: 0 }}>
            Phase 9 · Multi-Modal Pipeline
          </p>
          <h1 style={{ margin: 0, fontSize: "18px" }}> 自动化工作流与管线</h1>
        </div>

        <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
          <input
            type="file"
            id="pipeline-import-file-input"
            accept=".json"
            style={{ display: "none" }}
            onChange={handleImportFile}
          />
          <button
            type="button"
            className="btn"
            style={{ padding: "6px 12px", fontSize: "13px" }}
            onClick={() => {
              document.getElementById("pipeline-import-file-input")?.click();
            }}
            title="从本地 JSON 文件导入并校验工作流定义"
          >
            📥 导入 JSON
          </button>
          <button
            type="button"
            className="btn"
            style={{ padding: "6px 12px", fontSize: "13px" }}
            onClick={handleExport}
            title="导出当前工作流定义为 JSON 文件"
          >
            📤 导出 JSON
          </button>
          <button
            type="button"
            className="btn"
            style={{ padding: "6px 12px", fontSize: "13px" }}
            onClick={() => {
              setCurrentPipeline({
                id: "",
                name: "新建自定义工作流",
                description: "",
                steps: [
                  {
                    type: "input",
                    id: "step_1",
                    label: "初始输入",
                    defaultValue: "",
                  },
                ],
                createdAt: "",
                updatedAt: "",
              });
              setRunResult(null);
            }}
          >
            新建空白工作流
          </button>
          <button
            type="button"
            className="btn"
            style={{
              padding: "6px 14px",
              fontSize: "13px",
              background: "var(--accent)",
              color: "var(--accent-ink)",
              fontWeight: 600,
            }}
            onClick={handleSave}
          >
            保存工作流
          </button>
        </div>
      </header>

      {importFeedback && (
        <div
          style={{
            padding: "8px 24px",
            background: "var(--ok-subtle, rgba(16, 185, 129, 0.15))",
            color: "var(--ok, #10b981)",
            fontSize: "13px",
            fontWeight: 600,
            borderBottom: "1px solid var(--line)",
          }}
        >
          ✓ {importFeedback}
        </div>
      )}

      {/* Main Body */}
      <div
        style={{
          display: "flex",
          flex: 1,
          minHeight: 0,
          overflow: "hidden",
        }}
      >
        {/* Left Sidebar: Presets & Saved List */}
        <div
          style={{
            width: "280px",
            borderRight: "1px solid var(--line)",
            display: "flex",
            flexDirection: "column",
            background: "var(--bg-card)",
            padding: "16px",
            overflowY: "auto",
            gap: "20px",
            flexShrink: 0,
          }}
        >
          {/* Preset Workflows */}
          <div>
            <h3
              style={{
                fontSize: "13px",
                fontWeight: 600,
                color: "var(--muted)",
                textTransform: "uppercase",
                letterSpacing: "0.5px",
                marginBottom: "8px",
              }}
            >
              官方预设模版
            </h3>
            <div
              style={{ display: "flex", flexDirection: "column", gap: "6px" }}
            >
              {presetsQuery.data?.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => loadPreset(p)}
                  style={{
                    textAlign: "left",
                    padding: "8px 12px",
                    borderRadius: "6px",
                    border:
                      selectedPresetId === p.id
                        ? "1px solid var(--accent)"
                        : "1px solid var(--line)",
                    background:
                      selectedPresetId === p.id
                        ? "var(--accent-subtle, rgba(0,102,255,0.08))"
                        : "var(--bg)",
                    cursor: "pointer",
                    transition: "all 0.15s ease",
                  }}
                >
                  <div style={{ fontWeight: 600, fontSize: "13px" }}>
                    {p.name}
                  </div>
                  <div
                    style={{
                      fontSize: "11px",
                      color: "var(--muted)",
                      marginTop: "3px",
                      lineHeight: "1.3",
                    }}
                  >
                    {p.description}
                  </div>
                </button>
              ))}
            </div>
          </div>

          {/* User Saved Workflows */}
          <div>
            <h3
              style={{
                fontSize: "13px",
                fontWeight: 600,
                color: "var(--muted)",
                textTransform: "uppercase",
                letterSpacing: "0.5px",
                marginBottom: "8px",
              }}
            >
              我的工作流
            </h3>
            {pipelinesQuery.data && pipelinesQuery.data.length > 0 ? (
              <div
                style={{ display: "flex", flexDirection: "column", gap: "6px" }}
              >
                {pipelinesQuery.data.map((p) => (
                  <div
                    key={p.id}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      padding: "8px 12px",
                      borderRadius: "6px",
                      border:
                        currentPipeline.id === p.id
                          ? "1px solid var(--accent)"
                          : "1px solid var(--line)",
                      background:
                        currentPipeline.id === p.id
                          ? "var(--accent-subtle, rgba(0,102,255,0.08))"
                          : "var(--bg)",
                    }}
                  >
                    <button
                      type="button"
                      style={{
                        background: "none",
                        border: "none",
                        textAlign: "left",
                        cursor: "pointer",
                        flex: 1,
                        padding: 0,
                      }}
                      onClick={() => {
                        setSelectedPresetId("");
                        setCurrentPipeline(p);
                        const firstInput = p.steps.find(
                          (s) => s.type === "input",
                        );
                        if (
                          firstInput &&
                          firstInput.type === "input" &&
                          firstInput.defaultValue
                        ) {
                          setInitialInput(firstInput.defaultValue);
                        }
                      }}
                    >
                      <div style={{ fontWeight: 600, fontSize: "13px" }}>
                        {p.name}
                      </div>
                      <div style={{ fontSize: "11px", color: "var(--muted)" }}>
                        {p.steps.length} 个步骤 · {p.updatedAt.slice(0, 10)}
                      </div>
                    </button>
                    <button
                      type="button"
                      style={{
                        background: "none",
                        border: "none",
                        color: "var(--danger, #ff4d4f)",
                        cursor: "pointer",
                        fontSize: "14px",
                        padding: "2px 6px",
                      }}
                      title="删除工作流"
                      onClick={() => handleDelete(p.id)}
                    >
                      删除
                    </button>
                  </div>
                ))}
              </div>
            ) : (
              <div
                style={{
                  fontSize: "12px",
                  color: "var(--muted)",
                  padding: "8px 0",
                  fontStyle: "italic",
                }}
              >
                暂无自定义工作流，可点击上方保存创建。
              </div>
            )}
          </div>
        </div>

        {/* Center / Right: Step Editor & Live Execution Panel */}
        <div
          style={{
            flex: 1,
            display: "grid",
            gridTemplateColumns: "1.2fr 1fr",
            minHeight: 0,
            overflow: "hidden",
          }}
        >
          {/* Middle: Step Pipeline Visual Editor */}
          <div
            style={{
              padding: "20px 24px",
              overflowY: "auto",
              display: "flex",
              flexDirection: "column",
              gap: "16px",
              borderRight: "1px solid var(--line)",
            }}
          >
            {/* Pipeline Meta Config */}
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                gap: "8px",
                padding: "14px 16px",
                background: "var(--bg-card)",
                borderRadius: "8px",
                border: "1px solid var(--line)",
              }}
            >
              <div
                style={{ display: "flex", gap: "10px", alignItems: "center" }}
              >
                <input
                  type="text"
                  value={currentPipeline.name}
                  onChange={(e) =>
                    setCurrentPipeline((prev) => ({
                      ...prev,
                      name: e.target.value,
                    }))
                  }
                  placeholder="工作流名称"
                  style={{
                    fontSize: "16px",
                    fontWeight: 600,
                    flex: 1,
                    padding: "6px 10px",
                    borderRadius: "6px",
                    border: "1px solid var(--line)",
                    background: "var(--bg)",
                  }}
                />
              </div>
              <input
                type="text"
                value={currentPipeline.description}
                onChange={(e) =>
                  setCurrentPipeline((prev) => ({
                    ...prev,
                    description: e.target.value,
                  }))
                }
                placeholder="工作流描述（可选）"
                style={{
                  fontSize: "13px",
                  padding: "5px 10px",
                  borderRadius: "6px",
                  border: "1px solid var(--line)",
                  background: "var(--bg)",
                }}
              />
            </div>

            {/* Steps Visual Chain */}
            <div
              style={{ display: "flex", flexDirection: "column", gap: "12px" }}
            >
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                }}
              >
                <div style={{ fontSize: "14px", fontWeight: 600 }}>
                  管线步骤链路 ({currentPipeline.steps.length} 步)
                </div>
                <div style={{ display: "flex", gap: "6px" }}>
                  <button
                    type="button"
                    className="btn"
                    style={{ fontSize: "12px", padding: "4px 8px" }}
                    onClick={() => addStep("input")}
                  >
                    + 输入节点
                  </button>
                  <button
                    type="button"
                    className="btn"
                    style={{ fontSize: "12px", padding: "4px 8px" }}
                    onClick={() => addStep("model")}
                  >
                    + AI 模型节点
                  </button>
                  <button
                    type="button"
                    className="btn"
                    style={{ fontSize: "12px", padding: "4px 8px" }}
                    onClick={() => addStep("transform")}
                  >
                    + 转换节点
                  </button>
                </div>
              </div>

              {currentPipeline.steps.map((step, idx) => (
                <div
                  key={step.id || idx}
                  style={{
                    border: "1px solid var(--line)",
                    borderRadius: "8px",
                    background: "var(--bg-card)",
                    padding: "14px 16px",
                    display: "flex",
                    flexDirection: "column",
                    gap: "10px",
                    position: "relative",
                  }}
                >
                  {/* Step Header */}
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
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
                          width: "22px",
                          height: "22px",
                          borderRadius: "50%",
                          background: "var(--line)",
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                          fontSize: "12px",
                          fontWeight: 700,
                        }}
                      >
                        {idx + 1}
                      </span>
                      <span
                        style={{
                          fontSize: "11px",
                          fontWeight: 600,
                          padding: "2px 6px",
                          borderRadius: "4px",
                          background:
                            step.type === "input"
                              ? "rgba(16, 185, 129, 0.15)"
                              : step.type === "model"
                                ? "rgba(59, 130, 246, 0.15)"
                                : "rgba(245, 158, 11, 0.15)",
                          color:
                            step.type === "input"
                              ? "#10b981"
                              : step.type === "model"
                                ? "#3b82f6"
                                : "#f59e0b",
                        }}
                      >
                        {step.type === "input"
                          ? " 初始输入"
                          : step.type === "model"
                            ? " AI 模型"
                            : " 规则转换"}
                      </span>
                      <input
                        type="text"
                        value={step.label}
                        onChange={(e) =>
                          updateStep(idx, { ...step, label: e.target.value })
                        }
                        style={{
                          fontWeight: 600,
                          fontSize: "13px",
                          border: "none",
                          background: "transparent",
                          outline: "none",
                          padding: "2px 4px",
                        }}
                      />
                    </div>

                    <div style={{ display: "flex", gap: "4px" }}>
                      <button
                        type="button"
                        disabled={idx === 0}
                        onClick={() => moveStep(idx, "up")}
                        style={{
                          cursor: idx === 0 ? "not-allowed" : "pointer",
                          opacity: idx === 0 ? 0.3 : 1,
                          background: "none",
                          border: "none",
                          padding: "2px 4px",
                        }}
                      >
                        上移
                      </button>
                      <button
                        type="button"
                        disabled={idx === currentPipeline.steps.length - 1}
                        onClick={() => moveStep(idx, "down")}
                        style={{
                          cursor:
                            idx === currentPipeline.steps.length - 1
                              ? "not-allowed"
                              : "pointer",
                          opacity:
                            idx === currentPipeline.steps.length - 1 ? 0.3 : 1,
                          background: "none",
                          border: "none",
                          padding: "2px 4px",
                        }}
                      >
                        下移
                      </button>
                      <button
                        type="button"
                        onClick={() => removeStep(idx)}
                        style={{
                          cursor: "pointer",
                          background: "none",
                          border: "none",
                          color: "var(--danger, #ff4d4f)",
                          padding: "2px 4px",
                        }}
                      >
                        移除
                      </button>
                    </div>
                  </div>

                  {/* Step Body */}
                  {step.type === "input" && (
                    <div
                      style={{
                        display: "flex",
                        flexDirection: "column",
                        gap: "6px",
                      }}
                    >
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: "8px",
                        }}
                      >
                        <label
                          style={{
                            fontSize: "12px",
                            color: "var(--muted)",
                            whiteSpace: "nowrap",
                          }}
                        >
                          输入数据类型:
                        </label>
                        <select
                          value={step.dataType || "text"}
                          onChange={(e) =>
                            updateStep(idx, {
                              ...step,
                              dataType: e.target.value as
                                | "text"
                                | "image"
                                | "video"
                                | "audio",
                            })
                          }
                          style={{
                            fontSize: "12px",
                            padding: "3px 8px",
                            borderRadius: "6px",
                            border: "1px solid var(--line)",
                            background: "var(--bg)",
                          }}
                        >
                          <option value="text">纯文本 (Text)</option>
                          <option value="image">图像文件路径 (Image)</option>
                          <option value="video">视频文件路径 (Video)</option>
                          <option value="audio">音频文件路径 (Audio)</option>
                        </select>
                      </div>
                      <label
                        style={{ fontSize: "12px", color: "var(--muted)" }}
                      >
                        默认初始值（在右侧面板可实时覆盖）:
                      </label>
                      <textarea
                        rows={2}
                        value={step.defaultValue || ""}
                        onChange={(e) =>
                          updateStep(idx, {
                            ...step,
                            defaultValue: e.target.value,
                          })
                        }
                        style={{
                          width: "100%",
                          fontSize: "13px",
                          borderRadius: "6px",
                          border: "1px solid var(--line)",
                          background: "var(--bg)",
                          padding: "8px",
                          boxSizing: "border-box",
                        }}
                      />
                    </div>
                  )}

                  {step.type === "model" && (
                    <div
                      style={{
                        display: "flex",
                        flexDirection: "column",
                        gap: "8px",
                      }}
                    >
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: "8px",
                        }}
                      >
                        <label
                          style={{
                            fontSize: "12px",
                            color: "var(--muted)",
                            whiteSpace: "nowrap",
                          }}
                        >
                          选定模型:
                        </label>
                        <select
                          value={step.modelDefinitionId}
                          onChange={(e) =>
                            updateStep(idx, {
                              ...step,
                              modelDefinitionId: e.target.value,
                            })
                          }
                          style={{
                            flex: 1,
                            fontSize: "13px",
                            padding: "4px 8px",
                            borderRadius: "6px",
                            border: "1px solid var(--line)",
                            background: "var(--bg)",
                          }}
                        >
                          <option value="">-- 请选择模型 --</option>
                          {modelsQuery.data?.map((m) => (
                            <option key={m.id} value={m.id}>
                              [{m.modality.toUpperCase()}] {m.name} ({m.modelId}
                              )
                            </option>
                          ))}
                        </select>
                        {onOpenModels && (
                          <button
                            type="button"
                            onClick={onOpenModels}
                            style={{
                              fontSize: "12px",
                              background: "none",
                              border: "none",
                              color: "var(--accent)",
                              cursor: "pointer",
                              textDecoration: "underline",
                            }}
                          >
                            管理模型
                          </button>
                        )}
                      </div>

                      <div
                        style={{
                          display: "flex",
                          flexDirection: "column",
                          gap: "4px",
                        }}
                      >
                        <div
                          style={{
                            display: "flex",
                            justifyContent: "space-between",
                            alignItems: "center",
                          }}
                        >
                          <label
                            style={{ fontSize: "12px", color: "var(--muted)" }}
                          >
                            提示词模板 (Prompt Template):
                          </label>
                          <span
                            style={{ fontSize: "11px", color: "var(--muted)" }}
                          >
                            变量支持:{" "}
                            <code
                              style={{
                                background: "var(--line)",
                                padding: "1px 4px",
                              }}
                            >
                              {"{{input}}"}
                            </code>
                            ,{" "}
                            <code
                              style={{
                                background: "var(--line)",
                                padding: "1px 4px",
                              }}
                            >
                              {"{{prev}}"}
                            </code>
                          </span>
                        </div>
                        <textarea
                          rows={3}
                          value={step.promptTemplate}
                          onChange={(e) =>
                            updateStep(idx, {
                              ...step,
                              promptTemplate: e.target.value,
                            })
                          }
                          style={{
                            width: "100%",
                            fontSize: "13px",
                            borderRadius: "6px",
                            border: "1px solid var(--line)",
                            background: "var(--bg)",
                            padding: "8px",
                            boxSizing: "border-box",
                            fontFamily: "monospace",
                          }}
                        />
                      </div>

                      {(() => {
                        const matchedModel = modelsQuery.data?.find(
                          (m) => m.id === step.modelDefinitionId,
                        );
                        const isVisual =
                          matchedModel?.modality === "image" ||
                          matchedModel?.modality === "video";
                        if (!isVisual) return null;
                        return (
                          <div
                            style={{
                              display: "flex",
                              flexDirection: "column",
                              gap: "4px",
                            }}
                          >
                            <div
                              style={{
                                display: "flex",
                                justifyContent: "space-between",
                                alignItems: "center",
                              }}
                            >
                              <label
                                style={{
                                  fontSize: "12px",
                                  color: "var(--muted)",
                                }}
                              >
                                {matchedModel.modality === "video"
                                  ? "视频首帧媒体引用变量 (可选):"
                                  : "图生图原图媒体引用变量 (可选):"}
                              </label>
                              <span
                                style={{
                                  fontSize: "11px",
                                  color: "var(--muted)",
                                }}
                              >
                                留空默认从上一节点获取
                              </span>
                            </div>
                            <input
                              type="text"
                              value={step.mediaInputVariable || ""}
                              onChange={(e) =>
                                updateStep(idx, {
                                  ...step,
                                  mediaInputVariable:
                                    e.target.value.trim() || undefined,
                                })
                              }
                              placeholder="例如：{{step_keyframe.media}} 或 {{prev}}"
                              style={{
                                fontSize: "12px",
                                padding: "4px 8px",
                                borderRadius: "6px",
                                border: "1px solid var(--line)",
                                background: "var(--bg)",
                                fontFamily: "monospace",
                              }}
                            />
                          </div>
                        );
                      })()}

                      <div
                        style={{
                          display: "flex",
                          flexDirection: "column",
                          gap: "4px",
                        }}
                      >
                        <label
                          style={{ fontSize: "12px", color: "var(--muted)" }}
                        >
                          系统提示词 System Prompt（可选）:
                        </label>
                        <input
                          type="text"
                          value={step.systemPrompt || ""}
                          onChange={(e) =>
                            updateStep(idx, {
                              ...step,
                              systemPrompt: e.target.value || undefined,
                            })
                          }
                          placeholder="例如：你是一位专业文学翻译专家"
                          style={{
                            fontSize: "12px",
                            padding: "4px 8px",
                            borderRadius: "6px",
                            border: "1px solid var(--line)",
                            background: "var(--bg)",
                          }}
                        />
                      </div>
                    </div>
                  )}

                  {step.type === "transform" && (
                    <div
                      style={{
                        display: "flex",
                        flexDirection: "column",
                        gap: "8px",
                      }}
                    >
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: "8px",
                        }}
                      >
                        <label
                          style={{
                            fontSize: "12px",
                            color: "var(--muted)",
                            whiteSpace: "nowrap",
                          }}
                        >
                          转换规则类型:
                        </label>
                        <select
                          value={step.kind}
                          onChange={(e) =>
                            updateStep(idx, { ...step, kind: e.target.value })
                          }
                          style={{
                            fontSize: "13px",
                            padding: "4px 8px",
                            borderRadius: "6px",
                            border: "1px solid var(--line)",
                            background: "var(--bg)",
                          }}
                        >
                          <option value="template">模板包装 (Template)</option>
                          <option value="trim">去除前后空格 (Trim)</option>
                          <option value="uppercase">全大写 (Uppercase)</option>
                          <option value="lowercase">全小写 (Lowercase)</option>
                          <option value="regex">正则替换 (Regex)</option>
                        </select>
                      </div>

                      {step.kind === "template" && (
                        <div
                          style={{
                            display: "flex",
                            flexDirection: "column",
                            gap: "4px",
                          }}
                        >
                          <label
                            style={{ fontSize: "12px", color: "var(--muted)" }}
                          >
                            模板包装规则 (使用 {"{{prev}}"} 引用上一步):
                          </label>
                          <input
                            type="text"
                            value={step.rule}
                            onChange={(e) =>
                              updateStep(idx, { ...step, rule: e.target.value })
                            }
                            placeholder="{{prev}}, masterpiece, highly detailed"
                            style={{
                              fontSize: "13px",
                              padding: "5px 8px",
                              borderRadius: "6px",
                              border: "1px solid var(--line)",
                              background: "var(--bg)",
                              fontFamily: "monospace",
                            }}
                          />
                        </div>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>

          {/* Right: Pipeline Live Execution & Results Inspector */}
          <div
            style={{
              padding: "20px 24px",
              overflowY: "auto",
              display: "flex",
              flexDirection: "column",
              gap: "16px",
              background: "var(--bg)",
            }}
          >
            {/* Input & Execution Trigger */}
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                gap: "10px",
                padding: "16px",
                background: "var(--bg-card)",
                borderRadius: "8px",
                border: "1px solid var(--line)",
              }}
            >
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                }}
              >
                <div style={{ fontSize: "14px", fontWeight: 600 }}>
                  运行与即时验证
                </div>
                {runResult && (
                  <span
                    style={{
                      fontSize: "12px",
                      background: "rgba(16, 185, 129, 0.15)",
                      color: "#10b981",
                      padding: "2px 8px",
                      borderRadius: "12px",
                      fontWeight: 600,
                    }}
                  >
                    总耗时: {runResult.totalLatencyMs}ms
                  </span>
                )}
              </div>

              <div
                style={{ display: "flex", flexDirection: "column", gap: "4px" }}
              >
                <label style={{ fontSize: "12px", color: "var(--muted)" }}>
                  本次执行输入文本 (Initial Input):
                </label>
                <textarea
                  rows={3}
                  value={initialInput}
                  onChange={(e) => setInitialInput(e.target.value)}
                  placeholder="输入任意要处理的文章、提示词或故事..."
                  style={{
                    width: "100%",
                    fontSize: "13px",
                    borderRadius: "6px",
                    border: "1px solid var(--line)",
                    background: "var(--bg)",
                    padding: "8px",
                    boxSizing: "border-box",
                  }}
                />
              </div>

              <div style={{ display: "flex", gap: "8px" }}>
                <button
                  type="button"
                  disabled={isRunning}
                  onClick={handleRun}
                  style={{
                    flex: 1,
                    padding: "10px 16px",
                    borderRadius: "6px",
                    border: "none",
                    background: isRunning ? "var(--muted)" : "var(--accent)",
                    color: "var(--accent-ink)",
                    fontWeight: 600,
                    fontSize: "14px",
                    cursor: isRunning ? "not-allowed" : "pointer",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    gap: "8px",
                  }}
                >
                  {isRunning ? (
                    <>⏳ 正在多模态执行中 (Step-by-step)...</>
                  ) : (
                    <>▶ 立即顺序执行工作流</>
                  )}
                </button>
                {isRunning && (
                  <button
                    type="button"
                    onClick={handleCancel}
                    style={{
                      padding: "10px 16px",
                      borderRadius: "6px",
                      border: "1px solid var(--danger, #ef4444)",
                      background: "rgba(239, 68, 68, 0.1)",
                      color: "var(--danger, #ef4444)",
                      fontWeight: 600,
                      fontSize: "14px",
                      cursor: "pointer",
                    }}
                  >
                    ⏹ 中止
                  </button>
                )}
              </div>

              {isRunning && activeTask?.message && (
                <div
                  style={{
                    padding: "8px 12px",
                    borderRadius: "6px",
                    background: "rgba(59, 130, 246, 0.1)",
                    border: "1px solid rgba(59, 130, 246, 0.3)",
                    color: "var(--accent, #3b82f6)",
                    fontSize: "12px",
                    display: "flex",
                    alignItems: "center",
                    gap: "6px",
                  }}
                >
                  ⏳ {activeTask.message}{" "}
                  {activeTask.progress
                    ? `(${Math.round(activeTask.progress * 100)}%)`
                    : ""}
                </div>
              )}

              {runError && (
                <div
                  style={{
                    padding: "10px 14px",
                    borderRadius: "6px",
                    background: "rgba(239, 68, 68, 0.1)",
                    border: "1px solid rgba(239, 68, 68, 0.3)",
                    color: "var(--danger, #ef4444)",
                    fontSize: "12px",
                    display: "flex",
                    flexDirection: "column",
                    gap: "8px",
                  }}
                >
                  <div style={{ fontWeight: 600 }}>
                    执行中断/失败: {runError}
                  </div>
                  {intermediateSteps.length > 0 &&
                    intermediateSteps.every(
                      (s) => s.status === "completed",
                    ) && (
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "space-between",
                          borderTop: "1px dashed rgba(239, 68, 68, 0.3)",
                          paddingTop: "6px",
                        }}
                      >
                        <span style={{ fontSize: "11px", color: "var(--ink)" }}>
                          前 {intermediateSteps.length} 个步骤已成功完成并缓存
                          (不重复计费)。
                        </span>
                        <button
                          type="button"
                          onClick={() => handleResume(intermediateSteps.length)}
                          style={{
                            padding: "4px 10px",
                            borderRadius: "4px",
                            border: "1px solid var(--accent)",
                            background: "var(--accent)",
                            color: "var(--accent-ink)",
                            fontSize: "11px",
                            fontWeight: 600,
                            cursor: "pointer",
                          }}
                        >
                          🔄 从第 {intermediateSteps.length + 1} 步断点续跑
                        </button>
                      </div>
                    )}
                </div>
              )}
            </div>

            {/* Step-by-Step Latency & Trace Inspector */}
            {((runResult?.stepResults ?? []).length > 0 ||
              intermediateSteps.length > 0) && (
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: "12px",
                }}
              >
                <div style={{ fontSize: "13px", fontWeight: 600 }}>
                  节点执行追踪与中间产物 {isRunning ? "(实时执行中...)" : ""}
                </div>
                {(runResult?.stepResults ?? intermediateSteps).map(
                  (sr, idx) => (
                    <div
                      key={sr.stepId || idx}
                      style={{
                        border: "1px solid var(--line)",
                        borderRadius: "6px",
                        background: "var(--bg-card)",
                        padding: "12px",
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
                            fontWeight: 600,
                            fontSize: "13px",
                            display: "flex",
                            alignItems: "center",
                            gap: "6px",
                          }}
                        >
                          <span>
                            {idx + 1}. {sr.stepLabel}
                          </span>
                          <span
                            style={{
                              fontSize: "11px",
                              padding: "1px 5px",
                              borderRadius: "3px",
                              background:
                                sr.status === "completed"
                                  ? "rgba(16, 185, 129, 0.15)"
                                  : "rgba(239, 68, 68, 0.15)",
                              color:
                                sr.status === "completed"
                                  ? "#10b981"
                                  : "#ef4444",
                            }}
                          >
                            {sr.status}
                          </span>
                        </div>
                        <span
                          style={{ fontSize: "12px", color: "var(--muted)" }}
                        >
                          {sr.latencyMs}ms
                        </span>
                      </div>

                      {sr.textOutput && (
                        <div
                          style={{
                            fontSize: "12px",
                            background: "var(--bg)",
                            padding: "6px 8px",
                            borderRadius: "4px",
                            border: "1px solid var(--line)",
                            maxHeight: "100px",
                            overflowY: "auto",
                            whiteSpace: "pre-wrap",
                            wordBreak: "break-all",
                          }}
                        >
                          {sr.textOutput}
                        </div>
                      )}

                      {sr.mediaPaths && sr.mediaPaths.length > 0 && (
                        <div
                          style={{
                            display: "flex",
                            gap: "8px",
                            flexWrap: "wrap",
                            marginTop: "4px",
                          }}
                        >
                          {sr.mediaPaths.map((path, mIdx) => (
                            <div
                              key={mIdx}
                              style={{
                                fontSize: "11px",
                                color: "var(--accent)",
                                background: "rgba(59, 130, 246, 0.1)",
                                padding: "2px 6px",
                                borderRadius: "4px",
                              }}
                            >
                              {path}
                            </div>
                          ))}
                        </div>
                      )}

                      {sr.error && (
                        <div
                          style={{
                            fontSize: "12px",
                            color: "var(--danger, #ef4444)",
                          }}
                        >
                          错误: {sr.error}
                        </div>
                      )}
                    </div>
                  ),
                )}

                {/* Final Output Card */}
                {runResult && (
                  <div
                    style={{
                      marginTop: "8px",
                      border: "2px solid var(--accent)",
                      borderRadius: "8px",
                      background: "var(--bg-card)",
                      padding: "16px",
                      display: "flex",
                      flexDirection: "column",
                      gap: "10px",
                    }}
                  >
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                      }}
                    >
                      <div
                        style={{
                          fontWeight: 700,
                          fontSize: "14px",
                          color: "var(--accent)",
                        }}
                      >
                        管线最终生成结果 (
                        {runResult.finalOutputKind.toUpperCase()})
                      </div>
                    </div>

                    {runResult.finalText && (
                      <div
                        style={{
                          background: "var(--bg)",
                          padding: "10px 12px",
                          borderRadius: "6px",
                          border: "1px solid var(--line)",
                          fontSize: "13px",
                          lineHeight: "1.5",
                          whiteSpace: "pre-wrap",
                          wordBreak: "break-word",
                        }}
                      >
                        {runResult.finalText}
                      </div>
                    )}

                    {runResult.finalMediaPaths &&
                      runResult.finalMediaPaths.length > 0 && (
                        <div
                          style={{
                            display: "flex",
                            flexDirection: "column",
                            gap: "8px",
                          }}
                        >
                          {runResult.finalMediaPaths.map((mediaPath, idx) => (
                            <div
                              key={idx}
                              style={{
                                display: "flex",
                                alignItems: "center",
                                gap: "8px",
                                padding: "8px",
                                background: "var(--bg)",
                                borderRadius: "6px",
                                border: "1px solid var(--line)",
                              }}
                            >
                              <span style={{ fontSize: "16px" }}>
                                {runResult.finalOutputKind === "image"
                                  ? ""
                                  : runResult.finalOutputKind === "video"
                                    ? ""
                                    : ""}
                              </span>
                              <span
                                style={{
                                  fontSize: "12px",
                                  wordBreak: "break-all",
                                  flex: 1,
                                }}
                              >
                                {mediaPath}
                              </span>
                            </div>
                          ))}
                        </div>
                      )}
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
