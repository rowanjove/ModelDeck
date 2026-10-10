import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../../api";
import type {
  CleanReport,
  DiagnosticBundle,
  DiagnosticItem,
} from "../../types";

export function getFailureGuidance(
  item: DiagnosticItem,
): { label: string; tip: string; color: string } | null {
  if (item.status === "ok") return null;

  switch (item.failureReason) {
    case "network":
      return {
        label: "网络不可达 (Network Error)",
        tip: "请检查代理设置、本地网络连接或系统防火墙；若仅使用本地离线模型可忽略。",
        color: "#38bdf8",
      };
    case "credentials":
      return {
        label: "凭据异常 (Credentials Error)",
        tip: "请前往【设置 -> Provider】检查对应服务商的 API Key，或重新绑定安全凭据。",
        color: "#fb923c",
      };
    case "model":
      return {
        label: "模型配置异常 (Model Error)",
        tip: "当前未配置可用模型或 Provider 接入点未启用，请在【设置 -> 模型】启用模型。",
        color: "#c084fc",
      };
    case "disk":
      return {
        label: "存储空间或权限受限 (Disk Error)",
        tip: "磁盘剩余低于安全阈值 500MB 或目录无写入权限，建议清理碎片或释放磁盘空间。",
        color: "#f87171",
      };
    case "webview2":
      return {
        label: "WebView2 运行时缺失 (WebView2 Missing)",
        tip: "未检测到 Microsoft Edge WebView2 运行时，GUI 界面可能渲染异常，请前往微软官网下载安装。",
        color: "#f43f5e",
      };
    default:
      return {
        label: "运行告警 (Warning)",
        tip: item.detail,
        color: "#fbbf24",
      };
  }
}

