/**
 * 3D View Selector (SOLIDWORKS: View Orientation > View Selector, or Ctrl+Space).
 *
 * A translucent bevelled cube drawn over the model with the current view rotation.
 * It has 26 clickable facets: 6 faces, 12 edges, 8 corners. Faces pointing away
 * from the viewer are repeated as floating panels outside the cube, so every
 * direction can be picked from any view. Hovering a facet tints it light blue;
 * clicking orients the view to look along that facet's normal.
 */
import * as THREE from "three";

export interface Facet {
  normal: THREE.Vector3;
  kind: "face" | "edge" | "corner";
  name: string | null;
  mesh: THREE.Mesh;
  outline: THREE.LineLoop;
  panel?: { mesh: THREE.Mesh; outline: THREE.LineLoop };
}

const FACE_NAMES: Record<string, string> = {
  "1,0,0": "Right",
  "-1,0,0": "Left",
  "0,1,0": "Top",
  "0,-1,0": "Bottom",
  "0,0,1": "Front",
  "0,0,-1": "Back",
};

const FILL = new THREE.Color("#c9cbd0");
const HOVER = new THREE.Color("#aebdfa");

export class ViewSelector {
  scene = new THREE.Scene();
  camera = new THREE.OrthographicCamera(-1, 1, 1, -1, -100, 100);
  facets: Facet[] = [];
  visible = false;
  hover: Facet | null = null;
  private raycaster = new THREE.Raycaster();

  constructor() {
    const planes: { n: THREE.Vector3; d: number; kind: Facet["kind"] }[] = [];
    const B = 0.34; // edge chamfer size along each axis
    const Cc = 0.78; // corner cut along the diagonal
    for (let x = -1; x <= 1; x++)
      for (let y = -1; y <= 1; y++)
        for (let z = -1; z <= 1; z++) {
          const k = Math.abs(x) + Math.abs(y) + Math.abs(z);
          if (k === 0) continue;
          const n = new THREE.Vector3(x, y, z).normalize();
          const d = k === 1 ? 1 : k === 2 ? (2 - B) / Math.SQRT2 : (3 - Cc) / Math.sqrt(3);
          planes.push({ n, d, kind: k === 1 ? "face" : k === 2 ? "edge" : "corner" });
        }
    for (const p of planes) {
      const poly = clipFacet(p, planes);
      if (poly.length < 3) continue;
      const name = p.kind === "face" ? FACE_NAMES[[Math.sign(p.n.x), Math.sign(p.n.y), Math.sign(p.n.z)].join(",").replace(/-0/g, "0")] ?? null : null;
      const { mesh, outline } = makeFacet(poly);
      this.scene.add(mesh, outline);
      const facet: Facet = { normal: p.n.clone(), kind: p.kind, name, mesh, outline };
      if (p.kind === "face") {
        // Floating panel version (shown when this face points away from the viewer).
        const shrink = poly.map((v) => v.clone().sub(p.n.clone().multiplyScalar(p.d)).multiplyScalar(0.92).add(p.n.clone().multiplyScalar(p.d + 0.95)));
        const panel = makeFacet(shrink);
        panel.mesh.visible = panel.outline.visible = false;
        this.scene.add(panel.mesh, panel.outline);
        facet.panel = panel;
      }
      this.facets.push(facet);
    }
  }

  /** Update orientation and panel visibility for the current view. */
  update(quat: THREE.Quaternion, w: number, h: number) {
    const s = Math.min(w, h) * 0.17; // px per cube unit
    this.camera.left = -w / 2 / s;
    this.camera.right = w / 2 / s;
    this.camera.top = h / 2 / s;
    this.camera.bottom = -h / 2 / s;
    this.camera.quaternion.copy(quat);
    this.camera.position.set(0, 0, 0).add(new THREE.Vector3(0, 0, 10).applyQuaternion(quat));
    this.camera.updateProjectionMatrix();
    this.camera.updateMatrixWorld(true);
    const viewDir = new THREE.Vector3(0, 0, 1).applyQuaternion(quat);
    for (const f of this.facets) {
      if (!f.panel) continue;
      const show = f.normal.dot(viewDir) < 0.15;
      f.panel.mesh.visible = f.panel.outline.visible = show;
    }
    for (const f of this.facets) this.paint(f, f === this.hover);
  }

