import { useState } from "react";
import { api } from "../api";
import { parseOutlineToPlot } from "../utils/outlineParser";
import { extractWorldFromOutline, extractCharactersFromOutline } from "../utils/outlineExtractors";

interface Props {
  onCreated: (project: any) => void;
  onClose: () => void;
}

const genreOptions = [
  { value: "fantasy", label: "玄幻/仙侠" },
  { value: "scifi", label: "科幻" },
  { value: "urban", label: "都市" },
  { value: "romance", label: "言情" },
  { value: "mystery", label: "悬疑" },
  { value: "history", label: "历史" },
  { value: "horror", label: "恐怖" },
  { value: "other", label: "其他" },
];

export function OutlineImporter({ onCreated, onClose }: Props) {
  const [title, setTitle] = useState("");
  const [genre, setGenre] = useState("fantasy");
  const [tone, setTone] = useState("");
  const [themes, setThemes] = useState("");
  const [outlineText, setOutlineText] = useState("");
  const [outlineName, setOutlineName] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const handleOutlineFile = async (file: File) => {
    setError("");
    try {
      if (file.name.toLowerCase().endsWith(".docx")) {
        const arrayBuffer = await file.arrayBuffer();
        const mammoth: any = await import("mammoth");
        const result = await mammoth.extractRawText({ arrayBuffer });
        setOutlineText(result.value || "");
        setOutlineName(file.name);
      } else {
        const text = await file.text();
        setOutlineText(text);
        setOutlineName(file.name);
      }
    } catch (e: any) {
      setError(e.toString());
    }
  };

  const handleCreate = async () => {
    if (!title.trim() || !outlineText.trim()) { setError("标题和大纲不能为空"); return; }
    setLoading(true); setError("");
    try {
      const premise = outlineText.slice(0, 300);
      const project = await api.createProject({
        title: title.trim(),
        genre,
        premise,
        tone: tone.trim(),
        themes: themes.split(/[,，]/).map(s => s.trim()).filter(Boolean),
        targetChapterWords: 3000,
      });
      await api.saveOutlineSource(project.id, { name: outlineName || "手动导入大纲", text: outlineText, importedAt: new Date().toISOString() });
      const parsedPlot = parseOutlineToPlot(outlineText);
      if (parsedPlot.acts.length > 0) {
        await api.savePlotOutline(project.id, parsedPlot);
      }
      const world = extractWorldFromOutline(outlineText);
      await api.saveWorldData(project.id, world);
      const characters = extractCharactersFromOutline(outlineText);
      if (characters.characters.length > 0) {
        await api.saveCharactersData(project.id, characters);
      }
      onCreated(project);
    } catch (e: any) {
      setError(e.toString());
    }
    setLoading(false);
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal large" onClick={e => e.stopPropagation()}>
        <h2>从大纲导入创建小说</h2>
        {error && <div className="error">{error}</div>}
        <div className="form-group">
          <label>小说标题</label>
          <input value={title} onChange={e => setTitle(e.target.value)} placeholder="输入小说标题" />
        </div>
        <div className="form-group">
          <label>小说类型</label>
          <select value={genre} onChange={e => setGenre(e.target.value)}>
            {genreOptions.map(g => <option key={g.value} value={g.value}>{g.label}</option>)}
          </select>
        </div>
        <div className="form-group">
          <label>基调（可选）</label>
          <input value={tone} onChange={e => setTone(e.target.value)} placeholder="如：热血、黑暗、轻松" />
        </div>
        <div className="form-group">
          <label>主题（可选，逗号分隔）</label>
          <input value={themes} onChange={e => setThemes(e.target.value)} placeholder="成长, 复仇, 命运" />
        </div>
        <div className="form-group">
          <label>导入大纲</label>
          <div style={{ display: "flex", gap: 12, marginBottom: 10, flexWrap: "wrap" }}>
            <label className="btn-outline" style={{ cursor: "pointer" }}>
              上传 TXT / DOCX
              <input type="file" accept=".txt,.md,.docx" style={{ display: "none" }} onChange={e => { const f = e.target.files?.[0]; if (f) void handleOutlineFile(f); }} />
            </label>
            {outlineName && <span className="dim">已导入：{outlineName}</span>}
          </div>
          <textarea value={outlineText} onChange={e => setOutlineText(e.target.value)} rows={12} placeholder="粘贴你的全书大纲、分卷大纲、章节大纲..." />
        </div>
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button className="btn-outline" onClick={onClose}>取消</button>
          <button className="btn-primary" onClick={() => void handleCreate()} disabled={loading || !title.trim() || !outlineText.trim()}>
            {loading ? "创建中..." : "创建项目并导入大纲"}
          </button>
        </div>
      </div>
    </div>
  );
}
