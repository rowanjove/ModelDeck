export type ProviderType =
  | "openai"
  | "openai-compatible"
  | "ollama"
  | "comfyui"
  | "generic-http"
  | "anthropic"
  | "gemini"
  | "replicate"
  | "fal";

export type AudioTask = "tts" | "stt" | "sts" | "clone";

export type Modality = "text" | "image" | "video" | "audio";

export type ParameterType =
  | "boolean"
  | "number"
  | "range"
  | "select"
  | "text"
  | "textarea"
  | "image"
  | "images"
  | "video"
  | "videos"
  | "audio"
  | "audios";

export type MediaKind = "image" | "video" | "audio";

export interface SelectOption {
  label: string;
  value: string | number;
}

export interface ParameterDefinition {
  key: string;
  label: string;
  type: ParameterType;
  default?: unknown;
  min?: number;
  max?: number;
  step?: number;
  options?: SelectOption[];
  required?: boolean;
  advanced?: boolean;
  group?: string;
}

export type ConstraintDefinition =
  | {
      type: "mutuallyExclusive";
      fields: string[];
    }
  | {
      type: "requires";
      field: string;
      requiredFields: string[];
    }
  | {
      type: "conditionalVisibility";
      field: string;
      dependsOn: string;
      equals?: unknown;
      notEquals?: unknown;
    };

export interface WorkflowBinding {
  key: string;
  node: string;
  input: string;
}

export interface HeaderTemplate {
  name: string;
  value: string;
}

export interface HttpPoll {
  method: string;
  path: string;
  body?: Record<string, unknown> | null;
  statusPath: string;
  success: string;
  failure: string;
  intervalMs?: number | null;
}

export interface HttpRecipe {
  method: string;
  path: string;
  headers?: HeaderTemplate[];
  body?: Record<string, unknown> | null;
  taskIdPath?: string | null;
  poll?: HttpPoll | null;
  resultPath: string;
  resultKind?: string | null;
}

export interface TransportDefinition {
  chatPath?: string;
  modelsPath?: string;
  imagePath?: string;
  imageEditPath?: string;
  speechPath?: string;
  workflow?: Record<string, unknown>;
  bindings?: WorkflowBinding[];
  http?: HttpRecipe | null;
}

