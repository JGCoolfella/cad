/**
 * The native desktop app (`crates/oc-app`, Tauri 2) hosts this UI in a transparent webview
 * over its wgpu viewport. These helpers detect it and call its commands through Tauri's
 * IPC (the internal entry point `@tauri-apps/api` wraps; no extra dependency).
 *
 * In the browser on the native engine (OVERHAUL_PLAN W1b) the same commands go to Web
 * Workers instead (`src/web/`, installed with [`useWebIpc`]): this is the one seam, and
 * everything that calls `invoke` runs unchanged.
 */
export interface TauriInternals {
  invoke<T>(cmd: string, args?: unknown, options?: { headers?: Record<string, string> }): Promise<T>;
}

/** The browser's engine and render workers, when the page runs the native engine. */
let web: TauriInternals | undefined;

/** Routes `invoke` to the browser's workers (`src/web/`) instead of Tauri's IPC. */
export function useWebIpc(t: TauriInternals | undefined) {
  web = t;
}

function internals(): TauriInternals | undefined {
  return (globalThis as unknown as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__ ?? web;
}

/** Running on the native engine: inside the native app (Tauri), or a browser page whose
 * commands go to the engine's Web Workers ([`useWebIpc`]); not the TypeScript engine. */
export function isNative(): boolean {
  return !!internals();
}

/**
 * Commands sent and not yet answered, for the stuck-state dump of the native check runs
 * (`DUMP` in `crates/oc-app/src/smoke.rs` reads `globalThis.__opencadIpc`).
 */
const ipc = { pending: new Map<number, { cmd: string; t: number }>(), next: 1, sent: 0, done: 0 };
(globalThis as unknown as { __opencadIpc: typeof ipc }).__opencadIpc = ipc;

export function invoke<T>(cmd: string, args?: unknown, options?: { headers?: Record<string, string> }): Promise<T> {
  const t = internals();
  if (!t) return Promise.reject(new Error(`not in the native app (${cmd})`));
  const id = ipc.next++;
  ipc.sent++;
  ipc.pending.set(id, { cmd, t: performance.now() });
  return t.invoke<T>(cmd, args, options).finally(() => {
    ipc.pending.delete(id);
    ipc.done++;
  });
}
