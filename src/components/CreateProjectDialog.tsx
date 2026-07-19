import { useState } from "react";
import { api } from "../api";

interface Props {
  onClose: () => void;
  onCreated: (project: any) => void;
}

export function CreateProjectDialog({ onClose, onCreated }: Props) {
  const [title, setTitle] = useState("");
  const [genre, setGenre] = useState("fantasy");
  const [premise, setPremise] = useState("");
  const [tone, setTone] = useState("serious");
  const [themes, setThemes] = useState("");
  const [targetWords, setTargetWords] = useState(3000);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const handleSubmit = async () => {
    if (!title.trim() || !premise.trim()) {
      setError("请填写标题和故事前提");
      return;
    }

    setLoading(true);
    setError("");

    try {
      const safeTargetWords = Number.isFinite(targetWords) && targetWords > 0 ? Math.floor(targetWords) : 3000;
      const project = await api.createProject({
        title: title.trim(),
        genre,
        premise: premise.trim(),
        tone,
        themes: themes.split(/[,，、]/).map(t => t.trim()).filter(Boolean),
        targetChapterWords: safeTargetWords,
      });

      onCreated(project);
      onClose();
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="dialog-overlay" onClick={onClose}>
      <div className="dialog-content" onClick={e => e.stopPropagation()}>
        <div className="dialog-header">
          <h3>创建新小说</h3>
          <button className="btn-close" onClick={onClose}>×</button>
        </div>

        <div className="dialog-body">
          {error && <div className="error">{error}</div>}

          <label>
            小说标题 *
            <input
              type="text"
              value={title}
              onChange={e => setTitle(e.target.value)}
              placeholder="例如：人皇遗命"
              autoFocus
            />
          </label>

          <label>
            类型
            <select value={genre} onChange={e => setGenre(e.target.value)}>
              <option value="fantasy">玄幻</option>
              <option value="scifi">科幻</option>
              <option value="urban">都市</option>
              <option value="romance">言情</option>
              <option value="mystery">悬疑</option>
              <option value="history">历史</option>
              <option value="horror">恐怖</option>
              <option value="other">其他</option>
            </select>
          </label>

          <label>
            故事前提 *
            <textarea
              value={premise}
              onChange={e => setPremise(e.target.value)}
              rows={4}
              placeholder="简要描述故事的核心设定和主线，例如：凡人少年偶获上古人皇传承，在宗门倾轧与天道压制中逆流而上，重写人族万古命运。"
            />
          </label>

          <label>
            基调
            <select value={tone} onChange={e => setTone(e.target.value)}>
              <option value="serious">严肃</option>
              <option value="light">轻松</option>
              <option value="dark">黑暗</option>
              <option value="humorous">幽默</option>
              <option value="epic">史诗</option>
            </select>
          </label>

          <label>
            核心主题（用逗号分隔）
            <input
              type="text"
              value={themes}
              onChange={e => setThemes(e.target.value)}
              placeholder="例如：成长、复仇、权力、爱情"
            />
          </label>

          <label>
            目标章节字数
            <input
              type="number"
              value={targetWords}
              onChange={e => setTargetWords(Number(e.target.value))}
              min={1000}
              step={500}
            />
          </label>
        </div>

        <div className="dialog-footer">
          <button className="btn-outline" onClick={onClose} disabled={loading}>
            取消
          </button>
          <button className="btn-primary" onClick={handleSubmit} disabled={loading}>
            {loading ? "创建中..." : "创建小说"}
          </button>
        </div>
      </div>
    </div>
  );
}
