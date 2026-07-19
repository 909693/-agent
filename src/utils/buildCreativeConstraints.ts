import { api, type CreativeConstraintsPayload } from "../api";
import { getCreativeConstraints } from "./creativeConstraints";
import { getGenrePromptHint } from "../components/GenreManager";

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

export async function buildCreativeConstraintsPayload(genre?: string): Promise<CreativeConstraintsPayload> {
  const state = getCreativeConstraints();
  const prompts = loadPrompts().filter((p) => state.selectedPromptIds.includes(p.id));
  const skillsRaw: any[] = await api.listSkills();
  // Only inject skills that are BOTH selected in the constraints panel AND still
  // enabled in the skills manager (s.enabled === false means the user disabled it).
  const selectedSkills = (Array.isArray(skillsRaw) ? skillsRaw : []).filter(
    (s) => state.enabledSkillIds.includes(s.id) && s.enabled !== false
  );

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

  // Inject the project genre's authoring guide (promptHint) as a constraint so
  // it actually influences world/character/plot/chapter generation.
  const genreHint = genre ? getGenrePromptHint(genre) : "";
  const allPrompts = genreHint
    ? [...prompts, { id: "__genre_guide__", title: "类型创作指引", category: "genre", content: genreHint }]
    : prompts;

  return {
    mode: state.mode,
    skills: skillDetails,
    prompts: allPrompts,
  };
}
