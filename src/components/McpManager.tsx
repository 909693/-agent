import { useEffect, useState } from "react";
import { api } from "../api";

type Dict = Record<string, unknown>;
type TestInfo = { ok: boolean; message: string; testedAt: string };
type McpServerItem = {
  id: string; name: string; repoUrl: string; command: string; args: string[]; cwd: string;
  env: Record<string, string>; enabled: boolean; running: boolean; updatedAt: string; lastTest: TestInfo | null;
};
type McpApi = typeof api & {
  listMcpServers: () => Promise<unknown>;
  installMcpRepo: (repoUrl: string) => Promise<unknown>;
  saveMcpServer: (server: Record<string, unknown>) => Promise<unknown>;
  deleteMcpServer: (id: string) => Promise<unknown>;
  testMcpServer: (id: string) => Promise<unknown>;
  startMcpServer: (id: string) => Promise<unknown>;
  stopMcpServer: (id: string) => Promise<unknown>;
  getMcpLogs: (id: string) => Promise<unknown>;
};

type FormState = { id: string; name: string; command: string; args: string; cwd: string; env: string; enabled: boolean; repoUrl: string };
const mcpApi = api as McpApi;
const emptyForm: FormState = { id: "", name: "", command: "", args: "", cwd: "", env: "{}", enabled: true, repoUrl: "" };
const asRecord = (v: unknown): Dict => (v && typeof v === "object" ? (v as Dict) : {});
const getText = (v: unknown, ...keys: string[]) => { const r = asRecord(v); for (const k of keys) { const x = r[k]; if (typeof x === "string") return x; } return ""; };
const getBool = (v: unknown, ...keys: string[]) => { const r = asRecord(v); for (const k of keys) if (typeof r[k] === "boolean") return r[k] as boolean; return false; };
const getArray = (v: unknown, ...keys: string[]) => { const r = asRecord(v); for (const k of keys) if (Array.isArray(r[k])) return r[k] as unknown[]; return []; };
const getError = (e: unknown) => e instanceof Error ? e.message : "操作失败";
const repoName = (url: string) => url.replace(/\/+$/, "").split("/").filter(Boolean).slice(-1)[0] || "未命名";
const getList = (v: unknown) => Array.isArray(v) ? v : Array.isArray(asRecord(v).servers) ? (asRecord(v).servers as unknown[]) : Array.isArray(asRecord(v).items) ? (asRecord(v).items as unknown[]) : [];
const parseArgs = (value: string) => value.split(/\r?\n/).map(x => x.trim()).filter(Boolean);
const normalizeEnv = (value: unknown): Record<string, string> => {
  const source = asRecord(value);
  return Object.entries(source).reduce<Record<string, string>>((acc, [key, val]) => {
    if (typeof val === "string") acc[key] = val;
    else if (typeof val === "number" || typeof val === "boolean") acc[key] = String(val);
    return acc;
  }, {});
};
const normalizeTest = (value: unknown): TestInfo | null => {
  if (!value) return null;
  return { ok: getBool(value, "ok", "success", "passed"), message: getText(value, "message", "detail", "error") || "未提供详情", testedAt: getText(value, "testedAt", "tested_at") };
};
const normalizeServer = (value: unknown): McpServerItem => {
  const repoUrl = getText(value, "repoUrl", "repo_url", "url", "repo");
  return {
    id: getText(value, "id", "serverId") || repoUrl || repoName(repoUrl),
    name: getText(value, "name", "title") || repoName(repoUrl),
    repoUrl, command: getText(value, "command", "cmd"),
    args: getArray(value, "args", "arguments").map(item => String(item)),
    cwd: getText(value, "cwd", "workingDirectory"),
    env: normalizeEnv(asRecord(value).env),
    enabled: getBool(value, "enabled", "isEnabled", "active"),
    running: getBool(value, "running", "isRunning"),
    updatedAt: getText(value, "updatedAt", "updated_at"),
    lastTest: normalizeTest(asRecord(value).lastTest ?? asRecord(value).testResult),
  };
};

