import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, asAppError } from "../../api";
import { DialogFrame } from "../../components/DialogFrame";
import { ErrorNote } from "../../components/ErrorNote";
import type { AppError, ToolPermissionLevel } from "../../types";

const BUILTIN_TOOL_INFOS = [
  {
    name: "current_time",
    label: "获取当前时间 (current_time)",
    description: "读取系统当前 UTC 时间，无额外参数，零安全风险。",
    category: "system",
  },
  {
    name: "read_file",
    label: "读取文件 (read_file)",
    description: "读取指定路径的文本文件，受受控数据目录边界隔离保护。",
    category: "filesystem",
  },
  {
    name: "list_directory",
    label: "列出目录 (list_directory)",
    description: "查看指定目录下的文件列表与结构信息。",
    category: "filesystem",
  },
  {
    name: "memory",
    label: "长期记忆 (memory)",
    description: "向本地 SQLite 记忆库保存与检索长期偏好及上下文信息。",
    category: "memory",
  },
  {
    name: "http_request",
    label: "网络请求 (http_request)",
    description: "向外部或内网 HTTP/HTTPS 目标发起 GET/POST 请求。",
    category: "network",
  },
  {
    name: "write_file",
    label: "写入文件 (write_file)",
    description: "向本地磁盘创建或覆盖文本文件。高风险操作。",
    category: "filesystem",
  },
  {
    name: "delete_file",
    label: "删除文件 (delete_file)",
    description: "从本地磁盘永久删除指定文件。高风险不可逆操作。",
    category: "filesystem",
  },
  {
    name: "run_command",
    label: "执行 Shell 命令行 (run_command)",
    description: "在系统终端中执行 cmd/sh 命令并收集标准输出。最高安全敏感级。",
    category: "shell",
  },
];

