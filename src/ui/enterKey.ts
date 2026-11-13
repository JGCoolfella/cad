/**
 * Enter in a PropertyManager value box. The first Enter applies the typed value and stays
 * in the box (the preview updates); the next Enter, with no typing in between, goes on to
 * the PropertyManager, where it means OK. So a feature never closes on a value the user
 * has not seen applied: from inside a box it takes two Enters, from the graphics area one.
 */
export interface EnterState {
  /** Enter was pressed and nothing has been typed since. */
  armed: boolean;
}

export interface EnterStep {
  /** Apply the box's value. */
  apply: boolean;
  /** Pass the key on to the PropertyManager (Enter = OK). */
  propagate: boolean;
  next: EnterState;
}

/** What one key press in a value box does. */
export function enterStep(state: EnterState, key: string): EnterStep {
  if (key !== "Enter") return { apply: false, propagate: false, next: state };
  if (state.armed) return { apply: true, propagate: true, next: state };
  return { apply: true, propagate: false, next: { armed: true } };
}

/** Typing, focusing or leaving the box starts over: the next Enter only applies. */
export const DISARMED: EnterState = { armed: false };
