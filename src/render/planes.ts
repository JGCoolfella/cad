/**
 * Reference plane display, matching SOLIDWORKS:
 *  - very faint translucent fill (Document Properties > Plane Display: front/back
 *    face colours and transparency);
 *  - thin blue border; orange and thicker on hover; blue with 8 drag handles
 *    (corners + edge midpoints) when selected;
 *  - the plane's name drawn *in* the plane at its top-left corner, in a light blue
 *    Century Gothic-style font, so it foreshortens with the plane.
 */
import * as THREE from "three";
import { LineSegments2 } from "three/examples/jsm/lines/LineSegments2.js";
import { LineSegmentsGeometry } from "three/examples/jsm/lines/LineSegmentsGeometry.js";
import { LineMaterial } from "three/examples/jsm/lines/LineMaterial.js";
import type { Frame, Vec3 } from "../core/types";
import { frameToWorld } from "../core/math";

export type Extents = [u0: number, u1: number, v0: number, v1: number];

export interface PlaneStyle {
  front: string;
  back: string;
  /** 0..100 (percent transparent) */
  transparency: number;
  border: string;
  hover: string;
  selected: string;
  shaded: boolean;
}

export interface PlaneVisual {
  group: THREE.Group;
  pick: THREE.Mesh;
  /** World positions of the 8 handles (corners then mids) when selected. */
  handles: Vec3[];
}

const labelCache = new Map<string, { tex: THREE.CanvasTexture; aspect: number }>();

function labelTexture(text: string, color: string) {
  const key = `${text}|${color}`;
  const hit = labelCache.get(key);
  if (hit) return hit;
  const c = document.createElement("canvas");
  const ctx = c.getContext("2d")!;
  const font = "500 96px 'Century Gothic', 'Futura', 'Avenir', 'Segoe UI', sans-serif";
  ctx.font = font;
  const spacing = 6;
  const w = Math.ceil(ctx.measureText(text).width + spacing * text.length + 16);
  c.width = w;
  c.height = 128;
  ctx.font = font;
  ctx.fillStyle = color;
  ctx.textBaseline = "middle";
  let x = 8;
  for (const ch of text) {
    ctx.fillText(ch, x, 66);
    x += ctx.measureText(ch).width + spacing;
  }
  const tex = new THREE.CanvasTexture(c);
  tex.anisotropy = 8;
  tex.colorSpace = THREE.SRGBColorSpace;
  const v = { tex, aspect: c.width / c.height };
  labelCache.set(key, v);
  return v;
}

export function buildPlane(
  name: string,
  frame: Frame,
  ext: Extents,
  state: { hover: boolean; selected: boolean },
  style: PlaneStyle,
  resolution: THREE.Vector2,
): PlaneVisual {
  const [u0, u1, v0, v1] = ext;
  const W = (u: number, v: number) => new THREE.Vector3(...frameToWorld(frame, [u, v]));
  const corners = [W(u0, v0), W(u1, v0), W(u1, v1), W(u0, v1)];
  const group = new THREE.Group();

  // Fill: front and back faces get their own colours.
  const geo = new THREE.BufferGeometry().setFromPoints([corners[0], corners[1], corners[2], corners[0], corners[2], corners[3]]);
  geo.computeVertexNormals();
  const opacity = style.shaded ? Math.max(0.03, (100 - style.transparency) / 100) : 0;
  if (opacity > 0) {
    const front = new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ color: style.front, transparent: true, opacity, side: THREE.FrontSide, depthWrite: false }));
    const back = new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ color: style.back, transparent: true, opacity, side: THREE.BackSide, depthWrite: false }));
    front.renderOrder = back.renderOrder = 2;
    group.add(front, back);
  }

  // Border.
  const color = state.selected ? style.selected : state.hover ? style.hover : style.border;
  const width = state.hover ? 2.4 : state.selected ? 1.6 : 1.1;
  const pts: number[] = [];
  for (let i = 0; i < 4; i++) {
    const a = corners[i], b = corners[(i + 1) % 4];
    pts.push(a.x, a.y, a.z, b.x, b.y, b.z);
  }
  const lg = new LineSegmentsGeometry();
  lg.setPositions(pts);
  const lm = new LineMaterial({ color, linewidth: width, transparent: true, depthTest: true });
  lm.resolution.copy(resolution);
  const border = new LineSegments2(lg, lm);
  border.renderOrder = 3;
  group.add(border);

  // Name, lying in the plane at the top-left corner.
  const span = Math.min(u1 - u0, v1 - v0);
  const h = Math.max(1e-3, span * 0.075);
  const { tex, aspect } = labelTexture(name, "#3f86ec");
  const w = h * aspect;
  const pad = h * 0.35;
  const lx0 = u0 + pad, ly1 = v1 - pad * 0.6;
  const lq = [W(lx0, ly1 - h), W(lx0 + w, ly1 - h), W(lx0 + w, ly1), W(lx0, ly1)];
  const lgeo = new THREE.BufferGeometry().setFromPoints([lq[0], lq[1], lq[2], lq[0], lq[2], lq[3]]);
  lgeo.setAttribute("uv", new THREE.Float32BufferAttribute([0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1], 2));
  const label = new THREE.Mesh(lgeo, new THREE.MeshBasicMaterial({ map: tex, transparent: true, side: THREE.DoubleSide, depthWrite: false }));
  label.renderOrder = 4;
  group.add(label);

  const pick = new THREE.Mesh(geo.clone());
  const um = (u0 + u1) / 2, vm = (v0 + v1) / 2;
  const handles = state.selected
    ? ([[u0, v0], [u1, v0], [u1, v1], [u0, v1], [um, v0], [u1, vm], [um, v1], [u0, vm]] as [number, number][]).map((p) => frameToWorld(frame, p))
    : [];
  return { group, pick, handles };
}

/** Auto size: the model's footprint on the plane plus a margin (and never tiny). */
export function autoExtents(frame: Frame, box: THREE.Box3 | null, minHalf = 50): Extents {
  let u0 = -minHalf, u1 = minHalf, v0 = -minHalf, v1 = minHalf;
  if (box && !box.isEmpty()) {
    let a = Infinity, b = -Infinity, c = Infinity, d = -Infinity;
    for (let i = 0; i < 8; i++) {
      const p = new THREE.Vector3(i & 1 ? box.max.x : box.min.x, i & 2 ? box.max.y : box.min.y, i & 4 ? box.max.z : box.min.z);
      const rel = p.sub(new THREE.Vector3(...frame.origin));
      const u = rel.dot(new THREE.Vector3(...frame.x)), v = rel.dot(new THREE.Vector3(...frame.y));
      a = Math.min(a, u); b = Math.max(b, u); c = Math.min(c, v); d = Math.max(d, v);
    }
    // Planes through the origin always include it.
    a = Math.min(a, 0); b = Math.max(b, 0); c = Math.min(c, 0); d = Math.max(d, 0);
    const m = Math.max(b - a, d - c) * 0.12 + 2;
    u0 = a - m; u1 = b + m; v0 = c - m; v1 = d + m;
    const grow = (lo: number, hi: number): [number, number] => {
      const mid = (lo + hi) / 2, half = Math.max((hi - lo) / 2, minHalf * 0.6);
      return [mid - half, mid + half];
    };
    [u0, u1] = grow(u0, u1);
    [v0, v1] = grow(v0, v1);
  }
  return [u0, u1, v0, v1];
}
