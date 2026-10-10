import { useMutation, useQueryClient } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { api, asAppError } from "../../api";
import { ImageThumb } from "../../components/ImageThumb";
import { VideoThumb } from "../../components/VideoThumb";
import { chooseFiles } from "../../lib/files";
import { captureVideoFrame } from "../../lib/video";
import type { AppError, ModelDefinition, StoryboardShot } from "../../types";

const DEFAULT_SHOTS: StoryboardShot[] = [
  {
    id: "shot-1",
    index: 1,
    title: "镜头 01 · 序幕推镜",
    prompt: "未来赛博都市全景，霓虹灯闪烁，阴雨绵绵，镜头缓慢向前推入街道",
    firstFrame: null,
    lastFrame: null,
    camera: "Dolly In",
    motion: "Smooth",
    shot: "Wide Shot",
    status: "idle",
    outputVideoPath: null,
  },
  {
    id: "shot-2",
    index: 2,
    title: "镜头 02 · 角色登场",
    prompt: "神秘风衣侦探站在雨中的电话亭旁，雨滴从帽檐滴落，特写面部表情",
    firstFrame: null,
    lastFrame: null,
    camera: "Static",
    motion: "Subtle",
    shot: "Close-Up",
    status: "idle",
    outputVideoPath: null,
  },
];

const STORYBOARD_STORAGE_KEY = "vibe_storyboard_shots_v1";

interface StoryboardViewProps {
  model: ModelDefinition;
  onSelectShotForEditor: (shot: StoryboardShot) => void;
}

