import { invoke } from "@tauri-apps/api/core";

export interface ProjectMeta {
  id: string;
  title: string;
  genre: string;
  premise: string;
  tone: string;
  themes: string[];
  target_chapter_words: number;
  status: string;
  created_at: string;
}

export interface LlmParams {
  apiFormat: string;  // "openai" | "anthropic" | "gemini"
  apiKey: string;
  model: string;
  baseUrl: string;
}

export interface SkillRecord {
  id: string;
  name: string;
  repoUrl: string;
  installPath: string;
  description: string;
  enabled: boolean;
  installedAt: string;
  updatedAt: string;
}

export interface McpServerRecord {
  id: string;
  name: string;
  repoUrl: string;
  installPath: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  cwd: string;
  enabled: boolean;
  running: boolean;
  pid?: number | null;
  lastTestStatus: string;
  logPath: string;
}

export interface McpTestResult {
  success: boolean;
  status: string;
  logExcerpt: string;
}

export interface CreativeConstraintsPayload {
  mode: "strict" | "assist";
  skills: Array<{ id: string; name: string; content: string }>;
  prompts: Array<{ id: string; title: string; category: string; content: string }>;
}

export interface BatchProgress {
  current: number;
  total: number;
  chapter_number: number;
  phase: "context" | "generating" | "summarizing" | "done" | "skipped" | "failed" | "cancelled" | "summarize_failed";
  word_count: number;
  error: string;
}

export interface BatchComplete {
  completed: number;
  failed: number;
  skipped: number;
  total_words: number;
  elapsed_seconds: number;
  failed_chapters: number[];
}

function llmArgs(llm: LlmParams) {
  return { apiFormat: llm.apiFormat, apiKey: llm.apiKey, model: llm.model, baseUrl: llm.baseUrl };
}

