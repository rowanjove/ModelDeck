import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { api, asAppError } from "../../api";
import { ErrorNote } from "../../components/ErrorNote";
import { FileList } from "../../components/FileList";
import { ParameterPanel } from "../../components/ParameterPanel";
import { PresetBar } from "../../components/PresetBar";
import { chooseFiles } from "../../lib/files";
import {
  capabilityEnabled,
  collectParameters,
  fileName,
  initialParameterValues,
  issuesFor,
  parseExtra,
  supportsTextGenerate,
} from "../../lib/schema";
import { previewContextTokens } from "../../lib/tokens";
import { useModelPick, useRestore } from "../../store";
import type {
  AppError,
  ChatMessage,
  ConversationUpdate,
  ToolLog,
} from "../../types";
import { ToolPermissionsModal } from "./ToolPermissionsModal";

const AGENT_TOOLS = [
  { id: "current_time", label: "当前时间" },
  { id: "http_request", label: "HTTP 请求" },
  { id: "read_file", label: "读取文件" },
  { id: "write_file", label: "写入文件" },
  { id: "delete_file", label: "删除文件" },
  { id: "run_command", label: "Shell 命令行" },
  { id: "list_directory", label: "列出目录" },
  { id: "memory", label: "长期记忆" },
  { id: "mcp", label: "MCP" },
];

const STRATEGIES = [
  { id: "all", label: "全部" },
  { id: "recent", label: "最近 N 条" },
  { id: "tokens", label: "Token 上限" },
  { id: "summary", label: "摘要" },
];

