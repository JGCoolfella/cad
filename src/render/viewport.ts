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
import type { BodyMesh, ExtrudeFeature, Frame, SketchFeature, Vec2, Vec3 } from "../core/types";
import { STD_FRAMES, frameForPlane, frameToWorld, worldToFrame } from "../core/math";
import { sketchesOf } from "../core/features";
import { formatLength } from "../core/units";
import { featureParams } from "../core/featureParams";
import { ViewCamera } from "./camera";
import { ViewSelector } from "./viewSelector";
import { autoExtents, buildPlane, type Extents, type PlaneStyle } from "./planes";
import { SketchEditor, arrowHead, drawBox, type PointerInfo } from "../sketch/editor";
import { pointInPoly, sampleEntity, type Loop } from "../sketch/model";

/** Smallest closed area of a sketch containing a point (Selected Contours). */
function smallestFace(faces: Loop[], p: Vec2): Loop | null {
  let best: Loop | null = null;
  for (const f of faces) if ((!best || f.area < best.area) && pointInPoly(p, f.poly)) best = f;
  return best;
}

const T_FACE = 1, T_EDGE = 2, T_VERTEX = 3, T_PLANE = 4, T_SKETCH = 5, T_REF = 6;
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
  /** Section View: clipping plane and the stencil-built cap of the cut. */
  private clipPlane = new THREE.Plane();
  private capGroup = new THREE.Group();
  private capFor: unknown = null;
  /** Yellow transparent body of the feature being previewed. */
  private toolGroup = new THREE.Group();
  private lastTool: unknown = null;
  /** Read-only renderers for sketches whose dimensions are shown (double-clicked features). */
  private dimEditors = new Map<string, { ed: SketchEditor; frame: Frame }>();
  /** Screen boxes of the feature dimensions drawn this frame (for double-click). */
  private featureDimBoxes: { featureId: string; key: string; box: [number, number, number, number] }[] = [];
  /** Read-only renderer for the profile sketch of the open feature PropertyManager. */
  private pmSketch: { id: string; ed: SketchEditor } | null = null;
  /** Extrude depth arrow (drag to change the depth). */
  private depthDrag: { base: THREE.Vector3; dir: THREE.Vector3; mid: boolean } | null = null;
  private depthHover = false;
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
  /** Reference axes / points / coordinate systems drawn in the graphics area (pick ids). */
  private refPickIds: string[] = [];
  private refLabels: { id: string; at: THREE.Vector3 }[] = [];
  private lastBuiltFrom: unknown = null;
  private lastStyleKey = "";
  /** Left-drag view mode chosen from the context menu (Rotate View, Pan, Zoom In/Out, Roll View). */
  viewMode: "rotate" | "pan" | "zoom" | "roll" | "zoomArea" | null = null;
  private navLeft = false;
  private zoomBox = false;
  private downClient: { x: number; y: number } | null = null;
  /** Called after a plain left click that leaves something selected (context toolbar). */
  onSelectionClick: ((clientX: number, clientY: number) => void) | null = null;
  /** 3D View Selector cube. */
  selector = new ViewSelector();
  /** Called after a view selector facet is picked (UI closes its menu). */
  onSelectorPick: (() => void) | null = null;
  /** Plane resize handles of selected planes (world position + owning plane). */
  private planeHandles: { id: string; idx: number; world: Vec3; frame: Frame; ext: Extents }[] = [];
  private handleDrag: { id: string; idx: number; frame: Frame; ext: Extents } | null = null;
  /** Last pointer position in the graphics area (for hover tooltips). */
  private pointer: { x: number; y: number } | null = null;

  constructor(private host: HTMLElement, private app: App) {
    const o = app.options;
    this.renderer = new THREE.WebGLRenderer({ antialias: o.performance.msaa, alpha: true, powerPreference: "high-performance", preserveDrawingBuffer: false, stencil: true });
    // Section View clips with a plane; caps are drawn with the stencil buffer.
    this.renderer.localClippingEnabled = true;
    this.renderer.setPixelRatio(window.devicePixelRatio * o.performance.pixelRatio);
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.toneMapping = THREE.NoToneMapping;
    host.appendChild(this.renderer.domElement);
    this.renderer.domElement.className = "gl";
    this.overlay = document.createElement("canvas");
    this.overlay.className = "overlay";
    host.appendChild(this.overlay);
    this.ctx = this.overlay.getContext("2d")!;

    this.scene.add(this.bodyGroup, this.toolGroup, this.extrasGroup, this.capGroup);
    this.pickScene.add(this.pickBody, this.pickExtras);
    this.scene.add(new THREE.HemisphereLight(0xffffff, 0x8a8f98, 1.25));
    this.scene.add(this.dirLight, this.dirLight.target, this.dirLight2, this.dirLight2.target);
    this.hiMat = new LineMaterial({ color: 0xffffff, linewidth: 3, vertexColors: true, depthTest: true });
    this.hiEdges = new LineSegments2(new LineSegmentsGeometry(), this.hiMat);
    this.hiEdges.renderOrder = 5;
    this.scene.add(this.hiEdges);

    app.requestRender = () => {
      this.ensureEditor();
      this.invalidate();
    };
    app.viewHooks = {
      normalTo: (f, anim) => {
        this.cam.normalTo(f, anim === false ? 0 : this.anim(), this.sketchFitBox(f));
        this.invalidate();
      },
      leaveNormal: (f) => {
        const d = Math.abs(new THREE.Vector3(0, 0, 1).applyQuaternion(this.cam.targetQuat()).dot(new THREE.Vector3(...f.normal)));
        if (d > 0.985) {
          this.cam.setView("trimetric", true, this.anim(), this.modelBox());
          this.invalidate();
        }
      },
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
    this.renderer.domElement.remove();
    this.overlay.remove();
  }

  private anim() {
    return this.app.transition();
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

  /**
   * Create or drop the sketch editor to match the app's sketch session. Runs on every
   * app change (not only on rendered frames) so tools work even before the next frame.
   */
  private editorFor: string | null = null;
  private ensureEditor() {
    const app = this.app;
    if (app.sketch && (!this.editor || this.editorFor !== app.sketch.featureId)) {
      this.editor = new SketchEditor(app, {
        toSketch: (sx, sy) => this.toSketch(sx, sy),
        toScreen: (p) => this.toScreen(p),
        toSketchPoint: (sx, sy) => this.toSketch(sx, sy),
        pxSize: () => this.cam.worldPerPixel,
      });
      this.editorFor = app.sketch.featureId;
      app.editor = this.editor;
    } else if (!app.sketch && this.editor) {
      this.editor = null;
      this.editorFor = null;
      app.editor = null;
    }
  }

  /** Rebuild scene objects that depend on app state. Cheap when nothing changed. */
  private sync() {
    const app = this.app;
    this.ensureEditor();
    const inPM = !!(app.pm && "feature" in app.pm);
    const mesh = (inPM && app.previewResult?.mesh) || app.result?.mesh || null;
    const tool = (inPM && app.previewResult?.toolMesh) || null;
    const styleKey = `${this.displayStyle}|${this.modelColor()}|${app.options.colors.edges}|${app.opt("display.hiddenEdges")}`;
    if (mesh !== this.lastBuiltFrom || styleKey !== this.lastStyleKey) {
      this.lastBuiltFrom = mesh;
      this.lastStyleKey = styleKey;
      this.buildBody(mesh, false);
    }
    if (tool !== this.lastTool) {
      this.lastTool = tool;
      const pm = app.pm;
      this.buildTool(tool, !!(pm && (pm.kind === "extrude" || pm.kind === "revolve") && pm.feature.cut));
    }
    this.updateHighlights();
    this.syncSection();
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
    const baseColor = new THREE.Color(style === "hlr" || style === "hlv" ? "#ffffff" : this.modelColor());
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
        const dashed = this.app.opt<string>("display.hiddenEdges") !== "solid";
        const hidden = new THREE.LineSegments(
          egeo,
          dashed
            ? new THREE.LineDashedMaterial({ color: "#8a8a8a", dashSize: this.cam.worldPerPixel * 6, gapSize: this.cam.worldPerPixel * 4, depthFunc: THREE.GreaterDepth })
            : new THREE.LineBasicMaterial({ color: "#8a8a8a", depthFunc: THREE.GreaterDepth }),
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

  /**
   * SOLIDWORKS-style feature preview: the body a boss adds (or a cut removes) drawn in
   * transparent yellow with yellow edges, over the unchanged part.
   */
  private buildTool(mesh: BodyMesh | null, cut = false) {
    this.toolGroup.traverse((c) => {
      const m = c as THREE.Mesh;
      m.geometry?.dispose();
      (m.material as THREE.Material | undefined)?.dispose?.();
    });
    this.toolGroup.clear();
    if (!mesh) return;
    const color = this.app.opt<string>("color.tempShaded") || "#ffff80";
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(mesh.positions, 3));
    geo.setAttribute("normal", new THREE.BufferAttribute(mesh.normals, 3));
    geo.setIndex(new THREE.BufferAttribute(mesh.indices, 1));
    const body = new THREE.Mesh(
      geo,
      new THREE.MeshStandardMaterial({ color: new THREE.Color(color).lerp(new THREE.Color("#c8b400"), 0.25), transparent: true, opacity: cut ? 0.55 : 0.42, depthWrite: false, depthTest: !cut, side: THREE.DoubleSide, roughness: 0.6, metalness: 0, polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -1 }),
    );
    body.renderOrder = 2;
    this.toolGroup.add(body);
    const ep: number[] = [];
    for (const g of mesh.edgeGroups) {
      if (g.edgeId < 0 || mesh.edges[g.edgeId]?.seam) continue;
      for (let i = g.start; i < g.start + g.count; i++) ep.push(mesh.edgePositions[i * 3], mesh.edgePositions[i * 3 + 1], mesh.edgePositions[i * 3 + 2]);
    }
    const egeo = new LineSegmentsGeometry();
    egeo.setPositions(ep);
    const edges = new LineSegments2(egeo, new LineMaterial({ color: 0xf0e000, linewidth: 1.6, resolution: this.hiMat.resolution, transparent: true, depthTest: false }));
    edges.renderOrder = 3;
    this.toolGroup.add(edges);
  }

  /**
   * Section View: clip the body, its edges and pick geometry with the section plane, and
   * cap the cut (stencil: back faces +1, front faces -1, cap drawn where non-zero).
   */
  private syncSection() {
    const sp = this.app.sectionPlane();
    const planes = sp ? [this.clipPlane] : [];
    if (sp) {
      const n = new THREE.Vector3(...sp.n);
      this.clipPlane.set(n.clone().negate(), n.dot(new THREE.Vector3(...sp.o)));
    }
    const apply = (o: THREE.Object3D) =>
      o.traverse((c) => {
        const m = (c as THREE.Mesh).material as THREE.Material | THREE.Material[] | undefined;
        for (const mat of Array.isArray(m) ? m : m ? [m] : []) if (mat.clippingPlanes !== planes && (mat.clippingPlanes?.length ?? 0) !== planes.length) mat.clippingPlanes = planes;
      });
    for (const g of [this.bodyGroup, this.toolGroup, this.pickBody]) apply(g);
    this.hiMat.clippingPlanes = planes;
    const key = sp && this.bodyMesh ? this.bodyMesh : null;
    const capKey = key ? `${JSON.stringify(sp)}` : "";
    if (this.capFor === key && this.capGroup.userData.key === capKey) return;
    this.capFor = key;
    this.capGroup.userData.key = capKey;
    this.capGroup.traverse((c) => ((c as THREE.Mesh).material as THREE.Material | undefined)?.dispose?.());
    this.capGroup.clear();
    if (!sp || !this.bodyMesh) return;
    const geo = this.bodyMesh.geometry;
    const stencil = (side: THREE.Side, op: THREE.StencilOp) => {
      const m = new THREE.Mesh(
        geo,
        new THREE.MeshBasicMaterial({ side, clippingPlanes: planes, colorWrite: false, depthWrite: false, depthTest: false, stencilWrite: true, stencilFunc: THREE.AlwaysStencilFunc, stencilFail: op, stencilZFail: op, stencilZPass: op }),
      );
      m.renderOrder = 1;
      return m;
    };
    this.capGroup.add(stencil(THREE.BackSide, THREE.IncrementWrapStencilOp), stencil(THREE.FrontSide, THREE.DecrementWrapStencilOp));
    const size = this.modelBox().getSize(new THREE.Vector3()).length() * 4 + 10;
    const cap = new THREE.Mesh(
      new THREE.PlaneGeometry(size, size),
      new THREE.MeshStandardMaterial({ color: new THREE.Color(this.modelColor()).multiplyScalar(0.85), roughness: 0.6, metalness: 0.05, side: THREE.DoubleSide, stencilWrite: true, stencilRef: 0, stencilFunc: THREE.NotEqualStencilFunc, stencilFail: THREE.ReplaceStencilOp, stencilZFail: THREE.ReplaceStencilOp, stencilZPass: THREE.ReplaceStencilOp }),
    );
    cap.position.set(...sp.o);
    cap.quaternion.setFromUnitVectors(new THREE.Vector3(0, 0, 1), new THREE.Vector3(...sp.n));
    cap.renderOrder = 1.1;
    cap.onAfterRender = (r) => r.clearStencil();
    this.capGroup.add(cap);
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
      const faceTint = app.opt("display.shadedFaceHighlight") !== false;
      const faceEdges = (faceId: number, color: string) => {
        const sig = mesh.faces[faceId]?.sig;
        if (!sig) return;
        // Without shaded highlighting, outline the face via edges whose midpoints lie on it (approximation).
        void color;
      };
      // Fillet/chamfer previews: the faces the feature creates are shown in preview yellow.
      const pmId = app.pm && "feature" in app.pm && app.previewResult?.mesh && !app.previewResult.toolMesh ? app.pm.feature.id : null;
      if (pmId) mesh.faces.forEach((f) => f.owner === pmId && tint(f.id, "#f0dc3c", 0.75));
      for (const s of app.selection) {
        if (s.kind === "face") faceTint ? tint(s.id, C.selected, 0.65) : faceEdges(s.id, C.selected);
        if (s.kind === "edge") edgeHi(s.id, C.selected);
      }
      const h = app.opt("display.dynamicHighlight") === false ? null : app.hover;
      if (h?.kind === "face" && !app.isSelected(h) && faceTint) {
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
    this.refPickIds = [];
    this.refLabels = [];
    this.buildRefGeometry();
    this.planeHandles = [];
    // Planes size themselves to the model (the body, or the sketches before there is one).
    const content = this.contentBox();
    const bodyBox = this.bodyBox() ?? (content.isEmpty() ? null : content);
    // Standard planes are hidden by default (shown when hovered/selected, when picking a
    // sketch plane, or when the user shows them); reference planes are shown by default.
    const disp = app.doc.display ?? {};
    const shownStd = new Set(disp.shownPlanes ?? []);
    const hiddenRef = new Set(disp.hiddenPlanes ?? []);
    const planes: { id: string; name: string; frame: Frame; visible: boolean }[] = [
      { id: "front", name: "Front Plane", frame: STD_FRAMES.front, visible: shownStd.has("front") },
      { id: "top", name: "Top Plane", frame: STD_FRAMES.top, visible: shownStd.has("top") },
      { id: "right", name: "Right Plane", frame: STD_FRAMES.right, visible: shownStd.has("right") },
    ];
    // While a PropertyManager previews, show the planes of the preview (a new plane included).
    const pmFeature = app.pm && "feature" in app.pm ? app.pm.feature : null;
    const planeSrc = (pmFeature && app.previewResult) || app.result;
    for (const [id, p] of Object.entries(planeSrc?.planes ?? {})) {
      const f = app.featureById(id) ?? (pmFeature?.id === id ? pmFeature : undefined);
      if (!f || f.suppressed) continue;
      planes.push({ id, name: f.name, frame: p.frame, visible: !hiddenRef.has(id) });
    }
    const style: PlaneStyle = {
      front: app.docProp<string>("plane.front"),
      back: app.docProp<string>("plane.back"),
      transparency: app.docProp<number>("plane.transparency"),
      border: "#6c9be0",
      hover: C.hover,
      selected: C.selected,
      shaded: app.opt<boolean>("display.shadedPlanes") !== false,
    };
    const pmSketchId = this.pmSketchId();
    const pmSketchF = pmSketchId ? app.featureById(pmSketchId) : null;
    const pmPlane = pmSketchF?.type === "sketch" ? (pmSketchF.plane.kind === "std" ? pmSketchF.plane.plane : pmSketchF.plane.kind === "refplane" ? pmSketchF.plane.featureId : null) : null;
    for (const p of planes) {
      const item: SelItem = { kind: "plane", id: p.id };
      const sel = app.isSelected(item);
      const hov = app.hover?.kind === "plane" && app.hover.id === p.id;
      // The profile's sketch plane is shown while a feature previews, as in SOLIDWORKS.
      const sketchPlane = (app.sketch && sketchPlaneId(app) === p.id) || pmPlane === p.id;
      const planesOn = app.opt("view.planes") !== false;
      if (!(app.pendingSketch || sel || hov || (p.visible && planesOn) || sketchPlane)) continue;
      const ext = disp.planeExtents?.[p.id] ?? autoExtents(p.frame, bodyBox);
      const vis = buildPlane(p.name, p.frame, ext, { hover: hov, selected: sel }, style, this.hiMat.resolution);
      this.extrasGroup.add(vis.group);
      vis.handles.forEach((w, i) => this.planeHandles.push({ id: p.id, idx: i, world: w, frame: p.frame, ext }));
      const pickIdx = this.planePickIds.push(p.id) - 1;
      const pgeo = vis.pick.geometry;
      const col = encode(T_PLANE, pickIdx);
      pgeo.setAttribute("color", new THREE.Float32BufferAttribute([...col, ...col, ...col, ...col, ...col, ...col], 3));
      this.pickExtras.add(new THREE.Mesh(pgeo, new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.DoubleSide })));
    }
    // Inactive sketches: unconsumed ones are shown (like SOLIDWORKS); consumed ones only when selected/hovered.
    const consumed = new Set(app.doc.features.flatMap((f) => sketchesOf(f)));
    const frames = app.result?.frames ?? {};
    for (const f of app.activeFeatures()) {
      if (f.type !== "sketch" || f.suppressed || app.sketch?.featureId === f.id) continue;
      const item: SelItem = { kind: "feature", id: f.id };
      const sel = app.isSelected(item);
      const hov = app.hover?.kind === "feature" && app.hover.id === f.id;
      const isProfile = f.id === pmSketchId;
      if ((consumed.has(f.id) || !app.isSketchVisible(f.id)) && !sel && !hov && !isProfile) continue;
      const frame = frames[f.id];
      if (!frame) continue;
      const pts = sketchSegments(f, frame);
      if (!pts.length) continue;
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
      // The profile of a previewing feature draws over the transparent preview (same render pass).
      const lineMat = new THREE.LineBasicMaterial({ color: sel ? C.selected : hov ? C.hover : isProfile ? "#141414" : C.sketchInactive, depthTest: !isProfile, transparent: isProfile });
      const lines = new THREE.LineSegments(geo, lineMat);
      if (isProfile) lines.renderOrder = 4;
      this.extrasGroup.add(lines);
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
    if (this.selector.visible) {
      this.selector.update(this.cam.quat, this.size.w, this.size.h);
      this.selector.render(this.renderer);
    }
    this.drawOverlay();
  }

  private drawOverlay() {
    const ctx = this.ctx;
    const dpr = window.devicePixelRatio;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, this.size.w, this.size.h);
    if (this.editor && this.app.sketch) this.editor.draw(ctx);
    this.drawFeatureOverlay(ctx);
    this.drawShownDims(ctx);
    // Measure: the minimum-distance line between the two measured entities.
    const m = this.app.measureOpen ? this.app.measureResult : null;
    if (m) {
      const a = this.cam.project(new THREE.Vector3(...m.p1)), b = this.cam.project(new THREE.Vector3(...m.p2));
      ctx.strokeStyle = "#c000c0";
      ctx.fillStyle = "#c000c0";
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
      ctx.stroke();
      for (const q of [a, b]) {
        ctx.beginPath();
        ctx.arc(q[0], q[1], 3, 0, Math.PI * 2);
        ctx.fill();
      }
      drawTag(ctx, (a[0] + b[0]) / 2 + 6, (a[1] + b[1]) / 2 - 6, `${formatLength(m.distance, this.app.doc.units, false, 3, true)}`);
    }
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
    // Plane resize handles.
    for (const h of this.planeHandles) {
      const [x, y] = this.cam.project(new THREE.Vector3(...h.world));
      ctx.beginPath();
      ctx.arc(x, y, 4, 0, Math.PI * 2);
      ctx.fillStyle = "#3d8be8";
      ctx.fill();
      ctx.lineWidth = 1;
      ctx.strokeStyle = "#ffffff";
      ctx.stroke();
    }
    // Entity name tooltip at the pointer (planes, sketches), as SOLIDWORKS does on hover.
    const hov = this.app.hover;
    if (hov && this.pointer && !this.nav && !this.selector.visible && (hov.kind === "plane" || hov.kind === "feature") && !this.app.sketch) {
      const std: Record<string, string> = { front: "Front Plane", top: "Top Plane", right: "Right Plane" };
      const name = hov.kind === "plane" ? std[hov.id] ?? this.app.featureById(hov.id)?.name : this.app.featureById(hov.id)?.name;
      if (name) drawTag(ctx, this.pointer.x + 12, this.pointer.y + 18, name);
    }
    // Current standard view name, bottom-left ("*Isometric").
    const showTriad = this.app.opt("display.triad") !== false;
    if (this.cam.viewName && showTriad) {
      ctx.font = "italic 12px Segoe UI, sans-serif";
      ctx.fillStyle = "#222";
      ctx.textAlign = "left";
      ctx.textBaseline = "alphabetic";
      ctx.fillText(`*${this.cam.viewName}`, 14, this.size.h - 8);
    }
    this.drawOrigin(ctx);
    if (showTriad) this.drawTriad(ctx);
  }

  /** Reference axes (blue dash-dot lines), points (blue stars) and coordinate systems (triads). */
  private buildRefGeometry() {
    const app = this.app;
    const r = (app.pm && "feature" in app.pm && app.previewResult) || app.result;
    if (!r) return;
    const C = app.options.colors;
    const box = this.modelBox();
    const L = Math.max(10, box.getSize(new THREE.Vector3()).length() * 0.6);
    const wpp = this.cam.worldPerPixel;
    const colorOf = (id: string) => (app.isSelected({ kind: "feature", id }) ? C.selected : app.hover?.kind === "feature" && app.hover.id === id ? C.hover : "#2a62c9");
    const addPick = (id: string, obj: THREE.Object3D, geo: THREE.BufferGeometry) => {
      const idx = this.refPickIds.push(id) - 1;
      const col = encode(T_REF, idx);
      const n = geo.getAttribute("position").count;
      const cols = new Float32Array(n * 3);
      for (let i = 0; i < n; i++) cols.set(col, i * 3);
      const pg = geo.clone();
      pg.setAttribute("color", new THREE.BufferAttribute(cols, 3));
      this.pickExtras.add(obj instanceof THREE.Points ? new THREE.Points(pg, new THREE.PointsMaterial({ size: 11, sizeAttenuation: false, vertexColors: true })) : new THREE.LineSegments(pg, new THREE.LineBasicMaterial({ vertexColors: true })));
    };
    const hidden = new Set(app.doc.display?.hiddenPlanes ?? []);
    const pmFeature = app.pm && "feature" in app.pm ? app.pm.feature : null;
    const shown = (id: string) => {
      const f = app.featureById(id) ?? (pmFeature?.id === id ? pmFeature : undefined);
      return !!f && !f.suppressed && (!hidden.has(id) || app.isSelected({ kind: "feature", id }));
    };
    for (const [id, a] of Object.entries(r.axes ?? {})) {
      if (!shown(id)) continue;
      const p0 = new THREE.Vector3(...a.p).addScaledVector(new THREE.Vector3(...a.d), -L / 2);
      const p1 = new THREE.Vector3(...a.p).addScaledVector(new THREE.Vector3(...a.d), L / 2);
      const geo = new THREE.BufferGeometry().setFromPoints([p0, p1]);
      const line = new THREE.LineSegments(geo, new THREE.LineDashedMaterial({ color: colorOf(id), dashSize: wpp * 14, gapSize: wpp * 5 }));
      line.computeLineDistances();
      this.extrasGroup.add(line);
      addPick(id, line, geo);
      this.refLabels.push({ id, at: p1 });
    }
    for (const [id, p] of Object.entries(r.points ?? {})) {
      if (!shown(id)) continue;
      const geo = new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(...p)]);
      const pts = new THREE.Points(geo, new THREE.PointsMaterial({ color: colorOf(id), size: 7, sizeAttenuation: false, depthTest: false }));
      pts.renderOrder = 9;
      this.extrasGroup.add(pts);
      addPick(id, pts, geo);
    }
    for (const [id, f] of Object.entries(r.csys ?? {})) {
      if (!shown(id)) continue;
      const o = new THREE.Vector3(...f.origin);
      const s = wpp * 40;
      for (const [d, c] of [[f.x, "#d01818"], [f.y, "#18a018"], [f.normal, "#1830d0"]] as const) {
        const geo = new THREE.BufferGeometry().setFromPoints([o, o.clone().addScaledVector(new THREE.Vector3(...d), s)]);
        const line = new THREE.LineSegments(geo, new THREE.LineBasicMaterial({ color: app.isSelected({ kind: "feature", id }) ? C.selected : c, depthTest: false }));
        line.renderOrder = 9;
        this.extrasGroup.add(line);
        addPick(id, line, geo);
      }
    }
  }

  /** Sketch dimension renderer for a sketch feature (re-created when its plane moves). */
  private sketchDimEditor(id: string): SketchEditor | null {
    const frame = this.app.result?.frames[id];
    const f = this.app.featureById(id);
    if (!frame || f?.type !== "sketch") return null;
    const have = this.dimEditors.get(id);
    if (have && have.frame === frame) return have.ed;
    const ed = new SketchEditor(
      this.app,
      { toSketch: (sx, sy) => this.planePoint(frame, sx, sy), toScreen: (p) => this.cam.project(new THREE.Vector3(...frameToWorld(frame, p))), pxSize: () => this.cam.worldPerPixel },
      () => (this.app.featureById(id) as SketchFeature).sketch,
    );
    this.dimEditors.set(id, { ed, frame });
    return ed;
  }

  /** Dimensions of double-clicked features: their sketch dimensions and feature parameters. */
  private drawShownDims(ctx: CanvasRenderingContext2D) {
    this.featureDimBoxes = [];
    const app = this.app;
    if (app.sketch || !app.shownDims.length) return;
    const u = app.doc.units;
    const prec = Number(app.docProp("dim.precision") ?? 2);
    for (const fid of app.shownDims) {
      const f = app.featureById(fid);
      if (!f) continue;
      for (const sid of f.type === "sketch" ? [f.id] : sketchesOf(f)) this.sketchDimEditor(sid)?.drawDimensions(ctx);
      const params = featureParams(f);
      params.forEach((p, i) => {
        const text = p.angle ? `${+((p.value * 180) / Math.PI).toFixed(prec)}°` : formatLength(p.value, u, false, prec, app.docProp<string>("dim.zeros") === "show");
        let tx: number, ty: number;
        if (f.type === "extrude" && p.key === "end1.depth") {
          // Linear dimension along the extrusion, beside the profile.
          const frame = app.result?.frames[f.sketchId];
          const sk = app.featureById(f.sketchId);
          if (!frame || sk?.type !== "sketch") return;
          const pts = Object.values(sk.sketch.entities).filter((e) => e.type !== "point" && !(e as { construction?: boolean }).construction).flatMap((e) => sampleEntity(sk.sketch, e, 16));
          if (!pts.length) return;
          const corner = pts.reduce((m, q) => (q[0] + q[1] < m[0] + m[1] ? q : m));
          const sign = (f.reverse ? -1 : 1) * (f.cut ? -1 : 1);
          const dir = new THREE.Vector3(...frame.normal).multiplyScalar(sign);
          const A = new THREE.Vector3(...frameToWorld(frame, corner));
          if (f.end1.type === "midPlane") A.addScaledVector(dir, -p.value / 2);
          const B = A.clone().addScaledVector(dir, p.value);
          let a = this.cam.project(A), b = this.cam.project(B);
          const len = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
          const nx = -(b[1] - a[1]) / len, ny = (b[0] - a[0]) / len;
          const off = 26;
          const a2: [number, number] = [a[0] - nx * off, a[1] - ny * off], b2: [number, number] = [b[0] - nx * off, b[1] - ny * off];
          ctx.strokeStyle = ctx.fillStyle = app.options.colors.dimension;
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(a[0], a[1]);
          ctx.lineTo(a2[0] - nx * 4, a2[1] - ny * 4);
          ctx.moveTo(b[0], b[1]);
          ctx.lineTo(b2[0] - nx * 4, b2[1] - ny * 4);
          ctx.moveTo(a2[0], a2[1]);
          ctx.lineTo(b2[0], b2[1]);
          ctx.stroke();
          const ux = (b2[0] - a2[0]) / len, uy = (b2[1] - a2[1]) / len;
          arrowHead(ctx, a2, [-ux, -uy], 8);
          arrowHead(ctx, b2, [ux, uy], 8);
          a = a2;
          b = b2;
          tx = (a[0] + b[0]) / 2;
          ty = (a[1] + b[1]) / 2;
        } else {
          // Other feature dimensions: a callout near the model.
          const box = this.bodyBox();
          const anchor = box ? box.max.clone() : new THREE.Vector3();
          const [x, y] = this.cam.project(anchor);
          tx = x + 30;
          ty = y - 20 + i * 22;
        }
        const w = text.length * 7 + 8;
        const bx: [number, number, number, number] = [tx - w / 2, ty - 9, tx + w / 2, ty + 9];
        ctx.fillStyle = "rgba(255,255,255,0.9)";
        ctx.fillRect(bx[0], bx[1], w, 18);
        ctx.fillStyle = app.options.colors.dimension;
        ctx.font = "13px 'Century Gothic', 'Segoe UI', sans-serif";
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(text, tx, ty + 1);
        this.featureDimBoxes.push({ featureId: f.id, key: p.key, box: bx });
      });
    }
  }

  /** Double-click on a shown dimension: open Modify for it. Returns true if one was hit. */
  private dimDoubleClick(sx: number, sy: number, cx: number, cy: number): boolean {
    const inside = (b: [number, number, number, number] | null) => !!b && sx >= b[0] - 3 && sx <= b[2] + 3 && sy >= b[1] - 3 && sy <= b[3] + 3;
    for (const fd of this.featureDimBoxes) {
      if (inside(fd.box)) {
        this.app.openModifyTarget({ kind: "feature", featureId: fd.featureId, key: fd.key }, cx - 10, cy + 14);
        return true;
      }
    }
    for (const fid of this.app.shownDims) {
      const f = this.app.featureById(fid);
      if (!f) continue;
      for (const sid of f.type === "sketch" ? [f.id] : sketchesOf(f)) {
        const ed = this.sketchDimEditor(sid);
        const sk = this.app.featureById(sid);
        if (!ed || sk?.type !== "sketch") continue;
        for (const d of Object.values(sk.sketch.dimensions)) {
          if (d.driven) continue;
          if (inside(ed.dimLabelBox(d))) {
            this.app.openModifyTarget({ kind: "sketchDim", sketchId: sid, dimId: d.id }, cx - 10, cy + 14);
            return true;
          }
        }
      }
    }
    return false;
  }

  /** Sketch used by the open extrude/revolve PropertyManager. */
  private pmSketchId(): string | null {
    const pm = this.app.pm;
    return pm && (pm.kind === "extrude" || pm.kind === "revolve") && pm.feature.sketchId ? pm.feature.sketchId : null;
  }

  /** Profile sketch dimensions and the extrude depth arrow while a feature previews. */
  private drawFeatureOverlay(ctx: CanvasRenderingContext2D) {
    const hp = this.app.pm;
    if (hp?.kind === "hole" && hp.feature.face) {
      const fr = frameForPlane(hp.feature.face.center, hp.feature.face.normal);
      ctx.strokeStyle = "#d000d0";
      ctx.lineWidth = 1.5;
      for (const q of hp.feature.positions ?? []) {
        const [x, y] = this.cam.project(new THREE.Vector3(...frameToWorld(fr, q)));
        ctx.beginPath();
        ctx.moveTo(x - 6, y);
        ctx.lineTo(x + 6, y);
        ctx.moveTo(x, y - 6);
        ctx.lineTo(x, y + 6);
        ctx.stroke();
      }
    }
    const id = this.pmSketchId();
    const f = id ? this.app.featureById(id) : null;
    const frame = id ? this.app.result?.frames[id] : null;
    if (!id || !f || f.type !== "sketch" || !frame) {
      this.pmSketch = null;
      return;
    }
    if (this.pmSketch?.id !== id) {
      const ed = new SketchEditor(
        this.app,
        {
          toSketch: (sx, sy) => this.planePoint(frame, sx, sy),
          toScreen: (p) => this.cam.project(new THREE.Vector3(...frameToWorld(this.app.result?.frames[id] ?? frame, p))),
          pxSize: () => this.cam.worldPerPixel,
        },
        () => (this.app.featureById(id) as SketchFeature).sketch,
      );
      this.pmSketch = { id, ed };
    }
    if (this.app.opt("view.sketchDims") !== false) this.pmSketch.ed.drawDimensions(ctx);
    // Selected contours are shaded in the selection colour.
    const pm = this.app.pm;
    if ((pm?.kind === "extrude" || pm?.kind === "revolve") && pm.feature.contours?.length) {
      const ed = this.pmSketch.ed;
      const faces = ed.faces();
      const S = (q: Vec2) => this.cam.project(new THREE.Vector3(...frameToWorld(frame, q)));
      ctx.save();
      ctx.globalAlpha = 0.35;
      ctx.fillStyle = this.app.options.colors.selected;
      for (const q of pm.feature.contours) {
        const f0 = smallestFace(faces, q);
        if (!f0) continue;
        ctx.beginPath();
        for (const l of [f0, ...ed.regionHoles(f0)]) {
          l.poly.forEach((pt, i) => {
            const [x, y] = S(pt);
            if (i) ctx.lineTo(x, y);
            else ctx.moveTo(x, y);
          });
          ctx.closePath();
        }
        ctx.fill("evenodd");
      }
      ctx.restore();
    }
    const h = this.depthHandle();
    if (!h) return;
    const a = this.cam.project(h.base), b = this.cam.project(h.tip);
    const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
    if (len < 4) return;
    const ux = (b[0] - a[0]) / len, uy = (b[1] - a[1]) / len;
    const hot = this.depthHover || !!this.depthDrag;
    ctx.strokeStyle = hot ? this.app.options.colors.hover : "#7d7d7d";
    ctx.fillStyle = ctx.strokeStyle;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(a[0], a[1]);
    ctx.lineTo(b[0] - ux * 12, b[1] - uy * 12);
    ctx.stroke();
    // Cone head.
    ctx.beginPath();
    ctx.moveTo(b[0] + ux * 4, b[1] + uy * 4);
    ctx.lineTo(b[0] - ux * 14 - uy * 6, b[1] - uy * 14 + ux * 6);
    ctx.lineTo(b[0] - ux * 14 + uy * 6, b[1] - uy * 14 - ux * 6);
    ctx.closePath();
    ctx.fill();
    ctx.lineWidth = 1;
    ctx.strokeStyle = "#ffffff";
    ctx.stroke();
  }

  /** Extrude direction arrow: from the profile centre to the end of direction 1. */
  private depthHandle(): { base: THREE.Vector3; tip: THREE.Vector3; dir: THREE.Vector3; mid: boolean } | null {
    const pm = this.app.pm;
    if (!pm || pm.kind !== "extrude") return null;
    const fe: ExtrudeFeature = pm.feature;
    if (fe.end1.type !== "blind" && fe.end1.type !== "midPlane") return null;
    const sk = this.app.featureById(fe.sketchId);
    const frame = this.app.result?.frames[fe.sketchId];
    if (!sk || sk.type !== "sketch" || !frame) return null;
    const pts = Object.values(sk.sketch.entities)
      .filter((e) => e.type !== "point" && !(e as { construction?: boolean }).construction)
      .flatMap((e) => sampleEntity(sk.sketch, e, 16));
    if (!pts.length) return null;
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const [x, y] of pts) {
      x0 = Math.min(x0, x);
      y0 = Math.min(y0, y);
      x1 = Math.max(x1, x);
      y1 = Math.max(y1, y);
    }
    const base = new THREE.Vector3(...frameToWorld(frame, [(x0 + x1) / 2, (y0 + y1) / 2]));
    const sign = (fe.reverse ? -1 : 1) * (fe.cut ? -1 : 1);
    const dir = new THREE.Vector3(...frame.normal).multiplyScalar(sign);
    const mid = fe.end1.type === "midPlane";
    const tip = base.clone().addScaledVector(dir, mid ? fe.end1.depth / 2 : fe.end1.depth);
    return { base, tip, dir, mid };
  }

  /** Point on a sketch plane under the pointer, in that plane's 2D coordinates. */
  private planePoint(f: Frame, sx: number, sy: number): Vec2 | null {
    const ray = this.cam.ray(sx, sy);
    const n = new THREE.Vector3(...f.normal);
    const denom = n.dot(ray.direction);
    if (Math.abs(denom) < 1e-9) return null;
    const t = new THREE.Vector3(...f.origin).sub(ray.origin).dot(n) / denom;
    const p = ray.origin.clone().addScaledVector(ray.direction, t);
    return worldToFrame(f, [p.x, p.y, p.z]);
  }

  /** Distance along the depth arrow's axis closest to the pointer ray. */
  private axisParam(base: THREE.Vector3, dir: THREE.Vector3, sx: number, sy: number): number | null {
    const ray = this.cam.ray(sx, sy);
    const w0 = base.clone().sub(ray.origin);
    const a = dir.dot(dir), b = dir.dot(ray.direction), c = ray.direction.dot(ray.direction);
    const d = dir.dot(w0), e = ray.direction.dot(w0);
    const den = a * c - b * b;
    if (Math.abs(den) < 1e-9) return null;
    return (b * e - c * d) / den;
  }

  private overDepthHandle(sx: number, sy: number) {
    const h = this.depthHandle();
    if (!h) return false;
    const [x, y] = this.cam.project(h.tip);
    return Math.hypot(x - sx, y - sy) <= 10;
  }

  /** Part origin: blue axes at the model origin (View > Hide/Show > Origins), orange on hover. */
  private drawOrigin(ctx: CanvasRenderingContext2D) {
    const app = this.app;
    if (app.sketch) return; // the sketch draws its own (red) origin
    const hov = app.hover?.kind === "origin";
    const sel = app.isSelected({ kind: "origin" });
    if (!(hov || sel || app.opt("view.origins") !== false)) return;
    const C = app.options.colors;
    const color = sel ? C.selected : hov ? C.hover : "#1c3fd6";
    const o = this.cam.project(new THREE.Vector3(0, 0, 0));
    const wpp = this.cam.worldPerPixel;
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.lineWidth = hov || sel ? 2 : 1.4;
    for (const axis of [new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 1, 0), new THREE.Vector3(0, 0, 1)]) {
      const t = this.cam.project(axis.multiplyScalar(wpp * 24));
      const dx = t[0] - o[0], dy = t[1] - o[1];
      const len = Math.hypot(dx, dy);
      if (len < 3) continue;
      ctx.beginPath();
      ctx.moveTo(o[0], o[1]);
      ctx.lineTo(t[0], t[1]);
      ctx.stroke();
      const ux = dx / len, uy = dy / len;
      ctx.beginPath();
      ctx.moveTo(t[0] + ux * 5, t[1] + uy * 5);
      ctx.lineTo(t[0] - uy * 2.6, t[1] + ux * 2.6);
      ctx.lineTo(t[0] + uy * 2.6, t[1] - ux * 2.6);
      ctx.fill();
    }
    ctx.beginPath();
    ctx.arc(o[0], o[1], 2.2, 0, Math.PI * 2);
    ctx.fill();
  }

  /** Reference triad, bottom-left (red X, green Y, blue Z). */
  private drawTriad(ctx: CanvasRenderingContext2D) {
    const cx = 46, cy = this.size.h - 52, L = 30;
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
    // The origin is pickable whenever it is shown (PropertyManagers filter it if they don't take it).
    if (!this.app.sketch && this.app.opt("view.origins") !== false) {
      const o = this.cam.project(new THREE.Vector3(0, 0, 0));
      if (Math.hypot(o[0] - sx, o[1] - sy) <= 5) return { kind: "origin" };
    }
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
    // Selection filters: only the filtered entity types can be picked (with a larger window).
    const f = this.app.filterOn && this.app.selFilter.size ? this.app.selFilter : null;
    const ok = (k: "face" | "edge" | "vertex" | "plane" | "sketch" | "ref") => !f || f.has(k);
    const w = f ? 2 : 1;
    if (ok("vertex") && best[T_VERTEX] && best[T_VERTEX].d <= 5 * w && !wantEdgesOnly) return { kind: "vertex", id: best[T_VERTEX].idx };
    if (ok("edge") && best[T_EDGE] && best[T_EDGE].d <= 4 * w) return { kind: "edge", id: best[T_EDGE].idx };
    if (ok("sketch") && best[T_SKETCH] && best[T_SKETCH].d <= 4 * w) return { kind: "feature", id: this.sketchPickIds[best[T_SKETCH].idx] };
    if (ok("face") && best[T_FACE] && best[T_FACE].d <= 1.5 * w && !wantEdgesOnly) return { kind: "face", id: best[T_FACE].idx };
    if (ok("ref") && best[T_REF] && best[T_REF].d <= 4 * w) return { kind: "feature", id: this.refPickIds[best[T_REF].idx] };
    if (ok("plane") && best[T_PLANE] && best[T_PLANE].d <= 1.5 * w) return { kind: "plane", id: this.planePickIds[best[T_PLANE].idx] };
    return null;
  }

  /** Last right-click position (Select Other works there). */
  lastContext: { sx: number; sy: number; cx: number; cy: number } | null = null;

  /**
   * Select Other: every face along the ray under the last right-click (front to back,
   * including hidden ones), plus the edge/vertex picked there.
   */
  selectOtherCandidates(): SelItem[] {
    const c = this.lastContext;
    const mesh = this.currentMesh;
    if (!c || !mesh || !this.bodyMesh) return [];
    const out: SelItem[] = [];
    const top = this.pick(c.sx, c.sy);
    if (top && top.kind !== "face") out.push(top);
    const rc = new THREE.Raycaster();
    rc.ray.copy(this.cam.ray(c.sx, c.sy));
    const seen = new Set<number>();
    for (const hit of rc.intersectObject(this.bodyMesh, false)) {
      const tri = (hit.faceIndex ?? 0) * 3;
      const g = mesh.faceGroups.find((x) => tri >= x.start && tri < x.start + x.count);
      if (g && g.faceId >= 0 && !seen.has(g.faceId)) {
        seen.add(g.faceId);
        out.push({ kind: "face", id: g.faceId });
      }
    }
    return out;
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

  toScreen(p: Vec2): [number, number] {
    const s = this.app.sketch!;
    const w = frameToWorld(s.frame, p);
    return this.cam.project(new THREE.Vector3(...w));
  }

  // ================================================================ view commands

  /** Bounding box of the solid only (planes size themselves to this). */
  bodyBox(): THREE.Box3 | null {
    const mesh = this.app.result?.mesh;
    if (!mesh?.bbox) return null;
    return new THREE.Box3(new THREE.Vector3(...mesh.bbox[0]), new THREE.Vector3(...mesh.bbox[1]));
  }

  modelBox(): THREE.Box3 {
    const b = this.contentBox();
    if (b.isEmpty() || b.getSize(new THREE.Vector3()).length() < 1e-6) {
      b.makeEmpty();
      b.expandByPoint(new THREE.Vector3(-50, -50, -50));
      b.expandByPoint(new THREE.Vector3(50, 50, 50));
    }
    return b;
  }

  /** Everything shown: the body, a feature preview, sketches and the open sketch's dimensions. */
  contentBox(): THREE.Box3 {
    const b = new THREE.Box3();
    const pm = this.app.pm && "feature" in this.app.pm ? this.app.previewResult : null;
    for (const mesh of [this.app.result?.mesh, pm?.mesh, pm?.toolMesh]) {
      if (!mesh?.bbox) continue;
      b.expandByPoint(new THREE.Vector3(...mesh.bbox[0]));
      b.expandByPoint(new THREE.Vector3(...mesh.bbox[1]));
    }
    const frames = this.app.result?.frames ?? {};
    for (const f of this.app.activeFeatures()) {
      if (f.type !== "sketch" || !frames[f.id]) continue;
      for (const p of Object.values(f.sketch.points)) b.expandByPoint(new THREE.Vector3(...frameToWorld(frames[f.id], [p.x, p.y])));
    }
    if (this.app.sketch) {
      const { data, frame } = this.app.sketch;
      for (const p of Object.values(data.points)) b.expandByPoint(new THREE.Vector3(...frameToWorld(frame, [p.x, p.y])));
      // Dimension text is part of what Zoom to Fit shows.
      for (const d of Object.values(data.dimensions)) b.expandByPoint(new THREE.Vector3(...frameToWorld(frame, d.label)));
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
    this.cam.setView(name, true, this.anim(), this.app.opt<boolean>("view.zoomToFitOnStandard") ? this.modelBox() : undefined);
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
    // Ctrl+8 pressed again while already normal flips to the other side (explicit command only).
    this.cam.normalTo(frame, this.anim(), this.modelBox(), true);
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
    // Focus is set explicitly on pointerdown; the default mousedown focus change would take
    // it back from inputs opened by the click (the Smart Dimension Modify box).
    el.addEventListener("mousedown", (e) => e.preventDefault());
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
    this.downClient = { x: e.clientX, y: e.clientY };
    if (this.viewMode === "zoomArea") {
      // Zoom to Area: drag a box; the view zooms to it on release.
      this.box = { x0: p.sx, y0: p.sy, x1: p.sx, y1: p.sy };
      this.zoomBox = true;
      return;
    }
    if (this.viewMode) {
      this.nav = { mode: this.viewMode, x: p.sx, y: p.sy, pivot: this.modelBox().getCenter(new THREE.Vector3()), moved: false };
      this.navLeft = true;
      this.cam.pushUndo();
      return;
    }
    if (this.selector.visible) {
      const f = this.selector.pick(p.sx, p.sy, this.size.w, this.size.h);
      if (f) {
        const zoom = this.app.opt<boolean>("view.zoomToFitOnStandard");
        this.cam.lookFrom(f.normal.clone(), ViewSelector.upFor(f.normal), this.app.transition("view.selectorTransition"), zoom ? this.modelBox() : undefined, f.name);
        this.selector.visible = false;
        this.selector.hover = null;
        this.onSelectorPick?.();
        this.invalidate();
        return;
      }
    }
    if (!this.app.sketch) {
      for (const h of this.planeHandles) {
        const [x, y] = this.cam.project(new THREE.Vector3(...h.world));
        if (Math.hypot(x - p.sx, y - p.sy) <= 7) {
          this.app.checkpoint("Resize Plane");
          this.handleDrag = { id: h.id, idx: h.idx, frame: h.frame, ext: [...h.ext] as Extents };
          return;
        }
      }
    }
    if (!this.app.sketch && this.overDepthHandle(p.sx, p.sy)) {
      const h = this.depthHandle()!;
      this.depthDrag = { base: h.base, dir: h.dir, mid: h.mid };
      return;
    }
    if (this.editor && this.app.sketch && this.app.tool === "convert") {
      // Convert Entities picks model edges and faces.
      const hit = this.pick(p.sx, p.sy);
      if (hit?.kind === "edge" || hit?.kind === "face") this.app.select(hit, "toggle");
      this.invalidate();
      return;
    }
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
    this.pointer = { x: p.sx, y: p.sy };
    if (this.zoomBox && this.box) {
      this.box.x1 = p.sx;
      this.box.y1 = p.sy;
      this.invalidate();
      return;
    }
    if (this.handleDrag) {
      this.dragPlaneHandle(p);
      return;
    }
    if (this.depthDrag) {
      const d = this.depthDrag;
      const t = this.axisParam(d.base, d.dir, p.sx, p.sy);
      const pm = this.app.pm;
      if (t !== null && pm?.kind === "extrude") {
        // Drag snaps to 0.1 mm like the SOLIDWORKS ruler at typical zoom.
        const depth = Math.max(0.1, Math.round((d.mid ? 2 * t : t) * 10) / 10);
        if (depth !== pm.feature.end1.depth) {
          pm.feature.end1.depth = depth;
          this.app.schedulePreview();
          this.app.changed();
        }
      }
      return;
    }
    if (!this.app.sketch && e.buttons === 0) {
      const over = this.overDepthHandle(p.sx, p.sy);
      if (over !== this.depthHover) {
        this.depthHover = over;
        this.overlay.style.cursor = over ? "grab" : "";
        this.invalidate();
      }
    }
    if (this.selector.visible && e.buttons === 0) {
      const f = this.selector.pick(p.sx, p.sy, this.size.w, this.size.h);
      if (this.selector.setHover(f)) this.invalidate();
      if (f) {
        this.app.setHover(null);
        return;
      }
    }
    if (this.nav) {
      const n = this.nav;
      const dx = p.sx - n.x, dy = p.sy - n.y;
      if (Math.abs(dx) + Math.abs(dy) > 0) n.moved = true;
      n.x = p.sx;
      n.y = p.sy;
      if (n.mode === "rotate") {
        if (n.axis) this.cam.rotateAxis(n.axis, THREE.MathUtils.degToRad(dx * 0.4), n.pivot);
        else this.cam.rotate(dx, dy, n.pivot, 0.1 + 0.03 * Number(this.app.opt("view.mouseSpeed") ?? 10));
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
      if (this.app.tool === "convert" && e.buttons === 0) {
        const hit = this.pick(p.sx, p.sy);
        this.app.setHover(hit?.kind === "edge" || hit?.kind === "face" ? hit : null);
      }
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
    if (this.zoomBox && e.button === 0) {
      const b = this.box;
      this.zoomBox = false;
      this.box = null;
      if (b && Math.abs(b.x1 - b.x0) > 4 && Math.abs(b.y1 - b.y0) > 4) {
        const cx = (b.x0 + b.x1) / 2, cy = (b.y0 + b.y1) / 2;
        this.cam.pushUndo();
        this.cam.pan(this.size.w / 2 - cx, this.size.h / 2 - cy);
        const f = Math.min(this.size.w / Math.abs(b.x1 - b.x0), this.size.h / Math.abs(b.y1 - b.y0));
        this.cam.zoomAt(f, this.size.w / 2, this.size.h / 2);
      }
      this.invalidate();
      return;
    }
    if (this.depthDrag && e.button === 0) {
      this.depthDrag = null;
      this.invalidate();
      return;
    }
    if (this.handleDrag && e.button === 0) {
      this.handleDrag = null;
      this.app.dirty = true;
      this.app.changed();
      return;
    }
    if (e.button === 0 && this.nav && this.navLeft) {
      this.nav = null;
      this.navLeft = false;
      return;
    }
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
        let item: SelItem | null = null;
        if (this.app.sketch && this.editor) {
          // Right-clicking a sketch entity selects it (unless a tool is running) and shows its menu.
          const h = this.app.tool ? null : this.editor.resolveHit(this.editor.hitTest(p.sx, p.sy));
          if (h) {
            item = h.kind === "point" ? { kind: "skPoint", id: h.id } : h.kind === "dim" ? { kind: "skDim", id: h.id } : { kind: "skEntity", id: h.id };
            if (!this.app.isSelected(item)) this.app.select(item);
          }
        } else item = this.filterPick(this.pick(p.sx, p.sy));
        this.lastContext = { sx: p.sx, sy: p.sy, cx: e.clientX, cy: e.clientY };
        this.onContextMenu?.(e.clientX, e.clientY, item);
      } else if (g.dir !== null) this.runGesture(g.dir);
      return;
    }
    if (e.button !== 0) return;
    const clicked = this.downClient && Math.hypot(e.clientX - this.downClient.x, e.clientY - this.downClient.y) < 4;
    if (this.editor && this.app.sketch) {
      this.editor.onUp(p);
      this.invalidate();
      if (clicked && !this.app.tool && this.app.selection.length) this.onSelectionClick?.(e.clientX, e.clientY);
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
    // Hole Wizard positions: clicks on the placement face add hole centres.
    const hpm = this.app.pm;
    if (hpm?.kind === "hole" && !hpm.editing && !hpm.feature.face) {
      // The first click picks the face and places the first hole.
      const hit = this.pick(p.sx, p.sy);
      if (hit?.kind === "face") this.app.pmPick(hit);
    }
    if (hpm?.kind === "hole" && !hpm.editing && hpm.feature.face) {
      const P = this.hitPoint(p.sx, p.sy);
      const face = hpm.feature.face;
      if (P && Math.abs(new THREE.Vector3(...face.normal).dot(P.clone().sub(new THREE.Vector3(...face.center)))) < 1e-3 * Math.max(1, this.cam.worldPerPixel * 100)) {
        this.app.addHolePosition(worldToFrame(frameForPlane(face.center, face.normal), [P.x, P.y, P.z]));
        this.invalidate();
        return;
      }
    }
    // Extrude "Selected Contours": clicks pick regions of the profile sketch.
    if ((this.app.pm?.kind === "extrude" || this.app.pm?.kind === "revolve") && this.app.pmField === "contours" && this.pmSketch) {
      const ed = this.pmSketch.ed;
      const frame = this.app.result?.frames[this.pmSketch.id];
      const q = frame ? this.planePoint(frame, p.sx, p.sy) : null;
      if (q) this.app.toggleContour(q, (pt) => smallestFace(ed.faces(), pt));
      this.invalidate();
      return;
    }
    const hit = this.filterPick(this.pick(p.sx, p.sy));
    const mode = e.ctrlKey ? "toggle" : e.shiftKey ? "add" : this.selectionCollectingPM() ? "toggle" : "replace";
    if (!hit) {
      if (mode === "replace") this.app.select(null);
      return;
    }
    this.app.select(hit, mode);
    if (this.app.pendingSketch && hit.kind === "plane") this.app.startSketch();
    else if (this.app.pendingSketch && hit.kind === "face" && this.currentMesh?.faces[hit.id]?.planar) this.app.startSketch();
    else if (clicked && !this.app.pm && this.app.selection.length) this.onSelectionClick?.(e.clientX, e.clientY);
  };

  private onDblClick = (e: MouseEvent) => {
    const p = this.info(e);
    if (this.editor && this.app.sketch) {
      this.editor.onDblClick(p);
      this.invalidate();
      return;
    }
    if (this.app.pm) return;
    // Double-click a shown dimension to modify it; a feature (face, sketch) to show its dimensions.
    if (this.dimDoubleClick(p.sx, p.sy, e.clientX, e.clientY)) return;
    const hit = this.pick(p.sx, p.sy);
    const owner = hit?.kind === "face" ? this.currentMesh?.faces[hit.id]?.owner : hit?.kind === "feature" ? hit.id : null;
    if (owner && this.app.featureById(owner)) this.app.showFeatureDims(owner);
    this.invalidate();
  };

  /** Resize a plane by dragging one of its 8 handles. */
  private dragPlaneHandle(p: PointerInfo) {
    const d = this.handleDrag!;
    const f = d.frame;
    const ray = this.cam.ray(p.sx, p.sy);
    const n = new THREE.Vector3(...f.normal);
    const denom = n.dot(ray.direction);
    if (Math.abs(denom) < 1e-6) return;
    const t = new THREE.Vector3(...f.origin).sub(ray.origin).dot(n) / denom;
    const hit = ray.origin.clone().addScaledVector(ray.direction, t).sub(new THREE.Vector3(...f.origin));
    const u = hit.dot(new THREE.Vector3(...f.x)), v = hit.dot(new THREE.Vector3(...f.y));
    const e = d.ext;
    const minSpan = 2;
    // corners 0..3: (u0,v0) (u1,v0) (u1,v1) (u0,v1); mids 4..7: v0, u1, v1, u0
    const setU0 = () => (e[0] = Math.min(u, e[1] - minSpan));
    const setU1 = () => (e[1] = Math.max(u, e[0] + minSpan));
    const setV0 = () => (e[2] = Math.min(v, e[3] - minSpan));
    const setV1 = () => (e[3] = Math.max(v, e[2] + minSpan));
    const plan = [[setU0, setV0], [setU1, setV0], [setU1, setV1], [setU0, setV1], [setV0], [setU1], [setV1], [setU0]];
    plan[d.idx].forEach((fn) => fn());
    const disp = (this.app.doc.display ??= {});
    disp.planeExtents = { ...(disp.planeExtents ?? {}), [d.id]: [...e] as Extents };
    this.invalidate();
  }

  /** Part colour: Document Properties > Model Display > Shading, else the system default. */
  private modelColor() {
    return (this.app.doc.props?.["fc.shading"] as string | undefined) ?? this.app.options.colors.model;
  }

  setViewMode(m: "rotate" | "pan" | "zoom" | "roll" | "zoomArea" | null) {
    this.viewMode = this.viewMode === m ? null : m;
    this.overlay.style.cursor = this.viewMode === "pan" ? "move" : this.viewMode === "zoom" ? "ns-resize" : this.viewMode === "zoomArea" ? "zoom-in" : this.viewMode ? "grab" : "default";
    this.app.changed();
  }

  /** Zoom to Selection: fit the view to the selected faces, edges, planes or sketch entities. */
  zoomToSelection() {
    const box = new THREE.Box3();
    const app = this.app;
    const mesh = this.currentMesh;
    const add = (x: number, y: number, z: number) => box.expandByPoint(new THREE.Vector3(x, y, z));
    if (app.sketch && this.editor) {
      for (const p of this.editor.selectionPoints()) add(...frameToWorld(app.sketch.frame, p));
    }
    for (const s of app.selection) {
      if (s.kind === "face" && mesh) {
        const r = this.faceRanges[s.id];
        if (r) for (let v = r[0]; v < r[1]; v++) add(mesh.positions[v * 3], mesh.positions[v * 3 + 1], mesh.positions[v * 3 + 2]);
      } else if (s.kind === "edge" && mesh) {
        for (const g of mesh.edgeGroups) if (g.edgeId === s.id) for (let i = g.start; i < g.start + g.count; i++) add(mesh.edgePositions[i * 3], mesh.edgePositions[i * 3 + 1], mesh.edgePositions[i * 3 + 2]);
      } else if (s.kind === "vertex" && mesh) add(...mesh.vertices[s.id]);
      else if (s.kind === "plane") {
        const frame = app.frameOfPlane(s.id);
        if (frame) {
          const e = app.doc.display?.planeExtents?.[s.id] ?? autoExtents(frame, this.bodyBox());
          for (const [u, v] of [[e[0], e[2]], [e[1], e[2]], [e[1], e[3]], [e[0], e[3]]] as [number, number][]) add(...frameToWorld(frame, [u, v]));
        }
      } else if (s.kind === "feature") {
        const f = app.featureById(s.id);
        const frame = app.result?.frames[s.id];
        if (f?.type === "sketch" && frame) for (const p of Object.values(f.sketch.points)) add(...frameToWorld(frame, [p.x, p.y]));
        else if (mesh) mesh.faces.forEach((fi, i) => {
          if (fi.owner !== s.id) return;
          const r = this.faceRanges[i];
          if (r) for (let v = r[0]; v < r[1]; v++) add(mesh.positions[v * 3], mesh.positions[v * 3 + 1], mesh.positions[v * 3 + 2]);
        });
      }
    }
    if (box.isEmpty()) return this.zoomToFit();
    if (box.getSize(new THREE.Vector3()).length() < 1e-6) box.expandByScalar(10);
    this.cam.zoomToFit(box, this.anim());
    this.invalidate();
  }

  showSelector(on: boolean) {
    this.selector.visible = on;
    this.selector.hover = null;
    this.invalidate();
  }

  private selectionCollectingPM() {
    const k = this.app.pm?.kind;
    return k === "fillet" || k === "chamfer" || this.app.isFieldPM() || this.app.measureOpen;
  }

  /** Apply PropertyManager selection filters (e.g. fillet wants edges/faces). */
  private filterPick(hit: SelItem | null): SelItem | null {
    const k = this.app.pm?.kind;
    if (!hit) return null;
    if (k === "fillet" && hit.kind !== "edge" && hit.kind !== "face") return null;
    if (k === "chamfer" && hit.kind !== "edge") return null;
    if ((k === "extrude" || k === "revolve") && hit.kind === "origin") return null;
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

/** Small name tag near the pointer (hover tooltip). */
function drawTag(ctx: CanvasRenderingContext2D, x: number, y: number, text: string) {
  ctx.font = "12px Segoe UI, sans-serif";
  const w = ctx.measureText(text).width + 10;
  ctx.fillStyle = "rgba(255,255,255,0.96)";
  ctx.strokeStyle = "#9a9a9a";
  ctx.lineWidth = 1;
  ctx.fillRect(x, y, w, 18);
  ctx.strokeRect(x + 0.5, y + 0.5, w - 1, 17);
  ctx.fillStyle = "#1e1e1e";
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  ctx.fillText(text, x + 5, y + 9.5);
}

/** Id of the plane (standard or reference) the active sketch lies on, if any. */
function sketchPlaneId(app: App): string | null {
  const f = app.sketch ? app.featureById(app.sketch.featureId) : null;
  if (!f || f.type !== "sketch") return null;
  return f.plane.kind === "std" ? f.plane.plane : f.plane.kind === "refplane" ? f.plane.featureId : null;
}
