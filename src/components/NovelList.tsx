import { BookOpen, CalendarDays, FileUp, Plus } from "lucide-react";
import type { ProjectMeta } from "../api";

interface Props {
  projects: ProjectMeta[];
  onNewNovel: () => void;
  onImportOutline: () => void;
  onSelectNovel: (p: ProjectMeta) => void;
  onDeleteNovel: (id: string) => void;
}

const genreLabels: Record<string, string> = {
  fantasy: "玄幻", scifi: "科幻", urban: "都市", romance: "言情",
  mystery: "悬疑", history: "历史", horror: "恐怖", other: "其他",
};

export function NovelList({ projects, onNewNovel, onImportOutline, onSelectNovel, onDeleteNovel }: Props) {
  return (
    <div>
      <div className="page-header">
        <h2>小说列表</h2>
        <div className="toolbar-actions">
          <button className="btn-outline" onClick={onImportOutline}>
            <FileUp size={15} style={{ verticalAlign: -2, marginRight: 5 }} />
            从大纲导入
          </button>
          <button className="btn-primary" onClick={onNewNovel}>
            <Plus size={16} style={{ verticalAlign: -2, marginRight: 4 }} />
            创建新小说
          </button>
        </div>
      </div>

      {projects.length === 0 ? (
        <div className="empty-state">
          <div className="empty-icon"><BookOpen size={28} /></div>
          <p>还没有小说，点击上方按钮开始创作</p>
        </div>
      ) : (
        <div className="novel-grid">
          {projects.map(p => (
            <div key={p.id} className="novel-card" onClick={() => onSelectNovel(p)}>
              <div className="novel-card-header">
                <h3>{p.title}</h3>
                <button className="btn-danger" onClick={e => { e.stopPropagation(); if (confirm(`确定删除《${p.title}》？此操作不可恢复，将永久删除该书的全部章节与历史版本。`)) onDeleteNovel(p.id); }}>删除</button>
              </div>
              <span className="genre-tag">{genreLabels[p.genre] || p.genre}</span>
              {p.premise && <p className="premise">{p.premise}</p>}
              <div className="novel-card-footer">
                <span className="meta-with-icon"><CalendarDays size={14} />{p.created_at?.slice(0, 10)}</span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
