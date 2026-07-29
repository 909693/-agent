import { useEffect, useRef, useState } from "react";
import { api, type ProjectMeta, type TomatoConfig } from "../api";

interface Props {
  project: ProjectMeta;
  onClose: () => void;
  /** 建书成功后回调(带解析出的 book_id),供发布弹窗接力选中新书 */
  onCreated?: (bookId: string) => void;
}

interface CategoryOption {
  id: number;
  name: string;
  description: string;
}

const EMPTY_CONFIG: TomatoConfig = {
  nodePath: "",
  script: "",
  cookie: "",
  csrfToken: "",
  defaultBookId: "",
  defaultThumbUri: "",
  useAi: true,
};

const genreLabels: Record<string, string> = {
  fantasy: "玄幻", scifi: "科幻", urban: "都市", romance: "言情",
  mystery: "悬疑", history: "历史", horror: "恐怖", other: "其他",
};

// 根据书名/类型/频道/简介拼一个默认封面提示词
function defaultCoverPrompt(name: string, genre: string, gender: "male" | "female", abstract: string): string {
  const g = genreLabels[genre] || genre;
  const channel = gender === "female" ? "女频" : "男频";
  const brief = abstract.trim().replace(/\s+/g, "").slice(0, 60);
  return `网络小说竖版封面插画，${channel}${g}题材，书名《${name}》。${brief}。氛围恢弘，主角居中，画面精致有质感，电影级光影，高清，无多余文字水印。`;
}

// MCP list_categories 返回含 ```json 代码块的文本,提取出分类数组
function parseCategories(raw: string): CategoryOption[] {
  const m = raw.match(/```json\s*([\s\S]*?)```/);
  if (!m) return [];
  try {
    const arr = JSON.parse(m[1]);
    if (!Array.isArray(arr)) return [];
    return arr
      .filter((c: any) => c && typeof c.name === "string")
      .map((c: any) => ({
        id: Number(c.category_id) || 0,
        name: c.name,
        description: typeof c.description === "string" ? c.description : "",
      }));
  } catch {
    return [];
  }
}

