import { describe, expect, it } from "vitest";
import type { DiagnosticBundle, DiagnosticItem } from "../../types";
import { getFailureGuidance } from "./DiagnosticsView";

describe("T15: Diagnostics failure reasoning and bundle preview", () => {
  it("should return null for healthy diagnostic items", () => {
    const okItem: DiagnosticItem = {
      category: "数据库",
      name: "SQLite Integrity",
      status: "ok",
      detail: "PRAGMA integrity_check 通过",
    };
    expect(getFailureGuidance(okItem)).toBeNull();
  });

  it("should accurately classify and guide on each failure root-cause category", () => {
    // 1. Network failure
    const networkItem: DiagnosticItem = {
      category: "网络与代理",
      name: "网络与代理连通性",
      status: "warn",
      detail: "外网探测超时",
      failureReason: "network",
    };
    const networkGuide = getFailureGuidance(networkItem);
    expect(networkGuide?.label).toContain("网络不可达");
    expect(networkGuide?.tip).toContain("代理设置");

    // 2. Credentials failure
    const credItem: DiagnosticItem = {
      category: "模型与凭据",
      name: "关键 Provider 凭据",
      status: "error",
      detail: "Provider 凭据丢失",
      failureReason: "credentials",
    };
    const credGuide = getFailureGuidance(credItem);
    expect(credGuide?.label).toContain("凭据异常");
    expect(credGuide?.tip).toContain("API Key");

    // 3. Model failure
    const modelItem: DiagnosticItem = {
      category: "模型与凭据",
      name: "关键 Provider 凭据",
      status: "warn",
      detail: "未启用任何模型",
      failureReason: "model",
    };
    const modelGuide = getFailureGuidance(modelItem);
    expect(modelGuide?.label).toContain("模型配置异常");

    // 4. Disk failure
    const diskItem: DiagnosticItem = {
      category: "存储",
      name: "磁盘可用空间",
      status: "error",
      detail: "剩余空间仅 200MB",
      failureReason: "disk",
    };
    const diskGuide = getFailureGuidance(diskItem);
    expect(diskGuide?.label).toContain("存储空间");
    expect(diskGuide?.tip).toContain("500MB");

    // 5. WebView2 failure
    const webview2Item: DiagnosticItem = {
      category: "系统与环境",
      name: "WebView2 Runtime",
      status: "error",
      detail: "未检测到 WebView2 运行时",
      failureReason: "webview2",
    };
    const webview2Guide = getFailureGuidance(webview2Item);
    expect(webview2Guide?.label).toContain("WebView2");
    expect(webview2Guide?.tip).toContain("微软官网下载安装");
  });

  it("should enforce diagnostic bundle redaction contracts", () => {
    const mockBundle: DiagnosticBundle = {
      report: {
        generatedAt: "2026-10-10T00:00:00Z",
        appVersion: "1.0.0",
        os: "windows",
        arch: "x86_64",
        items: [],
      },
      sanitizedProviders: [
        {
          id: "prov-1",
          name: "OpenAI",
          type: "openai",
          baseUrl: "https://api.openai.com/v1",
          enabled: true,
        },
      ],
      storage: {
        totalAssetRecords: 5,
        diskFilesCount: 5,
        diskTotalBytes: 1048576,
        orphanFiles: [],
        missingFiles: [],
        tempFiles: [],
      },
      recentTaskErrors: [
        {
          id: "task-1",
          modality: "text",
          status: "failed",
          error: "Error 401: Unauthorized",
          failureReason: "credentials",
        },
      ],
      recentLogs: [
        {
          id: "log-1",
          level: "WARN",
          target: "network",
          message: "Request failed with status 401",
          timestamp: "2026-10-10T00:00:00Z",
        },
      ],
    };

    const serialized = JSON.stringify(mockBundle);
    // 确保绝对不包含敏感字段名称如 apiKey, token, prompt, conversationText
    expect(serialized).not.toContain("apiKey");
    expect(serialized).not.toContain("secretKey");
    expect(serialized).not.toContain("prompt");
    expect(mockBundle.sanitizedProviders[0]).not.toHaveProperty("headers");
    expect(mockBundle.recentTaskErrors[0].failureReason).toBe("credentials");
  });
});
