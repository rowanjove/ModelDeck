import { useInfiniteQuery } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../../api";
import { AudioThumb } from "../../components/AudioThumb";
import { DialogFrame } from "../../components/DialogFrame";
import { ImageThumb } from "../../components/ImageThumb";
import { VideoThumb } from "../../components/VideoThumb";
import { useAudioDraft, useImageDraft, useVideoDraft } from "../../store";
import type { AssetLibraryItem } from "../../types";

export function AssetLibraryView({
  onOpenImage,
  onOpenVideo,
  onOpenAudio,
}: {
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

  const copyPath = (path: string, id: string) => {
    navigator.clipboard.writeText(path);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const handleUseAsset = (asset: AssetLibraryItem) => {
    if (!asset.path) return;
    if (asset.assetType === "image") {
      useImageDraft.getState().patch({ sourcePath: asset.path });
      onOpenImage();
    } else if (asset.assetType === "video") {
      useVideoDraft.getState().patch({ firstPath: asset.path });
      onOpenVideo();
    } else if (asset.assetType === "audio") {
      useAudioDraft.getState().patch({ sourcePath: asset.path });
      onOpenAudio();
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
        background: "var(--bg, #0f1117)",
      }}
    >
      {/* Header bar */}
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
          <h2 style={{ margin: "0 0 4px", fontSize: "16px", fontWeight: 700 }}>
            媒体素材库 (Asset Library)
          </h2>
          <p className="muted" style={{ margin: 0, fontSize: "12.5px" }}>
            集中浏览与管理所有 AI
            生成的图像、视频和音频，支持一键复用到工作台与快速预览。
          </p>
        </div>

        {/* Search input */}
        <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
          <input
            type="text"
            placeholder="搜索 Prompt、模型或路径..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            style={{
              padding: "6px 12px",
              background: "var(--surface-1, #161922)",
              color: "var(--ink)",
              border: "1px solid var(--border, #2d3139)",
              borderRadius: "4px",
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
              padding: "6px 10px",
              background: "var(--surface-2, #212631)",
              color: "var(--fg, #e1e4ea)",
              border: "1px solid var(--border, #2d3139)",
              borderRadius: "4px",
              cursor: "pointer",
              fontSize: "12px",
            }}
          >
            {assetsQuery.isFetching ? "刷新中…" : "刷新"}
          </button>
        </div>
      </div>

      {/* Filter Tabs */}
      <div
        style={{
          display: "flex",
          gap: "6px",
          marginBottom: "18px",
          borderBottom: "1px solid var(--border, #2d3139)",
          paddingBottom: "8px",
        }}
      >
        {[
          { id: "all", label: `全部素材 (${assets.length})` },
          { id: "image", label: " 图像 (Image)" },
          { id: "video", label: " 视频 (Video)" },
          { id: "audio", label: " 音频 (Audio)" },
        ].map((tab) => (
          <button
            key={tab.id}
            type="button"
            onClick={() => setAssetType(tab.id)}
            style={{
              padding: "5px 12px",
              borderRadius: "4px",
              border: "none",
              background:
                assetType === tab.id ? "var(--accent, #3b82f6)" : "transparent",
              color: assetType === tab.id ? "#fff" : "var(--muted, #8b949e)",
              cursor: "pointer",
              fontWeight: 600,
              fontSize: "12px",
            }}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Grid of assets */}
      {assets.length === 0 ? (
        <div
          style={{
            flex: 1,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--muted, #8b949e)",
            padding: "48px 0",
          }}
        >
          <div style={{ fontSize: "14px" }}>暂无符合条件的媒体素材</div>
          <div style={{ fontSize: "12px", marginTop: "4px" }}>
            在图像、视频或音频工作台生成后将自动归档至此
          </div>
        </div>
      ) : (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(240px, 1fr))",
            gap: "16px",
          }}
        >
          {assets.map((asset) => {
            const aType =
              asset.assetType ||
              ("asset_type" in asset
                ? String((asset as Record<string, unknown>).asset_type)
                : undefined) ||
              "image";
            return (
              <div
                key={asset.id}
                style={{
                  background: "var(--surface-1, #161922)",
                  border: "1px solid var(--border, #2d3139)",
                  borderRadius: "8px",
                  overflow: "hidden",
                  display: "flex",
                  flexDirection: "column",
                  transition: "transform 0.15s, border-color 0.15s",
                }}
              >
                {/* Media Preview Box */}
                <div
                  style={{
                    height: "170px",
                    background: "#080a0f",
                    position: "relative",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    overflow: "hidden",
                    cursor: "pointer",
                  }}
                  onClick={() => setSelectedAsset(asset)}
                >
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

                  {/* Badges on preview */}
                  <div
                    style={{
                      position: "absolute",
                      top: "8px",
                      left: "8px",
                      padding: "2px 6px",
                      borderRadius: "4px",
                      background: "rgba(0,0,0,0.7)",
                      color: "var(--accent-ink)",
                      fontSize: "10px",
                      fontWeight: 700,
                      textTransform: "uppercase",
                    }}
                  >
                    {aType}
                  </div>

                  {(asset.width && asset.height) || asset.durationMs ? (
                    <div
                      style={{
                        position: "absolute",
                        bottom: "8px",
                        right: "8px",
                        padding: "2px 6px",
                        borderRadius: "4px",
                        background: "rgba(0,0,0,0.7)",
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

                {/* Metadata & Actions */}
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
                      color: "var(--fg, #e1e4ea)",
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
                      color: "var(--muted, #8b949e)",
                      display: "flex",
                      justifyContent: "space-between",
                    }}
                  >
                    <span>{asset.modelName}</span>
                    <span>
                      {new Date(asset.createdAt).toLocaleDateString()}
                    </span>
                  </div>

                  {/* Buttons */}
                  <div
                    style={{
                      display: "flex",
                      gap: "6px",
                      marginTop: "6px",
                      paddingTop: "6px",
                      borderTop: "1px solid var(--border, #2d3139)",
                    }}
                  >
                    <button
                      type="button"
                      onClick={() => handleUseAsset(asset)}
                      style={{
                        flex: 1,
                        padding: "4px 8px",
                        fontSize: "11px",
                        background: "var(--accent, #3b82f6)",
                        color: "var(--accent-ink)",
                        border: "none",
                        borderRadius: "4px",
                        cursor: "pointer",
                        fontWeight: 600,
                      }}
                    >
                      在工作台复用
                    </button>
                    {asset.path && (
                      <button
                        type="button"
                        onClick={() => copyPath(asset.path ?? "", asset.id)}
                        style={{
                          padding: "4px 8px",
                          fontSize: "11px",
                          background: "var(--surface-2, #212631)",
                          color:
                            copiedId === asset.id
                              ? "#34d399"
                              : "var(--fg, #e1e4ea)",
                          border: "1px solid var(--border, #2d3139)",
                          borderRadius: "4px",
                          cursor: "pointer",
                        }}
                      >
                        {copiedId === asset.id ? "已复制" : "复制路径"}
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
              background: "var(--surface-2, #212631)",
              color: "var(--fg, #e1e4ea)",
              border: "1px solid var(--border, #2d3139)",
              borderRadius: "6px",
              cursor: "pointer",
              fontSize: "13px",
            }}
          >
            {assetsQuery.isFetchingNextPage ? "加载中…" : "⬇ 加载更多素材"}
          </button>
        </div>
      )}

      {/* Asset Preview Modal */}
      {selectedAsset && (
        <DialogFrame label="素材预览" onClose={() => setSelectedAsset(null)}>
          <div
            style={{
              width: "700px",
              maxWidth: "92vw",
              maxHeight: "90vh",
              background: "var(--surface-1, #161922)",
              border: "1px solid var(--border, #2d3139)",
              borderRadius: "10px",
              padding: "20px",
              display: "flex",
              flexDirection: "column",
              gap: "14px",
              overflowY: "auto",
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
              }}
            >
              <div style={{ fontWeight: 700, fontSize: "15px" }}>
                素材详情 · {selectedAsset.assetType?.toUpperCase()}
              </div>
              <button
                onClick={() => setSelectedAsset(null)}
                style={{
                  background: "none",
                  border: "none",
                  color: "var(--ink)",
                  cursor: "pointer",
                  fontSize: "16px",
                }}
              >
                关闭
              </button>
            </div>

            {/* Media full preview */}
            <div
              style={{
                width: "100%",
                maxHeight: "380px",
                background: "#080a0f",
                borderRadius: "6px",
                overflow: "hidden",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              {selectedAsset.path && selectedAsset.assetType === "image" && (
                <ImageThumb
                  path={selectedAsset.path}
                  alt={selectedAsset.prompt}
                />
              )}
              {selectedAsset.path && selectedAsset.assetType === "video" && (
                <VideoThumb path={selectedAsset.path} />
              )}
              {selectedAsset.path && selectedAsset.assetType === "audio" && (
                <div style={{ width: "90%", padding: "20px 0" }}>
                  <AudioThumb path={selectedAsset.path} />
                </div>
              )}
            </div>

            {/* Prompt */}
            <div>
              <div
                style={{
                  fontSize: "11px",
                  color: "var(--muted, #8b949e)",
                  marginBottom: "4px",
                }}
              >
                提示词 (Prompt)
              </div>
              <div
                style={{
                  padding: "8px 12px",
                  background: "var(--surface-2, #1c202c)",
                  borderRadius: "4px",
                  fontSize: "12px",
                  lineHeight: "1.5",
                  maxHeight: "90px",
                  overflowY: "auto",
                }}
              >
                {selectedAsset.prompt || "（无）"}
              </div>
            </div>

            {/* Info details */}
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "1fr 1fr",
                gap: "10px",
                fontSize: "12px",
                background: "var(--surface-2, #1c202c)",
                padding: "10px 12px",
                borderRadius: "4px",
              }}
            >
              <div>
                模型: <strong>{selectedAsset.modelName}</strong>
              </div>
              <div>
                厂商: <strong>{selectedAsset.providerName}</strong>
              </div>
              <div>
                规格:{" "}
                <strong>
                  {selectedAsset.width && selectedAsset.height
                    ? `${selectedAsset.width} × ${selectedAsset.height}`
                    : selectedAsset.durationMs
                      ? `${((selectedAsset.durationMs || 0) / 1000).toFixed(1)} 秒`
                      : selectedAsset.mimeType || "-"}
                </strong>
              </div>
              <div>
                生成时间:{" "}
                <strong>
                  {new Date(selectedAsset.createdAt).toLocaleString()}
                </strong>
              </div>
            </div>

            {/* Path & Actions */}
            {selectedAsset.path && (
              <div
                style={{
                  fontSize: "11px",
                  color: "var(--muted, #8b949e)",
                  fontFamily: "monospace",
                  wordBreak: "break-all",
                }}
              >
                文件路径: {selectedAsset.path}
              </div>
            )}

            <div
              style={{
                display: "flex",
                justifyContent: "flex-end",
                gap: "8px",
                marginTop: "4px",
              }}
            >
              {selectedAsset.path && (
                <button
                  type="button"
                  onClick={() =>
                    copyPath(selectedAsset.path ?? "", selectedAsset.id)
                  }
                  style={{
                    padding: "6px 14px",
                    background: "var(--surface-2, #212631)",
                    color: "var(--fg, #e1e4ea)",
                    border: "1px solid var(--border, #2d3139)",
                    borderRadius: "4px",
                    cursor: "pointer",
                    fontSize: "12px",
                  }}
                >
                  {copiedId === selectedAsset.id
                    ? "已复制路径 "
                    : "复制文件路径"}
                </button>
              )}
              <button
                type="button"
                onClick={() => {
                  handleUseAsset(selectedAsset);
                  setSelectedAsset(null);
                }}
                style={{
                  padding: "6px 16px",
                  background: "var(--accent, #3b82f6)",
                  color: "var(--accent-ink)",
                  border: "none",
                  borderRadius: "4px",
                  cursor: "pointer",
                  fontSize: "12px",
                  fontWeight: 600,
                }}
              >
                在对应工作台复用
              </button>
            </div>
          </div>
        </DialogFrame>
      )}
    </div>
  );
}
