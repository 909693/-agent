import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("[ErrorBoundary]", error, info.componentStack);
  }

  handleReload = () => {
    window.location.reload();
  };

  render() {
    if (this.state.hasError) {
      return (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            height: "100vh",
            fontFamily:
              '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
            background: "#fafafa",
            color: "#333",
            padding: 24,
            textAlign: "center",
          }}
        >
          <div style={{ fontSize: 48, marginBottom: 16 }}>⚠️</div>
          <h1 style={{ fontSize: 20, fontWeight: 600, marginBottom: 8 }}>
            应用出现异常，请刷新重试
          </h1>
          <p
            style={{
              fontSize: 14,
              color: "#888",
              marginBottom: 24,
              maxWidth: 480,
            }}
          >
            {this.state.error?.message || "发生了未知错误"}
          </p>
          <button
            onClick={this.handleReload}
            style={{
              padding: "10px 32px",
              fontSize: 15,
              fontWeight: 500,
              border: "none",
              borderRadius: 8,
              background: "#4f46e5",
              color: "#fff",
              cursor: "pointer",
              transition: "background 0.2s",
            }}
            onMouseOver={(e) =>
              ((e.target as HTMLButtonElement).style.background = "#4338ca")
            }
            onMouseOut={(e) =>
              ((e.target as HTMLButtonElement).style.background = "#4f46e5")
            }
          >
            刷新
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
