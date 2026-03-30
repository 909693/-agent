import { useEffect, useMemo, useState } from "react";
import { api } from "../api";

type Dict = Record<string, unknown>;
type SkillItem = { id: string; name: string; description: string; repoUrl: string; enabled: boolean; updatedAt: string };
type SkillFile = { name: string; path: string; content: string };
type SkillDetail = {
  record: SkillItem & { installPath?: string };
  readme: string;
  skill: string;
  references: SkillFile[];
};

type SkillsApi = typeof api & {
  listSkills: () => Promise<unknown>;
  installSkillRepo: (repoUrl: string) => Promise<unknown>;
  updateSkillRepo: (id: string) => Promise<unknown>;
  toggleSkillRepo: (id: string, enabled: boolean) => Promise<unknown>;
  removeSkillRepo: (id: string) => Promise<unknown>;
  getSkillDetail: (id: string) => Promise<unknown>;
};

const skillsApi = api as SkillsApi;
const asRecord = (v: unknown): Dict => (v && typeof v === "object" ? (v as Dict) : {});
const getText = (v: unknown, ...keys: string[]) => { const r = asRecord(v); for (const k of keys) { const x = r[k]; if (typeof x === "string" && x.trim()) return x.trim(); } return ""; };
const getBool = (v: unknown, ...keys: string[]) => { const r = asRecord(v); for (const k of keys) if (typeof r[k] === "boolean") return r[k] as boolean; return false; };
const getList = (v: unknown) => Array.isArray(v) ? v : Array.isArray(asRecord(v).skills) ? (asRecord(v).skills as unknown[]) : Array.isArray(asRecord(v).items) ? (asRecord(v).items as unknown[]) : [];
const getError = (e: unknown) => e instanceof Error ? e.message : "操作失败，请稍后重试";
const repoName = (url: string) => url.replace(/\/+$/, "").split("/").filter(Boolean).slice(-1)[0] || "未命名技能";
const formatTime = (value: string) => !value ? "未记录" : Number.isNaN(Date.parse(value)) ? value : new Date(value).toLocaleString("zh-CN");
const normalizeSkill = (v: unknown): SkillItem => {
  const repoUrl = getText(v, "repoUrl", "repo_url", "url", "repo");
  return {
    id: getText(v, "id", "repoId", "repo_id") || repoUrl || repoName(repoUrl),
    name: getText(v, "name", "title") || repoName(repoUrl),
    description: getText(v, "description", "desc") || "未提供描述",
    repoUrl,
    enabled: getBool(v, "enabled", "isEnabled", "active"),
    updatedAt: getText(v, "updatedAt", "updated_at", "lastUpdated", "last_updated", "installedAt", "installed_at"),
  };
};
const normalizeDetail = (v: unknown): SkillDetail => {
  const recordRaw = asRecord(asRecord(v).record);
  return {
    record: {
      id: getText(recordRaw, "id"),
      name: getText(recordRaw, "name") || "未命名技能",
      description: getText(recordRaw, "description") || "未提供描述",
      repoUrl: getText(recordRaw, "repoUrl", "repo_url"),
      enabled: getBool(recordRaw, "enabled"),
      updatedAt: getText(recordRaw, "updatedAt", "updated_at"),
      installPath: getText(recordRaw, "installPath", "install_path"),
    },
    readme: getText(v, "readme"),
    skill: getText(v, "skill"),
    references: Array.isArray(asRecord(v).references) ? (asRecord(v).references as unknown[]).map((item) => ({
      name: getText(item, "name") || "未命名文件",
      path: getText(item, "path") || getText(item, "name"),
      content: getText(item, "content"),
    })) : [],
  };
};

