/**
 * The native desktop app (`crates/oc-app`, Tauri 2) hosts this UI in a transparent webview
 * over its wgpu viewport. These helpers detect it and call its commands through Tauri's
 * IPC (the internal entry point `@tauri-apps/api` wraps; no extra dependency).
 */
interface TauriInternals {
  invoke<T>(cmd: string, args?: unknown, options?: { headers?: Record<string, string> }): Promise<T>;
}

function internals(): TauriInternals | undefined {
  return (globalThis as unknown as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__;
}

/** Running inside the native app (Tauri), not a browser or Electron. */
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
