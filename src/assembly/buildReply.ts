/**
 * Assembly build replies (G3d): binary, and only what changed since the build the webview
 * holds. `crates/oc-app/src/asm.rs` (`encode_build`) writes them in the packed layout of
 * part rebuilds (`pack.rs`): a JSON header (components, parts, mates or their changes, DOF,
 * heals, timings) and typed arrays of placements and states. A reply names the generation
 * it changes (`base`); applied to that build, it gives the new one, sharing every entry
 * that did not change. A reply with `base` 0 is whole.
 */
import { unpack } from "../native/engine";
import type { BuildReply, InstanceState, Occurrence, Transform } from "./types";

/** `state_code` in `asm.rs`, in this order. */
const STATES: InstanceState[] = ["fixed", "under", "full", "over", "suppressed"];

/** A difference against a build other than the one given: ask again for a whole reply. */
export class StaleBase extends Error {
  constructor() {
    super("the assembly build reply changes another build");
  }
}

interface Packed {
  gen: number;
  base: number;
  instances: { ids?: string[]; at?: Uint32Array; tr: Float64Array; st: Uint32Array };
  occurrences: { paths?: string[][]; at?: Uint32Array; part: Uint32Array; w: Float64Array; hidden: Uint32Array };
  hashes: string[];
  mates?: BuildReply["mates"];
  mateChanges?: [number, BuildReply["mates"][number]][];
  [k: string]: unknown;
}

const placement = (a: Float64Array, k: number): Transform => ({
  position: [a[7 * k], a[7 * k + 1], a[7 * k + 2]],
  rotation: [a[7 * k + 3], a[7 * k + 4], a[7 * k + 5], a[7 * k + 6]],
});

/** The build a reply describes, given the build held (`base`; null: none). Throws
 * [`StaleBase`] when the reply is a difference against another build. */
export function decodeBuild(buf: ArrayBuffer, base: BuildReply | null): BuildReply {
  const p = unpack(buf) as Packed;
  const { instances: pi, occurrences: po, hashes, mates, mateChanges, ...rest } = p;
  const delta = !pi.ids || !po.paths || !mates;
  if (delta && (!base || p.base === 0 || base.gen !== p.base)) throw new StaleBase();
  const parts = rest.parts as BuildReply["parts"];
  const mesh = (hash: string) => parts[hash]?.handle ?? 0;

  let instances: BuildReply["instances"];
  if (pi.ids) {
    instances = pi.ids.map((id, k) => ({ id, transform: placement(pi.tr, k), state: STATES[pi.st[k] & 0xff], free: pi.st[k] >>> 8 }));
  } else {
    instances = base!.instances.slice();
    pi.at!.forEach((i, k) => {
      instances[i] = { id: instances[i].id, transform: placement(pi.tr, k), state: STATES[pi.st[k] & 0xff], free: pi.st[k] >>> 8 };
    });
  }

  let occurrences: Occurrence[];
  if (po.paths) {
    occurrences = po.paths.map((path, k) => {
      const part = hashes[po.part[k]];
      return { path, part, mesh: mesh(part), world: placement(po.w, k), hidden: po.hidden[k] !== 0 };
    });
  } else {
    // Every build hands out new mesh handles: each occurrence takes its part's.
    occurrences = base!.occurrences.map((o) => ({ ...o, mesh: mesh(o.part) }));
    po.at!.forEach((i, k) => {
      const part = hashes[po.part[k]];
      occurrences[i] = { path: occurrences[i].path, part, mesh: mesh(part), world: placement(po.w, k), hidden: po.hidden[k] !== 0 };
    });
  }

  let m: BuildReply["mates"];
  if (mates) m = mates;
  else {
    m = base!.mates.slice();
    for (const [i, x] of mateChanges ?? []) m[i] = x;
  }
  return { ...(rest as unknown as Omit<BuildReply, "instances" | "occurrences" | "mates">), gen: p.gen, instances, occurrences, mates: m };
}
