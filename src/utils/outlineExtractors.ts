function pickSection(text: string, keywords: string[]) {
  const lines = text.split(/\r?\n/);
  const result: string[] = [];
  let capture = false;
  for (const raw of lines) {
    const line = raw.trim();
    if (!line) continue;
    if (keywords.some((k) => line.includes(k))) {
      capture = true;
      continue;
    }
    if (capture && /^(第[一二三四五六七八九十0-9]+[章节卷幕]|[一二三四五六七八九十]+、|【.+】|#)/.test(line)) break;
    if (capture) result.push(line);
  }
  return result.join("\n");
}

export function extractWorldFromOutline(text: string) {
  const overview = pickSection(text, ["基础世界观", "世界观", "背景设定", "故事背景"]) || text.slice(0, 600);
  const history = pickSection(text, ["历史", "前史", "时代背景"]).split(/\n+/).filter(Boolean).slice(0, 8);
  const culture = pickSection(text, ["规则", "势力", "设定", "世界规则", "修炼体系"]).split(/\n+/).filter(Boolean).slice(0, 8);
  return {
    era: "待补充",
    overview,
    geography: [],
    rules: culture.slice(0, 5).map((line, idx) => ({ category: "world", name: `规则${idx + 1}`, description: line, limitations: [], plot_implications: [] })),
    factions: [],
    history,
    culture_notes: culture,
  };
}

const headingPattern = /^(?:[一二三四五六七八九十]+、)?(主角|男主角|男主|女主角|女主|反派|首席反派|阶段性反派|隐藏反派|配角|角色|挚友)[：:]\s*(.+)$/;

function roleFromLabel(label: string) {
  if (label.includes("反派")) return "antagonist";
  if (label.includes("配角") || label.includes("挚友")) return "supporting";
  return "protagonist";
}

function parseField(block: string[], keys: string[]) {
  const line = block.find((l) => keys.some((k) => l.includes(k)));
  if (!line) return "";
  const idx = line.indexOf("：") >= 0 ? line.indexOf("：") : line.indexOf(":");
  return idx >= 0 ? line.slice(idx + 1).trim() : line.trim();
}

export function extractCharactersFromOutline(text: string) {
  const lines = text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  const seen = new Set<string>();
  const blocks: Array<{ name: string; roleLabel: string; block: string[] }> = [];

  for (let i = 0; i < lines.length; i++) {
    const head = lines[i].match(headingPattern);
    if (!head) continue;

    const roleLabel = head[1];
    const rawName = head[2];
    const name = rawName.split(/[，,。；;（(·]/)[0].trim();
    if (!name || seen.has(name)) continue;
    seen.add(name);

    const block: string[] = [];
    let j = i + 1;
    while (j < lines.length) {
      const line = lines[j];
      if (headingPattern.test(line)) break;
      if (/^(第[一二三四五六七八九十0-9]+[章节卷幕]|[一二三四五六七八九十]+、[^：:]+|────────────────)/.test(line)) break;
      block.push(line);
      j += 1;
    }
    blocks.push({ name, roleLabel, block });
    i = j - 1;
  }

  const names = blocks.map((b) => b.name);
  const characters = blocks.map(({ name, roleLabel, block }, idx) => {
    const age = parseField(block, ["年龄"]);
    const appearance = parseField(block, ["外貌", "形象"]);
    const personality = parseField(block, ["性格"]);
    const background = parseField(block, ["背景", "家世", "定位"]);
    const motivationsText = parseField(block, ["核心矛盾", "目标", "命运设计", "功能"]);
    const faction = parseField(block, ["势力", "阵营"]);
    const relationText = parseField(block, ["与叶辰的关系", "关系"]);
    const arcText = parseField(block, ["人物弧线"]);
    const fallbackSummary = block.slice(0, 8).join(" ");

    const relationCandidates = block.join(" ");
    const relationships = names
      .filter((other) => other !== name && relationCandidates.includes(other))
      .slice(0, 6)
      .map((other) => {
        let relType = "relationship";
        if (relationCandidates.includes("并肩") || relationCandidates.includes("兄弟") || relationCandidates.includes("盟友")) relType = "ally";
        else if (relationCandidates.includes("对立") || relationCandidates.includes("敌") || relationCandidates.includes("宿命")) relType = "rival";
        else if (relationCandidates.includes("爱情") || relationCandidates.includes("心动") || relationCandidates.includes("情愫")) relType = "lover";
        return { target: other, rel_type: relType, description: relationText || relationCandidates.slice(0, 120) };
      });

    if (relationships.length === 0 && relationText) {
      relationships.push({ target: "叶辰", rel_type: "relationship", description: relationText });
    }

    return {
      id: `${name}-${idx + 1}`,
      name,
      role: roleFromLabel(roleLabel),
      age,
      appearance,
      personality: personality || fallbackSummary,
      backstory: background || fallbackSummary,
      motivations: motivationsText ? motivationsText.split(/[，,；;、]/).map((s) => s.trim()).filter(Boolean).slice(0, 4) : [],
      secrets: [],
      skills: [],
      arc: arcText ? { start_state: "待补充", end_state: "待补充", key_turning_points: [], internal_conflict: arcText } : null,
      relationships,
      faction,
    };
  });

  return { characters };
}
