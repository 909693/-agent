import { useEffect, useState } from "react";
import { api, type CreativeConstraintsPayload } from "../api";
import {
  getCreativeConstraints, setCreativeConstraints,
  matchPromptIdsForGenre, resolveEffectivePromptIds, AUTO_MANAGED_CATEGORIES,
} from "../utils/creativeConstraints";
import { type Prompt, loadPrompts } from "../utils/promptStore";

type SkillItem = { id: string; name: string; enabled: boolean };

export function CreativeConstraintsPanel({ genre, onChange, collapsible = false, defaultCollapsed = false }: { genre?: string; onChange?: (payload: CreativeConstraintsPayload) => void; collapsible?: boolean; defaultCollapsed?: boolean }) {
  const [skills, setSkills] = useState<SkillItem[]>([]);
  const [prompts] = useState<Prompt[]>(loadPrompts());
  const [selectedSkills, setSelectedSkills] = useState<string[]>(getCreativeConstraints().enabledSkillIds);
  const [selectedPrompts, setSelectedPrompts] = useState<string[]>(getCreativeConstraints().selectedPromptIds);
  const [mode, setMode] = useState<"strict" | "assist">(getCreativeConstraints().mode);
  const [autoPrompts, setAutoPrompts] = useState<boolean>(getCreativeConstraints().autoPrompts);
  const [open, setOpen] = useState(!defaultCollapsed);

  const autoActive = autoPrompts && !!genre;
  const autoIds = autoActive ? matchPromptIdsForGenre(genre).filter(id => prompts.some(p => p.id === id)) : [];
  const effectivePromptIds = resolveEffectivePromptIds(
    { enabledSkillIds: selectedSkills, selectedPromptIds: selectedPrompts, mode, autoPrompts },
    genre,
    prompts
  );

  useEffect(() => {
    api.listSkills().then((items: any) => {
      setSkills((Array.isArray(items) ? items : []).map((v: any) => ({ id: v.id, name: v.name, enabled: !!v.enabled })));
    }).catch(() => setSkills([]));
  }, []);

  useEffect(() => {
    const payload: CreativeConstraintsPayload = {
      mode,
      skills: skills.filter(s => selectedSkills.includes(s.id)).map(s => ({ id: s.id, name: s.name, content: `严格遵循技能 ${s.name} 的规则与写法要求。` })),
      prompts: prompts.filter(p => effectivePromptIds.includes(p.id)).map(p => ({ id: p.id, title: p.title, category: p.category, content: p.content })),
    };
    setCreativeConstraints({ enabledSkillIds: selectedSkills, selectedPromptIds: selectedPrompts, mode, autoPrompts });
    onChange?.(payload);
  }, [selectedSkills, selectedPrompts, mode, autoPrompts, skills, prompts, genre, onChange]);

  const toggle = (id: string, list: string[], setter: (v: string[]) => void) => {
    setter(list.includes(id) ? list.filter(x => x !== id) : [...list, id]);
  };

  const body = (
    <>
      <div className="constraints-head">
        <h4>创作约束</h4>
        <select value={mode} onChange={e => setMode(e.target.value as "strict" | "assist")}>
          <option value="strict">严格模式</option>
          <option value="assist">参考模式</option>
        </select>
      </div>
      <div className="constraints-block">
        <span className="constraints-label">Skills</span>
        <div className="constraints-chips">
          {skills.length === 0 ? <span className="constraints-empty">无已安装 Skills</span> : skills.map(s => (
            <button key={s.id} type="button" className={`constraint-chip ${selectedSkills.includes(s.id) ? "active" : ""}`} onClick={() => toggle(s.id, selectedSkills, setSelectedSkills)}>
              {s.name}
            </button>
          ))}
        </div>
      </div>
      <div className="constraints-block">
        <span className="constraints-label constraints-label-row">
          提示词
          {genre && (
            <label className="auto-prompts-toggle" title="按小说类型自动选用风格提示词；审校检查/自定义仍手动勾选">
              <input type="checkbox" checked={autoPrompts} onChange={e => setAutoPrompts(e.target.checked)} />
              按类型自动匹配
            </label>
          )}
        </span>
        {autoActive && autoIds.length === 0 && (
          <span className="constraints-empty">该类型暂无匹配的风格提示词，可关闭自动匹配后手动选择</span>
        )}
        <div className="constraints-chips">
          {prompts.length === 0 ? <span className="constraints-empty">无可用提示词</span> : prompts.map(p => {
            const isAutoManaged = autoActive && AUTO_MANAGED_CATEGORIES.includes(p.category);
            const active = effectivePromptIds.includes(p.id);
            return (
              <button
                key={p.id}
                type="button"
                className={`constraint-chip ${active ? "active" : ""} ${isAutoManaged ? "auto-locked" : ""}`}
                disabled={isAutoManaged}
                title={isAutoManaged ? "由类型自动匹配（关闭自动匹配可手动选择）" : undefined}
                onClick={() => !isAutoManaged && toggle(p.id, selectedPrompts, setSelectedPrompts)}
              >
                {p.title}{isAutoManaged && active ? " ·自动" : ""}
              </button>
            );
          })}
        </div>
      </div>
    </>
  );

  if (collapsible) {
    const summary = `${mode === "strict" ? "严格模式" : "参考模式"} · Skills ${selectedSkills.length} · 提示词 ${effectivePromptIds.length}${autoActive ? "（自动）" : ""}`;
    return (
      <div className={`constraints-panel collapsible ${open ? "open" : ""}`}>
        <button type="button" className="constraints-toggle" onClick={() => setOpen(v => !v)}>
          <span className="constraints-toggle-title">创作约束</span>
          <span className="constraints-toggle-summary">{summary}</span>
          <span className={`constraints-chevron ${open ? "open" : ""}`}>▾</span>
        </button>
        {open && <div className="constraints-body">{body}</div>}
      </div>
    );
  }

  return (
    <div className="constraints-panel">{body}</div>
  );
}
