/**
 * The graphics area: three.js rendering, GPU picking, SOLIDWORKS mouse navigation,
 * selection, and the 2D overlay (active sketch, triad, selection box).
 *
 * Rendering is on demand: a frame is drawn only when something changed, so an idle
 * window costs nothing (unlike always-on render loops).
 */
import * as THREE from "three";
import { LineSegments2 } from "three/examples/jsm/lines/LineSegments2.js";
import { LineSegmentsGeometry } from "three/examples/jsm/lines/LineSegmentsGeometry.js";
import { LineMaterial } from "three/examples/jsm/lines/LineMaterial.js";
import type { App, SelItem } from "../app/app";
import type { BodyMesh, Frame, SketchFeature, Vec2, Vec3 } from "../core/types";
import { STD_FRAMES, frameToWorld, worldToFrame } from "../core/math";
import { ViewCamera } from "./camera";
import { SketchEditor, drawBox, type PointerInfo } from "../sketch/editor";
import { sampleEntity } from "../sketch/model";

const T_FACE = 1, T_EDGE = 2, T_VERTEX = 3, T_PLANE = 4, T_SKETCH = 5;
const PICK = 13; // pick window in px

export type DisplayStyle = "shadedEdges" | "shaded" | "hlr" | "hlv" | "wireframe";

interface GestureState {
  x: number;
  y: number;
  active: boolean;
  dir: number | null;
}

export class Viewport {
  renderer: THREE.WebGLRenderer;
  scene = new THREE.Scene();
  pickScene = new THREE.Scene();
  cam = new ViewCamera();
  overlay: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  editor: SketchEditor | null = null;
  displayStyle: DisplayStyle = "shadedEdges";
  gesture: GestureState | null = null;
  onGesture: ((g: GestureState | null) => void) | null = null;
  onContextMenu: ((x: number, y: number, item: SelItem | null) => void) | null = null;

  private bodyGroup = new THREE.Group();
  private pickBody = new THREE.Group();
  private extrasGroup = new THREE.Group();
  private pickExtras = new THREE.Group();
  private hiEdges: LineSegments2;
  private hiMat: LineMaterial;
  private bodyMesh: THREE.Mesh | null = null;
  private faceRanges: [number, number][] = [];
  private currentMesh: BodyMesh | null = null;
  private pickTarget = new THREE.WebGLRenderTarget(PICK, PICK);
  private pickBuf = new Uint8Array(PICK * PICK * 4);
  private dirty = true;
  private raf = 0;
  private size = { w: 1, h: 1 };
  private nav: { mode: "rotate" | "pan" | "zoom" | "roll"; x: number; y: number; pivot: THREE.Vector3; axis?: THREE.Vector3; moved: boolean } | null = null;
  private rotateAbout: { pivot: THREE.Vector3; axis?: THREE.Vector3 } | null = null;
  private lastMiddleUp = 0;
  private box: { x0: number; y0: number; x1: number; y1: number } | null = null;
  private leftDown: { x: number; y: number } | null = null;
  private dirLight = new THREE.DirectionalLight(0xffffff, 1.6);
  private dirLight2 = new THREE.DirectionalLight(0xffffff, 0.5);
  private sketchPickIds: string[] = [];
  private planePickIds: string[] = [];
  private lastBuiltFrom: unknown = null;
  private lastStyleKey = "";

  constructor(private host: HTMLElement, private app: App) {
    const o = app.options;
    this.renderer = new THREE.WebGLRenderer({ antialias: o.performance.msaa, alpha: true, powerPreference: "high-performance", preserveDrawingBuffer: false });
    this.renderer.setPixelRatio(window.devicePixelRatio * o.performance.pixelRatio);
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.toneMapping = THREE.NoToneMapping;
    host.appendChild(this.renderer.domElement);
    this.renderer.domElement.className = "gl";
    this.overlay = document.createElement("canvas");
    this.overlay.className = "overlay";
    host.appendChild(this.overlay);
    this.ctx = this.overlay.getContext("2d")!;

    this.scene.add(this.bodyGroup, this.extrasGroup);
    this.pickScene.add(this.pickBody, this.pickExtras);
    this.scene.add(new THREE.HemisphereLight(0xffffff, 0x8a8f98, 1.25));
    this.scene.add(this.dirLight, this.dirLight.target, this.dirLight2, this.dirLight2.target);
    this.hiMat = new LineMaterial({ color: 0xffffff, linewidth: 3, vertexColors: true, depthTest: true });
    this.hiEdges = new LineSegments2(new LineSegmentsGeometry(), this.hiMat);
    this.hiEdges.renderOrder = 5;
    this.scene.add(this.hiEdges);

    app.requestRender = () => this.invalidate();
    app.viewHooks = {
      normalTo: (f, anim) => this.cam.normalTo(f, anim === false ? 0 : this.anim(), this.sketchFitBox(f)),
      fit: () => this.zoomToFit(),
      orient: (n) => this.orient(n),
    };

    const ro = new ResizeObserver(() => this.resize());
    ro.observe(host);
    this.resize();
    this.bindEvents();
    this.loop();
  }

  dispose() {
    cancelAnimationFrame(this.raf);
    this.renderer.dispose();
  }

