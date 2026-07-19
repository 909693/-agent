import { useState, useEffect } from "react";
import { BarChart3, Flame, PenLine, Trophy } from "lucide-react";
import {
  getTodayWords, getMonthWords, getHistory, getDailyGoal,
  setDailyGoal, getMonthlyGoal, setMonthlyGoal, type DayLog,
} from "../utils/writingLog";

export function WritingGoals() {
  const [dailyGoal, setDG] = useState(getDailyGoal());
  const [monthlyGoal, setMG] = useState(getMonthlyGoal());
  const [todayWords, setTodayWords] = useState(getTodayWords());
  const [monthWords, setMonthWords] = useState(getMonthWords());
  const [history, setHistory] = useState<DayLog[]>(getHistory());
  const [editingGoal, setEditingGoal] = useState(false);
  const [tempDaily, setTempDaily] = useState(dailyGoal);
  const [tempMonthly, setTempMonthly] = useState(monthlyGoal);

  useEffect(() => {
    const interval = setInterval(() => {
      setTodayWords(getTodayWords());
      setMonthWords(getMonthWords());
      setHistory(getHistory());
    }, 3000);
    return () => clearInterval(interval);
  }, []);

  const handleSaveGoals = () => {
    setDailyGoal(tempDaily);
    setMonthlyGoal(tempMonthly);
    setDG(tempDaily);
    setMG(tempMonthly);
    setEditingGoal(false);
  };

  // Last 30 days grid
  const last30: { date: string; words: number }[] = [];
  for (let i = 29; i >= 0; i--) {
    const d = new Date();
    d.setDate(d.getDate() - i);
    const ds = d.toISOString().slice(0, 10);
    const log = history.find((h) => h.date === ds);
    last30.push({ date: ds, words: log?.words ?? 0 });
  }
  // Streak calculation
  let streak = 0;
  for (let i = last30.length - 1; i >= 0; i--) {
    if (last30[i].words >= dailyGoal) streak++;
    else break;
  }

  const totalWords = history.reduce((s, h) => s + h.words, 0);
  const daysWithData = history.filter((h) => h.words > 0).length;
  const avgDaily = daysWithData > 0 ? Math.round(totalWords / daysWithData) : 0;
  const bestDay = history.reduce((best, h) => (h.words > best.words ? h : best), { date: "-", words: 0 });

  const todayPct = Math.min(100, Math.round((todayWords / dailyGoal) * 100));
  const monthPct = Math.min(100, Math.round((monthWords / monthlyGoal) * 100));

  return (
    <div>
      <div className="page-header">
        <h2>写作目标</h2>
        <button className="btn-primary" onClick={() => { setTempDaily(dailyGoal); setTempMonthly(monthlyGoal); setEditingGoal(true); }}>
          设置目标
        </button>
      </div>

      {editingGoal && (
        <div className="form-card">
          <h3>设置写作目标</h3>
          <div className="toolbar-actions" style={{ alignItems: "flex-end" }}>
            <label className="form-field">
              每日目标（字）
              <input type="number" value={tempDaily} onChange={(e) => setTempDaily(Number(e.target.value))} min={100} step={500} />
            </label>
            <label className="form-field">
              每月目标（字）
              <input type="number" value={tempMonthly} onChange={(e) => setTempMonthly(Number(e.target.value))} min={1000} step={10000} />
            </label>
            <button className="btn-primary" onClick={handleSaveGoals}>保存</button>
            <button className="btn-outline" onClick={() => setEditingGoal(false)}>取消</button>
          </div>
        </div>
      )}
      {/* Progress bars */}
      <div className="stats-row stats-row--two">
        <div className="stat-card">
          <div className="stat-label">今日进度</div>
          <div className="stat-value" style={{ fontSize: 22 }}>{todayWords} <span style={{ fontSize: 14, color: "var(--text-dim)" }}>/ {dailyGoal} 字</span></div>
          <div className="progress-track">
            <div className="progress-fill" style={{ background: todayPct >= 100 ? "var(--success)" : "var(--accent)", width: `${todayPct}%` }} />
          </div>
          <div style={{ fontSize: 12, color: "var(--text-dim)", marginTop: 4 }}>{todayPct}%</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">本月进度</div>
          <div className="stat-value" style={{ fontSize: 22 }}>{monthWords} <span style={{ fontSize: 14, color: "var(--text-dim)" }}>/ {monthlyGoal} 字</span></div>
          <div className="progress-track">
            <div className="progress-fill" style={{ background: monthPct >= 100 ? "var(--success)" : "var(--accent)", width: `${monthPct}%` }} />
          </div>
          <div style={{ fontSize: 12, color: "var(--text-dim)", marginTop: 4 }}>{monthPct}%</div>
        </div>
      </div>
      {/* Stats */}
      <div className="stats-row stats-row--four">
        <div className="stat-card">
          <div className="icon-chip"><Flame size={22} /></div>
          <div className="stat-value">{streak}</div>
          <div className="stat-label">连续天数</div>
        </div>
        <div className="stat-card">
          <div className="icon-chip"><PenLine size={22} /></div>
          <div className="stat-value">{totalWords.toLocaleString()}</div>
          <div className="stat-label">总字数</div>
        </div>
        <div className="stat-card">
          <div className="icon-chip"><BarChart3 size={22} /></div>
          <div className="stat-value">{avgDaily.toLocaleString()}</div>
          <div className="stat-label">日均字数</div>
        </div>
        <div className="stat-card">
          <div className="icon-chip"><Trophy size={22} /></div>
          <div className="stat-value">{bestDay.words.toLocaleString()}</div>
          <div className="stat-label">最佳单日 ({bestDay.date})</div>
        </div>
      </div>

      {/* 30-day grid */}
      <div className="content-section">
        <h3>最近 30 天</h3>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(10, 1fr)", gap: 6 }}>
          {last30.map((d) => {
            const pct = dailyGoal > 0 ? d.words / dailyGoal : 0;
            const bg = d.words === 0 ? "var(--border-light)" : pct >= 1 ? "var(--success)" : pct >= 0.5 ? "var(--accent)" : "#C7D2FE";
            return (
              <div key={d.date} title={`${d.date}: ${d.words} 字`}
                style={{ aspectRatio: "1", borderRadius: "var(--radius)", background: bg, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", fontSize: 10, color: d.words > 0 ? "white" : "var(--text-dim)" }}>
                <div>{d.date.slice(8)}</div>
                {d.words > 0 && <div style={{ fontWeight: 600 }}>{d.words >= 1000 ? `${(d.words / 1000).toFixed(1)}k` : d.words}</div>}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
