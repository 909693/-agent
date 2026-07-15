import { useState, useRef, useEffect, useCallback } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, type LlmParams, type AgentEvent } from "../api";
import { buildCreativeConstraintsPayload } from "../utils/buildCreativeConstraints";

interface AgentMessage {
  role: "user" | "assistant" | "tool";
  content: string;
  toolName?: string;
  toolSuccess?: boolean;
  streaming?: boolean;
}

interface Props {
  projectId: string;
  llm: LlmParams;
  messages: AgentMessage[];
  onMessagesChange: (msgs: AgentMessage[]) => void;
  onAction?: (action: { type: string }) => void;
}

const TOOL_LABELS: Record<string, string> = {
  get_project_info: "查看项目信息",
  get_world: "查看世界观",
  get_characters: "查看角色",
  get_plot_outline: "查看情节大纲",
  get_chapter_outline: "查看章节大纲",
  get_chapter: "查看章节",
  get_chapter_summaries: "查看章节摘要",
  search_chapters: "搜索章节",
  generate_world: "生成世界观",
  generate_characters: "生成角色",
  generate_plot: "生成情节大纲",
  expand_chapter: "扩写章节",
  continue_chapter: "续写章节",
  review_chapter: "审校章节",
  export_novel: "导出小说",
};

const WELCOME = "你好！我是你的 AI 写作助手。你可以直接告诉我你想做什么，比如：\n\n• 帮我生成完整框架（世界观 + 角色 + 大纲）\n• 把第一章扩写到 5000 字\n• 审校第三章\n• 导出小说\n• 叶辰的性格是什么？\n\n我会自动调用工具完成任务，无需手动确认。说吧，你想做什么？";

