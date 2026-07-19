import { useRef, useEffect, useState } from "react";
import { api, type LlmParams } from "../api";
import { CreativeConstraintsPanel } from "./CreativeConstraintsPanel";
import { buildCreativeConstraintsPayload } from "../utils/buildCreativeConstraints";

interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

interface ChatDraft {
  genre: string | null;
  messages: ChatMessage[];
  input: string;
  frameworkReady: boolean;
  error: string;
}

interface Props {
  llm: LlmParams;
  draft: ChatDraft;
  onDraftChange: (draft: ChatDraft) => void;
  onProjectCreated: (project: any) => void;
  onCancel: () => void;
}

const GENRES = [
  { value: "fantasy", label: "玄幻/仙侠", desc: "东方幻想，修炼问道" },
  { value: "scifi", label: "科幻", desc: "未来世界，技术遐想" },
  { value: "urban", label: "都市", desc: "现代都市，百态人生" },
  { value: "romance", label: "言情", desc: "心动瞬间，情感纠葛" },
  { value: "mystery", label: "悬疑", desc: "迷雾重重，真相待解" },
  { value: "history", label: "历史", desc: "长河回望，风云人物" },
  { value: "horror", label: "恐怖", desc: "暗夜低语，心跳加速" },
  { value: "other", label: "其他", desc: "不拘一格，自由发挥" },
];

export function ChatCreator({ llm, draft, onDraftChange, onProjectCreated, onCancel }: Props) {
  const [loading, setLoading] = useState(false);
  const [extracting, setExtracting] = useState(false);
  const chatEndRef = useRef<HTMLDivElement>(null);
  const draftRef = useRef(draft);
  draftRef.current = draft; // Always keep ref in sync with latest draft
  const { genre, messages, input, frameworkReady, error } = draft;

  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, loading]);

  // Use ref-based updater to avoid stale closures in async callbacks
  const updateDraft = (patch: Partial<ChatDraft>) => onDraftChange({ ...draftRef.current, ...patch });

  const handleGenreSelect = async (g: string) => {
    if (!llm.apiKey) { updateDraft({ error: "请先在系统设置中配置 API Key" }); return; }
    updateDraft({ genre: g, error: "" });
    setLoading(true);
    try {
      const payload = await buildCreativeConstraintsPayload(g || undefined);
      const reply = await api.chatWithAi([], g, llm, payload);
      updateDraft({ genre: g, messages: [{ role: "assistant", content: reply }], frameworkReady: false, error: "", input: "" });
    } catch (e: any) { updateDraft({ error: e.toString() }); }
    setLoading(false);
  };

  const handleSend = async () => {
    if (!input.trim() || loading || !genre) return;
    const userMsg = input.trim();
    const newMessages: ChatMessage[] = [...draftRef.current.messages, { role: "user", content: userMsg }];
    updateDraft({ messages: newMessages, input: "", error: "" });
    setLoading(true);
    try {
      const apiMessages: [string, string][] = newMessages.map(m => [m.role, m.content]);
      const payload = await buildCreativeConstraintsPayload(genre || undefined);
      const reply = await api.chatWithAi(apiMessages, genre, llm, payload);
      const isReady = reply.includes("[FRAMEWORK_READY]");
      const cleanReply = reply.replace("[FRAMEWORK_READY]", "").trim();
      updateDraft({ messages: [...newMessages, { role: "assistant", content: cleanReply }], frameworkReady: isReady || draftRef.current.frameworkReady, error: "" });
    } catch (e: any) { updateDraft({ error: e.toString() }); }
    setLoading(false);
  };

  const handleExtract = async () => {
    if (!genre) return;
    setExtracting(true);
    updateDraft({ error: "" });
    try {
      const apiMessages: [string, string][] = messages.map(m => [m.role, m.content]);
      const payload = await buildCreativeConstraintsPayload(genre || undefined);
      const framework = await api.extractFramework(apiMessages, genre, llm, payload);
      const project = await api.createProject({
        title: framework.title || "未命名小说",
        genre: framework.genre || genre,
        premise: framework.premise || "",
        tone: framework.tone || "",
        themes: framework.themes || [],
        targetChapterWords: 3000,
      });
      onProjectCreated(project);
    } catch (e: any) { updateDraft({ error: e.toString() }); }
    setExtracting(false);
  };

  if (!genre) {
    return (
      <div className="genre-select">
        <div className="genre-hero">
          <h2>想写什么类型的小说？</h2>
          <p className="dim">选择一个类型，AI 策划师帮你把灵感变成故事</p>
        </div>
        {error && <div className="error">{error}</div>}
        <div className="genre-grid">
          {GENRES.map(g => (
            <button key={g.value} className="genre-btn" onClick={() => handleGenreSelect(g.value)}>
              <span className="genre-name">{g.label}</span>
              <span className="genre-desc">{g.desc}</span>
            </button>
          ))}
        </div>
        <button className="cancel-btn genre-cancel" onClick={onCancel}>取消</button>
      </div>
    );
  }

  return (
    <div className="chat-creator">
      <div className="chat-header">
        <div className="chat-header-info">
          <span className="chat-header-title">AI 策划师</span>
          <span className="chat-header-genre">{GENRES.find(g => g.value === genre)?.label}</span>
        </div>
        <button className="cancel-btn" onClick={onCancel}>取消</button>
      </div>
      <div className="chat-constraints-wrap">
        <CreativeConstraintsPanel collapsible defaultCollapsed />
      </div>
      {error && <div className="error" style={{ margin: "0 20px" }}>{error}</div>}
      <div className="chat-messages">
        <div className="chat-messages-inner">
          {messages.map((msg, i) => (
            <div key={i} className={`chat-msg ${msg.role}`}>
              <div className="msg-label">{msg.role === "assistant" ? "AI 策划师" : "你"}</div>
              <div className="msg-content">{msg.content}</div>
            </div>
          ))}
          {loading && (
            <div className="chat-msg assistant">
              <div className="msg-label">AI 策划师</div>
              <div className="msg-content typing">思考中...</div>
            </div>
          )}
          <div ref={chatEndRef} />
        </div>
      </div>
      <div className="chat-input-area">
        <div className="chat-input-inner">
          {frameworkReady && (
            <button className="extract-btn" onClick={handleExtract} disabled={extracting}>
              {extracting ? "正在生成框架..." : "生成故事框架并创建项目"}
            </button>
          )}
          <div className="chat-input-row">
            <textarea
              value={input}
              onChange={e => updateDraft({ input: e.target.value })}
              onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void handleSend(); } }}
              placeholder="说说你的想法..."
              rows={2}
              disabled={loading}
            />
            <button onClick={() => void handleSend()} disabled={loading || !input.trim()}>发送</button>
          </div>
        </div>
      </div>
    </div>
  );
}
