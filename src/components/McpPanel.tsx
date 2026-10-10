import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, asAppError } from "../api";
import type {
  AppError,
  McpServer,
  McpToolInfo,
  ToolPermissionLevel,
} from "../types";
import { ErrorNote } from "./ErrorNote";

interface Draft {
  id: string | null;
  name: string;
  transport: string;
  command: string;
  argsText: string;
  url: string;
  enabled: boolean;
}

export function McpPanel() {
  const queryClient = useQueryClient();
  const servers = useQuery({ queryKey: ["mcp"], queryFn: api.mcpServers });
  const permissionsQuery = useQuery({
    queryKey: ["tool-permissions"],
    queryFn: api.toolPermissionsList,
  });
  const [draft, setDraft] = useState<Draft>(blank());
  const [error, setError] = useState<AppError | null>(null);
  const [tools, setTools] = useState<McpToolInfo[] | null>(null);

  const save = useMutation({
    mutationFn: async (next: Draft) => {
      const input = {
        name: next.name.trim(),
        transport: next.transport,
        command: next.command.trim(),
        args: next.argsText
          .split(/\r?\n/)
          .map((line) => line.trim())
          .filter((line) => line.length > 0),
        url: next.url.trim(),
        enabled: next.enabled,
      };
      if (next.id) {
        return api.updateMcpServer(next.id, input);
      }
      return api.createMcpServer(input);
    },
    onSuccess: async (server) => {
      await queryClient.invalidateQueries({ queryKey: ["mcp"] });
      setDraft(fromServer(server));
      setError(null);
      setTools(null);
    },
    onError: (reason) => setError(asAppError(reason)),
  });

  const remove = useMutation({
    mutationFn: api.deleteMcpServer,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["mcp"] });
      setDraft(blank());
      setError(null);
      setTools(null);
    },
    onError: (reason) => setError(asAppError(reason)),
  });

  const probe = useMutation({
    mutationFn: api.mcpTools,
    onSuccess: (listed) => {
      setError(null);
      setTools(listed);
    },
    onError: (reason) => {
      setTools(null);
      setError(asAppError(reason));
    },
  });

  const updateToolPerm = useMutation({
    mutationFn: ({
      toolName,
      level,
    }: { toolName: string; level: ToolPermissionLevel }) =>
      api.toolPermissionsSet(toolName, level),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["tool-permissions"] });
    },
    onError: (reason) => setError(asAppError(reason)),
  });

  const permissions = permissionsQuery.data ?? {};

  return (
    <section>
      <h2>MCP (Model Context Protocol)</h2>
      <p className="muted">
        只连接已启用的服务。命令和参数按行填写，不会经过 shell。Windows 上请填写
        npx.cmd，而不是 npx。不要把密钥写进地址。
      </p>
      <ul className="file-list">
        {(servers.data ?? []).map((server) => (
          <li key={server.id}>
            <button
              type="button"
              onClick={() => {
                setDraft(fromServer(server));
                setTools(null);
              }}
            >
              {server.name}
              {server.enabled ? "" : " · 停用"}
            </button>
          </li>
        ))}
      </ul>
      <div className="form-grid">
        <label>
          名称
          <input
            value={draft.name}
            onChange={(event) =>
              setDraft({ ...draft, name: event.target.value })
            }
          />
        </label>
        <label>
          传输
          <select
            value={draft.transport}
            onChange={(event) =>
              setDraft({ ...draft, transport: event.target.value })
            }
          >
            <option value="stdio">stdio</option>
            <option value="http">http</option>
          </select>
        </label>
      </div>
      {draft.transport === "http" ? (
        <label className="field">
          地址
          <input
            value={draft.url}
            onChange={(event) =>
              setDraft({ ...draft, url: event.target.value })
            }
            placeholder="http://127.0.0.1:3000/mcp"
          />
        </label>
      ) : (
        <>
          <label className="field">
            命令
            <input
              value={draft.command}
              onChange={(event) =>
                setDraft({ ...draft, command: event.target.value })
              }
              placeholder="npx.cmd"
            />
          </label>
          <label className="field">
            参数
            <textarea
              rows={3}
              value={draft.argsText}
              onChange={(event) =>
                setDraft({ ...draft, argsText: event.target.value })
              }
              placeholder={"-y\n@modelcontextprotocol/server-everything"}
            />
          </label>
        </>
      )}
      <label className="check">
        <input
          type="checkbox"
          checked={draft.enabled}
          onChange={(event) =>
            setDraft({ ...draft, enabled: event.target.checked })
          }
        />
        启用
      </label>

      {error ? <ErrorNote error={error} /> : null}

      <div className="inline-actions">
        <button
          type="button"
          className="primary"
          disabled={save.isPending || !draft.name.trim()}
          onClick={() => void save.mutateAsync(draft)}
        >
          {save.isPending ? "保存中…" : "保存 MCP"}
        </button>
        <button
          type="button"
          onClick={() => {
            setDraft(blank());
            setTools(null);
          }}
        >
          新建
        </button>
        {draft.id ? (
          <button
            type="button"
            disabled={probe.isPending}
            onClick={() => void probe.mutateAsync(draft.id ?? "")}
          >
            {probe.isPending ? "读取中…" : "读取工具"}
          </button>
        ) : null}
        {draft.id ? (
          <button
            type="button"
            className="danger"
            onClick={() => {
              if (draft.id && window.confirm(`删除 MCP「${draft.name}」？`)) {
                void remove.mutateAsync(draft.id);
              }
            }}
          >
            删除
          </button>
        ) : null}
      </div>

      {tools !== null && (
        <div
          style={{
            marginTop: "16px",
            borderTop: "1px solid var(--line)",
            paddingTop: "14px",
          }}
        >
          <h4>🛠️ 发现工具列表 ({tools.length})</h4>
          {tools.length === 0 ? (
            <p className="muted">此服务未返回可用工具。</p>
          ) : (
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                gap: "8px",
                marginTop: "10px",
              }}
            >
              {tools.map((tool) => {
                const currentLevel: ToolPermissionLevel =
                  permissions[tool.name] ?? "confirm";
                return (
                  <div
                    key={tool.name}
                    style={{
                      padding: "10px 12px",
                      background: "var(--sunken)",
                      border: "1px solid var(--line)",
                      borderRadius: "6px",
                      display: "flex",
                      justifyContent: "space-between",
                      alignItems: "center",
                      gap: "10px",
                    }}
                  >
                    <div>
                      <div style={{ fontWeight: 600, fontSize: "13px" }}>
                        {tool.name}
                      </div>
                      <div
                        className="muted"
                        style={{ fontSize: "12px", marginTop: "2px" }}
                      >
                        {tool.description || "无工具描述"}
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
                            fontSize: "11px",
                            padding: "3px 6px",
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
                            cursor: "pointer",
                          }}
                          disabled={updateToolPerm.isPending}
                          onClick={() =>
                            void updateToolPerm.mutateAsync({
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
          )}
        </div>
      )}
    </section>
  );
}

function blank(): Draft {
  return {
    id: null,
    name: "",
    transport: "stdio",
    command: "",
    argsText: "",
    url: "",
    enabled: true,
  };
}

function fromServer(server: McpServer): Draft {
  return {
    id: server.id,
    name: server.name,
    transport: server.transport,
    command: server.command,
    argsText: server.args.join("\n"),
    url: server.url,
    enabled: server.enabled,
  };
}
