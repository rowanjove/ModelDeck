import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../../api";
import type { CleanReport, DiagnosticBundle } from "../../types";

export function DiagnosticsView() {
  const queryClient = useQueryClient();
  const [notice, setNotice] = useState<string | null>(null);

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
      setNotice("脱敏诊断包已成功导出 (已过滤所有密钥与个人数据)");
      setTimeout(() => setNotice(null), 3000);
    },
  });

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
          <h2 style={{ margin: 0, fontSize: "18px" }}>系统健康度与存储维护</h2>
          <p
            style={{
              margin: "4px 0 0",
              fontSize: "13px",
              color: "var(--muted)",
            }}
          >
            PRD & Hardening 标准：检查 SQLite
            完整性、磁盘空间防护、任务与存储健康度、并提供安全脱敏诊断导出
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
            {diagQuery.isFetching ? "诊断中..." : "重新诊断"}
          </button>
          <button
            type="button"
            className="primary-btn"
            onClick={() => exportBundleMutation.mutate()}
            disabled={exportBundleMutation.isPending}
            style={{ padding: "6px 14px", fontSize: "13px" }}
          >
            {exportBundleMutation.isPending ? "导出中..." : "导出脱敏诊断包"}
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
            color: "#34d399",
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

      {/* 2. 诊断中心指标 */}
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
            系统诊断检查清单 (System Diagnostics)
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
            正在执行完整系统诊断...
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

              return (
                <div
                  key={idx}
                  style={{
                    display: "flex",
                    justifyContent: "space-between",
                    alignItems: "center",
                    padding: "10px 14px",
                    background: "var(--surface-2, #212631)",
                    border: "1px solid var(--border, #2d3139)",
                    borderRadius: "6px",
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
                    <span style={{ fontSize: "13px", fontWeight: 500 }}>
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
              );
            })}
          </div>
        )}
      </section>
    </div>
  );
}
