import { useInfiniteQuery } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../../api";
import { AudioThumb } from "../../components/AudioThumb";
import { DialogFrame } from "../../components/DialogFrame";
import { ImageThumb } from "../../components/ImageThumb";
import { VideoThumb } from "../../components/VideoThumb";
import {
  useAudioDraft,
  useImageDraft,
  useTextDraft,
  useVideoDraft,
} from "../../store";
import type { AssetLibraryItem } from "../../types";

export function AssetLibraryView({
  onOpenText,
  onOpenImage,
  onOpenVideo,
  onOpenAudio,
}: {
  onOpenText?: () => void;
  onOpenImage: () => void;
  onOpenVideo: () => void;
  onOpenAudio: () => void;
}) {
  const [assetType, setAssetType] = useState<string>("all");
  const [query, setQuery] = useState("");
  const [selectedAsset, setSelectedAsset] = useState<AssetLibraryItem | null>(
    null,
  );
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const pageSize = 60;
  const assetsQuery = useInfiniteQuery({
    queryKey: ["assets_list", assetType, query],
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      api.assetsList(
        assetType === "all" ? undefined : assetType,
        query.trim() || undefined,
        pageSize,
        pageParam,
      ),
    getNextPageParam: (lastPage, allPages) =>
      lastPage.length === pageSize ? allPages.length * pageSize : undefined,
  });

  const assets = assetsQuery.data?.pages.flat() ?? [];

  const copyText = (text: string, id: string) => {
    navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => {
      setCopiedId(null);
    }, 2000);
  };

  // 1. 复用至图生图原图
  const handleUseAsImageSource = (asset: AssetLibraryItem) => {
    if (!asset.path) return;
    useImageDraft.getState().patch({ sourcePath: asset.path });
    onOpenImage();
  };

  // 2. 复用至视频首帧 (T12 核心验收链路)
  const handleUseAsVideoFirstFrame = (asset: AssetLibraryItem) => {
    if (!asset.path) return;
    useVideoDraft.getState().patch({ firstPath: asset.path });
    onOpenVideo();
  };

  // 3. 复用至视频尾帧
  const handleUseAsVideoLastFrame = (asset: AssetLibraryItem) => {
    if (!asset.path) return;
    useVideoDraft.getState().patch({ lastPath: asset.path });
    onOpenVideo();
  };

  // 4. 复用至语音合成 / 声音克隆
  const handleUseAsAudioReference = (asset: AssetLibraryItem) => {
    if (!asset.path) return;
    useAudioDraft.getState().patch({
      referencePath: asset.path,
      sourcePath: asset.path,
      task: "clone",
    });
    onOpenAudio();
  };

  // 5. 提示词复用至文本对话
  const handleReusePromptInChat = (prompt: string) => {
    if (!prompt.trim()) return;
    useTextDraft.getState().patch({ prompt });
    if (onOpenText) onOpenText();
  };

  // 6. 提示词复用至语音合成 TTS
  const handleReusePromptInTts = (prompt: string) => {
    if (!prompt.trim()) return;
    useAudioDraft.getState().patch({ prompt, task: "tts" });
    onOpenAudio();
  };

  // 7. 完整还原参数并重新生成
  const handleReuseFullParameters = (asset: AssetLibraryItem) => {
    const rawParams = (asset.parameters as Record<string, unknown>) || {};
    if (asset.assetType === "image") {
      useImageDraft.getState().patch({
        prompt: asset.prompt,
        values: rawParams,
      });
      onOpenImage();
    } else if (asset.assetType === "video") {
      useVideoDraft.getState().patch({
        prompt: asset.prompt,
        values: rawParams,
      });
      onOpenVideo();
    } else if (asset.assetType === "audio") {
      useAudioDraft.getState().patch({
        prompt: asset.prompt,
        values: rawParams,
      });
      onOpenAudio();
    } else {
      useTextDraft.getState().patch({
        prompt: asset.prompt,
        values: rawParams,
      });
      if (onOpenText) onOpenText();
    }
  };

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        padding: "20px 24px",
        overflowY: "auto",
        background: "var(--bg)",
      }}
    >
      {/* 顶部标题与搜索栏 */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: "16px",
          gap: "12px",
        }}
      >
        <div>
          <h2
            style={{
              margin: "0 0 4px",
              fontSize: "16px",
              fontWeight: 700,
              color: "var(--ink)",
            }}
          >
            媒体资产库 (Asset Library)
          </h2>
          <p
            style={{
              margin: 0,
              fontSize: "12.5px",
              color: "var(--soft)",
            }}
          >
            跨模态统一管理 AI
            生成资产，支持生成来源追溯、跨工作台素材复用（如图片转视频首帧）与参数还原。
          </p>
        </div>

        {/* 搜索与刷新 */}
        <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
          <input
            type="text"
            placeholder="搜索 Prompt、模型或路径..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            style={{
              padding: "6px 12px",
              background: "var(--raised)",
              color: "var(--ink)",
              border: "1px solid var(--line)",
              borderRadius: "var(--radius-sm)",
              fontSize: "12px",
              width: "220px",
              outline: "none",
            }}
          />
          <button
            type="button"
            onClick={() => void assetsQuery.refetch()}
            disabled={assetsQuery.isFetching}
            style={{
              padding: "6px 12px",
              background: "var(--surface-2)",
              color: "var(--ink)",
              border: "1px solid var(--line)",
              borderRadius: "var(--radius-sm)",
              cursor: "pointer",
              fontSize: "12px",
            }}
          >
            {assetsQuery.isFetching ? "刷新中…" : "刷新"}
          </button>
        </div>
      </div>

      {/* 模态过滤选项卡 */}
      <div
        style={{
          display: "flex",
          gap: "6px",
          marginBottom: "18px",
          borderBottom: "1px solid var(--line)",
          paddingBottom: "8px",
        }}
      >
        {[
          { id: "all", label: `全部素材 (${assets.length})` },
          { id: "image", label: "🖼️ 图像 (Image)" },
          { id: "video", label: "🎬 视频 (Video)" },
          { id: "audio", label: "🎙️ 音频 (Audio)" },
        ].map((tab) => (
          <button
            key={tab.id}
            type="button"
            onClick={() => setAssetType(tab.id)}
            style={{
              padding: "5px 12px",
              borderRadius: "var(--radius-sm)",
              border: "none",
              background:
                assetType === tab.id ? "var(--accent)" : "transparent",
              color: assetType === tab.id ? "var(--accent-ink)" : "var(--soft)",
              cursor: "pointer",
              fontWeight: 600,
              fontSize: "12px",
            }}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* 素材网格 */}
      {assets.length === 0 ? (
        <div
          style={{
            flex: 1,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--muted)",
            padding: "48px 0",
          }}
        >
          <div style={{ fontSize: "14px" }}>暂无符合条件的媒体素材</div>
          <div style={{ fontSize: "12px", marginTop: "4px" }}>
            在图像、视频或音频工作台生成后将自动持久化至此
          </div>
        </div>
      ) : (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(260px, 1fr))",
            gap: "16px",
          }}
        >
          {assets.map((asset) => {
            const aType = asset.assetType || "image";
            const fileExists = asset.fileExists !== false;

            return (
              <div
                key={asset.id}
                style={{
                  background: "var(--raised)",
                  border: `1px solid ${
                    fileExists ? "var(--line)" : "var(--danger-border)"
                  }`,
                  borderRadius: "var(--radius-lg)",
                  overflow: "hidden",
                  display: "flex",
                  flexDirection: "column",
                  boxShadow: "var(--shadow-sm)",
                  transition: "transform 0.15s, border-color 0.15s",
                }}
              >
                {/* 媒体预览区 */}
                <div
                  style={{
                    height: "170px",
                    background: "var(--sunken)",
                    position: "relative",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    overflow: "hidden",
                    cursor: "pointer",
                  }}
                  onClick={() => setSelectedAsset(asset)}
                >
                  {fileExists ? (
                    <>
                      {asset.path && aType === "image" && (
                        <ImageThumb path={asset.path} alt={asset.prompt} />
                      )}
                      {asset.path && aType === "video" && (
                        <VideoThumb path={asset.path} />
                      )}
                      {asset.path && aType === "audio" && (
                        <div style={{ width: "90%" }}>
                          <AudioThumb path={asset.path} />
                        </div>
                      )}
                    </>
                  ) : (
                    <div
                      style={{
                        textAlign: "center",
                        padding: "16px",
                        color: "var(--danger)",
                        fontSize: "12px",
                      }}
                    >
                      <div style={{ fontSize: "20px", marginBottom: "4px" }}>
                        ⚠️
                      </div>
                      <strong>本地文件已丢失或移动</strong>
                      <div
                        style={{
                          fontSize: "11px",
                          color: "var(--muted)",
                          marginTop: "2px",
                        }}
                      >
                        点击查看生成参数与追溯信息
                      </div>
                    </div>
                  )}

                  {/* 模态徽章 */}
                  <div
                    style={{
                      position: "absolute",
                      top: "8px",
                      left: "8px",
                      padding: "2px 6px",
                      borderRadius: "4px",
                      background: "rgba(0,0,0,0.65)",
                      color: "#fff",
                      fontSize: "10px",
                      fontWeight: 700,
                      textTransform: "uppercase",
                    }}
                  >
                    {aType}
                  </div>

                  {/* 尺寸/时长徽章 */}
                  {(asset.width && asset.height) || asset.durationMs ? (
                    <div
                      style={{
                        position: "absolute",
                        bottom: "8px",
                        right: "8px",
                        padding: "2px 6px",
                        borderRadius: "4px",
                        background: "rgba(0,0,0,0.65)",
                        color: "#93c5fd",
                        fontSize: "10px",
                        fontFamily: "monospace",
                      }}
                    >
                      {asset.width && asset.height
                        ? `${asset.width}×${asset.height}`
                        : `${((asset.durationMs || 0) / 1000).toFixed(1)}s`}
                    </div>
                  ) : null}
                </div>

                {/* 元数据与操作栏 */}
                <div
                  style={{
                    padding: "12px",
                    display: "flex",
                    flexDirection: "column",
                    gap: "6px",
                    flex: 1,
                  }}
                >
                  <div
                    style={{
                      fontSize: "12px",
                      fontWeight: 600,
                      color: "var(--ink)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      display: "-webkit-box",
                      WebkitLineClamp: 2,
                      WebkitBoxOrient: "vertical",
                      lineHeight: "1.4",
                      minHeight: "33px",
                    }}
                    title={asset.prompt}
                  >
                    {asset.prompt || "（无 Prompt）"}
                  </div>

                  <div
                    style={{
                      fontSize: "11px",
                      color: "var(--muted)",
                      display: "flex",
                      justifyContent: "space-between",
                    }}
                  >
                    <span>{asset.modelName}</span>
                    <span>
                      {new Date(asset.createdAt).toLocaleDateString()}
                    </span>
                  </div>

                  {/* 底部按钮栏 */}
                  <div
                    style={{
                      display: "flex",
                      gap: "6px",
                      marginTop: "6px",
                      paddingTop: "6px",
                      borderTop: "1px solid var(--line)",
                    }}
                  >
                    <button
                      type="button"
                      onClick={() => setSelectedAsset(asset)}
                      style={{
                        flex: 1,
                        padding: "5px 8px",
                        fontSize: "11px",
                        background: "var(--accent)",
                        color: "var(--accent-ink)",
                        border: "none",
                        borderRadius: "var(--radius-sm)",
                        cursor: "pointer",
                        fontWeight: 600,
                      }}
                    >
                      溯源与复用
                    </button>
                    {asset.prompt && (
                      <button
                        type="button"
                        onClick={() => copyText(asset.prompt, asset.id)}
                        style={{
                          padding: "5px 8px",
                          fontSize: "11px",
                          background: "var(--surface-2)",
                          color:
                            copiedId === asset.id ? "var(--ok)" : "var(--ink)",
                          border: "1px solid var(--line)",
                          borderRadius: "var(--radius-sm)",
                          cursor: "pointer",
                        }}
                        title="复制提示词"
                      >
                        {copiedId === asset.id ? "已复制" : "复制词"}
                      </button>
                    )}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {assetsQuery.hasNextPage && (
        <div style={{ textAlign: "center", margin: "20px 0" }}>
          <button
            type="button"
            onClick={() => void assetsQuery.fetchNextPage()}
            disabled={assetsQuery.isFetchingNextPage}
            style={{
              padding: "8px 20px",
              background: "var(--surface-2)",
              color: "var(--ink)",
              border: "1px solid var(--line)",
              borderRadius: "var(--radius-md)",
              cursor: "pointer",
              fontSize: "13px",
            }}
          >
            {assetsQuery.isFetchingNextPage ? "加载中…" : "⬇ 加载更多素材"}
          </button>
        </div>
      )}

      {/* 资产详情、追溯与跨模态复用弹窗 */}
      {selectedAsset && (
        <DialogFrame
          label="素材来源追溯与跨模态复用"
          onClose={() => setSelectedAsset(null)}
        >
          <div
            style={{
              width: "720px",
              maxWidth: "92vw",
              maxHeight: "88vh",
              background: "var(--surface-1)",
              border: "1px solid var(--line)",
              borderRadius: "var(--radius-lg)",
              padding: "20px 24px",
              display: "flex",
              flexDirection: "column",
              gap: "14px",
              overflowY: "auto",
            }}
            onClick={(e) => e.stopPropagation()}
          >
            {/* 弹窗头部 */}
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                borderBottom: "1px solid var(--line)",
                paddingBottom: "10px",
              }}
            >
              <div>
                <h3
                  style={{
                    margin: 0,
                    fontWeight: 700,
                    fontSize: "16px",
                    color: "var(--ink)",
                  }}
                >
                  素材详情与来源追溯 · {selectedAsset.assetType?.toUpperCase()}
                </h3>
                <span
                  style={{
                    fontSize: "11px",
                    color: "var(--muted)",
                    fontFamily: "monospace",
                  }}
                >
                  任务 ID: {selectedAsset.taskId}
                </span>
              </div>
              <button
                type="button"
                onClick={() => setSelectedAsset(null)}
                style={{
                  background: "none",
                  border: "none",
                  color: "var(--muted)",
                  cursor: "pointer",
                  fontSize: "13px",
                }}
              >
                ✕ 关闭
              </button>
            </div>

            {/* 完整媒体预览区 */}
            <div
              style={{
                width: "100%",
                maxHeight: "360px",
                background: "var(--sunken)",
                borderRadius: "var(--radius-md)",
                overflow: "hidden",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                border: "1px solid var(--line)",
              }}
            >
              {selectedAsset.fileExists !== false ? (
                <>
                  {selectedAsset.path &&
                    selectedAsset.assetType === "image" && (
                      <ImageThumb
                        path={selectedAsset.path}
                        alt={selectedAsset.prompt}
                      />
                    )}
                  {selectedAsset.path &&
                    selectedAsset.assetType === "video" && (
                      <VideoThumb path={selectedAsset.path} />
                    )}
                  {selectedAsset.path &&
                    selectedAsset.assetType === "audio" && (
                      <div style={{ width: "90%", padding: "20px 0" }}>
                        <AudioThumb path={selectedAsset.path} />
                      </div>
                    )}
                </>
              ) : (
                <div
                  style={{
                    padding: "30px",
                    textAlign: "center",
                    color: "var(--danger)",
                  }}
                >
                  <div style={{ fontSize: "28px", marginBottom: "6px" }}>⚠️</div>
                  <strong>本地文件已从磁盘中移除或重命名</strong>
                  <div
                    style={{
                      fontSize: "12px",
                      color: "var(--soft)",
                      marginTop: "4px",
                    }}
                  >
                    物理文件虽然不可用，但原始生成参数和 Prompt
                    完整保留，支持一键重新生成。
                  </div>
                </div>
              )}
            </div>

            {/* Prompt 与复制 */}
            <div>
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                  fontSize: "12px",
                  fontWeight: 600,
                  color: "var(--ink)",
                  marginBottom: "4px",
                }}
              >
                <span>生成提示词 (Prompt)</span>
                {selectedAsset.prompt && (
                  <button
                    type="button"
                    onClick={() =>
                      copyText(selectedAsset.prompt, selectedAsset.id)
                    }
                    style={{
                      padding: "2px 8px",
                      fontSize: "11px",
                      borderRadius: "var(--radius-xs)",
                      border: "1px solid var(--line)",
                      background: "var(--surface-2)",
                      color: "var(--ink)",
                      cursor: "pointer",
                    }}
                  >
                    {copiedId === selectedAsset.id
                      ? "✓ 已复制提示词"
                      : "📋 复制提示词"}
                  </button>
                )}
              </div>
              <div
                style={{
                  padding: "8px 12px",
                  background: "var(--sunken-subtle)",
                  borderRadius: "var(--radius-sm)",
                  fontSize: "12.5px",
                  lineHeight: "1.5",
                  maxHeight: "80px",
                  overflowY: "auto",
                  color: "var(--ink)",
                  border: "1px solid var(--line)",
                }}
              >
                {selectedAsset.prompt || "（无 Prompt）"}
              </div>
            </div>

            {/* 来源元数据表格 */}
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "1fr 1fr",
                gap: "8px",
                fontSize: "12px",
                background: "var(--surface-2)",
                padding: "10px 14px",
                borderRadius: "var(--radius-sm)",
                border: "1px solid var(--line)",
              }}
            >
              <div>
                服务商: <strong>{selectedAsset.providerName}</strong>
              </div>
              <div>
                模型: <strong>{selectedAsset.modelName}</strong> (
                <code>{selectedAsset.modelId}</code>)
              </div>
              <div>
                物理规格:{" "}
                <strong>
                  {selectedAsset.width && selectedAsset.height
                    ? `${selectedAsset.width} × ${selectedAsset.height}`
                    : selectedAsset.durationMs
                      ? `${((selectedAsset.durationMs || 0) / 1000).toFixed(1)} 秒`
                      : selectedAsset.mimeType || "-"}
                </strong>
              </div>
              <div>
                物理文件状态:{" "}
                <strong
                  style={{
                    color:
                      selectedAsset.fileExists !== false
                        ? "var(--ok)"
                        : "var(--danger)",
                  }}
                >
                  {selectedAsset.fileExists !== false
                    ? `正常存在 (${
                        selectedAsset.fileSizeBytes
                          ? `${(
                              selectedAsset.fileSizeBytes / (1024 * 1024)
                            ).toFixed(2)} MB`
                          : "有效"
                      })`
                    : "已缺失 / 已清理"}
                </strong>
              </div>
              <div style={{ gridColumn: "span 2" }}>
                生成时间:{" "}
                <strong>
                  {new Date(selectedAsset.createdAt).toLocaleString()}
                </strong>
              </div>
              {selectedAsset.path && (
                <div
                  style={{
                    gridColumn: "span 2",
                    color: "var(--muted)",
                    fontSize: "11px",
                    fontFamily: "monospace",
                    wordBreak: "break-all",
                  }}
                >
                  存储路径: {selectedAsset.path}
                </div>
              )}
            </div>

            {/* 生成参数追溯 (Provenance Parameters) */}
            {selectedAsset.parameters &&
              Object.keys(selectedAsset.parameters).length > 0 && (
                <div>
                  <div
                    style={{
                      fontSize: "12px",
                      fontWeight: 600,
                      color: "var(--ink)",
                      marginBottom: "4px",
                    }}
                  >
                    生成采样参数 (Generation Parameters)
                  </div>
                  <div
                    style={{
                      padding: "8px 12px",
                      borderRadius: "var(--radius-sm)",
                      background: "var(--sunken-subtle)",
                      border: "1px solid var(--line)",
                      fontSize: "11px",
                      fontFamily: "monospace",
                      display: "flex",
                      flexWrap: "wrap",
                      gap: "8px 16px",
                      color: "var(--ink)",
                    }}
                  >
                    {Object.entries(selectedAsset.parameters).map(
                      ([key, val]) => (
                        <div key={key}>
                          <span style={{ color: "var(--soft)" }}>{key}: </span>
                          <strong>{JSON.stringify(val)}</strong>
                        </div>
                      ),
                    )}
                  </div>
                </div>
              )}

            {/* 跨模态复用快捷操作栏 (核心验收) */}
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                gap: "8px",
                borderTop: "1px solid var(--line)",
                paddingTop: "12px",
              }}
            >
              <div
                style={{
                  fontSize: "12px",
                  fontWeight: 600,
                  color: "var(--ink)",
                }}
              >
                跨模态素材复用与工作台联动：
              </div>
              <div
                style={{
                  display: "flex",
                  flexWrap: "wrap",
                  gap: "8px",
                }}
              >
                {/* 1. 图片素材跨模态动作 */}
                {selectedAsset.assetType === "image" &&
                  selectedAsset.path &&
                  selectedAsset.fileExists !== false && (
                    <>
                      <button
                        type="button"
                        onClick={() => {
                          handleUseAsVideoFirstFrame(selectedAsset);
                          setSelectedAsset(null);
                        }}
                        style={{
                          padding: "6px 12px",
                          borderRadius: "var(--radius-sm)",
                          border: "1px solid var(--accent)",
                          background: "var(--accent-subtle)",
                          color: "var(--accent)",
                          fontSize: "12px",
                          fontWeight: 600,
                          cursor: "pointer",
                        }}
                        title="作为视频首帧 (first_frame) 发送至视频工作台再生成"
                      >
                        🎬 作为视频首帧再生成
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          handleUseAsVideoLastFrame(selectedAsset);
                          setSelectedAsset(null);
                        }}
                        style={{
                          padding: "6px 12px",
                          borderRadius: "var(--radius-sm)",
                          border: "1px solid var(--line)",
                          background: "var(--surface-1)",
                          color: "var(--ink)",
                          fontSize: "12px",
                          cursor: "pointer",
                        }}
                      >
                        🎬 作为视频末帧
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          handleUseAsImageSource(selectedAsset);
                          setSelectedAsset(null);
                        }}
                        style={{
                          padding: "6px 12px",
                          borderRadius: "var(--radius-sm)",
                          border: "1px solid var(--line)",
                          background: "var(--surface-1)",
                          color: "var(--ink)",
                          fontSize: "12px",
                          cursor: "pointer",
                        }}
                      >
                        🖼️ 作为图生图原图
                      </button>
                    </>
                  )}

                {/* 2. 音频素材跨模态动作 */}
                {selectedAsset.assetType === "audio" &&
                  selectedAsset.path &&
                  selectedAsset.fileExists !== false && (
                    <button
                      type="button"
                      onClick={() => {
                        handleUseAsAudioReference(selectedAsset);
                        setSelectedAsset(null);
                      }}
                      style={{
                        padding: "6px 12px",
                        borderRadius: "var(--radius-sm)",
                        border: "1px solid var(--accent)",
                        background: "var(--accent-subtle)",
                        color: "var(--accent)",
                        fontSize: "12px",
                        fontWeight: 600,
                        cursor: "pointer",
                      }}
                    >
                      🎙️ 作为参考音频/克隆声音
                    </button>
                  )}

                {/* 3. 提示词跨模态动作 */}
                {selectedAsset.prompt && (
                  <>
                    <button
                      type="button"
                      onClick={() => {
                        handleReusePromptInTts(selectedAsset.prompt);
                        setSelectedAsset(null);
                      }}
                      style={{
                        padding: "6px 12px",
                        borderRadius: "var(--radius-sm)",
                        border: "1px solid var(--line)",
                        background: "var(--surface-1)",
                        color: "var(--ink)",
                        fontSize: "12px",
                        cursor: "pointer",
                      }}
                    >
                      🎙️ 提示词送至语音合成 (TTS)
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        handleReusePromptInChat(selectedAsset.prompt);
                        setSelectedAsset(null);
                      }}
                      style={{
                        padding: "6px 12px",
                        borderRadius: "var(--radius-sm)",
                        border: "1px solid var(--line)",
                        background: "var(--surface-1)",
                        color: "var(--ink)",
                        fontSize: "12px",
                        cursor: "pointer",
                      }}
                    >
                      💬 提示词送至对话 (Chat)
                    </button>
                  </>
                )}

                {/* 4. 一键复用参数重新生成 */}
                <button
                  type="button"
                  className="primary"
                  onClick={() => {
                    handleReuseFullParameters(selectedAsset);
                    setSelectedAsset(null);
                  }}
                  style={{
                    padding: "6px 16px",
                    borderRadius: "var(--radius-sm)",
                    fontSize: "12px",
                    fontWeight: 600,
                    marginLeft: "auto",
                  }}
                >
                  🔄 复用参数与提示词重新生成
                </button>
              </div>
            </div>
          </div>
        </DialogFrame>
      )}
    </div>
  );
}