export function McpManager() {
  const [servers, setServers] = useState<McpServerItem[]>([]);
  const [repoUrl, setRepoUrl] = useState("");
  const [form, setForm] = useState<FormState>(emptyForm);
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState("");
  const [error, setError] = useState("");
  const [formError, setFormError] = useState("");
  const [logServer, setLogServer] = useState<McpServerItem | null>(null);
  const [logs, setLogs] = useState("");

  const loadServers = async () => {
    setLoading(true);
    try { setServers(getList(await mcpApi.listMcpServers()).map(normalizeServer)); setError(""); }
    catch (e) { setError(getError(e)); }
    finally { setLoading(false); }
  };

  useEffect(() => { void loadServers(); }, []);

  const runAction = async (key: string, job: () => Promise<unknown>, after?: () => Promise<void> | void) => {
    setBusyKey(key);
    try { await job(); await loadServers(); if (after) await after(); }
    catch (e) { setError(getError(e)); }
    finally { setBusyKey(""); }
  };

  const resetForm = () => { setForm(emptyForm); setFormError(""); };

  const editServer = (server: McpServerItem) => {
    setForm({ id: server.id, name: server.name, command: server.command, args: server.args.join("\n"), cwd: server.cwd, env: JSON.stringify(server.env, null, 2), enabled: server.enabled, repoUrl: server.repoUrl });
    setFormError("");
  };

  const submitForm = async () => {
    if (!form.name.trim() || !form.command.trim()) { setFormError("名称和 command 不能为空"); return; }
    let env: Record<string, string>;
    try { const parsed = JSON.parse(form.env || "{}"); if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error(); env = normalizeEnv(parsed); }
    catch { setFormError("Env JSON 格式不合法"); return; }
    setFormError("");
    await runAction(form.id ? `save-${form.id}` : "create-mcp", () => mcpApi.saveMcpServer({
      id: form.id || undefined, name: form.name.trim(), command: form.command.trim(), args: parseArgs(form.args), cwd: form.cwd.trim(), env, enabled: form.enabled, repoUrl: form.repoUrl.trim() || undefined,
    }), async () => { if (!form.id) resetForm(); });
  };

  const openLogs = async (server: McpServerItem) => {
    setLogServer(server); setLogs("加载中..."); setBusyKey(`logs-${server.id}`);
    try { const result = await mcpApi.getMcpLogs(server.id); setLogs(typeof result === "string" ? (result || "暂无日志") : JSON.stringify(result, null, 2)); }
    catch (e) { setLogs("日志加载失败"); setError(getError(e)); }
    finally { setBusyKey(""); }
  };

  return (
    <div className="plugin-shell">
      {/* Compact install bar */}
      <div className="plugin-install-bar">
        <input value={repoUrl} onChange={e => setRepoUrl(e.target.value)} placeholder="输入 GitHub 仓库 URL 导入 MCP 服务..." />
        <button className="btn-primary" disabled={!repoUrl.trim() || !!busyKey} onClick={() => runAction("install-mcp", () => mcpApi.installMcpRepo(repoUrl.trim()), () => setRepoUrl(""))}>
          {busyKey === "install-mcp" ? "安装中..." : "导入"}
        </button>
      </div>

      {error && <div className="error">{error}</div>}

      <div className="mcp-workspace-grid">
        {/* Left: form */}
        <section className="plugin-panel">
          <div className="plugin-panel-head">
            <h3>{form.id ? "编辑服务" : "新增 MCP 服务"}</h3>
            <div className="plugin-inline-actions">
              <button className="btn-outline" onClick={resetForm}>清空</button>
              <button className="btn-primary" disabled={!!busyKey} onClick={() => void submitForm()}>{form.id ? "保存" : "新增"}</button>
            </div>
          </div>
          <div className="plugin-form-grid">
            <label className="form-group"><span>名称</span><input value={form.name} onChange={e => setForm(p => ({ ...p, name: e.target.value }))} placeholder="filesystem / playwright" /></label>
            <label className="form-group"><span>Command</span><input value={form.command} onChange={e => setForm(p => ({ ...p, command: e.target.value }))} placeholder="npx / uvx / node" /></label>
            <label className="form-group"><span>工作目录</span><input value={form.cwd} onChange={e => setForm(p => ({ ...p, cwd: e.target.value }))} placeholder="/path/to/cwd" /></label>
            <label className="form-group"><span>仓库 URL</span><input value={form.repoUrl} onChange={e => setForm(p => ({ ...p, repoUrl: e.target.value }))} placeholder="可选" /></label>
            <label className="form-group plugin-form-span-2"><span>Args（每行一个）</span><textarea value={form.args} onChange={e => setForm(p => ({ ...p, args: e.target.value }))} rows={3} placeholder="-y&#10;@modelcontextprotocol/server-filesystem" /></label>
            <label className="form-group plugin-form-span-2"><span>Env JSON</span><textarea value={form.env} onChange={e => setForm(p => ({ ...p, env: e.target.value }))} rows={2} placeholder='{"API_KEY":"xxx"}' /></label>
          </div>
          <label className="plugin-checkbox"><input type="checkbox" checked={form.enabled} onChange={e => setForm(p => ({ ...p, enabled: e.target.checked }))} />保存后立即启用</label>
          {formError && <div className="error plugin-inline-error">{formError}</div>}
        </section>

        {/* Right: server list */}
        <section className="plugin-panel">
          <div className="plugin-panel-head">
            <h3>服务列表</h3>
            <span className="plugin-panel-badge">{servers.length} 个</span>
          </div>
          {loading ? <div className="plugin-loading"><span className="loading-spinner" />加载中...</div> : servers.length === 0 ? (
            <div className="plugin-empty-box compact">
              <h4>暂无 MCP 服务</h4>
              <p>从上方导入仓库或左侧手动新增</p>
            </div>
          ) : (
            <div className="mcp-service-list">
              {servers.map(server => (
                <article className={`mcp-service-card ${server.running ? "running" : ""}`} key={server.id}>
                  <div className="plugin-title-row">
                    <h3>{server.name}</h3>
                    <span className={`plugin-status-pill ${server.running ? "online" : "offline"}`}>{server.running ? "运行中" : "停止"}</span>
                    <span className={`plugin-status-pill ${server.enabled ? "soft" : "ghost"}`}>{server.enabled ? "启用" : "禁用"}</span>
                  </div>
                  <p className="mcp-command-line">{server.command}{server.args.length ? ` ${server.args.join(" ")}` : ""}</p>
                  <div className="plugin-toolbar compact wrap">
                    <button className="btn-outline" disabled={!!busyKey} onClick={() => editServer(server)}>编辑</button>
                    <button className="btn-outline" disabled={!!busyKey} onClick={() => runAction(`test-${server.id}`, () => mcpApi.testMcpServer(server.id))}>测试</button>
                    <button className="btn-outline" disabled={!!busyKey || server.running} onClick={() => runAction(`start-${server.id}`, () => mcpApi.startMcpServer(server.id))}>启动</button>
                    <button className="btn-outline" disabled={!!busyKey || !server.running} onClick={() => runAction(`stop-${server.id}`, () => mcpApi.stopMcpServer(server.id))}>停止</button>
                    <button className="btn-outline" disabled={!!busyKey} onClick={() => void openLogs(server)}>日志</button>
                    <button className="btn-danger plugin-btn-danger solid" disabled={!!busyKey} onClick={() => runAction(`delete-${server.id}`, () => mcpApi.deleteMcpServer(server.id))}>删除</button>
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>
      </div>

      {logServer && (
        <section className="plugin-panel" style={{ marginTop: 16 }}>
          <div className="plugin-panel-head">
            <h3>{logServer.name} 日志</h3>
            <div className="plugin-inline-actions">
              <button className="btn-outline" disabled={!!busyKey} onClick={() => void openLogs(logServer)}>刷新</button>
              <button className="btn-outline" onClick={() => setLogServer(null)}>关闭</button>
            </div>
          </div>
          <pre className="plugin-log-viewer">{logs || "暂无日志"}</pre>
        </section>
      )}
    </div>
  );
}
