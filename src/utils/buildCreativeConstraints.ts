import { api, type CreativeConstraintsPayload } from "../api";
import { getCreativeConstraints, resolveEffectivePromptIds } from "./creativeConstraints";
import { getGenrePromptHint } from "../components/GenreManager";
import { loadPrompts } from "./promptStore";


export async function buildCreativeConstraintsPayload(genre?: string): Promise<CreativeConstraintsPayload> {
  const state = getCreativeConstraints();
  const allStoredPrompts = loadPrompts();
  const effectiveIds = resolveEffectivePromptIds(state, genre, allStoredPrompts);
  // Keep effectiveIds order (auto-matched style prompts first) — the backend
  // caps the injected prompt count, so ordering decides what survives.
  const prompts = effectiveIds
    .map((id) => allStoredPrompts.find((p) => p.id === id))
    .filter((p): p is NonNullable<typeof p> => !!p);
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
