/**
 * Global keyboard handling with SOLIDWORKS default shortcuts.
 * Shortcuts are customisable later via the same table.
 */
import { CMD, runCommand, type Ctx } from "./commands";

/** Map of normalised key chord → command id. */
export const DEFAULT_KEYMAP: Record<string, string> = {};
for (const c of Object.values(CMD)) {
  if (c.shortcut) DEFAULT_KEYMAP[normalise(c.shortcut)] = c.id;
}

function normalise(chord: string) {
  const parts = chord.split("+").map((p) => p.trim().toLowerCase());
  const key = parts.pop()!;
  const mods = ["ctrl", "alt", "shift"].filter((m) => parts.includes(m));
  return [...mods, key === "space" ? " " : key].join("+");
}

export function chordOf(e: KeyboardEvent) {
  const mods: string[] = [];
  if (e.ctrlKey || e.metaKey) mods.push("ctrl");
  if (e.altKey) mods.push("alt");
  if (e.shiftKey) mods.push("shift");
  let k = e.key.toLowerCase();
  if (k === "spacebar") k = " ";
  if (/^digit\d$/i.test(e.code)) k = e.code.slice(5);
  return [...mods, k].join("+");
}

export function handleKey(e: KeyboardEvent, ctx: Ctx): boolean {
  const t = e.target as HTMLElement | null;
  if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable)) return false;
  const { app, vp } = ctx;
  // Sketch editor gets first refusal (Esc, A, Delete...).
  if (app.sketch && app.editor && !e.ctrlKey && !e.altKey) {
    if (app.editor.onKey(e.key)) return true;
  }
  if (e.key === "Escape") {
    if (vp?.viewMode) {
      vp.setViewMode(null);
      return true;
    }
    if (app.modify) {
      app.modify = null;
      app.changed();
      return true;
    }
    if (app.pm) {
      app.cancelPM();
      return true;
    }
    if (app.pendingSketch) {
      app.pendingSketch = false;
      app.message = null;
      app.changed();
      return true;
    }
    app.clearSelection();
    return true;
  }
  // View manipulation with arrows.
  const inc = app.options.view.arrowKeyIncrement;
  const arrows: Record<string, [number, number]> = { arrowleft: [-1, 0], arrowright: [1, 0], arrowup: [0, -1], arrowdown: [0, 1] };
  const a = arrows[e.key.toLowerCase()];
  if (a && vp) {
    if (e.ctrlKey) vp.panStep(-a[0] * 40, -a[1] * 40);
    else if (e.altKey) vp.rollStep(a[0] * inc);
    else if (e.shiftKey) vp.rotateStep(a[0] * 90, a[1] * 90);
    else vp.rotateStep(a[0] * inc, a[1] * inc);
    return true;
  }
  // Enter with a PropertyManager open = OK (green check), as in SOLIDWORKS.
  if (e.key === "Enter" && !app.modify && app.pmHasOk()) {
    app.acceptPM();
    return true;
  }
  if (e.key === "Enter" && app.lastCommand && !app.pm && !app.modify) {
    runCommand(app.lastCommand, ctx);
    return true;
  }
  const id = DEFAULT_KEYMAP[chordOf(e)];
  if (id) {
    runCommand(id, ctx);
    return true;
  }
  return false;
}
