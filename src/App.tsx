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
import { CreateProjectDialog } from "./components/CreateProjectDialog";
import { ErrorBoundary } from "./components/ErrorBoundary";
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
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [agentMessages, setAgentMessages] = useState<Array<{ role: "user" | "assistant" | "tool"; content: string; action?: any; toolName?: string; toolSuccess?: boolean; streaming?: boolean }>>([]);
  const [chatDraft, setChatDraft] = useState<ChatDraft>({
    genre: null,
    messages: [],
    input: "",
    frameworkReady: false,
    error: "",
  });
  const [llm, setLlm] = useState<LlmParams>({
    apiFormat: "openai",
    apiKey: "",
    model: "",
    baseUrl: "",
  });
  const [llmLoaded, setLlmLoaded] = useState(false);
  const [theme, setTheme] = useState(localStorage.getItem("retl_theme") || "light");

  // Load LLM config from backend on mount
  useEffect(() => {
    api.getLlmConfig().then((config) => {
      if (config && typeof config === "object") {
        // Always load config, even if apiKey is empty (keychain might have failed)
        setLlm(prev => ({
          ...prev,
          ...config,
        }));
      }
      setLlmLoaded(true);
    }).catch(() => {
      setLlmLoaded(true);
    });
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("retl_theme", theme);
  }, [theme]);

  useEffect(() => {
    api.listProjects().then(setProjects).catch(console.error);
  }, []);

  // Save LLM config to backend when it changes (debounced, skip initial empty state)
  useEffect(() => {
    if (!llmLoaded) return;
    const timer = setTimeout(() => {
      api.saveLlmConfig(llm).catch(() => {});
    }, 1000);
    return () => clearTimeout(timer);
  }, [llm, llmLoaded]);
  const isApiConfigured = !!(llm.apiKey && llm.model);

  const handleNewProject = () => {
    setShowCreateDialog(true);
  };

  const handleNewProjectChat = () => {
    setPage("chat");
  };

  const handleProjectCreated = (project: ProjectMeta) => {
    setProjects(prev => [project, ...prev]);
    setCurrentProject(project);
    // Reset per-project agent/chat state so the previous project's Agent history
    // and create-chat draft don't bleed into the new project.
    setAgentMessages([]);
    setChatDraft({ genre: null, messages: [], input: "", frameworkReady: false, error: "" });
    setPage("chapters");
  };

  const handleSelectProject = (p: ProjectMeta) => {
    setCurrentProject(p);
    setActiveChapter(1);
    setAgentMessages([]);
    setChatDraft({ genre: null, messages: [], input: "", frameworkReady: false, error: "" });
    setPage("chapters");
  };

  const handleDeleteProject = async (id: string) => {
    try {
      await api.deleteProject(id);
      setProjects(prev => prev.filter(p => p.id !== id));
      if (currentProject?.id === id) {
        setCurrentProject(null);
        if (page === "chapters" || page === "editor") setPage("novels");
      }
    } catch (e: any) {
      console.error("删除项目失败:", e);
    }
  };

  const handleWriteChapter = (num: number) => {
    setActiveChapter(num);
    setPage("editor");
  };

  const handleNavigate = (target: string) => {
    const validPages: Page[] = ["dashboard", "novels", "chapters", "editor", "settings", "chat", "prompts", "goals", "genres", "skills", "mcp", "agent"];
    if (validPages.includes(target as Page)) {
      setPage(target as Page);
    }
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
  const pageShellClass =
    page === "editor" || page === "chapters"
      ? "page-shell page-shell--wide"
      : page === "chat" || page === "agent"
        ? "page-shell page-shell--fill"
        : "page-shell";
  return (
    <ErrorBoundary>
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
          {showCreateDialog && (
            <CreateProjectDialog
              onCreated={handleProjectCreated}
              onClose={() => setShowCreateDialog(false)}
            />
          )}
          <div className={pageShellClass}>
            {page === "dashboard" && (
              <Dashboard
                projects={projects}
                onNewNovel={handleNewProject}
                onNewNovelChat={handleNewProjectChat}
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
                genre={currentProject.genre}
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
                genre={currentProject.genre}
                llm={llm}
                messages={agentMessages}
                onMessagesChange={setAgentMessages}
                onAction={() => {}}
              />
            )}
          </div>
        </div>
      </div>
    </div>
    </ErrorBoundary>
  );
}

export default App;
