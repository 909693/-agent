import { useState, useEffect } from "react";
import { api, type LlmProvider } from "../api";
import { open as dialogOpen } from "@tauri-apps/plugin-dialog";

interface Props {
  providers: LlmProvider[];
  activeId: string;
  onProvidersChange: (providers: LlmProvider[]) => void;
  onActiveChange: (id: string) => void;
  theme: string;
  onThemeChange: (theme: string) => void;
}

const FORMAT_LABELS: Record<string, string> = {
  openai: "OpenAI 兼容",
  "openai-responses": "OpenAI Responses",
  anthropic: "Anthropic 原生",
  gemini: "Gemini 原生",
};

const formatHints: Record<string, { urlPlaceholder: string; modelPlaceholder: string; hint: string }> = {
  openai: {
    urlPlaceholder: "https://api.openai.com",
    modelPlaceholder: "gpt-5.4 / gpt-4o / claude-sonnet-4-...",
    hint: "OpenAI 兼容中转站：填中转站地址，路径自动拼 /v1/chat/completions",
  },
  "openai-responses": {
    urlPlaceholder: "https://api.openai.com",
    modelPlaceholder: "gpt-5.4 / glm-5.1 / ...",
    hint: "OpenAI Responses 协议：路径自动拼 /v1/responses，适用于要求 Codex 客户端走 Responses 协议的中转站",
  },
  anthropic: {
    urlPlaceholder: "https://api.anthropic.com",
    modelPlaceholder: "claude-sonnet-4-20250514 / claude-3-5-sonnet-...",
    hint: "Anthropic 原生协议：路径自动拼 /v1/messages，需要支持 Anthropic 格式的中转站",
  },
  gemini: {
    urlPlaceholder: "https://generativelanguage.googleapis.com",
    modelPlaceholder: "gemini-2.5-flash / gemini-2.0-pro",
    hint: "Gemini 原生协议：使用 Google AI 的 generateContent API",
  },
};

