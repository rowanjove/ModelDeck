import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, asAppError } from "../../api";
import { DialogFrame } from "../../components/DialogFrame";
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
  useAudioDraft,
  useImageDraft,
  useTextDraft,
  useVideoDraft,
} from "../../store";
import {
  type Modality,
  type ModelDefinition,
  PROVIDER_DEFAULTS,
  PROVIDER_LABELS,
  type Provider,
  type ProviderType,
} from "../../types";

export type SetupPurpose = "chat" | "image" | "video" | "audio" | "local";

interface PurposeOption {
  id: SetupPurpose;
  title: string;
  badge: string;
  description: string;
  recommendedModality: Modality;
  recommendedProviders: ProviderType[];
}

const PURPOSE_OPTIONS: PurposeOption[] = [
  {
    id: "chat",
    title: "文本对话与推理",
    badge: "推荐",
    description: "日常问答、代码辅助、文本理解与结构化输出",
    recommendedModality: "text",
    recommendedProviders: [
      "openai-compatible",
      "ollama",
      "openai",
      "anthropic",
      "gemini",
    ],
  },
  {
    id: "image",
    title: "图像生成",
    badge: "多模态",
    description: "文生图、图生图与视觉素材渲染",
    recommendedModality: "image",
    recommendedProviders: ["openai", "comfyui", "fal", "replicate"],
  },
  {
    id: "video",
    title: "视频生成",
    badge: "多模态",
    description: "文生视频与首帧驱动视频",
    recommendedModality: "video",
    recommendedProviders: ["comfyui", "fal", "replicate"],
  },
  {
    id: "audio",
    title: "语音合成",
    badge: "多模态",
    description: "文本转语音 (TTS) 与参考音频驱动",
    recommendedModality: "audio",
    recommendedProviders: ["openai"],
  },
  {
    id: "local",
    title: "本地离线服务",
    badge: "免外部网络",
    description: "连接本地 Ollama 或 ComfyUI，离线运行且无云端调用费用",
    recommendedModality: "text",
    recommendedProviders: ["ollama", "comfyui", "openai-compatible"],
  },
];

export function diagnoseConnectionError(
  error: unknown,
  isLocal: boolean,
): string {
  const errStr =
    typeof error === "string"
      ? error
      : (error as { message?: string })?.message || "";
  const lower = errStr.toLowerCase();

  if (
    errStr.includes("401") ||
    lower.includes("unauthorized") ||
    lower.includes("invalid_api_key") ||
    lower.includes("incorrect api key")
  ) {
    return "API 凭据无效或已过期：请核对服务商控制台中的 API Key 是否正确复制，并确认前后未包含空格或多余字符。";
  }
  if (
    errStr.includes("403") ||
    lower.includes("permission_denied") ||
    lower.includes("forbidden")
  ) {
    return "权限不足：当前 API Key 缺少对应接口权限，或所在地域受到限制。";
  }
  if (
    errStr.includes("429") ||
    lower.includes("insufficient_quota") ||
    lower.includes("rate_limit") ||
    lower.includes("quota exceeded")
  ) {
    return "余额不足或超出频率限制：请前往服务商控制台检查账户额度与充值状态。";
  }
  if (
    errStr.includes("ECONNREFUSED") ||
    errStr.includes("连接失败") ||
    errStr.includes("Connection refused") ||
    errStr.includes("10061") ||
    errStr.includes("failed to connect")
  ) {
    return isLocal
      ? "未能连接到本地服务端口：请确认对应后台服务已正常启动（例如在终端执行 `ollama serve` 或启动 ComfyUI），且端口设置无冲突。"
      : "无法连接到目标服务地址：请检查网络连通性，或核对 Base URL 拼写。";
  }
  if (lower.includes("timeout") || errStr.includes("超时")) {
    return "请求连接超时：若处于受限网络或需外部代理，请前往“设置 - 网络代理”配置 HTTP/SOCKS5 代理。";
  }
  return `连接失败：${errStr.slice(0, 160)}。请检查网络状态与服务配置。`;
}

