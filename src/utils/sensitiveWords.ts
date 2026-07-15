// Common sensitive words for Chinese web novel platforms
// This is a simplified set; real platforms have much larger lists
const SENSITIVE_PATTERNS: string[] = [
  // Political
  "共产", "国民党", "文革", "天安门", "六四", "法轮",
  // Violence (extreme)
  "肢解", "分尸", "虐杀",
  // Profanity
  "他妈的", "操你", "狗日的",
  // Drugs
  "吸毒", "贩毒", "冰毒", "海洛因", "大麻",
  // Other restricted
  "自杀方法", "制造炸弹",
];

export interface SensitiveMatch {
  word: string;
  count: number;
  positions: number[];
}

export function checkSensitiveWords(text: string): SensitiveMatch[] {
  const results: SensitiveMatch[] = [];
  for (const word of SENSITIVE_PATTERNS) {
    const positions: number[] = [];
    let idx = 0;
    while (true) {
      const found = text.indexOf(word, idx);
      if (found === -1) break;
      positions.push(found);
      idx = found + word.length;
    }
    if (positions.length > 0) {
      results.push({ word, count: positions.length, positions });
    }
  }
  return results;
}
