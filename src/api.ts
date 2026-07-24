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
  proxyUrl?: string;  // Optional proxy URL (e.g., "http://127.0.0.1:7897")
  userAgent?: string; // Optional User-Agent to mimic a specific client (e.g., Claude Code / Codex)
}

// 多供应商:每个 Provider 是完全自包含的一套 LLM 配置(含独立代理/UA)。
// 选中一个即派生出当前生效的 LlmParams 传给下游。
export interface LlmProvider {
  id: string;         // uuid,内部标识
  name: string;       // 用户自定义显示名
  apiFormat: string;
  apiKey: string;
  model: string;      // 当前选用的模型(从 models 池里选一个)
  models?: string[];  // 该供应商配置的模型池,可有多个;写作时从中选一个
  baseUrl: string;
  proxyUrl?: string;
  userAgent?: string;
}

export interface LlmProvidersData {
  activeId: string;
  providers: LlmProvider[];
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
  phase: "context" | "generating" | "retrying" | "summarizing" | "done" | "skipped" | "failed" | "cancelled" | "summarize_failed";
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

export interface PlotProgress {
  phase: "skeleton" | "act_details" | "done" | "error";
  current_act: number;
  total_acts: number;
  message: string;
}

// ===== Domain Types =====

export interface ChapterData {
  text: string;
}

export interface OutlineSource {
  name: string;
  text: string;
  importedAt: string;
}

export interface WorldData {
  era?: string;
  overview?: string;
  geography?: unknown;
  rules?: unknown;
  factions?: unknown[];
  history?: unknown[];
  [key: string]: unknown;
}

export interface CharactersData {
  characters: Array<{
    id?: string;
    name: string;
    role: string;
    personality?: string;
    backstory?: string;
    motivations?: string[];
    faction?: string;
    relationships?: Array<{ target?: string; rel_type?: string; description?: string }>;
    arc?: { start_state?: string; end_state?: string; internal_conflict?: string; [key: string]: unknown } | null;
    [key: string]: unknown;
  }>;
}

export interface PlotData {
  acts: Array<{
    title?: string;
    chapters: Array<{
      number: number;
      title: string;
      summary: string;
      pov_character?: string;
      plot_points?: string[];
      [key: string]: unknown;
    }>;
    [key: string]: unknown;
  }>;
  [key: string]: unknown;
}

export interface TimelineData {
  events?: Array<{ time?: string; event?: string; [key: string]: unknown }>;
  [key: string]: unknown;
}

export interface ChapterSummary {
  chapter: number;
  summary: string;
  key_events: string[];
  characters_appeared: string[];
  character_changes: Array<{ name: string; change: string }>;
  foreshadowing_planted: string[];
  foreshadowing_resolved: string[];
  settings_introduced: string[];
  end_state: string;
}

export interface ChapterContext {
  world_brief: string;
  characters: Array<{ name: string; role: string }>;
  previous_chapters: string[];
  character_states: Array<{ chapter: number; name: string; change: string }>;
  active_foreshadowing: string[];
  last_chapter_end_state: string;
}

export interface ConsistencyResult {
  issues: Array<{
    type: string;
    severity: string;
    description: string;
    chapters?: number[];
    [key: string]: unknown;
  }>;
  [key: string]: unknown;
}

export interface ReaderSimResult {
  [key: string]: unknown;
}

export interface StyleProfile {
  summary?: string;
  [key: string]: unknown;
}

export interface SensitivityResult {
  issues?: Array<{ text?: string; risk?: string; suggestion?: string; [key: string]: unknown }>;
  [key: string]: unknown;
}

export interface SnapshotInfo {
  file: string;
  timestamp: string;
  word_count: number;
}

export interface SearchMatch {
  offset: number;
  context: string;
}

export interface SearchResult {
  chapter_number: number;
  title: string;
  matches: SearchMatch[];
}

export interface AgentChatResponse {
  reply: string;
  action: { type: string; params?: Record<string, unknown> } | null;
  text?: string; // Fallback field from some LLM responses
  [key: string]: unknown;
}

export type AgentEvent =
  | { type: "token"; delta: string }
  | { type: "tool_call"; id: string; name: string; input: Record<string, unknown> }
  | { type: "tool_result"; name: string; success: boolean; result: string }
  | { type: "done"; reply: string }
  | { type: "error"; error: string };

export interface FrameworkData {
  title: string;
  genre: string;
  premise: string;
  tone: string;
  themes: string[];
  protagonist?: string;
  antagonist?: string;
  core_conflict?: string;
  world_brief?: string;
}

export interface NameGenResult {
  names?: string[];
  [key: string]: unknown;
}

export interface SyncOutlineResult {
  summary?: string;
  [key: string]: unknown;
}

export interface SkillDetail {
  id: string;
  name: string;
  files: string[];
  readme?: string;
  [key: string]: unknown;
}

// ===== Request Cancellation =====

const activeRequests = new Map<string, AbortController>();

/** Cancel a specific in-flight request by key */
export function cancelRequest(key: string): void {
  const ctrl = activeRequests.get(key);
  if (ctrl) {
    ctrl.abort();
    activeRequests.delete(key);
  }
}

/** Cancel all in-flight requests */
export function cancelAllRequests(): void {
  for (const [key, ctrl] of activeRequests) {
    ctrl.abort();
    activeRequests.delete(key);
  }
}

/**
 * Wrap a Tauri invoke call with cancellation support.
 * Returns the invoke result, but throws if the request was cancelled.
 */
async function cancellableInvoke<T>(key: string, cmd: string, args?: Record<string, unknown>): Promise<T> {
  // Cancel any previous request with the same key
  cancelRequest(key);
  const controller = new AbortController();
  activeRequests.set(key, controller);
  try {
    const result = await invoke<T>(cmd, args);
    if (controller.signal.aborted) {
      throw new Error("请求已取消");
    }
    return result;
  } finally {
    // Only delete if this controller is still the active one for this key
    if (activeRequests.get(key) === controller) {
      activeRequests.delete(key);
    }
  }
}

function llmArgs(llm: LlmParams) {
  return { apiFormat: llm.apiFormat, apiKey: llm.apiKey, model: llm.model, baseUrl: llm.baseUrl, proxyUrl: llm.proxyUrl, userAgent: llm.userAgent };
}

export const api = {
  getDataDir: () => invoke<string>("get_data_dir"),
  setDataDir: (newDir: string, migrate: boolean) =>
    invoke<string>("set_data_dir", { newDir, migrate }),
  testLlm: (apiFormat: string, apiKey: string, model: string, baseUrl: string, proxyUrl?: string, userAgent?: string) =>
    invoke<string>("test_llm", { apiFormat, apiKey, model, baseUrl, proxyUrl, userAgent }),
  fetchModels: (apiFormat: string, apiKey: string, baseUrl: string, proxyUrl?: string, userAgent?: string) =>
    invoke<string[]>("fetch_models", { apiFormat, apiKey, baseUrl, proxyUrl, userAgent }),

  saveLlmConfig: (config: LlmParams) => invoke("save_llm_config", { config }),
  getLlmConfig: () => invoke<LlmParams | null>("get_llm_config"),

  saveLlmProfiles: (profiles: Record<string, { apiKey: string; model: string; baseUrl: string }>) =>
    invoke("save_llm_profiles", { profiles }),
  getLlmProfiles: () => invoke<Record<string, { apiKey: string; model: string; baseUrl: string }>>("get_llm_profiles"),

  saveLlmProviders: (data: LlmProvidersData) => invoke("save_llm_providers", { data }),
  getLlmProviders: () => invoke<LlmProvidersData | null>("get_llm_providers"),
  // 思考等级："off" | "low" | "medium" | "high"，后端全局生效于所有写作类 LLM 调用
  setThinkingLevel: (level: string) => invoke("set_thinking_level", { level }),

  createProject: (data: {
    title: string; genre: string; premise: string;
    tone: string; themes: string[]; targetChapterWords: number;
  }) => invoke<ProjectMeta>("create_project", data),

  listProjects: () => invoke<ProjectMeta[]>("list_projects"),
  getProject: (projectId: string) => invoke<ProjectMeta>("get_project", { projectId }),
  saveOutlineSource: (projectId: string, outline: Record<string, unknown>) => invoke<OutlineSource>("save_outline_source", { projectId, outline }),
  getOutlineSource: (projectId: string) => invoke<OutlineSource>("get_outline_source", { projectId }),
  deleteProject: (projectId: string) => invoke("delete_project", { projectId }),

  generateWorld: (projectId: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke<WorldData>("generate_world", { projectId, constraints, ...llmArgs(llm) }),
  getWorld: (projectId: string) => invoke<WorldData>("get_world", { projectId }),
  saveWorldData: (projectId: string, world: WorldData) => invoke<WorldData>("save_world_data", { projectId, world }),

  generateCharacters: (projectId: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke<CharactersData>("generate_characters", { projectId, constraints, ...llmArgs(llm) }),
  getCharacters: (projectId: string) => invoke<CharactersData>("get_characters", { projectId }),
  saveCharactersData: (projectId: string, characters: CharactersData) => invoke<CharactersData>("save_characters_data", { projectId, characters }),

  generatePlot: (projectId: string, llm: LlmParams, constraints?: CreativeConstraintsPayload, targetChapters?: number) =>
    invoke<PlotData>("generate_plot", { projectId, constraints, targetChapters, ...llmArgs(llm) }),
  getPlot: (projectId: string) => invoke<PlotData>("get_plot", { projectId }),
  savePlotOutline: (projectId: string, plot: PlotData) => invoke<PlotData>("save_plot_outline", { projectId, plot }),

  generateTimeline: (projectId: string, llm: LlmParams) =>
    invoke<TimelineData>("generate_timeline", { projectId, ...llmArgs(llm) }),
  getTimeline: (projectId: string) => invoke<TimelineData>("get_timeline", { projectId }),

  expandChapter: (projectId: string, chapterNumber: number, userContent: string, targetWords: number, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    cancellableInvoke<ChapterData>("expand_chapter", "expand_chapter", { projectId, chapterNumber, userContent, targetWords, constraints, ...llmArgs(llm) }),

  continueWriting: (projectId: string, chapterNumber: number, instruction: string, targetWords: number, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    cancellableInvoke<ChapterData>("continue_writing", "continue_writing", { projectId, chapterNumber, instruction, targetWords, constraints, ...llmArgs(llm) }),

  saveChapter: (projectId: string, chapterNumber: number, text: string, snapshot?: boolean) =>
    invoke("save_chapter", { projectId, chapterNumber, text, snapshot }),
  swapChapters: (projectId: string, a: number, b: number) =>
    invoke("swap_chapters", { projectId, a, b }),
  getChapter: (projectId: string, chapterNumber: number) =>
    invoke<ChapterData>("get_chapter", { projectId, chapterNumber }),

  reviewChapter: (projectId: string, chapterNumber: number, chapterText: string, platform: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    cancellableInvoke<string>("review_chapter", "review_chapter", { projectId, chapterNumber, chapterText, platform, constraints, ...llmArgs(llm) }),

  // RAG: Chapter summaries
  summarizeChapter: (projectId: string, chapterNumber: number, llm: LlmParams) =>
    invoke<ChapterSummary>("summarize_chapter", { projectId, chapterNumber, ...llmArgs(llm) }),
  getChapterSummaries: (projectId: string) =>
    invoke<Record<string, ChapterSummary>>("get_chapter_summaries", { projectId }),
  buildChapterContext: (projectId: string, chapterNumber: number) =>
    invoke<ChapterContext>("build_chapter_context", { projectId, chapterNumber }),

  rewriteSelection: (projectId: string, chapterNumber: number, selectedText: string, instruction: string, targetDelta: number, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    cancellableInvoke<ChapterData>("rewrite_selection", "rewrite_selection", { projectId, chapterNumber, selectedText, instruction, targetDelta, constraints, ...llmArgs(llm) }),

  agentChat: (projectId: string, message: string, history: [string, string][], llm: LlmParams) =>
    cancellableInvoke<AgentChatResponse>("agent_chat", "agent_chat", { projectId, message, history, ...llmArgs(llm) }),

  agentChatStream: (projectId: string, message: string, history: Array<{role: string; content: string}>, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    invoke("agent_chat_stream", { projectId, message, history, constraints, ...llmArgs(llm) }),

  cancelAgentChat: () => invoke("cancel_agent_chat"),

  chatWithAi: (messages: [string, string][], genre: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    cancellableInvoke<string>("chat_with_ai", "chat_with_ai", { messages, genre, constraints, ...llmArgs(llm) }),

  extractFramework: (messages: [string, string][], genre: string, llm: LlmParams, constraints?: CreativeConstraintsPayload) =>
    cancellableInvoke<FrameworkData>("extract_framework", "extract_framework", { messages, genre, constraints, ...llmArgs(llm) }),

  exportNovel: (projectId: string, format: string, mode: "single" | "chapters") =>
    invoke<{ path: string; count: number }>("export_novel", { projectId, format, mode }),

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
    invoke<ConsistencyResult>("check_consistency", { projectId, ...llmArgs(llm) }),

  // Snapshots
  listChapterSnapshots: (projectId: string, chapterNumber: number) =>
    invoke<SnapshotInfo[]>("list_chapter_snapshots", { projectId, chapterNumber }),
  restoreSnapshot: (projectId: string, chapterNumber: number, snapshotFile: string) =>
    invoke<ChapterData>("restore_snapshot", { projectId, chapterNumber, snapshotFile }),

  // Search
  searchChapters: (projectId: string, query: string) =>
    invoke<SearchResult[]>("search_chapters", { projectId, query }),

  // Reader simulation & Style
  simulateReader: (projectId: string, chapterNumber: number, chapterText: string, llm: LlmParams) =>
    cancellableInvoke<ReaderSimResult>("simulate_reader", "simulate_reader", { projectId, chapterNumber, chapterText, ...llmArgs(llm) }),
  analyzeWritingStyle: (projectId: string, llm: LlmParams) =>
    invoke<StyleProfile>("analyze_writing_style", { projectId, ...llmArgs(llm) }),
  getStyleProfile: (projectId: string) =>
    invoke<StyleProfile>("get_style_profile", { projectId }),

  // Outline sync, name generator, sensitivity check
  syncOutlineFromChapter: (projectId: string, chapterNumber: number, llm: LlmParams) =>
    invoke<SyncOutlineResult>("sync_outline_from_chapter", { projectId, chapterNumber, ...llmArgs(llm) }),
  generateNames: (projectId: string, nameType: string, count: number, llm: LlmParams) =>
    invoke<NameGenResult>("generate_names", { projectId, nameType, count, ...llmArgs(llm) }),
  deepSensitivityCheck: (chapterText: string, llm: LlmParams) =>
    cancellableInvoke<SensitivityResult>("deep_sensitivity_check", "deep_sensitivity_check", { chapterText, ...llmArgs(llm) }),

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
    invoke<SkillDetail>("get_skill_detail", { skillId }),
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
