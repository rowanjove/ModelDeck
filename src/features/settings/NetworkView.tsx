import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, asAppError } from "../../api";
import { ErrorNote } from "../../components/ErrorNote";
import type { AppError, ProxySettings } from "../../types";

export function NetworkView() {
  const queryClient = useQueryClient();
  const [error, setError] = useState<AppError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [mode, setMode] = useState<"system" | "none" | "http" | "socks5">(
    "system",
  );
  const [httpProxy, setHttpProxy] = useState("");
  const [socks5Proxy, setSocks5Proxy] = useState("");
  const [testResult, setTestResult] = useState<string | null>(null);

  const proxyQuery = useQuery({
    queryKey: ["network-proxy"],
    queryFn: api.networkGetProxy,
  });

  useEffect(() => {
    if (proxyQuery.data) {
      setMode(proxyQuery.data.mode as "system" | "none" | "http" | "socks5");
      setHttpProxy(proxyQuery.data.httpProxy ?? "");
      setSocks5Proxy(proxyQuery.data.socks5Proxy ?? "");
    }
  }, [proxyQuery.data]);

  const saveMutation = useMutation({
    mutationFn: (settings: ProxySettings) => api.networkSetProxy(settings),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["network-proxy"] });
      setNotice("代理设置已保存！");
      setError(null);
    },
    onError: (err) => {
      setError(asAppError(err));
    },
  });

  const testMutation = useMutation({
    mutationFn: (settings: ProxySettings) => api.networkTestProxy(settings),
    onSuccess: (latencyMs) => {
      setTestResult(`连接成功！代理往返耗时: ${latencyMs} ms`);
      setError(null);
    },
    onError: (err) => {
      setTestResult(null);
      setError(asAppError(err));
    },
  });

  function getDraftSettings(): ProxySettings {
    return {
      mode,
      httpProxy: mode === "http" ? httpProxy.trim() || null : null,
      socks5Proxy: mode === "socks5" ? socks5Proxy.trim() || null : null,
      updatedAt: "",
    };
  }

  function handleSave() {
    setNotice(null);
    setTestResult(null);
    saveMutation.mutate(getDraftSettings());
  }

  function handleTest() {
    setNotice(null);
    setTestResult(null);
    testMutation.mutate(getDraftSettings());
  }

  return (
    <div style={{ maxWidth: "700px" }}>
      <div style={{ marginBottom: "20px" }}>
        <h2 style={{ margin: "0 0 4px", fontSize: "16px" }}>
          网络代理设置 (Network / Proxy)
        </h2>
        <p className="muted" style={{ margin: 0, fontSize: "13px" }}>
          配置应用发起外部 API 请求时的网络代理通道，支持系统默认、直连、HTTP 与
          SOCKS5 代理。
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

      {testResult && (
        <div
          style={{
            padding: "10px 14px",
            background: "rgba(59, 130, 246, 0.1)",
            border: "1px solid var(--accent)",
            borderRadius: "6px",
            color: "var(--accent)",
            marginBottom: "16px",
            fontSize: "13px",
          }}
        >
          {testResult}
        </div>
      )}

      {error && <ErrorNote error={error} />}

      <div
        style={{
          background: "var(--raised)",
          border: "1px solid var(--line)",
          borderRadius: "8px",
          padding: "20px",
          display: "grid",
          gap: "16px",
        }}
      >
        <div>
          <div
            style={{ display: "block", marginBottom: "8px", fontWeight: 600 }}
          >
            代理模式 (Proxy Mode)
          </div>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(4, 1fr)",
              gap: "8px",
            }}
          >
            {[
              { id: "system", label: "系统代理 (默认)" },
              { id: "none", label: "直连 (无代理)" },
              { id: "http", label: "HTTP 代理" },
              { id: "socks5", label: "SOCKS5 代理" },
            ].map((item) => (
              <button
                key={item.id}
                type="button"
                className={mode === item.id ? "primary" : ""}
                onClick={() =>
                  setMode(item.id as "system" | "none" | "http" | "socks5")
                }
                style={{ fontSize: "12.5px", padding: "8px" }}
              >
                {item.label}
              </button>
            ))}
          </div>
        </div>

        {mode === "http" && (
          <label className="field">
            <span>HTTP 代理地址</span>
            <input
              value={httpProxy}
              onChange={(e) => setHttpProxy(e.target.value)}
              placeholder="例如: http://127.0.0.1:7890"
            />
          </label>
        )}

        {mode === "socks5" && (
          <label className="field">
            <span>SOCKS5 代理地址</span>
            <input
              value={socks5Proxy}
              onChange={(e) => setSocks5Proxy(e.target.value)}
              placeholder="例如: socks5://127.0.0.1:10808"
            />
          </label>
        )}

        <div style={{ display: "flex", gap: "10px", marginTop: "8px" }}>
          <button
            type="button"
            className="primary"
            disabled={saveMutation.isPending}
            onClick={handleSave}
          >
            {saveMutation.isPending ? "正在保存…" : "保存设置"}
          </button>
          <button
            type="button"
            disabled={testMutation.isPending}
            onClick={handleTest}
          >
            {testMutation.isPending ? "正在测试连通性…" : "测试代理连接"}
          </button>
        </div>
      </div>

      <div
        style={{
          marginTop: "16px",
          padding: "14px 16px",
          background: "var(--surface-2, rgba(255, 255, 255, 0.03))",
          border: "1px solid var(--line)",
          borderRadius: "8px",
          fontSize: "12.5px",
          color: "var(--muted)",
          lineHeight: "1.6",
        }}
      >
        <div
          style={{ fontWeight: 600, color: "var(--fg)", marginBottom: "6px" }}
        >
          代理切换生效边界说明 (Proxy Switching Lifecycle)
        </div>
        <div>
          • <strong>新请求即时生效</strong>
          ：保存设置后，应用全局网络客户端立即热更新，所有随后发起的新任务、端点校验与元数据请求将立刻使用新代理模式。
        </div>
        <div>
          • <strong>进行中任务不中断</strong>
          ：当前正在传输的流式对话、长轮询任务或媒体下载由现有连接持有，不保证即时强制切换通道。建议等待其完成或主动取消后重试。
        </div>
      </div>
    </div>
  );
}
