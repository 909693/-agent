import { useState } from "react";
import { Library, FileText, PenTool, Sparkles, FileUp, Plus, MessageSquare, Zap } from "lucide-react";
import type { ProjectMeta } from "../api";
import { getHistory, getTodayWords, getDailyGoal } from "../utils/writingLog";

interface Props {
  projects: ProjectMeta[];
  onNewNovel: () => void;
  onNewNovelChat: () => void;
  onImportOutline: () => void;
  onSelectNovel: (p: ProjectMeta) => void;
}

const genreLabels: Record<string, string> = {
  fantasy: "玄幻", scifi: "科幻", urban: "都市", romance: "言情",
  mystery: "悬疑", history: "历史", horror: "恐怖", other: "其他",
};

export function Dashboard({ projects, onNewNovel, onNewNovelChat, onImportOutline, onSelectNovel }: Props) {
  const [showCreateChoice, setShowCreateChoice] = useState(false);
  const totalNovels = projects.length;
  const history = getHistory();
  const totalWords = history.reduce((sum, d) => sum + d.words, 0);
  const todayWords = getTodayWords();
  const dailyGoal = getDailyGoal();
  const todayPct = Math.min(100, Math.round((todayWords / dailyGoal) * 100));
  const recentProjects = [...projects].slice(0, 5);

  return (
    <div>
      <div className="welcome-banner">
        <div>
          <h2>欢迎使用祈愿创作平台</h2>
          <p>AI 驱动的小说创作助手，从构思到成稿一站式完成</p>
        </div>
        <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
          <button className="btn-white" onClick={() => setShowCreateChoice(true)}>
            <Plus size={16} style={{ verticalAlign: "middle", marginRight: 4 }} />
            创建新小说
          </button>
          <button className="btn-outline" style={{ background: "rgba(255,255,255,0.12)", color: "white", borderColor: "rgba(255,255,255,0.35)" }} onClick={onImportOutline}>
            <FileUp size={16} style={{ verticalAlign: "middle", marginRight: 4 }} />
            从大纲导入
          </button>
        </div>
      </div>

      {showCreateChoice && (
        <div className="modal-overlay" onClick={() => setShowCreateChoice(false)}>
          <div className="create-choice-panel" onClick={e => e.stopPropagation()}>
            <h3>选择创建方式</h3>
            <div className="create-choice-grid">
              <button className="create-choice-card" onClick={() => { setShowCreateChoice(false); onNewNovel(); }}>
                <Zap size={32} />
                <h4>快速创建</h4>
                <p>填写标题、类型、前提等基本信息，立即创建小说项目</p>
              </button>
              <button className="create-choice-card" onClick={() => { setShowCreateChoice(false); onNewNovelChat(); }}>
                <MessageSquare size={32} />
                <h4>AI 对话创建</h4>
                <p>与 AI 对话交流，逐步构思世界观、角色和大纲，再生成小说</p>
              </button>
            </div>
          </div>
        </div>
      )}

      <div className="stats-row">
        <div className="stat-card"><div className="stat-icon"><Library size={24} /></div><div className="stat-value">{totalNovels}</div><div className="stat-label">总小说数</div></div>
        <div className="stat-card"><div className="stat-icon"><PenTool size={24} /></div><div className="stat-value">{totalWords.toLocaleString()}</div><div className="stat-label">总字数</div></div>
        <div className="stat-card"><div className="stat-icon"><FileText size={24} /></div><div className="stat-value">{todayWords}</div><div className="stat-label">今日字数 ({todayPct}%)</div></div>
      </div>
      <div className="dashboard-grid">
        <div className="dash-section">
          <h3>快捷操作</h3>
          <button className="quick-action" onClick={() => setShowCreateChoice(true)}>
            <span className="qa-icon"><Sparkles size={20} /></span><div className="qa-text"><h4>创建新小说</h4><p>通过表单或 AI 对话构思你的新故事</p></div>
          </button>
          <button className="quick-action" onClick={onImportOutline}>
            <span className="qa-icon"><FileUp size={20} /></span><div className="qa-text"><h4>从大纲导入</h4><p>导入 TXT / DOCX / 粘贴大纲，直接开始按纲创作</p></div>
          </button>
        </div>
        <div className="dash-section">
          <h3>最近编辑的小说</h3>
          {recentProjects.length === 0 ? (
            <div className="empty-state"><p>还没有小说，点击「创建新小说」或「从大纲导入」开始吧</p></div>
          ) : recentProjects.map(p => (
            <div key={p.id} className="recent-novel">
              <div className="recent-novel-info"><h4>{p.title}</h4><p>{genreLabels[p.genre] || p.genre} · {p.created_at?.slice(0, 10)}</p></div>
              <button className="btn-sm" onClick={() => onSelectNovel(p)}>继续写作</button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