export function ChatView({
  fresh,
  onOpenModels,
  onShowGenerate,
  onShowCompare,
}: {
  fresh: number;
  onOpenModels: () => void;
  onShowGenerate: () => void;
  onShowCompare?: () => void;
}) {
  const queryClient = useQueryClient();
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: api.providers,
  });
  const models = useQuery({ queryKey: ["models"], queryFn: api.models });
  const conversations = useQuery({
    queryKey: ["conversations"],
    queryFn: api.conversations,
  });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const detail = useQuery({
    queryKey: ["conversation", selectedId],
    queryFn: () => {
      if (!selectedId) {
        throw new Error("没有选中的对话");
      }
      return api.conversation(selectedId);
    },
    enabled: Boolean(selectedId),
  });
  const [modelId, setModelId] = useState<string | null>(null);
  const [boundModelId, setBoundModelId] = useState<string | null>(null);
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [systemPrompt, setSystemPrompt] = useState("");
  const [strategy, setStrategy] = useState("recent");
  const [limit, setLimit] = useState(20);
  const [extraText, setExtraText] = useState("");
  const [title, setTitle] = useState("新对话");
  const [query, setQuery] = useState("");
  const [prompt, setPrompt] = useState("");
  const [images, setImages] = useState<string[]>([]);
  const [replaceId, setReplaceId] = useState<string | null>(null);
  const [streamsByConv, setStreamsByConv] = useState<
    Record<
      string,
      {
        draftText: string;
        draftReasoning: string;
        liveLogs: ToolLog[];
        liveMessages: ChatMessage[] | null;
      }
    >
  >({});
  const [streamingByConv, setStreamingByConv] = useState<
    Record<string, boolean>
  >({});
  const [agent, setAgent] = useState(false);
  const [agentTools, setAgentTools] = useState<string[]>([]);
  const [maxSteps, setMaxSteps] = useState(10);

  const currentKey = selectedId ?? "__new__";
  const currentStream = streamsByConv[currentKey];
  const draftText = currentStream?.draftText ?? "";
  const draftReasoning = currentStream?.draftReasoning ?? "";
  const liveLogs = currentStream?.liveLogs ?? [];
  const liveMessages = currentStream?.liveMessages ?? null;
  const streaming = Boolean(streamingByConv[currentKey]);

  const updateStream = (
    convKey: string,
    updater: (prev: {
      draftText: string;
      draftReasoning: string;
      liveLogs: ToolLog[];
      liveMessages: ChatMessage[] | null;
    }) => Partial<{
      draftText: string;
      draftReasoning: string;
      liveLogs: ToolLog[];
      liveMessages: ChatMessage[] | null;
    }>,
  ) => {
    setStreamsByConv((all) => {
      const existing = all[convKey] ?? {
        draftText: "",
        draftReasoning: "",
        liveLogs: [],
        liveMessages: null,
      };
      return {
        ...all,
        [convKey]: {
          ...existing,
          ...updater(existing),
        },
      };
    });
  };
  const [contextTokens, setContextTokens] = useState<number | null>(null);
  const [summaryError, setSummaryError] = useState<string | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [showMemoryPanel, setShowMemoryPanel] = useState(false);
  const [showToolPermissionsModal, setShowToolPermissionsModal] =
    useState(false);
  const [memoryFilterScope, setMemoryFilterScope] = useState<string>("all");
  const [memorySearch, setMemorySearch] = useState("");
  const [newMemKey, setNewMemKey] = useState("");
  const [newMemContent, setNewMemContent] = useState("");
  const [newMemScope, setNewMemScope] = useState<
    "global" | "session" | "conversation"
  >("global");
  const [newMemImportance, setNewMemImportance] = useState(1);

  const memoriesQuery = useQuery({
    queryKey: ["agent-memories", selectedId],
    queryFn: () => api.agentMemoriesList(selectedId || undefined),
    enabled: showMemoryPanel,
  });

  const saveMemory = useMutation({
    mutationFn: (input: {
      scope: string;
      key: string;
      content: string;
      importance: number;
      conversationId?: string | null;
    }) => api.agentMemoriesSave(input),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["agent-memories"] });
      setNewMemKey("");
      setNewMemContent("");
    },
    onError: (err) => setError(asAppError(err)),
  });

  const deleteMemory = useMutation({
    mutationFn: (id: string) => api.agentMemoriesDelete(id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["agent-memories"] });
    },
    onError: (err) => setError(asAppError(err)),
  });
  const loaded = useRef<string | null>(null);
  const activeId = useRef<string | null>(null);
  const seenPick = useRef(0);
  const pick = useModelPick((state) => state.pick);
  const seenFresh = useRef(fresh);
  const seenRestore = useRef(0);
  const pickedRestore = useRef(0);
  const restore = useRestore((state) => state.request);
  const endRef = useRef<HTMLDivElement>(null);
  const threadScrollRef = useRef<HTMLDivElement>(null);
  const composerInputRef = useRef<HTMLTextAreaElement>(null);
  const userScrolledUpRef = useRef(false);

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
    textModels.find((item) => item.id === modelId) ?? textModels[0] ?? null;
  const provider = providers.data?.find(
    (item) => item.id === model?.providerId,
  );
  const extra = parseExtra(extraText);
  const issues = model
    ? issuesFor(model.parameters, model.constraints, values)
    : [];
  const runnable = supportsTextGenerate(provider?.type);
  const vision = model
    ? capabilityEnabled(model.capabilities, "vision")
    : false;
  const shown = liveMessages ?? detail.data?.messages ?? [];
  const summary = detail.data?.conversation.summary;
  const approximate = previewContextTokens({
    messages: shown.map((message) => ({
      text: message.text,
      images: message.images,
    })),
    strategy,
    limit,
    systemPrompt,
    summary,
  });

  useEffect(() => {
    activeId.current = selectedId;
  }, [selectedId]);

  useEffect(() => {
    if (!model || boundModelId === model.id) {
      return;
    }
    setBoundModelId(model.id);
    setModelId(model.id);
    setValues(initialParameterValues(model.parameters));
    if (!capabilityEnabled(model.capabilities, "vision")) {
      setImages([]);
    }
  }, [model, boundModelId]);

  useEffect(() => {
    if (!restore || restore.kind !== "chat") {
      return;
    }
    if (seenRestore.current !== restore.nonce) {
      seenRestore.current = restore.nonce;
      pickedRestore.current = 0;
      setBoundModelId(restore.modelDefinitionId);
      setModelId(restore.modelDefinitionId);
      setSystemPrompt(restore.systemPrompt);
      setValues(restore.parameters);
      setExtraText(restore.extraText);
      setPrompt(restore.prompt);
      setImages(restore.images);
      setReplaceId(null);
    }
    if (pickedRestore.current === restore.nonce) {
      return;
    }
    if (!conversations.isFetched) {
      return;
    }
    const id = restore.conversationId;
    if (id && conversations.data?.some((item) => item.id === id)) {
      loaded.current = id;
      activeId.current = id;
      setSelectedId(id);
    } else {
      activeId.current = null;
      setSelectedId(null);
    }
    pickedRestore.current = restore.nonce;
    useRestore.getState().clear();
  }, [restore, conversations.data, conversations.isFetched]);

  useEffect(() => {
    const conversation = detail.data?.conversation;
    if (!conversation || loaded.current === conversation.id) {
      return;
    }
    loaded.current = conversation.id;
    setModelId(conversation.modelDefinitionId);
    setSystemPrompt(conversation.systemPrompt);
    setStrategy(conversation.contextStrategy);
    setLimit(conversation.contextLimit);
    setTitle(conversation.title);
  }, [detail.data]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: updateStream is a stable local helper
  useEffect(() => {
    if (streaming || !liveMessages?.length || !detail.data) {
      return;
    }
    const last = liveMessages[liveMessages.length - 1];
    if (detail.data.messages.some((message) => message.id === last.id)) {
      updateStream(currentKey, () => ({ liveMessages: null }));
    }
  }, [detail.data, liveMessages, streaming, currentKey]);

  const handleThreadScroll = () => {
    const el = threadScrollRef.current;
    if (!el) return;
    const distanceToBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    userScrolledUpRef.current = distanceToBottom > 120;
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: auto-scroll when message content updates
  useEffect(() => {
    if (!userScrolledUpRef.current) {
      endRef.current?.scrollIntoView({ block: "end" });
    }
  }, [shown.length, streaming, draftText, draftReasoning]);

  useEffect(() => {
    if (seenFresh.current === fresh) {
      return;
    }
    seenFresh.current = fresh;
    if (activeId.current) {
      void api.chatCancel(activeId.current);
    }
    activeId.current = null;
    setSelectedId(null);
    setPrompt("");
    setImages([]);
    setReplaceId(null);
    setStreamingByConv({});
    setStreamsByConv({});
    setSystemPrompt("");
    setTitle("新对话");
    setSummaryError(null);
    setContextTokens(null);
    setError(null);
    loaded.current = null;
  }, [fresh]);

  useEffect(() => {
    if (!pick || pick.modality !== "text" || pick.nonce === seenPick.current) {
      return;
    }
    seenPick.current = pick.nonce;
    setModelId(pick.modelId);
    const id = activeId.current;
    if (!id) {
      return;
    }
    void api
      .updateConversation(id, { modelDefinitionId: pick.modelId })
      .then(() =>
        queryClient.invalidateQueries({ queryKey: ["conversations"] }),
      );
  }, [pick, queryClient]);

  useEffect(() => {
    if (!conversations.isFetched || !selectedId) {
      return;
    }
    const exists = (conversations.data ?? []).some(
      (item) => item.id === selectedId,
    );
    if (!exists) {
      setSelectedId(null);
      activeId.current = null;
      setTitle("新对话");
      loaded.current = null;
      setStreamsByConv((prev) => {
        const { [selectedId]: _, ...rest } = prev;
        return rest;
      });
      setStreamingByConv((prev) => {
        const { [selectedId]: _, ...rest } = prev;
        return rest;
      });
    }
  }, [conversations.data, conversations.isFetched, selectedId]);

  async function persist(patch: ConversationUpdate) {
    if (!selectedId) {
      return;
    }
    try {
      await api.updateConversation(selectedId, patch);
      await queryClient.invalidateQueries({ queryKey: ["conversations"] });
      await queryClient.invalidateQueries({
        queryKey: ["conversation", selectedId],
      });
    } catch (reason) {
      setError(asAppError(reason));
    }
  }

  async function onSend(override?: {
    replaceFromMessageId?: string;
    prompt: string;
    images?: string[];
    confirmedTools?: string[];
  }) {
    if (!model || !extra.ok || issues.length > 0 || !runnable || streaming) {
      return;
    }
    const promptText = override ? override.prompt : prompt;
    const imagePaths = override?.images ?? images;
    const replaceFrom = override?.replaceFromMessageId ?? replaceId;
    if (!override && !promptText.trim()) {
      return;
    }
    setError(null);
    userScrolledUpRef.current = false;
    let targetConvId = activeId.current;
    const initialConvKey = targetConvId ?? "__new__";
    setStreamingByConv((prev) => ({ ...prev, [initialConvKey]: true }));
    updateStream(initialConvKey, () => ({
      draftText: "",
      draftReasoning: "",
      liveLogs: [],
    }));
    try {
      const result = await api.chatSend(
        {
          conversationId: activeId.current,
          modelDefinitionId: model.id,
          systemPrompt: systemPrompt.trim() ? systemPrompt : null,
          contextStrategy: strategy,
          contextLimit: strategy === "all" ? 0 : limit,
          prompt: promptText,
          images: imagePaths.map((path) => ({ path })),
          parameters: collectParameters(model.parameters, values),
          extra: extra.value,
          replaceFromMessageId: replaceFrom,
          tools: agent ? agentTools : [],
          confirmedTools: override?.confirmedTools ?? [],
          maxSteps: agent && agentTools.length > 0 ? maxSteps : null,
        },
        (notice) => {
          if (notice.type === "started") {
            const newId = notice.conversation.id;
            targetConvId = newId;
            setStreamingByConv((prev) => {
              const { [initialConvKey]: _, ...rest } = prev;
              return { ...rest, [newId]: true };
            });
            if (!activeId.current || activeId.current === newId) {
              activeId.current = newId;
              loaded.current = newId;
              setSelectedId(newId);
              setTitle(notice.conversation.title);
            }
            updateStream(newId, () => ({
              liveMessages: notice.messages,
              liveLogs: [],
            }));
            void queryClient.invalidateQueries({ queryKey: ["conversations"] });
            return;
          }
          const convKey = targetConvId ?? initialConvKey;
          if (notice.type === "delta") {
            updateStream(convKey, (prev) => ({
              draftText: prev.draftText + notice.text,
            }));
            return;
          }
          if (notice.type === "tool") {
            updateStream(convKey, (prev) => ({
              liveLogs: [...prev.liveLogs, notice.log],
            }));
            return;
          }
          updateStream(convKey, (prev) => ({
            draftReasoning: prev.draftReasoning + notice.text,
          }));
        },
      );
      setContextTokens(result.contextTokens);
      setSummaryError(result.summaryError ?? null);
      const finalKey = result.conversation.id;
      if (activeId.current === finalKey) {
        setPrompt("");
        setImages([]);
        setReplaceId(null);
      }
      updateStream(finalKey, (prev) => ({
        liveMessages: [...(prev.liveMessages ?? []), result.assistant],
        draftText: "",
        draftReasoning: "",
        liveLogs: [],
      }));
      setStreamingByConv((prev) => {
        const { [finalKey]: _1, [initialConvKey]: _2, ...rest } = prev;
        return rest;
      });
      await queryClient.invalidateQueries({
        queryKey: ["conversation", result.conversation.id],
      });
      await queryClient.invalidateQueries({ queryKey: ["conversations"] });
      await queryClient.invalidateQueries({ queryKey: ["history"] });
    } catch (reason) {
      setError(asAppError(reason));
      const failKey = targetConvId ?? initialConvKey;
      setStreamingByConv((prev) => {
        const { [failKey]: _1, [initialConvKey]: _2, ...rest } = prev;
        return rest;
      });
      const id = activeId.current;
      await queryClient.invalidateQueries({ queryKey: ["history"] });
      await queryClient.invalidateQueries({ queryKey: ["conversations"] });
      if (id) {
        await queryClient.invalidateQueries({ queryKey: ["conversation", id] });
      }
    }
  }

  const workspaceRef = useRef<HTMLElement>(null);
  const sendRef = useRef(onSend);
  sendRef.current = onSend;
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (
        !workspaceRef.current?.getClientRects().length ||
        event.isComposing ||
        event.altKey
      )
        return;
      if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
        const target = event.target as HTMLElement | null;
        if (target) {
          const isOtherInput =
            (target.tagName === "INPUT" ||
              target.tagName === "TEXTAREA" ||
              target.isContentEditable) &&
            target !== composerInputRef.current;
          if (isOtherInput) {
            return;
          }
        }
        event.preventDefault();
        void sendRef.current();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function chooseStrategy(next: string) {
    const nextLimit = snappedLimit(next, limit);
    setStrategy(next);
    setLimit(nextLimit);
    if (selectedId) {
      void persist({
        contextStrategy: next,
        contextLimit: next === "all" ? 0 : nextLimit,
      });
    }
  }

  async function onDeleteMessage(message: ChatMessage) {
    if (!selectedId) {
      return;
    }
    if (!window.confirm("删除这条消息以及它之后的内容？")) {
      return;
    }
    try {
      const next = await api.deleteMessage(selectedId, message.id);
      queryClient.setQueryData(["conversation", selectedId], next);
      updateStream(currentKey, () => ({ liveMessages: null }));
      if (replaceId === message.id) {
        setReplaceId(null);
      }
      await queryClient.invalidateQueries({ queryKey: ["conversations"] });
    } catch (reason) {
      setError(asAppError(reason));
    }
  }

  async function onDeleteConversation() {
    if (!selectedId) {
      return;
    }
    if (!window.confirm("删除这个对话？消息会一并删除。")) {
      return;
    }
    try {
      await api.chatCancel(selectedId);
      await api.deleteConversation(selectedId);
      const delId = selectedId;
      setStreamingByConv((prev) => {
        const { [delId]: _, ...rest } = prev;
        return rest;
      });
      setStreamsByConv((prev) => {
        const { [delId]: _, ...rest } = prev;
        return rest;
      });
      activeId.current = null;
      setSelectedId(null);
      setTitle("新对话");
      loaded.current = null;
      await queryClient.invalidateQueries({ queryKey: ["conversations"] });
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
        <ModeSwitch
          onShowGenerate={onShowGenerate}
          onShowCompare={onShowCompare}
        />
        <h1>对话</h1>
        <p>还没有可用的文本模型。配置之后就可以在这里连续对话。</p>
        <button type="button" className="primary" onClick={onOpenModels}>
          去配置模型
        </button>
      </section>
    );
  }

  const filtered = (conversations.data ?? []).filter((item) =>
    item.title.toLowerCase().includes(query.trim().toLowerCase()),
  );
  const blocked =
    streaming || !extra.ok || issues.length > 0 || !runnable || !prompt.trim();

  return (
    <section className="chat" ref={workspaceRef}>
      <aside className="chat-list">
        <ModeSwitch
          onShowGenerate={onShowGenerate}
          onShowCompare={onShowCompare}
        />
        <button
          type="button"
          className="primary"
          onClick={() => {
            if (activeId.current && streaming) {
              void api.chatCancel(activeId.current);
            }
            activeId.current = null;
            setSelectedId(null);
            setPrompt("");
            setImages([]);
            setReplaceId(null);
            setSystemPrompt("");
            setTitle("新对话");
            setSummaryError(null);
            setContextTokens(null);
            setError(null);
            loaded.current = null;
            setStreamsByConv((prev) => {
              const { __new__: _, ...rest } = prev;
              return rest;
            });
          }}
        >
          新对话
        </button>
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="筛选标题"
          aria-label="筛选标题"
        />
        {conversations.error ? (
          <ErrorNote error={asAppError(conversations.error)} />
        ) : null}
        <ul>
          {filtered.map((item) => (
            <li key={item.id}>
              <button
                type="button"
                className="chat-pick"
                aria-current={item.id === selectedId ? "true" : undefined}
                onClick={() => {
                  activeId.current = item.id;
                  setSelectedId(item.id);
                  setReplaceId(null);
                  setError(null);
                }}
              >
                <strong>{item.title}</strong>
                <span>{item.updatedAt.replace("T", " ").slice(0, 16)}</span>
              </button>
            </li>
          ))}
        </ul>
        {filtered.length === 0 ? (
          <p className="muted">没有匹配的对话。</p>
        ) : null}
      </aside>
      <div className="chat-thread">
        <header className="chat-head">
          <input
            aria-label="对话标题"
            value={title}
            disabled={!selectedId || streaming}
            onChange={(event) => setTitle(event.target.value)}
            onBlur={() => {
              const next = title.trim();
              if (
                !selectedId ||
                !next ||
                next === detail.data?.conversation.title
              ) {
                return;
              }
              void persist({ title: next });
            }}
          />
          {selectedId ? (
            <div className="inline-actions" style={{ alignItems: "center" }}>
              <button
                type="button"
                className="linkish"
                onClick={async () => {
                  try {
                    await api.conversationsCompact(selectedId);
                    await queryClient.invalidateQueries({
                      queryKey: ["conversation", selectedId],
                    });
                  } catch (e) {
                    setError(asAppError(e));
                  }
                }}
              >
                压缩上下文
              </button>
              <button
                type="button"
                className="linkish"
                onClick={() => setShowToolPermissionsModal(true)}
              >
                工具权限
              </button>
              <button
                type="button"
                className="linkish"
                onClick={() => setShowMemoryPanel(!showMemoryPanel)}
              >
                {showMemoryPanel ? "收起记忆" : "Agent 记忆库"}
              </button>
              <button
                type="button"
                className="linkish"
                onClick={() => void onDeleteConversation()}
              >
                删除对话
              </button>
            </div>
          ) : null}
        </header>

        {showMemoryPanel && (
          <div
            className="agent-memory-panel"
            style={{
              padding: "14px 18px",
              background: "var(--raised)",
              borderBottom: "1px solid var(--line)",
            }}
          >
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                marginBottom: "10px",
              }}
            >
              <div>
                <strong>Agent 长期记忆库</strong>
                <span
                  className="muted"
                  style={{ marginLeft: "8px", fontSize: "12px" }}
                >
                  记忆库内容在对话时自动注入系统提示词，支持 Agent 自主调用。
                </span>
              </div>
              <button
                type="button"
                className="linkish"
                onClick={() => setShowMemoryPanel(false)}
              >
                收起
              </button>
            </div>

            <div
              className="agent-memory-form"
              style={{
                display: "grid",
                gap: "8px",
                alignItems: "center",
                marginBottom: "12px",
              }}
            >
              <select
                value={newMemScope}
                onChange={(e) =>
                  setNewMemScope(
                    e.target.value as "global" | "session" | "conversation",
                  )
                }
              >
                <option value="global"> 全局通用</option>
                <option value="session">⏳ 会话记忆</option>
                <option value="conversation"> 仅当前对话</option>
              </select>
              <input
                placeholder="键 (如 user_pref)"
                value={newMemKey}
                onChange={(e) => setNewMemKey(e.target.value)}
              />
              <input
                placeholder="记忆内容 (content)"
                value={newMemContent}
                onChange={(e) => setNewMemContent(e.target.value)}
              />
              <input
                type="number"
                min={1}
                max={5}
                value={newMemImportance}
                onChange={(e) => setNewMemImportance(Number(e.target.value))}
                title="权重 (1-5)"
              />
              <button
                type="button"
                className="primary"
                disabled={
                  !newMemKey.trim() ||
                  !newMemContent.trim() ||
                  saveMemory.isPending
                }
                onClick={() =>
                  void saveMemory.mutateAsync({
                    scope: newMemScope,
                    key: newMemKey.trim(),
                    content: newMemContent.trim(),
                    importance: newMemImportance,
                    conversationId:
                      newMemScope === "conversation" ? selectedId : null,
                  })
                }
              >
                保存
              </button>
            </div>

            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                marginBottom: "8px",
                gap: "10px",
              }}
            >
              <div style={{ display: "flex", gap: "6px" }}>
                {[
                  { id: "all", label: "全部" },
                  { id: "global", label: " 全局" },
                  { id: "session", label: "⏳ 会话" },
                  { id: "conversation", label: " 当前" },
                ].map((tab) => (
                  <button
                    key={tab.id}
                    type="button"
                    style={{
                      fontSize: "11px",
                      padding: "2px 8px",
                      borderRadius: "var(--radius-sm)",
                      background:
                        memoryFilterScope === tab.id
                          ? "var(--accent)"
                          : "var(--sunken)",
                      color:
                        memoryFilterScope === tab.id ? "#fff" : "var(--muted)",
                      border: "none",
                      cursor: "pointer",
                    }}
                    onClick={() => setMemoryFilterScope(tab.id)}
                  >
                    {tab.label}
                  </button>
                ))}
              </div>
              <input
                placeholder=" 搜索记忆..."
                value={memorySearch}
                onChange={(e) => setMemorySearch(e.target.value)}
                style={{ fontSize: "11px", padding: "2px 8px", width: "160px" }}
              />
            </div>

            <div
              style={{
                display: "flex",
                flexWrap: "wrap",
                gap: "8px",
                maxHeight: "130px",
                overflowY: "auto",
              }}
            >
              {(() => {
                const allMemories = memoriesQuery.data ?? [];
                const filtered = allMemories.filter((m) => {
                  if (
                    memoryFilterScope !== "all" &&
                    m.scope !== memoryFilterScope
                  ) {
                    return false;
                  }
                  if (memorySearch.trim()) {
                    const q = memorySearch.toLowerCase();
                    return (
                      m.key.toLowerCase().includes(q) ||
                      m.content.toLowerCase().includes(q)
                    );
                  }
                  return true;
                });

                if (filtered.length === 0) {
                  return (
                    <p
                      className="muted"
                      style={{ margin: 0, fontSize: "13px" }}
                    >
                      没有符合条件的记忆项。
                    </p>
                  );
                }

                return filtered.map((m) => {
                  const scopeLabel =
                    m.scope === "global"
                      ? "全局"
                      : m.scope === "session"
                        ? "会话"
                        : "当前";
                  return (
                    <div
                      key={m.id}
                      style={{
                        padding: "4px 8px",
                        background: "var(--sunken)",
                        borderRadius: "4px",
                        fontSize: "12px",
                        display: "flex",
                        alignItems: "center",
                        gap: "6px",
                      }}
                    >
                      <span style={{ fontWeight: 600 }}>
                        [{scopeLabel}] {m.key}:
                      </span>
                      <span>{m.content}</span>
                      <button
                        type="button"
                        className="linkish"
                        style={{ color: "var(--danger)", padding: 0 }}
                        onClick={() => void deleteMemory.mutateAsync(m.id)}
                      >
                        ×
                      </button>
                    </div>
                  );
                });
              })()}
            </div>
          </div>
        )}

        <div
          className="thread-scroll"
          ref={threadScrollRef}
          onScroll={handleThreadScroll}
        >
          {summary ? (
            <div className="summary-note">
              <strong>此前对话摘要</strong>
              <p>{summary}</p>
            </div>
          ) : null}
          {shown.length === 0 ? (
            <p className="muted">
              输入内容开始对话。发送后会保存在这台电脑上。
            </p>
          ) : null}
          {shown.map((message) => (
            <article
              key={message.id}
              className={message.role === "user" ? "bubble user" : "bubble"}
              style={
                message.isPinned
                  ? {
                      borderColor: "var(--accent)",
                      boxShadow: "0 0 0 1px var(--accent)",
                    }
                  : undefined
              }
            >
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  borderBottom: "1px solid var(--line)",
                  paddingBottom: "5px",
                  marginBottom: "6px",
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
                      fontFamily: "var(--mono)",
                      fontSize: "10.5px",
                      fontWeight: 750,
                      letterSpacing: "0.04em",
                      textTransform: "uppercase",
                      padding: "1px 6px",
                      borderRadius: "var(--radius-xs)",
                      background:
                        message.role === "user"
                          ? "var(--surface-3)"
                          : "var(--accent-subtle)",
                      color:
                        message.role === "user"
                          ? "var(--ink)"
                          : "var(--accent)",
                      border: `1px solid ${
                        message.role === "user"
                          ? "var(--line)"
                          : "var(--accent-border)"
                      }`,
                    }}
                  >
                    {message.role === "user" ? "User" : "Assistant"}
                  </span>
                  {message.isPinned && (
                    <span
                      style={{
                        fontSize: "11px",
                        color: "var(--accent)",
                        fontWeight: 600,
                      }}
                    >
                      已固定
                    </span>
                  )}
                  {message.usage ? (
                    <span
                      style={{
                        fontSize: "11px",
                        color: "var(--muted)",
                        fontFamily: "var(--mono)",
                      }}
                    >
                      {message.usage.totalTokens != null
                        ? `${message.usage.totalTokens} tokens`
                        : ""}
                      {message.usage.reasoningTokens
                        ? ` (思考 ${message.usage.reasoningTokens})`
                        : ""}
                    </span>
                  ) : null}
                </div>
                <div
                  className="bubble-actions"
                  style={{ margin: 0, gap: "4px" }}
                >
                  <button
                    type="button"
                    title={message.isPinned ? "取消固定" : "固定此消息"}
                    style={{
                      color: message.isPinned ? "var(--accent)" : undefined,
                      fontWeight: message.isPinned ? 600 : undefined,
                    }}
                    onClick={async () => {
                      await api.messagesTogglePin(message.id);
                      await queryClient.invalidateQueries({
                        queryKey: ["conversation", selectedId],
                      });
                    }}
                  >
                    {message.isPinned ? "取消固定" : "固定"}
                  </button>
                  {message.role === "user" ? (
                    <button
                      type="button"
                      onClick={() => {
                        setReplaceId(message.id);
                        setPrompt(message.text);
                        setImages(message.images ?? []);
                      }}
                    >
                      编辑
                    </button>
                  ) : (
                    <>
                      <button
                        type="button"
                        onClick={() =>
                          void onSend({
                            replaceFromMessageId: message.id,
                            prompt: "",
                            images: [],
                          })
                        }
                      >
                        重新生成
                      </button>
                      <button
                        type="button"
                        onClick={() =>
                          void onSend({
                            prompt:
                              "请接着上面的内容继续，不要重复之前已经输出的内容。",
                            images: [],
                          })
                        }
                        title="让模型继续上一条回复"
                      >
                        继续 ⏩
                      </button>
                    </>
                  )}
                  <button
                    type="button"
                    onClick={() => void onDeleteMessage(message)}
                  >
                    删除
                  </button>
                </div>
              </div>
              {message.reasoning ? (
                <p className="reasoning">{message.reasoning}</p>
              ) : null}
              <ToolLogs
                logs={message.toolLogs ?? []}
                limited={message.maxStepsHit}
                onConfirmTool={(log) => {
                  const confirmation = encodeToolConfirmation(log);
                  if (!confirmation) {
                    return;
                  }
                  void onSend({
                    prompt: `已确认授权工具「${log.name}」按原参数执行一次，请继续。`,
                    confirmedTools: [confirmation],
                  });
                }}
                onSetToolSafe={async (toolName) => {
                  try {
                    await api.toolPermissionsSet(toolName, "safe");
                    await queryClient.invalidateQueries({
                      queryKey: ["tool-permissions"],
                    });
                    void onSend({
                      prompt: `已将工具「${toolName}」权限设为 Safe，请继续执行。`,
                    });
                  } catch (e) {
                    setError(asAppError(e));
                  }
                }}
              />
              {message.text ? <p>{message.text}</p> : null}
              {(message.images ?? []).length > 0 ? (
                <p className="muted">
                  {(message.images ?? []).map(fileName).join("、")}
                </p>
              ) : null}
              {message.error ? (
                <p className="constraint">{message.error}</p>
              ) : null}
              {message.stopped ? <p className="muted">已停止</p> : null}
            </article>
          ))}
          {streaming ? (
            <article className="bubble">
              {draftReasoning ? (
                <p className="reasoning">{draftReasoning}</p>
              ) : null}
              <ToolLogs logs={liveLogs} />
              <p>
                {draftText ||
                  (liveLogs.length > 0 ? "正在调用工具…" : "正在输出…")}
              </p>
            </article>
          ) : null}
          <div ref={endRef} />
        </div>
        <div className="chat-composer">
          {replaceId ? (
            <p className="muted">
              正在修改选中的消息，发送后会替换它并删除之后的回复。
              <button
                type="button"
                className="linkish"
                onClick={() => setReplaceId(null)}
              >
                取消
              </button>
            </p>
          ) : null}
          <textarea
            ref={composerInputRef}
            rows={4}
            value={prompt}
            placeholder="写给模型的内容"
            aria-label="消息"
            onChange={(event) => setPrompt(event.target.value)}
          />
          {vision ? (
            <FileList
              label="图像输入"
              paths={images}
              multiple
              onChoose={() => chooseFiles("image", true)}
              onChange={setImages}
            />
          ) : null}
          {error ? <ErrorNote error={error} /> : null}
          {summaryError ? (
            <p className="constraint">
              摘要失败，已改为最近消息：{summaryError}
            </p>
          ) : null}
          <button
            type="button"
            className="primary generate"
            disabled={blocked && !streaming}
            onClick={() => {
              if (streaming) {
                const target = selectedId ?? activeId.current;
                if (target) {
                  void api.chatCancel(target);
                }
                setStreamingByConv((prev) => {
                  const { [currentKey]: _, ...rest } = prev;
                  return rest;
                });
                return;
              }
              void onSend();
            }}
          >
            {streaming ? "停止" : "发送"}
            <kbd>Ctrl+Enter</kbd>
          </button>
        </div>
      </div>
      <aside className="chat-rail">
        <label className="model-pick">
          <span>模型</span>
          <select
            value={model.id}
            onChange={(event) => {
              const next = event.target.value;
              setModelId(next);
              if (selectedId) {
                void persist({ modelDefinitionId: next });
              }
            }}
          >
            {textModels.map((item) => {
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
        <label className="check-line">
          <input
            type="checkbox"
            checked={agent}
            onChange={(event) => setAgent(event.target.checked)}
          />
          Agent
        </label>
        {agent ? (
          <div className="agent-box">
            <label className="field">
              <span>最大步数</span>
              <input
                type="number"
                min={1}
                max={20}
                value={maxSteps}
                onChange={(event) => setMaxSteps(Number(event.target.value))}
                onBlur={() =>
                  setMaxSteps((current) =>
                    Math.min(20, Math.max(1, current || 10)),
                  )
                }
              />
            </label>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                marginTop: "4px",
                marginBottom: "4px",
              }}
            >
              <span style={{ fontSize: "12px", fontWeight: 600 }}>
                可调用工具
              </span>
              <button
                type="button"
                className="linkish"
                style={{ fontSize: "11px", color: "var(--accent)" }}
                onClick={() => setShowToolPermissionsModal(true)}
              >
                权限分级
              </button>
            </div>
            <div className="checks">
              {AGENT_TOOLS.map((tool) => (
                <label key={tool.id}>
                  <input
                    type="checkbox"
                    checked={agentTools.includes(tool.id)}
                    onChange={(event) =>
                      setAgentTools((current) =>
                        event.target.checked
                          ? [...current, tool.id]
                          : current.filter((item) => item !== tool.id),
                      )
                    }
                  />
                  {tool.label}
                </label>
              ))}
            </div>
            <p className="muted">
              勾选后，模型会按步调用这些工具。达到最大步数就停止。
            </p>
          </div>
        ) : null}
        <label className="field">
          <span>System Prompt</span>
          <textarea
            rows={3}
            value={systemPrompt}
            onChange={(event) => setSystemPrompt(event.target.value)}
            onBlur={() => {
              if (selectedId) {
                void persist({ systemPrompt });
              }
            }}
          />
        </label>
        <label className="field">
          <span>上下文</span>
          <select
            value={strategy}
            aria-label="上下文策略"
            onChange={(event) => chooseStrategy(event.target.value)}
          >
            {STRATEGIES.map((item) => (
              <option key={item.id} value={item.id}>
                {item.label}
              </option>
            ))}
          </select>
        </label>
        {strategy === "all" ? null : (
          <label className="field">
            <span>{strategy === "tokens" ? "Token 上限" : "保留条数"}</span>
            <input
              type="number"
              value={limit}
              min={strategy === "tokens" ? 128 : 1}
              max={strategy === "tokens" ? 200000 : 200}
              onChange={(event) => setLimit(Number(event.target.value))}
              onBlur={() => {
                const next = snappedLimit(strategy, limit);
                setLimit(next);
                if (selectedId) {
                  void persist({ contextLimit: next });
                }
              }}
            />
          </label>
        )}
        <p className="muted">约 {approximate} token</p>
        {contextTokens != null ? (
          <p className="muted">已发送上下文 {contextTokens} token</p>
        ) : null}
        <PresetBar
          modelId={model.id}
          values={values}
          systemPrompt={systemPrompt}
          userTemplate={prompt}
          onApply={setValues}
          onApplyPreset={(p) => {
            if (p.systemPrompt !== undefined && p.systemPrompt !== null) {
              setSystemPrompt(p.systemPrompt);
              if (selectedId) {
                void persist({ systemPrompt: p.systemPrompt });
              }
            }
            if (p.userTemplate !== undefined && p.userTemplate !== null) {
              setPrompt(p.userTemplate);
            }
          }}
        />
        <ParameterPanel
          parameters={model.parameters}
          constraints={model.constraints}
          values={values}
          onChange={setValues}
          chooseFiles={chooseFiles}
        />
        <details className="fold">
          <summary>Extra JSON</summary>
          <textarea
            rows={5}
            value={extraText}
            onChange={(event) => setExtraText(event.target.value)}
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
            这个 Provider 类型还不能执行文本对话。当前支持 OpenAI、OpenAI
            Compatible 与 Ollama。
          </p>
        )}
        {showToolPermissionsModal && (
          <ToolPermissionsModal
            onClose={() => setShowToolPermissionsModal(false)}
          />
        )}
      </aside>
    </section>
  );
}

