import { useState, useRef, useEffect } from "react";
import { api, type LlmParams } from "../api";
import { buildCreativeConstraintsPayload } from "../utils/buildCreativeConstraints";

interface AgentMessage {
  role: "user" | "assistant";
  content: string;
  action?: any;
  executing?: boolean;
  done?: boolean;
}

interface Props {
  projectId: string;
  llm: LlmParams;
  messages: AgentMessage[];
  onMessagesChange: (msgs: AgentMessage[]) => void;
  onAction?: (action: any) => void;
}

const WELCOME = "你好！我是你的 AI 写作助手。你可以直接告诉我你想做什么，比如：\n\n• 帮我生成完整框架\n• 把第一章扩写到 5000 字\n• 审校第三章\n• 导出小说\n• 叶辰的性格是什么？\n\n说吧，你想做什么？";

export function AgentChat({ projectId, llm, messages, onMessagesChange, onAction }: Props) {
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [executingAction, setExecutingAction] = useState(false);
  const chatEndRef = useRef<HTMLDivElement>(null);

  const msgs = messages.length === 0
    ? [{ role: "assistant" as const, content: WELCOME }]
    : messages;

  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [msgs, loading, executingAction]);

  const addMsg = (msg: AgentMessage) => {
    const updated = [...msgs, msg];
    onMessagesChange(updated);
    return updated;
  };

  const handleSend = async () => {
    if (!input.trim() || loading) return;
    if (!llm.apiKey) { addMsg({ role: "assistant", content: "请先在系统设置中配置 API Key。" }); return; }
    const userMsg = input.trim();
    setInput("");
    let current = addMsg({ role: "user", content: userMsg });
    setLoading(true);

    try {
      const history: [string, string][] = current.map(m => [m.role, m.content]);
      const result = await api.agentChat(projectId, userMsg, history, llm);
      const reply = result.reply || result.text || JSON.stringify(result);
      const action = result.action || null;

      // Check for multi-step
      if (action?.type === "generate_all") {
        current = [...current, { role: "assistant", content: reply + "\n\n开始自动执行完整框架生成...", action: null }];
        onMessagesChange(current);
        await executeMultiStep(["generate_world", "generate_characters", "generate_plot"], current);
      } else {
        current = [...current, { role: "assistant", content: reply, action }];
        onMessagesChange(current);
        // Auto-execute if action exists
        if (action) {
          await executeOneAction(action, current);
        }
      }
    } catch (e: any) {
      addMsg({ role: "assistant", content: `出错了：${e.toString()}` });
    }
    setLoading(false);
  };

  const executeMultiStep = async (steps: string[], current: AgentMessage[]) => {
    setExecutingAction(true);
    const payload = await buildCreativeConstraintsPayload();
    for (const step of steps) {
      current = [...current, { role: "assistant", content: `正在执行：${stepLabel(step)}...`, executing: true }];
      onMessagesChange(current);
      try {
        await executeStep(step, payload);
        current = current.map((m, i) => i === current.length - 1 ? { ...m, content: `✅ ${stepLabel(step)} 完成`, executing: false, done: true } : m);
        onMessagesChange(current);
      } catch (e: any) {
        current = current.map((m, i) => i === current.length - 1 ? { ...m, content: `❌ ${stepLabel(step)} 失败：${e.toString()}`, executing: false } : m);
        onMessagesChange(current);
        break;
      }
    }
    current = [...current, { role: "assistant", content: "框架生成完毕！你可以去章节管理页查看世界观、角色和情节大纲。" }];
    onMessagesChange(current);
    setExecutingAction(false);
    onAction?.({ type: "generate_all" });
  };

  const executeOneAction = async (action: any, current: AgentMessage[]) => {
    setExecutingAction(true);
    const payload = await buildCreativeConstraintsPayload();
    current = [...current, { role: "assistant", content: `正在执行：${stepLabel(action.type)}...`, executing: true }];
    onMessagesChange(current);
    try {
      const resultMsg = await runAction(action, payload);
      current = current.map((m, i) => i === current.length - 1 ? { ...m, content: `✅ ${resultMsg}`, executing: false, done: true } : m);
      onMessagesChange(current);
    } catch (e: any) {
      current = current.map((m, i) => i === current.length - 1 ? { ...m, content: `❌ 执行失败：${e.toString()}`, executing: false } : m);
      onMessagesChange(current);
    }
    setExecutingAction(false);
    onAction?.(action);
  };

  const executeStep = async (type: string, payload: any) => {
    switch (type) {
      case "generate_world": await api.generateWorld(projectId, llm, payload); break;
      case "generate_characters": await api.generateCharacters(projectId, llm, payload); break;
      case "generate_plot": await api.generatePlot(projectId, llm, payload); break;
      default: throw new Error(`Unknown step: ${type}`);
    }
  };

  const runAction = async (action: any, payload: any): Promise<string> => {
    switch (action.type) {
      case "generate_world": await api.generateWorld(projectId, llm, payload); return "世界观已生成！";
      case "generate_characters": await api.generateCharacters(projectId, llm, payload); return "角色已生成！";
      case "generate_plot": await api.generatePlot(projectId, llm, payload); return "情节大纲已生成！";
      case "expand_chapter": {
        const ch = action.params?.chapter || 1;
        const words = action.params?.target_words || 3000;
        await api.expandChapter(projectId, ch, action.params?.hint || "", words, llm, payload);
        return `第${ch}章已扩写完成（目标 ${words} 字）！`;
      }
      case "continue_chapter": {
        const ch = action.params?.chapter || 1;
        const words = action.params?.target_words || 1000;
        await api.continueWriting(projectId, ch, action.params?.instruction || "", words, llm, payload);
        return `第${ch}章续写完成！`;
      }
      case "review_chapter": {
        const ch = action.params?.chapter || 1;
        const platform = action.params?.platform || "番茄";
        let chText = "";
        try { const d: any = await api.getChapter(projectId, ch); chText = d.text || ""; } catch {}
        if (!chText) return `第${ch}章还没有内容，无法审校。`;
        const review = await api.reviewChapter(projectId, ch, chText, platform, llm, payload);
        return `第${ch}章审校报告：\n\n${review}`;
      }
      case "export": { const path = await api.exportNovel(projectId, "txt"); return `小说已导出到：${path}`; }
      case "show_characters": {
        const chars: any = await api.getCharacters(projectId);
        const names = (chars?.characters || []).map((c: any) => `${c.name}（${c.role}）`).join("、");
        return `当前角色：${names || "暂无角色"}`;
      }
      case "show_world": {
        const w: any = await api.getWorld(projectId);
        return `世界观：${w?.era || "未生成"}\n${w?.overview || ""}`;
      }
      case "show_plot": {
        const p: any = await api.getPlot(projectId);
        const chCount = (p?.acts || []).reduce((sum: number, a: any) => sum + (a.chapters?.length || 0), 0);
        return `情节大纲：共 ${(p?.acts || []).length} 幕，${chCount} 章`;
      }
      case "show_chapter": {
        const ch = action.params?.chapter || 1;
        try {
          const d: any = await api.getChapter(projectId, ch);
          const t = d.text || "";
          return `第${ch}章（${t.length} 字）：\n\n${t.slice(0, 500)}${t.length > 500 ? "..." : ""}`;
        } catch { return `第${ch}章还没有内容。`; }
      }
      default: return `未知动作：${action.type}`;
    }
  };

  const stepLabel = (type: string) => ({
    generate_world: "生成世界观",
    generate_characters: "生成角色",
    generate_plot: "生成情节大纲",
    expand_chapter: "扩写章节",
    continue_chapter: "续写章节",
    review_chapter: "审校章节",
    export: "导出小说",
  } as Record<string, string>)[type] || type;

  return (
    <div className="agent-chat">
      <div className="agent-chat-messages">
        {msgs.map((msg, i) => (
          <div key={i} className={`agent-msg ${msg.role}`}>
            <div className="agent-msg-label">{msg.role === "assistant" ? "🤖 AI 助手" : "你"}</div>
            <div className={`agent-msg-content ${msg.executing ? "executing" : ""} ${msg.done ? "done" : ""}`}>
              {msg.executing && <span className="loading-spinner" />}
              {msg.content}
            </div>
          </div>
        ))}
        {loading && (
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
          disabled={loading || executingAction}
        />
        <button onClick={() => void handleSend()} disabled={loading || executingAction || !input.trim()}>发送</button>
      </div>
    </div>
  );
}
