/**
 * SmartMates (G1d), as SOLIDWORKS': Alt-drag a face, edge or vertex of a component (or, in
 * the Move Component PropertyManager with SmartMates on, double-click it) and the component
 * follows the pointer, transparent, within the freedom its mates leave. Over a face, edge or
 * vertex of another component that the pair can mate, the component snaps to the proposed
 * mate (a trial solve that leaves the document alone) and the pointer shows its type;
 * moving off lets it follow the pointer again. Dropping there opens the Mate
 * PropertyManager with both entities picked and the Mate pop-up toolbar to confirm or
 * change the mate; dropping elsewhere is an ordinary drag (Move Component, one undo step).
 */
import type { SelItem } from "../app/app";
import type { Id, Vec3 } from "../core/types";
import { invoke } from "../native/tauri";
import type { AssemblySession } from "./session";
import { type BuildReply, type Mate, type MateEntity, type MateRef, type MateType, type Transform, MATE_LABEL } from "./types";

/** Placements of the drag, kept while a proposed mate is previewed. */
interface Placements {
  instances: { id: Id; transform: Transform }[];
  occurrences: Transform[];
}

const isEntity = (x: SelItem | null): x is SelItem & { body: number } =>
  !!x && (x.kind === "face" || x.kind === "edge" || x.kind === "vertex") && "body" in x && x.body !== undefined;

export class SmartMates {
  /** The component SmartMates moves (top-level instance). */
  readonly instance: Id;
  /** The entity dragged from, resolved to a mate reference (null: it cannot be mated). */
  private fromRef: Promise<MateRef | null>;
  /** The entity under the pointer on another component (the drop target), or null. */
  target: (SelItem & { body: number }) | null = null;
  private key = "";
  /** The mate proposed for the target and solved (null: none, or it cannot be solved). */
  proposal: { type: MateType; label: string; mate: Mate; to: MateRef } | null = null;
  /** The drag's placements while the proposal is shown. */
  private held: Placements | null = null;
  /** The newest pointer target while the proposal holds the component. */
  private pending: Vec3 | null = null;
  private seq = 0;
  /** What the trial solves answered (for scripts). */
  log: string[] = [];
  private probing: Promise<void> = Promise.resolve();
  /** Where the pointer is in the graphics area (for the pointer label). */
  at: [number, number] = [0, 0];

  constructor(
    readonly s: AssemblySession,
    readonly from: SelItem & { body: number },
    /** The component follows the pointer (an Alt-drag of a component free to move). */
    readonly moving: boolean,
  ) {
    this.instance = s.instanceOfOccurrence(from.body) ?? "";
    this.fromRef = this.resolve(from);
  }

  private async resolve(item: SelItem & { body: number }): Promise<MateRef | null> {
    const occ = this.s.occurrences()[item.body];
    if (!occ) return null;
    try {
      const entity = await invoke<MateEntity>("asm_entity", { part: occ.part, kind: item.kind, index: (item as { id: number }).id, quality: this.s.quality });
      return { occurrence: occ.path, entity };
    } catch {
      return null;
    }
  }

  /** The pointer moved to `target` (in the plane through the grabbed point facing the
   * viewer): the component follows, unless a proposed mate holds it. */
  move(target: Vec3) {
    if (!this.moving) return;
    if (this.proposal || this.held) this.pending = target;
    else this.s.dragTo(target);
  }

  /** What is under the pointer (the dragged component is not pickable while dragging). */
  hover(hit: SelItem | null) {
    const ok = isEntity(hit) && this.s.instanceOfOccurrence(hit.body) !== this.instance ? hit : null;
    const key = ok ? `${ok.body}:${ok.kind}:${(ok as { id: number }).id}` : "";
    if (key === this.key) return;
    this.key = key;
    const seq = ++this.seq;
    this.target = ok;
    this.probing = this.probing.then(() => this.propose(ok, seq));
  }

  /** Shows the proposed mate for `to` (null: back to following the pointer). */
  private async propose(to: (SelItem & { body: number }) | null, seq: number) {
    if (seq !== this.seq) return;
    if (this.held) {
      this.s.showPlacements(this.held);
      this.held = null;
    }
    this.proposal = null;
    this.s.app.changed();
    if (!to) {
      const p = this.pending;
      this.pending = null;
      if (p) this.s.dragTo(p);
      return;
    }
    const [a, b] = await Promise.all([this.fromRef, this.resolve(to)]);
    if (seq !== this.seq || !a || !b) return;
    await this.s.settled();
    if (seq !== this.seq || !this.s.built) return;
    const pa = this.s.pickOf(a);
    const pb = this.s.pickOf(b);
    const pm = this.s.propose([pa, pb]);
    if (!pm) return;
    const mate: Mate = { id: "$smart", name: MATE_LABEL[pm.type], type: pm.type, refs: [a, b] };
    if (pm.type !== "lock" && pm.type !== "perpendicular") mate.align = pm.align;
    if (pm.type === "distance" || pm.type === "angle") mate.value = pm.value;
    // Solved from where the drag has the components now; the last build stays the drag's.
    const doc = this.s.docWith(mate);
    const now = this.s.built.instances;
    doc.instances.forEach((i) => {
      const t = now.find((x) => x.id === i.id)?.transform;
      if (t) i.transform = t;
    });
    let r: BuildReply;
    try {
      r = await invoke<BuildReply>("asm_build", { doc, path: this.s.path, lightweight: this.s.lightweight, quality: this.s.quality, overrides: this.s.overrides, probe: true });
    } catch (e) {
      return void this.log.push(`probe failed: ${e}`);
    }
    this.log.push(`probe ${seq}/${this.seq} ${pm.type} ${r.mates.find((m) => m.id === mate.id)?.status}`);
    if (seq !== this.seq || r.mates.find((m) => m.id === mate.id)?.status !== "ok") return;
    this.held = this.s.snapshot();
    this.s.showPlacements({ instances: r.instances, occurrences: this.s.occurrences().map((o) => r.occurrences.find((x) => x.path.join("/") === o.path.join("/"))?.world ?? o.world) });
    this.proposal = { type: pm.type, label: MATE_LABEL[pm.type], mate, to: b };
    this.s.app.changed();
  }

  /** The pointer is released (or the target clicked): over a proposed mate the Mate
   * PropertyManager opens with both entities and the pop-up toolbar; elsewhere the drag
   * ends as Move Component does. */
  async end() {
    // The proposal for what is under the pointer now is let finish; later ones are dropped.
    await this.probing;
    this.seq++;
    const proposal = this.proposal;
    if (this.held) this.s.showPlacements(this.held);
    this.held = null;
    this.proposal = null;
    if (this.moving) await this.s.dragEnd();
    const a = proposal && (await this.fromRef);
    this.s.smart = null;
    if (proposal && a) {
      this.s.openMate();
      this.s.addPick(a);
      this.s.addPick(proposal.to);
    }
    this.s.app.changed();
  }

  /** Esc: the drag is undone and nothing is mated. */
  cancel() {
    this.seq++;
    this.held = null;
    this.proposal = null;
    this.s.smart = null;
    if (this.moving) this.s.dragAbort();
    this.s.app.changed();
  }
}