export function StoryboardView({
  model,
  onSelectShotForEditor,
}: StoryboardViewProps) {
  const queryClient = useQueryClient();
  const [error, setError] = useState<AppError | null>(null);
  const [activeShotId, setActiveShotId] = useState<string | null>(null);

  const [shots, setShots] = useState<StoryboardShot[]>(() => {
    try {
      const saved = localStorage.getItem(STORYBOARD_STORAGE_KEY);
      if (saved) {
        const parsed = JSON.parse(saved);
        if (Array.isArray(parsed) && parsed.length > 0) {
          return parsed.map((s) => ({
            ...s,
            status: s.status === "running" ? "idle" : s.status,
          }));
        }
      }
    } catch {
      // 容错读取
    }
    return DEFAULT_SHOTS;
  });

  useEffect(() => {
    try {
      localStorage.setItem(STORYBOARD_STORAGE_KEY, JSON.stringify(shots));
    } catch (e) {
      console.warn("Failed to persist storyboard shots to localStorage", e);
    }
  }, [shots]);

  const generateShotMutation = useMutation({
    mutationFn: async (shot: StoryboardShot) => {
      setActiveShotId(shot.id);
      setShots((prev) =>
        prev.map((s) =>
          s.id === shot.id ? { ...s, status: "running", error: null } : s,
        ),
      );

      const taskId = crypto.randomUUID();
      const res = await api.generateVideo(
        {
          modelDefinitionId: shot.modelDefinitionId || model.id,
          prompt: `${shot.prompt}${shot.camera ? `, ${shot.camera} camera` : ""}${shot.shot ? `, ${shot.shot}` : ""}`,
          negativePrompt: null,
          firstFrame: shot.firstFrame,
          lastFrame: shot.lastFrame,
          referenceImages: shot.referenceImages,
          referenceVideos: shot.referenceVideos,
          referenceAudios: shot.referenceAudios,
          parameters: {},
        },
        taskId,
      );

      return { shotId: shot.id, result: res };
    },
    onSuccess: ({ shotId, result }) => {
      setActiveShotId(null);
      const videoPath = result.videos[0]?.path ?? null;
      setShots((prev) =>
        prev.map((s) =>
          s.id === shotId
            ? {
                ...s,
                status: "completed",
                outputVideoPath: videoPath,
                error: null,
              }
            : s,
        ),
      );
      void queryClient.invalidateQueries({ queryKey: ["history"] });
    },
    onError: (err, shot) => {
      setActiveShotId(null);
      const appErr = asAppError(err);
      setError(appErr);
      setShots((prev) =>
        prev.map((s) =>
          s.id === shot.id
            ? { ...s, status: "failed", error: appErr.message }
            : s,
        ),
      );
    },
  });

  async function handleExtendToNext(sourceShot: StoryboardShot) {
    if (!sourceShot.outputVideoPath) return;

    try {
      // 提取该镜头的最后一帧图片为 Data URL
      const src = convertFileSrc(sourceShot.outputVideoPath);
      const lastFrameDataUrl = await captureVideoFrame(src, 0.98);

      const newIndex = shots.length + 1;
      const newShot: StoryboardShot = {
        id: `shot-${Date.now()}`,
        index: newIndex,
        title: `镜头 ${String(newIndex).padStart(2, "0")} · 接续`,
        prompt: "接续上一镜头：镜头继续移动，展现更丰富的场景细节",
        firstFrame: { path: lastFrameDataUrl, enabled: true },
        lastFrame: null,
        camera: "Smooth",
        motion: "Smooth",
        shot: "Medium Shot",
        status: "idle",
        outputVideoPath: null,
      };

      setShots((prev) => [...prev, newShot]);
    } catch (err) {
      setError(asAppError(err));
    }
  }

  function addShot() {
    const nextIdx = shots.length + 1;
    setShots((prev) => [
      ...prev,
      {
        id: `shot-${Date.now()}`,
        index: nextIdx,
        title: `镜头 ${String(nextIdx).padStart(2, "0")}`,
        prompt: "",
        firstFrame: null,
        lastFrame: null,
        status: "idle",
        outputVideoPath: null,
      },
    ]);
  }

  function removeShot(id: string) {
    if (shots.length <= 1) return;
    setShots((prev) => prev.filter((s) => s.id !== id));
  }

  async function generateAll() {
    for (const shot of shots) {
      if (shot.status === "completed" && shot.outputVideoPath) continue;
      try {
        await generateShotMutation.mutateAsync(shot);
      } catch {
        break;
      }
    }
  }

  return (
    <div className="storyboard-view" style={{ padding: "12px 0" }}>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: "1rem",
        }}
      >
        <div>
          <h3
            style={{
              margin: 0,
              display: "flex",
              alignItems: "center",
              gap: "8px",
            }}
          >
            <span> 分镜故事板 (Storyboard)</span>
            <small
              style={{
                fontWeight: "normal",
                color: "var(--text-muted, #888)",
                fontSize: "12px",
              }}
            >
              共 {shots.length} 个镜头 · 轻量化连续视频管线
            </small>
          </h3>
        </div>
        <div style={{ display: "flex", gap: "8px" }}>
          <button
            type="button"
            className="button secondary small"
            onClick={() => {
              if (window.confirm("确定重置分镜草稿为初始模板吗？")) {
                setShots(DEFAULT_SHOTS);
              }
            }}
          >
            重置草稿
          </button>
          <button
            type="button"
            className="button secondary small"
            onClick={addShot}
          >
            + 添加镜头 (Add Shot)
          </button>
          <button
            type="button"
            className="button primary small"
            onClick={() => void generateAll()}
            disabled={generateShotMutation.isPending}
          >
            {generateShotMutation.isPending
              ? "生成分镜中…"
              : "▶ 顺序生成全部镜头"}
          </button>
        </div>
      </div>

      {error ? (
        <p className="error" style={{ margin: "8px 0" }}>
          {error.message}
        </p>
      ) : null}

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(360px, 1fr))",
          gap: "1rem",
        }}
      >
        {shots.map((shot) => (
          <div
            key={shot.id}
            className="card shot-card"
            style={{
              padding: "1rem",
              border:
                activeShotId === shot.id
                  ? "2px solid var(--accent, #646cff)"
                  : "1px solid var(--border-color, #444)",
              borderRadius: "8px",
              background: "var(--bg-secondary, #242424)",
            }}
          >
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                marginBottom: "8px",
              }}
            >
              <input
                type="text"
                value={shot.title}
                onChange={(e) =>
                  setShots((prev) =>
                    prev.map((s) =>
                      s.id === shot.id ? { ...s, title: e.target.value } : s,
                    ),
                  )
                }
                style={{
                  fontWeight: "bold",
                  fontSize: "14px",
                  background: "transparent",
                  border: "none",
                  borderBottom: "1px dashed var(--border-color, #555)",
                  padding: "2px 4px",
                  color: "inherit",
                }}
              />
              <div style={{ display: "flex", gap: "4px" }}>
                <span
                  style={{
                    fontSize: "11px",
                    padding: "2px 6px",
                    borderRadius: "4px",
                    background:
                      shot.status === "completed"
                        ? "#2e7d32"
                        : shot.status === "running"
                          ? "#1565c0"
                          : shot.status === "failed"
                            ? "#c62828"
                            : "var(--bg-tertiary, #333)",
                    color: "var(--accent-ink)",
                  }}
                >
                  {shot.status === "completed"
                    ? "已完成"
                    : shot.status === "running"
                      ? "生成中"
                      : shot.status === "failed"
                        ? "失败"
                        : "待生成"}
                </span>
                {shots.length > 1 && (
                  <button
                    type="button"
                    className="button tiny danger"
                    onClick={() => removeShot(shot.id)}
                    title="删除分镜"
                  >
                    ×
                  </button>
                )}
              </div>
            </div>

            <textarea
              rows={3}
              placeholder="描述当前镜头的画面、主体与运镜…"
              value={shot.prompt}
              onChange={(e) =>
                setShots((prev) =>
                  prev.map((s) =>
                    s.id === shot.id ? { ...s, prompt: e.target.value } : s,
                  ),
                )
              }
              style={{
                width: "100%",
                fontSize: "12px",
                marginBottom: "8px",
                borderRadius: "4px",
                boxSizing: "border-box",
              }}
            />

            <div
              style={{
                display: "flex",
                gap: "8px",
                marginBottom: "8px",
                fontSize: "11px",
                alignItems: "center",
              }}
            >
              {shot.firstFrame?.path ? (
                <div
                  style={{ display: "flex", alignItems: "center", gap: "4px" }}
                >
                  <div
                    style={{
                      width: 32,
                      height: 32,
                      overflow: "hidden",
                      borderRadius: 4,
                    }}
                  >
                    <ImageThumb path={shot.firstFrame.path} alt="首帧" />
                  </div>
                  <span style={{ color: "var(--text-muted, #aaa)" }}>
                    已设首帧
                  </span>
                  <button
                    type="button"
                    className="button tiny"
                    onClick={() =>
                      setShots((prev) =>
                        prev.map((s) =>
                          s.id === shot.id ? { ...s, firstFrame: null } : s,
                        ),
                      )
                    }
                  >
                    清除
                  </button>
                </div>
              ) : (
                <button
                  type="button"
                  className="button tiny secondary"
                  onClick={async () => {
                    const picked = await chooseFiles("image", false);
                    if (picked?.[0]) {
                      setShots((prev) =>
                        prev.map((s) =>
                          s.id === shot.id
                            ? {
                                ...s,
                                firstFrame: { path: picked[0], enabled: true },
                              }
                            : s,
                        ),
                      );
                    }
                  }}
                >
                  + 上传首帧
                </button>
              )}
            </div>

            {shot.outputVideoPath ? (
              <div style={{ marginBottom: "8px" }}>
                <VideoThumb path={shot.outputVideoPath} />
              </div>
            ) : null}

            {shot.error ? (
              <p
                className="error"
                style={{ fontSize: "11px", margin: "4px 0" }}
              >
                {shot.error}
              </p>
            ) : null}

            <div style={{ display: "flex", gap: "6px", marginTop: "8px" }}>
              <button
                type="button"
                className="button primary small"
                style={{ flex: 1 }}
                onClick={() => void generateShotMutation.mutateAsync(shot)}
                disabled={generateShotMutation.isPending || !shot.prompt.trim()}
              >
                {activeShotId === shot.id ? "生成中…" : " 生成此镜头"}
              </button>
              {shot.outputVideoPath ? (
                <button
                  type="button"
                  className="button secondary small"
                  onClick={() => void handleExtendToNext(shot)}
                  title="抓取尾帧创建下一连续分镜"
                >
                  ⏩ 续写下镜
                </button>
              ) : null}
              <button
                type="button"
                className="button secondary small"
                onClick={() => onSelectShotForEditor(shot)}
                title="导入当前参数到主工作台编辑"
              >
                载入
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
