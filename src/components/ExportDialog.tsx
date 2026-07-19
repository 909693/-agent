import { useState } from "react";
import { api } from "../api";

interface Props {
  projectId: string;
  projectTitle: string;
  onClose: () => void;
}

export function ExportDialog({ projectId, projectTitle, onClose }: Props) {
  const [format, setFormat] = useState("txt");
  const [mode, setMode] = useState<"chapters" | "single">("chapters");
  const [exporting, setExporting] = useState(false);
  const [result, setResult] = useState<{ path: string; count: number } | null>(null);
  const [error, setError] = useState("");

  const handleExport = async () => {
    setExporting(true);
    setError("");
    setResult(null);
    try {
      const res = await api.exportNovel(projectId, format, mode);
      setResult(res);
    } catch (e: any) {
      setError(e.toString());
    }
    setExporting(false);
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
        <h3 style={{ fontSize: 16, fontWeight: 600, marginBottom: 16 }}>导出小说</h3>
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 4 }}>小说标题</div>
          <div style={{ fontSize: 15, fontWeight: 500 }}>{projectTitle}</div>
        </div>
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 6 }}>导出方式</div>
          <div style={{ display: "flex", gap: 8 }}>
            {[{ value: "chapters", label: "按章分文件" }, { value: "single", label: "整本合并" }].map((m) => (
              <button key={m.value}
                className={`btn-sm`}
                style={mode === m.value ? { background: "var(--accent)", color: "white", borderColor: "var(--accent)" } : {}}
                onClick={() => { setMode(m.value as "chapters" | "single"); setResult(null); }}>
                {m.label}
              </button>
            ))}
          </div>
        </div>
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 6 }}>导出格式</div>
          <div style={{ display: "flex", gap: 8 }}>
            {[{ value: "txt", label: "TXT 纯文本" }, { value: "md", label: "Markdown" }, { value: "html", label: "HTML (可打印PDF)" }].map((f) => (
              <button key={f.value}
                className={`btn-sm`}
                style={format === f.value ? { background: "var(--accent)", color: "white", borderColor: "var(--accent)" } : {}}
                onClick={() => { setFormat(f.value); setResult(null); }}>
                {f.label}
              </button>
            ))}
          </div>
        </div>
        {error && <div className="error">{error}</div>}
        {result && (
          <div style={{ background: "var(--success-light)", border: "1px solid var(--success)", borderRadius: "var(--radius)", padding: "10px 14px", marginBottom: 16, fontSize: 13 }}>
            导出成功！共 {result.count} 章{mode === "chapters" ? "，每章一个文件，已保存到目录：" : "，已合并保存到："}<br />
            <span style={{ fontWeight: 500, wordBreak: "break-all" }}>{result.path}</span>
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