  private paint(f: Facet, on: boolean) {
    for (const m of [f.mesh, f.panel?.mesh]) {
      if (!m) continue;
      const mat = m.material as THREE.MeshBasicMaterial;
      mat.color.copy(on ? HOVER : FILL);
      mat.opacity = on ? 0.75 : f.kind === "face" ? 0.32 : 0.42;
    }
    for (const o of [f.outline, f.panel?.outline]) {
      if (!o) continue;
      (o.material as THREE.LineBasicMaterial).color.set(on ? "#3048e0" : "#1b1b1b");
    }
  }

  render(renderer: THREE.WebGLRenderer) {
    if (!this.visible) return;
    const auto = renderer.autoClear;
    renderer.autoClear = false;
    renderer.clearDepth();
    renderer.render(this.scene, this.camera);
    renderer.autoClear = auto;
  }

  /** Facet under a screen point (px within the viewport), or null. */
  pick(sx: number, sy: number, w: number, h: number): Facet | null {
    if (!this.visible) return null;
    const ndc = new THREE.Vector2((sx / w) * 2 - 1, -(sy / h) * 2 + 1);
    this.raycaster.setFromCamera(ndc, this.camera);
    const objs: THREE.Object3D[] = [];
    for (const f of this.facets) {
      objs.push(f.mesh);
      if (f.panel?.mesh.visible) objs.push(f.panel.mesh);
    }
    const hit = this.raycaster.intersectObjects(objs, false)[0];
    if (!hit) return null;
    return this.facets.find((f) => f.mesh === hit.object || f.panel?.mesh === hit.object) ?? null;
  }

  setHover(f: Facet | null) {
    if (f === this.hover) return false;
    this.hover = f;
    return true;
  }

  /** Camera up vector to use when looking from a facet's direction. */
  static upFor(n: THREE.Vector3) {
    if (Math.abs(n.y) > 0.9 && Math.abs(n.x) < 0.2 && Math.abs(n.z) < 0.2) return new THREE.Vector3(0, 0, n.y > 0 ? -1 : 1);
    return new THREE.Vector3(0, 1, 0);
  }
}

function clipFacet(p: { n: THREE.Vector3; d: number }, all: { n: THREE.Vector3; d: number }[]): THREE.Vector3[] {
  // Big square in the plane, clipped by every other half-space n·x <= d.
  const o = p.n.clone().multiplyScalar(p.d);
  const a = Math.abs(p.n.x) < 0.9 ? new THREE.Vector3(1, 0, 0) : new THREE.Vector3(0, 1, 0);
  const u = a.clone().sub(p.n.clone().multiplyScalar(a.dot(p.n))).normalize();
  const v = p.n.clone().cross(u);
  let poly = [
    o.clone().addScaledVector(u, -4).addScaledVector(v, -4),
    o.clone().addScaledVector(u, 4).addScaledVector(v, -4),
    o.clone().addScaledVector(u, 4).addScaledVector(v, 4),
    o.clone().addScaledVector(u, -4).addScaledVector(v, 4),
  ];
  for (const q of all) {
    if (q === p) continue;
    const out: THREE.Vector3[] = [];
    for (let i = 0; i < poly.length; i++) {
      const A = poly[i], Bv = poly[(i + 1) % poly.length];
      const da = q.n.dot(A) - q.d, db = q.n.dot(Bv) - q.d;
      if (da <= 1e-9) out.push(A);
      if ((da < -1e-9 && db > 1e-9) || (da > 1e-9 && db < -1e-9)) out.push(A.clone().lerp(Bv, da / (da - db)));
    }
    poly = out;
    if (poly.length < 3) return [];
  }
  return poly;
}

function makeFacet(poly: THREE.Vector3[]) {
  const pos: number[] = [];
  for (let i = 1; i < poly.length - 1; i++) pos.push(...poly[0].toArray(), ...poly[i].toArray(), ...poly[i + 1].toArray());
  const geo = new THREE.BufferGeometry();
  geo.setAttribute("position", new THREE.Float32BufferAttribute(pos, 3));
  const mesh = new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ color: FILL, transparent: true, opacity: 0.35, side: THREE.DoubleSide, depthWrite: false }));
  const outline = new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(poly), new THREE.LineBasicMaterial({ color: "#1b1b1b", transparent: true, opacity: 0.85 }));
  return { mesh, outline };
}
