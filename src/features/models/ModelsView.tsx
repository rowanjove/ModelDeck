import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, asAppError } from "../../api";
import { ErrorNote } from "../../components/ErrorNote";
import { emptyCapabilities } from "../../lib/schema";
import { suggestWorkflowBindings } from "../../lib/workflow";
import {
  audioCapabilities,
  audioParameters,
} from "../../registry/audioTemplate";
import {
  imageCapabilities,
  imageParameters,
} from "../../registry/imageTemplate";
import { textCapabilities, textParameters } from "../../registry/textTemplate";
import {
  videoCapabilities,
  videoParameters,
} from "../../registry/videoTemplate";
import {
  CAPABILITY_FIELDS,
  type ConstraintDefinition,
  type HttpRecipe,
  MODALITY_LABELS,
  type Modality,
  type ModelDefinition,
  type ModelInput,
  PARAMETER_TYPE_LABELS,
  type ParameterDefinition,
  type ParameterType,
  type SelectOption,
} from "../../types";

type EditorConstraint = ConstraintDefinition & {
  editorId: string;
};

interface EditorBinding {
  editorId: string;
  key: string;
  node: string;
  input: string;
}

interface EditorState {
  id: string | null;
  name: string;
  providerId: string;
  modelId: string;
  modality: Modality;
  capabilities: Record<string, unknown>;
  parameters: ParameterDefinition[];
  constraints: EditorConstraint[];
  chatPath: string;
  modelsPath: string;
  imagePath: string;
  imageEditPath: string;
  speechPath: string;
  workflowText: string;
  bindings: EditorBinding[];
  httpText: string;
  enabled: boolean;
}

