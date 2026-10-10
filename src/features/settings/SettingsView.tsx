import { useState } from "react";
import { ConfigIoView } from "./ConfigIoView";
import { DiagnosticsView } from "./DiagnosticsView";
import { DiscoveryView } from "./DiscoveryView";
import { NetworkView } from "./NetworkView";
import { ProvidersView } from "./ProvidersView";

type SettingsTab =
  | "providers"
  | "discovery"
  | "network"
  | "config_io"
  | "diagnostics";

export function SettingsView() {
  const [tab, setTab] = useState<SettingsTab>("providers");

  return (
    <div className="settings-workspace">
      <header className="settings-header">
        <div style={{ marginRight: "16px" }}>
          <p className="eyebrow" style={{ margin: 0 }}>
            Settings
          </p>
          <h1 style={{ margin: 0, fontSize: "18px" }}>系统设置</h1>
        </div>

        <div className="mode-switch" style={{ margin: 0 }}>
          <button
            type="button"
            aria-pressed={tab === "providers"}
            onClick={() => setTab("providers")}
          >
            Provider 接入与插件
          </button>
          <button
            type="button"
            aria-pressed={tab === "discovery"}
            onClick={() => setTab("discovery")}
          >
            本地服务管理
          </button>
          <button
            type="button"
            aria-pressed={tab === "network"}
            onClick={() => setTab("network")}
          >
            网络代理
          </button>
          <button
            type="button"
            aria-pressed={tab === "config_io"}
            onClick={() => setTab("config_io")}
          >
            配置导入与导出
          </button>
          <button
            type="button"
            aria-pressed={tab === "diagnostics"}
            onClick={() => setTab("diagnostics")}
          >
            系统维护与诊断
          </button>
        </div>
      </header>

      <div style={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
        {tab === "providers" && <ProvidersView />}
        {tab === "discovery" && (
          <div
            style={{ height: "100%", overflowY: "auto", padding: "24px 32px" }}
          >
            <DiscoveryView />
          </div>
        )}
        {tab === "network" && (
          <div
            style={{ height: "100%", overflowY: "auto", padding: "24px 32px" }}
          >
            <NetworkView />
          </div>
        )}
        {tab === "config_io" && (
          <div
            style={{ height: "100%", overflowY: "auto", padding: "24px 32px" }}
          >
            <ConfigIoView />
          </div>
        )}
        {tab === "diagnostics" && (
          <div
            style={{ height: "100%", overflowY: "auto", padding: "24px 32px" }}
          >
            <DiagnosticsView />
          </div>
        )}
      </div>
    </div>
  );
}
