import { useEffect, useRef, useState } from "react";
import { api, type TomatoConfig } from "../api";

interface Props {
  projectId: string;
  chapterNumber: number;
  chapterTitle: string;
  chapterWords: number;
  onClose: () => void;
}

interface NovelOption {
  bookId: string;
  name: string;
}

const EMPTY_CONFIG: TomatoConfig = {
  nodePath: "",
  script: "",
  cookie: "",
  csrfToken: "",
  defaultBookId: "",
  useAi: true,
};

// MCP list_novels 返回可读文本,形如「- 《书名》 · book_id=123 · 4556 字」
function parseNovels(raw: string): NovelOption[] {
  const out: NovelOption[] = [];
  for (const line of raw.split("\n")) {
    const m = line.match(/《(.+?)》.*?book_id=([^\s·]+)/);
    if (m) out.push({ name: m[1], bookId: m[2] });
  }
  return out;
}

export function PublishDialog({ projectId, chapterNumber, chapterTitle, chapterWords, onClose }: Props) {
  // 大纲标题一般不带「第N章」前缀,带了就不重复拼
  const defaultTitle = /^第.{1,6}[章回]/.test(chapterTitle)
    ? chapterTitle
    : `第${chapterNumber}章 ${chapterTitle}`.trim();

  const [config, setConfig] = useState<TomatoConfig>(EMPTY_CONFIG);
  const [configLoaded, setConfigLoaded] = useState(false);
  const [showConfig, setShowConfig] = useState(false);
  const [configStatus, setConfigStatus] = useState<"idle" | "saving" | "saved">("idle");

  const [novels, setNovels] = useState<NovelOption[]>([]);
  const [novelsRaw, setNovelsRaw] = useState("");
  const [loadingNovels, setLoadingNovels] = useState(false);

  const [bookId, setBookId] = useState("");
  const [title, setTitle] = useState(defaultTitle);
  const [publishTime, setPublishTime] = useState("");
  const [useAi, setUseAi] = useState(true);

  const [busy, setBusy] = useState<"" | "preview" | "publish">("");
  const [armed, setArmed] = useState(false);
  const [result, setResult] = useState("");
  const [published, setPublished] = useState(false);
  const [error, setError] = useState("");
  const armTimer = useRef<number | null>(null);

  useEffect(() => {
    api.getTomatoConfig().then((c) => {
      if (c) {
        setConfig({ ...EMPTY_CONFIG, ...c });
        setBookId(c.defaultBookId || "");
        setUseAi(c.useAi !== false);
        if (!c.script || !c.cookie || !c.csrfToken) setShowConfig(true);
      } else {
        // 首次使用:预填本机 tomato-writer-mcp 的默认位置
        setConfig({ ...EMPTY_CONFIG, script: "/Users/wuwei/Documents/mcp/dist/index.js" });
        setShowConfig(true);
      }
      setConfigLoaded(true);
    }).catch((e) => { setError(String(e)); setConfigLoaded(true); setShowConfig(true); });
    return () => { if (armTimer.current) window.clearTimeout(armTimer.current); };
  }, []);

  const disarm = () => {
    setArmed(false);
    if (armTimer.current) { window.clearTimeout(armTimer.current); armTimer.current = null; }
  };

  const handleSaveConfig = async () => {
    setError("");
    setConfigStatus("saving");
    try {
      await api.saveTomatoConfig({ ...config, defaultBookId: bookId, useAi });
      setConfigStatus("saved");
      setTimeout(() => setConfigStatus((s) => (s === "saved" ? "idle" : s)), 2500);
    } catch (e: any) {
      setConfigStatus("idle");
      setError(String(e));
    }
  };

  const handleLoadNovels = async () => {
    setError("");
    setLoadingNovels(true);
    try {
      const raw = await api.tomatoListNovels();
      setNovelsRaw(raw);
      const list = parseNovels(raw);
      setNovels(list);
      if (list.length > 0 && !list.some((n) => n.bookId === bookId)) {
        setBookId(list[0].bookId);
      }
      if (list.length === 0) {
        setError("未解析到书目，原始返回见下方结果区");
        setResult(raw);
      }
    } catch (e: any) {
      setError(String(e));
    }
    setLoadingNovels(false);
  };

  const doPublish = async (dryRun: boolean) => {
    setError("");
    setResult("");
    setBusy(dryRun ? "preview" : "publish");
    try {
      const text = await api.tomatoPublishChapter(projectId, chapterNumber, {
        bookId: bookId || undefined,
        title: title.trim(),
        publishTime: publishTime ? publishTime.replace("T", " ") : undefined,
        useAi,
        dryRun,
      });
      setResult(text);
      if (!dryRun) {
        setPublished(true);
        // 记住本次的书目与 AI 申报选择,下次直接带出
        api.saveTomatoConfig({ ...config, defaultBookId: bookId, useAi }).catch(() => {});
      }
    } catch (e: any) {
      setError(String(e));
    }
    setBusy("");
    disarm();
  };

  const handlePublishClick = () => {
    if (!armed) {
      setArmed(true);
      armTimer.current = window.setTimeout(() => setArmed(false), 5000);
      return;
    }
    disarm();
    void doPublish(false);
  };

  const labelStyle = { fontSize: 13, color: "var(--text-secondary)", marginBottom: 4 } as const;
  const inputStyle = {
    width: "100%", padding: "7px 10px", border: "1px solid var(--border)",
    borderRadius: "var(--radius)", fontSize: 13, fontFamily: "inherit", boxSizing: "border-box",
  } as const;

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" style={{ width: 560, maxHeight: "85vh", overflowY: "auto" }} onClick={(e) => e.stopPropagation()}>
        <h3 style={{ fontSize: 16, fontWeight: 600, marginBottom: 4 }}>发布到番茄小说</h3>
        <div className="dim" style={{ fontSize: 13, marginBottom: 16 }}>
          第{chapterNumber}章 · {chapterWords} 字 · 经本机 tomato-writer-mcp 提交到番茄作家后台
        </div>

        {/* 鉴权与 MCP 配置 */}
        <div style={{ marginBottom: 16 }}>
          <button className="btn-sm" onClick={() => setShowConfig(!showConfig)}>
            {showConfig ? "收起配置 ▲" : "发布配置（MCP 路径 / 鉴权）▼"}
          </button>
          {showConfig && (
            <div style={{ marginTop: 10, padding: 12, border: "1px solid var(--border)", borderRadius: "var(--radius)", display: "flex", flexDirection: "column", gap: 10 }}>
              <div>
                <div style={labelStyle}>MCP 脚本路径（tomato-writer-mcp/dist/index.js）</div>
                <input style={inputStyle} value={config.script} placeholder="/Users/you/Documents/mcp/dist/index.js"
                  onChange={(e) => setConfig({ ...config, script: e.target.value })} />
              </div>
              <div>
                <div style={labelStyle}>node 路径（留空自动从 PATH / Homebrew 查找）</div>
                <input style={inputStyle} value={config.nodePath} placeholder="node"
                  onChange={(e) => setConfig({ ...config, nodePath: e.target.value })} />
              </div>
              <div>
                <div style={labelStyle}>番茄作家后台 Cookie（浏览器登录后从 /api/author/ 请求头复制）</div>
                <input type="password" style={inputStyle} value={config.cookie} placeholder="粘贴完整 Cookie 头"
                  onChange={(e) => setConfig({ ...config, cookie: e.target.value })} />
              </div>
              <div>
                <div style={labelStyle}>X-Secsdk-Csrf-Token</div>
                <input type="password" style={inputStyle} value={config.csrfToken} placeholder="同一请求头里的 X-Secsdk-Csrf-Token"
                  onChange={(e) => setConfig({ ...config, csrfToken: e.target.value })} />
              </div>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <button className="btn-primary" onClick={handleSaveConfig} disabled={configStatus === "saving"}>
                  {configStatus === "saving" ? <><span className="loading-spinner" />保存中...</> : "保存配置"}
                </button>
                {configStatus === "saved" && <span className="saved-tag">✓ 已保存（Cookie 加密落盘）</span>}
              </div>
            </div>
          )}
        </div>

        {/* 目标书目 */}
        <div style={{ marginBottom: 14 }}>
          <div style={labelStyle}>目标书目</div>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            {novels.length > 0 ? (
              <select style={{ ...inputStyle, flex: 1 }} value={bookId} onChange={(e) => { setBookId(e.target.value); disarm(); }}>
                {novels.map((n) => (
                  <option key={n.bookId} value={n.bookId}>《{n.name}》（{n.bookId}）</option>
                ))}
              </select>
            ) : (
              <input style={{ ...inputStyle, flex: 1 }} value={bookId} placeholder="book_id（留空则用 MCP 当前选中/账号第一本）"
                onChange={(e) => { setBookId(e.target.value); disarm(); }} />
            )}
            <button className="btn-outline" onClick={handleLoadNovels} disabled={loadingNovels || !configLoaded}>
              {loadingNovels ? <><span className="loading-spinner" />加载中</> : novels.length > 0 ? "刷新书目" : "加载书目"}
            </button>
          </div>
          {novelsRaw && novels.length > 0 && (
            <div className="dim" style={{ fontSize: 12, marginTop: 4 }}>共 {novels.length} 本</div>
          )}
        </div>

        {/* 章节标题 */}
        <div style={{ marginBottom: 14 }}>
          <div style={labelStyle}>发布标题</div>
          <input style={inputStyle} value={title} onChange={(e) => { setTitle(e.target.value); disarm(); }} />
        </div>

        {/* 定时与 AI 申报 */}
        <div style={{ display: "flex", gap: 16, alignItems: "center", flexWrap: "wrap", marginBottom: 14 }}>
          <div>
            <div style={labelStyle}>定时发布（留空立即发布）</div>
            <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <input type="datetime-local" style={inputStyle} value={publishTime}
                onChange={(e) => { setPublishTime(e.target.value); disarm(); }} />
              {publishTime && <button className="btn-sm" onClick={() => { setPublishTime(""); disarm(); }}>清除</button>}
            </div>
          </div>
          <label style={{ display: "flex", gap: 6, alignItems: "center", fontSize: 13, cursor: "pointer", marginTop: 18 }}>
            <input type="checkbox" checked={useAi} onChange={(e) => { setUseAi(e.target.checked); disarm(); }} />
            申报「使用 AI 创作」
          </label>
        </div>

        {error && <div className="error" style={{ whiteSpace: "pre-wrap" }}>{error}</div>}
        {result && (
          <div style={{
            background: published ? "var(--success-light)" : "var(--bg-secondary, rgba(127,127,127,0.08))",
            border: `1px solid ${published ? "var(--success)" : "var(--border)"}`,
            borderRadius: "var(--radius)", padding: "10px 14px", marginBottom: 14,
            fontSize: 13, whiteSpace: "pre-wrap", wordBreak: "break-all",
          }}>{result}</div>
        )}

        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", alignItems: "center" }}>
          {armed && <span className="dim" style={{ fontSize: 12 }}>再点一次确认提交，5 秒后自动取消</span>}
          <button className="btn-outline" onClick={onClose}>关闭</button>
          {!published && (
            <>
              <button className="btn-outline" onClick={() => void doPublish(true)} disabled={!!busy || !title.trim()}>
                {busy === "preview" ? <><span className="loading-spinner" />预览中...</> : "预览（不提交）"}
              </button>
              <button className={armed ? "btn-danger" : "btn-primary"} onClick={handlePublishClick} disabled={!!busy || !title.trim()}>
                {busy === "publish"
                  ? <><span className="loading-spinner" />提交中...</>
                  : armed ? "确认发布" : publishTime ? "定时发布" : "立即发布"}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