export function SkillsManager() {
  const [skills, setSkills] = useState<SkillItem[]>([]);
  const [repoUrl, setRepoUrl] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [busyKey, setBusyKey] = useState("");
  const [selectedId, setSelectedId] = useState("");
  const [detail, setDetail] = useState<SkillDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [activeDoc, setActiveDoc] = useState<"readme" | "skill" | string>("readme");

  const loadSkills = async () => {
    setLoading(true);
    try {
      const list = getList(await skillsApi.listSkills()).map(normalizeSkill);
      setSkills(list);
      setError("");
      if (!selectedId && list.length > 0) setSelectedId(list[0].id);
      if (selectedId && !list.find((s) => s.id === selectedId)) setSelectedId(list[0]?.id || "");
    } catch (e) { setError(getError(e)); }
    finally { setLoading(false); }
  };

  useEffect(() => { void loadSkills(); }, []);

  useEffect(() => {
    if (!selectedId) { setDetail(null); return; }
    let cancelled = false;
    const loadDetail = async () => {
      setDetailLoading(true);
      try {
        const raw = await skillsApi.getSkillDetail(selectedId);
        if (!cancelled) {
          const parsed = normalizeDetail(raw);
          setDetail(parsed);
          setActiveDoc(parsed.skill ? "skill" : parsed.readme ? "readme" : parsed.references[0]?.path || "readme");
        }
      } catch (e) {
        if (!cancelled) setError(getError(e));
      } finally {
        if (!cancelled) setDetailLoading(false);
      }
    };
    void loadDetail();
    return () => { cancelled = true; };
  }, [selectedId]);

  const runAction = async (key: string, job: () => Promise<unknown>, clearRepo = false) => {
    setBusyKey(key);
    try {
      await job();
      if (clearRepo) setRepoUrl("");
      await loadSkills();
    } catch (e) { setError(getError(e)); }
    finally { setBusyKey(""); }
  };

  const currentDoc = useMemo(() => {
    if (!detail) return "";
    if (activeDoc === "skill") return detail.skill || "";
    if (activeDoc === "readme") return detail.readme || "";
    return detail.references.find((ref) => ref.path === activeDoc)?.content || "";
  }, [detail, activeDoc]);

  return (
    <div className="plugin-shell">
      {/* Compact install bar */}
      <div className="plugin-install-bar">
        <input value={repoUrl} onChange={e => setRepoUrl(e.target.value)} placeholder="输入 GitHub 仓库 URL 安装 Skill..." />
        <button className="btn-primary" disabled={!repoUrl.trim() || !!busyKey} onClick={() => runAction("install-skill", () => skillsApi.installSkillRepo(repoUrl.trim()), true)}>
          {busyKey === "install-skill" ? "安装中..." : "安装"}
        </button>
      </div>

      {error && <div className="error">{error}</div>}

      <div className="skills-browser-grid">
        {/* Left: skill list */}
        <section className="plugin-panel">
          <div className="plugin-panel-head">
            <h3>已安装 Skills</h3>
            <span className="plugin-panel-badge">{skills.length} 个</span>
          </div>
          {loading ? <div className="plugin-loading"><span className="loading-spinner" />加载中...</div> : skills.length === 0 ? (
            <div className="plugin-empty-box compact">
              <h4>暂无 Skills</h4>
              <p>从上方输入仓库地址安装</p>
            </div>
          ) : (
            <div className="skills-list-compact">
              {skills.map(skill => (
                <article className={`skill-row-card ${selectedId === skill.id ? "selected" : ""}`} key={skill.id} onClick={() => setSelectedId(skill.id)}>
                  <div className="skill-row-main">
                    <div className="plugin-title-row">
                      <h3>{skill.name}</h3>
                      <span className={`plugin-status-pill ${skill.enabled ? "online" : "offline"}`}>{skill.enabled ? "启用" : "禁用"}</span>
                    </div>
                    <p>{skill.description}</p>
                    <small>{formatTime(skill.updatedAt)}</small>
                  </div>
                  <div className="plugin-toolbar wrap">
                    <button className="btn-outline" disabled={!!busyKey} onClick={(e) => { e.stopPropagation(); void runAction(`toggle-${skill.id}`, () => skillsApi.toggleSkillRepo(skill.id, !skill.enabled)); }}>{skill.enabled ? "禁用" : "启用"}</button>
                    <button className="btn-outline" disabled={!!busyKey} onClick={(e) => { e.stopPropagation(); void runAction(`update-${skill.id}`, () => skillsApi.updateSkillRepo(skill.id)); }}>更新</button>
                    <button className="btn-danger plugin-btn-danger solid" disabled={!!busyKey} onClick={(e) => { e.stopPropagation(); void runAction(`remove-${skill.id}`, () => skillsApi.removeSkillRepo(skill.id)); }}>删除</button>
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>

        {/* Right: skill detail */}
        <section className="plugin-panel skill-detail-panel">
          <div className="plugin-panel-head">
            <h3>{detail?.record.name || "Skill 详情"}</h3>
          </div>
          {!selectedId ? (
            <div className="plugin-empty-box compact"><p>选择一个 Skill 查看详情</p></div>
          ) : detailLoading ? (
            <div className="plugin-loading"><span className="loading-spinner" />读取中...</div>
          ) : detail ? (
            <>
              <div className="skill-meta-compact">
                <span>仓库：</span><code>{detail.record.repoUrl || "未记录"}</code>
              </div>
              <div className="doc-tabs">
                {detail.skill && <button className={`doc-tab ${activeDoc === "skill" ? "active" : ""}`} onClick={() => setActiveDoc("skill")}>SKILL.md</button>}
                {detail.readme && <button className={`doc-tab ${activeDoc === "readme" ? "active" : ""}`} onClick={() => setActiveDoc("readme")}>README.md</button>}
                {detail.references.map((ref) => (
                  <button key={ref.path} className={`doc-tab ${activeDoc === ref.path ? "active" : ""}`} onClick={() => setActiveDoc(ref.path)}>{ref.name}</button>
                ))}
              </div>
              <pre className="skill-doc-viewer">{currentDoc || "该文档为空"}</pre>
            </>
          ) : (
            <div className="plugin-empty-box compact"><p>未能读取 Skill 内容</p></div>
          )}
        </section>
      </div>
    </div>
  );
}
