import { useState, useEffect } from "react";
import { api, type LlmParams } from "../api";
import { open as dialogOpen } from "@tauri-apps/plugin-dialog";

interface Props {
  llm: LlmParams;
  onChange: (llm: LlmParams) => void;
  theme: string;
  onThemeChange: (theme: string) => void;
}

type Profile = { apiKey: string; model: string; baseUrl: string };
type Profiles = Record<string, Profile>;

const PROFILES_KEY = "llm_profiles";

function loadProfiles(): Profiles {
  try {
    const raw = localStorage.getItem(PROFILES_KEY);
    if (raw) return JSON.parse(raw);
  } catch {}
  return {};
}

function saveProfiles(p: Profiles) {
  localStorage.setItem(PROFILES_KEY, JSON.stringify(p));
}

export function SettingsPage({ llm, onChange, theme, onThemeChange }: Props) {
  const isConfigured = !!(llm.apiKey && llm.model);
  const [dataDir, setDataDir] = useState("");
  const [changing, setChanging] = useState(false);
  const [msg, setMsg] = useState("");
  const [profiles, setProfiles] = useState<Profiles>(loadProfiles);
  const [testResult, setTestResult] = useState("");
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    api.getDataDir().then(setDataDir).catch(() => setDataDir("未知"));
  }, []);

  // When format changes, save current config to old profile, load new profile
  const handleFormatChange = (newFormat: string) => {
    // Save current to profiles
    const updated = {
      ...profiles,
      [llm.apiFormat]: { apiKey: llm.apiKey, model: llm.model, baseUrl: llm.baseUrl },
    };
    setProfiles(updated);
    saveProfiles(updated);

    // Load saved profile for new format (or empty)
    const saved = updated[newFormat] || { apiKey: "", model: "", baseUrl: "" };
    onChange({ apiFormat: newFormat, apiKey: saved.apiKey, model: saved.model, baseUrl: saved.baseUrl });
  };

  const handleChangeDir = async (migrate: boolean) => {
    try {
      const selected = await dialogOpen({ directory: true, multiple: false, title: "选择数据存放目录" });
      if (!selected) return;
      const newDir = typeof selected === "string" ? selected : (selected as any);
      if (!newDir || newDir === dataDir) return;
      setChanging(true); setMsg("");
      const result = await api.setDataDir(newDir, migrate);
      setDataDir(newDir); setMsg(result);
    } catch (e: any) { setMsg("错误：" + e.toString()); }
    finally { setChanging(false); }
  };

  const formatHints: Record<string, { urlPlaceholder: string; modelPlaceholder: string; hint: string }> = {
    openai: {
      urlPlaceholder: "https://new-api.xt-url.com",
      modelPlaceholder: "gpt-5.4 / gpt-4o / claude-sonnet-4-...",
      hint: "OpenAI 兼容中转站：填中转站地址，路径自动拼 /v1/chat/completions",
    },
    anthropic: {
      urlPlaceholder: "https://pay.kxaug.xyz",
      modelPlaceholder: "claude-sonnet-4-20250514 / claude-3-5-sonnet-...",
      hint: "Anthropic 原生协议：路径自动拼 /v1/messages，需要支持 Anthropic 格式的中转站",
    },
    gemini: {
      urlPlaceholder: "https://generativelanguage.googleapis.com",
      modelPlaceholder: "gemini-2.5-flash / gemini-2.0-pro",
      hint: "Gemini 原生协议：使用 Google AI 的 generateContent API",
    },
  };

  const fh = formatHints[llm.apiFormat] || formatHints.openai;

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

      <div className="settings-card">
        <h3>LLM 配置</h3>

        <div className="form-group">
          <label>API 格式</label>
          <select value={llm.apiFormat} onChange={e => handleFormatChange(e.target.value)}>
            <option value="openai">OpenAI 兼容</option>
            <option value="anthropic">Anthropic 原生</option>
            <option value="gemini">Gemini 原生</option>
          </select>
        </div>

        <div className="form-group">
          <label>API Base URL</label>
          <input
            value={llm.baseUrl}
            onChange={e => onChange({ ...llm, baseUrl: e.target.value })}
            placeholder={fh.urlPlaceholder}
          />
        </div>

        <div className="form-group">
          <label>API Key</label>
          <input
            type="password"
            value={llm.apiKey}
            onChange={e => onChange({ ...llm, apiKey: e.target.value })}
            placeholder="sk-..."
          />
        </div>

        <div className="form-group">
          <label>模型名称</label>
          <input
            value={llm.model}
            onChange={e => onChange({ ...llm, model: e.target.value })}
            placeholder={fh.modelPlaceholder}
          />
        </div>

        <div className="settings-hint-box">
          {fh.hint}
          <br />
          切换格式时会自动保存/恢复对应的 Base URL、Key 和模型配置。
        </div>

        <div className="settings-status">
          <span className={`dot ${isConfigured ? "green" : "red"}`} />
          {isConfigured ? "API 已配置" : "请填写 API Key 和模型名称"}
          {isConfigured && (
            <button
              className="btn-outline"
              style={{ marginLeft: 12, fontSize: 12, padding: "4px 12px" }}
              disabled={testing}
              onClick={async () => {
                setTesting(true); setTestResult("");
                try {
                  const r = await api.testLlm(llm.apiFormat, llm.apiKey, llm.model, llm.baseUrl);
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
      </div>

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