function ToolLogs({
  logs,
  limited,
  onConfirmTool,
  onSetToolSafe,
}: {
  logs: ToolLog[];
  limited?: boolean;
  onConfirmTool?: (log: ToolLog) => void;
  onSetToolSafe?: (toolName: string) => void;
}) {
  if (logs.length === 0 && !limited) {
    return null;
  }
  return (
    <div className="tool-logs">
      {limited ? <p className="muted">已达到最大步数</p> : null}
      {logs.map((log) => {
        const needsConfirm =
          !log.ok &&
          (log.result.includes("人工确认") || log.result.includes("Confirm"));
        return (
          <details key={log.id}>
            <summary className={log.ok ? undefined : "constraint"}>
              {log.ok ? ` ${log.name}` : ` ${log.name} (需处理)`}
            </summary>
            {log.arguments ? (
              <p
                className="muted"
                style={{ fontFamily: "monospace", fontSize: "12px" }}
              >
                {log.arguments}
              </p>
            ) : null}
            <p style={{ whiteSpace: "pre-wrap" }}>{log.result}</p>
            {needsConfirm && (
              <div
                style={{
                  marginTop: "8px",
                  padding: "6px 10px",
                  background: "var(--raised)",
                  borderRadius: "4px",
                  display: "flex",
                  gap: "8px",
                  alignItems: "center",
                  border: "1px solid var(--warning, #eab308)",
                }}
              >
                <span
                  style={{
                    fontSize: "11px",
                    color: "var(--warning, #eab308)",
                    fontWeight: 600,
                  }}
                >
                  待人工确认 (Confirm):
                </span>
                {onConfirmTool && (
                  <button
                    type="button"
                    className="primary"
                    style={{
                      fontSize: "11px",
                      padding: "3px 8px",
                      cursor: "pointer",
                    }}
                    onClick={() => onConfirmTool(log)}
                  >
                    授权执行一次
                  </button>
                )}
                {onSetToolSafe &&
                  !["read_file", "list_directory"].includes(log.name) && (
                    <button
                      type="button"
                      style={{
                        fontSize: "11px",
                        padding: "3px 8px",
                        cursor: "pointer",
                      }}
                      onClick={() => onSetToolSafe(log.name)}
                    >
                      设为 Safe 永久允许
                    </button>
                  )}
              </div>
            )}
          </details>
        );
      })}
    </div>
  );
}

function encodeToolConfirmation(log: ToolLog): string | null {
  try {
    const argumentsValue: unknown = JSON.parse(log.arguments || "{}");
    if (
      argumentsValue === null ||
      typeof argumentsValue !== "object" ||
      Array.isArray(argumentsValue)
    ) {
      return null;
    }
    return JSON.stringify({ name: log.name, arguments: argumentsValue });
  } catch {
    return null;
  }
}

function ModeSwitch({
  onShowGenerate,
  onShowCompare,
}: {
  onShowGenerate: () => void;
  onShowCompare?: () => void;
}) {
  return (
    <div className="mode-switch">
      <button type="button" aria-pressed={false} onClick={onShowGenerate}>
        生成
      </button>
      <button type="button" aria-pressed={true}>
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

function snappedLimit(strategy: string, limit: number): number {
  if (strategy === "tokens") {
    if (limit >= 128 && limit <= 200_000) {
      return limit;
    }
    return 4096;
  }
  if (strategy === "all") {
    return 0;
  }
  if (limit >= 1 && limit <= 200) {
    return limit;
  }
  return 20;
}
