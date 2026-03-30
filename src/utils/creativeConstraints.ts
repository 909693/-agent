export interface CreativeConstraints {
  enabledSkillIds: string[];
  selectedPromptIds: string[];
  mode: "strict" | "assist";
}

const STORAGE_KEY = "retl_creative_constraints";

const DEFAULT_CONSTRAINTS: CreativeConstraints = {
  enabledSkillIds: [],
  selectedPromptIds: [],
  mode: "strict",
};

export function getCreativeConstraints(): CreativeConstraints {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_CONSTRAINTS };
    const parsed = JSON.parse(raw);
    return {
      enabledSkillIds: Array.isArray(parsed.enabledSkillIds) ? parsed.enabledSkillIds : [],
      selectedPromptIds: Array.isArray(parsed.selectedPromptIds) ? parsed.selectedPromptIds : [],
      mode: parsed.mode === "assist" ? "assist" : "strict",
    };
  } catch {
    return { ...DEFAULT_CONSTRAINTS };
  }
}

export function setCreativeConstraints(value: CreativeConstraints) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
}