export function SetupWizardModal({
  onClose,
  onFinishAndNavigate,
}: {
  onClose: () => void;
  onFinishAndNavigate: (route: string) => void;
}) {
  const queryClient = useQueryClient();
  const [step, setStep] = useState<1 | 2 | 3 | 4>(1);
  const [dontRemindAgain, setDontRemindAgain] = useState(
    () => localStorage.getItem("modeldeck_setup_wizard_never_remind") === "1",
  );

  // 步骤 1：用途
  const [purpose, setPurpose] = useState<SetupPurpose>("chat");

  // 步骤 2：服务商配置
  const [providerType, setProviderType] =
    useState<ProviderType>("openai-compatible");
  const [providerName, setProviderName] = useState("DeepSeek API");
  const [baseUrl, setBaseUrl] = useState("https://api.deepseek.com");
  const [apiKey, setApiKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [createdProvider, setCreatedProvider] = useState<Provider | null>(null);
  const [connectionLatency, setConnectionLatency] = useState<number | null>(
    null,
  );
  const [connectionDiagnostic, setConnectionDiagnostic] = useState<
    string | null
  >(null);
  const [isTesting, setIsTesting] = useState(false);

  // 步骤 3：模型选择
  const [selectedModelId, setSelectedModelId] = useState("");
  const [createdModel, setCreatedModel] = useState<ModelDefinition | null>(
    null,
  );
  const [remoteModelsList, setRemoteModelsList] = useState<string[]>([]);
  const [isDetectingRemote, setIsDetectingRemote] = useState(false);
  const [modelStepError, setModelStepError] = useState<string | null>(null);

  const isLocalService =
    providerType === "ollama" || providerType === "comfyui";

  // 读取现有模板供快速匹配
  const registryQuery = useQuery({
    queryKey: ["registry"],
    queryFn: api.registryGet,
  });

  // 处理用途切换时的智能预选
  function handleSelectPurpose(p: SetupPurpose) {
    setPurpose(p);
    if (p === "chat") {
      setProviderType("openai-compatible");
      setProviderName("DeepSeek API");
      setBaseUrl("https://api.deepseek.com");
      setSelectedModelId("deepseek-chat");
    } else if (p === "image") {
      setProviderType("openai");
      setProviderName("OpenAI DALL-E");
      setBaseUrl(PROVIDER_DEFAULTS.openai);
      setSelectedModelId("dall-e-3");
    } else if (p === "video") {
      setProviderType("comfyui");
      setProviderName("ComfyUI 本地视频");
      setBaseUrl(PROVIDER_DEFAULTS.comfyui);
      setSelectedModelId("comfyui-video-default");
    } else if (p === "audio") {
      setProviderType("openai");
      setProviderName("OpenAI 语音合成");
      setBaseUrl(PROVIDER_DEFAULTS.openai);
      setSelectedModelId("tts-1");
    } else if (p === "local") {
      setProviderType("ollama");
      setProviderName("Ollama 本地服务");
      setBaseUrl(PROVIDER_DEFAULTS.ollama);
      setSelectedModelId("llama3.2");
    }
  }

  // 切换 Provider 类型时刷新默认 Base URL
  function handleProviderTypeChange(type: ProviderType) {
    setProviderType(type);
    setBaseUrl(PROVIDER_DEFAULTS[type] || "");
    if (type === "ollama") {
      setProviderName("Ollama 本地服务");
      setSelectedModelId("llama3.2");
    } else if (type === "comfyui") {
      setProviderName("ComfyUI 本地服务");
      setSelectedModelId("comfyui-default");
    } else if (type === "openai") {
      setProviderName("OpenAI 官方服务");
      setSelectedModelId("gpt-4o-mini");
    } else if (type === "anthropic") {
      setProviderName("Anthropic Claude");
      setSelectedModelId("claude-3-5-sonnet-20241022");
    } else if (type === "gemini") {
      setProviderName("Google Gemini");
      setSelectedModelId("gemini-2.5-flash");
    } else if (type === "openai-compatible") {
      setProviderName("OpenAI 兼容端点");
      setSelectedModelId("deepseek-chat");
    } else {
      setProviderName(PROVIDER_LABELS[type] || "自定义服务");
    }
  }

  // 测试并创建 Provider
  const saveAndTestProvider = useMutation({
    mutationFn: async () => {
      setIsTesting(true);
      setConnectionDiagnostic(null);
      setConnectionLatency(null);

      // 1. 创建 Provider
      const provider = await api.createProvider({
        name: providerName.trim() || PROVIDER_LABELS[providerType],
        type: providerType,
        baseUrl: baseUrl.trim(),
        apiKey: apiKey.trim() ? apiKey.trim() : null,
        clearApiKey: false,
        headers: {},
        timeoutMs: 15000,
        enabled: true,
      });

      // 2. 测试连接
      try {
        const report = await api.testProvider(provider.id);
        setConnectionLatency(report.latencyMs);
        return { provider, report };
      } catch (testErr) {
        // 遇到测试错误，先保留已创建的 provider 但给出明确诊断
        const diag = diagnoseConnectionError(testErr, isLocalService);
        setConnectionDiagnostic(diag);
        throw new Error(diag);
      } finally {
        setIsTesting(false);
      }
    },
    onSuccess: async ({ provider }) => {
      setCreatedProvider(provider);
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      // 顺利通过，进入模型步骤
      setStep(3);
    },
    onError: (err) => {
      setIsTesting(false);
      setConnectionDiagnostic(err.message);
    },
  });

  // 探测本地模型
  async function handleDetectRemoteModels() {
    if (!createdProvider) return;
    setIsDetectingRemote(true);
    setModelStepError(null);
    try {
      const models = await api.remoteModels(createdProvider.id);
      const ids = models.map((m) => m.id);
      setRemoteModelsList(ids);
      if (ids.length > 0) {
        setSelectedModelId(ids[0]);
      } else {
        setModelStepError(
          "未检测到本地已安装的模型。请确认本地服务已下载模型（例如在终端执行 `ollama run llama3.2`）。",
        );
      }
    } catch (err) {
      setModelStepError(diagnoseConnectionError(err, isLocalService));
    } finally {
      setIsDetectingRemote(false);
    }
  }

  // 步骤 3：导入或创建首个模型
  const createFirstModel = useMutation({
    mutationFn: async () => {
      if (!createdProvider) throw new Error("缺失 Provider");
      const modelIdToUse = selectedModelId.trim() || "default-model";

      // 检查 Registry 是否有匹配模板
      const catalog = registryQuery.data;
      const matchedTemplate = catalog?.models.find(
        (m) =>
          m.providerType === createdProvider.type &&
          (m.modelId.toLowerCase() === modelIdToUse.toLowerCase() ||
            m.id.toLowerCase() === modelIdToUse.toLowerCase()),
      );

      if (matchedTemplate) {
        // 使用 registryImportModel 一键导入
        return await api.registryImportModel(
          matchedTemplate.id,
          createdProvider.id,
          true,
        );
      }

      // 根据用途确定模态与默认参数模板
      const activeOption = PURPOSE_OPTIONS.find((p) => p.id === purpose);
      const modality: Modality = activeOption?.recommendedModality ?? "text";

      const caps =
        modality === "text"
          ? textCapabilities
          : modality === "image"
            ? imageCapabilities
            : modality === "video"
              ? videoCapabilities
              : audioCapabilities;

      const params =
        modality === "text"
          ? textParameters
          : modality === "image"
            ? imageParameters
            : modality === "video"
              ? videoParameters
              : audioParameters;

      return await api.createModel({
        name: modelIdToUse,
        providerId: createdProvider.id,
        modelId: modelIdToUse,
        modality,
        capabilities: caps,
        parameters: params,
        constraints: [],
        transport: {
          chatPath: "/v1/chat/completions",
          modelsPath: "/v1/models",
          imagePath: "/v1/images/generations",
          imageEditPath: undefined,
          speechPath: "/v1/audio/speech",
          workflow: undefined,
          bindings: [],
          http: undefined,
        },
        enabled: true,
      });
    },
    onSuccess: async (model) => {
      setCreatedModel(model);
      await queryClient.invalidateQueries({ queryKey: ["models"] });
      // 预选至对应模态 Draft 中，省去用户再次选择
      if (model.modality === "text") {
        useTextDraft.getState().patch({
          modelDefinitionId: model.id,
          boundModelId: model.modelId,
        });
      } else if (model.modality === "image") {
        useImageDraft.getState().patch({
          modelDefinitionId: model.id,
          boundModelId: model.modelId,
        });
      } else if (model.modality === "video") {
        useVideoDraft.getState().patch({
          modelDefinitionId: model.id,
          boundModelId: model.modelId,
        });
      } else if (model.modality === "audio") {
        useAudioDraft.getState().patch({
          modelDefinitionId: model.id,
          boundModelId: model.modelId,
        });
      }
      setStep(4);
    },
    onError: (err) => {
      setModelStepError(asAppError(err).message);
    },
  });

  // 完成并离开
  function handleDismiss() {
    if (dontRemindAgain) {
      localStorage.setItem("modeldeck_setup_wizard_never_remind", "1");
    } else {
      localStorage.removeItem("modeldeck_setup_wizard_never_remind");
    }
    localStorage.setItem("modeldeck_setup_wizard_dismissed", "1");
    onClose();
  }

  function handleFinish() {
    localStorage.setItem("modeldeck_setup_wizard_dismissed", "1");
    localStorage.setItem("modeldeck_setup_wizard_completed", "1");
    const activeOption = PURPOSE_OPTIONS.find((p) => p.id === purpose);
    const targetRoute =
      activeOption?.recommendedModality === "image"
        ? "image"
        : activeOption?.recommendedModality === "video"
          ? "video"
          : activeOption?.recommendedModality === "audio"
            ? "audio"
            : "text";
    onFinishAndNavigate(targetRoute);
  }

  return (
    <DialogFrame label="ModelDeck 首次使用向导" onClose={handleDismiss}>
      <div
        className="switcher"
        style={{
          width: "min(680px, calc(100vw - 32px))",
          maxHeight: "86vh",
          display: "flex",
          flexDirection: "column",
          gap: "16px",
          padding: "24px 28px",
        }}
      >
        {/* 顶部标题与进度 */}
        <header
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "flex-start",
            borderBottom: "1px solid var(--line)",
            paddingBottom: "14px",
          }}
        >
          <div>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: "8px",
                marginBottom: "4px",
              }}
            >
              <h2
                style={{
                  margin: 0,
                  fontSize: "18px",
                  fontWeight: 600,
                  color: "var(--ink)",
                }}
              >
                ModelDeck 首次使用向导
              </h2>
              <span
                style={{
                  fontSize: "12px",
                  padding: "2px 8px",
                  borderRadius: "12px",
                  background: "var(--accent-subtle)",
                  color: "var(--accent)",
                  fontWeight: 500,
                }}
              >
                步骤 {step} / 4
              </span>
            </div>
            <p
              style={{
                margin: 0,
                fontSize: "13px",
                color: "var(--soft)",
                lineHeight: 1.4,
              }}
            >
              {step === 1 && "选择主要使用场景，后续可在设置中随时扩展或调整。"}
              {step === 2 && "配置模型服务端点并测试网络连通性。"}
              {step === 3 && "指定或自动发现模型，确认支持的能力与默认参数。"}
              {step === 4 && "配置已就绪，可直接进入对应工作区开始使用。"}
            </p>
          </div>
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-end",
              gap: "4px",
            }}
          >
            <button
              type="button"
              onClick={handleDismiss}
              style={{
                background: "transparent",
                border: "none",
                color: "var(--muted)",
                fontSize: "13px",
                cursor: "pointer",
                padding: "4px 8px",
                borderRadius: "4px",
              }}
              title="跳过向导，稍后可随时在设置中重新运行"
            >
              以后再设置 (跳过)
            </button>
            <label
              style={{
                display: "flex",
                alignItems: "center",
                gap: "5px",
                fontSize: "12px",
                color: "var(--soft)",
                cursor: "pointer",
                userSelect: "none",
                paddingRight: "6px",
              }}
            >
              <input
                type="checkbox"
                checked={dontRemindAgain}
                onChange={(e) => setDontRemindAgain(e.target.checked)}
                style={{ cursor: "pointer" }}
              />
              以后不再提醒
            </label>
          </div>
        </header>

        {/* 步骤内容区域 */}
        <div
          style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "4px 0" }}
        >
          {/* STEP 1: 选择用途 */}
          {step === 1 && (
            <div style={{ display: "grid", gap: "10px" }}>
              <p
                style={{
                  margin: "0 0 4px 0",
                  fontSize: "13px",
                  fontWeight: 500,
                  color: "var(--ink)",
                }}
              >
                选择默认工作场景（其他模态随时可在设置中添加）：
              </p>
              {PURPOSE_OPTIONS.map((opt) => {
                const isSelected = purpose === opt.id;
                return (
                  <button
                    key={opt.id}
                    type="button"
                    onClick={() => handleSelectPurpose(opt.id)}
                    style={{
                      display: "flex",
                      alignItems: "flex-start",
                      justifyContent: "space-between",
                      gap: "12px",
                      padding: "12px 14px",
                      background: isSelected
                        ? "var(--accent-subtle)"
                        : "var(--raised)",
                      border: `1px solid ${
                        isSelected ? "var(--accent)" : "var(--line)"
                      }`,
                      borderRadius: "var(--radius-md)",
                      textAlign: "left",
                      cursor: "pointer",
                      transition: "all var(--transition-fast)",
                    }}
                  >
                    <div>
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: "8px",
                          marginBottom: "4px",
                        }}
                      >
                        <strong
                          style={{ fontSize: "14px", color: "var(--ink)" }}
                        >
                          {opt.title}
                        </strong>
                        <span
                          style={{
                            fontSize: "11px",
                            padding: "1px 6px",
                            borderRadius: "10px",
                            background: "var(--sunken)",
                            color: "var(--soft)",
                          }}
                        >
                          {opt.badge}
                        </span>
                      </div>
                      <p
                        style={{
                          margin: 0,
                          fontSize: "12px",
                          color: "var(--soft)",
                        }}
                      >
                        {opt.description}
                      </p>
                    </div>

                    <span
                      style={{
                        width: "18px",
                        height: "18px",
                        borderRadius: "50%",
                        border: `2px solid ${
                          isSelected ? "var(--accent)" : "var(--line-strong)"
                        }`,
                        background: isSelected
                          ? "var(--accent)"
                          : "transparent",
                        display: "inline-flex",
                        alignItems: "center",
                        justifyContent: "center",
                        marginTop: "2px",
                      }}
                    >
                      {isSelected ? (
                        <span
                          style={{
                            width: "6px",
                            height: "6px",
                            borderRadius: "50%",
                            background: "#fff",
                          }}
                        />
                      ) : null}
                    </span>
                  </button>
                );
              })}

              <div
                style={{
                  marginTop: "4px",
                  padding: "8px 12px",
                  borderRadius: "var(--radius-sm)",
                  background: "var(--surface-2)",
                  border: "1px solid var(--line)",
                  fontSize: "12px",
                  color: "var(--soft)",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                }}
              >
                <span>全局命令面板快捷键：</span>
                <span
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    gap: "4px",
                  }}
                >
                  <kbd
                    style={{
                      padding: "1px 6px",
                      background: "var(--raised)",
                      border: "1px solid var(--line)",
                      borderRadius: "3px",
                      fontSize: "11px",
                      color: "var(--ink)",
                      fontFamily: "monospace",
                    }}
                  >
                    Ctrl
                  </kbd>
                  <span>+</span>
                  <kbd
                    style={{
                      padding: "1px 6px",
                      background: "var(--raised)",
                      border: "1px solid var(--line)",
                      borderRadius: "3px",
                      fontSize: "11px",
                      color: "var(--ink)",
                      fontFamily: "monospace",
                    }}
                  >
                    K
                  </kbd>
                  <span style={{ marginLeft: "4px", fontSize: "11px" }}>
                    （任意界面均可呼出进行页面跳转与搜索）
                  </span>
                </span>
              </div>
            </div>
          )}

          {/* STEP 2: 配置 Provider */}
          {step === 2 && (
            <div style={{ display: "grid", gap: "14px" }}>
              {/* 服务类别指示条 */}
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: "10px",
                  padding: "10px 14px",
                  borderRadius: "var(--radius-md)",
                  background: isLocalService
                    ? "var(--ok-subtle)"
                    : "var(--info-subtle)",
                  border: `1px solid ${
                    isLocalService ? "var(--ok-border)" : "var(--info-border)"
                  }`,
                  fontSize: "13px",
                  color: "var(--ink)",
                }}
              >
                <span>{isLocalService ? "💻" : "☁️"}</span>
                <div>
                  <strong>
                    {isLocalService ? "本地私有服务" : "云端 API 服务"}
                  </strong>
                  <span style={{ marginLeft: "6px", color: "var(--soft)" }}>
                    {isLocalService
                      ? "无需 API Key，模型在本地显卡/CPU 离线计算，需确认对应后台服务已开启"
                      : "需填入对应服务商的 API Key，请求通过互联网直接连通官方接口"}
                  </span>
                </div>
              </div>

              {/* Provider 类别下拉 */}
              <label style={{ display: "grid", gap: "6px" }}>
                <span style={{ fontSize: "13px", fontWeight: 500 }}>
                  服务商类型 (Provider Type)
                </span>
                <select
                  value={providerType}
                  onChange={(e) =>
                    handleProviderTypeChange(e.target.value as ProviderType)
                  }
                  style={{
                    padding: "8px 10px",
                    borderRadius: "var(--radius-sm)",
                    border: "1px solid var(--line)",
                    background: "var(--raised)",
                  }}
                >
                  <optgroup label="主流云端服务商">
                    <option value="openai-compatible">
                      OpenAI 兼容规范 (DeepSeek / 硅基流动 / 自建代理)
                    </option>
                    <option value="openai">OpenAI 官方服务</option>
                    <option value="anthropic">Anthropic (Claude)</option>
                    <option value="gemini">Google Gemini</option>
                    <option value="fal">fal.ai (图像/视频云端加速)</option>
                    <option value="replicate">Replicate</option>
                  </optgroup>
                  <optgroup label="本地私有服务">
                    <option value="ollama">Ollama (本地私有大模型)</option>
                    <option value="comfyui">
                      ComfyUI (本地节点生图/生视频)
                    </option>
                  </optgroup>
                </select>
              </label>

              {/* 名称与 Base URL */}
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "1fr 1.6fr",
                  gap: "10px",
                }}
              >
                <label style={{ display: "grid", gap: "6px" }}>
                  <span style={{ fontSize: "13px", fontWeight: 500 }}>
                    显示名称
                  </span>
                  <input
                    type="text"
                    value={providerName}
                    onChange={(e) => setProviderName(e.target.value)}
                    style={{
                      padding: "8px 10px",
                      borderRadius: "var(--radius-sm)",
                      border: "1px solid var(--line)",
                      background: "var(--raised)",
                    }}
                  />
                </label>
                <label style={{ display: "grid", gap: "6px" }}>
                  <span style={{ fontSize: "13px", fontWeight: 500 }}>
                    Base URL 端点地址
                  </span>
                  <input
                    type="text"
                    value={baseUrl}
                    onChange={(e) => setBaseUrl(e.target.value)}
                    style={{
                      padding: "8px 10px",
                      borderRadius: "var(--radius-sm)",
                      border: "1px solid var(--line)",
                      background: "var(--raised)",
                    }}
                  />
                </label>
              </div>

              {/* API Key (仅非本地服务显示) */}
              {!isLocalService ? (
                <label style={{ display: "grid", gap: "6px" }}>
                  <span style={{ fontSize: "13px", fontWeight: 500 }}>
                    API Key (密钥仅安全持久化于系统本地凭据存储，不写入明文配置)
                  </span>
                  <div style={{ display: "flex", gap: "6px" }}>
                    <input
                      type={showKey ? "text" : "password"}
                      value={apiKey}
                      placeholder="sk-..."
                      onChange={(e) => setApiKey(e.target.value)}
                      style={{
                        flex: 1,
                        padding: "8px 10px",
                        borderRadius: "var(--radius-sm)",
                        border: "1px solid var(--line)",
                        background: "var(--raised)",
                      }}
                    />
                    <button
                      type="button"
                      onClick={() => setShowKey(!showKey)}
                      style={{
                        padding: "0 12px",
                        borderRadius: "var(--radius-sm)",
                        border: "1px solid var(--line)",
                        background: "var(--surface-2)",
                        fontSize: "12px",
                      }}
                    >
                      {showKey ? "隐藏" : "显示"}
                    </button>
                  </div>
                </label>
              ) : (
                <p
                  style={{
                    margin: 0,
                    fontSize: "12px",
                    color: "var(--soft)",
                    background: "var(--sunken-subtle)",
                    padding: "8px 12px",
                    borderRadius: "var(--radius-sm)",
                  }}
                >
                  💡 本地服务不需要填写 API
                  Key。请确保本地后台正在运行监听对应端口。
                </p>
              )}

              {/* 连通性测试报告或错误提示 */}
              {connectionLatency !== null && (
                <div
                  style={{
                    padding: "8px 12px",
                    borderRadius: "var(--radius-sm)",
                    background: "var(--ok-subtle)",
                    border: "1px solid var(--ok-border)",
                    color: "var(--ok)",
                    fontSize: "13px",
                    display: "flex",
                    alignItems: "center",
                    gap: "8px",
                  }}
                >
                  <span>✓</span>
                  <span>连通性测试成功！响应延迟：{connectionLatency} ms</span>
                </div>
              )}

              {connectionDiagnostic && (
                <div
                  style={{
                    padding: "10px 12px",
                    borderRadius: "var(--radius-sm)",
                    background: "var(--danger-subtle)",
                    border: "1px solid var(--danger-border)",
                    color: "var(--danger)",
                    fontSize: "13px",
                    lineHeight: 1.4,
                  }}
                >
                  <strong>诊断建议：</strong> {connectionDiagnostic}
                </div>
              )}
            </div>
          )}

          {/* STEP 3: 模型选择与导入 */}
          {step === 3 && (
            <div style={{ display: "grid", gap: "14px" }}>
              <div
                style={{
                  padding: "10px 14px",
                  borderRadius: "var(--radius-md)",
                  background: "var(--surface-2)",
                  fontSize: "13px",
                }}
              >
                已连通服务商：
                <strong> {createdProvider?.name}</strong> (
                <code>{createdProvider?.type}</code>)
              </div>

              {isLocalService && providerType === "ollama" && (
                <div>
                  <div
                    style={{
                      display: "flex",
                      justifyContent: "space-between",
                      alignItems: "center",
                      marginBottom: "6px",
                    }}
                  >
                    <span style={{ fontSize: "13px", fontWeight: 500 }}>
                      探测本地 Ollama 模型
                    </span>
                    <button
                      type="button"
                      disabled={isDetectingRemote}
                      onClick={handleDetectRemoteModels}
                      style={{
                        fontSize: "12px",
                        padding: "4px 8px",
                        borderRadius: "var(--radius-sm)",
                        border: "1px solid var(--line)",
                        background: "var(--surface-1)",
                      }}
                    >
                      {isDetectingRemote ? "正在探测..." : "🔍 刷新已下载模型"}
                    </button>
                  </div>
                  {remoteModelsList.length > 0 ? (
                    <select
                      value={selectedModelId}
                      onChange={(e) => setSelectedModelId(e.target.value)}
                      style={{
                        width: "100%",
                        padding: "8px 10px",
                        borderRadius: "var(--radius-sm)",
                        border: "1px solid var(--line)",
                        background: "var(--raised)",
                      }}
                    >
                      {remoteModelsList.map((m) => (
                        <option key={m} value={m}>
                          {m}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <p
                      style={{
                        margin: 0,
                        fontSize: "12px",
                        color: "var(--soft)",
                      }}
                    >
                      点击上方按钮自动获取本地已安装模型，或在下方手动指定模型
                      ID。
                    </p>
                  )}
                </div>
              )}

              <label style={{ display: "grid", gap: "6px" }}>
                <span style={{ fontSize: "13px", fontWeight: 500 }}>
                  模型 ID (Model ID)
                </span>
                <input
                  type="text"
                  value={selectedModelId}
                  placeholder={
                    providerType === "ollama"
                      ? "llama3.2 / qwen2.5:7b"
                      : "gpt-4o-mini / deepseek-chat"
                  }
                  onChange={(e) => setSelectedModelId(e.target.value)}
                  style={{
                    padding: "8px 10px",
                    borderRadius: "var(--radius-sm)",
                    border: "1px solid var(--line)",
                    background: "var(--raised)",
                  }}
                />
              </label>

              {/* 模型特性提示卡片 */}
              <div
                style={{
                  padding: "12px",
                  borderRadius: "var(--radius-md)",
                  border: "1px solid var(--line)",
                  background: "var(--raised)",
                  display: "grid",
                  gap: "6px",
                  fontSize: "13px",
                }}
              >
                <div style={{ fontWeight: 600, color: "var(--ink)" }}>
                  模型能力与运行特性：
                </div>
                <div
                  style={{
                    display: "flex",
                    flexWrap: "wrap",
                    gap: "6px",
                    marginTop: "4px",
                  }}
                >
                  <span
                    style={{
                      fontSize: "11px",
                      padding: "2px 8px",
                      borderRadius: "10px",
                      background: "var(--sunken)",
                    }}
                  >
                    流式传输 (Streaming)
                  </span>
                  <span
                    style={{
                      fontSize: "11px",
                      padding: "2px 8px",
                      borderRadius: "10px",
                      background: "var(--sunken)",
                    }}
                  >
                    参数约束校验 (Schema)
                  </span>
                  <span
                    style={{
                      fontSize: "11px",
                      padding: "2px 8px",
                      borderRadius: "10px",
                      background: "var(--sunken)",
                    }}
                  >
                    任务中断恢复保障
                  </span>
                </div>
              </div>

              {modelStepError && (
                <div
                  style={{
                    padding: "8px 12px",
                    borderRadius: "var(--radius-sm)",
                    background: "var(--danger-subtle)",
                    border: "1px solid var(--danger-border)",
                    color: "var(--danger)",
                    fontSize: "13px",
                  }}
                >
                  {modelStepError}
                </div>
              )}
            </div>
          )}

          {/* STEP 4: 完成与启程 */}
          {step === 4 && (
            <div
              style={{
                textAlign: "center",
                padding: "20px 10px",
                display: "grid",
                gap: "16px",
              }}
            >
              <div
                style={{
                  width: "56px",
                  height: "56px",
                  borderRadius: "50%",
                  background: "var(--ok-subtle)",
                  color: "var(--ok)",
                  fontSize: "28px",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  margin: "0 auto",
                  border: "1px solid var(--ok-border)",
                }}
              >
                ✓
              </div>
              <div>
                <h3
                  style={{
                    margin: "0 0 6px 0",
                    fontSize: "18px",
                    color: "var(--ink)",
                  }}
                >
                  初始化配置已就绪
                </h3>
                <p
                  style={{
                    margin: 0,
                    fontSize: "13px",
                    color: "var(--soft)",
                    lineHeight: 1.5,
                  }}
                >
                  已接入服务商 <strong>{createdProvider?.name}</strong>，
                  默认模型已设为 <strong>{createdModel?.name}</strong>。
                  <br />
                  配置参数已自动载入，点击下方按钮即可直接进入对应工作区开始使用。
                </p>
              </div>

              {/* 快捷键交互演示卡片 */}
              <div
                style={{
                  padding: "12px 14px",
                  borderRadius: "var(--radius-md)",
                  background: "var(--surface-2)",
                  border: "1px solid var(--line)",
                  textAlign: "left",
                  display: "grid",
                  gap: "6px",
                }}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                  }}
                >
                  <strong style={{ fontSize: "13px", color: "var(--ink)" }}>
                    常用快捷键：全局命令面板
                  </strong>
                  <span
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: "4px",
                    }}
                  >
                    <kbd
                      style={{
                        padding: "2px 7px",
                        borderRadius: "4px",
                        background: "var(--raised)",
                        border: "1px solid var(--line)",
                        fontSize: "12px",
                        fontFamily: "monospace",
                        color: "var(--ink)",
                        boxShadow: "var(--shadow-sm)",
                      }}
                    >
                      Ctrl
                    </kbd>
                    <span style={{ color: "var(--soft)" }}>+</span>
                    <kbd
                      style={{
                        padding: "2px 7px",
                        borderRadius: "4px",
                        background: "var(--raised)",
                        border: "1px solid var(--line)",
                        fontSize: "12px",
                        fontFamily: "monospace",
                        color: "var(--ink)",
                        boxShadow: "var(--shadow-sm)",
                      }}
                    >
                      K
                    </kbd>
                  </span>
                </div>
                <p
                  style={{
                    margin: 0,
                    fontSize: "12px",
                    color: "var(--soft)",
                    lineHeight: 1.5,
                  }}
                >
                  在任何页面均可随时按下{" "}
                  <kbd
                    style={{
                      padding: "1px 4px",
                      fontSize: "11px",
                      border: "1px solid var(--line)",
                      borderRadius: "3px",
                    }}
                  >
                    Ctrl+K
                  </kbd>{" "}
                  呼出命令面板，支持快速切换文本、图像、视频工作区、检索模型或直达系统设置。
                </p>
              </div>

              <div
                style={{
                  padding: "10px 12px",
                  borderRadius: "var(--radius-md)",
                  background: "var(--surface-1)",
                  fontSize: "12px",
                  color: "var(--soft)",
                  textAlign: "left",
                }}
              >
                <strong>设置与调整：</strong>
                <p style={{ margin: "4px 0 0 0" }}>
                  后续可随时在左侧“模型”中调整参数，或在“设置”中追加其他多模态服务商与网络代理。
                </p>
              </div>
            </div>
          )}
        </div>

        {/* 底部按钮栏 */}
        <footer
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            borderTop: "1px solid var(--line)",
            paddingTop: "14px",
          }}
        >
          {step > 1 && step < 4 ? (
            <button
              type="button"
              onClick={() => setStep((s) => (s - 1) as 1 | 2 | 3)}
              style={{
                padding: "8px 14px",
                borderRadius: "var(--radius-sm)",
                border: "1px solid var(--line)",
                background: "var(--surface-1)",
                color: "var(--ink)",
                fontSize: "13px",
              }}
            >
              上一步
            </button>
          ) : (
            <div />
          )}

          <div style={{ display: "flex", gap: "10px" }}>
            {step === 1 && (
              <button
                type="button"
                className="primary"
                onClick={() => setStep(2)}
                style={{
                  padding: "8px 18px",
                  borderRadius: "var(--radius-sm)",
                  fontSize: "13px",
                  fontWeight: 500,
                }}
              >
                下一步：配置服务商
              </button>
            )}

            {step === 2 && (
              <button
                type="button"
                className="primary"
                disabled={isTesting || saveAndTestProvider.isPending}
                onClick={() => saveAndTestProvider.mutate()}
                style={{
                  padding: "8px 18px",
                  borderRadius: "var(--radius-sm)",
                  fontSize: "13px",
                  fontWeight: 500,
                }}
              >
                {isTesting || saveAndTestProvider.isPending
                  ? "正在测试连通性..."
                  : "测试连接并继续"}
              </button>
            )}

            {step === 3 && (
              <button
                type="button"
                className="primary"
                disabled={!selectedModelId.trim() || createFirstModel.isPending}
                onClick={() => createFirstModel.mutate()}
                style={{
                  padding: "8px 18px",
                  borderRadius: "var(--radius-sm)",
                  fontSize: "13px",
                  fontWeight: 500,
                }}
              >
                {createFirstModel.isPending
                  ? "正在保存模型..."
                  : "完成模型配置"}
              </button>
            )}

            {step === 4 && (
              <button
                type="button"
                className="primary"
                onClick={handleFinish}
                style={{
                  padding: "8px 22px",
                  borderRadius: "var(--radius-sm)",
                  fontSize: "13px",
                  fontWeight: 600,
                }}
              >
                进入工作台
              </button>
            )}
          </div>
        </footer>
      </div>
    </DialogFrame>
  );
}