export const api = {
  getDataDir: () => invoke<string>("get_data_dir"),
  setDataDir: (newDir: string, migrate: boolean) =>
    invoke<string>("set_data_dir", { newDir, migrate }),
  testLlm: (apiFormat: string, apiKey: string, model: string, baseUrl: string) =>
    invoke<string>("test_llm", { apiFormat, apiKey, model, baseUrl }),

  createProject: (data: {
    title: string; genre: string; premise: string;
    tone: string; themes: string[]; targetChapterWords: number;
  }) => invoke<ProjectMeta>("create_project", data),

  listProjects: () => invoke<ProjectMeta[]>("list_projects"),
  getProject: (projectId: string) => invoke<ProjectMeta>("get_project", { projectId }),
  saveOutlineSource: (projectId: string, outline: any) => invoke<any>("save_outline_source", { projectId, outline }),
  getOutlineSource: (projectId: string) => invoke<any>("get_outline_source", { projectId }),
  deleteProject: (projectId: string) => invoke("delete_project", { projectId }),

  generateWorld: (projectId: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke("generate_world", { projectId, constraints, ...llmArgs(llm) }),
  getWorld: (projectId: string) => invoke("get_world", { projectId }),
  saveWorldData: (projectId: string, world: any) => invoke<any>("save_world_data", { projectId, world }),

  generateCharacters: (projectId: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke("generate_characters", { projectId, constraints, ...llmArgs(llm) }),
  getCharacters: (projectId: string) => invoke("get_characters", { projectId }),
  saveCharactersData: (projectId: string, characters: any) => invoke<any>("save_characters_data", { projectId, characters }),

  generatePlot: (projectId: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke("generate_plot", { projectId, constraints, ...llmArgs(llm) }),
  getPlot: (projectId: string) => invoke("get_plot", { projectId }),
  savePlotOutline: (projectId: string, plot: any) => invoke<any>("save_plot_outline", { projectId, plot }),

  generateTimeline: (projectId: string, llm: LlmParams) =>
    invoke("generate_timeline", { projectId, ...llmArgs(llm) }),
  getTimeline: (projectId: string) => invoke("get_timeline", { projectId }),

  expandChapter: (projectId: string, chapterNumber: number, userContent: string, targetWords: number, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke("expand_chapter", { projectId, chapterNumber, userContent, targetWords, constraints, ...llmArgs(llm) }),

  continueWriting: (projectId: string, chapterNumber: number, instruction: string, targetWords: number, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke("continue_writing", { projectId, chapterNumber, instruction, targetWords, constraints, ...llmArgs(llm) }),

  saveChapter: (projectId: string, chapterNumber: number, text: string) =>
    invoke("save_chapter", { projectId, chapterNumber, text }),
  getChapter: (projectId: string, chapterNumber: number) =>
    invoke("get_chapter", { projectId, chapterNumber }),

  reviewChapter: (projectId: string, chapterNumber: number, chapterText: string, platform: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke<string>("review_chapter", { projectId, chapterNumber, chapterText, platform, constraints, ...llmArgs(llm) }),

  // RAG: Chapter summaries
  summarizeChapter: (projectId: string, chapterNumber: number, llm: LlmParams) =>
    invoke<any>("summarize_chapter", { projectId, chapterNumber, ...llmArgs(llm) }),
  getChapterSummaries: (projectId: string) =>
    invoke<any>("get_chapter_summaries", { projectId }),
  buildChapterContext: (projectId: string, chapterNumber: number) =>
    invoke<any>("build_chapter_context", { projectId, chapterNumber }),

  rewriteSelection: (projectId: string, chapterNumber: number, selectedText: string, instruction: string, targetDelta: number, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke<any>("rewrite_selection", { projectId, chapterNumber, selectedText, instruction, targetDelta, constraints, ...llmArgs(llm) }),

  agentChat: (projectId: string, message: string, history: [string, string][], llm: LlmParams) =>
    invoke<any>("agent_chat", { projectId, message, history, ...llmArgs(llm) }),

  chatWithAi: (messages: [string, string][], genre: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke<string>("chat_with_ai", { messages, genre, constraints, ...llmArgs(llm) }),

  extractFramework: (messages: [string, string][], genre: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke<any>("extract_framework", { messages, genre, constraints, ...llmArgs(llm) }),

  exportNovel: (projectId: string, format: string) =>
    invoke<string>("export_novel", { projectId, format }),

  // Batch generation
  batchGenerateChapters: (
    projectId: string, startChapter: number, endChapter: number,
    targetWords: number, skipWritten: boolean, llm: LlmParams,
    constraints?: CreativeConstraintsPayload,
  ) => invoke("batch_generate_chapters", {
    projectId, startChapter, endChapter, targetWords, skipWritten, constraints, ...llmArgs(llm),
  }),
  cancelBatchGeneration: () => invoke("cancel_batch_generation"),

  // Consistency check
  checkConsistency: (projectId: string, llm: LlmParams) =>
    invoke<any>("check_consistency", { projectId, ...llmArgs(llm) }),

  listSkills: () => invoke<SkillRecord[]>("list_skills"),
  installSkillRepo: (repoUrl: string) =>
    invoke<SkillRecord>("install_skill_repo", { repoUrl }),
  updateSkillRepo: (skillId: string) =>
    invoke<SkillRecord>("update_skill_repo", { skillId }),
  toggleSkillRepo: (skillId: string, enabled: boolean) =>
    invoke<SkillRecord>("toggle_skill_repo", { skillId, enabled }),
  removeSkillRepo: (skillId: string) =>
    invoke<void>("remove_skill_repo", { skillId }),
  getSkillDetail: (skillId: string) =>
    invoke<any>("get_skill_detail", { skillId }),
  readSkillFile: (skillId: string, relativePath: string) =>
    invoke<string>("read_skill_file", { skillId, relativePath }),

  listMcpServers: () => invoke<McpServerRecord[]>("list_mcp_servers"),
  installMcpRepo: (repoUrl: string) =>
    invoke<McpServerRecord>("install_mcp_repo", { repoUrl }),
  saveMcpServer: (server: Partial<McpServerRecord>) =>
    invoke<McpServerRecord>("save_mcp_server", { server }),
  deleteMcpServer: (serverId: string) =>
    invoke<void>("delete_mcp_server", { serverId }),
  testMcpServer: (serverId: string) =>
    invoke<McpTestResult>("test_mcp_server", { serverId }),
  startMcpServer: (serverId: string) =>
    invoke<McpServerRecord>("start_mcp_server", { serverId }),
  stopMcpServer: (serverId: string) =>
    invoke<McpServerRecord>("stop_mcp_server", { serverId }),
  getMcpLogs: (serverId: string) =>
    invoke<string>("get_mcp_logs", { serverId }),
};
