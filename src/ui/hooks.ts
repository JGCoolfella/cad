import { useSyncExternalStore } from "react";
import type { App } from "../app/app";

/** Re-render when the app changes. */
export function useApp(app: App) {
  useSyncExternalStore(app.subscribe, app.getVersion);
  return app;
}
