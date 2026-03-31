import { useState, useEffect } from "react";
import { api, type ProjectMeta, type LlmParams } from "./api";
import { Sidebar } from "./components/Sidebar";
import { Dashboard } from "./components/Dashboard";
import { NovelList } from "./components/NovelList";
import { ChapterManager } from "./components/ChapterManager";
import { ChapterEditor } from "./components/ChapterEditor";
import { SettingsPage } from "./components/SettingsPage";
import { ChatCreator } from "./components/ChatCreator";
import { PromptLibrary } from "./components/PromptLibrary";
import { WritingGoals } from "./components/WritingGoals";
import { GenreManager } from "./components/GenreManager";
import { SkillsManager } from "./components/SkillsManager";
import { McpManager } from "./components/McpManager";
import { OutlineImporter } from "./components/OutlineImporter";
import { AgentChat } from "./components/AgentChat";
import "./App.css";

type Page = "dashboard" | "novels" | "chapters" | "editor" | "settings" | "chat" | "prompts" | "goals" | "genres" | "skills" | "mcp" | "agent";

type ChatDraft = {
  genre: string | null;
  messages: Array<{ role: "user" | "assistant"; content: string }>;
  input: string;
  frameworkReady: boolean;
  error: string;
};

function App() {
  const [projects, setProjects] = useState<ProjectMeta[]>([]);
  const [currentProject, setCurrentProject] = useState<ProjectMeta | null>(null);
  const [page, setPage] = useState<Page>("dashboard");
  const [activeChapter, setActiveChapter] = useState(1);
  const [showOutlineImporter, setShowOutlineImporter] = useState(false);
  const [agentMessages, setAgentMessages] = useState<Array<{ role: "user" | "assistant"; content: string; action?: any }>>([]);
  const [chatDraft, setChatDraft] = useState<ChatDraft>({
    genre: null,
    messages: [],
    input: "",
    frameworkReady: false,
    error: "",
  });
  const [llm, setLlm] = useState<LlmParams>({
    apiFormat: localStorage.getItem("llm_api_format") || "openai",
    apiKey: localStorage.getItem("llm_api_key") || "",
    model: localStorage.getItem("llm_model") || "",
    baseUrl: localStorage.getItem("llm_base_url") || "",
  });
  const [theme, setTheme] = useState(localStorage.getItem("retl_theme") || "light");

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("retl_theme", theme);
  }, [theme]);

  useEffect(() => {
    api.listProjects().then(setProjects).catch(console.error);
  }, []);

  useEffect(() => {
    localStorage.setItem("llm_api_format", llm.apiFormat);
    localStorage.setItem("llm_api_key", llm.apiKey);
    localStorage.setItem("llm_model", llm.model);
    localStorage.setItem("llm_base_url", llm.baseUrl);
  }, [llm]);
  const isApiConfigured = !!(llm.apiKey && llm.model);

  const handleNewProject = () => {
    setChatDraft({ genre: null, messages: [], input: "", frameworkReady: false, error: "" });
    setPage("chat");
  };

  const handleProjectCreated = (project: ProjectMeta) => {
    setProjects(prev => [project, ...prev]);
    setCurrentProject(project);
    setPage("chapters");
  };

  const handleSelectProject = (p: ProjectMeta) => {
    setCurrentProject(p);
    setPage("chapters");
  };

  const handleDeleteProject = async (id: string) => {
    await api.deleteProject(id);
    setProjects(prev => prev.filter(p => p.id !== id));
    if (currentProject?.id === id) {
      setCurrentProject(null);
      if (page === "chapters" || page === "editor") setPage("novels");
    }
  };

  const handleWriteChapter = (num: number) => {
    setActiveChapter(num);
    setPage("editor");
  };

  const handleNavigate = (target: string) => {
    setPage(target as Page);
  };

  const pageTitle: Record<Page, string> = {
    dashboard: "首页",
    novels: "小说列表",
    chapters: "章节管理",
    editor: "章节写作",
    settings: "系统设置",
    chat: "创建新小说",
    prompts: "提示词库",
    goals: "写作目标",
    genres: "小说类型管理",
    skills: "Skills 安装",
    mcp: "MCP 管理",
    agent: "AI 助手",
  };
  return (
    <div className="app">
      <Sidebar
        currentPage={page === "editor" ? "chapters" : page === "chat" ? "novels" : page}
        hasSelectedNovel={!!currentProject}
        onNavigate={handleNavigate}
      />
      <div className="layout-right">
        <div className="top-bar">
          <span className="top-bar-title">{pageTitle[page]}</span>
          <div className="top-bar-right">
            <span
              className={`api-badge ${isApiConfigured ? "configured" : "not-configured"}`}
              onClick={isApiConfigured ? undefined : () => setPage("settings")}
            >
              {isApiConfigured ? "API 已配置" : "配置 API"}
            </span>
          </div>
        </div>
        <div className="main-content">
          {showOutlineImporter && (
            <OutlineImporter
              onCreated={handleProjectCreated}
              onClose={() => setShowOutlineImporter(false)}
            />
          )}
          {page === "dashboard" && (
            <Dashboard
              projects={projects}
              onNewNovel={handleNewProject}
              onImportOutline={() => setShowOutlineImporter(true)}
              onSelectNovel={handleSelectProject}
            />
          )}
          {page === "novels" && (
            <NovelList
              projects={projects}
              onNewNovel={handleNewProject}
              onImportOutline={() => setShowOutlineImporter(true)}
              onSelectNovel={handleSelectProject}
              onDeleteNovel={handleDeleteProject}
            />
          )}
          {page === "chapters" && currentProject && (
            <ChapterManager
              project={currentProject}
              llm={llm}
              onWriteChapter={handleWriteChapter}
            />
          )}
          {page === "editor" && currentProject && (
            <ChapterEditor
              projectId={currentProject.id}
              llm={llm}
              initialChapter={activeChapter}
              onBack={() => setPage("chapters")}
            />
          )}
          {page === "settings" && (
            <SettingsPage llm={llm} onChange={setLlm} theme={theme} onThemeChange={setTheme} />
          )}
          {page === "chat" && (
            <ChatCreator
              llm={llm}
              draft={chatDraft}
              onDraftChange={setChatDraft}
              onProjectCreated={handleProjectCreated}
              onCancel={() => setPage(currentProject ? "chapters" : "dashboard")}
            />
          )}
          {page === "prompts" && <PromptLibrary />}
          {page === "goals" && <WritingGoals />}
          {page === "genres" && <GenreManager />}
          {page === "skills" && <SkillsManager />}
          {page === "mcp" && <McpManager />}
          {page === "agent" && currentProject && (
            <AgentChat
              projectId={currentProject.id}
              llm={llm}
              messages={agentMessages}
              onMessagesChange={setAgentMessages}
              onAction={() => {}}
            />
          )}
        </div>
      </div>
    </div>
  );
}

export default App;
