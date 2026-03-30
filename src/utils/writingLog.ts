const STORAGE_KEY = "retl_writing_log";
const DAILY_GOAL_KEY = "retl_daily_goal";
const MONTHLY_GOAL_KEY = "retl_monthly_goal";

export interface DayLog {
  date: string; // "2026-03-29"
  words: number;
}

function today(): string {
  return new Date().toISOString().slice(0, 10);
}

function thisMonth(): string {
  return new Date().toISOString().slice(0, 7);
}

function loadLog(): DayLog[] {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) || "[]");
  } catch {
    return [];
  }
}

function saveLog(log: DayLog[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(log));
}

export function logWords(count: number) {
  const log = loadLog();
  const d = today();
  const existing = log.find((l) => l.date === d);
  if (existing) {
    existing.words += count;
  } else {
    log.push({ date: d, words: count });
  }
  saveLog(log);
}

export function getTodayWords(): number {
  return loadLog().find((l) => l.date === today())?.words ?? 0;
}

export function getMonthWords(): number {
  const m = thisMonth();
  return loadLog()
    .filter((l) => l.date.startsWith(m))
    .reduce((s, l) => s + l.words, 0);
}

export function getHistory(): DayLog[] {
  return loadLog();
}

export function getDailyGoal(): number {
  return Number(localStorage.getItem(DAILY_GOAL_KEY)) || 5000;
}

export function setDailyGoal(n: number) {
  localStorage.setItem(DAILY_GOAL_KEY, String(n));
}

export function getMonthlyGoal(): number {
  return Number(localStorage.getItem(MONTHLY_GOAL_KEY)) || 100000;
}

export function setMonthlyGoal(n: number) {
  localStorage.setItem(MONTHLY_GOAL_KEY, String(n));
}
