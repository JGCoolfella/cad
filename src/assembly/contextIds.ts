/**
 * Picks on the other components while a part is edited in an assembly (G1d, `context.ts`):
 * their bodies are drawn with ids from `CTX_BODY` (the occurrence index added), and their
 * faces, edges and vertices are selection items with that `body`. Kept apart from
 * `context.ts` so the viewport needs nothing else of assemblies.
 */
import type { SelItem } from "../app/app";

export const CTX_BODY = 2_000_000;

/** A pick on another component while editing in context. */
export const isContextItem = (s: SelItem | null): s is SelItem & { kind: "face" | "edge" | "vertex"; id: number; body: number } =>
  !!s && (s.kind === "face" || s.kind === "edge" || s.kind === "vertex") && s.body !== undefined && s.body >= CTX_BODY;
