import { useEffect, useState } from "react";
import { api, type CreativeConstraintsPayload } from "../api";
import { getCreativeConstraints, setCreativeConstraints } from "../utils/creativeConstraints";

type PromptItem = { id: string; title: string; category: string; content: string };
type SkillItem = { id: string; name: string; enabled: boolean };

const PROMPTS_KEY = "retl_prompts";

function loadPrompts(): PromptItem[] {
  try {
    const raw = localStorage.getItem(PROMPTS_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

export function CreativeConstraintsPanel({ onChange }: { onChange?: (payload: CreativeConstraintsPayload) => void }) {
  const [skills, setSkills] = useState<SkillItem[]>([]);
  const [prompts] = useState<PromptItem[]>(loadPrompts());
  const [selectedSkills, setSelectedSkills] = useState<string[]>(getCreativeConstraints().enabledSkillIds);
  const [selectedPrompts, setSelectedPrompts] = useState<string[]>(getCreativeConstraints().selectedPromptIds);
  const [mode, setMode] = useState<"strict" | "assist">(getCreativeConstraints().mode);

  useEffect(() => {
    api.listSkills().then((items: any) => {
      setSkills((Array.isArray(items) ? items : []).map((v: any) => ({ id: v.id, name: v.name, enabled: !!v.enabled })));
    }).catch(() => setSkills([]));
  }, []);

  useEffect(() => {
    const payload: CreativeConstraintsPayload = {
      mode,
      skills: skills.filter(s => selectedSkills.includes(s.id)).map(s => ({ id: s.id, name: s.name, content: `严格遵循技能 ${s.name} 的规则与写法要求。` })),
      prompts: prompts.filter(p => selectedPrompts.includes(p.id)).map(p => ({ id: p.id, title: p.title, category: p.category, content: p.content })),
    };
    setCreativeConstraints({ enabledSkillIds: selectedSkills, selectedPromptIds: selectedPrompts, mode });
    onChange?.(payload);
  }, [selectedSkills, selectedPrompts, mode, skills, prompts, onChange]);

  const toggle = (id: string, list: string[], setter: (v: string[]) => void) => {
    setter(list.includes(id) ? list.filter(x => x !== id) : [...list, id]);
  };

  return (
    <div className="constraints-panel">
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
        <span className="constraints-label">提示词</span>
        <div className="constraints-chips">
          {prompts.length === 0 ? <span className="constraints-empty">无可用提示词</span> : prompts.slice(0, 12).map(p => (
            <button key={p.id} type="button" className={`constraint-chip ${selectedPrompts.includes(p.id) ? "active" : ""}`} onClick={() => toggle(p.id, selectedPrompts, setSelectedPrompts)}>
              {p.title}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
