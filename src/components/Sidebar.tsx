import {
  Home,
  Library,
  FileText,
  Bot,
  ClipboardList,
  BarChart3,
  Puzzle,
  Plug,
  FolderOpen,
  Settings,
} from "lucide-react";

interface Props {
  currentPage: string;
  hasSelectedNovel: boolean;
  onNavigate: (page: string) => void;
}

export function Sidebar({ currentPage, hasSelectedNovel, onNavigate }: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar-brand">
        <h1>祈愿</h1>
        <p>AI 小说创作助手</p>
      </div>
      <nav className="sidebar-nav">
        <button
          className={`nav-item ${currentPage === "dashboard" ? "active" : ""}`}
          onClick={() => onNavigate("dashboard")}
        >
          <Home size={18} />
          首页
        </button>
        <button
          className={`nav-item ${currentPage === "novels" ? "active" : ""}`}
          onClick={() => onNavigate("novels")}
        >
          <Library size={18} />
          小说列表
        </button>
        {hasSelectedNovel && (
          <>
            <button
              className={`nav-item ${currentPage === "chapters" ? "active" : ""}`}
              onClick={() => onNavigate("chapters")}
            >
              <FileText size={18} />
              章节管理
            </button>
            <button
              className={`nav-item ${currentPage === "agent" ? "active" : ""}`}
              onClick={() => onNavigate("agent")}
            >
              <Bot size={18} />
              AI 助手
            </button>
          </>
        )}
        <button
          className={`nav-item ${currentPage === "prompts" ? "active" : ""}`}
          onClick={() => onNavigate("prompts")}
        >
          <ClipboardList size={18} />
          提示词库
        </button>
        <button
          className={`nav-item ${currentPage === "goals" ? "active" : ""}`}
          onClick={() => onNavigate("goals")}
        >
          <BarChart3 size={18} />
          写作目标
        </button>
        <button
          className={`nav-item ${currentPage === "skills" ? "active" : ""}`}
          onClick={() => onNavigate("skills")}
        >
          <Puzzle size={18} />
          Skills 安装
        </button>
        <button
          className={`nav-item ${currentPage === "mcp" ? "active" : ""}`}
          onClick={() => onNavigate("mcp")}
        >
          <Plug size={18} />
          MCP 管理
        </button>
        <div className="nav-divider" />
        <button
          className={`nav-item ${currentPage === "genres" ? "active" : ""}`}
          onClick={() => onNavigate("genres")}
        >
          <FolderOpen size={18} />
          小说类型管理
        </button>
        <button
          className={`nav-item ${currentPage === "settings" ? "active" : ""}`}
          onClick={() => onNavigate("settings")}
        >
          <Settings size={18} />
          系统设置
        </button>
      </nav>
    </aside>
  );
}
