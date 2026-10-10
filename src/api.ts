import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AgentMemory,
  AgentMemoryInput,
  AnalyticsSummary,
  ApiTestPreset,
  AppError,
  AssetLibraryItem,
  AudioFile,
  AudioResult,
  BatchGenerateInput,
  BatchGenerateResult,
  ChatNotice,
  ChatSendInput,
  ChatTurnResult,
  CleanReport,
  ComfyQueueStatus,
  CompareInput,
  CompareResult,
  ConnectionReport,
  Conversation,
  ConversationDetail,
  ConversationUpdate,
  DevRequestInput,
  DevResponseOutput,
  DiagnosticBundle,
  DiscoveredService,
  ExportedConfig,
  GenerateAudioInput,
  GenerateImageInput,
  GenerateTextInput,
  GenerateVideoInput,
  HistoryDetail,
  HistoryPage,
  HistoryQuery,
  HttpRecipe,
  ImageFile,
  ImageResult,
  ImportResult,
  McpServer,
  McpServerInput,
  McpToolInfo,
  ModelDefinition,
  ModelInput,
  ModelPricing,
  PipelineDefinition,
  PipelineExecutionInput,
  PipelineExecutionResult,
  PluginValidationReport,
  Preset,
  PresetInput,
  Provider,
  ProviderInput,
  ProviderPlugin,
  ProxySettings,
  RegistryCatalog,
  RegistryUpdateCheck,
  RemoteModel,
  SdkTemplateInfo,
  StorageReport,
  SystemDiagnosticsReport,
  TaskInfo,
  TestAdapterResult,
  TextResult,
  ToolPermissionLevel,
  ToolPermissionMap,
  UnifiedSearchOutput,
  VideoFile,
  VideoResult,
  Voice,
  VoiceInput,
  VoiceUpdate,
} from "./types";

function isAppError(value: unknown): value is AppError {
  return Boolean(
    value &&
      typeof value === "object" &&
      "code" in value &&
      "message" in value &&
      typeof value.code === "string" &&
      typeof value.message === "string",
  );
}

export function asAppError(error: unknown): AppError {
  if (isAppError(error)) {
    return error;
  }
  if (typeof error === "string") {
    try {
      const parsed: unknown = JSON.parse(error);
      if (isAppError(parsed)) {
        return parsed;
      }
    } catch {
      return { code: "ProviderError", message: error };
    }
    return { code: "ProviderError", message: error };
  }
  if (error instanceof Error) {
    return { code: "ProviderError", message: error.message };
  }
  return { code: "ProviderError", message: "请求失败" };
}

