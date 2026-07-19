const STORAGE_KEY = "retl_writing_log";
const DAILY_GOAL_KEY = "retl_daily_goal";
const MONTHLY_GOAL_KEY = "retl_monthly_goal";

export interface DayLog {
  date: string; // "2026-03-29"
  words: number;
}

function today(): string {
  // Use LOCAL date, not UTC — otherwise UTC+8 users writing between 0:00-8:00
  // have their words counted toward the previous day (breaking stats & streaks).
  const d = new Date();
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function thisMonth(): string {
  return today().slice(0, 7);
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
  const n = Number(localStorage.getItem(DAILY_GOAL_KEY));
  return Number.isFinite(n) && n > 0 ? n : 5000;
}

export function setDailyGoal(n: number) {
  // Clamp to a positive integer: a 0/negative goal yields NaN%/always-met streaks.
  localStorage.setItem(DAILY_GOAL_KEY, String(Math.max(1, Math.floor(n) || 0)));
}

export function getMonthlyGoal(): number {
  const n = Number(localStorage.getItem(MONTHLY_GOAL_KEY));
  return Number.isFinite(n) && n > 0 ? n : 100000;
}

export function setMonthlyGoal(n: number) {
  localStorage.setItem(MONTHLY_GOAL_KEY, String(Math.max(1, Math.floor(n) || 0)));
}