export function ModelsView({ onOpenSettings }: { onOpenSettings: () => void }) {
  const queryClient = useQueryClient();
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: api.providers,
  });
  const models = useQuery({ queryKey: ["models"], queryFn: api.models });
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [error, setError] = useState<ReturnType<typeof asAppError> | null>(
    null,
  );
  const [remote, setRemote] = useState<string[]>([]);
  const [showRegistry, setShowRegistry] = useState(false);
  const [registryNotice, setRegistryNotice] = useState<string | null>(null);
  const [targetProviderId, setTargetProviderId] = useState<string>("");
  const [customRegistryUrl, setCustomRegistryUrl] = useState("");
  const [showCustomUrl, setShowCustomUrl] = useState(false);
  const [registryModalityFilter, setRegistryModalityFilter] = useState("");
  const [registrySearch, setRegistrySearch] = useState("");

  const registryQuery = useQuery({
    queryKey: ["registry"],
    queryFn: api.registryGet,
    enabled: showRegistry,
  });

  const checkUpdate = useMutation({
    mutationFn: () => api.registryCheckUpdate(customRegistryUrl || undefined),
    onSuccess: (data) => {
      if (data.hasUpdate) {
        setRegistryNotice(
          `发现新版本 ${data.remoteVersion}（当前 ${data.currentVersion}）: ${data.releaseNotes ?? ""}`,
        );
      } else {
        setRegistryNotice(`当前已是最新版本 (${data.currentVersion})`);
      }
    },
    onError: (err) => setError(asAppError(err)),
  });

  const applyUpdate = useMutation({
    mutationFn: () => api.registryApplyUpdate(customRegistryUrl || undefined),
    onSuccess: async (data) => {
      await queryClient.invalidateQueries({ queryKey: ["registry"] });
      setRegistryNotice(
        `模型注册表已成功更新至版本 ${data.version}！包含 ${data.models.length} 个模型模板`,
      );
    },
    onError: (err) => setError(asAppError(err)),
  });

  const rollback = useMutation({
    mutationFn: () => api.registryRollback(),
    onSuccess: async (data) => {
      await queryClient.invalidateQueries({ queryKey: ["registry"] });
      setRegistryNotice(`已成功回滚至上一版本 (${data.version})！`);
    },
    onError: (err) => setError(asAppError(err)),
  });

  const importModel = useMutation({
    mutationFn: async ({
      templateId,
      targetId,
      overwrite,
    }: {
      templateId: string;
      targetId: string;
      overwrite?: boolean;
    }) => {
      return api.registryImportModel(templateId, targetId, overwrite);
    },
    onSuccess: async (imported) => {
      await queryClient.invalidateQueries({ queryKey: ["models"] });
      setEditor(fromModel(imported));
      setShowRegistry(false);
      setError(null);
    },
    onError: (err) => setError(asAppError(err)),
  });

  const save = useMutation({
    mutationFn: async (state: EditorState) => {
      const input = toInput(state);
      return state.id
        ? api.updateModel(state.id, input)
        : api.createModel(input);
    },
    onSuccess: async (model) => {
      await queryClient.invalidateQueries({ queryKey: ["models"] });
      setEditor(fromModel(model));
      setError(null);
    },
    onError: (reason) => setError(asAppError(reason)),
  });
  const remove = useMutation({
    mutationFn: api.deleteModel,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["models"] });
      setEditor(null);
    },
    onError: (reason) => setError(asAppError(reason)),
  });
  const fetchRemote = useMutation({
    mutationFn: async () => {
      if (!editor?.providerId) {
        throw new Error("请先选择 Provider");
      }
      return api.remoteModels(
        editor.providerId,
        editor.modelsPath.trim() || undefined,
      );
    },
    onSuccess: (items) => {
      setRemote(items.map((item) => item.id));
      setError(null);
    },
    onError: (reason) => setError(asAppError(reason)),
  });

  if (providers.isLoading || models.isLoading) {
    return <p className="page-message">正在读取模型…</p>;
  }
  if (providers.error || models.error) {
    return <ErrorNote error={asAppError(providers.error ?? models.error)} />;
  }
  const providerList = providers.data ?? [];
  const editorProvider = providerList.find(
    (item) => item.id === editor?.providerId,
  );
  if (providerList.length === 0) {
    return (
      <section className="page-message">
        <h1>模型</h1>
        <p>先添加 Provider，再为它声明模型能力和参数。</p>
        <button type="button" className="primary" onClick={onOpenSettings}>
          去添加 Provider
        </button>
      </section>
    );
  }

  return (
    <section className="catalog">
      <div className="catalog-list">
        <header>
          <h1>模型</h1>
          <div className="inline-actions">
            <button
              type="button"
              onClick={() => {
                setShowRegistry(!showRegistry);
                setRegistryNotice(null);
              }}
            >
              {showRegistry ? "返回模型编辑" : "在线模型库"}
            </button>
            <button
              type="button"
              className="primary"
              onClick={() => {
                setEditor(blank(providerList[0].id));
                setError(null);
                setShowRegistry(false);
              }}
            >
              新建
            </button>
          </div>
        </header>
        <ul>
          {(models.data ?? []).map((model) => {
            const isSelected = editor?.id === model.id && !showRegistry;
            const provider = (providers.data ?? []).find(
              (p) => p.id === model.providerId,
            );
            return (
              <li key={model.id}>
                <button
                  type="button"
                  aria-current={isSelected ? "page" : undefined}
                  onClick={() => {
                    setEditor(fromModel(model));
                    setShowRegistry(false);
                  }}
                  style={{
                    opacity: model.enabled ? 1 : 0.55,
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
                      {model.name}
                    </strong>
                    <span
                      style={{
                        fontSize: "10px",
                        fontFamily: "var(--mono)",
                        padding: "1px 5px",
                        borderRadius: "var(--radius-xs)",
                        background: "var(--surface-3)",
                        color: "var(--ink-secondary)",
                        flexShrink: 0,
                      }}
                    >
                      {MODALITY_LABELS[model.modality]}
                    </span>
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
                    {provider?.name ?? "未指定"} · {model.modelId}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </div>
      <div className="catalog-detail">
        {showRegistry ? (
          <div>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                marginBottom: "16px",
                flexWrap: "wrap",
                gap: "10px",
              }}
            >
              <div>
                <h2 style={{ margin: "0 0 4px" }}>在线模型注册表</h2>
                <p className="muted" style={{ margin: 0 }}>
                  版本: {registryQuery.data?.version ?? "..."} · 更新时间:{" "}
                  {registryQuery.data?.updatedAt
                    ? new Date(registryQuery.data.updatedAt).toLocaleString()
                    : "..."}
                </p>
              </div>
              <div className="inline-actions" style={{ flexWrap: "wrap" }}>
                <button
                  type="button"
                  onClick={() => setShowCustomUrl(!showCustomUrl)}
                >
                  {showCustomUrl ? "收起自定义源" : "自定义源"}
                </button>
                <button
                  type="button"
                  onClick={() => void checkUpdate.mutateAsync()}
                  disabled={checkUpdate.isPending}
                >
                  {checkUpdate.isPending ? "检查中…" : "检查更新"}
                </button>
                <button
                  type="button"
                  onClick={() => void applyUpdate.mutateAsync()}
                  disabled={applyUpdate.isPending}
                >
                  {applyUpdate.isPending ? "拉取中…" : "在线拉取更新"}
                </button>
                {registryQuery.data?.canRollback ? (
                  <button
                    type="button"
                    className="danger"
                    onClick={() => void rollback.mutateAsync()}
                    disabled={rollback.isPending}
                    title="回滚至更新前的本地备份版本"
                  >
                    {rollback.isPending
                      ? "回滚中…"
                      : `回滚 (${registryQuery.data.backupVersion ?? "上版本"})`}
                  </button>
                ) : null}
                <button type="button" onClick={() => setShowRegistry(false)}>
                  关闭
                </button>
              </div>
            </div>

            {showCustomUrl && (
              <div
                style={{
                  display: "flex",
                  gap: "8px",
                  alignItems: "center",
                  marginBottom: "16px",
                  padding: "10px 12px",
                  background: "var(--raised)",
                  borderRadius: "6px",
                  border: "1px solid var(--line)",
                }}
              >
                <input
                  type="text"
                  placeholder="输入自定义 Registry JSON 远程地址 (留空则默认官方 GitHub 源)"
                  value={customRegistryUrl}
                  onChange={(e) => setCustomRegistryUrl(e.target.value)}
                  style={{ flex: 1 }}
                />
                {customRegistryUrl ? (
                  <button
                    type="button"
                    onClick={() => setCustomRegistryUrl("")}
                  >
                    重置为官方源
                  </button>
                ) : null}
              </div>
            )}

            {registryNotice && (
              <div
                style={{
                  padding: "8px 12px",
                  background: "var(--sunken)",
                  borderRadius: "6px",
                  marginBottom: "16px",
                }}
              >
                {registryNotice}
              </div>
            )}

            <div
              style={{
                display: "flex",
                gap: "12px",
                alignItems: "center",
                marginBottom: "16px",
                flexWrap: "wrap",
              }}
            >
              <input
                type="search"
                placeholder="搜索模型名称、ID 或描述..."
                value={registrySearch}
                onChange={(e) => setRegistrySearch(e.target.value)}
                style={{ maxWidth: "260px" }}
              />
              <div className="checks" style={{ gap: "6px" }}>
                {(["", "text", "image", "video", "audio"] as const).map(
                  (mod) => (
                    <button
                      key={mod}
                      type="button"
                      className={
                        registryModalityFilter === mod ? "primary" : ""
                      }
                      style={{ padding: "4px 10px", fontSize: "12px" }}
                      onClick={() => setRegistryModalityFilter(mod)}
                    >
                      {mod === "" ? "全部模态" : MODALITY_LABELS[mod]}
                    </button>
                  ),
                )}
              </div>
            </div>

            <div style={{ display: "grid", gap: "12px" }}>
              {(registryQuery.data?.models ?? [])
                .filter((template) => {
                  if (
                    registryModalityFilter &&
                    template.modality !== registryModalityFilter
                  ) {
                    return false;
                  }
                  if (registrySearch.trim()) {
                    const q = registrySearch.trim().toLowerCase();
                    const matchName = template.name.toLowerCase().includes(q);
                    const matchId = template.modelId.toLowerCase().includes(q);
                    const matchDesc = template.description
                      .toLowerCase()
                      .includes(q);
                    if (!matchName && !matchId && !matchDesc) {
                      return false;
                    }
                  }
                  return true;
                })
                .map((template) => {
                  const provId = targetProviderId || providerList[0]?.id;
                  const existingModel = (models.data ?? []).find(
                    (m) =>
                      m.providerId === provId && m.modelId === template.modelId,
                  );

                  return (
                    <div
                      key={template.id}
                      style={{
                        border: "1px solid var(--line)",
                        background: "var(--raised)",
                        borderRadius: "8px",
                        padding: "14px",
                      }}
                    >
                      <div
                        style={{
                          display: "flex",
                          justifyContent: "space-between",
                          alignItems: "flex-start",
                          marginBottom: "6px",
                          gap: "12px",
                          flexWrap: "wrap",
                        }}
                      >
                        <div style={{ flex: 1, minWidth: "260px" }}>
                          <div
                            style={{
                              display: "flex",
                              alignItems: "center",
                              gap: "8px",
                            }}
                          >
                            <strong>{template.name}</strong>
                            <span className="muted">
                              {MODALITY_LABELS[template.modality]} ·{" "}
                              {template.modelId} · {template.providerType}
                            </span>
                            {existingModel ? (
                              <span
                                style={{
                                  fontSize: "11px",
                                  color: "var(--danger)",
                                }}
                              >
                                (目标 Provider 已配置此模型)
                              </span>
                            ) : null}
                          </div>
                          <p
                            style={{
                              margin: "6px 0 8px",
                              color: "var(--soft)",
                            }}
                          >
                            {template.description}
                          </p>
                          <div
                            style={{
                              display: "flex",
                              gap: "12px",
                              fontSize: "12px",
                              color: "var(--soft)",
                            }}
                          >
                            {template.parameters.length > 0 ? (
                              <span>参数: {template.parameters.length} 项</span>
                            ) : (
                              <span>无预设参数</span>
                            )}
                            {(template.constraints?.length ?? 0) > 0 ? (
                              <span>
                                约束: {template.constraints?.length} 项联动
                              </span>
                            ) : null}
                          </div>
                        </div>
                        <div
                          className="inline-actions"
                          style={{ alignItems: "center" }}
                        >
                          <select
                            value={provId}
                            onChange={(e) =>
                              setTargetProviderId(e.target.value)
                            }
                            style={{ maxWidth: "160px" }}
                          >
                            {providerList.map((p) => (
                              <option key={p.id} value={p.id}>
                                {p.name}
                              </option>
                            ))}
                          </select>
                          {existingModel ? (
                            <button
                              type="button"
                              className="danger"
                              disabled={importModel.isPending}
                              title="覆盖该 Provider 下的现有模型配置"
                              onClick={() =>
                                void importModel.mutateAsync({
                                  templateId: template.id,
                                  targetId: provId,
                                  overwrite: true,
                                })
                              }
                            >
                              覆盖导入
                            </button>
                          ) : (
                            <button
                              type="button"
                              className="primary"
                              disabled={importModel.isPending}
                              onClick={() =>
                                void importModel.mutateAsync({
                                  templateId: template.id,
                                  targetId: provId,
                                })
                              }
                            >
                              一键导入
                            </button>
                          )}
                        </div>
                      </div>
                    </div>
                  );
                })}
            </div>
          </div>
        ) : editor ? (
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void save.mutateAsync(editor);
            }}
          >
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
                Provider
                <select
                  value={editor.providerId}
                  onChange={(event) =>
                    setEditor({ ...editor, providerId: event.target.value })
                  }
                >
                  {providerList.map((provider) => (
                    <option key={provider.id} value={provider.id}>
                      {provider.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                模型 ID
                <input
                  list="remote-model-ids"
                  value={editor.modelId}
                  onChange={(event) =>
                    setEditor({ ...editor, modelId: event.target.value })
                  }
                />
                <datalist id="remote-model-ids">
                  {remote.map((id) => (
                    <option key={id} value={id} />
                  ))}
                </datalist>
              </label>
              <label>
                模态
                <select
                  value={editor.modality}
                  onChange={(event) => {
                    const modality = event.target.value as Modality;
                    setEditor({
                      ...editor,
                      modality,
                      capabilities: emptyCapabilities(modality),
                    });
                  }}
                >
                  {(Object.keys(MODALITY_LABELS) as Modality[]).map(
                    (modality) => (
                      <option key={modality} value={modality}>
                        {MODALITY_LABELS[modality]}
                      </option>
                    ),
                  )}
                </select>
              </label>
            </div>
            <div className="inline-actions">
              <button
                type="button"
                onClick={() => void fetchRemote.mutateAsync()}
                disabled={fetchRemote.isPending}
              >
                {fetchRemote.isPending ? "读取中…" : "读取远程模型 ID"}
              </button>
              <button
                type="button"
                onClick={() => {
                  if (
                    editor.parameters.length > 0 &&
                    !window.confirm("用文本参数模板替换当前参数？")
                  ) {
                    return;
                  }
                  setEditor({
                    ...editor,
                    modality: "text",
                    capabilities: { ...textCapabilities },
                    parameters: textParameters.map((parameter) => ({
                      ...parameter,
                    })),
                    constraints: [],
                  });
                }}
              >
                填入文本参数模板
              </button>
              <button
                type="button"
                onClick={() => {
                  if (
                    editor.parameters.length > 0 &&
                    !window.confirm("用图像参数模板替换当前参数？")
                  ) {
                    return;
                  }
                  setEditor({
                    ...editor,
                    modality: "image",
                    capabilities: { ...imageCapabilities },
                    parameters: imageParameters.map((parameter) => ({
                      ...parameter,
                    })),
                    constraints: [],
                  });
                }}
              >
                填入图像参数模板
              </button>
              <button
                type="button"
                onClick={() => {
                  if (
                    editor.parameters.length > 0 &&
                    !window.confirm("用视频参数模板替换当前参数？")
                  ) {
                    return;
                  }
                  setEditor({
                    ...editor,
                    modality: "video",
                    capabilities: { ...videoCapabilities },
                    parameters: videoParameters.map((parameter) => ({
                      ...parameter,
                    })),
                    constraints: [],
                  });
                }}
              >
                填入视频参数模板
              </button>
              <button
                type="button"
                onClick={() => {
                  if (
                    editor.parameters.length > 0 &&
                    !window.confirm("用音频参数模板替换当前参数？")
                  ) {
                    return;
                  }
                  setEditor({
                    ...editor,
                    modality: "audio",
                    capabilities: { ...audioCapabilities },
                    parameters: audioParameters.map((parameter) => ({
                      ...parameter,
                    })),
                    constraints: [],
                  });
                }}
              >
                填入音频参数模板
              </button>
            </div>
            <fieldset>
              <legend>能力</legend>
              <div className="checks">
                {CAPABILITY_FIELDS[editor.modality].map((field) => (
                  <label key={field.key}>
                    <input
                      type="checkbox"
                      checked={editor.capabilities[field.key] === true}
                      onChange={(event) =>
                        setEditor({
                          ...editor,
                          capabilities: {
                            ...editor.capabilities,
                            [field.key]: event.target.checked,
                          },
                        })
                      }
                    />
                    {field.label}
                  </label>
                ))}
              </div>
            </fieldset>
            <fieldset>
              <legend>参数</legend>
              <p className="muted">
                界面按这里的 Schema
                生成。媒体参数可以声明，文本适配器只发送标量参数。
              </p>
              {editor.parameters.map((parameter, index) => (
                <ParameterEditor
                  key={`${parameter.key}-${index}`}
                  parameter={parameter}
                  onChange={(next) => {
                    const parameters = editor.parameters.slice();
                    parameters[index] = next;
                    setEditor({ ...editor, parameters });
                  }}
                  onRemove={() =>
                    setEditor({
                      ...editor,
                      parameters: editor.parameters.filter(
                        (_, item) => item !== index,
                      ),
                    })
                  }
                />
              ))}
              <button
                type="button"
                onClick={() =>
                  setEditor({
                    ...editor,
                    parameters: [
                      ...editor.parameters,
                      {
                        key: `param_${editor.parameters.length + 1}`,
                        label: `参数 ${editor.parameters.length + 1}`,
                        type: "text",
                      },
                    ],
                  })
                }
              >
                添加参数
              </button>
            </fieldset>
            <fieldset>
              <legend>互斥约束</legend>
              {editor.constraints.map((constraint, index) => {
                if (constraint.type === "mutuallyExclusive") {
                  return (
                    <div key={constraint.editorId} className="constraint-edit">
                      <div className="checks">
                        {constraintChoices(editor).map((choice) => (
                          <label key={choice.key}>
                            <input
                              type="checkbox"
                              checked={constraint.fields.includes(choice.key)}
                              onChange={(event) => {
                                const fields = event.target.checked
                                  ? [...constraint.fields, choice.key]
                                  : constraint.fields.filter(
                                      (field) => field !== choice.key,
                                    );
                                const constraints = editor.constraints.slice();
                                constraints[index] = {
                                  ...constraint,
                                  fields,
                                };
                                setEditor({ ...editor, constraints });
                              }}
                            />
                            {choice.label}
                          </label>
                        ))}
                      </div>
                      <button
                        type="button"
                        onClick={() =>
                          setEditor({
                            ...editor,
                            constraints: editor.constraints.filter(
                              (_, item) => item !== index,
                            ),
                          })
                        }
                      >
                        删除约束
                      </button>
                    </div>
                  );
                }
                if (constraint.type === "requires") {
                  return (
                    <div key={constraint.editorId} className="constraint-edit">
                      <p className="muted" style={{ margin: "4px 0" }}>
                        依赖约束：提供 <code>{constraint.field}</code>{" "}
                        时，必须提供{" "}
                        <code>{constraint.requiredFields.join("、")}</code>
                      </p>
                      <button
                        type="button"
                        onClick={() =>
                          setEditor({
                            ...editor,
                            constraints: editor.constraints.filter(
                              (_, item) => item !== index,
                            ),
                          })
                        }
                      >
                        删除约束
                      </button>
                    </div>
                  );
                }
                if (constraint.type === "conditionalVisibility") {
                  return (
                    <div key={constraint.editorId} className="constraint-edit">
                      <p className="muted" style={{ margin: "4px 0" }}>
                        条件可见性：<code>{constraint.field}</code> 联动依赖{" "}
                        <code>{constraint.dependsOn}</code>
                      </p>
                      <button
                        type="button"
                        onClick={() =>
                          setEditor({
                            ...editor,
                            constraints: editor.constraints.filter(
                              (_, item) => item !== index,
                            ),
                          })
                        }
                      >
                        删除约束
                      </button>
                    </div>
                  );
                }
                return null;
              })}
              <button
                type="button"
                onClick={() =>
                  setEditor({
                    ...editor,
                    constraints: [
                      ...editor.constraints,
                      {
                        type: "mutuallyExclusive",
                        fields: [],
                        editorId: crypto.randomUUID(),
                      },
                    ],
                  })
                }
              >
                添加互斥约束
              </button>
            </fieldset>
            <fieldset>
              <legend>传输覆盖</legend>
              <p className="muted">
                chatPath 和 modelsPath 留给 OpenAI 兼容文本，留空则用
                chat/completions 和 models。imagePath 和 imageEditPath
                留给图像接口，留空则用 images/generations 和 images/edits。
                speechPath 留给语音接口，留空则用 audio/speech。
              </p>
              <div className="form-grid">
                <label>
                  chatPath
                  <input
                    value={editor.chatPath}
                    onChange={(event) =>
                      setEditor({ ...editor, chatPath: event.target.value })
                    }
                    placeholder="chat/completions"
                  />
                </label>
                <label>
                  modelsPath
                  <input
                    value={editor.modelsPath}
                    onChange={(event) =>
                      setEditor({ ...editor, modelsPath: event.target.value })
                    }
                    placeholder="models"
                  />
                </label>
                <label>
                  imagePath
                  <input
                    value={editor.imagePath}
                    onChange={(event) =>
                      setEditor({ ...editor, imagePath: event.target.value })
                    }
                    placeholder="images/generations"
                  />
                </label>
                <label>
                  imageEditPath
                  <input
                    value={editor.imageEditPath}
                    onChange={(event) =>
                      setEditor({
                        ...editor,
                        imageEditPath: event.target.value,
                      })
                    }
                    placeholder="images/edits"
                  />
                </label>
                <label>
                  speechPath
                  <input
                    value={editor.speechPath}
                    onChange={(event) =>
                      setEditor({ ...editor, speechPath: event.target.value })
                    }
                    placeholder="audio/speech"
                  />
                </label>
              </div>
            </fieldset>
            {editorProvider?.type === "comfyui" ? (
              <fieldset>
                <legend>ComfyUI Workflow</legend>
                <p className="muted">
                  粘贴工作流 JSON，再用绑定把提示词、原图和参数填进节点输入。
                </p>
                <label>
                  Workflow JSON
                  <textarea
                    rows={8}
                    value={editor.workflowText}
                    onChange={(event) =>
                      setEditor({ ...editor, workflowText: event.target.value })
                    }
                    placeholder='{"6":{"class_type":"CLIPTextEncode","inputs":{"text":""}}}'
                  />
                </label>
                {editor.bindings.map((binding) => (
                  <div key={binding.editorId} className="form-grid">
                    <label>
                      键
                      <input
                        value={binding.key}
                        placeholder="prompt"
                        onChange={(event) =>
                          setEditor({
                            ...editor,
                            bindings: editor.bindings.map((item) =>
                              item.editorId === binding.editorId
                                ? { ...item, key: event.target.value }
                                : item,
                            ),
                          })
                        }
                      />
                    </label>
                    <label>
                      节点
                      <input
                        value={binding.node}
                        placeholder="6"
                        onChange={(event) =>
                          setEditor({
                            ...editor,
                            bindings: editor.bindings.map((item) =>
                              item.editorId === binding.editorId
                                ? { ...item, node: event.target.value }
                                : item,
                            ),
                          })
                        }
                      />
                    </label>
                    <label>
                      输入
                      <input
                        value={binding.input}
                        placeholder="text"
                        onChange={(event) =>
                          setEditor({
                            ...editor,
                            bindings: editor.bindings.map((item) =>
                              item.editorId === binding.editorId
                                ? { ...item, input: event.target.value }
                                : item,
                            ),
                          })
                        }
                      />
                    </label>
                    <button
                      type="button"
                      onClick={() =>
                        setEditor({
                          ...editor,
                          bindings: editor.bindings.filter(
                            (item) => item.editorId !== binding.editorId,
                          ),
                        })
                      }
                    >
                      删除绑定
                    </button>
                  </div>
                ))}
                <button
                  type="button"
                  onClick={() =>
                    setEditor({
                      ...editor,
                      bindings: [
                        ...editor.bindings,
                        {
                          editorId: crypto.randomUUID(),
                          key: "",
                          node: "",
                          input: "",
                        },
                      ],
                    })
                  }
                >
                  添加绑定
                </button>
                <button
                  type="button"
                  onClick={() => {
                    const workflow = parseWorkflow(editor.workflowText);
                    if (!workflow.ok || !workflow.value) {
                      setError({
                        code: "ValidationError",
                        message: workflow.ok
                          ? "先粘贴工作流 JSON"
                          : workflow.message,
                      });
                      return;
                    }
                    const suggestions = suggestWorkflowBindings(
                      workflow.value,
                      editor.modality,
                    );
                    const taken = new Set(
                      editor.bindings.map(
                        (item) => `${item.node}\n${item.input}`,
                      ),
                    );
                    const extra = suggestions.filter(
                      (item) => !taken.has(`${item.node}\n${item.input}`),
                    );
                    setError(null);
                    setEditor({
                      ...editor,
                      bindings: [
                        ...editor.bindings,
                        ...extra.map((item) => ({
                          editorId: crypto.randomUUID(),
                          key: item.key,
                          node: item.node,
                          input: item.input,
                        })),
                      ],
                    });
                  }}
                >
                  识别可绑定输入
                </button>
              </fieldset>
            ) : null}
            {editorProvider?.type === "generic-http" ? (
              <fieldset>
                <legend>HTTP 请求模板</legend>
                <p className="muted">
                  正文里用双花括号占位，例如 prompt、model、apiKey
                  和参数键。不要把真实密钥写进模板。异步任务要同时填写
                  taskIdPath 和 poll。
                </p>
                <label>
                  HTTP JSON
                  <textarea
                    rows={8}
                    value={editor.httpText}
                    onChange={(event) =>
                      setEditor({ ...editor, httpText: event.target.value })
                    }
                    placeholder='{"method":"POST","path":"v1/videos","body":{"prompt":"{{prompt}}"},"resultPath":"$.url"}'
                  />
                </label>
              </fieldset>
            ) : null}
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
            {error ? <ErrorNote error={error} /> : null}
            <div className="inline-actions">
              <button
                type="submit"
                className="primary"
                disabled={save.isPending}
              >
                {save.isPending ? "保存中…" : "保存模型"}
              </button>
              {editor.id ? (
                <button
                  type="button"
                  className="danger"
                  onClick={() => {
                    if (
                      editor.id &&
                      window.confirm(`删除模型「${editor.name}」？`)
                    ) {
                      void remove.mutateAsync(editor.id);
                    }
                  }}
                >
                  删除
                </button>
              ) : null}
            </div>
          </form>
        ) : (
          <p className="muted">选择一个模型，或新建模型定义。</p>
        )}
      </div>
    </section>
  );
}

function ParameterEditor({
  parameter,
  onChange,
  onRemove,
}: {
  parameter: ParameterDefinition;
  onChange: (parameter: ParameterDefinition) => void;
  onRemove: () => void;
}) {
  const numeric = parameter.type === "number" || parameter.type === "range";
  return (
    <div className="param-edit">
      <div className="form-grid">
        <label>
          名称
          <input
            value={parameter.label}
            onChange={(event) =>
              onChange({ ...parameter, label: event.target.value })
            }
          />
        </label>
        <label>
          键
          <input
            value={parameter.key}
            onChange={(event) =>
              onChange({ ...parameter, key: event.target.value })
            }
          />
        </label>
        <label>
          类型
          <select
            value={parameter.type}
            onChange={(event) =>
              onChange({
                ...parameter,
                type: event.target.value as ParameterType,
                options: undefined,
              })
            }
          >
            {(Object.keys(PARAMETER_TYPE_LABELS) as ParameterType[]).map(
              (type) => (
                <option key={type} value={type}>
                  {PARAMETER_TYPE_LABELS[type]}
                </option>
              ),
            )}
          </select>
        </label>
      </div>
      <div className="checks">
        <label>
          <input
            type="checkbox"
            checked={parameter.advanced === true}
            onChange={(event) =>
              onChange({ ...parameter, advanced: event.target.checked })
            }
          />
          高级
        </label>
        <label>
          <input
            type="checkbox"
            checked={parameter.required === true}
            onChange={(event) =>
              onChange({ ...parameter, required: event.target.checked })
            }
          />
          必填
        </label>
      </div>
      {numeric ? (
        <div className="form-grid">
          <label>
            最小
            <input
              type="number"
              value={parameter.min ?? ""}
              onChange={(event) =>
                onChange({
                  ...parameter,
                  min:
                    event.target.value === ""
                      ? undefined
                      : Number(event.target.value),
                })
              }
            />
          </label>
          <label>
            最大
            <input
              type="number"
              value={parameter.max ?? ""}
              onChange={(event) =>
                onChange({
                  ...parameter,
                  max:
                    event.target.value === ""
                      ? undefined
                      : Number(event.target.value),
                })
              }
            />
          </label>
          <label>
            步长
            <input
              type="number"
              value={parameter.step ?? ""}
              onChange={(event) =>
                onChange({
                  ...parameter,
                  step:
                    event.target.value === ""
                      ? undefined
                      : Number(event.target.value),
                })
              }
            />
          </label>
          <label>
            默认
            <input
              type="number"
              value={
                typeof parameter.default === "number" ? parameter.default : ""
              }
              onChange={(event) =>
                onChange({
                  ...parameter,
                  default:
                    event.target.value === ""
                      ? undefined
                      : Number(event.target.value),
                })
              }
            />
          </label>
        </div>
      ) : null}
      {parameter.type === "boolean" ? (
        <label className="check">
          <input
            type="checkbox"
            checked={parameter.default === true}
            onChange={(event) =>
              onChange({ ...parameter, default: event.target.checked })
            }
          />
          默认开启
        </label>
      ) : null}
      {parameter.type === "text" || parameter.type === "textarea" ? (
        <label>
          默认
          <input
            value={
              typeof parameter.default === "string" ? parameter.default : ""
            }
            onChange={(event) =>
              onChange({
                ...parameter,
                default:
                  event.target.value === "" ? undefined : event.target.value,
              })
            }
          />
        </label>
      ) : null}
      {parameter.type === "select" ? (
        <SelectOptions
          options={parameter.options ?? []}
          onChange={(options) => onChange({ ...parameter, options })}
        />
      ) : null}
      <button type="button" onClick={onRemove}>
        删除参数
      </button>
    </div>
  );
}

function SelectOptions({
  options,
  onChange,
}: {
  options: SelectOption[];
  onChange: (options: SelectOption[]) => void;
}) {
  return (
    <div className="options">
      {options.map((option, index) => (
        <div key={`${option.label}-${index}`} className="form-grid">
          <input
            value={option.label}
            aria-label="选项名称"
            onChange={(event) => {
              const next = options.slice();
              next[index] = { ...option, label: event.target.value };
              onChange(next);
            }}
          />
          <input
            value={String(option.value)}
            aria-label="选项值"
            onChange={(event) => {
              const next = options.slice();
              next[index] = { ...option, value: event.target.value };
              onChange(next);
            }}
          />
          <button
            type="button"
            onClick={() =>
              onChange(options.filter((_, item) => item !== index))
            }
          >
            删除选项
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={() =>
          onChange([...options, { label: "选项", value: "value" }])
        }
      >
        添加选项
      </button>
    </div>
  );
}

function blank(providerId: string): EditorState {
  return {
    id: null,
    name: "",
    providerId,
    modelId: "",
    modality: "text",
    capabilities: emptyCapabilities("text"),
    parameters: [],
    constraints: [],
    chatPath: "",
    modelsPath: "",
    imagePath: "",
    imageEditPath: "",
    speechPath: "",
    workflowText: "",
    bindings: [],
    httpText: "",
    enabled: true,
  };
}

function fromModel(model: ModelDefinition): EditorState {
  return {
    id: model.id,
    name: model.name,
    providerId: model.providerId,
    modelId: model.modelId,
    modality: model.modality,
    capabilities: model.capabilities,
    parameters: model.parameters,
    constraints: model.constraints.map((constraint) => ({
      ...constraint,
      editorId: crypto.randomUUID(),
    })),
    chatPath: model.transport?.chatPath ?? "",
    modelsPath: model.transport?.modelsPath ?? "",
    imagePath: model.transport?.imagePath ?? "",
    imageEditPath: model.transport?.imageEditPath ?? "",
    speechPath: model.transport?.speechPath ?? "",
    workflowText: model.transport?.workflow
      ? JSON.stringify(model.transport.workflow, null, 2)
      : "",
    bindings: (model.transport?.bindings ?? []).map((binding) => ({
      ...binding,
      editorId: crypto.randomUUID(),
    })),
    httpText: model.transport?.http
      ? JSON.stringify(model.transport.http, null, 2)
      : "",
    enabled: model.enabled,
  };
}

const VIDEO_INPUTS = [
  { key: "firstFrame", label: "首帧" },
  { key: "lastFrame", label: "尾帧" },
  { key: "referenceImages", label: "参考图" },
  { key: "referenceVideos", label: "参考视频" },
  { key: "referenceAudio", label: "参考音频" },
];

const AUDIO_INPUTS = [{ key: "referenceAudio", label: "参考音频" }];

function constraintChoices(
  editor: EditorState,
): { key: string; label: string }[] {
  const fromParameters = editor.parameters.map((parameter) => ({
    key: parameter.key,
    label: parameter.label || parameter.key,
  }));
  const extras =
    editor.modality === "video"
      ? VIDEO_INPUTS
      : editor.modality === "audio"
        ? AUDIO_INPUTS
        : [];
  if (extras.length === 0) {
    return fromParameters;
  }
  const present = new Set(fromParameters.map((item) => item.key));
  return [
    ...fromParameters,
    ...extras.filter((item) => !present.has(item.key)),
  ];
}

function parseWorkflow(
  text: string,
):
  | { ok: true; value?: Record<string, unknown> }
  | { ok: false; message: string } {
  const trimmed = text.trim();
  if (!trimmed) {
    return { ok: true };
  }
  try {
    const value: unknown = JSON.parse(trimmed);
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      return { ok: false, message: "ComfyUI Workflow 必须是对象" };
    }
    return { ok: true, value: value as Record<string, unknown> };
  } catch {
    return { ok: false, message: "ComfyUI Workflow 不是合法 JSON" };
  }
}

function parseHttp(
  text: string,
): { ok: true; value?: HttpRecipe } | { ok: false; message: string } {
  const trimmed = text.trim();
  if (!trimmed) {
    return { ok: true };
  }
  try {
    const value: unknown = JSON.parse(trimmed);
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      return { ok: false, message: "HTTP 请求模板必须是对象" };
    }
    return { ok: true, value: value as HttpRecipe };
  } catch {
    return { ok: false, message: "HTTP 请求模板不是合法 JSON" };
  }
}

function toInput(editor: EditorState): ModelInput {
  const workflow = parseWorkflow(editor.workflowText);
  if (!workflow.ok) {
    throw new Error(workflow.message);
  }
  const http = parseHttp(editor.httpText);
  if (!http.ok) {
    throw new Error(http.message);
  }
  const bindings = editor.bindings
    .map((item) => ({
      key: item.key.trim(),
      node: item.node.trim(),
      input: item.input.trim(),
    }))
    .filter((item) => item.key || item.node || item.input);
  if (bindings.some((item) => !item.key || !item.node || !item.input)) {
    throw new Error("工作流绑定需要键、节点和输入");
  }
  const chatPath = editor.chatPath.trim();
  const modelsPath = editor.modelsPath.trim();
  const imagePath = editor.imagePath.trim();
  const imageEditPath = editor.imageEditPath.trim();
  const speechPath = editor.speechPath.trim();
  const hasTransport = Boolean(
    chatPath ||
      modelsPath ||
      imagePath ||
      imageEditPath ||
      speechPath ||
      workflow.value ||
      bindings.length > 0 ||
      http.value,
  );
  return {
    name: editor.name,
    providerId: editor.providerId,
    modelId: editor.modelId,
    modality: editor.modality,
    capabilities: editor.capabilities,
    parameters: editor.parameters,
    constraints: editor.constraints
      .filter((constraint) => {
        if (constraint.type === "mutuallyExclusive") {
          return constraint.fields.length >= 2;
        }
        return true;
      })
      .map((constraint) => {
        const { editorId: _editorId, ...rest } = constraint;
        return rest as ConstraintDefinition;
      }),
    transport: hasTransport
      ? {
          chatPath: chatPath || undefined,
          modelsPath: modelsPath || undefined,
          imagePath: imagePath || undefined,
          imageEditPath: imageEditPath || undefined,
          speechPath: speechPath || undefined,
          workflow: workflow.value,
          bindings: bindings.length > 0 ? bindings : undefined,
          http: http.value,
        }
      : null,
    enabled: editor.enabled,
  };
}
