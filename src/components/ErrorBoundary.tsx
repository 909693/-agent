import { Component, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle } from "lucide-react";

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
            background: "#f5f5f7",
            color: "#1d1d1f",
            padding: 24,
            textAlign: "center",
          }}
        >
          <AlertTriangle size={48} color="#FF9500" style={{ marginBottom: 16 }} />
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
              background: "#007AFF",
              color: "#fff",
              cursor: "pointer",
              transition: "background 0.2s",
            }}
            onMouseOver={(e) =>
              ((e.target as HTMLButtonElement).style.background = "#005FCC")
            }
            onMouseOut={(e) =>
              ((e.target as HTMLButtonElement).style.background = "#007AFF")
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
