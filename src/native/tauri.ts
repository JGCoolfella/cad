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

export function invoke<T>(cmd: string, args?: unknown, options?: { headers?: Record<string, string> }): Promise<T> {
  const t = internals();
  if (!t) return Promise.reject(new Error(`not in the native app (${cmd})`));
  return t.invoke<T>(cmd, args, options);
}
