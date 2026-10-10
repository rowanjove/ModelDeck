import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, asAppError } from "../../api";
import { ErrorNote } from "../../components/ErrorNote";
import type { AppError, ExportedConfig } from "../../types";

export function ConfigIoView() {
  const queryClient = useQueryClient();
  const [error, setError] = useState<AppError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [importJsonText, setImportJsonText] = useState("");
  const [exportJsonPreview, setExportJsonPreview] = useState("");

  const exportMutation = useMutation({
    mutationFn: api.configExportAll,
    onSuccess: (data) => {
      const text = JSON.stringify(data, null, 2);
      setExportJsonPreview(text);
      setNotice("配置导出成功！已加载到预览区，可直接复制或下载。");
      setError(null);
    },
    onError: (err) => {
      setError(asAppError(err));
    },
  });

  const importMutation = useMutation({
    mutationFn: (config: ExportedConfig) => api.configImportAll(config),
    onSuccess: async (res) => {
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      await queryClient.invalidateQueries({ queryKey: ["models"] });
      await queryClient.invalidateQueries({ queryKey: ["presets"] });
      setNotice(
        `配置导入成功！共导入 ${res.providersImported} 个 Provider，${res.modelsImported} 个模型，${res.presetsImported} 个参数预设。`,
      );
      setImportJsonText("");
      setError(null);
    },
    onError: (err) => {
      setError(asAppError(err));
    },
  });

  async function handleCopy(text: string) {
    await navigator.clipboard.writeText(text);
    setNotice("已复制到剪贴板！");
  }

  function handleDownload(text: string) {
    const blob = new Blob([text], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `ai-workbench-config-${new Date().toISOString().slice(0, 10)}.json`;
    a.click();
    URL.revokeObjectURL(url);
    setNotice("配置文件已下载！");
  }

  function handleExecuteImport() {
    if (!importJsonText.trim()) {
      return;
    }
    setError(null);
    try {
      const parsed = JSON.parse(importJsonText) as ExportedConfig;
      if (!parsed.providers || !parsed.models) {
        throw new Error("无效的配置文件格式：缺少 providers 或 models 字段");
      }
      importMutation.mutate(parsed);
    } catch (e) {
      setError(asAppError(e));
    }
  }

  function handleFileUpload(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) {
      return;
    }
    const reader = new FileReader();
    reader.onload = (event) => {
      const content = event.target?.result as string;
      if (content) {
        setImportJsonText(content);
        setNotice(`已加载文件 ${file.name}，请点击下方「执行导入」完成恢复。`);
      }
    };
    reader.readAsText(file);
  }

  return (
    <div style={{ maxWidth: "800px" }}>
      <div style={{ marginBottom: "20px" }}>
        <h2 style={{ margin: "0 0 4px", fontSize: "16px" }}>
          配置导出与导入 (Config Import / Export)
        </h2>
        <p className="muted" style={{ margin: 0, fontSize: "13px" }}>
          备份或迁移您的自定义 Provider、模型架构及参数预设。所有敏感凭据（如
          API Key）在导出时均会自动剔除脱敏。
        </p>
      </div>

      {notice && (
        <div
          style={{
            padding: "10px 14px",
            background: "rgba(34, 197, 94, 0.1)",
            border: "1px solid var(--ok)",
            borderRadius: "6px",
            color: "var(--ok)",
            marginBottom: "16px",
            fontSize: "13px",
          }}
        >
          {notice}
        </div>
      )}

      {error && <ErrorNote error={error} />}

      <div style={{ display: "grid", gap: "20px" }}>
        {/* 1. Export Block */}
        <div
          style={{
            background: "var(--raised)",
            border: "1px solid var(--line)",
            borderRadius: "8px",
            padding: "20px",
          }}
        >
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
              marginBottom: "12px",
            }}
          >
            <div>
              <strong style={{ fontSize: "15px" }}>导出全局配置</strong>
              <p
                className="muted"
                style={{ margin: "2px 0 0", fontSize: "12.5px" }}
              >
                导出当前数据库中所有 Provider、Model Definition 与 Presets 为
                JSON。
              </p>
            </div>
            <button
              type="button"
              className="primary"
              disabled={exportMutation.isPending}
              onClick={() => exportMutation.mutate()}
            >
              {exportMutation.isPending ? "正在导出…" : "生成导出配置 "}
            </button>
          </div>

          {exportJsonPreview && (
            <div style={{ marginTop: "14px" }}>
              <div
                style={{
                  display: "flex",
                  gap: "10px",
                  justifyContent: "flex-end",
                  marginBottom: "8px",
                }}
              >
                <button
                  type="button"
                  className="linkish"
                  onClick={() => void handleCopy(exportJsonPreview)}
                >
                  复制 JSON
                </button>
                <button
                  type="button"
                  onClick={() => handleDownload(exportJsonPreview)}
                >
                  下载 .json 文件 ⬇
                </button>
              </div>
              <textarea
                rows={8}
                readOnly
                value={exportJsonPreview}
                style={{ fontFamily: "monospace", fontSize: "12px" }}
              />
            </div>
          )}
        </div>

        {/* 2. Import Block */}
        <div
          style={{
            background: "var(--raised)",
            border: "1px solid var(--line)",
            borderRadius: "8px",
            padding: "20px",
          }}
        >
          <div style={{ marginBottom: "12px" }}>
            <strong style={{ fontSize: "15px" }}>导入与恢复配置</strong>
            <p
              className="muted"
              style={{ margin: "2px 0 0", fontSize: "12.5px" }}
            >
              粘贴配置 JSON 内容或上传 `.json` 文件以增量恢复 Provider
              与模型定义。
            </p>
          </div>

          <div style={{ marginBottom: "12px" }}>
            <input
              type="file"
              accept=".json"
              onChange={handleFileUpload}
              style={{ fontSize: "13px" }}
            />
          </div>

          <label className="field" style={{ marginBottom: "14px" }}>
            <span>配置 JSON 内容</span>
            <textarea
              rows={6}
              value={importJsonText}
              onChange={(e) => setImportJsonText(e.target.value)}
              placeholder="请粘贴导出的 JSON 配置文件内容..."
              style={{ fontFamily: "monospace", fontSize: "12px" }}
            />
          </label>

          <button
            type="button"
            className="primary"
            disabled={!importJsonText.trim() || importMutation.isPending}
            onClick={handleExecuteImport}
          >
            {importMutation.isPending ? "正在导入…" : "执行导入恢复 "}
          </button>
        </div>
      </div>
    </div>
  );
}
