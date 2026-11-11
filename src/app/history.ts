/**
 * Undo/redo history with a memory budget.
 *
 * Part snapshots store each feature as its own JSON string. A feature that did not
 * change since the previous snapshot reuses that snapshot's string, so a step costs
 * roughly the size of what changed, not the size of the whole part. The stack is
 * trimmed (oldest first) to a step count and a character budget.
 */
import type { Feature, PartDocument } from "../core/types";

export interface Snapshot {
  label: string;
  /** Document without its features. */
  rest: string;
  /** One JSON string per feature, in order. */
  features: string[];
  /** Characters this snapshot adds over the strings it shares with its neighbour. */
  cost: number;
}

export class History {
  private undoStack: Snapshot[] = [];
  private redoStack: Snapshot[] = [];
  private used = 0;

  /** Defaults: 200 steps and 32M characters (about 64 MB), plenty for parts. */
  constructor(private maxSteps = 200, private maxChars = 32 * 1024 * 1024) {}

  get canUndo() {
    return this.undoStack.length > 0;
  }
  get canRedo() {
    return this.redoStack.length > 0;
  }
  get undoLabel() {
    return this.undoStack.at(-1)?.label ?? "";
  }
  get redoLabel() {
    return this.redoStack.at(-1)?.label ?? "";
  }
  /** Approximate memory held, in characters. */
  get size() {
    return this.used;
  }
  get steps() {
    return this.undoStack.length + this.redoStack.length;
  }

  /** Record the document before a change. Clears the redo list. */
  push(doc: PartDocument, label: string) {
    this.dropAll(this.redoStack);
    this.redoStack = [];
    this.add(this.undoStack, this.snap(doc, label, this.undoStack.at(-1)));
    this.trim();
  }

  /** Undo: returns the document to restore, recording `current` for redo. */
  undo(current: PartDocument): PartDocument | null {
    const s = this.undoStack.pop();
    if (!s) return null;
    this.used -= s.cost;
    this.add(this.redoStack, this.snap(current, s.label, s));
    return restore(s);
  }

  redo(current: PartDocument): PartDocument | null {
    const s = this.redoStack.pop();
    if (!s) return null;
    this.used -= s.cost;
    this.add(this.undoStack, this.snap(current, s.label, s));
    return restore(s);
  }

  /** Forget the most recent undo point (an edit that turned out to change nothing). */
  dropLast() {
    const s = this.undoStack.pop();
    if (s) this.used -= s.cost;
  }

  clear() {
    this.undoStack = [];
    this.redoStack = [];
    this.used = 0;
  }

  private snap(doc: PartDocument, label: string, near?: Snapshot): Snapshot {
    const { features, ...rest } = doc;
    const restText = JSON.stringify(rest);
    let cost = near && near.rest === restText ? 0 : restText.length;
    const shared = new Map<string, string>();
    if (near) for (const t of near.features) shared.set(t, t);
    const texts = features.map((f: Feature) => {
      const t = JSON.stringify(f);
      const old = shared.get(t);
      if (old !== undefined) return old; // same characters: keep one copy in memory
      cost += t.length;
      return t;
    });
    return { label, rest: near && near.rest === restText ? near.rest : restText, features: texts, cost: cost + 64 };
  }

  private add(stack: Snapshot[], s: Snapshot) {
    stack.push(s);
    this.used += s.cost;
  }

  private dropAll(stack: Snapshot[]) {
    for (const s of stack) this.used -= s.cost;
  }

  private trim() {
    while (this.undoStack.length > 1 && (this.undoStack.length > this.maxSteps || this.used > this.maxChars)) {
      const s = this.undoStack.shift()!;
      this.used -= s.cost;
    }
  }
}

function restore(s: Snapshot): PartDocument {
  return { ...JSON.parse(s.rest), features: s.features.map((t) => JSON.parse(t)) } as PartDocument;
}

/**
 * Sketch-level undo (inside an open sketch): whole-sketch JSON snapshots with the same
 * step and size limits.
 */
export class TextHistory {
  private undoStack: string[] = [];
  private redoStack: string[] = [];
  private used = 0;

  constructor(private maxSteps = 300, private maxChars = 16 * 1024 * 1024) {}

  get canUndo() {
    return this.undoStack.length > 0;
  }
  get canRedo() {
    return this.redoStack.length > 0;
  }

  push(text: string) {
    for (const t of this.redoStack) this.used -= t.length;
    this.redoStack = [];
    // Consecutive identical states are one step.
    if (this.undoStack.at(-1) === text) return;
    this.undoStack.push(text);
    this.used += text.length;
    while (this.undoStack.length > 1 && (this.undoStack.length > this.maxSteps || this.used > this.maxChars)) this.used -= this.undoStack.shift()!.length;
  }

  undo(current: string): string | null {
    const t = this.undoStack.pop();
    if (t === undefined) return null;
    this.used += current.length - t.length;
    this.redoStack.push(current);
    return t;
  }

  redo(current: string): string | null {
    const t = this.redoStack.pop();
    if (t === undefined) return null;
    this.used += current.length - t.length;
    this.undoStack.push(current);
    return t;
  }
}