export const api = {
  providers: () => invoke<Provider[]>("providers_list"),
  createProvider: (input: ProviderInput) =>
    invoke<Provider>("providers_create", { input }),
  updateProvider: (id: string, input: ProviderInput) =>
    invoke<Provider>("providers_update", { id, input }),
  deleteProvider: (id: string) => invoke<void>("providers_delete", { id }),
  testProvider: (id: string) =>
    invoke<ConnectionReport>("providers_test", { id }),
  remoteModels: (id: string, modelsPath?: string) =>
    invoke<RemoteModel[]>("providers_list_remote_models", {
      id,
      modelsPath: modelsPath ?? null,
    }),
  models: () => invoke<ModelDefinition[]>("models_list"),
  createModel: (input: ModelInput) =>
    invoke<ModelDefinition>("models_create", { input }),
  updateModel: (id: string, input: ModelInput) =>
    invoke<ModelDefinition>("models_update", { id, input }),
  deleteModel: (id: string) => invoke<void>("models_delete", { id }),
  presets: (modelId: string) => invoke<Preset[]>("presets_list", { modelId }),
  createPreset: (input: PresetInput) =>
    invoke<Preset>("presets_create", { input }),
  updatePreset: (id: string, input: PresetInput) =>
    invoke<Preset>("presets_update", { id, input }),
  deletePreset: (id: string) => invoke<void>("presets_delete", { id }),
  mcpServers: () => invoke<McpServer[]>("mcp_servers_list"),
  createMcpServer: (input: McpServerInput) =>
    invoke<McpServer>("mcp_servers_create", { input }),
  updateMcpServer: (id: string, input: McpServerInput) =>
    invoke<McpServer>("mcp_servers_update", { id, input }),
  deleteMcpServer: (id: string) => invoke<void>("mcp_servers_delete", { id }),
  mcpTools: (id: string) => invoke<McpToolInfo[]>("mcp_servers_tools", { id }),
  generateText: (input: GenerateTextInput) =>
    invoke<TextResult>("generate_text", { input }),
  generateImage: (input: GenerateImageInput, taskId?: string) =>
    invoke<ImageResult>("generate_image", { input, taskId }),
  readImage: (path: string) => invoke<ImageFile>("read_image_file", { path }),
  generateVideo: (input: GenerateVideoInput, taskId?: string) =>
    invoke<VideoResult>("generate_video", { input, taskId }),
  readVideo: (path: string) => invoke<VideoFile>("read_video_file", { path }),
  generateAudio: (input: GenerateAudioInput, taskId?: string) =>
    invoke<AudioResult>("generate_audio", { input, taskId }),
  tasksCancel: (id: string) => invoke<boolean>("tasks_cancel", { id }),
  tasksSubmitImage: (input: GenerateImageInput, taskId?: string) =>
    invoke<string>("tasks_submit_image", { input, taskId: taskId || null }),
  tasksSubmitVideo: (input: GenerateVideoInput, taskId?: string) =>
    invoke<string>("tasks_submit_video", { input, taskId: taskId || null }),
  tasksSubmitAudio: (input: GenerateAudioInput, taskId?: string) =>
    invoke<string>("tasks_submit_audio", { input, taskId: taskId || null }),
  tasksSubmitBatch: (input: BatchGenerateInput, taskId?: string) =>
    invoke<string>("tasks_submit_batch", { input, taskId: taskId || null }),
  tasksSubmitPipeline: (input: PipelineExecutionInput, taskId?: string) =>
    invoke<string>("tasks_submit_pipeline", { input, taskId: taskId || null }),
  tasksGetStatus: (taskId: string) =>
    invoke<TaskInfo | null>("tasks_get_status", { taskId }),
  tasksListActive: () => invoke<TaskInfo[]>("tasks_list_active"),
  readAudio: (path: string) => invoke<AudioFile>("read_audio_file", { path }),
  voices: () => invoke<Voice[]>("voices_list"),
  createVoice: (input: VoiceInput) => invoke<Voice>("voices_create", { input }),
  updateVoice: (id: string, input: VoiceUpdate) =>
    invoke<Voice>("voices_update", { id, input }),
  deleteVoice: (id: string) => invoke<void>("voices_delete", { id }),
  saveTextFile: (path: string, content: string) =>
    invoke<void>("save_text_file", { path, content }),
  conversations: () => invoke<Conversation[]>("conversations_list"),
  conversation: (id: string) =>
    invoke<ConversationDetail>("conversations_get", { id }),
  updateConversation: (id: string, input: ConversationUpdate) =>
    invoke<Conversation>("conversations_update", { id, input }),
  deleteConversation: (id: string) =>
    invoke<void>("conversations_delete", { id }),
  deleteMessage: (conversationId: string, messageId: string) =>
    invoke<ConversationDetail>("messages_delete", {
      conversationId,
      messageId,
    }),
  chatCancel: (conversationId: string) =>
    invoke<void>("chat_cancel", { conversationId }),
  chatSend: (input: ChatSendInput, onEvent: (notice: ChatNotice) => void) => {
    const channel = new Channel<ChatNotice>();
    channel.onmessage = onEvent;
    return invoke<ChatTurnResult>("chat_send", { input, onEvent: channel });
  },
  historyList: (query: HistoryQuery) =>
    invoke<HistoryPage>("history_list", { query }),
  historyGet: (id: string) => invoke<HistoryDetail>("history_get", { id }),
  historyDelete: (id: string) => invoke<void>("history_delete", { id }),
  historyDeleteBatch: (ids: string[]) =>
    invoke<void>("history_delete_batch", { ids }),
  historyDeleteAll: (modality?: string) =>
    invoke<void>("history_delete_all", { modality: modality || null }),

  // 1. Model Registry
  registryGet: () => invoke<RegistryCatalog>("registry_get"),
  registryCheckUpdate: (url?: string) =>
    invoke<RegistryUpdateCheck>("registry_check_update", { url: url || null }),
  registryApplyUpdate: (url?: string) =>
    invoke<RegistryCatalog>("registry_apply_update", { url: url || null }),
  registryImportModel: (
    templateId: string,
    targetProviderId: string,
    overwrite?: boolean,
  ) =>
    invoke<ModelDefinition>("registry_import_model", {
      templateId,
      providerId: targetProviderId,
      overwrite: overwrite ?? null,
    }),
  registryRollback: () => invoke<RegistryCatalog>("registry_rollback"),

  // 2. Provider Plugin
  pluginsList: () => invoke<ProviderPlugin[]>("plugins_list"),
  pluginsInstall: (manifestJson: string) =>
    invoke<ProviderPlugin>("plugins_install", { manifestJson }),
  pluginsUninstall: (id: string) => invoke<void>("plugins_uninstall", { id }),
  pluginsInstantiate: (pluginId: string, apiKey?: string) =>
    invoke<Provider>("plugins_instantiate", { pluginId, apiKey }),
  pluginsValidate: (manifestJson: string) =>
    invoke<PluginValidationReport>("plugins_validate", { manifestJson }),
  pluginsTestRecipe: (recipe: HttpRecipe, sampleResponse: string) =>
    invoke<TestAdapterResult>("plugins_test_recipe", {
      recipe,
      sampleResponse,
    }),
  pluginsGetSdkTemplate: () =>
    invoke<SdkTemplateInfo>("plugins_get_sdk_template"),

  // 3. Agent Memory
  agentMemoriesList: (conversationId?: string) =>
    invoke<AgentMemory[]>("agent_memories_list", { conversationId }),
  agentMemoriesSave: (input: AgentMemoryInput) =>
    invoke<AgentMemory>("agent_memories_save", { input }),
  agentMemoriesDelete: (id: string) =>
    invoke<void>("agent_memories_delete", { id }),

  // 4. Context Compaction & Pinning
  messagesTogglePin: (id: string) =>
    invoke<boolean>("messages_toggle_pin", { id }),
  conversationsCompact: (id: string) =>
    invoke<ConversationDetail>("conversations_compact", { id }),

  // 5. Model Cost & Analytics
  analyticsGetSummary: (timeRange?: string) =>
    invoke<AnalyticsSummary>("analytics_get_summary", {
      timeRange: typeof timeRange === "string" ? timeRange : undefined,
    }),
  analyticsSetPricing: (
    modelId: string,
    inputPerM: number,
    outputPerM: number,
  ) =>
    invoke<void>("analytics_set_pricing", { modelId, inputPerM, outputPerM }),
  analyticsListPricing: () => invoke<ModelPricing[]>("analytics_list_pricing"),

  // 6. Compare
  compareText: (input: CompareInput) =>
    invoke<CompareResult>("compare_text", { input }),

  // 7. Batch Generation
  batchGenerate: (input: BatchGenerateInput) =>
    invoke<BatchGenerateResult>("batch_generate", { input }),

  // 8. Local Service Discovery
  discoveryScanLocal: () => invoke<DiscoveredService[]>("discovery_scan_local"),
  discoveryAddService: (serviceId: string) =>
    invoke<Provider>("discovery_add_service", { serviceId }),

  // 9. Config Import / Export
  configExportAll: () => invoke<ExportedConfig>("config_export_all"),
  configImportAll: (config: ExportedConfig) =>
    invoke<ImportResult>("config_import_all", { config }),

  // 10. Network Proxy
  networkGetProxy: () => invoke<ProxySettings>("network_get_proxy"),
  networkSetProxy: (settings: ProxySettings) =>
    invoke<ProxySettings>("network_set_proxy", { settings }),
  networkTestProxy: (settings: ProxySettings) =>
    invoke<number>("network_test_proxy", { settings }),

  // 12. ComfyUI Queue
  comfyuiGetQueue: (providerId: string) =>
    invoke<ComfyQueueStatus>("comfyui_get_queue", { providerId }),
  comfyuiInterrupt: (providerId: string) =>
    invoke<boolean>("comfyui_interrupt", { providerId }),
  comfyuiClearQueue: (providerId: string) =>
    invoke<boolean>("comfyui_clear_queue", { providerId }),

  // 13. Tool Permissions (Safe, Confirm, Restricted)
  toolPermissionsList: () => invoke<ToolPermissionMap>("tool_permissions_list"),
  toolPermissionsSet: (toolName: string, level: ToolPermissionLevel) =>
    invoke<void>("tool_permissions_set", { toolName, level }),

  // 14. Automation / Workflow Pipeline (Phase 9)
  pipelinesList: () => invoke<PipelineDefinition[]>("pipelines_list"),
  pipelinesGet: (id: string) =>
    invoke<PipelineDefinition>("pipelines_get", { id }),
  pipelinesSave: (definition: PipelineDefinition) =>
    invoke<PipelineDefinition>("pipelines_save", { definition }),
  pipelinesDelete: (id: string) => invoke<void>("pipelines_delete", { id }),
  pipelinesPresets: () => invoke<PipelineDefinition[]>("pipelines_presets"),
  pipelinesValidate: (definition: PipelineDefinition) =>
    invoke<void>("pipelines_validate", { definition }),
  pipelinesExport: (definition: PipelineDefinition) =>
    invoke<string>("pipelines_export", { definition }),
  pipelinesImport: (jsonStr: string) =>
    invoke<PipelineDefinition>("pipelines_import", { jsonStr }),
  pipelinesRun: (input: PipelineExecutionInput, taskId?: string) =>
    invoke<PipelineExecutionResult>("pipelines_run", { input, taskId }),

  // 15. Developer Mode (AI API Debugger & Postman) - Phase 12
  developerSendRequest: (input: DevRequestInput) =>
    invoke<DevResponseOutput>("developer_send_request", { input }),
  developerListPresets: () => invoke<ApiTestPreset[]>("developer_list_presets"),
  developerSavePreset: (preset: ApiTestPreset) =>
    invoke<ApiTestPreset>("developer_save_preset", { preset }),
  developerDeletePreset: (id: string) =>
    invoke<void>("developer_delete_preset", { id }),

  // 16. Unified Search & Asset Library (Phase 14)
  searchUnified: (
    query: string,
    category?: string,
    modality?: string,
    limit?: number,
  ) =>
    invoke<UnifiedSearchOutput>("search_unified", {
      query,
      category: category || null,
      modality: modality || null,
      limit: limit || null,
    }),
  assetsList: (
    assetType?: string,
    query?: string,
    limit?: number,
    offset?: number,
  ) =>
    invoke<AssetLibraryItem[]>("assets_list", {
      assetType: assetType || null,
      query: query || null,
      limit: limit || null,
      offset: offset || null,
    }),

  // 17. Hardening: Storage Maintenance & Diagnostics
  storageInspect: () => invoke<StorageReport>("storage_inspect"),
  storageClean: (files: string[]) =>
    invoke<CleanReport>("storage_clean", { files }),
  dbIntegrityCheck: () => invoke<string[]>("db_integrity_check"),
  diagnosticsRun: () => invoke<SystemDiagnosticsReport>("diagnostics_run"),
  diagnosticsExportBundle: () =>
    invoke<DiagnosticBundle>("diagnostics_export_bundle"),
};