export function TomatoCreateDialog({ project, onClose, onCreated }: Props) {
  const [config, setConfig] = useState<TomatoConfig>(EMPTY_CONFIG);
  const [configLoaded, setConfigLoaded] = useState(false);
  const [showConfig, setShowConfig] = useState(false);
  const [configStatus, setConfigStatus] = useState<"idle" | "saving" | "saved">("idle");

  // 书名 ≤15 字、简介 ≥50 字,预填项目的标题与故事前提
  const [bookName, setBookName] = useState(project.title.slice(0, 15));
  const [abstractText, setAbstractText] = useState(project.premise || "");
  // 言情默认女频,其余默认男频
  const [gender, setGender] = useState<"male" | "female">(project.genre === "romance" ? "female" : "male");
  const [category, setCategory] = useState("");
  const [protagonist, setProtagonist] = useState("");
  const [thumbUri, setThumbUri] = useState("");

  // AI 封面生成
  const [coverPrompt, setCoverPrompt] = useState(() =>
    defaultCoverPrompt(project.title.slice(0, 15), project.genre, project.genre === "romance" ? "female" : "male", project.premise || ""));
  const [coverBusy, setCoverBusy] = useState(false);
  const [coverError, setCoverError] = useState("");
  const [coverPreview, setCoverPreview] = useState("");
  const [showCoverPanel, setShowCoverPanel] = useState(false);

  const [categories, setCategories] = useState<CategoryOption[]>([]);
  const [loadingCategories, setLoadingCategories] = useState(false);

  const [busy, setBusy] = useState(false);
  const [armed, setArmed] = useState(false);
  const [result, setResult] = useState("");
  const [createdBookId, setCreatedBookId] = useState("");
  const [error, setError] = useState("");
  const armTimer = useRef<number | null>(null);
  const resultEndRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    api.getTomatoConfig().then((c) => {
      if (c) {
        setConfig({ ...EMPTY_CONFIG, ...c });
        setThumbUri(c.defaultThumbUri || "");
        if (!c.script || !c.cookie || !c.csrfToken) setShowConfig(true);
      } else {
        setConfig({ ...EMPTY_CONFIG, script: "/Users/wuwei/Documents/mcp/dist/index.js" });
        setShowConfig(true);
      }
      setConfigLoaded(true);
    }).catch((e) => { setError(String(e)); setConfigLoaded(true); setShowConfig(true); });
    return () => { if (armTimer.current) window.clearTimeout(armTimer.current); };
  }, []);

  useEffect(() => {
    if (result || error) resultEndRef.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }, [result, error]);

  const disarm = () => {
    setArmed(false);
    if (armTimer.current) { window.clearTimeout(armTimer.current); armTimer.current = null; }
  };

  const handleSaveConfig = async () => {
    setError("");
    setConfigStatus("saving");
    try {
      await api.saveTomatoConfig({ ...config, defaultThumbUri: thumbUri });
      setConfigStatus("saved");
      setTimeout(() => setConfigStatus((s) => (s === "saved" ? "idle" : s)), 2500);
    } catch (e: any) {
      setConfigStatus("idle");
      setError(String(e));
    }
  };

  const handleGenerateCover = async () => {
    if (!coverPrompt.trim()) { setCoverError("提示词不能为空"); return; }
    setCoverError("");
    setCoverBusy(true);
    try {
      const { picUri, picUrl, picData } = await api.generateCover(coverPrompt.trim());
      setThumbUri(picUri);
      // 预览优先用本地 data URL(绕开番茄 CDN 防盗链),缺失时回退远程 url。
      setCoverPreview(picData || picUrl);
      disarm();
      // 顺手记住封面 uri,下次带出
      api.saveTomatoConfig({ ...config, defaultThumbUri: picUri }).catch(() => {});
    } catch (e: any) {
      setCoverError(String(e));
    }
    setCoverBusy(false);
  };

  const handleLoadCategories = async () => {
    setError("");
    setLoadingCategories(true);
    try {
      const raw = await api.tomatoListCategories();
      const list = parseCategories(raw);
      setCategories(list);
      if (list.length === 0) {
        setError("未解析到分类，原始返回见下方结果区");
        setResult(raw);
      }
    } catch (e: any) {
      setError(String(e));
    }
    setLoadingCategories(false);
  };

  const nameLen = bookName.trim().length;
  const abstractLen = abstractText.trim().length;
  const formValid = nameLen > 0 && nameLen <= 15 && abstractLen >= 50 && thumbUri.trim().length > 0;

  const doCreate = async () => {
    setError("");
    setResult("");
    setBusy(true);
    try {
      const text = await api.tomatoCreateBook({
        bookName: bookName.trim(),
        abstractText: abstractText.trim(),
        thumbUri: thumbUri.trim(),
        gender,
        category: category || undefined,
        protagonist: protagonist.split(/[,，、]/).map((s) => s.trim()).filter(Boolean),
      });
      setResult(text);
      // MCP 成功文案形如「已创建《书名》(book_id=xxx)。」,解析出 book_id
      const m = text.match(/book_id=([^\s)）,，。]+)/);
      const bookId = m ? m[1] : "";
      setCreatedBookId(bookId || "created");
      if (bookId) onCreated?.(bookId);
      // 记住封面 uri 与新书 book_id,发布弹窗直接带出
      api.saveTomatoConfig({
        ...config,
        defaultThumbUri: thumbUri.trim(),
        ...(bookId ? { defaultBookId: bookId } : {}),
      }).catch(() => {});
    } catch (e: any) {
      setError(String(e));
    }
    setBusy(false);
    disarm();
  };

  const handleCreateClick = () => {
    if (!armed) {
      setArmed(true);
      armTimer.current = window.setTimeout(() => setArmed(false), 5000);
      return;
    }
    disarm();
    void doCreate();
  };

  const labelStyle = { fontSize: 13, color: "var(--text-secondary)", marginBottom: 4 } as const;
  const inputStyle = {
    width: "100%", padding: "7px 10px", border: "1px solid var(--border)",
    borderRadius: "var(--radius)", fontSize: 13, fontFamily: "inherit", boxSizing: "border-box",
  } as const;
  const counterStyle = (ok: boolean) =>
    ({ fontSize: 12, marginTop: 3, color: ok ? "var(--text-secondary)" : "var(--danger, #d33)" }) as const;

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        className="modal modal-solid"
        style={{ width: 560, maxHeight: "85vh", display: "flex", flexDirection: "column" }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* 固定头部 */}
        <div style={{ padding: "20px 24px 12px", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
          <h3 style={{ fontSize: 16, fontWeight: 600, margin: 0 }}>在番茄创建新书</h3>
          <div className="dim" style={{ fontSize: 13, marginTop: 4 }}>
            《{project.title}》 · 经本机 tomato-writer-mcp 在番茄作家后台建书，建成后可直接发布章节
          </div>
        </div>

        {/* 可滚动内容区 */}
        <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "16px 24px" }}>
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
                  {configStatus === "saved" && <span className="saved-tag">✓ 已保存（Cookie 加密落盘，并登记到 MCP 管理）</span>}
                </div>
              </div>
            )}
          </div>

          {/* 书名 */}
          <div style={{ marginBottom: 14 }}>
            <div style={labelStyle}>书名（番茄要求不超过 15 字）</div>
            <input style={inputStyle} value={bookName} onChange={(e) => { setBookName(e.target.value); disarm(); }} />
            <div style={counterStyle(nameLen > 0 && nameLen <= 15)}>{nameLen}/15 字</div>
          </div>

          {/* 简介 */}
          <div style={{ marginBottom: 14 }}>
            <div style={labelStyle}>作品简介（番茄要求至少 50 字）</div>
            <textarea style={{ ...inputStyle, resize: "vertical" }} rows={4} value={abstractText}
              onChange={(e) => { setAbstractText(e.target.value); disarm(); }} />
            <div style={counterStyle(abstractLen >= 50)}>{abstractLen} 字{abstractLen < 50 ? `（还差 ${50 - abstractLen} 字）` : ""}</div>
          </div>

          {/* 频道与分类 */}
          <div style={{ display: "flex", gap: 12, marginBottom: 14 }}>
            <div style={{ width: 120 }}>
              <div style={labelStyle}>频道</div>
              <select style={inputStyle} value={gender} onChange={(e) => { setGender(e.target.value as "male" | "female"); disarm(); }}>
                <option value="male">男频</option>
                <option value="female">女频</option>
              </select>
            </div>
            <div style={{ flex: 1 }}>
              <div style={labelStyle}>作品分类（可选）</div>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                {categories.length > 0 ? (
                  <select style={{ ...inputStyle, flex: 1 }} value={category} onChange={(e) => { setCategory(e.target.value); disarm(); }}>
                    <option value="">（不指定）</option>
                    {categories.map((c) => (
                      <option key={c.id} value={c.name} title={c.description}>{c.name}</option>
                    ))}
                  </select>
                ) : (
                  <input style={{ ...inputStyle, flex: 1 }} value={category} placeholder="如：东方仙侠（可点右侧加载）"
                    onChange={(e) => { setCategory(e.target.value); disarm(); }} />
                )}
                <button className="btn-outline" onClick={handleLoadCategories} disabled={loadingCategories || !configLoaded}>
                  {loadingCategories ? <><span className="loading-spinner" />加载中</> : categories.length > 0 ? "刷新" : "加载分类"}
                </button>
              </div>
            </div>
          </div>

          {/* 主角 */}
          <div style={{ marginBottom: 14 }}>
            <div style={labelStyle}>主角名（可选，多个用逗号分隔）</div>
            <input style={inputStyle} value={protagonist} placeholder="例如：林凡、苏青雪"
              onChange={(e) => { setProtagonist(e.target.value); disarm(); }} />
          </div>

          {/* 封面 */}
          <div style={{ marginBottom: 14 }}>
            <div style={labelStyle}>封面 thumb_uri（必填，空封面会被番茄拒绝）</div>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <input style={{ ...inputStyle, flex: 1 }} value={thumbUri} placeholder="AI 生成，或粘贴已有书的 thumb_uri"
                onChange={(e) => { setThumbUri(e.target.value); disarm(); }} />
              <button className="btn-outline" onClick={() => setShowCoverPanel((v) => !v)}>
                {showCoverPanel ? "收起 ▲" : "AI 生成封面 ▼"}
              </button>
            </div>
            {coverPreview && (
              <div style={{ marginTop: 8, display: "flex", gap: 10, alignItems: "flex-start" }}>
                <img src={coverPreview} alt="封面预览" style={{ width: 90, height: 120, objectFit: "cover", borderRadius: 6, border: "1px solid var(--border)" }} />
                <div className="dim" style={{ fontSize: 12 }}>已生成并上传番茄，thumb_uri 已填入上方。可点「AI 生成封面」重来。</div>
              </div>
            )}
            {showCoverPanel && (
              <div style={{ marginTop: 10, padding: 12, border: "1px solid var(--border)", borderRadius: "var(--radius)" }}>
                <div style={labelStyle}>封面提示词（可编辑）</div>
                <textarea style={{ ...inputStyle, resize: "vertical" }} rows={3} value={coverPrompt}
                  onChange={(e) => setCoverPrompt(e.target.value)} />
                <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 8 }}>
                  <button className="btn-primary" onClick={handleGenerateCover} disabled={coverBusy}>
                    {coverBusy ? <><span className="loading-spinner" />生成中（约 10-30 秒）...</> : "生成并上传"}
                  </button>
                  <span className="dim" style={{ fontSize: 12 }}>需先在「系统设置 → 封面生图」配置生图模型</span>
                </div>
                {coverError && <div className="error" style={{ whiteSpace: "pre-wrap", marginTop: 8 }}>{coverError}</div>}
              </div>
            )}
            <div className="dim" style={{ fontSize: 12, marginTop: 3 }}>
              建过一次会自动记住，下次直接带出。封面 uri 可跨书复用。
            </div>
          </div>

          {error && <div className="error" style={{ whiteSpace: "pre-wrap" }}>{error}</div>}
          {result && (
            <div style={{
              background: createdBookId ? "var(--success-light)" : "var(--bg-soft)",
              border: `1px solid ${createdBookId ? "var(--success)" : "var(--border)"}`,
              borderRadius: "var(--radius)", padding: "10px 14px", marginBottom: 4,
              fontSize: 13, whiteSpace: "pre-wrap", wordBreak: "break-all",
            }}>{result}</div>
          )}
          <div ref={resultEndRef} />
        </div>

        {/* 固定底栏 */}
        <div style={{ padding: "12px 24px 18px", borderTop: "1px solid var(--border)", flexShrink: 0, display: "flex", gap: 8, justifyContent: "flex-end", alignItems: "center" }}>
          {armed && <span className="dim" style={{ fontSize: 12 }}>再点一次确认建书，5 秒后自动取消（番茄有每日建书上限）</span>}
          <button className="btn-outline" onClick={onClose}>{createdBookId ? "完成" : "关闭"}</button>
          {!createdBookId && (
            <button className={armed ? "btn-danger" : "btn-primary"} onClick={handleCreateClick} disabled={busy || !formValid}>
              {busy ? <><span className="loading-spinner" />建书中...</> : armed ? "确认创建" : "在番茄创建"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
