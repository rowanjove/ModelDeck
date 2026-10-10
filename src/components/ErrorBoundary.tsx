import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  moduleName: string;
  children: ReactNode;
  onReset?: () => void;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  public override state: State = {
    hasError: false,
    error: null,
  };

  public static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  public override componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error(
      `[ErrorBoundary] ${this.props.moduleName} crashed:`,
      error,
      errorInfo,
    );
  }

  private handleReset = () => {
    this.setState({ hasError: false, error: null });
    if (this.props.onReset) {
      this.props.onReset();
    }
  };

  private handleCopy = () => {
    if (this.state.error) {
      navigator.clipboard.writeText(
        `[${this.props.moduleName}] ${this.state.error.name}: ${this.state.error.message}\n${this.state.error.stack || ""}`,
      );
    }
  };

  public override render() {
    if (this.state.hasError) {
      return (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            height: "100%",
            padding: "32px",
            background: "var(--bg)",
            color: "var(--ink)",
            textAlign: "center",
          }}
        >
          <div
            style={{
              maxWidth: "520px",
              padding: "24px 28px",
              background: "var(--surface-1)",
              border: "1px solid var(--line)",
              borderRadius: "var(--radius-lg)",
              boxShadow: "var(--shadow-md)",
            }}
          >
            <div style={{ fontSize: "24px", marginBottom: "10px" }}>⚠️</div>
            <h3
              style={{
                margin: "0 0 8px",
                fontSize: "15px",
                fontWeight: 650,
                color: "var(--ink)",
              }}
            >
              {this.props.moduleName} 模块渲染发生异常
            </h3>
            <p
              className="muted"
              style={{
                fontSize: "13px",
                lineHeight: "1.5",
                margin: "0 0 16px",
                color: "var(--soft)",
              }}
            >
              已拦截局部异常，应用其它功能未受影响。您可以重试恢复此页面。
            </p>
            {this.state.error ? (
              <div
                style={{
                  background: "var(--danger-subtle)",
                  border: "1px solid var(--danger-border)",
                  padding: "10px 12px",
                  borderRadius: "var(--radius-md)",
                  fontSize: "12px",
                  color: "var(--danger)",
                  textAlign: "left",
                  fontFamily: "var(--mono)",
                  wordBreak: "break-all",
                  marginBottom: "16px",
                  maxHeight: "120px",
                  overflowY: "auto",
                }}
              >
                {this.state.error.message || "未知错误"}
              </div>
            ) : null}
            <div
              style={{ display: "flex", gap: "10px", justifyContent: "center" }}
            >
              <button
                type="button"
                className="primary"
                onClick={this.handleReset}
              >
                🔄 重试恢复页面
              </button>
              <button type="button" onClick={this.handleCopy}>
                📋 复制错误日志
              </button>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
