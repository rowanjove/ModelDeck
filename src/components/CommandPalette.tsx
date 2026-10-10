import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useModelPick } from "../store";
import { MODALITY_LABELS } from "../types";

export interface CommandPaletteProps {
  onClose: () => void;
  onNavigate: (route: string) => void;
  onStartNew: () => void;
}

interface PaletteAction {
  id: string;
  type: "nav" | "action" | "model";
  title: string;
  subtitle?: string;
  badge?: string;
  shortcut?: string;
  action: () => void;
}

export function CommandPalette({
  onClose,
  onNavigate,
  onStartNew,
}: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);

  const dialogRef = useRef<HTMLDialogElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const models = useQuery({ queryKey: ["models"], queryFn: api.models });
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: api.providers,
  });

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    dialog.showModal();
    inputRef.current?.focus();
    return () => {
      if (dialog.open) {
        dialog.close();
      }
    };
  }, []);

  const needle = query.trim().toLowerCase();

  // Navigation Items
  const navItems: PaletteAction[] = [
    {
      id: "nav-text",
      type: "nav",
      title: "文本对话 (Text Chat)",
      subtitle: "多轮对话、上下文压缩、代码与长文本生成",
      badge: "页面",
      shortcut: "Ctrl+1",
      action: () => {
        onNavigate("text");
        onClose();
      },
    },
    {
      id: "nav-image",
      type: "nav",
      title: "图像工作台 (Image Workspace)",
      subtitle: "文生图、图生图、局部重绘与风格参考",
      badge: "页面",
      shortcut: "Ctrl+2",
      action: () => {
        onNavigate("image");
        onClose();
      },
    },
    {
      id: "nav-video",
      type: "nav",
      title: "视频工作台 (Video Workspace)",
      subtitle: "首末帧生成、运动控制、视频转视频",
      badge: "页面",
      shortcut: "Ctrl+3",
      action: () => {
        onNavigate("video");
        onClose();
      },
    },
    {
      id: "nav-audio",
      type: "nav",
      title: "音频工作台 (Audio Workspace)",
      subtitle: "TTS 语音合成、语音克隆、声音库管理",
      badge: "页面",
      shortcut: "Ctrl+4",
      action: () => {
        onNavigate("audio");
        onClose();
      },
    },
    {
      id: "nav-workflow",
      type: "nav",
      title: "工作流流水线 (Pipeline)",
      subtitle: "跨模态自动化串联、链式提示词与媒体流转",
      badge: "页面",
      shortcut: "Ctrl+5",
      action: () => {
        onNavigate("workflow");
        onClose();
      },
    },
    {
      id: "nav-history",
      type: "nav",
      title: "历史与素材库 (History & Library)",
      subtitle: "生成任务回溯、媒体资产画廊、跨库全文检索",
      badge: "页面",
      shortcut: "Ctrl+6",
      action: () => {
        onNavigate("history");
        onClose();
      },
    },
    {
      id: "nav-analytics",
      type: "nav",
      title: "用量与成本统计 (Analytics)",
      subtitle: "Token 消耗明细、模态分布、自定义模型单价",
      badge: "页面",
      shortcut: "Ctrl+7",
      action: () => {
        onNavigate("analytics");
        onClose();
      },
    },
    {
      id: "nav-developer",
      type: "nav",
      title: "API 调试器 (Developer Mode)",
      subtitle: "Postman 风格接口测试、SSE 事件观测、cURL 导出",
      badge: "页面",
      shortcut: "Ctrl+8",
      action: () => {
        onNavigate("developer");
        onClose();
      },
    },
    {
      id: "nav-models",
      type: "nav",
      title: "模型配置库 (Models)",
      subtitle: "查看与配置各模态模型定义及参数规范",
      badge: "页面",
      shortcut: "Ctrl+9",
      action: () => {
        onNavigate("models");
        onClose();
      },
    },
    {
      id: "nav-settings",
      type: "nav",
      title: "系统设置 (Settings)",
      subtitle: "Provider 接入、本地服务发现与网络代理",
      badge: "页面",
      action: () => {
        onNavigate("settings");
        onClose();
      },
    },
  ];

  // Quick Actions
  const actionItems: PaletteAction[] = [
    {
      id: "action-new",
      type: "action",
      title: "＋ 新建草稿 (New Draft)",
      subtitle: "清空当前工作区输入并开启全新任务",
      badge: "操作",
      shortcut: "Ctrl+N",
      action: () => {
        onStartNew();
        onClose();
      },
    },
  ];

  // Model Items
  const modelItems: PaletteAction[] = (models.data ?? [])
    .filter((model) => {
      if (!model.enabled) return false;
      const provider = providers.data?.find((p) => p.id === model.providerId);
      return provider ? provider.enabled : true;
    })
    .map((model) => {
      const provider = providers.data?.find((p) => p.id === model.providerId);
      return {
        id: `model-${model.id}`,
        type: "model",
        title: model.name,
        subtitle: `${provider?.name || "本地服务"} · ${model.modelId}`,
        badge: MODALITY_LABELS[model.modality] || model.modality,
        action: () => {
          useModelPick.getState().request(model.id, model.modality);
          onNavigate(model.modality);
          onClose();
        },
      };
    });

  // Filter all actions based on search query
  const allActions = [...navItems, ...actionItems, ...modelItems].filter(
    (item) => {
      if (!needle) return true;
      return (
        item.title.toLowerCase().includes(needle) ||
        item.subtitle?.toLowerCase().includes(needle) ||
        item.badge?.toLowerCase().includes(needle)
      );
    },
  );

  // Keep selected index within bounds
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset index when search query changes
  useEffect(() => {
    setSelectedIndex(0);
  }, [needle]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((prev) => (prev + 1) % Math.max(1, allActions.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex(
        (prev) =>
          (prev - 1 + allActions.length) % Math.max(1, allActions.length),
      );
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (allActions[selectedIndex]) {
        allActions[selectedIndex].action();
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  return (
    <dialog
      ref={dialogRef}
      className="switcher-backdrop"
      aria-label="命令面板"
      onClick={(e) => {
        if (e.target === e.currentTarget) {
          onClose();
        }
      }}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      style={{
        position: "fixed",
        inset: 0,
        width: "100%",
        height: "100%",
        border: "none",
        background: "rgba(0,0,0,0.6)",
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        paddingTop: "10vh",
        zIndex: 9999,
      }}
    >
      <div
        className="switcher"
        style={{
          width: "min(580px, calc(100vw - 32px))",
          background: "var(--surface-1)",
          border: "1px solid var(--line)",
          borderRadius: "var(--radius-lg)",
          boxShadow: "var(--shadow-lg)",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          maxHeight: "68vh",
          padding: 0,
        }}
      >
        {/* Search Input Bar */}
        <div
          style={{
            padding: "12px 16px",
            borderBottom: "1px solid var(--line)",
            display: "flex",
            alignItems: "center",
            gap: "10px",
            background: "var(--raised)",
          }}
        >
          <span style={{ fontSize: "14px", color: "var(--muted)" }}>⌘</span>
          <input
            ref={inputRef}
            aria-label="搜索命令、功能或 AI 模型"
            placeholder="搜索页面、功能命令或 AI 模型... (Ctrl+K)"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            style={{
              flex: 1,
              background: "none",
              border: "none",
              outline: "none",
              color: "var(--ink)",
              fontSize: "13.5px",
              padding: 0,
            }}
          />
          {query && (
            <button
              onClick={() => setQuery("")}
              style={{
                background: "none",
                border: "none",
                color: "var(--muted)",
                cursor: "pointer",
                fontSize: "12px",
                padding: "2px 6px",
              }}
            >
              ✕
            </button>
          )}
        </div>

        {/* Action Results List */}
        <div
          style={{
            flex: 1,
            overflowY: "auto",
            padding: "6px 8px",
            display: "flex",
            flexDirection: "column",
            gap: "3px",
            background: "var(--surface-1)",
          }}
        >
          {allActions.length === 0 ? (
            <div
              style={{
                padding: "32px 16px",
                textAlign: "center",
                color: "var(--muted)",
                fontSize: "13px",
              }}
            >
              未找到匹配的命令或模型
            </div>
          ) : (
            allActions.map((item, idx) => {
              const isSelected = idx === selectedIndex;
              return (
                <div
                  key={item.id}
                  onClick={item.action}
                  onMouseEnter={() => setSelectedIndex(idx)}
                  style={{
                    padding: "7px 12px",
                    borderRadius: "var(--radius-md)",
                    cursor: "pointer",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    background: isSelected
                      ? "var(--accent-subtle)"
                      : "transparent",
                    border: isSelected
                      ? "1px solid var(--accent-border)"
                      : "1px solid transparent",
                    transition: "background var(--transition-fast)",
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      flexDirection: "column",
                      gap: "1px",
                      minWidth: 0,
                    }}
                  >
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: "8px",
                      }}
                    >
                      <strong
                        style={{
                          fontSize: "13px",
                          color: isSelected ? "var(--accent)" : "var(--ink)",
                          fontWeight: 600,
                        }}
                      >
                        {item.title}
                      </strong>
                      {item.badge && (
                        <span
                          style={{
                            fontSize: "10px",
                            padding: "1px 5px",
                            borderRadius: "var(--radius-xs)",
                            background:
                              item.type === "model"
                                ? "var(--ok-subtle)"
                                : item.type === "nav"
                                  ? "var(--info-subtle)"
                                  : "var(--warn-subtle)",
                            color:
                              item.type === "model"
                                ? "var(--ok)"
                                : item.type === "nav"
                                  ? "var(--info)"
                                  : "var(--warn)",
                            border: `1px solid ${
                              item.type === "model"
                                ? "var(--ok-border)"
                                : item.type === "nav"
                                  ? "var(--info-border)"
                                  : "var(--warn-border)"
                            }`,
                            fontWeight: 600,
                          }}
                        >
                          {item.badge}
                        </span>
                      )}
                    </div>
                    {item.subtitle && (
                      <span
                        style={{
                          fontSize: "11.5px",
                          color: "var(--soft)",
                          whiteSpace: "nowrap",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        }}
                      >
                        {item.subtitle}
                      </span>
                    )}
                  </div>

                  {item.shortcut && (
                    <kbd
                      style={{
                        fontSize: "11px",
                        fontFamily: "var(--mono)",
                        color: "var(--soft)",
                        background: "rgba(0,0,0,0.04)",
                        padding: "2px 6px",
                        borderRadius: "var(--radius-xs)",
                        border: "1px solid var(--line)",
                        flexShrink: 0,
                        marginLeft: "12px",
                      }}
                    >
                      {item.shortcut}
                    </kbd>
                  )}
                </div>
              );
            })
          )}
        </div>

        {/* Footer shortcuts hint */}
        <div
          style={{
            padding: "8px 14px",
            borderTop: "1px solid var(--line)",
            background: "var(--surface-2)",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            fontSize: "11.5px",
            color: "var(--muted)",
          }}
        >
          <div style={{ display: "flex", gap: "12px" }}>
            <span>↑↓ 移动</span>
            <span>↵ 执行</span>
            <span>Esc 关闭</span>
          </div>
          <div>Ctrl+1~9 快捷切换</div>
        </div>
      </div>
    </dialog>
  );
}