  private anim() {
    return this.app.options.view.animationSpeed;
  }

  invalidate() {
    this.dirty = true;
  }

  private resize() {
    const r = this.host.getBoundingClientRect();
    this.size = { w: Math.max(1, r.width), h: Math.max(1, r.height) };
    this.renderer.setSize(this.size.w, this.size.h);
    const dpr = window.devicePixelRatio;
    this.overlay.width = this.size.w * dpr;
    this.overlay.height = this.size.h * dpr;
    this.overlay.style.width = this.size.w + "px";
    this.overlay.style.height = this.size.h + "px";
    this.cam.resize(this.size.w, this.size.h);
    this.hiMat.resolution.set(this.size.w, this.size.h);
    this.invalidate();
  }

  private loop = () => {
    this.raf = requestAnimationFrame(this.loop);
    if (this.cam.tick(performance.now())) this.dirty = true;
    if (!this.dirty) return;
    this.dirty = false;
    this.sync();
    this.render();
  };

  // ================================================================ scene sync

  /** Rebuild scene objects that depend on app state. Cheap when nothing changed. */
  private sync() {
    const app = this.app;
    // Sketch editor lifecycle.
    if (app.sketch && !this.editor) {
      this.editor = new SketchEditor(app, {
        toSketch: (sx, sy) => this.toSketch(sx, sy),
        toScreen: (p) => this.toScreen(p),
        pxSize: () => this.cam.worldPerPixel,
      });
      app.editor = this.editor;
    } else if (!app.sketch && this.editor) {
      this.editor = null;
      app.editor = null;
    }
    const mesh = (app.pm && "feature" in app.pm && app.previewResult?.mesh) || app.result?.mesh || null;
    const preview = !!(app.pm && "feature" in app.pm && app.previewResult?.mesh);
    const styleKey = `${this.displayStyle}|${preview}|${app.options.colors.model}|${app.options.colors.edges}`;
    if (mesh !== this.lastBuiltFrom || styleKey !== this.lastStyleKey) {
      this.lastBuiltFrom = mesh;
      this.lastStyleKey = styleKey;
      this.buildBody(mesh, preview);
    }
    this.updateHighlights();
    this.buildExtras();
    // Lights follow the camera (SOLIDWORKS default: lights fixed to the view).
    const q = this.cam.quat;
    const d1 = new THREE.Vector3(0.35, 0.55, 1).normalize().applyQuaternion(q);
    const d2 = new THREE.Vector3(-0.6, -0.3, 0.5).normalize().applyQuaternion(q);
    this.dirLight.position.copy(this.cam.target).addScaledVector(d1, 1000);
    this.dirLight.target.position.copy(this.cam.target);
    this.dirLight2.position.copy(this.cam.target).addScaledVector(d2, 1000);
    this.dirLight2.target.position.copy(this.cam.target);
  }

  private buildBody(mesh: BodyMesh | null, preview: boolean) {
    for (const g of [this.bodyGroup, this.pickBody]) {
      g.children.forEach((c) => {
        const m = c as THREE.Mesh;
        m.geometry?.dispose();
      });
      g.clear();
    }
    this.bodyMesh = null;
    this.currentMesh = mesh;
    if (!mesh) return;
    const C = this.app.options.colors;
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(mesh.positions, 3));
    geo.setAttribute("normal", new THREE.BufferAttribute(mesh.normals, 3));
    geo.setIndex(new THREE.BufferAttribute(mesh.indices, 1));
    const nv = mesh.positions.length / 3;
    const colors = new Float32Array(nv * 3);
    geo.setAttribute("color", new THREE.BufferAttribute(colors, 3));
    // Vertex range per face (replicad emits face vertices contiguously).
    this.faceRanges = [];
    for (const g of mesh.faceGroups) {
      let lo = Infinity, hi = -1;
      for (let i = g.start; i < g.start + g.count; i++) {
        const v = mesh.indices[i];
        if (v < lo) lo = v;
        if (v > hi) hi = v;
      }
      this.faceRanges[g.faceId] = [lo, hi + 1];
    }
    const style = this.displayStyle;
    const baseColor = new THREE.Color(style === "hlr" || style === "hlv" ? "#ffffff" : C.model);
    if (preview) baseColor.lerp(new THREE.Color("#ffd54a"), 0.35);
    const mat = new THREE.MeshStandardMaterial({
      vertexColors: true,
      roughness: 0.42,
      metalness: 0.08,
      polygonOffset: true,
      polygonOffsetFactor: 1,
      polygonOffsetUnits: 1,
      side: THREE.DoubleSide,
    });
    if (style === "hlr" || style === "hlv") {
      (mat as THREE.MeshStandardMaterial).emissive = new THREE.Color("#ffffff");
    }
    const body = new THREE.Mesh(geo, mat);
    body.userData.base = baseColor;
    if (style === "wireframe") body.visible = false;
    this.bodyMesh = body;
    this.bodyGroup.add(body);

