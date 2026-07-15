# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RETL is an AI-powered novel writing assistant built with Tauri (Rust backend) + React (TypeScript frontend). It helps authors create long-form fiction through AI-assisted world-building, character development, plot outlining, and chapter generation with advanced features like RAG context, foreshadowing tracking, and consistency checking.

## Development Commands

### Frontend (React + Vite)
```bash
npm run dev          # Start Vite dev server (port 1420)
npm run build        # Build frontend (outputs to dist/)
npm run preview      # Preview production build
```

### Backend (Tauri + Rust)
```bash
npm run tauri dev    # Start Tauri app in dev mode (runs npm run dev + Rust backend)
npm run tauri build  # Build production app bundle
```

### Full Development Workflow
```bash
npm run tauri dev    # This is the primary command - starts both frontend and backend
```

The Tauri config (`src-tauri/tauri.conf.json`) automatically runs `npm run dev` before starting the Rust backend.

## Architecture

### Data Flow: Three-Phase Novel Creation

1. **Framework Generation** (`ChatCreator.tsx` → `engine/mod.rs`)
   - User provides premise, genre, tone, themes
   - Backend generates: `world.json` → `characters.json` → `plot.json` → `timeline.json`
   - Each phase uses structured prompts from `engine/prompts.rs`
   - LLM calls go through `llm/client.rs` which supports OpenAI/Anthropic/Gemini formats

2. **Chapter Management** (`ChapterManager.tsx` → Tauri commands)
   - Displays plot outline from `plot.json` (acts → chapters structure)
   - Batch generation: iterates chapters, builds RAG context, generates + summarizes
   - Summaries stored in `chapter_summaries.json` for consistency tracking

3. **Chapter Writing** (`ChapterEditor.tsx` → `engine/mod.rs`)
   - Multiple modes: fill (from outline), expand (user draft), continue (from cursor), partial (selection rewrite)
   - RAG context injection: smart windowing (first 3 + last 10 chapters), character states, active foreshadowing
   - Real-time features: reader simulation, sensitivity check (local regex + AI), version snapshots

### Storage Architecture (`storage/mod.rs`)

Projects stored in `~/Library/Application Support/retl/projects/{project_id}/`:
```
{project_id}/
├── meta.json              # ProjectMeta (title, genre, premise, themes, etc.)
├── world.json             # WorldSetting (geography, rules, factions, history)
├── characters.json        # Character array (personality, arc, relationships)
├── plot.json              # Plot structure (acts → chapters with outlines)
├── timeline.json          # Event timeline
├── chapter_summaries.json # Per-chapter analysis (key_events, character_changes, foreshadowing)
├── chapters/
│   ├── 1.txt             # Raw chapter text
│   ├── 2.txt
│   └── ...
└── snapshots/
    └── {chapter_num}/
        └── {timestamp}.txt
```

Custom data directory can be set via Settings UI (`set_data_dir` command).

### LLM Integration (`llm/client.rs`)

Unified client supporting three API formats:
- **OpenAI**: `/v1/chat/completions` (default, also works with compatible APIs)
- **Anthropic**: `/v1/messages` (Claude API)
- **Gemini**: `/v1beta/models/{model}:generateContent`

All generation functions in `engine/mod.rs` use `client.generate_json()` which enforces JSON output and handles provider-specific formatting.

### RAG Context System (`build_rich_context_string` in `lib.rs`)

Smart context assembly for chapter generation:
1. **Smart windowing**: First 3 chapters + last 10 chapters (prevents context bloat)
2. **Character state tracking**: Extracts `character_changes` from summaries (location, injuries, emotions)
3. **Foreshadowing management**: Tracks planted vs resolved foreshadowing across chapters
4. **End state continuity**: Injects previous chapter's `end_state` for seamless transitions

This context is injected into all chapter generation prompts to maintain consistency.

### Plugin System (`plugins/`)

Two extensibility mechanisms:

1. **Skills** (`skills.rs`): Git repos containing prompt templates/rules
   - Installed to `~/Library/Application Support/retl/skills/{repo_name}/`
   - UI in `SkillsManager.tsx` for install/enable/disable
   - Applied via `CreativeConstraintsPanel` in chapter editor

2. **MCP Servers** (`mcp.rs`): Model Context Protocol servers for external tools
   - Managed lifecycle (start/stop/test) with process spawning
   - Config stored in `~/Library/Application Support/retl/mcp_servers.json`
   - UI in `McpManager.tsx`

### Tauri Command Pattern

All backend functions exposed via `#[tauri::command]` in `lib.rs`. Frontend calls through `api.ts` using `invoke()`:

```typescript
// Frontend (api.ts)
export const api = {
  createProject: (data) => invoke<ProjectMeta>("create_project", data),
  expandChapter: (projectId, chapterNum, ...) => invoke<string>("expand_chapter", {...}),
  // ... 50+ commands
};

// Backend (lib.rs)
#[tauri::command]
async fn expand_chapter(project_id: String, chapter_number: u32, ...) -> Result<String, String> {
  // Implementation
}
```

### Batch Generation Flow

`batch_generate_chapters` command (`lib.rs:600+`):
1. Emits progress events via `app.emit("batch_progress", {...})`
2. For each chapter: build context → generate → save → summarize
3. Cancellable via `BATCH_CANCEL` atomic flag
4. Frontend (`ChapterManager.tsx`) listens to events and updates progress UI

### Consistency Checking

`check_consistency` command analyzes all chapter summaries against world/character data:
- Detects character state contradictions (e.g., dead character reappears)
- Finds unresolved foreshadowing
- Identifies setting/rule violations
- Returns structured JSON with issues categorized by severity

## Key Files

- `src-tauri/src/lib.rs`: All Tauri commands (API surface)
- `src-tauri/src/engine/mod.rs`: Core AI generation functions
- `src-tauri/src/engine/prompts.rs`: Prompt templates for all generation tasks
- `src-tauri/src/llm/client.rs`: Multi-provider LLM client
- `src-tauri/src/storage/mod.rs`: File-based project storage
- `src/api.ts`: TypeScript API wrapper for Tauri commands
- `src/App.tsx`: Main routing and state management
- `src/components/ChapterEditor.tsx`: Primary writing interface (800+ lines)

## Important Patterns

### Error Handling
- Rust functions return `Result<T, String>` with Chinese error messages
- Frontend displays errors in UI, no console.error() spam
- LLM client has robust retry logic and timeout handling (300s)

### State Management
- No Redux/Zustand - React useState + props drilling
- LLM config persisted to localStorage
- Theme persisted to localStorage with `data-theme` attribute

### JSON Schema Enforcement
All AI generation uses structured prompts that specify exact JSON schema. The `generate_json()` method in `llm/client.rs` adds provider-specific JSON mode flags (OpenAI: `response_format`, Gemini: `response_mime_type`).

### Sensitive Content Detection
Two-layer approach:
1. Local regex (`utils/sensitiveWords.ts`): Fast, rule-based, highlights in UI
2. AI-powered (`sensitivity_check` in `engine/mod.rs`): Context-aware, platform-specific (番茄/起点/晋江)

## Notes

- No test suite currently exists
- No linting configured (no ESLint/Prettier/rustfmt in package.json)
- Tauri uses `danger_accept_invalid_certs(true)` for LLM client (supports self-signed certs)
- All AI prompts are in Chinese, targeting Chinese web novel market
- Version snapshots are manual (user-triggered), not auto-saved
