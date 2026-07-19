import { listen } from "@tauri-apps/api/event";
import { api, type BatchProgress, type BatchComplete, type LlmParams, type CreativeConstraintsPayload } from "../api";

export const BATCH_CHUNK_SIZE = 100;
// Stall watchdog: give up on a chunk only if NO progress event arrives for this
// long (each chapter emits a progress event), instead of a fixed budget that a
// long chunk would always blow past.
const BATCH_HEARTBEAT_MS = 10 * 60 * 1000;

export interface BatchState {
  running: boolean;
  projectId: string | null;
  chunkInfo: { current: number; total: number } | null;
  progress: BatchProgress | null;
  statuses: Record<number, string>;
  result: BatchComplete | null;
  error: string;
}

export interface StartBatchParams {
  projectId: string;
  from: number;
  to: number;
  targetWords: number;
  skipWritten: boolean;
  llm: LlmParams;
  payload: CreativeConstraintsPayload;
}

// Module-level state so a running batch survives ChapterManager unmount (the user
// navigating to the editor and back). Components subscribe via useSyncExternalStore.
let state: BatchState = {
  running: false, projectId: null, chunkInfo: null,
  progress: null, statuses: {}, result: null, error: "",
};

const listeners = new Set<() => void>();
let chunkResolver: ((c: BatchComplete) => void) | null = null;
let watchdog: ReturnType<typeof setTimeout> | null = null;
let rearmWatchdog: (() => void) | null = null;
let cancelled = false;
let installed = false;

function emit() {
  // New object identity each change so useSyncExternalStore detects updates.
  state = { ...state };
  listeners.forEach((l) => l());
}

async function ensureListeners() {
  if (installed) return;
  await listen<BatchProgress>("batch_progress", (e) => {
    state.progress = e.payload;
    state.statuses = { ...state.statuses, [e.payload.chapter_number]: e.payload.phase };
    emit();
    // Progress = liveness; reset the stall watchdog.
    rearmWatchdog?.();
  });
  await listen<BatchComplete>("batch_complete", (e) => {
    const r = chunkResolver;
    chunkResolver = null;
    if (r) r(e.payload);
  });
  // Set only after both listeners install, so a failed listen() doesn't lock out
  // future installs (startBatch is single-flight via the `running` guard).
  installed = true;
}

export function getBatchState(): BatchState {
  return state;
}

export function subscribeBatch(l: () => void): () => void {
  listeners.add(l);
  return () => { listeners.delete(l); };
}

export function cancelBatch(): void {
  cancelled = true;
  api.cancelBatchGeneration().catch(() => {});
}

function clearWatch() {
  if (watchdog) { clearTimeout(watchdog); watchdog = null; }
  rearmWatchdog = null;
}

export async function startBatch(params: StartBatchParams): Promise<void> {
  if (state.running) return;
  cancelled = false;
  const aggregate: BatchComplete = {
    completed: 0, failed: 0, skipped: 0, total_words: 0, elapsed_seconds: 0, failed_chapters: [],
  };
  state = {
    running: true, projectId: params.projectId, chunkInfo: null,
    progress: null, statuses: {}, result: null, error: "",
  };
  emit();

  const chunks: Array<{ from: number; to: number }> = [];
  for (let from = params.from; from <= params.to; from += BATCH_CHUNK_SIZE) {
    chunks.push({ from, to: Math.min(from + BATCH_CHUNK_SIZE - 1, params.to) });
  }

  try {
    // Install listeners inside the try so a failure still reaches `finally` and
    // resets `running` (otherwise the batch feature would lock up until refresh).
    await ensureListeners();
    state.chunkInfo = { current: 0, total: chunks.length };
    emit();

    for (let i = 0; i < chunks.length; i++) {
      if (cancelled) break;
      const { from, to } = chunks[i];
      state.chunkInfo = { current: i + 1, total: chunks.length };
      emit();

      const done = new Promise<BatchComplete>((resolve, reject) => {
        const arm = () => {
          if (watchdog) clearTimeout(watchdog);
          watchdog = setTimeout(() => {
            clearWatch();
            reject(new Error("批量生成超时：10 分钟内无进度，已停止"));
          }, BATCH_HEARTBEAT_MS);
        };
        rearmWatchdog = arm;
        chunkResolver = (c) => { clearWatch(); resolve(c); };
        arm();
      });

      try {
        await api.batchGenerateChapters(
          params.projectId, from, to, params.targetWords, params.skipWritten, params.llm, params.payload,
        );
      } catch (e) {
        chunkResolver = null;
        clearWatch();
        state.error = `第 ${i + 1}/${chunks.length} 批启动失败：${e}`;
        break;
      }

      let chunkResult: BatchComplete;
      try {
        chunkResult = await done;
      } catch (e) {
        chunkResolver = null;
        clearWatch();
        // Stop the backend task so it doesn't keep running as a zombie.
        cancelled = true;
        try { await api.cancelBatchGeneration(); } catch { /* ignore */ }
        state.error = (e instanceof Error ? e.message : String(e)) || "批量生成超时，已请求停止后台任务";
        break;
      }

      aggregate.completed += chunkResult.completed;
      aggregate.failed += chunkResult.failed;
      aggregate.skipped += chunkResult.skipped;
      aggregate.total_words += chunkResult.total_words;
      aggregate.elapsed_seconds += chunkResult.elapsed_seconds;
      aggregate.failed_chapters = aggregate.failed_chapters.concat(chunkResult.failed_chapters);

      // If this chunk was cancelled mid-way, stop scheduling more chunks.
      if (state.progress?.phase === "cancelled" || cancelled) break;
    }
  } catch (e) {
    // e.g. ensureListeners failed — surface it instead of leaving a silent hang.
    state.error = e instanceof Error ? e.message : String(e);
  } finally {
    clearWatch();
    chunkResolver = null;
    state.running = false;
    state.chunkInfo = null;
    state.result = { ...aggregate };
    emit();
  }
}
