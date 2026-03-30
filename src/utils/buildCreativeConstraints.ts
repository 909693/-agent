import { api, type CreativeConstraintsPayload } from "../api";
import { getCreativeConstraints } from "./creativeConstraints";

type PromptItem = { id: string; title: string; category: string; content: string };

const PROMPTS_KEY = "retl_prompts";

function loadPrompts(): PromptItem[] {
  try {
    const raw = localStorage.getItem(PROMPTS_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

export async function buildCreativeConstraintsPayload(): Promise<CreativeConstraintsPayload> {
  const state = getCreativeConstraints();
  const prompts = loadPrompts().filter((p) => state.selectedPromptIds.includes(p.id));
  const skillsRaw: any[] = await api.listSkills();
  const selectedSkills = (Array.isArray(skillsRaw) ? skillsRaw : []).filter((s) => state.enabledSkillIds.includes(s.id));

  const skillDetails = await Promise.all(
    selectedSkills.map(async (s) => {
      try {
        const detail: any = await api.getSkillDetail(s.id);
        const pieces = [detail.skill, detail.readme, ...(Array.isArray(detail.references) ? detail.references.map((r: any) => r.content) : [])]
          .filter(Boolean)
          .join("\n\n");
        return { id: s.id, name: s.name || s.id, content: pieces };
      } catch {
        return { id: s.id, name: s.name || s.id, content: "" };
      }
    })
  );

  return {
    mode: state.mode,
    skills: skillDetails,
    prompts,
  };
}