    // Edges (seams hidden).
    const ep: number[] = [];
    const edgeIdPerVertex: number[] = [];
    for (const g of mesh.edgeGroups) {
      if (g.edgeId < 0 || mesh.edges[g.edgeId]?.seam) continue;
      for (let i = g.start; i < g.start + g.count; i++) {
        ep.push(mesh.edgePositions[i * 3], mesh.edgePositions[i * 3 + 1], mesh.edgePositions[i * 3 + 2]);
        edgeIdPerVertex.push(g.edgeId);
      }
    }
    const egeo = new THREE.BufferGeometry();
    egeo.setAttribute("position", new THREE.Float32BufferAttribute(ep, 3));
    if (style !== "shaded") {
      const edges = new THREE.LineSegments(egeo, new THREE.LineBasicMaterial({ color: C.edges }));
      this.bodyGroup.add(edges);
      if (style === "hlv") {
        const hidden = new THREE.LineSegments(
          egeo,
          new THREE.LineDashedMaterial({ color: "#8a8a8a", dashSize: this.cam.worldPerPixel * 6, gapSize: this.cam.worldPerPixel * 4, depthFunc: THREE.GreaterDepth }),
        );
        hidden.computeLineDistances();
        this.bodyGroup.add(hidden);
      }
    }

    // Pick geometry.
    const pc = new Float32Array(nv * 3);
    mesh.faceGroups.forEach((g) => {
      const r = this.faceRanges[g.faceId];
      if (!r) return;
      const c = encode(T_FACE, g.faceId);
      for (let v = r[0]; v < r[1]; v++) pc.set(c, v * 3);
    });
    const pgeo = new THREE.BufferGeometry();
    pgeo.setAttribute("position", geo.getAttribute("position"));
    pgeo.setIndex(geo.getIndex());
    pgeo.setAttribute("color", new THREE.BufferAttribute(pc, 3));
    const pmat = new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.DoubleSide, polygonOffset: true, polygonOffsetFactor: 2, polygonOffsetUnits: 2 });
    const pickFaces = new THREE.Mesh(pgeo, pmat);
    if (style === "wireframe") pickFaces.visible = false;
    this.pickBody.add(pickFaces);
    const ec = new Float32Array(edgeIdPerVertex.length * 3);
    edgeIdPerVertex.forEach((id, i) => ec.set(encode(T_EDGE, id), i * 3));
    const pegeo = new THREE.BufferGeometry();
    pegeo.setAttribute("position", new THREE.Float32BufferAttribute(ep, 3));
    pegeo.setAttribute("color", new THREE.BufferAttribute(ec, 3));
    this.pickBody.add(new THREE.LineSegments(pegeo, new THREE.LineBasicMaterial({ vertexColors: true })));
    const vp: number[] = [], vc: number[] = [];
    mesh.vertices.forEach((v, i) => {
      vp.push(...v);
      vc.push(...encode(T_VERTEX, i));
    });
    const vgeo = new THREE.BufferGeometry();
    vgeo.setAttribute("position", new THREE.Float32BufferAttribute(vp, 3));
    vgeo.setAttribute("color", new THREE.Float32BufferAttribute(vc, 3));
    this.pickBody.add(new THREE.Points(vgeo, new THREE.PointsMaterial({ size: 9, sizeAttenuation: false, vertexColors: true })));
  }

  /** Recolour hovered/selected faces and rebuild the fat highlight edges. */
  private updateHighlights() {
    const app = this.app;
    const C = app.options.colors;
    const mesh = this.currentMesh;
    const hiPos: number[] = [], hiCol: number[] = [];
    if (this.bodyMesh && mesh) {
      const attr = this.bodyMesh.geometry.getAttribute("color") as THREE.BufferAttribute;
      const arr = attr.array as Float32Array;
      const base = this.bodyMesh.userData.base as THREE.Color;
      for (let i = 0; i < arr.length; i += 3) {
        arr[i] = base.r;
        arr[i + 1] = base.g;
        arr[i + 2] = base.b;
      }
      const tint = (faceId: number, color: string, amount: number) => {
        const r = this.faceRanges[faceId];
        if (!r) return;
        const c = base.clone().lerp(new THREE.Color(color), amount);
        for (let v = r[0]; v < r[1]; v++) arr.set([c.r, c.g, c.b], v * 3);
      };
      const edgeHi = (edgeId: number, color: string) => {
        const c = new THREE.Color(color);
        for (const g of mesh.edgeGroups) {
          if (g.edgeId !== edgeId) continue;
          for (let i = g.start; i < g.start + g.count; i++) {
            hiPos.push(mesh.edgePositions[i * 3], mesh.edgePositions[i * 3 + 1], mesh.edgePositions[i * 3 + 2]);
            hiCol.push(c.r, c.g, c.b);
          }
        }
      };
      // Features selected in the tree highlight their sketch/faces loosely: selected faces only here.
      for (const s of app.selection) {
        if (s.kind === "face") tint(s.id, C.selected, 0.65);
        if (s.kind === "edge") edgeHi(s.id, C.selected);
      }
      const h = app.hover;
      if (h?.kind === "face" && !app.isSelected(h)) {
        tint(h.id, C.hover, 0.5);
        // SOLIDWORKS outlines the hovered face's boundary.
        const sig = mesh.faces[h.id];
        void sig;
      }
      if (h?.kind === "edge") edgeHi(h.id, C.hover);
      attr.needsUpdate = true;
    }
    const g = new LineSegmentsGeometry();
    if (hiPos.length) {
      g.setPositions(hiPos);
      g.setColors(hiCol);
    }
    this.hiEdges.geometry.dispose();
    this.hiEdges.geometry = g;
    this.hiEdges.visible = hiPos.length > 0;
  }

  /** Planes, inactive sketches, origin, vertex markers. */
  private buildExtras() {
    for (const g of [this.extrasGroup, this.pickExtras]) {
      g.traverse((c) => {
        const m = c as THREE.Mesh;
        m.geometry?.dispose();
      });
      g.clear();
    }
    const app = this.app;
    const C = app.options.colors;
    this.planePickIds = [];
    this.sketchPickIds = [];
    const box = this.modelBox();
    const size = box.isEmpty() ? 60 : Math.max(40, box.getSize(new THREE.Vector3()).length() * 0.6);
    // Standard planes: hidden by default; shown when hovered/selected or when picking a sketch plane.
    const showAll = app.pendingSketch;
    const planes: { id: string; frame: Frame; size: number; always?: boolean }[] = [
      { id: "front", frame: STD_FRAMES.front, size },
      { id: "top", frame: STD_FRAMES.top, size },
      { id: "right", frame: STD_FRAMES.right, size },
    ];
    for (const [id, p] of Object.entries(app.result?.planes ?? {})) {
      if (app.featureById(id)?.suppressed) continue;
      planes.push({ id, frame: p.frame, size: p.size, always: true });
    }
    for (const p of planes) {
      const item: SelItem = { kind: "plane", id: p.id };
      const sel = app.isSelected(item);
      const hov = app.hover?.kind === "plane" && app.hover.id === p.id;
      if (!(showAll || sel || hov || p.always)) continue;
      const color = sel ? C.selected : hov ? C.hover : C.plane;
      const s = p.size / 2;
      const corners: Vec2[] = [[-s, -s], [s, -s], [s, s], [-s, s]];
      const w = corners.map((c) => frameToWorld(p.frame, c));
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.Float32BufferAttribute([...w[0], ...w[1], ...w[2], ...w[0], ...w[2], ...w[3]], 3));
      const fill = new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0.18, side: THREE.DoubleSide, depthWrite: false }));
      fill.renderOrder = 2;
      this.extrasGroup.add(fill);
      const lgeo = new THREE.BufferGeometry().setFromPoints([...w, w[0]].map((v) => new THREE.Vector3(...v)));
      this.extrasGroup.add(new THREE.Line(lgeo, new THREE.LineBasicMaterial({ color: sel || hov ? color : "#4d7ab0" })));
      const pickIdx = this.planePickIds.push(p.id) - 1;
      const pgeo = geo.clone();
      const col = encode(T_PLANE, pickIdx);
      pgeo.setAttribute("color", new THREE.Float32BufferAttribute([...col, ...col, ...col, ...col, ...col, ...col], 3));
      this.pickExtras.add(new THREE.Mesh(pgeo, new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.DoubleSide })));
    }
    // Inactive sketches: unconsumed ones are shown (like SOLIDWORKS); consumed ones only when selected/hovered.
    const consumed = new Set(app.doc.features.flatMap((f) => ("sketchId" in f ? [f.sketchId] : [])));
    const frames = app.result?.frames ?? {};
    for (const f of app.activeFeatures()) {
      if (f.type !== "sketch" || f.suppressed || app.sketch?.featureId === f.id) continue;
      const item: SelItem = { kind: "feature", id: f.id };
      const sel = app.isSelected(item);
      const hov = app.hover?.kind === "feature" && app.hover.id === f.id;
      if (consumed.has(f.id) && !sel && !hov) continue;
      const frame = frames[f.id];
      if (!frame) continue;
      const pts = sketchSegments(f, frame);
      if (!pts.length) continue;
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
      this.extrasGroup.add(new THREE.LineSegments(geo, new THREE.LineBasicMaterial({ color: sel ? C.selected : hov ? C.hover : C.sketchInactive })));
      const idx = this.sketchPickIds.push(f.id) - 1;
      const pg = geo.clone();
      const col = encode(T_SKETCH, idx);
      const cols = new Float32Array(pts.length);
      for (let i = 0; i < pts.length; i += 3) cols.set(col, i);
      pg.setAttribute("color", new THREE.BufferAttribute(cols, 3));
      this.pickExtras.add(new THREE.LineSegments(pg, new THREE.LineBasicMaterial({ vertexColors: true })));
    }
    // Hovered/selected vertices.
    const mesh = this.currentMesh;
    const vtx = [...app.selection, ...(app.hover ? [app.hover] : [])].filter((s) => s.kind === "vertex");
    if (mesh && vtx.length) {
      for (const v of vtx) {
        const p = mesh.vertices[v.id as number];
        if (!p) continue;
        const g = new THREE.BufferGeometry();
        g.setAttribute("position", new THREE.Float32BufferAttribute(p, 3));
        const color = app.isSelected(v) ? C.selected : C.hover;
        const pts = new THREE.Points(g, new THREE.PointsMaterial({ color, size: 8, sizeAttenuation: false, depthTest: false }));
        pts.renderOrder = 10;
        this.extrasGroup.add(pts);
      }
    }
  }

  // ================================================================ rendering

  private render() {
    this.renderer.setRenderTarget(null);
    this.renderer.render(this.scene, this.cam.camera);
    this.drawOverlay();
  }

  private drawOverlay() {
    const ctx = this.ctx;
    const dpr = window.devicePixelRatio;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, this.size.w, this.size.h);
    if (this.editor && this.app.sketch) this.editor.draw(ctx);
    if (this.box) drawBox(ctx, this.box);
    if (this.rotateAbout) {
      const [x, y] = this.cam.project(this.rotateAbout.pivot);
      ctx.strokeStyle = "#d40000";
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(x, y, 6, 0, Math.PI * 2);
      ctx.moveTo(x - 10, y);
      ctx.lineTo(x + 10, y);
      ctx.moveTo(x, y - 10);
      ctx.lineTo(x, y + 10);
      ctx.stroke();
    }
    this.drawTriad(ctx);
  }

  /** Reference triad, bottom-left (red X, green Y, blue Z). */
  private drawTriad(ctx: CanvasRenderingContext2D) {
    const cx = 46, cy = this.size.h - 46, L = 30;
    const inv = this.cam.quat.clone().invert();
    const axes: [THREE.Vector3, string, string][] = [
      [new THREE.Vector3(1, 0, 0), "#e01010", "X"],
      [new THREE.Vector3(0, 1, 0), "#10a010", "Y"],
      [new THREE.Vector3(0, 0, 1), "#1030e0", "Z"],
    ];
    const proj = axes.map(([v, c, n]) => ({ v: v.clone().applyQuaternion(inv), c, n }));
    proj.sort((a, b) => a.v.z - b.v.z);
    ctx.font = "bold 11px Segoe UI, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (const { v, c, n } of proj) {
      const x = cx + v.x * L, y = cy - v.y * L;
      ctx.strokeStyle = c;
      ctx.fillStyle = c;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.lineTo(x, y);
      ctx.stroke();
      const len = Math.hypot(x - cx, y - cy);
      if (len > 4) {
        const ux = (x - cx) / len, uy = (y - cy) / len;
        ctx.beginPath();
        ctx.moveTo(x + ux * 6, y + uy * 6);
        ctx.lineTo(x - uy * 3, y + ux * 3);
        ctx.lineTo(x + uy * 3, y - ux * 3);
        ctx.fill();
      }
      ctx.fillText(n, cx + v.x * (L + 13), cy - v.y * (L + 13));
    }
  }

  // ================================================================ picking

  pick(sx: number, sy: number): SelItem | null {
    const cam = this.cam.camera as THREE.OrthographicCamera | THREE.PerspectiveCamera;
    const half = Math.floor(PICK / 2);
    cam.setViewOffset(this.size.w, this.size.h, sx - half, sy - half, PICK, PICK);
    this.renderer.setRenderTarget(this.pickTarget);
    this.renderer.setClearColor(0x000000, 1);
    this.renderer.clear();
    this.renderer.render(this.pickScene, cam);
    this.renderer.readRenderTargetPixels(this.pickTarget, 0, 0, PICK, PICK, this.pickBuf);
    this.renderer.setRenderTarget(null);
    this.renderer.setClearColor(0x000000, 0);
    cam.clearViewOffset();
    this.cam.update();
    const best: Record<number, { d: number; idx: number }> = {};
    for (let y = 0; y < PICK; y++)
      for (let x = 0; x < PICK; x++) {
        const o = ((PICK - 1 - y) * PICK + x) * 4;
        const v = (this.pickBuf[o] << 16) | (this.pickBuf[o + 1] << 8) | this.pickBuf[o + 2];
        if (!v) continue;
        const type = v >> 20, idx = (v & 0xfffff) - 1;
        const d = Math.hypot(x - half, y - half);
        if (!best[type] || d < best[type].d) best[type] = { d, idx };
      }
    const sel = this.app.pm?.kind;
    const wantEdgesOnly = sel === "chamfer";
    if (best[T_VERTEX] && best[T_VERTEX].d <= 5 && !wantEdgesOnly) return { kind: "vertex", id: best[T_VERTEX].idx };
    if (best[T_EDGE] && best[T_EDGE].d <= 4) return { kind: "edge", id: best[T_EDGE].idx };
    if (best[T_SKETCH] && best[T_SKETCH].d <= 4) return { kind: "feature", id: this.sketchPickIds[best[T_SKETCH].idx] };
    if (best[T_FACE] && best[T_FACE].d <= 1.5 && !wantEdgesOnly) return { kind: "face", id: best[T_FACE].idx };
    if (best[T_PLANE] && best[T_PLANE].d <= 1.5) return { kind: "plane", id: this.planePickIds[best[T_PLANE].idx] };
    return null;
  }

  /** 3D point on the body under the cursor (for rotate-about). */
  private hitPoint(sx: number, sy: number): THREE.Vector3 | null {
    if (!this.bodyMesh) return null;
    const rc = new THREE.Raycaster();
    rc.ray.copy(this.cam.ray(sx, sy));
    const hit = rc.intersectObject(this.bodyMesh, false)[0];
    return hit ? hit.point : null;
  }

  // ================================================================ sketch coordinate mapping

  private toSketch(sx: number, sy: number): Vec2 | null {
    const s = this.app.sketch;
    if (!s) return null;
    const f = s.frame;
    const ray = this.cam.ray(sx, sy);
    const n = new THREE.Vector3(...f.normal);
    const o = new THREE.Vector3(...f.origin);
    const denom = n.dot(ray.direction);
    if (Math.abs(denom) < 1e-9) return null;
    const t = o.clone().sub(ray.origin).dot(n) / denom;
    const p = ray.origin.clone().addScaledVector(ray.direction, t);
    return worldToFrame(f, [p.x, p.y, p.z]);
  }

  private toScreen(p: Vec2): [number, number] {
    const s = this.app.sketch!;
    const w = frameToWorld(s.frame, p);
    return this.cam.project(new THREE.Vector3(...w));
  }

  // ================================================================ view commands

  modelBox(): THREE.Box3 {
    const b = new THREE.Box3();
    const mesh = this.app.result?.mesh;
    if (mesh?.bbox) {
      b.expandByPoint(new THREE.Vector3(...mesh.bbox[0]));
      b.expandByPoint(new THREE.Vector3(...mesh.bbox[1]));
    }
    const frames = this.app.result?.frames ?? {};
    for (const f of this.app.activeFeatures()) {
      if (f.type !== "sketch" || !frames[f.id]) continue;
      for (const p of Object.values(f.sketch.points)) b.expandByPoint(new THREE.Vector3(...frameToWorld(frames[f.id], [p.x, p.y])));
    }
    if (this.app.sketch) {
      for (const p of Object.values(this.app.sketch.data.points)) b.expandByPoint(new THREE.Vector3(...frameToWorld(this.app.sketch.frame, [p.x, p.y])));
    }
    if (b.isEmpty() || b.getSize(new THREE.Vector3()).length() < 1e-6) {
      b.expandByPoint(new THREE.Vector3(-50, -50, -50));
      b.expandByPoint(new THREE.Vector3(50, 50, 50));
    }
    return b;
  }

  private sketchFitBox(_f: Frame): THREE.Box3 | undefined {
    const b = this.modelBox();
    return b;
  }

  zoomToFit() {
    this.cam.zoomToFit(this.modelBox(), this.anim());
    this.invalidate();
  }

  orient(name: string) {
    if (name === "normalTo") return this.normalToSelection();
    this.cam.setView(name, true, this.anim(), this.modelBox());
    this.invalidate();
  }

  normalToSelection() {
    const app = this.app;
    let frame: Frame | null = null;
    if (app.sketch && !app.selection.length) frame = app.sketch.frame;
    else {
      const p = app.planeRefFromSelection();
      if (p) frame = p.frame;
    }
    if (!frame) {
      app.flash("Select a planar face or plane for Normal To.", "info");
      return;
    }
    this.cam.normalTo(frame, this.anim(), this.modelBox());
    this.invalidate();
  }

  zoomStep(inward: boolean) {
    this.cam.zoomAt(inward ? 1.25 : 1 / 1.25, this.size.w / 2, this.size.h / 2);
    this.invalidate();
  }

  rotateStep(dxDeg: number, dyDeg: number) {
    const pivot = this.modelBox().getCenter(new THREE.Vector3());
    const right = new THREE.Vector3(1, 0, 0).applyQuaternion(this.cam.quat);
    const up = new THREE.Vector3(0, 1, 0).applyQuaternion(this.cam.quat);
    if (dxDeg) this.cam.rotateAxis(up, THREE.MathUtils.degToRad(dxDeg), pivot);
    if (dyDeg) this.cam.rotateAxis(right, THREE.MathUtils.degToRad(dyDeg), pivot);
    this.invalidate();
  }

  rollStep(deg: number) {
    this.cam.roll(THREE.MathUtils.degToRad(deg));
    this.invalidate();
  }

  panStep(dx: number, dy: number) {
    this.cam.pan(dx, dy);
    this.invalidate();
  }

  setDisplayStyle(s: DisplayStyle) {
    this.displayStyle = s;
    this.invalidate();
  }

  // ================================================================ input

  private bindEvents() {
    const el = this.overlay;
    el.tabIndex = 0;
    el.addEventListener("contextmenu", (e) => e.preventDefault());
    el.addEventListener("pointerdown", this.onPointerDown);
    el.addEventListener("pointermove", this.onPointerMove);
    el.addEventListener("pointerup", this.onPointerUp);
    el.addEventListener("pointerleave", () => {
      if (!this.nav) this.app.setHover(null);
    });
    el.addEventListener("dblclick", this.onDblClick);
    el.addEventListener("wheel", this.onWheel, { passive: false });
    el.addEventListener("auxclick", (e) => e.preventDefault());
  }

  private info(e: PointerEvent | MouseEvent): PointerInfo {
    const r = this.overlay.getBoundingClientRect();
    return { sx: e.clientX - r.left, sy: e.clientY - r.top, button: e.button, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey };
  }

  private onWheel = (e: WheelEvent) => {
    e.preventDefault();
    const p = this.info(e);
    // SOLIDWORKS default: wheel forward (deltaY < 0) zooms out; option reverses.
    let inward = e.deltaY > 0;
    if (this.app.options.view.reverseWheelZoom) inward = !inward;
    const steps = Math.min(3, Math.max(1, Math.abs(e.deltaY) / 100));
    const factor = Math.pow(e.shiftKey ? 1.35 : 1.18, steps);
    const center = this.app.options.view.zoomAboutScreenCenter;
    this.cam.zoomAt(inward ? factor : 1 / factor, center ? this.size.w / 2 : p.sx, center ? this.size.h / 2 : p.sy);
    this.invalidate();
    // Keep sketch inference live while zooming.
    if (this.editor) this.editor.onMove(p);
  };

  private onPointerDown = (e: PointerEvent) => {
    this.overlay.focus();
    this.overlay.setPointerCapture(e.pointerId);
    const p = this.info(e);
    if (e.button === 1) {
      e.preventDefault();
      const pivotData = this.rotateAbout;
      const mode = e.ctrlKey ? "pan" : e.shiftKey ? "zoom" : e.altKey ? "roll" : "rotate";
      const pivot = pivotData?.pivot ?? this.modelBox().getCenter(new THREE.Vector3());
      this.nav = { mode, x: p.sx, y: p.sy, pivot, axis: pivotData?.axis, moved: false };
      this.cam.pushUndo();
      return;
    }
    if (e.button === 2) {
      this.gesture = { x: p.sx, y: p.sy, active: false, dir: null };
      return;
    }
    if (e.button !== 0) return;
    if (this.editor && this.app.sketch) {
      // In a sketch, clicks on model geometry are only used while no tool is active and nothing is hit.
      this.editor.onDown(p);
      this.invalidate();
      return;
    }
    this.leftDown = { x: p.sx, y: p.sy };
  };

  private onPointerMove = (e: PointerEvent) => {
    const p = this.info(e);
    if (this.nav) {
      const n = this.nav;
      const dx = p.sx - n.x, dy = p.sy - n.y;
      if (Math.abs(dx) + Math.abs(dy) > 0) n.moved = true;
      n.x = p.sx;
      n.y = p.sy;
      if (n.mode === "rotate") {
        if (n.axis) this.cam.rotateAxis(n.axis, THREE.MathUtils.degToRad(dx * 0.4), n.pivot);
        else this.cam.rotate(dx, dy, n.pivot);
      } else if (n.mode === "pan") this.cam.pan(dx, dy);
      else if (n.mode === "zoom") this.cam.zoomAt(Math.pow(1.01, -dy), this.size.w / 2, this.size.h / 2);
      else if (n.mode === "roll") this.cam.roll(THREE.MathUtils.degToRad(dx * 0.4));
      this.invalidate();
      return;
    }
    if (this.gesture) {
      const g = this.gesture;
      const d = Math.hypot(p.sx - g.x, p.sy - g.y);
      if (d > 12) g.active = true;
      if (g.active) {
        g.dir = d > 40 ? Math.round(Math.atan2(-(p.sy - g.y), p.sx - g.x) / (Math.PI / 2)) & 3 : null;
        this.onGesture?.({ ...g });
      }
      return;
    }
    if (this.editor && this.app.sketch) {
      this.editor.onMove(p);
      this.invalidate();
      return;
    }
    if (this.leftDown) {
      const d = Math.hypot(p.sx - this.leftDown.x, p.sy - this.leftDown.y);
      if (d > 4 || this.box) {
        this.box = { x0: this.leftDown.x, y0: this.leftDown.y, x1: p.sx, y1: p.sy };
        this.invalidate();
        return;
      }
    }
    if (e.buttons === 0) this.app.setHover(this.filterPick(this.pick(p.sx, p.sy)));
  };

  private onPointerUp = (e: PointerEvent) => {
    const p = this.info(e);
    if (e.button === 1 && this.nav) {
      const n = this.nav;
      this.nav = null;
      if (!n.moved) {
        // Middle click: double-click = zoom to fit; on an entity = set rotation centre.
        const now = performance.now();
        if (now - this.lastMiddleUp < 350) {
          this.rotateAbout = null;
          this.zoomToFit();
        } else {
          const hit = this.pick(p.sx, p.sy);
          this.rotateAbout = hit ? this.rotateCenterFor(hit, p) : null;
        }
        this.lastMiddleUp = now;
      } else {
        this.rotateAbout = null;
      }
      this.invalidate();
      return;
    }
    if (e.button === 2 && this.gesture) {
      const g = this.gesture;
      this.gesture = null;
      this.onGesture?.(null);
      if (!g.active) {
        const item = this.app.sketch ? null : this.filterPick(this.pick(p.sx, p.sy));
        this.onContextMenu?.(e.clientX, e.clientY, item);
      } else if (g.dir !== null) this.runGesture(g.dir);
      return;
    }
    if (e.button !== 0) return;
    if (this.editor && this.app.sketch) {
      this.editor.onUp(p);
      this.invalidate();
      return;
    }
    const down = this.leftDown;
    this.leftDown = null;
    if (this.box) {
      this.boxSelectModel(this.box, e.ctrlKey);
      this.box = null;
      this.invalidate();
      return;
    }
    if (!down) return;
    const hit = this.filterPick(this.pick(p.sx, p.sy));
    const mode = e.ctrlKey ? "toggle" : e.shiftKey ? "add" : this.selectionCollectingPM() ? "toggle" : "replace";
    if (!hit) {
      if (mode === "replace") this.app.select(null);
      return;
    }
    this.app.select(hit, mode);
    if (this.app.pendingSketch && hit.kind === "plane") this.app.startSketch();
    else if (this.app.pendingSketch && hit.kind === "face" && this.currentMesh?.faces[hit.id]?.planar) this.app.startSketch();
  };

  private onDblClick = (e: MouseEvent) => {
    const p = this.info(e);
    if (this.editor && this.app.sketch) {
      this.editor.onDblClick(p);
      this.invalidate();
    }
  };

  private selectionCollectingPM() {
    const k = this.app.pm?.kind;
    return k === "fillet" || k === "chamfer";
  }

  /** Apply PropertyManager selection filters (e.g. fillet wants edges/faces). */
  private filterPick(hit: SelItem | null): SelItem | null {
    const k = this.app.pm?.kind;
    if (!hit) return null;
    if (k === "fillet" && hit.kind !== "edge" && hit.kind !== "face") return null;
    if (k === "chamfer" && hit.kind !== "edge") return null;
    return hit;
  }

  private rotateCenterFor(hit: SelItem, p: PointerInfo) {
    const mesh = this.currentMesh;
    if (!mesh) return null;
    if (hit.kind === "vertex") return { pivot: new THREE.Vector3(...mesh.vertices[hit.id]) };
    if (hit.kind === "edge") {
      const e = mesh.edges[hit.id];
      if (e.linear) {
        const a = new THREE.Vector3(...e.sig.a), b = new THREE.Vector3(...e.sig.b);
        return { pivot: a.clone().add(b).multiplyScalar(0.5), axis: b.sub(a).normalize() };
      }
      return { pivot: new THREE.Vector3(...e.sig.mid) };
    }
    if (hit.kind === "face") {
      const hp = this.hitPoint(p.sx, p.sy);
      return hp ? { pivot: hp } : null;
    }
    return null;
  }

  private boxSelectModel(b: { x0: number; y0: number; x1: number; y1: number }, ctrl: boolean) {
    const mesh = this.currentMesh;
    if (!mesh) return;
    const xa = Math.min(b.x0, b.x1), xb = Math.max(b.x0, b.x1), ya = Math.min(b.y0, b.y1), yb = Math.max(b.y0, b.y1);
    const crossing = b.x1 < b.x0;
    const inside = (v: THREE.Vector3) => {
      const [x, y] = this.cam.project(v);
      return x >= xa && x <= xb && y >= ya && y <= yb;
    };
    if (!ctrl) this.app.selection = [];
    for (const g of mesh.edgeGroups) {
      if (g.edgeId < 0 || mesh.edges[g.edgeId].seam) continue;
      const pts: THREE.Vector3[] = [];
      for (let i = g.start; i < g.start + g.count; i++) pts.push(new THREE.Vector3(mesh.edgePositions[i * 3], mesh.edgePositions[i * 3 + 1], mesh.edgePositions[i * 3 + 2]));
      const ok = crossing ? pts.some(inside) : pts.every(inside);
      if (ok) this.app.select({ kind: "edge", id: g.edgeId }, "add");
    }
    this.app.changed();
  }

  /** Mouse gestures: 0=right, 1=up, 2=left, 3=down. */
  gestureCommands(): { label: string; run: () => void }[] {
    const app = this.app;
    if (app.sketch && this.editor) {
      const ed = this.editor;
      return [
        { label: "Line", run: () => ed.setTool("line") },
        { label: "Smart Dimension", run: () => ed.setTool("smartDimension") },
        { label: "Circle", run: () => ed.setTool("circle") },
        { label: "Corner Rectangle", run: () => ed.setTool("rectangle") },
      ];
    }
    return [
      { label: "Right", run: () => this.orient("right") },
      { label: "Top", run: () => this.orient("top") },
      { label: "Left", run: () => this.orient("left") },
      { label: "Bottom", run: () => this.orient("bottom") },
    ];
  }

  private runGesture(dir: number) {
    this.gestureCommands()[dir]?.run();
  }
}

// ---------------------------------------------------------------- helpers

function encode(type: number, idx: number): [number, number, number] {
  const v = (type << 20) | ((idx + 1) & 0xfffff);
  return [((v >> 16) & 255) / 255, ((v >> 8) & 255) / 255, (v & 255) / 255];
}

function sketchSegments(f: SketchFeature, frame: Frame): number[] {
  const out: number[] = [];
  const sk = f.sketch;
  for (const e of Object.values(sk.entities)) {
    if (e.type === "point") continue;
    const pts = sampleEntity(sk, e, 64).map((p) => frameToWorld(frame, p));
    for (let i = 1; i < pts.length; i++) out.push(...pts[i - 1], ...pts[i]);
  }
  return out;
}

export type { Vec3 };
