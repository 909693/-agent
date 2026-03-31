import { useState, useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, type ProjectMeta, type LlmParams, type BatchProgress, type BatchComplete } from "../api";
import { ExportDialog } from "./ExportDialog";
import { CreativeConstraintsPanel } from "./CreativeConstraintsPanel";
import { buildCreativeConstraintsPayload } from "../utils/buildCreativeConstraints";
import { parseOutlineToPlot } from "../utils/outlineParser";
import { extractWorldFromOutline, extractCharactersFromOutline } from "../utils/outlineExtractors";

interface Props {
  project: ProjectMeta;
  llm: LlmParams;
  onWriteChapter: (chapterNum: number) => void;
}

type ManagerTab = "world" | "characters" | "plot" | "chapter-list" | "tracking";

const genreLabels: Record<string, string> = {
  fantasy: "玄幻", scifi: "科幻", urban: "都市", romance: "言情",
  mystery: "悬疑", history: "历史", horror: "恐怖", other: "其他",
};

const roleLabel: Record<string, string> = {
  protagonist: "主角", antagonist: "反派", supporting: "配角", minor: "龙套",
};

export function ChapterManager({ project, llm, onWriteChapter }: Props) {
  const [tab, setTab] = useState<ManagerTab>("world");
  const [world, setWorld] = useState<any>(null);
  const [characters, setCharacters] = useState<any>(null);
  const [plot, setPlot] = useState<any>(null);
  const [loading, setLoading] = useState("");
  const [error, setError] = useState("");
  const [chapterTexts, setChapterTexts] = useState<Record<number, string>>({});
  const [showExport, setShowExport] = useState(false);
  const [outlineText, setOutlineText] = useState("");
  const [outlineName, setOutlineName] = useState("");

  // Batch generation state
  const [batchRunning, setBatchRunning] = useState(false);
  const [batchStart, setBatchStart] = useState(1);
  const [batchEnd, setBatchEnd] = useState(1);
  const [skipWritten, setSkipWritten] = useState(true);
  const [batchProgress, setBatchProgress] = useState<BatchProgress | null>(null);
  const [batchResult, setBatchResult] = useState<BatchComplete | null>(null);
  const [chapterStatuses, setChapterStatuses] = useState<Record<number, string>>({});

  // Tracking state
  const [summaries, setSummaries] = useState<any>(null);
  const [consistencyResult, setConsistencyResult] = useState<any>(null);
  const [checkingConsistency, setCheckingConsistency] = useState(false);
  const [styleProfile, setStyleProfile] = useState<any>(null);
  const [analyzingStyle, setAnalyzingStyle] = useState(false);

  // Search state
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<any[] | null>(null);
  const [searching, setSearching] = useState(false);

  useEffect(() => {
    api.getWorld(project.id).then(setWorld).catch(() => setWorld(null));
    api.getCharacters(project.id).then(setCharacters).catch(() => setCharacters(null));
    api.getChapterSummaries(project.id).then(setSummaries).catch(() => setSummaries(null));
    api.getStyleProfile(project.id).then(setStyleProfile).catch(() => setStyleProfile(null));
    api.getPlot(project.id).then(p => {
      setPlot(p);
      loadChapterTexts(p);
    }).catch(() => setPlot(null));
    api.getOutlineSource(project.id).then((d) => {
      setOutlineText(d?.text || "");
      setOutlineName(d?.name || "");
    }).catch(() => { setOutlineText(""); setOutlineName(""); });
  }, [project.id]);

  // Batch event listeners
  useEffect(() => {
    let unProgress: UnlistenFn | undefined;
    let unComplete: UnlistenFn | undefined;
    (async () => {
      unProgress = await listen<BatchProgress>("batch_progress", (e) => {
        setBatchProgress(e.payload);
        setChapterStatuses(prev => ({ ...prev, [e.payload.chapter_number]: e.payload.phase }));
      });
      unComplete = await listen<BatchComplete>("batch_complete", (e) => {
        setBatchResult(e.payload);
        setBatchRunning(false);
        if (plot) loadChapterTexts(plot);
      });
    })();
    return () => { unProgress?.(); unComplete?.(); };
  }, [plot]);

  // Set default batch range when chapters load
  useEffect(() => {
    if (!plot?.acts) return;
    const chapters: number[] = [];
    for (const act of plot.acts) {
      for (const ch of act.chapters || []) chapters.push(ch.number);
    }
    if (chapters.length > 0) {
      const firstUnwritten = chapters.find(n => !chapterTexts[n]);
      setBatchStart(firstUnwritten ?? chapters[0]);
      setBatchEnd(chapters[chapters.length - 1]);
    }
  }, [plot, chapterTexts]);
  const loadChapterTexts = async (plotData: any) => {
    if (!plotData?.acts) return;
    const texts: Record<number, string> = {};
    for (const act of plotData.acts) {
      for (const ch of act.chapters || []) {
        try {
          const d: any = await api.getChapter(project.id, ch.number);
          if (d.text) texts[ch.number] = d.text;
        } catch { /* no chapter yet */ }
      }
    }
    setChapterTexts(texts);
  };

  const saveOutline = async (text: string, name = "手动导入大纲") => {
    const payload = { name, text, importedAt: new Date().toISOString() };
    const saved = await api.saveOutlineSource(project.id, payload);
    setOutlineText(saved.text || text);
    setOutlineName(saved.name || name);

    const parsedPlot = parseOutlineToPlot(text);
    if (parsedPlot.acts.length > 0) {
      await api.savePlotOutline(project.id, parsedPlot);
      setPlot(parsedPlot);
      loadChapterTexts(parsedPlot);
    }

    const worldDraft = extractWorldFromOutline(text);
    await api.saveWorldData(project.id, worldDraft);
    setWorld(worldDraft);

    const characterDraft = extractCharactersFromOutline(text);
    if (characterDraft.characters.length > 0) {
      await api.saveCharactersData(project.id, characterDraft);
      setCharacters(characterDraft);
    }
  };

  const handleOutlineFile = async (file: File) => {
    setError("");
    try {
      if (file.name.toLowerCase().endsWith(".docx")) {
        const arrayBuffer = await file.arrayBuffer();
        const mammoth: any = await import("mammoth");
        const result = await mammoth.extractRawText({ arrayBuffer });
        await saveOutline(result.value, file.name);
      } else {
        const text = await file.text();
        await saveOutline(text, file.name);
      }
    } catch (e: any) {
      setError(e.toString());
    }
  };

  const handleGenWorld = async () => {
    if (!llm.apiKey) { setError("请先在系统设置中配置 API Key"); return; }
    setLoading("world"); setError("");
    try {
      const payload = await buildCreativeConstraintsPayload();
      setWorld(await api.generateWorld(project.id, llm, payload));
    }
    catch (e: any) { setError(e.toString()); }
    setLoading("");
  };

  const handleGenCharacters = async () => {
    if (!llm.apiKey) { setError("请先在系统设置中配置 API Key"); return; }
    setLoading("characters"); setError("");
    try {
      const payload = await buildCreativeConstraintsPayload();
      setCharacters(await api.generateCharacters(project.id, llm, payload));
    }
    catch (e: any) { setError(e.toString()); }
    setLoading("");
  };

  const handleGenPlot = async () => {
    if (!llm.apiKey) { setError("请先在系统设置中配置 API Key"); return; }
    setLoading("plot"); setError("");
    try {
      const payload = await buildCreativeConstraintsPayload();
      const p = await api.generatePlot(project.id, llm, payload);
      setPlot(p);
      loadChapterTexts(p);
    } catch (e: any) { setError(e.toString()); }
    setLoading("");
  };
  const handleGenerateAll = async () => {
    if (!llm.apiKey) { setError("请先在系统设置中配置 API Key"); return; }
    const payload = await buildCreativeConstraintsPayload();
    setError("");
    setLoading("world");
    try { setWorld(await api.generateWorld(project.id, llm, payload)); } catch (e: any) { setError(e.toString()); setLoading(""); return; }
    setLoading("characters");
    try { setCharacters(await api.generateCharacters(project.id, llm, payload)); } catch (e: any) { setError(e.toString()); setLoading(""); return; }
    setLoading("plot");
    try {
      const p = await api.generatePlot(project.id, llm, payload);
      setPlot(p);
      loadChapterTexts(p);
    } catch (e: any) { setError(e.toString()); }
    setLoading("");
  };

  const handleBatchGenerate = async () => {
    if (!llm.apiKey) { setError("请先在系统设置中配置 API Key"); return; }
    if (batchStart > batchEnd) { setError("起始章号不能大于结束章号"); return; }
    setError(""); setBatchResult(null); setChapterStatuses({});
    setBatchRunning(true);
    try {
      const payload = await buildCreativeConstraintsPayload();
      await api.batchGenerateChapters(
        project.id, batchStart, batchEnd,
        project.target_chapter_words || 3000, skipWritten, llm, payload,
      );
    } catch (e: any) { setError(e.toString()); setBatchRunning(false); }
  };

  const handleCancelBatch = async () => {
    try { await api.cancelBatchGeneration(); } catch (e: any) { setError(e.toString()); }
  };

  // === Tracking data processing ===
  const characterTimeline: Record<string, Array<{ chapter: number; change: string }>> = {};
  const foreshadowingItems: Array<{ content: string; plantedChapter: number; resolvedChapter: number | null; status: "active" | "resolved" }> = [];

  if (summaries && typeof summaries === "object") {
    const sortedKeys = Object.keys(summaries).sort((a, b) => Number(a) - Number(b));
    const allResolved: Array<{ text: string; chapter: number }> = [];

    for (const key of sortedKeys) {
      const ch = Number(key);
      const s = summaries[key];
      // Character timeline
      if (Array.isArray(s.character_changes)) {
        for (const c of s.character_changes) {
          const name = c.name || "";
          if (name) {
            if (!characterTimeline[name]) characterTimeline[name] = [];
            characterTimeline[name].push({ chapter: ch, change: c.change || "" });
          }
        }
      }
      // Foreshadowing planted
      if (Array.isArray(s.foreshadowing_planted)) {
        for (const f of s.foreshadowing_planted) {
          if (f) foreshadowingItems.push({ content: f, plantedChapter: ch, resolvedChapter: null, status: "active" });
        }
      }
      // Foreshadowing resolved
      if (Array.isArray(s.foreshadowing_resolved)) {
        for (const f of s.foreshadowing_resolved) {
          if (f) allResolved.push({ text: f, chapter: ch });
        }
      }
    }
    // Cross-reference: mark resolved
    for (const r of allResolved) {
      const match = foreshadowingItems.find(fi => fi.status === "active" && fi.content.includes(r.text));
      if (match) { match.status = "resolved"; match.resolvedChapter = r.chapter; }
    }
  }

  const activeForeshadowing = foreshadowingItems.filter(f => f.status === "active");
  const resolvedForeshadowing = foreshadowingItems.filter(f => f.status === "resolved");

  const handleCheckConsistency = async () => {
    if (!llm.apiKey) { setError("请先配置 API Key"); return; }
    setCheckingConsistency(true); setError(""); setConsistencyResult(null);
    try {
      const result = await api.checkConsistency(project.id, llm);
      setConsistencyResult(result);
    } catch (e: any) { setError(e.toString()); }
    setCheckingConsistency(false);
  };

  const handleSearch = async () => {
    if (!searchQuery.trim()) return;
    setSearching(true); setError("");
    try {
      const results = await api.searchChapters(project.id, searchQuery.trim());
      setSearchResults(Array.isArray(results) ? results : []);
    } catch (e: any) { setError(e.toString()); }
    setSearching(false);
  };

  const handleAnalyzeStyle = async () => {
    if (!llm.apiKey) { setError("请先配置 API Key"); return; }
    setAnalyzingStyle(true); setError("");
    try {
      const result = await api.analyzeWritingStyle(project.id, llm);
      setStyleProfile(result);
    } catch (e: any) { setError(e.toString()); }
    setAnalyzingStyle(false);
  };

  const allChapters: { number: number; title: string; summary: string }[] = [];
  const characterList = characters?.characters || [];
  const protagonist = characterList.find((c: any) => c.role === "protagonist") || characterList[0] || null;
  const relationshipEdges = characterList.flatMap((c: any) =>
    (c.relationships || []).map((r: any) => ({ from: c.name, to: r.target, type: r.rel_type, description: r.description }))
  );
  const relationGroups = {
    allies: relationshipEdges.filter((e: any) => ["ally", "friend", "mentor", "family", "relationship"].includes(e.type)),
    rivals: relationshipEdges.filter((e: any) => ["rival", "enemy", "antagonist"].includes(e.type)),
    lovers: relationshipEdges.filter((e: any) => ["lover", "romance"].includes(e.type)),
    others: relationshipEdges.filter((e: any) => !["ally", "friend", "mentor", "family", "relationship", "rival", "enemy", "antagonist", "lover", "romance"].includes(e.type)),
  };
  if (plot?.acts) {
    for (const act of plot.acts) {
      for (const ch of act.chapters || []) {
        allChapters.push({ number: ch.number, title: ch.title, summary: ch.summary });
      }
    }
  }

  return (
    <div>
      <div className="project-info-bar">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
          <h2>{project.title}</h2>
          <button className="btn-outline" onClick={() => setShowExport(true)}>📥 导出</button>
        </div>
        <div className="project-meta">
          <span className="genre-tag">{genreLabels[project.genre] || project.genre}</span>
          {project.tone && <span className="dim">基调：{project.tone}</span>}
          {project.themes?.length > 0 && <span className="dim">主题：{project.themes.join("、")}</span>}
        </div>
        {project.premise && <p className="premise-text">{project.premise}</p>}
      </div>

      <div className="content-section" style={{ marginBottom: 16 }}>
        <h3>导入现有大纲</h3>
        <p className="dim" style={{ marginBottom: 12 }}>支持粘贴文本、上传 TXT、上传 Word（.docx）。导入后，后续框架生成与章节创作会优先按导入大纲执行。</p>
        <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap", marginBottom: 12 }}>
          <label className="btn-outline" style={{ cursor: "pointer" }}>
            上传大纲文件
            <input type="file" accept=".txt,.md,.docx" style={{ display: "none" }} onChange={e => { const f = e.target.files?.[0]; if (f) void handleOutlineFile(f); }} />
          </label>
          {outlineName && <span className="dim">已导入：{outlineName}</span>}
        </div>
        <textarea
          value={outlineText}
          onChange={e => setOutlineText(e.target.value)}
          rows={8}
          placeholder="你可以直接粘贴全书大纲 / 分卷大纲 / 章节大纲..."
          style={{ width: "100%", padding: "12px", border: "1px solid var(--border)", borderRadius: "var(--radius)", fontSize: 14, fontFamily: "inherit", resize: "vertical" }}
        />
        <div style={{ display: "flex", gap: 8, marginTop: 12 }}>
          <button className="btn-primary" onClick={() => void saveOutline(outlineText, outlineName || "手动导入大纲")} disabled={!outlineText.trim()}>保存大纲</button>
        </div>
      </div>

      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 4 }}>
        <div className="manager-tabs">
          {([["world", "世界观"], ["characters", "角色"], ["plot", "情节大纲"], ["chapter-list", "章节列表"], ["tracking", "追踪"]] as const).map(([key, label]) => (
            <button key={key} className={`manager-tab ${tab === key ? "active" : ""}`} onClick={() => setTab(key)}>
              {label}
            </button>
          ))}
        </div>
        <button className="btn-primary" onClick={handleGenerateAll} disabled={!!loading}>
          {loading ? <><span className="loading-spinner" />{loading === "world" ? "生成世界观..." : loading === "characters" ? "生成角色..." : "生成情节..."}</> : "一键生成框架"}
        </button>
      </div>

      {error && <div className="error">{error}</div>}
      <div style={{ marginBottom: 16 }}>
        <CreativeConstraintsPanel />
      </div>
      {tab === "world" && (
        <div>
          <div className="generate-bar">
            <button className="btn-primary" onClick={handleGenWorld} disabled={!!loading}>
              {loading === "world" ? <><span className="loading-spinner" />生成中...</> : world ? "重新生成" : "生成世界观"}
            </button>
          </div>
          {world && (
            <>
              <div className="content-section">
                <h3>时代：{world.era}</h3>
                <p>{world.overview}</p>
              </div>
              {world.geography?.length > 0 && (
                <div className="content-section">
                  <h3>地理</h3>
                  <div className="card-grid">
                    {world.geography.map((g: any, i: number) => (
                      <div key={i} className="info-card">
                        <h4>{g.name}</h4>
                        <p>{g.description}</p>
                        {g.significance && <p className="dim">意义：{g.significance}</p>}
                      </div>
                    ))}
                  </div>
                </div>
              )}
              {world.rules?.length > 0 && (
                <div className="content-section">
                  <h3>世界规则</h3>
                  <div className="card-grid">
                    {world.rules.map((r: any, i: number) => (
                      <div key={i} className="info-card">
                        <h4>{r.name} <span className="tag">{r.category}</span></h4>
                        <p>{r.description}</p>
                      </div>
                    ))}
                  </div>
                </div>
              )}
              {world.factions?.length > 0 && (
                <div className="content-section">
                  <h3>势力</h3>
                  <div className="card-grid">
                    {world.factions.map((f: any, i: number) => (
                      <div key={i} className="info-card">
                        <h4>{f.name}</h4>
                        <p>{f.description}</p>
                        {f.goals?.length > 0 && <p className="dim">目标：{f.goals.join("、")}</p>}
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      )}
      {tab === "characters" && (
        <div>
          <div className="generate-bar">
            <button className="btn-primary" onClick={handleGenCharacters} disabled={!!loading}>
              {loading === "characters" ? <><span className="loading-spinner" />生成中...</> : characters ? "重新生成" : "生成角色"}
            </button>
          </div>
          {relationshipEdges.length > 0 && (
            <div className="content-section">
              <h3>角色关系网络</h3>
              {protagonist && (
                <div className="relation-center-card">
                  <span className="dim">关系核心</span>
                  <h4>{protagonist.name}</h4>
                  <p>{protagonist.personality || protagonist.backstory || "核心角色"}</p>
                </div>
              )}
              <div className="relation-groups-grid">
                <div className="relation-group-card">
                  <h4>盟友 / 同伴</h4>
                  {relationGroups.allies.length === 0 ? <p className="dim">暂无</p> : relationGroups.allies.map((edge: any, idx: number) => <div key={idx} className="relation-item"><strong>{edge.from}</strong><span>→</span><strong>{edge.to || "未知角色"}</strong></div>)}
                </div>
                <div className="relation-group-card">
                  <h4>对立 / 宿敌</h4>
                  {relationGroups.rivals.length === 0 ? <p className="dim">暂无</p> : relationGroups.rivals.map((edge: any, idx: number) => <div key={idx} className="relation-item"><strong>{edge.from}</strong><span>→</span><strong>{edge.to || "未知角色"}</strong></div>)}
                </div>
                <div className="relation-group-card">
                  <h4>情感 / 暧昧</h4>
                  {relationGroups.lovers.length === 0 ? <p className="dim">暂无</p> : relationGroups.lovers.map((edge: any, idx: number) => <div key={idx} className="relation-item"><strong>{edge.from}</strong><span>→</span><strong>{edge.to || "未知角色"}</strong></div>)}
                </div>
                <div className="relation-group-card">
                  <h4>其他联系</h4>
                  {relationGroups.others.length === 0 ? <p className="dim">暂无</p> : relationGroups.others.map((edge: any, idx: number) => <div key={idx} className="relation-item"><strong>{edge.from}</strong><span>→</span><strong>{edge.to || "未知角色"}</strong></div>)}
                </div>
              </div>
            </div>
          )}
          {characters?.characters && (
            <div className="card-grid">
              {characters.characters.map((c: any, i: number) => (
                <div key={i} className="info-card character-card">
                  <h4>{c.name} <span className="tag">{roleLabel[c.role] || c.role}</span></h4>
                  {c.age && <p className="dim">年龄：{c.age}</p>}
                  {c.personality && <p><strong>性格：</strong>{c.personality}</p>}
                  {c.backstory && <p><strong>背景：</strong>{c.backstory}</p>}
                  {c.motivations?.length > 0 && <p><strong>动机：</strong>{c.motivations.join("、")}</p>}
                  {c.arc && <p className="dim">{c.arc.start_state} → {c.arc.end_state}</p>}
                  {c.relationships?.length > 0 && (
                    <div style={{ marginTop: 8 }}>
                      <strong>关系：</strong>
                      {c.relationships.map((r: any, j: number) => (
                        <span key={j} className="rel-tag">{r.target}（{r.rel_type}）</span>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
      {tab === "plot" && (
        <div>
          <div className="generate-bar">
            <button className="btn-primary" onClick={handleGenPlot} disabled={!!loading}>
              {loading === "plot" ? <><span className="loading-spinner" />生成中...</> : plot ? "重新生成" : "生成情节"}
            </button>
          </div>
          {plot?.acts && plot.acts.map((act: any, i: number) => (
            <div key={i} className="act-section">
              <h3>第{act.number}幕：{act.title}</h3>
              {act.theme && <p className="dim" style={{ marginBottom: 8 }}>主题：{act.theme}</p>}
              <div className="chapters-list">
                {act.chapters?.map((ch: any, j: number) => (
                  <div key={j} className="chapter-item clickable" onClick={() => onWriteChapter(ch.number)}>
                    <div className="chapter-header">
                      <span className="chapter-num">第{ch.number}章</span>
                      <span className="chapter-title">{ch.title}</span>
                      <button className="write-btn" onClick={e => { e.stopPropagation(); onWriteChapter(ch.number); }}>写作</button>
                    </div>
                    <p className="chapter-summary">{ch.summary}</p>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {tab === "chapter-list" && (
        <div>
          {allChapters.length === 0 ? (
            <div className="empty-state">
              <div className="empty-icon">📋</div>
              <p>请先生成情节大纲</p>
            </div>
          ) : (
            <>
              {/* Search Bar */}
              <div className="search-bar">
                <input
                  type="text"
                  placeholder="搜索章节内容（角色名、情节关键词...）"
                  value={searchQuery}
                  onChange={e => setSearchQuery(e.target.value)}
                  onKeyDown={e => e.key === "Enter" && handleSearch()}
                />
                <button className="btn-primary" onClick={handleSearch} disabled={searching || !searchQuery.trim()}>
                  {searching ? <><span className="loading-spinner" />搜索中</> : "搜索"}
                </button>
              </div>
              {searchResults && (
                <div className="search-results">
                  {searchResults.length === 0 ? (
                    <p className="dim" style={{ padding: 8 }}>未找到匹配结果</p>
                  ) : (
                    searchResults.map((r: any, i: number) => (
                      <div key={i} className="search-result-chapter">
                        <div className="search-result-header" onClick={() => onWriteChapter(r.chapter_number)} style={{ cursor: "pointer" }}>
                          <span className="chapter-ref">第{r.chapter_number}章</span>
                          <span>{r.title}</span>
                          <span className="dim">（{r.matches?.length} 处匹配）</span>
                        </div>
                        {r.matches?.slice(0, 3).map((m: any, j: number) => (
                          <div key={j} className="search-result-context">
                            ...{m.context}...
                          </div>
                        ))}
                      </div>
                    ))
                  )}
                </div>
              )}

              {/* Batch Generation Panel */}
              <div className="batch-panel">
                <h3>批量生成章节</h3>
                <div className="batch-controls">
                  <label>
                    从第
                    <select value={batchStart} onChange={e => setBatchStart(Number(e.target.value))}>
                      {allChapters.map(ch => <option key={ch.number} value={ch.number}>{ch.number}</option>)}
                    </select>
                    章
                  </label>
                  <label>
                    到第
                    <select value={batchEnd} onChange={e => setBatchEnd(Number(e.target.value))}>
                      {allChapters.map(ch => <option key={ch.number} value={ch.number}>{ch.number}</option>)}
                    </select>
                    章
                  </label>
                  <label className="batch-checkbox">
                    <input type="checkbox" checked={skipWritten} onChange={e => setSkipWritten(e.target.checked)} />
                    跳过已写章节
                  </label>
                  {!batchRunning ? (
                    <button className="btn-primary" onClick={handleBatchGenerate} disabled={!!loading}>
                      开始批量生成（{batchEnd - batchStart + 1} 章）
                    </button>
                  ) : (
                    <button className="btn-danger" onClick={handleCancelBatch}>取消生成</button>
                  )}
                </div>

                {/* Progress Display */}
                {batchRunning && batchProgress && (
                  <div className="batch-progress">
                    <div className="batch-progress-bar">
                      <div className="batch-progress-fill" style={{ width: `${(batchProgress.current / batchProgress.total) * 100}%` }} />
                    </div>
                    <div className="batch-progress-info">
                      <span>第 {batchProgress.chapter_number} 章</span>
                      <span className={`phase-badge phase-${batchProgress.phase}`}>
                        {batchProgress.phase === "context" && "构建上下文..."}
                        {batchProgress.phase === "generating" && "生成中..."}
                        {batchProgress.phase === "summarizing" && "摘要中..."}
                        {batchProgress.phase === "done" && `完成 ${batchProgress.word_count} 字`}
                        {batchProgress.phase === "skipped" && "已跳过"}
                        {batchProgress.phase === "failed" && `失败: ${batchProgress.error}`}
                        {batchProgress.phase === "cancelled" && "已取消"}
                      </span>
                      <span className="dim">{batchProgress.current}/{batchProgress.total}</span>
                    </div>
                  </div>
                )}

                {/* Completion Summary */}
                {batchResult && (
                  <div className="batch-result">
                    <span>完成 {batchResult.completed} 章</span>
                    {batchResult.skipped > 0 && <span className="dim">跳过 {batchResult.skipped}</span>}
                    {batchResult.failed > 0 && <span className="batch-failed">失败 {batchResult.failed}（第 {batchResult.failed_chapters.join("、")} 章）</span>}
                    <span className="dim">共 {batchResult.total_words} 字 · 耗时 {Math.floor(batchResult.elapsed_seconds / 60)}分{batchResult.elapsed_seconds % 60}秒</span>
                  </div>
                )}
              </div>

              <table className="chapter-table">
              <thead>
                <tr>
                  <th>章节</th>
                  <th>标题</th>
                  <th>字数</th>
                  <th>状态</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                {allChapters.map(ch => {
                  const text = chapterTexts[ch.number] || "";
                  const wc = text.length;
                  const written = wc > 0;
                  const batchStatus = chapterStatuses[ch.number];
                  return (
                    <tr key={ch.number}>
                      <td>第{ch.number}章</td>
                      <td>{ch.title}</td>
                      <td>{wc} 字</td>
                      <td>
                        {batchStatus ? (
                          <span className={`status-badge batch-${batchStatus}`}>
                            {batchStatus === "generating" && <><span className="loading-spinner" />生成中</>}
                            {batchStatus === "summarizing" && <><span className="loading-spinner" />摘要中</>}
                            {batchStatus === "context" && <><span className="loading-spinner" />准备中</>}
                            {batchStatus === "done" && "已生成"}
                            {batchStatus === "skipped" && "已跳过"}
                            {batchStatus === "failed" && "失败"}
                          </span>
                        ) : (
                          <span className={`status-badge ${written ? "written" : "unwritten"}`}>
                            {written ? "已写" : "未写"}
                          </span>
                        )}
                      </td>
                      <td>
                        <button className="btn-sm" onClick={() => onWriteChapter(ch.number)}>
                          {written ? "编辑" : "写作"}
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            </>
          )}
        </div>
      )}

      {tab === "tracking" && (
        <div>
          {!summaries || Object.keys(summaries).length === 0 ? (
            <div className="empty-state">
              <div className="empty-icon">📊</div>
              <p>暂无追踪数据，请先生成章节并运行摘要</p>
            </div>
          ) : (
            <>
              {/* Character State Timeline */}
              <div className="content-section">
                <h3>角色状态追踪</h3>
                {Object.keys(characterTimeline).length === 0 ? (
                  <p className="dim">暂无角色状态变化记录</p>
                ) : (
                  <div className="card-grid">
                    {Object.entries(characterTimeline).map(([name, changes]) => (
                      <div key={name} className="character-timeline-card">
                        <h4>{name} <span className="current-state-badge">{changes[changes.length - 1]?.change}</span></h4>
                        <div className="timeline-list">
                          {changes.map((c, i) => (
                            <div key={i} className="timeline-entry">
                              <span className="chapter-ref">第{c.chapter}章</span>
                              <span className="change-text">{c.change}</span>
                            </div>
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              {/* Foreshadowing Dashboard */}
              <div className="content-section">
                <h3>伏笔追踪 <span className="tag">活跃 {activeForeshadowing.length}</span> <span className="tag" style={{ background: "var(--success-light)", color: "var(--success)" }}>已回收 {resolvedForeshadowing.length}</span></h3>
                {foreshadowingItems.length === 0 ? (
                  <p className="dim">暂无伏笔记录</p>
                ) : (
                  <>
                    {activeForeshadowing.length > 0 && (
                      <div style={{ marginBottom: 16 }}>
                        <h4 style={{ fontSize: 14, marginBottom: 8 }}>活跃伏笔</h4>
                        {activeForeshadowing.map((f, i) => (
                          <div key={i} className="foreshadow-item foreshadow-active">
                            <div className="foreshadow-content">{f.content}</div>
                            <div className="foreshadow-meta">第{f.plantedChapter}章埋设</div>
                          </div>
                        ))}
                      </div>
                    )}
                    {resolvedForeshadowing.length > 0 && (
                      <div>
                        <h4 style={{ fontSize: 14, marginBottom: 8 }}>已回收</h4>
                        {resolvedForeshadowing.map((f, i) => (
                          <div key={i} className="foreshadow-item foreshadow-resolved">
                            <div className="foreshadow-content">{f.content}</div>
                            <div className="foreshadow-meta">第{f.plantedChapter}章 → 第{f.resolvedChapter}章</div>
                          </div>
                        ))}
                      </div>
                    )}
                  </>
                )}
              </div>

              {/* Consistency Check */}
              <div className="content-section">
                <h3>一致性检查</h3>
                <div className="generate-bar">
                  <button className="btn-primary" onClick={handleCheckConsistency} disabled={checkingConsistency}>
                    {checkingConsistency ? <><span className="loading-spinner" />检查中...</> : "运行 AI 一致性检查"}
                  </button>
                </div>
                {consistencyResult && (
                  <div style={{ marginTop: 16 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 16, marginBottom: 16 }}>
                      <div className="consistency-score">{consistencyResult.overall_score ?? "—"}</div>
                      <div>
                        <div style={{ fontWeight: 600 }}>一致性评分</div>
                        <div className="dim" style={{ fontSize: 13 }}>{consistencyResult.summary}</div>
                      </div>
                    </div>
                    {Array.isArray(consistencyResult.issues) && consistencyResult.issues.length > 0 ? (
                      consistencyResult.issues.map((issue: any, i: number) => (
                        <div key={i} className={`consistency-issue issue-severity-${issue.severity}`}>
                          <div className="issue-header">
                            <span className="issue-category">{
                              { character: "角色", timeline: "时间线", setting: "设定", foreshadowing: "伏笔", plot_hole: "情节漏洞" }[issue.category as string] || issue.category
                            }</span>
                            <span className="issue-location">{issue.location}</span>
                          </div>
                          <p style={{ margin: "4px 0", fontSize: 13 }}>{issue.description}</p>
                          {issue.suggestion && <p className="dim" style={{ fontSize: 12 }}>建议：{issue.suggestion}</p>}
                        </div>
                      ))
                    ) : (
                      <p className="dim">未发现一致性问题</p>
                    )}
                  </div>
                )}
              </div>

              {/* Style Analysis */}
              <div className="content-section">
                <h3>文风分析</h3>
                <div className="generate-bar">
                  <button className="btn-primary" onClick={handleAnalyzeStyle} disabled={analyzingStyle}>
                    {analyzingStyle ? <><span className="loading-spinner" />分析中...</> : "分析我的文风"}
                  </button>
                  {styleProfile && <span className="dim" style={{ fontSize: 12 }}>已学习，后续生成将自动应用</span>}
                </div>
                {styleProfile && (
                  <div style={{ marginTop: 12 }}>
                    <div className="style-summary">{styleProfile.summary}</div>
                    <div className="card-grid" style={{ marginTop: 12 }}>
                      {[
                        ["叙述视角", styleProfile.narrative_voice],
                        ["句式特征", styleProfile.sentence_style],
                        ["对话风格", styleProfile.dialogue_style],
                        ["描写详略", styleProfile.description_level],
                        ["叙事节奏", styleProfile.pacing_pattern],
                        ["用词倾向", styleProfile.vocabulary_tendency],
                        ["情感基调", styleProfile.emotional_tone],
                      ].filter(([, v]) => v).map(([label, value], i) => (
                        <div key={i} className="info-card" style={{ minWidth: 200 }}>
                          <h4 style={{ fontSize: 13 }}>{label}</h4>
                          <p style={{ fontSize: 12 }}>{value}</p>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>

              {/* Narrative Lines */}
              <div className="content-section">
                <h3>多线叙事</h3>
                {(() => {
                  const povLines: Record<string, Array<{ number: number; title: string }>> = {};
                  for (const act of (plot?.acts || [])) {
                    for (const ch of (act.chapters || [])) {
                      const pov = ch.pov_character || "全知视角";
                      if (!povLines[pov]) povLines[pov] = [];
                      povLines[pov].push({ number: ch.number, title: ch.title || "" });
                    }
                  }
                  const lineNames = Object.keys(povLines);
                  if (lineNames.length <= 1) {
                    return <p className="dim">仅检测到单一叙事线（或未设置 POV 角色）</p>;
                  }
                  return (
                    <div className="narrative-lines">
                      {lineNames.map(name => (
                        <div key={name} className="narrative-line">
                          <div className="narrative-line-label">{name}</div>
                          <div className="narrative-line-track">
                            {povLines[name].map(ch => (
                              <div key={ch.number} className="narrative-node" title={`第${ch.number}章 ${ch.title}`} onClick={() => onWriteChapter(ch.number)}>
                                {ch.number}
                              </div>
                            ))}
                          </div>
                        </div>
                      ))}
                    </div>
                  );
                })()}
              </div>
            </>
          )}
        </div>
      )}
      {showExport && (
        <ExportDialog
          projectId={project.id}
          projectTitle={project.title}
          onClose={() => setShowExport(false)}
        />
      )}
    </div>
  );
}
