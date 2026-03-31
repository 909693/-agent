import { useState, useEffect, useRef } from "react";
import { api, type LlmParams } from "../api";
import { logWords } from "../utils/writingLog";
import { CreativeConstraintsPanel } from "./CreativeConstraintsPanel";
import { buildCreativeConstraintsPayload } from "../utils/buildCreativeConstraints";

interface Props {
  projectId: string;
  llm: LlmParams;
  initialChapter?: number;
  onBack: () => void;
}

interface ChapterInfo {
  number: number;
  title: string;
  summary: string;
  povCharacter?: string;
  plotPoints?: string[];
}

interface CharacterInfo {
  id: string;
  name: string;
  role: string;
  personality?: string;
  backstory?: string;
  motivations?: string[];
  faction?: string;
  relationships?: Array<{ target?: string; rel_type?: string; description?: string }>;
  arc?: { start_state?: string; end_state?: string; internal_conflict?: string };
}
export function ChapterEditor({ projectId, llm, initialChapter = 1, onBack }: Props) {
  const [chapterNum, setChapterNum] = useState(initialChapter);
  const [text, setText] = useState("");
  const [userInput, setUserInput] = useState("");
  const [instruction, setInstruction] = useState("");
  const [targetWords, setTargetWords] = useState(3000);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [mode, setMode] = useState<"fill" | "partial" | "expand" | "continue" | "review">("fill");
  const [chapters, setChapters] = useState<ChapterInfo[]>([]);
  const [characters, setCharacters] = useState<CharacterInfo[]>([]);
  const [selectedCharacterId, setSelectedCharacterId] = useState("");
  const [saved, setSaved] = useState(false);
  const [reviewResult, setReviewResult] = useState("");
  const [platform, setPlatform] = useState("番茄");
  const [fillHint, setFillHint] = useState("");
  const [selectionText, setSelectionText] = useState("");
  const [selectionStart, setSelectionStart] = useState(0);
  const [selectionEnd, setSelectionEnd] = useState(0);
  const [partialHint, setPartialHint] = useState("");
  const [partialDelta, setPartialDelta] = useState(300);
  const [partialPreview, setPartialPreview] = useState("");
  const prevWordCount = useRef(0);
  const textAreaRef = useRef<HTMLTextAreaElement | null>(null);
  const [chapterContext, setChapterContext] = useState<any>(null);
  const [showStates, setShowStates] = useState(false);
  const [showForeshadowing, setShowForeshadowing] = useState(false);
  const [snapshots, setSnapshots] = useState<any[]>([]);
  const [showSnapshots, setShowSnapshots] = useState(false);

  useEffect(() => {
    api.getPlot(projectId).then((plot: any) => {
      const list: ChapterInfo[] = [];
      plot?.acts?.forEach((act: any) => {
        act.chapters?.forEach((ch: any) => {
          list.push({ number: ch.number, title: ch.title, summary: ch.summary, povCharacter: ch.pov_character, plotPoints: ch.plot_points });
        });
      });
      setChapters(list);
    }).catch(() => {});
    api.getCharacters(projectId).then((data: any) => {
      setCharacters(Array.isArray(data?.characters) ? data.characters : []);
    }).catch(() => setCharacters([]));
  }, [projectId]);

  useEffect(() => { setChapterNum(initialChapter); }, [initialChapter]);

  useEffect(() => {
    setSaved(false);
    api.getChapter(projectId, chapterNum)
      .then((d: any) => {
        const t = d.text || "";
        setText(t);
        prevWordCount.current = t.length;
      })
      .catch(() => { setText(""); prevWordCount.current = 0; });
  }, [projectId, chapterNum]);

  useEffect(() => {
    api.buildChapterContext(projectId, chapterNum)
      .then(setChapterContext)
      .catch(() => setChapterContext(null));
  }, [projectId, chapterNum]);

  const currentChapter = chapters.find(c => c.number === chapterNum);
  const relevantCharacters = characters.filter((c) => {
    const chapterText = `${currentChapter?.summary || ""} ${currentChapter?.title || ""}`;
    const byName = c.name && chapterText.includes(c.name);
    const byPov = currentChapter?.povCharacter && (currentChapter.povCharacter === c.name || currentChapter.povCharacter === c.id);
    return byName || byPov;
  });
  const displayedCharacters = relevantCharacters.length > 0 ? relevantCharacters : characters.slice(0, 6);
  const selectedCharacter = displayedCharacters.find((c) => c.id === selectedCharacterId) || displayedCharacters[0] || null;
  const chapterNotes = selectedCharacter ? [
    currentChapter?.povCharacter && (currentChapter.povCharacter === selectedCharacter.id || currentChapter.povCharacter === selectedCharacter.name)
      ? `本章 POV 角色是 ${selectedCharacter.name}，叙事要优先贴合他的视角和认知边界。`
      : `${selectedCharacter.name} 在本章中应保持与既有人设一致，避免突然 OOC。`,
    selectedCharacter.personality ? `性格关键词：${selectedCharacter.personality}` : null,
    selectedCharacter.arc?.internal_conflict ? `当前写作要持续体现他的内在冲突：${selectedCharacter.arc.internal_conflict}` : null,
    selectedCharacter.motivations?.length ? `推动行为的核心动机：${selectedCharacter.motivations.join("、")}` : null,
    currentChapter?.summary ? `本章剧情重点：${currentChapter.summary}` : null,
  ].filter(Boolean) as string[] : [];

  useEffect(() => {
    if (displayedCharacters.length > 0 && !displayedCharacters.find((c) => c.id === selectedCharacterId)) {
      setSelectedCharacterId(displayedCharacters[0].id);
    }
  }, [chapterNum, displayedCharacters, selectedCharacterId]);
  const handleFillToTarget = async () => {
    if (!llm.apiKey) { setError("请先配置 API Key"); return; }
    if (!currentChapter) { setError("缺少章节大纲"); return; }
    if (!text.trim() && !fillHint.trim()) { setError("请至少提供现有正文或补充说明"); return; }
    setLoading(true); setError("");
    try {
      const payload = await buildCreativeConstraintsPayload();
      const existingContext = text.trim() ? `以下是我已经写好的正文，请严格保留已写内容的核心情节和表达方向，只在不足处补充扩写：\n\n${text}` : "";
      const hintContext = fillHint.trim() ? `\n\n补充要求：${fillHint}` : "";
      const result: any = await api.expandChapter(projectId, chapterNum, `${existingContext}${hintContext}`, targetWords, llm, payload);
      setText(result.text || JSON.stringify(result));
      setSaved(false);
    } catch (e: any) { setError(e.toString()); }
    setLoading(false);
  };

  const handleExpand = async () => {
    if (!llm.apiKey) { setError("请先配置 API Key"); return; }
    setLoading(true); setError("");
    try {
      const payload = await buildCreativeConstraintsPayload();
      const result: any = await api.expandChapter(projectId, chapterNum, userInput, targetWords, llm, payload);
      setText(result.text || JSON.stringify(result));
      setSaved(false);
    } catch (e: any) { setError(e.toString()); }
    setLoading(false);
  };

  const handleContinue = async () => {
    if (!llm.apiKey) { setError("请先配置 API Key"); return; }
    setLoading(true); setError("");
    try {
      const payload = await buildCreativeConstraintsPayload();
      const result: any = await api.continueWriting(projectId, chapterNum, instruction, targetWords, llm, payload);
      setText(result.text || JSON.stringify(result));
      setSaved(false);
    } catch (e: any) { setError(e.toString()); }
    setLoading(false);
  };

  const handleReview = async () => {
    if (!llm.apiKey) { setError("请先配置 API Key"); return; }
    if (!text.trim()) { setError("章节内容为空，无法审校"); return; }
    setLoading(true); setError(""); setReviewResult("");
    try {
      const payload = await buildCreativeConstraintsPayload();
      const result = await api.reviewChapter(projectId, chapterNum, text, platform, llm, payload);
      setReviewResult(result);
    } catch (e: any) { setError(e.toString()); }
    setLoading(false);
  };

  const handleSelectionChange = () => {
    const el = textAreaRef.current;
    if (!el) return;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    setSelectionStart(start);
    setSelectionEnd(end);
    setSelectionText(start !== end ? text.slice(start, end) : "");
  };

  const handlePartialRewrite = async () => {
    if (!llm.apiKey) { setError("请先配置 API Key"); return; }
    if (!selectionText.trim()) { setError("请先在正文中选中一段内容"); return; }
    setLoading(true); setError("");
    try {
      const payload = await buildCreativeConstraintsPayload();
      const result: any = await api.rewriteSelection(projectId, chapterNum, selectionText, partialHint, partialDelta, llm, payload);
      const replacement = result.text || "";
      setPartialPreview(replacement);
      setSaved(false);
    } catch (e: any) { setError(e.toString()); }
    setLoading(false);
  };

  const applyPartialRewrite = () => {
    if (!partialPreview) return;
    const newText = text.slice(0, selectionStart) + partialPreview + text.slice(selectionEnd);
    setText(newText);
    setSelectionText(partialPreview);
    setPartialPreview("");
    setSaved(false);
  };

  const handleSave = async () => {
    try {
      await api.saveChapter(projectId, chapterNum, text);
      const delta = text.length - prevWordCount.current;
      if (delta > 0) logWords(delta);
      prevWordCount.current = text.length;
      setSaved(true);
      setError("");
    } catch (e: any) { setError(e.toString()); }
  };

  const loadSnapshots = async () => {
    try {
      const list = await api.listChapterSnapshots(projectId, chapterNum);
      setSnapshots(Array.isArray(list) ? list : []);
      setShowSnapshots(true);
    } catch { setSnapshots([]); }
  };

  const handleRestore = async (file: string) => {
    if (!confirm("确定恢复此版本？当前内容将被备份后替换。")) return;
    try {
      const result: any = await api.restoreSnapshot(projectId, chapterNum, file);
      setText(result.text || "");
      prevWordCount.current = (result.text || "").length;
      setSaved(true);
      setShowSnapshots(false);
    } catch (e: any) { setError(e.toString()); }
  };

  const goNext = () => {
    const idx = chapters.findIndex(c => c.number === chapterNum);
    if (idx >= 0 && idx < chapters.length - 1) setChapterNum(chapters[idx + 1].number);
    else setChapterNum(chapterNum + 1);
  };

  const goPrev = () => {
    const idx = chapters.findIndex(c => c.number === chapterNum);
    if (idx > 0) setChapterNum(chapters[idx - 1].number);
    else if (chapterNum > 1) setChapterNum(chapterNum - 1);
  };

  const wordCount = text.length;
  return (
    <div>
      <div className="editor-header">
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <button className="btn-outline" onClick={onBack}>← 返回</button>
          <h2 style={{ fontSize: 18, fontWeight: 600 }}>章节写作</h2>
        </div>
        <div className="chapter-controls">
          <button onClick={goPrev}>←</button>
          <select value={chapterNum} onChange={e => setChapterNum(Number(e.target.value))}>
            {chapters.length > 0 ? chapters.map(c => (
              <option key={c.number} value={c.number}>第{c.number}章：{c.title}</option>
            )) : (
              <option value={chapterNum}>第 {chapterNum} 章</option>
            )}
          </select>
          <button onClick={goNext}>→</button>
          <span className="word-count">{wordCount} 字</span>
          {saved && <span className="saved-tag">✓ 已保存</span>}
        </div>
      </div>

      {currentChapter && (
        <div className="chapter-outline-bar">
          <span className="outline-label">大纲：</span>
          {currentChapter.summary}
        </div>
      )}

      {error && <div className="error">{error}</div>}

      <div className="editor-layout">
        <div className="editor-main">
          <textarea
            ref={textAreaRef}
            className="chapter-text"
            value={text}
            onChange={e => { setText(e.target.value); setSaved(false); }}
            onSelect={handleSelectionChange}
            onKeyUp={handleSelectionChange}
            onMouseUp={handleSelectionChange}
            placeholder={currentChapter ? `开始写第${chapterNum}章：${currentChapter.title}...` : "章节内容..."}
          />
          <div className="editor-bottom-bar">
            <button className="save-btn" onClick={handleSave}>保存</button>
            <button className="btn-outline" onClick={loadSnapshots}>历史版本</button>
            <button className="next-chapter-btn" onClick={() => { handleSave(); goNext(); }}>
              保存并下一章 →
            </button>
          </div>
          {showSnapshots && (
            <div className="snapshots-panel">
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 8 }}>
                <h4 style={{ margin: 0 }}>历史版本</h4>
                <button className="btn-sm" onClick={() => setShowSnapshots(false)}>关闭</button>
              </div>
              {snapshots.length === 0 ? (
                <p className="dim" style={{ fontSize: 13 }}>暂无历史版本</p>
              ) : (
                snapshots.map((s: any, i: number) => (
                  <div key={i} className="snapshot-item">
                    <div>
                      <span style={{ fontSize: 13 }}>{new Date(s.timestamp).toLocaleString()}</span>
                      <span className="dim" style={{ marginLeft: 8, fontSize: 12 }}>{s.word_count} 字</span>
                    </div>
                    <button className="btn-sm" onClick={() => handleRestore(s.file)}>恢复</button>
                  </div>
                ))
              )}
            </div>
          )}
        </div>
        <div className="editor-panel">
          <CreativeConstraintsPanel />
          <div className="character-reference-panel">
            <div className="character-reference-head">
              <h4>角色参考</h4>
              <span>{displayedCharacters.length} 人</span>
            </div>
            {displayedCharacters.length === 0 ? (
              <div className="character-reference-empty">暂无角色数据，先去生成角色。</div>
            ) : (
              <>
                <div className="character-reference-tabs">
                  {displayedCharacters.map((c) => (
                    <button key={c.id} className={`constraint-chip ${selectedCharacter?.id === c.id ? "active" : ""}`} onClick={() => setSelectedCharacterId(c.id)}>
                      {c.name}
                    </button>
                  ))}
                </div>
                {selectedCharacter && (
                  <div className="character-reference-card">
                    <div className="character-reference-title-row">
                      <strong>{selectedCharacter.name}</strong>
                      <span className="tag">{selectedCharacter.role || "角色"}</span>
                    </div>
                    {selectedCharacter.personality && <p><b>性格：</b>{selectedCharacter.personality}</p>}
                    {selectedCharacter.motivations?.length ? <p><b>动机：</b>{selectedCharacter.motivations.join("、")}</p> : null}
                    {selectedCharacter.faction ? <p><b>阵营：</b>{selectedCharacter.faction}</p> : null}
                    {selectedCharacter.arc?.internal_conflict ? <p><b>内在冲突：</b>{selectedCharacter.arc.internal_conflict}</p> : null}
                    {selectedCharacter.relationships?.length ? (
                      <div className="character-reference-relations">
                        <b>关系：</b>
                        {selectedCharacter.relationships.slice(0, 4).map((rel, idx) => (
                          <span key={idx} className="rel-tag">{rel.target || "角色"}{rel.rel_type ? `（${rel.rel_type}）` : ""}</span>
                        ))}
                      </div>
                    ) : null}
                    {chapterNotes.length > 0 && (
                      <div className="character-note-box">
                        <strong>本章写作注意事项</strong>
                        <ul>
                          {chapterNotes.map((note, idx) => <li key={idx}>{note}</li>)}
                        </ul>
                      </div>
                    )}
                  </div>
                )}
              </>
            )}
          </div>

          {/* Compact Tracking Panels */}
          {chapterContext && (
            <div style={{ display: "flex", flexDirection: "column", gap: 8, marginBottom: 8 }}>
              {/* Character States */}
              {Array.isArray(chapterContext.character_states) && chapterContext.character_states.length > 0 && (
                <div className="tracking-compact-panel">
                  <div className="tracking-compact-head" onClick={() => setShowStates(!showStates)}>
                    <h4>前情状态 ({chapterContext.character_states.length})</h4>
                    <span>{showStates ? "▼" : "▶"}</span>
                  </div>
                  {showStates && (
                    <div className="tracking-compact-body">
                      {chapterContext.character_states.slice(-8).map((s: any, i: number) => (
                        <div key={i} className="tracking-compact-item">
                          <strong>{s.name}</strong>：{s.change} <span className="dim">（第{s.chapter}章）</span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
              {/* Active Foreshadowing */}
              {Array.isArray(chapterContext.active_foreshadowing) && chapterContext.active_foreshadowing.length > 0 && (
                <div className="tracking-compact-panel">
                  <div className="tracking-compact-head" onClick={() => setShowForeshadowing(!showForeshadowing)}>
                    <h4>活跃伏笔 <span className="tag">{chapterContext.active_foreshadowing.length}</span></h4>
                    <span>{showForeshadowing ? "▼" : "▶"}</span>
                  </div>
                  {showForeshadowing && (
                    <div className="tracking-compact-body">
                      {chapterContext.active_foreshadowing.map((f: string, i: number) => (
                        <span key={i} className="foreshadow-chip">{f}</span>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          )}

          <div className="mode-switch">
            <button className={mode === "fill" ? "active" : ""} onClick={() => setMode("fill")}>补足字数</button>
            <button className={mode === "partial" ? "active" : ""} onClick={() => setMode("partial")}>局部补写</button>
            <button className={mode === "expand" ? "active" : ""} onClick={() => setMode("expand")}>按大纲扩写</button>
            <button className={mode === "continue" ? "active" : ""} onClick={() => setMode("continue")}>逐段续写</button>
            <button className={mode === "review" ? "active" : ""} onClick={() => setMode("review")}>AI审校</button>
          </div>

          {mode === "review" ? (
            <>
              <label>
                目标平台
                <select value={platform} onChange={e => setPlatform(e.target.value)}>
                  <option value="番茄">番茄小说</option>
                  <option value="起点">起点中文网</option>
                  <option value="纵横">纵横中文网</option>
                </select>
              </label>
              <button onClick={handleReview} disabled={loading || !text.trim()}>
                {loading ? <><span className="loading-spinner" />审校中...</> : "开始审校"}
              </button>
              {reviewResult && (
                <div className="review-result">
                  {reviewResult}
                </div>
              )}
            </>
          ) : mode === "fill" ? (
            <>
              <label>
                目标字数
                <input type="number" value={targetWords} onChange={e => setTargetWords(Number(e.target.value))} min={500} step={500} />
              </label>
              <div className="fill-word-info">
                当前 {wordCount} 字 / 目标 {targetWords} 字 {targetWords > wordCount ? `（还差 ${targetWords - wordCount} 字）` : "（已达到或超过目标）"}
              </div>
              <label>
                补充要求（可选）
                <textarea value={fillHint} onChange={e => setFillHint(e.target.value)} rows={5} placeholder="比如：加强打斗细节、补充心理描写、增加对话与场景过渡，但不要改变核心剧情。" />
              </label>
              <button onClick={handleFillToTarget} disabled={loading || targetWords <= wordCount}>
                {loading ? <><span className="loading-spinner" />补充中...</> : "补足到目标字数"}
              </button>
            </>
          ) : mode === "partial" ? (
            <>
              <div className="fill-word-info">
                {selectionText ? `已选中 ${selectionText.length} 字，将只重写这一段。` : "请先在左侧正文中选中要补写的段落。"}
              </div>
              <div className="partial-quick-actions">
                {[
                  "增强人物对话",
                  "补充心理描写",
                  "增加动作细节",
                  "强化场景氛围",
                  "制造章节钩子",
                ].map((preset) => (
                  <button key={preset} type="button" className="constraint-chip" onClick={() => setPartialHint(preset)}>
                    {preset}
                  </button>
                ))}
              </div>
              <label>
                补写方向
                <textarea value={partialHint} onChange={e => setPartialHint(e.target.value)} rows={5} placeholder="比如：增强人物对话、补充心理描写、增加动作与场景细节、让节奏更紧张。" />
              </label>
              <label>
                增量字数
                <input type="number" value={partialDelta} onChange={e => setPartialDelta(Number(e.target.value))} min={100} step={100} />
              </label>
              <button onClick={handlePartialRewrite} disabled={loading || !selectionText.trim()}>
                {loading ? <><span className="loading-spinner" />生成补写预览...</> : "生成补写预览"}
              </button>
              {partialPreview && (
                <div className="partial-preview-box">
                  <div className="partial-preview-head"><strong>原文</strong><strong>补写预览</strong></div>
                  <div className="partial-preview-grid">
                    <pre>{selectionText}</pre>
                    <pre>{partialPreview}</pre>
                  </div>
                  <div className="partial-preview-actions">
                    <button className="btn-outline" onClick={() => setPartialPreview("")}>取消</button>
                    <button onClick={applyPartialRewrite}>确认替换</button>
                  </div>
                </div>
              )}
            </>
          ) : (
            <>
              <label>
                目标字数
                <input type="number" value={targetWords} onChange={e => setTargetWords(Number(e.target.value))} min={500} step={500} />
              </label>

              {mode === "expand" ? (
                <>
                  <label>
                    你的内容/要点（可选）
                    <textarea value={userInput} onChange={e => setUserInput(e.target.value)} rows={6} placeholder="写下情节要点、对话片段...留空则根据大纲生成" />
                  </label>
                  <button onClick={handleExpand} disabled={loading}>
                    {loading ? <><span className="loading-spinner" />扩写中...</> : "开始扩写"}
                  </button>
                </>
              ) : (
                <>
                  <label>
                    续写指示
                    <textarea value={instruction} onChange={e => setInstruction(e.target.value)} rows={6} placeholder="描述接下来的情节走向..." />
                  </label>
                  <button onClick={handleContinue} disabled={loading}>
                    {loading ? <><span className="loading-spinner" />续写中...</> : "开始续写"}
                  </button>
                </>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
