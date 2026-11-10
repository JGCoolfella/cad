/**
 * View camera with SOLIDWORKS-style manipulation: free (trackball) rotation about a
 * pivot, pan, zoom about the pointer, roll, standard orientations and animated
 * transitions. Orthographic by default (SOLIDWORKS default), perspective optional.
 */
import * as THREE from "three";
import type { Frame } from "../core/types";

export const STANDARD_VIEWS: Record<string, { dir: [number, number, number]; up: [number, number, number] }> = {
  front: { dir: [0, 0, 1], up: [0, 1, 0] },
  back: { dir: [0, 0, -1], up: [0, 1, 0] },
  left: { dir: [-1, 0, 0], up: [0, 1, 0] },
  right: { dir: [1, 0, 0], up: [0, 1, 0] },
  top: { dir: [0, 1, 0], up: [0, 0, -1] },
  bottom: { dir: [0, -1, 0], up: [0, 0, 1] },
  isometric: { dir: [1, 1, 1], up: [0, 1, 0] },
  trimetric: { dir: [0.70710678, 0.5, 0.70710678].map((x) => x) as [number, number, number], up: [0, 1, 0] },
  dimetric: { dir: [1, 0.8165, 1], up: [0, 1, 0] },
};

const FAR = 1e5;

export class ViewCamera {
  ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, -FAR, FAR);
  persp = new THREE.PerspectiveCamera(30, 1, 0.1, FAR * 2);
  /** Point the camera looks at. */
  target = new THREE.Vector3();
  /** Camera orientation; camera looks along -Z of this rotation. */
  quat = new THREE.Quaternion();
  /** Visible world height (ortho). */
  height = 200;
  width = 1;
  heightPx = 1;
  perspective = false;
  private anim: { t0: number; dur: number; q0: THREE.Quaternion; q1: THREE.Quaternion; tg0: THREE.Vector3; tg1: THREE.Vector3; h0: number; h1: number } | null = null;
  private undoStack: { q: THREE.Quaternion; t: THREE.Vector3; h: number }[] = [];

  constructor() {
    this.setView("isometric", false);
  }

  get camera(): THREE.Camera {
    return this.perspective ? this.persp : this.ortho;
  }

  resize(w: number, h: number) {
    this.width = w;
    this.heightPx = h;
    this.update();
  }

  /** px → world units at the target depth. */
  get worldPerPixel() {
    return this.height / Math.max(1, this.heightPx);
  }

  update() {
    const aspect = this.width / Math.max(1, this.heightPx);
    const h = this.height / 2;
    const dir = new THREE.Vector3(0, 0, 1).applyQuaternion(this.quat);
    const o = this.ortho;
    o.left = -h * aspect;
    o.right = h * aspect;
    o.top = h;
    o.bottom = -h;
    o.position.copy(this.target).addScaledVector(dir, FAR / 2);
    o.quaternion.copy(this.quat);
    o.near = 0;
    o.far = FAR;
    o.updateProjectionMatrix();
    o.updateMatrixWorld(true);
    const p = this.persp;
    p.aspect = aspect;
    const dist = h / Math.tan(THREE.MathUtils.degToRad(p.fov / 2));
    p.position.copy(this.target).addScaledVector(dir, dist);
    p.quaternion.copy(this.quat);
    p.near = Math.max(0.01, dist / 1000);
    p.far = dist * 1000;
    p.updateProjectionMatrix();
    p.updateMatrixWorld(true);
  }

  pushUndo() {
    this.undoStack.push({ q: this.quat.clone(), t: this.target.clone(), h: this.height });
    if (this.undoStack.length > 50) this.undoStack.shift();
  }

  /** Previous View (Ctrl+Shift+Z). */
  previousView(duration: number) {
    const s = this.undoStack.pop();
    if (!s) return;
    this.animateTo(s.q, s.t, s.h, duration, false);
  }

  // ------------------------------------------------------------ manipulation

  /** Free rotation: dx/dy in pixels, about a world pivot. */
  rotate(dx: number, dy: number, pivot: THREE.Vector3, degPerPx = 0.4) {
    const right = new THREE.Vector3(1, 0, 0).applyQuaternion(this.quat);
    const up = new THREE.Vector3(0, 1, 0).applyQuaternion(this.quat);
    const qa = new THREE.Quaternion().setFromAxisAngle(up, THREE.MathUtils.degToRad(-dx * degPerPx));
    const qb = new THREE.Quaternion().setFromAxisAngle(right, THREE.MathUtils.degToRad(-dy * degPerPx));
    const q = qa.multiply(qb);
    this.applyRotation(q, pivot);
  }

  /** Rotation about an arbitrary world axis through `pivot`. */
  rotateAxis(axis: THREE.Vector3, angle: number, pivot: THREE.Vector3) {
    const q = new THREE.Quaternion().setFromAxisAngle(axis.clone().normalize(), angle);
    this.applyRotation(q, pivot);
  }

  /** Roll about the screen normal (Alt + arrows / Alt + middle drag). */
  roll(angle: number) {
    const fwd = new THREE.Vector3(0, 0, 1).applyQuaternion(this.quat);
    const q = new THREE.Quaternion().setFromAxisAngle(fwd, angle);
    this.quat.premultiply(q).normalize();
    this.update();
  }

  private applyRotation(q: THREE.Quaternion, pivot: THREE.Vector3) {
    this.quat.premultiply(q).normalize();
    this.target.sub(pivot).applyQuaternion(q).add(pivot);
    this.update();
  }

  pan(dxPx: number, dyPx: number) {
    const wpp = this.worldPerPixel;
    const right = new THREE.Vector3(1, 0, 0).applyQuaternion(this.quat);
    const up = new THREE.Vector3(0, 1, 0).applyQuaternion(this.quat);
    this.target.addScaledVector(right, -dxPx * wpp).addScaledVector(up, dyPx * wpp);
    this.update();
  }

  /** Zoom by factor (>1 = zoom in) keeping the world point under (sx, sy) fixed. */
  zoomAt(factor: number, sx: number, sy: number) {
    const before = this.screenToPlanePoint(sx, sy);
    this.height = THREE.MathUtils.clamp(this.height / factor, 1e-4, 1e7);
    this.update();
    const after = this.screenToPlanePoint(sx, sy);
    this.target.add(before.sub(after));
    this.update();
  }

  /** World point on the plane through target, perpendicular to view, under a screen pixel. */
  screenToPlanePoint(sx: number, sy: number) {
    const ndc = new THREE.Vector2((sx / this.width) * 2 - 1, -(sy / this.heightPx) * 2 + 1);
    const right = new THREE.Vector3(1, 0, 0).applyQuaternion(this.quat);
    const up = new THREE.Vector3(0, 1, 0).applyQuaternion(this.quat);
    const aspect = this.width / Math.max(1, this.heightPx);
    return this.target
      .clone()
      .addScaledVector(right, (ndc.x * this.height * aspect) / 2)
      .addScaledVector(up, (ndc.y * this.height) / 2);
  }

  ray(sx: number, sy: number) {
    const ndc = new THREE.Vector2((sx / this.width) * 2 - 1, -(sy / this.heightPx) * 2 + 1);
    const rc = new THREE.Raycaster();
    rc.setFromCamera(ndc, this.camera);
    return rc.ray;
  }

  project(p: THREE.Vector3): [number, number] {
    const v = p.clone().project(this.camera);
    return [((v.x + 1) / 2) * this.width, ((1 - v.y) / 2) * this.heightPx];
  }

  // ------------------------------------------------------------ orientations

  orientationFor(dir: [number, number, number], up: [number, number, number]) {
    const z = new THREE.Vector3(...dir).normalize();
    let u = new THREE.Vector3(...up);
    if (Math.abs(u.dot(z)) > 0.999) u = new THREE.Vector3(0, 0, -1);
    const x = u.clone().cross(z).normalize();
    const y = z.clone().cross(x).normalize();
    const m = new THREE.Matrix4().makeBasis(x, y, z);
    return new THREE.Quaternion().setFromRotationMatrix(m);
  }

  setView(name: string, animate = true, duration = 0.35, fitBox?: THREE.Box3) {
    const v = STANDARD_VIEWS[name];
    if (!v) return;
    const q = this.orientationFor(v.dir, v.up);
    const fit = fitBox ? this.fitParams(fitBox, q) : null;
    this.animateTo(q, fit?.target ?? this.target.clone(), fit?.height ?? this.height, animate ? duration : 0);
  }

  /** Normal To: look straight at a plane (sketch frame), keeping its X right and Y up. */
  normalTo(frame: Frame, duration: number, fitBox?: THREE.Box3) {
    const x = new THREE.Vector3(...frame.x);
    const y = new THREE.Vector3(...frame.y);
    const z = new THREE.Vector3(...frame.normal);
    // Pressing Normal To again while already normal flips to the back side.
    const cur = new THREE.Vector3(0, 0, 1).applyQuaternion(this.quat);
    let q = new THREE.Quaternion().setFromRotationMatrix(new THREE.Matrix4().makeBasis(x, y, z));
    if (cur.dot(z) > 0.9999 && !this.anim) {
      q = new THREE.Quaternion().setFromRotationMatrix(new THREE.Matrix4().makeBasis(x.clone().negate(), y, z.clone().negate()));
    }
    const fit = fitBox ? this.fitParams(fitBox, q) : null;
    this.animateTo(q, fit?.target ?? this.target.clone(), fit?.height ?? this.height, duration);
  }

  fitParams(box: THREE.Box3, q: THREE.Quaternion) {
    if (box.isEmpty()) return { target: new THREE.Vector3(), height: 200 };
    const inv = q.clone().invert();
    const corners: THREE.Vector3[] = [];
    for (let i = 0; i < 8; i++)
      corners.push(new THREE.Vector3(i & 1 ? box.max.x : box.min.x, i & 2 ? box.max.y : box.min.y, i & 4 ? box.max.z : box.min.z).applyQuaternion(inv));
    const vb = new THREE.Box3().setFromPoints(corners);
    const aspect = this.width / Math.max(1, this.heightPx);
    const h = Math.max(vb.max.y - vb.min.y, (vb.max.x - vb.min.x) / aspect) * 1.12 || 100;
    const c = vb.getCenter(new THREE.Vector3()).applyQuaternion(q);
    return { target: c, height: h };
  }

  zoomToFit(box: THREE.Box3, duration: number) {
    const f = this.fitParams(box, this.quat);
    this.animateTo(this.quat.clone(), f.target, f.height, duration);
  }

  animateTo(q: THREE.Quaternion, target: THREE.Vector3, height: number, duration: number, record = true) {
    if (record) this.pushUndo();
    if (duration <= 0) {
      this.quat.copy(q);
      this.target.copy(target);
      this.height = height;
      this.anim = null;
      this.update();
      return;
    }
    this.anim = { t0: performance.now(), dur: duration * 1000, q0: this.quat.clone(), q1: q, tg0: this.target.clone(), tg1: target, h0: this.height, h1: height };
  }

  /** Advance animation. Returns true while animating. */
  tick(now: number): boolean {
    const a = this.anim;
    if (!a) return false;
    let t = Math.min(1, (now - a.t0) / a.dur);
    const e = t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
    this.quat.slerpQuaternions(a.q0, a.q1, e);
    this.target.lerpVectors(a.tg0, a.tg1, e);
    this.height = a.h0 + (a.h1 - a.h0) * e;
    this.update();
    if (t >= 1) {
      this.anim = null;
      t = 1;
    }
    return true;
  }

  get animating() {
    return !!this.anim;
  }

  viewDir() {
    return new THREE.Vector3(0, 0, 1).applyQuaternion(this.quat);
  }
}
