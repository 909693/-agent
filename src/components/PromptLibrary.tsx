import { useState } from "react";
import { ClipboardList } from "lucide-react";
import { type Prompt, CATEGORIES, loadPrompts, savePrompts } from "../utils/promptStore";

export function PromptLibrary() {
  const [prompts, setPrompts] = useState<Prompt[]>(loadPrompts);
  const [filter, setFilter] = useState("全部");
  const [search, setSearch] = useState("");
  const [editing, setEditing] = useState<Prompt | null>(null);
  const [toast, setToast] = useState("");
  const [form, setForm] = useState({ title: "", category: "自定义", content: "" });
  const [showForm, setShowForm] = useState(false);

  const filtered = prompts.filter((p) => {
    const catOk = filter === "全部" || p.category === filter;
    const searchOk = !search || p.title.includes(search) || p.content.includes(search);
    return catOk && searchOk;
  });

  const handleCopy = (content: string) => {
    navigator.clipboard.writeText(content);
    setToast("已复制");
    setTimeout(() => setToast(""), 1500);
  };

  const handleSave = () => {
    if (!form.title.trim() || !form.content.trim()) return;
    let updated: Prompt[];
    if (editing) {
      updated = prompts.map((p) => (p.id === editing.id ? { ...p, ...form } : p));
    } else {
      updated = [...prompts, { id: Date.now().toString(), ...form }];
    }
    setPrompts(updated);
    savePrompts(updated);
    setShowForm(false);
    setEditing(null);
    setForm({ title: "", category: "自定义", content: "" });
  };

  const handleDelete = (id: string) => {
    const updated = prompts.filter((p) => p.id !== id);
    setPrompts(updated);
    savePrompts(updated);
  };

  const handleEdit = (p: Prompt) => {
    setEditing(p);
    setForm({ title: p.title, category: p.category, content: p.content });
    setShowForm(true);
  };
  return (
    <div>
      <div className="page-header">
        <h2>提示词库</h2>
        <button className="btn-primary" onClick={() => { setEditing(null); setForm({ title: "", category: "自定义", content: "" }); setShowForm(true); }}>
          + 新建提示词
        </button>
      </div>

      <div className="filter-bar">
        {["全部", ...CATEGORIES].map((c) => (
          <button key={c} className={`btn-sm ${filter === c ? "active" : ""}`}
            onClick={() => setFilter(c)}>{c}</button>
        ))}
        <input
          className="search-input"
          placeholder="搜索提示词..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      {toast && (
        <div style={{ position: "fixed", top: 80, right: 32, background: "var(--success)", color: "white", padding: "8px 20px", borderRadius: "var(--radius)", fontSize: 14, zIndex: 999 }}>
          {toast}
        </div>
      )}
      {showForm && (
        <div className="form-card">
          <h3>{editing ? "编辑提示词" : "新建提示词"}</h3>
          <div className="form-stack">
            <input placeholder="标题" value={form.title} onChange={(e) => setForm({ ...form, title: e.target.value })} />
            <select value={form.category} onChange={(e) => setForm({ ...form, category: e.target.value })}>
              {CATEGORIES.map((c) => <option key={c} value={c}>{c}</option>)}
            </select>
            <textarea placeholder="提示词内容" value={form.content} onChange={(e) => setForm({ ...form, content: e.target.value })} rows={4} />
            <div className="toolbar-actions">
              <button className="btn-primary" onClick={handleSave}>保存</button>
              <button className="btn-outline" onClick={() => { setShowForm(false); setEditing(null); }}>取消</button>
            </div>
          </div>
        </div>
      )}
      <div className="novel-grid">
        {filtered.map((p) => (
          <div key={p.id} className="novel-card" onClick={() => handleCopy(p.content)} style={{ cursor: "pointer" }}>
            <div className="novel-card-header">
              <h3 style={{ fontSize: 15 }}>{p.title}</h3>
              <span className="genre-tag">{p.category}</span>
            </div>
            <p className="premise" style={{ WebkitLineClamp: 3 }}>{p.content}</p>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button className="btn-sm" onClick={(e) => { e.stopPropagation(); handleEdit(p); }}>编辑</button>
              <button className="btn-sm" style={{ borderColor: "var(--danger)", color: "var(--danger)" }}
                onClick={(e) => { e.stopPropagation(); handleDelete(p.id); }}>删除</button>
            </div>
          </div>
        ))}
      </div>
      {filtered.length === 0 && (
        <div className="empty-state">
          <div className="empty-icon"><ClipboardList size={28} /></div>
          <p>没有匹配的提示词</p>
        </div>
      )}
    </div>
  );
}