const uaPresets: Array<{ label: string; value: string }> = [
  { label: "Claude Code", value: "claude-cli/2.0.14 (external, cli)" },
  { label: "Codex CLI", value: "codex_cli_rs/0.42.0 (Mac OS 15.7.4; arm64) iTerm.app" },
  { label: "Chrome", value: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36" },
];

function newId(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return `p-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  }
}

function hostOf(url: string): string {
  return url.replace(/^https?:\/\//, "").split("/")[0] || "";
}

export function SettingsPage({ providers, activeId, onProvidersChange, onActiveChange, theme, onThemeChange }: Props) {
  const [dataDir, setDataDir] = useState("");
  const [changing, setChanging] = useState(false);
  const [msg, setMsg] = useState("");

  // 编辑态:null = 列表视图;否则编辑该草稿(id 为空表示新增)
  const [draft, setDraft] = useState<LlmProvider | null>(null);

  const [testResult, setTestResult] = useState("");
  const [testing, setTesting] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [modelsError, setModelsError] = useState("");
  const [showModels, setShowModels] = useState(false);
  const [modelFilter, setModelFilter] = useState("");
  const [modelInput, setModelInput] = useState(""); // 手动输入模型名加入池

  useEffect(() => {
    api.getDataDir().then(setDataDir).catch(() => setDataDir("未知"));
  }, []);

  const startAdd = () => {
    setDraft({ id: "", name: "", apiFormat: "openai", apiKey: "", model: "", models: [], baseUrl: "", proxyUrl: "", userAgent: "" });
    resetEditTransient();
  };

  const startEdit = (p: LlmProvider) => {
    setDraft({ ...p, models: p.models ?? [], proxyUrl: p.proxyUrl || "", userAgent: p.userAgent || "" });
    resetEditTransient();
  };

  const resetEditTransient = () => {
    setTestResult(""); setTesting(false);
    setModels([]); setShowModels(false); setModelsError(""); setModelFilter("");
  };

  const handleDuplicate = (p: LlmProvider) => {
    const copy: LlmProvider = { ...p, id: newId(), name: `${p.name || FORMAT_LABELS[p.apiFormat] || p.apiFormat} 副本` };
    onProvidersChange([...providers, copy]);
  };

  const handleDelete = (p: LlmProvider) => {
    const rest = providers.filter(x => x.id !== p.id);
    onProvidersChange(rest);
    if (activeId === p.id) {
      onActiveChange(rest[0]?.id ?? "");
    }
  };

  const handleSaveDraft = () => {
    if (!draft) return;
    const name = draft.name.trim() || `${FORMAT_LABELS[draft.apiFormat] || draft.apiFormat} · ${hostOf(draft.baseUrl)}`;
    const clean: LlmProvider = {
      ...draft,
      name,
      proxyUrl: draft.proxyUrl?.trim() || undefined,
      userAgent: draft.userAgent?.trim() || undefined,
    };
    if (clean.id) {
      onProvidersChange(providers.map(p => (p.id === clean.id ? clean : p)));
    } else {
      clean.id = newId();
      onProvidersChange([...providers, clean]);
      // 首个供应商自动设为启用
      if (providers.length === 0) onActiveChange(clean.id);
    }
    setDraft(null);
  };

  // 模型池:加入/移出。当前选用(model)随之维护——移出当前项则回退到池首个。
  const toggleModelInPool = (name: string) => {
    setDraft(d => {
      if (!d) return d;
      const pool = d.models ?? [];
      const inPool = pool.includes(name);
      const nextPool = inPool ? pool.filter(m => m !== name) : [...pool, name];
      let nextModel = d.model;
      if (inPool) {
        if (d.model === name) nextModel = nextPool[0] ?? "";
      } else if (!d.model) {
        nextModel = name; // 池从空到有,自动设为当前
      }
      return { ...d, models: nextPool, model: nextModel };
    });
  };

  // 手动输入模型名并加入池(输入框失焦/回车时)
  const addModelFromInput = (name: string) => {
    const n = name.trim();
    if (!n) return;
    setDraft(d => {
      if (!d) return d;
      const pool = d.models ?? [];
      const nextPool = pool.includes(n) ? pool : [...pool, n];
      return { ...d, models: nextPool, model: n };
    });
    setModelInput("");
  };

  const removeModel = (name: string) => {
    setDraft(d => {
      if (!d) return d;
      const nextPool = (d.models ?? []).filter(m => m !== name);
      const nextModel = d.model === name ? (nextPool[0] ?? "") : d.model;
      return { ...d, models: nextPool, model: nextModel };
    });
  };

  const patchDraft = (patch: Partial<LlmProvider>) => {
    setDraft(d => (d ? { ...d, ...patch } : d));
  };

  const handleFetchModels = async () => {
    if (!draft) return;
    setFetchingModels(true);
    setModelsError("");
    try {
      const list = await api.fetchModels(draft.apiFormat, draft.apiKey, draft.baseUrl, draft.proxyUrl || undefined, draft.userAgent || undefined);
      setModels(list);
      setModelFilter("");
      setShowModels(true);
    } catch (e: any) {
      setModels([]);
      setShowModels(false);
      setModelsError(e.toString());
    } finally {
      setFetchingModels(false);
    }
  };

  const handleChangeDir = async (migrate: boolean) => {
    try {
      const selected = await dialogOpen({ directory: true, multiple: false, title: "选择数据存放目录" });
      if (!selected) return;
      const newDir = typeof selected === "string" ? selected : "";
      if (!newDir || newDir === dataDir) return;
      setChanging(true); setMsg("");
      const result = await api.setDataDir(newDir, migrate);
      setDataDir(newDir); setMsg(result);
    } catch (e: any) { setMsg("错误：" + e.toString()); }
    finally { setChanging(false); }
  };

  const fh = draft ? (formatHints[draft.apiFormat] || formatHints.openai) : formatHints.openai;
  const filteredModels = models.filter(m => m.toLowerCase().includes(modelFilter.trim().toLowerCase()));
  const draftConfigured = !!(draft && draft.apiKey && draft.model);

  return (
    <div className="settings-page">
      <div className="settings-card">
        <h3>外观设置</h3>
        <div className="form-group">
          <label>主题</label>
          <div style={{ display: "flex", gap: 8 }}>
            <button className={`btn-outline ${theme === "light" ? "active" : ""}`} onClick={() => onThemeChange("light")}>
              亮色
            </button>
            <button className={`btn-outline ${theme === "dark" ? "active" : ""}`} onClick={() => onThemeChange("dark")}>
              暗色
            </button>
          </div>
        </div>
      </div>

      {draft === null ? (
        <div className="settings-card">
          <div className="provider-list-head">
            <h3>供应商</h3>
            <button className="btn-primary" onClick={startAdd}>+ 新增供应商</button>
          </div>
          {providers.length === 0 ? (
            <div className="provider-empty">
              还没有配置任何供应商。点「新增供应商」添加一个 API 渠道。
            </div>
          ) : (
            <div className="provider-list">
              {providers.map(p => {
                const active = p.id === activeId;
                return (
                  <div key={p.id} className={`provider-item ${active ? "active" : ""}`}>
                    <div className="provider-item-main">
                      <div className="provider-item-title">
                        <span className="provider-name">{p.name}</span>
                        <span className="provider-badge">{FORMAT_LABELS[p.apiFormat] || p.apiFormat}</span>
                        {active && <span className="provider-badge active-badge">启用中</span>}
                      </div>
                      <div className="provider-item-sub">
                        <span className="provider-url">{p.baseUrl || "（未填 URL）"}</span>
                        {p.model && <span className="provider-model">{p.model}</span>}
                        {(p.models?.length ?? 0) > 1 && (
                          <span className="provider-model-count">共 {p.models!.length} 个模型</span>
                        )}
                      </div>
                    </div>
                    <div className="provider-item-actions">
                      {!active && (
                        <button className="btn-primary" onClick={() => onActiveChange(p.id)}>启用</button>
                      )}
                      <button className="btn-outline" onClick={() => startEdit(p)}>编辑</button>
                      <button className="btn-outline" onClick={() => handleDuplicate(p)}>复制</button>
                      <button className="btn-outline danger" onClick={() => handleDelete(p)}>删除</button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      ) : (
        <div className="settings-card">
          <div className="provider-list-head">
            <h3>{draft.id ? "编辑供应商" : "新增供应商"}</h3>
            <button className="btn-outline" onClick={() => setDraft(null)}>← 返回列表</button>
          </div>

          <div className="form-group">
            <label>供应商名称</label>
            <input
              value={draft.name}
              onChange={e => patchDraft({ name: e.target.value })}
              placeholder="留空则自动用「格式 · 域名」"
            />
          </div>

          <div className="form-group">
            <label>API 格式</label>
            <select value={draft.apiFormat} onChange={e => { patchDraft({ apiFormat: e.target.value }); setModels([]); setShowModels(false); setModelsError(""); setModelFilter(""); }}>
              <option value="openai">OpenAI 兼容</option>
              <option value="openai-responses">OpenAI Responses</option>
              <option value="anthropic">Anthropic 原生</option>
              <option value="gemini">Gemini 原生</option>
            </select>
          </div>

          <div className="form-group">
            <label>API Base URL</label>
            <input
              value={draft.baseUrl}
              onChange={e => patchDraft({ baseUrl: e.target.value })}
              placeholder={fh.urlPlaceholder}
            />
          </div>

          <div className="form-group">
            <label>API Key</label>
            <input
              type="password"
              value={draft.apiKey}
              onChange={e => patchDraft({ apiKey: e.target.value })}
              placeholder="sk-..."
            />
          </div>

          <div className="form-group">
            <label>模型池</label>
            <small style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 6, display: "block" }}>
              可添加多个模型，写作时在顶栏随时切换。带勾的是当前默认选用。
            </small>

            {/* 已入池模型 chips */}
            {(draft.models ?? []).length > 0 && (
              <div className="model-pool">
                {(draft.models ?? []).map(m => (
                  <span
                    key={m}
                    className={`model-chip ${m === draft.model ? "current" : ""}`}
                    onClick={() => patchDraft({ model: m })}
                    title={m === draft.model ? "当前默认选用" : "点击设为默认选用"}
                  >
                    {m === draft.model && <span className="model-chip-check">✓</span>}
                    {m}
                    <span
                      className="model-chip-remove"
                      title="移出模型池"
                      onClick={e => { e.stopPropagation(); removeModel(m); }}
                    >
                      ×
                    </span>
                  </span>
                ))}
              </div>
            )}

            {/* 手动输入添加 + 拉取 */}
            <div className="model-input-row">
              <input
                value={modelInput}
                onChange={e => setModelInput(e.target.value)}
                onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); addModelFromInput(modelInput); } }}
                placeholder={fh.modelPlaceholder}
              />
              <button
                className="btn-outline"
                disabled={!modelInput.trim()}
                onClick={() => addModelFromInput(modelInput)}
              >
                添加
              </button>
              <button
                className="btn-outline"
                disabled={fetchingModels || !draft.apiKey}
                title={!draft.apiKey ? "请先填写 API Key" : "从 API 拉取可用模型列表"}
                onClick={handleFetchModels}
              >
                {fetchingModels ? "拉取中..." : "拉取模型"}
              </button>
            </div>
            {modelsError && <div className="model-fetch-error">{modelsError}</div>}
            {showModels && (
              <div className="model-list-panel">
                <div className="model-list-head">
                  <input
                    autoFocus
                    value={modelFilter}
                    onChange={e => setModelFilter(e.target.value)}
                    placeholder={`搜索 ${models.length} 个模型（点击加入/移出池）...`}
                  />
                  <button className="btn-outline" onClick={() => setShowModels(false)}>收起</button>
                </div>
                <div className="model-list-items">
                  {filteredModels.map(m => {
                    const inPool = (draft.models ?? []).includes(m);
                    return (
                      <div
                        key={m}
                        className={`model-list-item ${inPool ? "active" : ""}`}
                        onClick={() => toggleModelInPool(m)}
                        title={inPool ? "点击移出模型池" : "点击加入模型池"}
                      >
                        <span className="model-list-check">{inPool ? "✓" : "+"}</span>
                        {m}
                      </div>
                    );
                  })}
                  {filteredModels.length === 0 && (
                    <div className="model-list-empty">无匹配模型</div>
                  )}
                </div>
              </div>
            )}
          </div>

          <div className="form-group">
            <label>代理地址（可选）</label>
            <input
              value={draft.proxyUrl || ""}
              onChange={e => patchDraft({ proxyUrl: e.target.value })}
              placeholder="http://127.0.0.1:7897"
            />
            <small style={{ color: "var(--text-secondary)", fontSize: 12, marginTop: 4, display: "block" }}>
              留空则使用系统环境变量（HTTPS_PROXY/HTTP_PROXY）
            </small>
          </div>

          <div className="form-group">
            <label>User-Agent（可选）</label>
            <input
              value={draft.userAgent || ""}
              onChange={e => patchDraft({ userAgent: e.target.value })}
              placeholder="留空则不发送 User-Agent"
            />
            <div style={{ display: "flex", gap: 8, marginTop: 6, flexWrap: "wrap" }}>
              {uaPresets.map(p => (
                <button
                  key={p.label}
                  className={`btn-outline ${draft.userAgent === p.value ? "active" : ""}`}
                  style={{ fontSize: 12, padding: "4px 12px" }}
                  onClick={() => patchDraft({ userAgent: p.value })}
                >
                  {p.label}
                </button>
              ))}
            </div>
            <small style={{ color: "var(--text-secondary)", fontSize: 12, marginTop: 4, display: "block" }}>
              部分中转站限制客户端类型，可用预设模拟 Claude Code / Codex 等客户端；版本号可自行修改
            </small>
          </div>

          <div className="settings-hint-box">{fh.hint}</div>

          <div className="settings-status">
            <span className={`dot ${draftConfigured ? "green" : "red"}`} />
            {draftConfigured ? "配置完整" : "请填写 API Key 和模型名称"}
            {draftConfigured && (
              <button
                className="btn-outline"
                style={{ marginLeft: 12, fontSize: 12, padding: "4px 12px" }}
                disabled={testing}
                onClick={async () => {
                  setTesting(true); setTestResult("");
                  try {
                    const r = await api.testLlm(draft.apiFormat, draft.apiKey, draft.model, draft.baseUrl, draft.proxyUrl || undefined, draft.userAgent || undefined);
                    setTestResult(r);
                  } catch (e: any) { setTestResult("错误：" + e.toString()); }
                  finally { setTesting(false); }
                }}
              >
                {testing ? "测试中..." : "测试连接"}
              </button>
            )}
          </div>
          {testResult && (
            <pre className="test-result-box">{testResult}</pre>
          )}

          <div className="provider-edit-footer">
            <button className="btn-outline" onClick={() => setDraft(null)}>取消</button>
            <button className="btn-primary" onClick={handleSaveDraft}>保存</button>
          </div>
        </div>
      )}

      <div className="settings-card">
        <h3>数据存储</h3>
        <div className="form-group">
          <label>数据存放目录</label>
          <div className="data-dir-row">
            <code className="data-dir-path">{dataDir || "加载中..."}</code>
          </div>
          <div className="data-dir-actions">
            <button className="btn-secondary" onClick={() => handleChangeDir(true)} disabled={changing}>
              {changing ? "迁移中..." : "更换目录（迁移数据）"}
            </button>
            <button className="btn-outline" onClick={() => handleChangeDir(false)} disabled={changing}>
              更换目录（不迁移）
            </button>
          </div>
          {msg && <div className={`data-dir-msg ${msg.startsWith("错误") ? "error" : "success"}`}>{msg}</div>}
          <div className="form-hint">选择「迁移数据」会将已有项目复制到新位置。</div>
        </div>
      </div>
    </div>
  );
}
