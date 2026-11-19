/**
 * Whether the view is moving, for the level of detail the native renderer draws (G2b).
 *
 * SOLIDWORKS' Options > Performance > Level of detail lowers the detail "during dynamic
 * view operations (zoom, pan, and rotate)" and draws full detail again when they end. A
 * drag (rotate, pan, zoom, roll with the middle button or a view tool) and a view
 * transition (standard views, the View Selector, Zoom to Fit) are moving from their first
 * camera change until they end; the frame after is at rest at once. A camera change with no
 * end event (a wheel step, an arrow key, Zoom to Area) keeps the view moving for
 * `SETTLE_MS`, and the caller draws one more frame when that has passed (`settleIn`).
 */

/** How long after a camera change with no end event the view still counts as moving. */
export const SETTLE_MS = 200;

/** The camera fields that place the view (not the graphics area's size). */
interface CameraLike {
  target: number[];
  quat: number[];
  height: number;
  perspective: boolean;
}

export class ViewMotion {
  private key: string | null = null;
  private changedAt = -Infinity;
  /** The previous frame was part of a drag or a transition. */
  private active = false;

  /**
   * Whether the frame about to be sent at `now` (ms) shows a moving view. `active`: a
   * navigation drag has moved the view and is not released, or a view transition runs.
   */
  update(now: number, camera: CameraLike, active: boolean): boolean {
    const key = JSON.stringify([camera.target, camera.quat, camera.height, camera.perspective]);
    const changed = this.key !== null && key !== this.key;
    this.key = key;
    if (active) {
      this.active = true;
      this.changedAt = -Infinity;
      return true;
    }
    // The last change of a drag or a transition ends it.
    if (changed && !this.active) this.changedAt = now;
    this.active = false;
    return now - this.changedAt < SETTLE_MS;
  }

  /** Milliseconds until the view settles after a change with no end event (null: none pending). */
  settleIn(now: number): number | null {
    const left = this.changedAt + SETTLE_MS - now;
    return left > 0 ? left : null;
  }
}

/** The Level of detail slider's right end (SOLIDWORKS' `swLevelOfDetail` is 0 to 20). */
export const LOD_MAX = 20;
/** The slider's default (🔍 SOLIDWORKS' factory position is not documented). */
export const LOD_DEFAULT = 10;

/** The stored `performance.lod` as a slider position: 0 (Off) to `LOD_MAX`. */
export function lodSlider(v: unknown): number {
  const n = typeof v === "number" ? v : NaN;
  if (!Number.isFinite(n)) return LOD_DEFAULT;
  return Math.min(LOD_MAX, Math.max(0, Math.round(n)));
}

/**
 * Whether Level of detail applies to what is shown: SOLIDWORKS names assemblies, multibody
 * parts and draft-quality drawing views. A part edited in an assembly is shown in the
 * assembly's window, so it applies there too. A single-body part is always drawn at full
 * detail.
 */
export function lodApplies(d: { assembly: boolean; context: boolean; solids: number | undefined }): boolean {
  return d.assembly || d.context || (d.solids ?? 1) > 1;
}
