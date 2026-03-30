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

type ManagerTab = "world" | "characters" | "plot" | "chapter-list";

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

  useEffect(() => {
    api.getWorld(project.id).then(setWorld).catch(() => setWorld(null));
    api.getCharacters(project.id).then(setCharacters).catch(() => setCharacters(null));
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
          {([["world", "世界观"], ["characters", "角色"], ["plot", "情节大纲"], ["chapter-list", "章节列表"]] as const).map(([key, label]) => (
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