export function DiagnosticsView() {
  const queryClient = useQueryClient();
  const [notice, setNotice] = useState<string | null>(null);
  const [previewBundle, setPreviewBundle] = useState<DiagnosticBundle | null>(
    null,
  );
  const [copied, setCopied] = useState(false);

  const diagQuery = useQuery({
    queryKey: ["system_diagnostics"],
    queryFn: () => api.diagnosticsRun(),
  });

  const storageQuery = useQuery({
    queryKey: ["storage_report"],
    queryFn: () => api.storageInspect(),
  });

  const cleanMutation = useMutation({
    mutationFn: (files: string[]) => api.storageClean(files),
    onSuccess: (res: CleanReport) => {
      setNotice(
        `清理完成: 共删除 ${res.cleanedFilesCount} 个文件，释放 ${(res.freedBytes / (1024 * 1024)).toFixed(2)} MB 空间`,
      );
      queryClient.invalidateQueries({ queryKey: ["storage_report"] });
      queryClient.invalidateQueries({ queryKey: ["system_diagnostics"] });
      setTimeout(() => setNotice(null), 4000);
    },
  });

  const exportBundleMutation = useMutation({
    mutationFn: () => api.diagnosticsExportBundle(),
    onSuccess: (bundle: DiagnosticBundle) => {
      setPreviewBundle(bundle);
    },
  });

  const handleDownloadBundle = (bundle: DiagnosticBundle) => {
    const blob = new Blob([JSON.stringify(bundle, null, 2)], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `ai-workbench-diagnostic-${new Date().toISOString().slice(0, 10)}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    setNotice("脱敏诊断包已成功下载！");
    setTimeout(() => setNotice(null), 3000);
  };

  const handleCopyBundle = async (bundle: DiagnosticBundle) => {
    try {
      await navigator.clipboard.writeText(JSON.stringify(bundle, null, 2));
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setNotice("复制到剪贴板失败，请手动下载文件");
    }
  };

  const handleCleanAllTrash = () => {
    const report = storageQuery.data;
    if (!report) return;
    const allTrash = [...report.tempFiles, ...report.orphanFiles];
    if (allTrash.length === 0) {
      setNotice("没有可清理的临时文件或孤儿资产");
      setTimeout(() => setNotice(null), 3000);
      return;
    }
    if (
      window.confirm(
        `确定要清理 ${allTrash.length} 个废弃文件吗？(包含 ${report.tempFiles.length} 个临时碎片和 ${report.orphanFiles.length} 个无引用孤儿文件)`,
      )
    ) {
      cleanMutation.mutate(allTrash);
    }
  };

  return (
    <div style={{ maxWidth: "880px", margin: "0 auto", paddingBottom: "40px" }}>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: "20px",
        }}
      >
        <div>
          <h2 style={{ margin: 0, fontSize: "18px" }}>系统健康度与诊断中心</h2>
          <p
            style={{
              margin: "4px 0 0",
              fontSize: "13px",
              color: "var(--muted)",
            }}
          >
            一键自检：SQLite 完整性、磁盘空间、WebView2 运行时、网络代理通道与
            Provider 凭据可用性
          </p>
        </div>
        <div style={{ display: "flex", gap: "10px" }}>
          <button
            type="button"
            className="secondary-btn"
            onClick={() => {
              diagQuery.refetch();
              storageQuery.refetch();
            }}
            disabled={diagQuery.isFetching || storageQuery.isFetching}
            style={{ padding: "6px 14px", fontSize: "13px" }}
          >
            {diagQuery.isFetching ? "自检中..." : "重新自检"}
          </button>
          <button
            type="button"
            className="primary-btn"
            onClick={() => exportBundleMutation.mutate()}
            disabled={exportBundleMutation.isPending}
            style={{ padding: "6px 14px", fontSize: "13px" }}
          >
            {exportBundleMutation.isPending ? "生成中..." : "导出脱敏诊断包"}
          </button>
        </div>
      </div>

      {notice && (
        <div
          style={{
            padding: "10px 14px",
            marginBottom: "16px",
            borderRadius: "6px",
            background: "rgba(52, 211, 153, 0.12)",
            border: "1px solid rgba(52, 211, 153, 0.3)",
            color: "var(--ok, #34d399)",
            fontSize: "13px",
          }}
        >
          {notice}
        </div>
      )}

      {/* 1. 存储健康度分析 */}
      <section
        style={{
          background: "var(--surface-1, #1a1d24)",
          border: "1px solid var(--border, #2d3139)",
          borderRadius: "8px",
          padding: "18px",
          marginBottom: "20px",
        }}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: "14px",
          }}
        >
          <h3 style={{ margin: 0, fontSize: "15px", fontWeight: 600 }}>
            存储与媒体文件资产健康度 (Asset GC)
          </h3>
          {storageQuery.data &&
            (storageQuery.data.tempFiles.length > 0 ||
              storageQuery.data.orphanFiles.length > 0) && (
              <button
                type="button"
                className="secondary-btn"
                onClick={handleCleanAllTrash}
                disabled={cleanMutation.isPending}
                style={{
                  padding: "4px 10px",
                  fontSize: "12px",
                  color: "#f87171",
                  borderColor: "rgba(248, 113, 113, 0.3)",
                }}
              >
                {cleanMutation.isPending ? "清理中..." : "一键清理无用碎片"}
              </button>
            )}
        </div>

        {storageQuery.isLoading && (
          <p style={{ fontSize: "13px", color: "var(--muted)" }}>
            正在分析磁盘存储...
          </p>
        )}

        {storageQuery.data && (
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(4, 1fr)",
              gap: "12px",
            }}
          >
            <div
              style={{
                padding: "12px",
                background: "var(--surface-2, #212631)",
                borderRadius: "6px",
                border: "1px solid var(--border, #2d3139)",
              }}
            >
              <div style={{ fontSize: "12px", color: "var(--muted)" }}>
                登记资产数
              </div>
              <div
                style={{ fontSize: "20px", fontWeight: 600, marginTop: "4px" }}
              >
                {storageQuery.data.totalAssetRecords}
              </div>
            </div>

            <div
              style={{
                padding: "12px",
                background: "var(--surface-2, #212631)",
                borderRadius: "6px",
                border: "1px solid var(--border, #2d3139)",
              }}
            >
              <div style={{ fontSize: "12px", color: "var(--muted)" }}>
                磁盘文件占用
              </div>
              <div
                style={{ fontSize: "20px", fontWeight: 600, marginTop: "4px" }}
              >
                {(storageQuery.data.diskTotalBytes / (1024 * 1024)).toFixed(1)}{" "}
                MB
              </div>
            </div>

            <div
              style={{
                padding: "12px",
                background: "var(--surface-2, #212631)",
                borderRadius: "6px",
                border: "1px solid var(--border, #2d3139)",
              }}
            >
              <div style={{ fontSize: "12px", color: "var(--muted)" }}>
                孤儿文件
              </div>
              <div
                style={{
                  fontSize: "20px",
                  fontWeight: 600,
                  marginTop: "4px",
                  color:
                    storageQuery.data.orphanFiles.length > 0
                      ? "#fbbf24"
                      : "var(--fg)",
                }}
              >
                {storageQuery.data.orphanFiles.length}
              </div>
            </div>

            <div
              style={{
                padding: "12px",
                background: "var(--surface-2, #212631)",
                borderRadius: "6px",
                border: "1px solid var(--border, #2d3139)",
              }}
            >
              <div style={{ fontSize: "12px", color: "var(--muted)" }}>
                临时碎片文件
              </div>
              <div
                style={{
                  fontSize: "20px",
                  fontWeight: 600,
                  marginTop: "4px",
                  color:
                    storageQuery.data.tempFiles.length > 0
                      ? "#f87171"
                      : "var(--fg)",
                }}
              >
                {storageQuery.data.tempFiles.length}
              </div>
            </div>
          </div>
        )}
      </section>

      {/* 2. 诊断中心自检清单 */}
      <section
        style={{
          background: "var(--surface-1, #1a1d24)",
          border: "1px solid var(--border, #2d3139)",
          borderRadius: "8px",
          padding: "18px",
        }}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: "14px",
          }}
        >
          <h3 style={{ margin: 0, fontSize: "15px", fontWeight: 600 }}>
            系统诊断检查清单 (System Health Checks)
          </h3>
          {diagQuery.data && (
            <span style={{ fontSize: "12px", color: "var(--muted)" }}>
              平台: {diagQuery.data.os} ({diagQuery.data.arch}) · 版本: v
              {diagQuery.data.appVersion}
            </span>
          )}
        </div>

        {diagQuery.isLoading && (
          <p style={{ fontSize: "13px", color: "var(--muted)" }}>
            正在执行全面系统诊断...
          </p>
        )}

        {diagQuery.data && (
          <div
            style={{ display: "flex", flexDirection: "column", gap: "10px" }}
          >
            {diagQuery.data.items.map((item, idx) => {
              const badgeBg =
                item.status === "ok"
                  ? "rgba(52, 211, 153, 0.15)"
                  : item.status === "warn"
                    ? "rgba(251, 191, 36, 0.15)"
                    : "rgba(248, 113, 113, 0.15)";
              const badgeColor =
                item.status === "ok"
                  ? "#34d399"
                  : item.status === "warn"
                    ? "#fbbf24"
                    : "#f87171";

              const guidance = getFailureGuidance(item);

              return (
                <div
                  key={idx}
                  style={{
                    display: "flex",
                    flexDirection: "column",
                    gap: "6px",
                    padding: "12px 14px",
                    background: "var(--surface-2, #212631)",
                    border: "1px solid var(--border, #2d3139)",
                    borderRadius: "6px",
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      justifyContent: "space-between",
                      alignItems: "center",
                    }}
                  >
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: "10px",
                      }}
                    >
                      <span
                        style={{
                          padding: "2px 8px",
                          fontSize: "11px",
                          borderRadius: "4px",
                          background: "rgba(255,255,255,0.06)",
                          color: "var(--muted)",
                        }}
                      >
                        {item.category}
                      </span>
                      <span style={{ fontSize: "13.5px", fontWeight: 600 }}>
                        {item.name}
                      </span>
                    </div>

                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: "12px",
                      }}
                    >
                      <span style={{ fontSize: "12px", color: "var(--muted)" }}>
                        {item.detail}
                      </span>
                      <span
                        style={{
                          padding: "2px 8px",
                          fontSize: "11px",
                          fontWeight: 600,
                          borderRadius: "4px",
                          background: badgeBg,
                          color: badgeColor,
                          textTransform: "uppercase",
                        }}
                      >
                        {item.status}
                      </span>
                    </div>
                  </div>

                  {guidance && (
                    <div
                      style={{
                        marginTop: "4px",
                        padding: "6px 10px",
                        background: "rgba(0, 0, 0, 0.2)",
                        borderLeft: `3px solid ${guidance.color}`,
                        borderRadius: "2px 4px 4px 2px",
                        fontSize: "12px",
                        display: "flex",
                        alignItems: "center",
                        gap: "8px",
                      }}
                    >
                      <span style={{ fontWeight: 600, color: guidance.color }}>
                        {guidance.label}:
                      </span>
                      <span style={{ color: "var(--muted)" }}>
                        {guidance.tip}
                      </span>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </section>

      {/* 3. 脱敏诊断包导出前预览 Modal */}
      {previewBundle && (
        <div
          style={{
            position: "fixed",
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            background: "rgba(0, 0, 0, 0.7)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 1000,
            padding: "20px",
          }}
        >
          <div
            style={{
              background: "var(--raised, #1a1d24)",
              border: "1px solid var(--line, #2d3139)",
              borderRadius: "10px",
              width: "100%",
              maxWidth: "760px",
              maxHeight: "90vh",
              display: "flex",
              flexDirection: "column",
              overflow: "hidden",
              boxShadow: "0 20px 40px rgba(0,0,0,0.4)",
            }}
          >
            {/* Modal Header */}
            <div
              style={{
                padding: "16px 20px",
                borderBottom: "1px solid var(--line, #2d3139)",
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
              }}
            >
              <div>
                <h3 style={{ margin: 0, fontSize: "16px", fontWeight: 600 }}>
                  脱敏诊断包导出预览 (Diagnostic Bundle Preview)
                </h3>
                <p
                  style={{
                    margin: "2px 0 0",
                    fontSize: "12px",
                    color: "var(--muted)",
                  }}
                >
                  请在下载或复制前确认将导出的字段与文件内容，确保无任何密钥与隐私泄漏
                </p>
              </div>
              <button
                type="button"
                className="secondary-btn"
                onClick={() => setPreviewBundle(null)}
                style={{ padding: "4px 10px", fontSize: "12px" }}
              >
                ✕ 关闭
              </button>
            </div>

            {/* Modal Content */}
            <div
              style={{
                padding: "18px 20px",
                overflowY: "auto",
                display: "flex",
                flexDirection: "column",
                gap: "14px",
              }}
            >
              {/* 安全脱敏保证卡片 */}
              <div
                style={{
                  padding: "12px 14px",
                  borderRadius: "6px",
                  background: "rgba(34, 197, 94, 0.1)",
                  border: "1px solid var(--ok, #22c55e)",
                  fontSize: "12.5px",
                  color: "var(--fg)",
                  lineHeight: "1.5",
                }}
              >
                <div
                  style={{
                    fontWeight: 600,
                    color: "var(--ok, #22c55e)",
                    marginBottom: "4px",
                  }}
                >
                  安全脱敏防护已生效 (Strict Redaction Verified)
                </div>
                <div>
                  • <strong>绝不包含密钥</strong>：所有已配置 API Key、Token
                  授权及请求头均已被彻底排除或掩码脱敏。
                </div>
                <div>
                  • <strong>绝不包含对话素材</strong>：已剔除所有用户
                  Prompt、对话正文、图片/视频/音频原始媒体二进制数据。
                </div>
                <div>
                  • <strong>透明可控</strong>：仅导出软件环境、体检结果、脱敏
                  Provider 端点及脱敏错误日志。
                </div>
              </div>

              {/* 字段与结构清单 */}
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "repeat(3, 1fr)",
                  gap: "10px",
                }}
              >
                <div
                  style={{
                    padding: "10px",
                    background: "var(--surface-2, #212631)",
                    borderRadius: "6px",
                    border: "1px solid var(--line, #2d3139)",
                  }}
                >
                  <div style={{ fontSize: "11px", color: "var(--muted)" }}>
                    系统体检指标
                  </div>
                  <div
                    style={{
                      fontSize: "15px",
                      fontWeight: 600,
                      marginTop: "2px",
                    }}
                  >
                    {previewBundle.report.items.length} 项自检
                  </div>
                </div>

                <div
                  style={{
                    padding: "10px",
                    background: "var(--surface-2, #212631)",
                    borderRadius: "6px",
                    border: "1px solid var(--line, #2d3139)",
                  }}
                >
                  <div style={{ fontSize: "11px", color: "var(--muted)" }}>
                    脱敏 Provider
                  </div>
                  <div
                    style={{
                      fontSize: "15px",
                      fontWeight: 600,
                      marginTop: "2px",
                    }}
                  >
                    {previewBundle.sanitizedProviders.length} 个端点 (无 Key)
                  </div>
                </div>

                <div
                  style={{
                    padding: "10px",
                    background: "var(--surface-2, #212631)",
                    borderRadius: "6px",
                    border: "1px solid var(--line, #2d3139)",
                  }}
                >
                  <div style={{ fontSize: "11px", color: "var(--muted)" }}>
                    脱敏错误与日志
                  </div>
                  <div
                    style={{
                      fontSize: "15px",
                      fontWeight: 600,
                      marginTop: "2px",
                    }}
                  >
                    {previewBundle.recentTaskErrors.length} 条错误 /{" "}
                    {previewBundle.recentLogs?.length || 0} 行日志
                  </div>
                </div>
              </div>

              {/* JSON 文本预览 */}
              <div>
                <div
                  style={{
                    fontSize: "12px",
                    fontWeight: 600,
                    marginBottom: "6px",
                    color: "var(--muted)",
                  }}
                >
                  JSON 导出版原始预览：
                </div>
                <pre
                  style={{
                    margin: 0,
                    padding: "12px",
                    background: "var(--bg, #0f1117)",
                    border: "1px solid var(--line, #2d3139)",
                    borderRadius: "6px",
                    maxHeight: "240px",
                    overflow: "auto",
                    fontSize: "11.5px",
                    lineHeight: "1.4",
                    color: "var(--fg, #e2e8f0)",
                    fontFamily: "monospace",
                  }}
                >
                  {JSON.stringify(previewBundle, null, 2)}
                </pre>
              </div>
            </div>

            {/* Modal Footer */}
            <div
              style={{
                padding: "14px 20px",
                borderTop: "1px solid var(--line, #2d3139)",
                display: "flex",
                justifyContent: "flex-end",
                gap: "10px",
                background: "var(--surface-1, #1a1d24)",
              }}
            >
              <button
                type="button"
                className="secondary-btn"
                onClick={() => handleCopyBundle(previewBundle)}
                style={{ padding: "6px 14px", fontSize: "13px" }}
              >
                {copied ? "已复制到剪贴板！" : "复制脱敏 JSON"}
              </button>
              <button
                type="button"
                className="primary-btn"
                onClick={() => handleDownloadBundle(previewBundle)}
                style={{ padding: "6px 16px", fontSize: "13px" }}
              >
                下载诊断文件 (.json)
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