export interface Provider {
  id: string;
  name: string;
  type: ProviderType;
  baseUrl: string;
  hasCredential: boolean;
  headers: Record<string, string>;
  timeoutMs: number;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface ProviderInput {
  name: string;
  type: ProviderType;
  baseUrl: string;
  apiKey?: string | null;
  clearApiKey?: boolean;
  headers: Record<string, string>;
  timeoutMs: number;
  enabled: boolean;
}

export interface ModelDefinition {
  id: string;
  name: string;
  providerId: string;
  modelId: string;
  modality: Modality;
  capabilities: Record<string, unknown>;
  parameters: ParameterDefinition[];
  constraints: ConstraintDefinition[];
  transport?: TransportDefinition | null;
  schemaVersion?: number;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface ModelInput {
  name: string;
  providerId: string;
  modelId: string;
  modality: Modality;
  capabilities: Record<string, unknown>;
  parameters: ParameterDefinition[];
  constraints: ConstraintDefinition[];
  transport?: TransportDefinition | null;
  enabled: boolean;
}

export interface Usage {
  inputTokens?: number | null;
  outputTokens?: number | null;
  totalTokens?: number | null;
  reasoningTokens?: number | null;
}

export interface RawHttp {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: unknown;
}

export interface TextResult {
  text: string;
  usage?: Usage | null;
  latencyMs: number;
  providerId: string;
  providerName: string;
  modelDefinitionId: string;
  modelName: string;
  modelId: string;
  rawRequest: RawHttp;
  rawResponse: unknown;
  status: number;
}

export interface GenerateTextInput {
  modelDefinitionId: string;
  systemPrompt?: string | null;
  prompt: string;
  parameters: Record<string, unknown>;
  extra?: Record<string, unknown> | null;
  images: Array<{ path: string }>;
}

export interface Conversation {
  id: string;
  title: string;
  modelDefinitionId: string;
  systemPrompt: string;
  contextStrategy: string;
  contextLimit: number;
  summary?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ToolLog {
  id: string;
  name: string;
  arguments: string;
  result: string;
  ok: boolean;
}

export interface ChatMessage {
  id: string;
  role: string;
  text: string;
  images?: string[];
  reasoning?: string;
  usage?: Usage | null;
  stopped?: boolean;
  error?: string | null;
  toolLogs?: ToolLog[];
  maxStepsHit?: boolean;
  isPinned?: boolean;
  tokenCount: number;
  createdAt: string;
}

export interface ConversationDetail {
  conversation: Conversation;
  messages: ChatMessage[];
}

export type ChatNotice =
  | { type: "started"; conversation: Conversation; messages: ChatMessage[] }
  | { type: "delta"; text: string }
  | { type: "reasoning"; text: string }
  | { type: "tool"; log: ToolLog };

export interface ChatSendInput {
  conversationId?: string | null;
  modelDefinitionId: string;
  systemPrompt?: string | null;
  contextStrategy: string;
  contextLimit: number;
  prompt: string;
  images: Array<{ path: string }>;
  parameters: Record<string, unknown>;
  extra?: Record<string, unknown> | null;
  replaceFromMessageId?: string | null;
  tools?: string[];
  confirmedTools?: string[];
  maxSteps?: number | null;
}

export type ToolPermissionLevel = "safe" | "confirm" | "restricted";
export type ToolPermissionMap = Record<string, ToolPermissionLevel>;

export interface ChatTurnResult {
  conversation: Conversation;
  assistant: ChatMessage;
  contextTokens: number;
  summaryError?: string | null;
}

export interface ConversationUpdate {
  title?: string | null;
  modelDefinitionId?: string | null;
  systemPrompt?: string | null;
  contextStrategy?: string | null;
  contextLimit?: number | null;
}

export interface HistoryQuery {
  modality?: string | null;
  providerId?: string | null;
  modelId?: string | null;
  status?: string | null;
  dateFrom?: string | null;
  dateTo?: string | null;
  query?: string | null;
  limit?: number | null;
  offset?: number | null;
}

export interface HistoryItem {
  id: string;
  modality: Modality;
  providerId: string;
  providerName: string;
  modelId: string;
  modelName: string;
  status: string;
  promptPreview: string;
  error?: string | null;
  createdAt: string;
  completedAt?: string | null;
}

export interface Asset {
  id: string;
  taskId: string;
  type: string;
  path?: string | null;
  url?: string | null;
  mimeType?: string | null;
  width?: number | null;
  height?: number | null;
  durationMs?: number | null;
  metadata: Record<string, unknown>;
  createdAt: string;
}

export interface ReferenceImage {
  path: string;
  role?: string | null;
  weight?: number | null;
  description?: string | null;
  enabled?: boolean | null;
}

export interface GenerateImageInput {
  modelDefinitionId: string;
  prompt: string;
  negativePrompt?: string | null;
  sourceImage?: { path: string } | null;
  referenceImages?: ReferenceImage[];
  maskBase64?: string | null;
  maskPath?: string | null;
  parameters: Record<string, unknown>;
  extra?: Record<string, unknown> | null;
}

export interface GeneratedImage {
  path: string;
  mimeType: string;
  width?: number | null;
  height?: number | null;
}

export interface ImageResult {
  images: GeneratedImage[];
  latencyMs: number;
  providerId: string;
  providerName: string;
  modelDefinitionId: string;
  modelName: string;
  modelId: string;
  rawRequest: unknown;
  rawResponse: unknown;
  status: number;
}

export interface ImageFile {
  mimeType: string;
  base64: string;
}

export interface MediaRef {
  path: string;
  role?: string | null;
  weight?: number | null;
  description?: string | null;
  enabled?: boolean | null;
}

export interface GenerateVideoInput {
  modelDefinitionId: string;
  prompt: string;
  negativePrompt?: string | null;
  firstFrame?: MediaRef | null;
  lastFrame?: MediaRef | null;
  referenceImages?: ReferenceImage[];
  referenceVideos?: MediaRef[];
  referenceAudios?: MediaRef[];
  parameters: Record<string, unknown>;
  extra?: Record<string, unknown> | null;
}

export interface StoryboardShot {
  id: string;
  index: number;
  title: string;
  prompt: string;
  firstFrame?: MediaRef | null;
  lastFrame?: MediaRef | null;
  referenceImages?: ReferenceImage[];
  referenceVideos?: MediaRef[];
  referenceAudios?: MediaRef[];
  modelDefinitionId?: string | null;
  duration?: number | null;
  fps?: number | null;
  camera?: string | null;
  motion?: string | null;
  shot?: string | null;
  status: "idle" | "running" | "completed" | "failed";
  outputVideoPath?: string | null;
  error?: string | null;
}

export interface Preset {
  id: string;
  name: string;
  modelId: string;
  systemPrompt?: string | null;
  userTemplate?: string | null;
  parameters: Record<string, unknown>;
  schemaVersion?: number;
  createdAt: string;
  updatedAt: string;
}

export interface PresetInput {
  name: string;
  modelId: string;
  systemPrompt?: string | null;
  userTemplate?: string | null;
  parameters: Record<string, unknown>;
}

export interface GeneratedVideo {
  path: string;
  mimeType: string;
}

export interface VideoResult {
  videos: GeneratedVideo[];
  latencyMs: number;
  providerId: string;
  providerName: string;
  modelDefinitionId: string;
  modelName: string;
  modelId: string;
  rawRequest: unknown;
  rawResponse: unknown;
  status: number;
}

export interface VideoFile {
  mimeType: string;
  base64: string;
}

export interface GenerateAudioInput {
  modelDefinitionId: string;
  task?: AudioTask | null;
  text: string;
  voice?: string | null;
  sourceAudio?: { path: string } | null;
  referenceAudio?: { path: string } | null;
  referenceTranscript?: string | null;
  parameters: Record<string, unknown>;
  extra?: Record<string, unknown> | null;
}

export interface GeneratedAudio {
  path: string;
  mimeType: string;
}

export interface AudioResult {
  audios: GeneratedAudio[];
  latencyMs: number;
  providerId: string;
  providerName: string;
  modelDefinitionId: string;
  modelName: string;
  modelId: string;
  rawRequest: unknown;
  rawResponse: unknown;
  status: number;
  transcript?: string;
}

export interface AudioFile {
  mimeType: string;
  base64: string;
}

export interface Voice {
  id: string;
  name: string;
  filePath: string;
  transcript: string;
  tags: string;
  language?: string;
  notes?: string;
  referenceFiles?: string[];
  createdAt: string;
  updatedAt: string;
}

export interface VoiceInput {
  name: string;
  sourcePath: string;
  transcript?: string;
  tags?: string;
  language?: string;
  notes?: string;
  referencePaths?: string[];
}

export interface VoiceUpdate {
  name: string;
  transcript?: string;
  tags?: string;
  language?: string;
  notes?: string;
  sourcePath?: string | null;
  referencePaths?: string[];
}

export interface HistoryDetail {
  id: string;
  modality: Modality;
  kind: string;
  providerId: string;
  providerName: string;
  modelId: string;
  modelName: string;
  modelApiId: string;
  status: string;
  prompt: string;
  systemPrompt?: string | null;
  parameters: Record<string, unknown>;
  extra?: unknown;
  images: Array<{ path: string }>;
  negativePrompt?: string | null;
  sourceImage?: { path: string } | null;
  mask?: { path: string } | null;
  referenceImages?: ReferenceImage[];
  referenceVideos?: Array<{ path: string }>;
  referenceAudios?: Array<{ path: string }>;
  firstFrame?: { path: string } | null;
  lastFrame?: { path: string } | null;
  voice?: string | null;
  referenceTranscript?: string | null;
  referenceAudio?: { path: string } | null;
  sourceAudio?: { path: string } | null;
  audioTask?: string | null;
  conversationId?: string | null;
  rawRequest: unknown;
  rawResponse: unknown;
  output: string;
  error?: string | null;
  assets: Asset[];
  createdAt: string;
  completedAt?: string | null;
}

export interface HistoryPage {
  items: HistoryItem[];
  total: number;
  limit: number;
  offset: number;
}

export interface AppError {
  code: string;
  message: string;
  status?: number | null;
  provider?: string | null;
  model?: string | null;
  raw?: unknown;
}

export interface ConnectionReport {
  latencyMs: number;
  message: string;
}

export interface RemoteModel {
  id: string;
  name: string;
}

export const PROVIDER_LABELS: Record<ProviderType, string> = {
  openai: "OpenAI",
  "openai-compatible": "OpenAI Compatible",
  ollama: "Ollama",
  comfyui: "ComfyUI",
  "generic-http": "Generic HTTP",
  anthropic: "Anthropic",
  gemini: "Gemini",
  replicate: "Replicate",
  fal: "fal.ai",
};

export const PROVIDER_DEFAULTS: Record<ProviderType, string> = {
  openai: "https://api.openai.com/v1",
  "openai-compatible": "http://127.0.0.1:1234/v1",
  ollama: "http://127.0.0.1:11434",
  comfyui: "http://127.0.0.1:8188",
  "generic-http": "http://127.0.0.1:8000",
  anthropic: "https://api.anthropic.com",
  gemini: "https://generativelanguage.googleapis.com",
  replicate: "https://api.replicate.com",
  fal: "https://queue.fal.run",
};

export interface McpServer {
  id: string;
  name: string;
  transport: "stdio" | "http" | string;
  command: string;
  args: string[];
  url: string;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface McpServerInput {
  name: string;
  transport: string;
  command: string;
  args: string[];
  url: string;
  enabled: boolean;
}

export interface McpToolInfo {
  name: string;
  description: string;
}

export const MODALITY_LABELS: Record<Modality, string> = {
  text: "文本",
  image: "图像",
  video: "视频",
  audio: "音频",
};

export const CAPABILITY_FIELDS: Record<
  Modality,
  Array<{ key: string; label: string }>
> = {
  text: [
    { key: "streaming", label: "流式输出" },
    { key: "vision", label: "图像输入" },
    { key: "tools", label: "工具调用" },
    { key: "reasoning", label: "推理" },
    { key: "reasoningEffort", label: "推理强度" },
    { key: "structuredOutput", label: "结构化输出" },
  ],
  image: [
    { key: "generate", label: "文生图" },
    { key: "imageToImage", label: "图生图" },
    { key: "edit", label: "编辑" },
    { key: "mask", label: "蒙版" },
    { key: "referenceImages", label: "参考图" },
  ],
  video: [
    { key: "textToVideo", label: "文生视频" },
    { key: "imageToVideo", label: "图生视频" },
    { key: "firstFrame", label: "首帧" },
    { key: "lastFrame", label: "尾帧" },
    { key: "referenceImages", label: "参考图" },
    { key: "referenceVideos", label: "参考视频" },
    { key: "referenceAudio", label: "参考音频" },
  ],
  audio: [
    { key: "textToSpeech", label: "文本转语音" },
    { key: "speechToText", label: "语音转文本" },
    { key: "speechToSpeech", label: "语音转语音" },
    { key: "voiceClone", label: "声音克隆" },
  ],
};

export const PARAMETER_TYPE_LABELS: Record<ParameterType, string> = {
  boolean: "布尔",
  number: "数字",
  range: "滑块",
  select: "选项",
  text: "单行文本",
  textarea: "多行文本",
  image: "图像",
  images: "多张图像",
  video: "视频",
  videos: "多个视频",
  audio: "音频",
  audios: "多个音频",
};

// 1. Model Registry
export interface RegistryProviderTemplate {
  id: string;
  name: string;
  providerType: ProviderType;
  defaultBaseUrl: string;
  description: string;
}

export interface RegistryModelTemplate {
  id: string;
  name: string;
  providerType: ProviderType;
  modelId: string;
  modality: Modality;
  capabilities: Record<string, unknown>;
  parameters: ParameterDefinition[];
  constraints?: ConstraintDefinition[];
  transport?: TransportDefinition | null;
  recommendedDefaults?: Record<string, unknown> | null;
  description: string;
}

export interface RegistryCatalog {
  version: string;
  updatedAt: string;
  sourceUrl?: string | null;
  providers: RegistryProviderTemplate[];
  models: RegistryModelTemplate[];
  canRollback?: boolean;
  backupVersion?: string | null;
}

export interface RegistryUpdateCheck {
  hasUpdate: boolean;
  currentVersion: string;
  latestVersion: string;
  remoteVersion: string;
  remoteModelCount?: number;
  releaseNotes?: string | null;
}

// 2. Provider Plugin
export interface PluginModelTemplate {
  id: string;
  name: string;
  modelId: string;
  modality: Modality;
  capabilities: Record<string, unknown>;
}

export interface ProviderPlugin {
  id: string;
  name: string;
  version: string;
  description: string;
  protocol: string;
  defaultBaseUrl: string;
  defaultHeaders: Record<string, string>;
  recommendedModels: PluginModelTemplate[];
}

export interface PluginValidationReport {
  valid: boolean;
  pluginId?: string | null;
  pluginName?: string | null;
  warnings: string[];
  errors: string[];
  securityPassed: boolean;
}

export interface TestAdapterResult {
  success: boolean;
  extractedValue?: string | null;
  resolvedStatus?: string | null;
  error?: string | null;
}

export interface SdkTemplateInfo {
  schemaVersion: string;
  manifestTemplate: string;
  genericHttpTemplate: string;
  documentation: string;
  supportedProtocols: string[];
}

// 3. Agent Memory
export interface AgentMemory {
  id: string;
  conversationId?: string | null;
  scope: string;
  key: string;
  content: string;
  importance: number;
  createdAt: string;
  updatedAt: string;
}

export interface AgentMemoryInput {
  id?: string | null;
  conversationId?: string | null;
  scope?: string | null;
  key: string;
  content: string;
  importance?: number | null;
}

// 5. Model Pricing & Analytics
export interface ModelPricing {
  modelId: string;
  providerId?: string;
  inputPerM: number;
  outputPerM: number;
  pricingUnit?: string;
  pricePerUnit?: number;
  source?: string;
  currency: string;
  updatedAt?: string;
}

export interface CostEstimationResult {
  hasPricing: boolean;
  currency: string;
  estimatedCostMin: number;
  estimatedCostMax: number;
  pricingUnit: string;
  source: string;
  updatedAt?: string | null;
  isOutdated: boolean;
  note: string;
}

export interface ModelUsageStat {
  modelId: string;
  modelName: string;
  taskCount: number;
  totalTokens: number;
  costUsd: number;
  officialCostUsd?: number;
  estimatedCostUsd?: number;
  meteringSource?: string; // "official" | "estimated" | "unmetered" | "mixed"
  avgLatencyMs: number;
}

export interface ProviderUsageStat {
  providerId: string;
  providerName: string;
  taskCount: number;
  totalTokens: number;
  costUsd: number;
  officialCostUsd?: number;
  estimatedCostUsd?: number;
}

export interface DailyStat {
  date: string;
  taskCount: number;
  totalTokens: number;
  costUsd: number;
  officialCostUsd?: number;
  estimatedCostUsd?: number;
}

export interface AnalyticsSummary {
  totalTasks: number;
  completedTasks: number;
  failedTasks: number;
  totalPromptTokens: number;
  totalCompletionTokens: number;
  totalTokens: number;
  estimatedCostUsd: number;
  officialCostUsd?: number;
  estimatedPortionCostUsd?: number;
  totalCostUsd?: number;
  officialTaskCount?: number;
  estimatedTaskCount?: number;
  unmeteredTaskCount?: number;
  avgLatencyMs: number;
  modalityCounts: Record<string, number>;
  topModels: ModelUsageStat[];
  topProviders: ProviderUsageStat[];
  recentDays: DailyStat[];
}

// 6. Compare
export interface CompareInput {
  prompt: string;
  systemPrompt?: string | null;
  modelIds: string[];
  parameters: Record<string, unknown>;
}

export interface CompareItemResult {
  modelId: string;
  modelName: string;
  providerName: string;
  text: string;
  latencyMs: number;
  tokens: number;
  costUsd: number;
  reasoning?: string | null;
  reasoningTokens?: number | null;
  error?: string | null;
}

export interface CompareResult {
  prompt: string;
  items: CompareItemResult[];
}

// 7. Batch Generation
export interface BatchGenerateInput {
  modality: string;
  modelId: string;
  prompts: string[];
  systemPrompt?: string | null;
  seeds?: number[] | null;
  parameters: Record<string, unknown>;
}

export interface BatchItemResult {
  index: number;
  prompt: string;
  seed?: number | null;
  output?: string | null;
  latencyMs: number;
  error?: string | null;
}

export interface BatchGenerateResult {
  total: number;
  succeeded: number;
  failed: number;
  items: BatchItemResult[];
}

// 8. Local Service Discovery
export interface DiscoveredService {
  id: string;
  name: string;
  serviceType: string;
  baseUrl: string;
  port: number;
  online: boolean;
  latencyMs?: number | null;
  alreadyAdded: boolean;
  details?: string | null;
  version?: string | null;
  detectedModels?: string[];
  webUiUrl?: string | null;
}

// 9. Config Import / Export
export interface ExportedProvider {
  id: string;
  name: string;
  providerType: ProviderType;
  baseUrl: string;
  headers: Record<string, string>;
  timeoutMs: number;
  enabled: boolean;
}

export interface ExportedConfig {
  version: string;
  exportedAt: string;
  providers: ExportedProvider[];
  models: ModelDefinition[];
  presets: Preset[];
}

export interface ImportResult {
  providersImported: number;
  modelsImported: number;
  presetsImported: number;
}

// 10. Network Proxy
export interface ProxySettings {
  mode: "system" | "none" | "http" | "socks5";
  httpProxy?: string | null;
  socks5Proxy?: string | null;
  updatedAt: string;
}

// 11. ComfyUI Queue
export interface ComfyQueueItem {
  promptId: string;
  number: number;
}

export interface ComfyQueueStatus {
  running: ComfyQueueItem[];
  pending: ComfyQueueItem[];
  total: number;
}

// 12. Automation / Workflow Pipeline (Phase 9)
export type PipelineStep =
  | {
      type: "input";
      id: string;
      label: string;
      defaultValue?: string;
      dataType?: "text" | "image" | "video" | "audio" | string;
    }
  | {
      type: "model";
      id: string;
      label: string;
      modelDefinitionId: string;
      promptTemplate: string;
      systemPrompt?: string | null;
      parameters?: Record<string, unknown>;
      expectedInputKind?: "text" | "image" | "video" | "audio" | string;
      mediaInputVariable?: string | null;
    }
  | {
      type: "transform";
      id: string;
      label: string;
      kind: "template" | "regex" | "json_field" | "trim" | string;
      rule: string;
    };

export interface PipelineDefinition {
  id: string;
  name: string;
  description: string;
  schemaVersion?: number;
  steps: PipelineStep[];
  createdAt: string;
  updatedAt: string;
}

export interface PipelineExecutionInput {
  pipelineId?: string | null;
  steps: PipelineStep[];
  initialInput: string;
  resumeFromStepIndex?: number | null;
  cachedStepResults?: PipelineStepResult[] | null;
}

export interface PipelineStepResult {
  stepId: string;
  stepLabel: string;
  stepType: string;
  outputKind: string; // "text" | "image" | "video" | "audio"
  textOutput?: string | null;
  mediaPaths: string[];
  latencyMs: number;
  status: "completed" | "failed" | string;
  error?: string | null;
}

export interface PipelineExecutionResult {
  totalLatencyMs: number;
  stepResults: PipelineStepResult[];
  finalOutputKind: string;
  finalText?: string | null;
  finalMediaPaths: string[];
}

// 13. Developer Mode (AI API Debugger & Postman) - Phase 12
export interface ApiTestPreset {
  id: string;
  name: string;
  method: string;
  path: string;
  providerId?: string | null;
  headers: Record<string, string>;
  body: string;
  createdAt: string;
  updatedAt: string;
}

export interface DevRequestInput {
  method: string;
  url: string;
  providerId?: string | null;
  headers: Record<string, string>;
  body?: string | null;
  timeoutMs?: number | null;
}

export interface SseEventItem {
  index: number;
  eventType: string;
  data: string;
  offsetMs: number;
}

export interface DevResponseOutput {
  status: number;
  statusText: string;
  headers: Record<string, string>;
  body: string;
  latencyMs: number;
  sizeBytes: number;
  curlCommand: string;
  isSse: boolean;
  sseEvents: SseEventItem[];
}

// 14. Unified Search & Asset Library - Phase 14
export interface AssetLibraryItem {
  id: string;
  taskId: string;
  assetType: "image" | "video" | "audio" | string;
  path?: string | null;
  url?: string | null;
  mimeType?: string | null;
  width?: number | null;
  height?: number | null;
  durationMs?: number | null;
  prompt: string;
  modelId: string;
  modelName: string;
  providerName: string;
  createdAt: string;
  fileExists: boolean;
  fileSizeBytes?: number | null;
  parameters?: Record<string, unknown> | null;
  metadata?: Record<string, unknown> | null;
}

export interface UnifiedSearchItem {
  id: string;
  category: "conversation" | "asset" | "task" | "model" | "pipeline" | string;
  title: string;
  snippet: string;
  modality?: string | null;
  path?: string | null;
  metadata?: Record<string, unknown> | null;
  createdAt: string;
}

export interface UnifiedSearchOutput {
  items: UnifiedSearchItem[];
  total: number;
}

// 15. Hardening: Storage Maintenance & System Diagnostics
export interface StorageReport {
  totalAssetRecords: number;
  diskFilesCount: number;
  diskTotalBytes: number;
  orphanFiles: string[];
  missingFiles: string[];
  tempFiles: string[];
}

export interface CleanReport {
  cleanedFilesCount: number;
  freedBytes: number;
}

export interface DiagnosticItem {
  category: string;
  name: string;
  status: "ok" | "warn" | "error";
  detail: string;
}

export interface SystemDiagnosticsReport {
  generatedAt: string;
  appVersion: string;
  os: string;
  arch: string;
  items: DiagnosticItem[];
}

export interface DiagnosticBundle {
  report: SystemDiagnosticsReport;
  sanitizedProviders: Record<string, unknown>[];
  storage: StorageReport;
  recentTaskErrors: Record<string, unknown>[];
}

export interface TaskEvent {
  taskId: string;
  eventType:
    | "queued"
    | "submitting"
    | "started"
    | "progress"
    | "intermediate"
    | "downloading"
    | "completed"
    | "failed"
    | "cancelled"
    | "interrupted"
    | "unknown_submission";
  modality: string;
  status: string;
  progress?: number | null;
  stage?: string | null;
  message?: string | null;
  outputAsset?: unknown;
  error?: string | null;
  errorCode?: string | null;
  retryable?: boolean | null;
  remoteJobId?: string | null;
  billingRisk?: string | null;
  timestamp: string;
}

export interface TaskInfo {
  id: string;
  modality: string;
  status: string;
  progress?: number | null;
  stage?: string | null;
  message?: string | null;
  error?: string | null;
  errorCode?: string | null;
  retryable?: boolean | null;
  remoteJobId?: string | null;
  billingRisk?: string | null;
  output?: unknown;
  createdAt: string;
  updatedAt: string;
}