export function AgentChat({ projectId, llm, messages, onMessagesChange, onAction }: Props) {
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const chatEndRef = useRef<HTMLDivElement>(null);
  const messagesRef = useRef<AgentMessage[]>(messages);
  const streamingTextRef = useRef("");

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  const displayMsgs = messages.length === 0
    ? [{ role: "assistant" as const, content: WELCOME }]
    : messages;

  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [displayMsgs, loading]);

  const updateMessages = useCallback((updater: (prev: AgentMessage[]) => AgentMessage[]) => {
    const updated = updater(messagesRef.current);
    messagesRef.current = updated;
    onMessagesChange(updated);
  }, [onMessagesChange]);

  const handleSend = async () => {
    if (!input.trim() || loading) return;
    if (!llm.apiKey) {
      updateMessages(prev => [...prev, { role: "assistant", content: "请先在系统设置中配置 API Key。" }]);
      return;
    }

    const userMsg = input.trim();
    setInput("");
    setLoading(true);
    streamingTextRef.current = "";

    updateMessages(prev => [...prev, { role: "user", content: userMsg }]);

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listen<AgentEvent>("agent_event", (event) => {
        const data = event.payload;
        switch (data.type) {
          case "token": {
            streamingTextRef.current += data.delta;
            const text = streamingTextRef.current;
            updateMessages(prev => {
              const last = prev[prev.length - 1];
              if (last?.streaming) {
                return [...prev.slice(0, -1), { ...last, content: text }];
              }
              return [...prev, { role: "assistant", content: text, streaming: true }];
            });
            break;
          }
          case "tool_call": {
            const label = TOOL_LABELS[data.name] || data.name;
            // Finalize any streaming text first
            if (streamingTextRef.current) {
              updateMessages(prev => {
                const last = prev[prev.length - 1];
                if (last?.streaming) {
                  return [...prev.slice(0, -1), { ...last, streaming: false }];
                }
                return prev;
              });
              streamingTextRef.current = "";
            }
            updateMessages(prev => [...prev, {
              role: "tool" as const,
              content: `⏳ ${label}...`,
              toolName: data.name,
            }]);
            break;
          }
          case "tool_result": {
            const label = TOOL_LABELS[data.name] || data.name;
            const icon = data.success ? "✅" : "❌";
            const summary = data.result.length > 200 ? data.result.slice(0, 200) + "..." : data.result;
            updateMessages(prev => {
              const idx = prev.map((m, i) => ({ m, i })).reverse().find(x => x.m.role === "tool" && x.m.toolName === data.name && x.m.content.startsWith("⏳"))?.i ?? -1;
              if (idx >= 0) {
                const updated = [...prev];
                updated[idx] = {
                  ...updated[idx],
                  content: `${icon} ${label}\n${summary}`,
                  toolSuccess: data.success,
                };
                return updated;
              }
              return [...prev, {
                role: "tool",
                content: `${icon} ${label}\n${summary}`,
                toolName: data.name,
                toolSuccess: data.success,
              }];
            });
            break;
          }
          case "done": {
            // Finalize streaming text or add done message
            if (streamingTextRef.current) {
              updateMessages(prev => {
                const last = prev[prev.length - 1];
                if (last?.streaming) {
                  return [...prev.slice(0, -1), { ...last, streaming: false }];
                }
                return prev;
              });
            } else if (data.reply) {
              updateMessages(prev => [...prev, { role: "assistant", content: data.reply }]);
            }
            streamingTextRef.current = "";
            setLoading(false);
            onAction?.({ type: "agent_done" });
            break;
          }
          case "error": {
            updateMessages(prev => [...prev, { role: "assistant", content: `出错了：${data.error}` }]);
            streamingTextRef.current = "";
            setLoading(false);
            break;
          }
        }
      });

      // Build history for backend (only user/assistant messages, not tool messages)
      const history = messagesRef.current
        .filter(m => m.role === "user" || m.role === "assistant")
        .map(m => ({ role: m.role, content: m.content }));

      const constraints = await buildCreativeConstraintsPayload();
      await api.agentChatStream(projectId, userMsg, history, llm, constraints);
    } catch (e: unknown) {
      const errMsg = e instanceof Error ? e.message : String(e);
      updateMessages(prev => [...prev, { role: "assistant", content: `出错了：${errMsg}` }]);
      setLoading(false);
    } finally {
      if (unlisten) unlisten();
    }
  };

  const handleCancel = async () => {
    try {
      await api.cancelAgentChat();
    } catch {}
  };

  return (
    <div className="agent-chat">
      <div className="agent-chat-messages">
        {displayMsgs.map((msg, i) => (
          <div key={i} className={`agent-msg ${msg.role}`}>
            <div className="agent-msg-label">
              {msg.role === "assistant" ? "🤖 AI 助手" : msg.role === "tool" ? "🔧 工具" : "你"}
            </div>
            <div className={`agent-msg-content ${msg.role === "tool" ? "tool-msg" : ""} ${msg.streaming ? "streaming" : ""}`}>
              {msg.content.startsWith("⏳") && <span className="loading-spinner" />}
              <span style={{ whiteSpace: "pre-wrap" }}>{msg.content}</span>
              {msg.streaming && <span className="cursor-blink">▍</span>}
            </div>
          </div>
        ))}
        {loading && !displayMsgs.some(m => m.streaming) && !displayMsgs.some(m => m.content?.startsWith("⏳")) && (
          <div className="agent-msg assistant">
            <div className="agent-msg-label">🤖 AI 助手</div>
            <div className="agent-msg-content"><span className="loading-spinner" />思考中...</div>
          </div>
        )}
        <div ref={chatEndRef} />
      </div>
      <div className="agent-chat-input">
        <textarea
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void handleSend(); } }}
          placeholder="告诉我你想做什么..."
          rows={2}
          disabled={loading}
        />
        {loading ? (
          <button onClick={() => void handleCancel()} className="btn-cancel">取消</button>
        ) : (
          <button onClick={() => void handleSend()} disabled={!input.trim()}>发送</button>
        )}
      </div>
    </div>
  );
}
