import { useEffect, useState } from "react";
import { api } from "../../api";
import { DialogFrame } from "../../components/DialogFrame";
import type {
  HttpRecipe,
  PluginValidationReport,
  SdkTemplateInfo,
  TestAdapterResult,
} from "../../types";

export function PluginSdkModal({
  onClose,
  onInstalled,
}: {
  onClose: () => void;
  onInstalled: () => void;
}) {
  const [activeTab, setActiveTab] = useState<
    "validator" | "sandbox" | "template"
  >("validator");
  const [sdkInfo, setSdkInfo] = useState<SdkTemplateInfo | null>(null);

  // Validator State
  const [manifestJson, setManifestJson] = useState("");
  const [validationReport, setValidationReport] =
    useState<PluginValidationReport | null>(null);
  const [isValidating, setIsValidating] = useState(false);
  const [installStatus, setInstallStatus] = useState<string | null>(null);

  // Response Parser Sandbox State
  const testMethod = "POST";
  const testPath = "/v1/images/generations";
  const [resultPath, setResultPath] = useState("$.data[0].url");
  const [statusPath, setStatusPath] = useState("$.status");
  const [sampleResponse, setSampleResponse] = useState(
    JSON.stringify(
      {
        status: "SUCCESS",
        data: [{ url: "https://cdn.example.com/art_rendered_8k.png" }],
        usage: { prompt_tokens: 32 },
      },
      null,
      2,
    ),
  );
  const [testResult, setTestResult] = useState<TestAdapterResult | null>(null);
  const [isTestingRecipe, setIsTestingRecipe] = useState(false);

  useEffect(() => {
    void api.pluginsGetSdkTemplate().then((info) => {
      setSdkInfo(info);
      setManifestJson(info.manifestTemplate);
    });
  }, []);

  const handleValidate = async () => {
    setIsValidating(true);
    setInstallStatus(null);
    try {
      const report = await api.pluginsValidate(manifestJson);
      setValidationReport(report);
    } catch (e: unknown) {
      setValidationReport({
        valid: false,
        warnings: [],
        errors: [String(e)],
        securityPassed: false,
      });
    } finally {
      setIsValidating(false);
    }
  };

  const handleInstall = async () => {
    setInstallStatus(null);
    try {
      await api.pluginsInstall(manifestJson);
      setInstallStatus(" 插件安装成功！已添加至本地插件库。");
      onInstalled();
    } catch (e: unknown) {
      setInstallStatus(` 安装失败: ${String(e)}`);
    }
  };

  const handleTestRecipe = async () => {
    setIsTestingRecipe(true);
    try {
      const recipe: HttpRecipe = {
        method: testMethod,
        path: testPath,
        headers: [],
        resultPath,
        poll: statusPath.trim()
          ? {
              method: "GET",
              path: "/status",
              statusPath,
              success: "SUCCESS",
              failure: "FAILED",
            }
          : undefined,
      };
      const res = await api.pluginsTestRecipe(recipe, sampleResponse);
      setTestResult(res);
    } catch (e: unknown) {
      setTestResult({
        success: false,
        error: String(e),
      });
    } finally {
      setIsTestingRecipe(false);
    }
  };

  return (
    <DialogFrame label="Provider 插件" onClose={onClose}>
      <div
        style={{
          width: "820px",
          maxWidth: "95vw",
          height: "85vh",
          background: "var(--bg-card, #1e1e24)",
          borderRadius: "var(--radius-lg)",
          border: "1px solid var(--line, #333)",
          boxShadow: "var(--shadow-lg)",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        {/* Modal Header */}
        <div
          style={{
            padding: "16px 20px",
            borderBottom: "1px solid var(--line)",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            background: "var(--bg)",
          }}
        >
          <div>
            <h2 style={{ margin: 0, fontSize: "16px", fontWeight: 600 }}>
              Plugin / Adapter SDK 开发者套件
            </h2>
            <p
              style={{
                margin: "4px 0 0 0",
                fontSize: "12px",
                color: "var(--muted)",
              }}
            >
              声明式 Provider 清单接入、安全沙箱检验与响应解析测试沙盒
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            style={{
              background: "none",
              border: "none",
              fontSize: "20px",
              cursor: "pointer",
              color: "var(--muted)",
            }}
          >
            关闭
          </button>
        </div>

        {/* Tab Switcher */}
        <div
          style={{
            display: "flex",
            borderBottom: "1px solid var(--line)",
            background: "var(--bg)",
            padding: "0 16px",
          }}
        >
          <button
            type="button"
            onClick={() => setActiveTab("validator")}
            style={{
              padding: "10px 16px",
              border: "none",
              background: "none",
              borderBottom:
                activeTab === "validator"
                  ? "2px solid var(--accent)"
                  : "2px solid transparent",
              color:
                activeTab === "validator" ? "var(--accent)" : "var(--muted)",
              fontWeight: activeTab === "validator" ? 600 : 400,
              cursor: "pointer",
              fontSize: "13px",
            }}
          >
            插件清单与安全校验 (Validator)
          </button>
          <button
            type="button"
            onClick={() => setActiveTab("sandbox")}
            style={{
              padding: "10px 16px",
              border: "none",
              background: "none",
              borderBottom:
                activeTab === "sandbox"
                  ? "2px solid var(--accent)"
                  : "2px solid transparent",
              color: activeTab === "sandbox" ? "var(--accent)" : "var(--muted)",
              fontWeight: activeTab === "sandbox" ? 600 : 400,
              cursor: "pointer",
              fontSize: "13px",
            }}
          >
            适配器响应解析沙盒 (Parser Sandbox)
          </button>
          <button
            type="button"
            onClick={() => setActiveTab("template")}
            style={{
              padding: "10px 16px",
              border: "none",
              background: "none",
              borderBottom:
                activeTab === "template"
                  ? "2px solid var(--accent)"
                  : "2px solid transparent",
              color:
                activeTab === "template" ? "var(--accent)" : "var(--muted)",
              fontWeight: activeTab === "template" ? 600 : 400,
              cursor: "pointer",
              fontSize: "13px",
            }}
          >
            SDK 规范与模板 (Docs & Templates)
          </button>
        </div>

        {/* Tab Content */}
        <div
          style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "20px" }}
        >
          {/* TAB 1: Validator */}
          {activeTab === "validator" && (
            <div
              style={{ display: "flex", flexDirection: "column", gap: "16px" }}
            >
              <div style={{ fontSize: "12px", color: "var(--muted)" }}>
                在下方输入或粘贴第三方 Provider 清单
                JSON，点击「校验插件安全与格式」评估安全性。只有通过安全沙箱检查的插件才可安装。
              </div>

              <textarea
                rows={12}
                value={manifestJson}
                onChange={(e) => setManifestJson(e.target.value)}
                placeholder="粘贴插件清单 JSON..."
                style={{
                  width: "100%",
                  fontFamily: "monospace",
                  fontSize: "12px",
                  padding: "10px",
                  borderRadius: "6px",
                  border: "1px solid var(--line)",
                  background: "var(--bg)",
                  boxSizing: "border-box",
                }}
              />

              <div
                style={{ display: "flex", gap: "10px", alignItems: "center" }}
              >
                <button
                  type="button"
                  onClick={handleValidate}
                  disabled={isValidating}
                  style={{
                    padding: "8px 16px",
                    borderRadius: "6px",
                    border: "none",
                    background: "var(--accent)",
                    color: "var(--accent-ink)",
                    fontWeight: 600,
                    cursor: "pointer",
                    fontSize: "13px",
                  }}
                >
                  {isValidating ? "校验中..." : " 校验插件安全与格式"}
                </button>

                <button
                  type="button"
                  onClick={handleInstall}
                  disabled={
                    !validationReport?.valid ||
                    !validationReport?.securityPassed
                  }
                  style={{
                    padding: "8px 16px",
                    borderRadius: "6px",
                    border: "none",
                    background:
                      validationReport?.valid &&
                      validationReport?.securityPassed
                        ? "#10b981"
                        : "var(--line)",
                    color:
                      validationReport?.valid &&
                      validationReport?.securityPassed
                        ? "#fff"
                        : "var(--muted)",
                    fontWeight: 600,
                    cursor:
                      validationReport?.valid &&
                      validationReport?.securityPassed
                        ? "pointer"
                        : "not-allowed",
                    fontSize: "13px",
                  }}
                >
                  安装插件到 Workbench
                </button>
              </div>

              {/* Validation Feedback */}
              {validationReport && (
                <div
                  style={{
                    border: "1px solid var(--line)",
                    borderRadius: "8px",
                    padding: "16px",
                    background: "var(--bg)",
                    display: "flex",
                    flexDirection: "column",
                    gap: "10px",
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
                        padding: "3px 8px",
                        borderRadius: "4px",
                        fontSize: "12px",
                        fontWeight: 600,
                        background: validationReport.valid
                          ? "rgba(16, 185, 129, 0.15)"
                          : "rgba(239, 68, 68, 0.15)",
                        color: validationReport.valid ? "#10b981" : "#ef4444",
                      }}
                    >
                      {validationReport.valid ? "格式校验通过" : "格式存在错误"}
                    </span>
                    <span
                      style={{
                        padding: "3px 8px",
                        borderRadius: "4px",
                        fontSize: "12px",
                        fontWeight: 600,
                        background: validationReport.securityPassed
                          ? "rgba(16, 185, 129, 0.15)"
                          : "rgba(239, 68, 68, 0.15)",
                        color: validationReport.securityPassed
                          ? "#10b981"
                          : "#ef4444",
                      }}
                    >
                      {validationReport.securityPassed
                        ? " 安全沙箱检测合规"
                        : " 拦截到高危配置"}
                    </span>
                  </div>

                  {validationReport.errors.length > 0 && (
                    <div
                      style={{
                        display: "flex",
                        flexDirection: "column",
                        gap: "4px",
                      }}
                    >
                      <div
                        style={{
                          fontSize: "12px",
                          fontWeight: 600,
                          color: "#ef4444",
                        }}
                      >
                        错误项:
                      </div>
                      {validationReport.errors.map((err, idx) => (
                        <div
                          key={idx}
                          style={{
                            fontSize: "12px",
                            color: "#ef4444",
                            paddingLeft: "10px",
                          }}
                        >
                          • {err}
                        </div>
                      ))}
                    </div>
                  )}

                  {validationReport.warnings.length > 0 && (
                    <div
                      style={{
                        display: "flex",
                        flexDirection: "column",
                        gap: "4px",
                      }}
                    >
                      <div
                        style={{
                          fontSize: "12px",
                          fontWeight: 600,
                          color: "#f59e0b",
                        }}
                      >
                        安全与配置建议:
                      </div>
                      {validationReport.warnings.map((warn, idx) => (
                        <div
                          key={idx}
                          style={{
                            fontSize: "12px",
                            color: "#f59e0b",
                            paddingLeft: "10px",
                          }}
                        >
                          {warn}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {installStatus && (
                <div
                  style={{
                    padding: "10px 14px",
                    borderRadius: "6px",
                    fontSize: "13px",
                    background: installStatus.includes("安装成功")
                      ? "rgba(16, 185, 129, 0.15)"
                      : "rgba(239, 68, 68, 0.15)",
                    color: installStatus.includes("安装成功")
                      ? "#10b981"
                      : "#ef4444",
                  }}
                >
                  {installStatus}
                </div>
              )}
            </div>
          )}

          {/* TAB 2: Response Parser Sandbox */}
          {activeTab === "sandbox" && (
            <div
              style={{ display: "flex", flexDirection: "column", gap: "16px" }}
            >
              <div style={{ fontSize: "12px", color: "var(--muted)" }}>
                针对任意非标第三方 HTTP API，在此模拟测试 Response Parser
                提取能力（无需真实发起网络请求），支持 JSONPath
                (`$.data[0].url`) 或 JSON Pointer (`/choices/0/text`)。
              </div>

              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "1fr 1fr",
                  gap: "12px",
                }}
              >
                <div>
                  <label
                    style={{
                      fontSize: "12px",
                      color: "var(--muted)",
                      display: "block",
                      marginBottom: "4px",
                    }}
                  >
                    提取字段路径 (resultPath):
                  </label>
                  <input
                    type="text"
                    value={resultPath}
                    onChange={(e) => setResultPath(e.target.value)}
                    style={{
                      width: "100%",
                      fontFamily: "monospace",
                      fontSize: "12px",
                      padding: "6px 10px",
                      borderRadius: "6px",
                      border: "1px solid var(--line)",
                      background: "var(--bg)",
                      boxSizing: "border-box",
                    }}
                  />
                </div>

                <div>
                  <label
                    style={{
                      fontSize: "12px",
                      color: "var(--muted)",
                      display: "block",
                      marginBottom: "4px",
                    }}
                  >
                    轮询状态路径 statusPath (可选):
                  </label>
                  <input
                    type="text"
                    value={statusPath}
                    onChange={(e) => setStatusPath(e.target.value)}
                    placeholder="如 $.status 或 /state"
                    style={{
                      width: "100%",
                      fontFamily: "monospace",
                      fontSize: "12px",
                      padding: "6px 10px",
                      borderRadius: "6px",
                      border: "1px solid var(--line)",
                      background: "var(--bg)",
                      boxSizing: "border-box",
                    }}
                  />
                </div>
              </div>

              <div>
                <label
                  style={{
                    fontSize: "12px",
                    color: "var(--muted)",
                    display: "block",
                    marginBottom: "4px",
                  }}
                >
                  模拟上游返回的原始 JSON Response:
                </label>
                <textarea
                  rows={8}
                  value={sampleResponse}
                  onChange={(e) => setSampleResponse(e.target.value)}
                  style={{
                    width: "100%",
                    fontFamily: "monospace",
                    fontSize: "12px",
                    padding: "8px",
                    borderRadius: "6px",
                    border: "1px solid var(--line)",
                    background: "var(--bg)",
                    boxSizing: "border-box",
                  }}
                />
              </div>

              <button
                type="button"
                onClick={handleTestRecipe}
                disabled={isTestingRecipe}
                style={{
                  padding: "8px 16px",
                  borderRadius: "6px",
                  border: "none",
                  background: "var(--accent)",
                  color: "var(--accent-ink)",
                  fontWeight: 600,
                  cursor: "pointer",
                  fontSize: "13px",
                  alignSelf: "flex-start",
                }}
              >
                {isTestingRecipe ? "解析中..." : " 执行响应提取解析测试"}
              </button>

              {testResult && (
                <div
                  style={{
                    border: "1px solid var(--line)",
                    borderRadius: "8px",
                    padding: "14px",
                    background: "var(--bg)",
                    display: "flex",
                    flexDirection: "column",
                    gap: "8px",
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: "8px",
                    }}
                  >
                    <span
                      style={{
                        padding: "2px 8px",
                        borderRadius: "4px",
                        fontSize: "12px",
                        fontWeight: 600,
                        background: testResult.success
                          ? "rgba(16, 185, 129, 0.15)"
                          : "rgba(239, 68, 68, 0.15)",
                        color: testResult.success ? "#10b981" : "#ef4444",
                      }}
                    >
                      {testResult.success ? " 字段匹配成功" : " 字段提取失败"}
                    </span>
                    {testResult.resolvedStatus && (
                      <span style={{ fontSize: "12px", color: "var(--muted)" }}>
                        识别状态值: <strong>{testResult.resolvedStatus}</strong>
                      </span>
                    )}
                  </div>

                  {testResult.extractedValue && (
                    <div
                      style={{
                        display: "flex",
                        flexDirection: "column",
                        gap: "4px",
                      }}
                    >
                      <div style={{ fontSize: "11px", color: "var(--muted)" }}>
                        提取结果值 (Extracted Output):
                      </div>
                      <div
                        style={{
                          background: "var(--bg-card)",
                          padding: "8px 12px",
                          borderRadius: "4px",
                          fontFamily: "monospace",
                          fontSize: "12px",
                          color: "var(--accent)",
                          wordBreak: "break-all",
                        }}
                      >
                        {testResult.extractedValue}
                      </div>
                    </div>
                  )}

                  {testResult.error && (
                    <div style={{ fontSize: "12px", color: "#ef4444" }}>
                      {testResult.error}
                    </div>
                  )}
                </div>
              )}
            </div>
          )}

          {/* TAB 3: Docs & Templates */}
          {activeTab === "template" && (
            <div
              style={{ display: "flex", flexDirection: "column", gap: "16px" }}
            >
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                }}
              >
                <span style={{ fontSize: "13px", fontWeight: 600 }}>
                  插件标准清单模板 (Provider Manifest Template)
                </span>
                <button
                  type="button"
                  onClick={() => {
                    if (sdkInfo) {
                      navigator.clipboard.writeText(sdkInfo.manifestTemplate);
                      alert("已复制清单模板到剪贴板！");
                    }
                  }}
                  style={{
                    padding: "4px 10px",
                    fontSize: "12px",
                    borderRadius: "4px",
                    border: "1px solid var(--line)",
                    background: "var(--bg)",
                    cursor: "pointer",
                  }}
                >
                  复制清单模板
                </button>
              </div>

              <pre
                style={{
                  background: "var(--bg)",
                  padding: "12px",
                  borderRadius: "6px",
                  border: "1px solid var(--line)",
                  fontFamily: "monospace",
                  fontSize: "11px",
                  overflowX: "auto",
                }}
              >
                {sdkInfo?.manifestTemplate}
              </pre>

              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                }}
              >
                <span style={{ fontSize: "13px", fontWeight: 600 }}>
                  声明式 Generic HTTP 适配器模板 (HttpRecipe)
                </span>
                <button
                  type="button"
                  onClick={() => {
                    if (sdkInfo) {
                      navigator.clipboard.writeText(
                        sdkInfo.genericHttpTemplate,
                      );
                      alert("已复制适配器模板到剪贴板！");
                    }
                  }}
                  style={{
                    padding: "4px 10px",
                    fontSize: "12px",
                    borderRadius: "4px",
                    border: "1px solid var(--line)",
                    background: "var(--bg)",
                    cursor: "pointer",
                  }}
                >
                  复制适配器模板
                </button>
              </div>

              <pre
                style={{
                  background: "var(--bg)",
                  padding: "12px",
                  borderRadius: "6px",
                  border: "1px solid var(--line)",
                  fontFamily: "monospace",
                  fontSize: "11px",
                  overflowX: "auto",
                }}
              >
                {sdkInfo?.genericHttpTemplate}
              </pre>
            </div>
          )}
        </div>
      </div>
    </DialogFrame>
  );
}