export function ToolPermissionsModal({ onClose }: { onClose: () => void }) {
  const queryClient = useQueryClient();
  const [error, setError] = useState<AppError | null>(null);

  const permissionsQuery = useQuery({
    queryKey: ["tool-permissions"],
    queryFn: api.toolPermissionsList,
  });

  const mcpServersQuery = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: api.mcpServers,
  });

  const updatePermission = useMutation({
    mutationFn: ({
      toolName,
      level,
    }: { toolName: string; level: ToolPermissionLevel }) =>
      api.toolPermissionsSet(toolName, level),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["tool-permissions"] });
    },
    onError: (err) => setError(asAppError(err)),
  });

  const permissions = permissionsQuery.data ?? {};

  const getLevel = (
    toolName: string,
    defaultLevel: ToolPermissionLevel,
  ): ToolPermissionLevel => {
    return permissions[toolName] ?? defaultLevel;
  };

  const renderBadge = (level: ToolPermissionLevel) => {
    switch (level) {
      case "safe":
        return (
          <span
            style={{
              color: "var(--success, #22c55e)",
              fontWeight: 600,
              fontSize: "12px",
            }}
          >
            自动允许 (Safe)
          </span>
        );
      case "confirm":
        return (
          <span
            style={{
              color: "var(--warning, #eab308)",
              fontWeight: 600,
              fontSize: "12px",
            }}
          >
            需人工确认 (Confirm)
          </span>
        );
      case "restricted":
        return (
          <span
            style={{
              color: "var(--danger, #ef4444)",
              fontWeight: 600,
              fontSize: "12px",
            }}
          >
            禁止调用 (Restricted)
          </span>
        );
    }
  };

  return (
    <DialogFrame label="工具权限" onClose={onClose}>
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--line)",
          borderRadius: "10px",
          width: "100%",
          maxWidth: "760px",
          maxHeight: "85vh",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          boxShadow: "var(--shadow-lg)",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          style={{
            padding: "16px 20px",
            borderBottom: "1px solid var(--line)",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <div>
            <h3 style={{ margin: 0, fontSize: "17px" }}>
              Agent 工具权限分级配置
            </h3>
            <span className="muted" style={{ fontSize: "12px" }}>
              控制 Agent
              工具的自动化边界。高风险操作建议设置为「需确认」或「禁止」。
            </span>
          </div>
          <button
            type="button"
            className="linkish"
            onClick={onClose}
            style={{ fontSize: "18px" }}
          >
            关闭
          </button>
        </div>

        <div style={{ padding: "16px 20px", overflowY: "auto", flex: 1 }}>
          {error && <ErrorNote error={error} />}

          <h4
            style={{
              margin: "0 0 10px 0",
              fontSize: "14px",
              color: "var(--accent)",
            }}
          >
            内置工具权限 (Built-in Tools)
          </h4>
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: "10px",
              marginBottom: "24px",
            }}
          >
            {BUILTIN_TOOL_INFOS.map((tool) => {
              const currentLevel = getLevel(
                tool.name,
                tool.category === "system" || tool.category === "memory"
                  ? "safe"
                  : "confirm",
              );
              return (
                <div
                  key={tool.name}
                  className="permission-row"
                  style={{
                    padding: "12px 14px",
                    background: "var(--raised)",
                    border: "1px solid var(--line)",
                    borderRadius: "6px",
                    display: "flex",
                    justifyContent: "space-between",
                    alignItems: "center",
                    gap: "12px",
                  }}
                >
                  <div style={{ flex: 1 }}>
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: "8px",
                        marginBottom: "4px",
                      }}
                    >
                      <strong style={{ fontSize: "13px" }}>{tool.label}</strong>
                      {renderBadge(currentLevel)}
                    </div>
                    <div
                      className="muted"
                      style={{ fontSize: "12px", lineHeight: "1.4" }}
                    >
                      {tool.description}
                    </div>
                  </div>

                  <div style={{ display: "flex", gap: "4px" }}>
                    {(
                      ["safe", "confirm", "restricted"] as ToolPermissionLevel[]
                    ).map((lvl) => (
                      <button
                        key={lvl}
                        type="button"
                        style={{
                          fontSize: "12px",
                          padding: "4px 8px",
                          borderRadius: "4px",
                          background:
                            currentLevel === lvl
                              ? "var(--accent)"
                              : "transparent",
                          color: currentLevel === lvl ? "#fff" : "var(--muted)",
                          border:
                            currentLevel === lvl
                              ? "1px solid var(--accent)"
                              : "1px solid var(--line)",
                          fontWeight: currentLevel === lvl ? 600 : "normal",
                          cursor: "pointer",
                        }}
                        disabled={updatePermission.isPending}
                        onClick={() =>
                          void updatePermission.mutateAsync({
                            toolName: tool.name,
                            level: lvl,
                          })
                        }
                      >
                        {lvl === "safe"
                          ? "Safe"
                          : lvl === "confirm"
                            ? "Confirm"
                            : "Restricted"}
                      </button>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>

          <h4
            style={{
              margin: "0 0 10px 0",
              fontSize: "14px",
              color: "var(--accent)",
            }}
          >
            MCP 外部服务与工具 (Model Context Protocol)
          </h4>
          <div
            style={{ display: "flex", flexDirection: "column", gap: "10px" }}
          >
            {(mcpServersQuery.data ?? []).length === 0 ? (
              <p className="muted" style={{ fontSize: "13px" }}>
                暂未配置 MCP 服务，可在「设置 → MCP」中添加外部 stdio / HTTP
                服务。
              </p>
            ) : (
              (mcpServersQuery.data ?? []).map((server) => {
                const serverPermKey = `mcp_server_${server.id}`;
                const currentLevel = getLevel(serverPermKey, "confirm");
                return (
                  <div
                    key={server.id}
                    className="permission-row"
                    style={{
                      padding: "12px 14px",
                      background: "var(--raised)",
                      border: "1px solid var(--line)",
                      borderRadius: "6px",
                      display: "flex",
                      justifyContent: "space-between",
                      alignItems: "center",
                      gap: "12px",
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
                        <strong style={{ fontSize: "13px" }}>
                          {server.name}
                        </strong>
                        <span className="muted" style={{ fontSize: "11px" }}>
                          [{server.transport}]{" "}
                          {server.enabled ? "已启用" : "已停用"}
                        </span>
                        {renderBadge(currentLevel)}
                      </div>
                      <div className="muted" style={{ fontSize: "12px" }}>
                        {server.transport === "http"
                          ? server.url
                          : `${server.command} ${server.args.join(" ")}`}
                      </div>
                    </div>

                    <div style={{ display: "flex", gap: "4px" }}>
                      {(
                        [
                          "safe",
                          "confirm",
                          "restricted",
                        ] as ToolPermissionLevel[]
                      ).map((lvl) => (
                        <button
                          key={lvl}
                          type="button"
                          style={{
                            fontSize: "12px",
                            padding: "4px 8px",
                            borderRadius: "4px",
                            background:
                              currentLevel === lvl
                                ? "var(--accent)"
                                : "transparent",
                            color:
                              currentLevel === lvl ? "#fff" : "var(--muted)",
                            border:
                              currentLevel === lvl
                                ? "1px solid var(--accent)"
                                : "1px solid var(--line)",
                            fontWeight: currentLevel === lvl ? 600 : "normal",
                            cursor: "pointer",
                          }}
                          disabled={updatePermission.isPending}
                          onClick={() =>
                            void updatePermission.mutateAsync({
                              toolName: serverPermKey,
                              level: lvl,
                            })
                          }
                        >
                          {lvl === "safe"
                            ? "Safe"
                            : lvl === "confirm"
                              ? "Confirm"
                              : "Restricted"}
                        </button>
                      ))}
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>

        <div
          style={{
            padding: "12px 20px",
            borderTop: "1px solid var(--line)",
            display: "flex",
            justifyContent: "flex-end",
          }}
        >
          <button type="button" className="primary" onClick={onClose}>
            完成
          </button>
        </div>
      </div>
    </DialogFrame>
  );
}
