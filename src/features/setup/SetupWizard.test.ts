import { describe, expect, it } from "vitest";
import { PROVIDER_DEFAULTS } from "../../types";
import { diagnoseConnectionError } from "./SetupWizardModal";

describe("T11 Setup Wizard Diagnostic and Flow Verification", () => {
  describe("Connection Error Diagnosis & Sanitization Gate", () => {
    it("should classify 401 unauthorized errors with actionable API key guidance", () => {
      const err = new Error(
        "Request failed with status code 401: Invalid API Key provided",
      );
      const diagnosis = diagnoseConnectionError(err, false);
      expect(diagnosis).toContain("API 凭据无效或已过期");
      expect(diagnosis).toContain("前后未包含空格");
      // 绝不包含敏感字样
      expect(diagnosis).not.toContain("Bearer");
    });

    it("should classify local service ECONNREFUSED errors with background process guidance", () => {
      const err = new Error(
        "Connection refused (os error 10061): 127.0.0.1:11434",
      );
      const diagnosis = diagnoseConnectionError(err, true);
      expect(diagnosis).toContain("未能连接到本地服务端口");
      expect(diagnosis).toContain("ollama serve");
    });

    it("should classify remote service network connection failure", () => {
      const err = new Error("failed to connect to server: api.openai.com");
      const diagnosis = diagnoseConnectionError(err, false);
      expect(diagnosis).toContain("无法连接到目标服务地址");
      expect(diagnosis).toContain("Base URL");
    });

    it("should classify 429 quota exhaustion errors with balance guidance", () => {
      const err = "429 Too Many Requests: Insufficient quota";
      const diagnosis = diagnoseConnectionError(err, false);
      expect(diagnosis).toContain("余额不足或超出频率限制");
      expect(diagnosis).toContain("账户额度与充值状态");
    });

    it("should classify network timeout with proxy configuration advice", () => {
      const err = new Error("client request timeout after 15000ms");
      const diagnosis = diagnoseConnectionError(err, false);
      expect(diagnosis).toContain("请求连接超时");
      expect(diagnosis).toContain("网络代理");
    });
  });

  describe("Provider Defaults and Local Boundary", () => {
    it("should have correct local default endpoints for offline services", () => {
      expect(PROVIDER_DEFAULTS.ollama).toBe("http://127.0.0.1:11434");
      expect(PROVIDER_DEFAULTS.comfyui).toBe("http://127.0.0.1:8188");
      expect(PROVIDER_DEFAULTS.openai).toBe("https://api.openai.com/v1");
    });
  });
});
