import { useState } from "react";
import { api } from "../api";

interface Props {
  projectId: string;
  projectTitle: string;
  onClose: () => void;
}

export function ExportDialog({ projectId, projectTitle, onClose }: Props) {
  const [format, setFormat] = useState("txt");
  const [exporting, setExporting] = useState(false);
  const [result, setResult] = useState("");
  const [error, setError] = useState("");

  const handleExport = async () => {
    setExporting(true);
    setError("");
    setResult("");
    try {
      const path = await api.exportNovel(projectId, format);
      setResult(path);
    } catch (e: any) {
      setError(e.toString());
    }
    setExporting(false);
  };

  return (
    <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.3)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 100 }}
      onClick={onClose}>
      <div style={{ background: "var(--bg-white)", borderRadius: "var(--radius-lg)", padding: 24, width: 420, boxShadow: "var(--shadow-md)" }}
        onClick={(e) => e.stopPropagation()}>
        <h3 style={{ fontSize: 16, fontWeight: 600, marginBottom: 16 }}>导出小说</h3>
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 4 }}>小说标题</div>
          <div style={{ fontSize: 15, fontWeight: 500 }}>{projectTitle}</div>
        </div>
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 6 }}>导出格式</div>
          <div style={{ display: "flex", gap: 8 }}>
            {[{ value: "txt", label: "TXT 纯文本" }, { value: "md", label: "Markdown" }, { value: "html", label: "HTML (可打印PDF)" }].map((f) => (
              <button key={f.value}
                className={`btn-sm`}
                style={format === f.value ? { background: "var(--accent)", color: "white", borderColor: "var(--accent)" } : {}}
                onClick={() => setFormat(f.value)}>
                {f.label}
              </button>
            ))}
          </div>
        </div>
        {error && <div className="error">{error}</div>}
        {result && (
          <div style={{ background: "var(--success-light)", border: "1px solid var(--success)", borderRadius: "var(--radius)", padding: "10px 14px", marginBottom: 16, fontSize: 13 }}>
            导出成功！文件已保存到：<br />
            <span style={{ fontWeight: 500, wordBreak: "break-all" }}>{result}</span>
          </div>
        )}
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button className="btn-outline" onClick={onClose}>关闭</button>
          {!result && (
            <button className="btn-primary" onClick={handleExport} disabled={exporting}>
              {exporting ? <><span className="loading-spinner" />导出中...</> : "导出"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
