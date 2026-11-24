/**
 * Which OpenCAD this is: the version (`package.json`, the same in `crates/oc-app`'s
 * `tauri.conf.json` and `Cargo.toml`), the build (commit and date, set by `vite.config.ts`)
 * and the edition (the native Windows app, the browser version or the Electron app). Shown
 * in Help > About OpenCAD.
 */
import { isNative } from "../native/tauri";

declare const __APP_VERSION__: string;
declare const __APP_COMMIT__: string;
declare const __APP_BUILD_DATE__: string;

export const APP_VERSION: string = typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "dev";
/** Short commit hash of the build ("" outside a git checkout). */
export const APP_COMMIT: string = typeof __APP_COMMIT__ === "string" ? __APP_COMMIT__ : "";
/** Build date, YYYY-MM-DD. */
export const APP_BUILD_DATE: string = typeof __APP_BUILD_DATE__ === "string" ? __APP_BUILD_DATE__ : "";

export type Edition = "native" | "electron" | "browser";

export function edition(): Edition {
  if (isNative()) return "native";
  if (typeof navigator !== "undefined" && /\bElectron\//.test(navigator.userAgent)) return "electron";
  return "browser";
}

export function editionName(e: Edition = edition()): string {
  switch (e) {
    case "native":
      return "Desktop app (native engine)";
    case "electron":
      return "Desktop app (Electron)";
    case "browser":
      return "Browser version";
  }
}

/** "Version 0.9.0 (build 83182db, 2026-11-23)". */
export function versionText(): string {
  const build = [APP_COMMIT && `build ${APP_COMMIT}`, APP_BUILD_DATE].filter(Boolean).join(", ");
  return `Version ${APP_VERSION}${build ? ` (${build})` : ""}`;
}
