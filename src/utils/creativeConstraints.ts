export interface CreativeConstraints {
  enabledSkillIds: string[];
  selectedPromptIds: string[];
  mode: "strict" | "assist";
  /** 按小说类型自动匹配风格类提示词（审校/自定义仍手动） */
  autoPrompts: boolean;
}

const STORAGE_KEY = "retl_creative_constraints";

const DEFAULT_CONSTRAINTS: CreativeConstraints = {
  enabledSkillIds: [],
  selectedPromptIds: [],
  mode: "strict",
  autoPrompts: true,
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
      autoPrompts: parsed.autoPrompts !== false,
    };
  } catch {
    return { ...DEFAULT_CONSTRAINTS };
  }
}

export function setCreativeConstraints(value: CreativeConstraints) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
}

// ===== 按类型自动匹配提示词 =====

/** 自动模式只接管风格类分类；审校检查/自定义始终由用户手动勾选 */
export const AUTO_MANAGED_CATEGORIES = ["场景描写", "人物对话", "战斗场面", "情感描写", "环境氛围"];

// 内置类型 key（CreateProjectDialog/ChatCreator 使用）→ 中文关键词
const GENRE_KEY_ALIASES: Record<string, string> = {
  fantasy: "玄幻", scifi: "科幻", urban: "都市", romance: "言情",
  mystery: "悬疑", history: "历史", horror: "恐怖",
};

// 类型关键词 → 默认提示词 id（对应 PromptLibrary 的内置提示词）
const GENRE_PROMPT_RULES: { keywords: string[]; promptIds: string[] }[] = [
  { keywords: ["武侠", "江湖"], promptIds: ["d1", "d5"] },
  { keywords: ["玄幻", "仙侠", "修真", "修仙", "奇幻", "异世界"], promptIds: ["d2b", "d5b", "d10b"] },
  { keywords: ["科幻", "未来", "星际", "赛博", "太空"], promptIds: ["d2c", "d6"] },
  { keywords: ["言情", "爱情", "甜宠", "虐恋", "浪漫", "恋爱"], promptIds: ["d3b", "d7", "d10"] },
  { keywords: ["悬疑", "推理", "侦探", "犯罪"], promptIds: ["d9b", "d6b"] },
  { keywords: ["历史", "王朝", "宫廷", "古代"], promptIds: ["d1", "d3d", "d10b"] },
  { keywords: ["恐怖", "灵异", "惊悚", "怪谈"], promptIds: ["d9", "d7d"] },
  { keywords: ["游戏", "网游", "电竞", "无限流"], promptIds: ["d2b", "d5b"] },
  { keywords: ["末日", "废土", "丧尸", "求生"], promptIds: ["d2c", "d7d"] },
  { keywords: ["都市", "职场", "商战", "现代"], promptIds: ["d2", "d3", "d3d"] },
];

/** 解析项目 genre（内置 key / 自定义类型的 id 或名称）为可做关键词匹配的名称 */
function resolveGenreName(genre: string): string {
  const alias = GENRE_KEY_ALIASES[genre];
  if (alias) return alias;
  try {
    const raw = localStorage.getItem("retl_genres");
    const genres: { id: string; name: string }[] = raw ? JSON.parse(raw) : [];
    const match = genres.find((g) => g.id === genre || g.name === genre);
    if (match) return match.name;
  } catch { /* ignore */ }
  return genre;
}

/** 按小说类型匹配默认风格提示词 id；无匹配返回 [] */
export function matchPromptIdsForGenre(genre?: string): string[] {
  if (!genre) return [];
  const name = resolveGenreName(genre);
  for (const rule of GENRE_PROMPT_RULES) {
    if (rule.keywords.some((k) => name.includes(k))) return rule.promptIds;
  }
  return [];
}

/** 计算最终生效的提示词 id：
 *  自动模式下 = 类型匹配的风格提示词 + 用户手动勾选的非风格类（审校/自定义）；
 *  手动模式或无类型信息时 = 用户勾选原样生效 */
export function resolveEffectivePromptIds(
  state: CreativeConstraints,
  genre: string | undefined,
  prompts: { id: string; category: string }[]
): string[] {
  if (!state.autoPrompts || !genre) return state.selectedPromptIds;
  const auto = matchPromptIdsForGenre(genre).filter((id) => prompts.some((p) => p.id === id));
  const manualKept = state.selectedPromptIds.filter((id) => {
    const p = prompts.find((x) => x.id === id);
    return p && !AUTO_MANAGED_CATEGORIES.includes(p.category);
  });
  return [...auto, ...manualKept.filter((id) => !auto.includes(id))];
}
