/**
 * The graphics area: three.js rendering, GPU picking, SOLIDWORKS mouse navigation,
 * selection, and the 2D overlay (active sketch, triad, selection box).
 *
 * Rendering is on demand: a frame is drawn only when something changed, so an idle
 * window costs nothing (unlike always-on render loops).
 *
 * In the native app (`src/native/view.ts`) `oc-render` draws the model and the View
 * Selector and answers picks; no three.js scene or WebGL context is made. Its picks are
 * asynchronous, so pointer events go through a queue (`dispatch`): each event waits for
 * the pick at its position, then runs the same handler as in the browser, where `pick()`
 * returns that answer. Events keep their order; consecutive moves are coalesced.
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
import { ViewSelector, type Facet } from "./viewSelector";
import { autoExtents, buildPlane, planeHandles, type Extents, type PlaneStyle } from "./planes";
import { SketchEditor, arrowHead, drawBox, type PointerInfo } from "../sketch/editor";
import { pointInPoly, sampleEntity, type Loop } from "../sketch/model";
import type { ModelSnaps } from "../sketch/inference";
import { NativeSelector, NativeView, type NativeHit, type PickOptions } from "../native/view";
import { isNative } from "../native/tauri";

/** Smallest closed area of a sketch containing a point (Selected Contours). */
function smallestFace(faces: Loop[], p: Vec2): Loop | null {
  let best: Loop | null = null;
  for (const f of faces) if ((!best || f.area < best.area) && pointInPoly(p, f.poly)) best = f;
  return best;
}

const T_FACE = 1, T_EDGE = 2, T_VERTEX = 3, T_PLANE = 4, T_SKETCH = 5, T_REF = 6;
const PICK = 13; // pick window in px

export type DisplayStyle = "shadedEdges" | "shaded" | "hlr" | "hlv" | "wireframe";

/** Pointer events queued for the native app's asynchronous picks. */
type EventKind = "down" | "move" | "up" | "leave" | "dblclick" | "editorMove";

interface GestureState {
  x: number;
  y: number;
  active: boolean;
  dir: number | null;
}

export class Viewport {
  /** three.js renderer (browser and Electron; none in the native app). */
  renderer: THREE.WebGLRenderer | null = null;
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
  /** Native assemblies: a component grabbed at `at` (world) by a left drag (G1a). */
  private compDrag: { at: THREE.Vector3; x: number; y: number; moved: boolean } | null = null;
  /** SmartMates: an Alt-drag from a face, edge or vertex of a component (G1b). */
  private smartDrag: { from: SelItem; x: number; y: number } | null = null;
  /** Insert Components: a press while the new component follows the pointer. */
  private placingDown = false;
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
  /** 3D View Selector cube (three.js, or drawn and picked by `oc-render` in the native app). */
  selector: ViewSelector | NativeSelector;
  /** Called after a view selector facet is picked (UI closes its menu). */
  onSelectorPick: (() => void) | null = null;
  /** Plane resize handles of selected planes (world position + owning plane). */
  private planeHandles: { id: string; idx: number; world: Vec3; frame: Frame; ext: Extents }[] = [];
  private handleDrag: { id: string; idx: number; frame: Frame; ext: Extents } | null = null;
  /** Last pointer position in the graphics area (for hover tooltips). */
  private pointer: { x: number; y: number } | null = null;
  /** Native app: `oc-render` draws the scene under the webview and picks (src/native/view.ts). */
  native: NativeView | null = null;
  /** Native app: events waiting for their pick, and whether the queue is being run. */
  private events: { kind: EventKind; e: MouseEvent | null; run: (e: MouseEvent) => void | Promise<void> }[] = [];
  private draining = false;
  /** Native app: the pick for the event being handled, the last one, and the last right-click's (Select Other). */
  private eventHit: NativeHit | null = null;
  private lastHit: { hit: NativeHit; cam: string; mesh: BodyMesh | null } | null = null;
  private contextHit: NativeHit | null = null;

  constructor(private host: HTMLElement, private app: App) {
    const o = app.options;
    if (isNative()) this.native = new NativeView(app, () => this.invalidate());
    else {
      this.renderer = new THREE.WebGLRenderer({ antialias: o.performance.msaa, alpha: true, powerPreference: "high-performance", preserveDrawingBuffer: false, stencil: true });
      // Section View clips with a plane; caps are drawn with the stencil buffer.
      this.renderer.localClippingEnabled = true;
      this.renderer.setPixelRatio(window.devicePixelRatio * o.performance.pixelRatio);
      this.renderer.setClearColor(0x000000, 0);
      this.renderer.toneMapping = THREE.NoToneMapping;
      host.appendChild(this.renderer.domElement);
      this.renderer.domElement.className = "gl";
    }
    this.selector = this.native ? new NativeSelector() : new ViewSelector();
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
      scaleAbout: (p, k) => {
        // Zoom by k about world point p, so geometry scaled by k about p looks unchanged.
        const c = new THREE.Vector3(...p);
        const t = this.cam.target.clone().sub(c).multiplyScalar(k).add(c);
        this.cam.animateTo(this.cam.targetQuat(), t, this.cam.height * k, 0, false);
        this.invalidate();
      },
    };

    const ro = new ResizeObserver(() => this.resize());
    ro.observe(host);
    this.resize();
    this.bindEvents();
    this.loop();
  }

  dispose() {
    cancelAnimationFrame(this.raf);
    this.renderer?.dispose();
    this.renderer?.domElement.remove();
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
    this.renderer?.setSize(this.size.w, this.size.h);
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
        modelSnaps: () => this.modelSnaps(),
        modelPick: (sx, sy) => this.filterPick(this.pick(sx, sy)),
        modelSnapAt: (sx, sy) => this.modelSnapAt(sx, sy),
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
    const disp = app.doc.display ?? {};
    const styleKey = `${this.displayStyle}|${this.modelColor()}|${app.options.colors.edges}|${app.opt("display.hiddenEdges")}|${!!disp.bodyHidden}|${!!disp.transparent}`;
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
    // Hide/Show Body: hidden bodies are neither drawn nor pickable.
    const hidden = !!app.doc.display?.bodyHidden;
    this.bodyGroup.visible = !hidden;
    this.pickBody.visible = !hidden;
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
    // The native renderer draws and picks the body from its own copy of the mesh.
    if (this.native) return;
    const C = this.app.options.colors;
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(mesh.positions, 3));
    geo.setAttribute("normal", new THREE.BufferAttribute(mesh.normals, 3));
    geo.setIndex(new THREE.BufferAttribute(mesh.indices, 1));
    const nv = mesh.positions.length / 3;
    const colors = new Float32Array(nv * 3);
    geo.setAttribute("color", new THREE.BufferAttribute(colors, 3));
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
    const disp = this.app.doc.display ?? {};
    if (disp.transparent) {
      mat.transparent = true;
      mat.opacity = 0.45;
      mat.depthWrite = false;
    }
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
    const hiddenV = new Set(mesh.hiddenVertices ?? []);
    mesh.vertices.forEach((v, i) => {
      if (hiddenV.has(i)) return;
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
    if (!mesh || this.native) return;
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
    if (this.native) return; // oc-render clips and caps
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
    if (this.native) return; // oc-render highlights from the scene's hover and selection
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
    this.native?.reset();
    this.buildRefGeometry();
    this.planeHandles = [];
    // Planes size themselves to the model (the body, or the sketches before there is one).
    const bodyBox = this.planeSizeBox();
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
      if (this.native) {
        this.native.scene.plane(p.id, p.frame, ext, this.native.label(p.name));
        if (sel) planeHandles(p.frame, ext).forEach((w, i) => this.planeHandles.push({ id: p.id, idx: i, world: w, frame: p.frame, ext }));
        continue;
      }
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
      if (this.native) {
        this.native.scene.sketch(f.id, pts, isProfile);
        continue;
      }
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
    if (mesh && vtx.length && !this.native) {
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
    if (this.native) {
      const sel = this.selector as NativeSelector;
      this.native.send({ cam: this.cam, host: this.host, style: this.displayStyle, modelColor: this.modelColor(), selector: sel.visible ? { hover: sel.hover } : null });
    } else if (this.renderer) {
      this.renderer.setRenderTarget(null);
      this.renderer.render(this.scene, this.cam.camera);
      const sel = this.selector as ViewSelector;
      if (sel.visible) {
        sel.update(this.cam.quat, this.size.w, this.size.h);
        sel.render(this.renderer);
      }
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
    // The native renderer draws the origin and the triad itself.
    if (!this.native) {
      this.drawOrigin(ctx);
      if (showTriad) this.drawTriad(ctx);
    }
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
      this.refLabels.push({ id, at: p1 });
      if (this.native) {
        this.native.scene.axis(id, a.p, a.d, L);
        continue;
      }
      const geo = new THREE.BufferGeometry().setFromPoints([p0, p1]);
      const line = new THREE.LineSegments(geo, new THREE.LineDashedMaterial({ color: colorOf(id), dashSize: wpp * 14, gapSize: wpp * 5 }));
      line.computeLineDistances();
      this.extrasGroup.add(line);
      addPick(id, line, geo);
    }
    for (const [id, p] of Object.entries(r.points ?? {})) {
      if (!shown(id)) continue;
      if (this.native) {
        this.native.scene.point(id, p);
        continue;
      }
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
      if (this.native) {
        this.native.scene.frame(id, f);
        continue;
      }
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
      id,
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

  /** Screen positions of pattern instances while the Instances to Skip box is active. */
  private skipMarkers(): { key: string; x: number; y: number; skipped: boolean }[] {
    const pm = this.app.pm;
    if (!pm || (pm.kind !== "linearPattern" && pm.kind !== "circularPattern") || this.app.pmField !== "skip") return [];
    return (this.app.previewResult?.instances ?? []).map((m) => {
      const [x, y] = this.cam.project(new THREE.Vector3(...m.p));
      return { key: m.key, x, y, skipped: m.skipped };
    });
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
    // Instances to Skip: a dot at each instance (hollow = skipped), as SOLIDWORKS shows them.
    for (const m of this.skipMarkers()) {
      ctx.beginPath();
      ctx.arc(m.x, m.y, 5, 0, 2 * Math.PI);
      ctx.lineWidth = 2;
      ctx.strokeStyle = "#d000d0";
      ctx.fillStyle = "#d000d0";
      if (m.skipped) ctx.stroke();
      else ctx.fill();
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
    if (this.native) return this.nativeAt(sx, sy)?.item ?? null;
    if (!this.renderer) return null;
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

  /**
   * Native app: the renderer's answer at (sx, sy): the event being handled's, else the
   * last one while the view and the model are unchanged (a sketch tool re-runs its hover
   * from a timer). Null when there is none: picks are only made for events.
   */
  private nativeAt(sx: number, sy: number): NativeHit | null {
    const h = this.eventHit;
    if (h && h.sx === sx && h.sy === sy) return h;
    const c = this.lastHit;
    if (c && c.hit.sx === sx && c.hit.sy === sy && c.cam === this.camKey() && c.mesh === this.currentMesh) return c.hit;
    return null;
  }

  private camKey() {
    const c = this.cam;
    return `${c.target.toArray()}|${c.quat.toArray()}|${c.height}|${c.perspective}|${this.size.w}x${this.size.h}`;
  }

  /** What `oc-render` finds under an event, with the app's selection filter. */
  private nativePick(e: MouseEvent, need: Omit<PickOptions, "x" | "y" | "filter">): Promise<NativeHit> {
    const p = this.info(e);
    const app = this.app;
    const only = app.filterOn && app.selFilter.size ? [...app.selFilter] : null;
    return this.native!.pick(this.cam, this.host, { x: p.sx, y: p.sy, filter: { only, edgesOnly: app.pm?.kind === "chamfer" }, ...need });
  }

  /**
   * Native app: the pick an event's handler will need, decided when it is about to run
   * (after the events before it): hover during plain moves and sketching, clicks, the
   * rotate-about point on a middle click, Select Other's faces on a right click. Navigation
   * drags need none.
   */
  private pickNeed(kind: EventKind, e: MouseEvent): Omit<PickOptions, "x" | "y" | "filter"> | null {
    const sketching = !!(this.editor && this.app.sketch);
    const selector = this.selector.visible;
    switch (kind) {
      case "move":
        if (this.nav || this.gesture || this.handleDrag || this.depthDrag || this.zoomBox) return null;
        if (e.buttons !== 0 && !sketching) return null;
        return { wait: false, ray: "none", selector: selector && e.buttons === 0 };
      case "editorMove":
        return sketching ? { wait: false, ray: "none", selector: false } : null;
      case "down":
        // Assemblies: the grabbed point of a component drag comes from the ray.
        return e.button === 0 && !this.viewMode ? { wait: true, ray: this.app.asm ? "first" : "none", selector } : null;
      case "up":
        if (e.button === 0) return this.zoomBox || this.depthDrag || this.handleDrag || this.navLeft || this.box ? null : { wait: true, ray: "first", selector: false };
        if (e.button === 1) return this.nav && !this.nav.moved ? { wait: true, ray: "first", selector: false } : null;
        if (e.button === 2) return this.gesture && !this.gesture.active ? { wait: true, ray: "all", selector: false } : null;
        return null;
      case "dblclick":
        return { wait: true, ray: "none", selector: false };
      default:
        return null;
    }
  }

  /**
   * Runs a pointer event's handler: at once in the browser and Electron; in the native app
   * after the events before it, with the pick it needs fetched first.
   */
  private dispatch<E extends MouseEvent>(kind: EventKind, e: E | null, run: (e: E) => void | Promise<void>) {
    if (!this.native) {
      void run(e as E);
      return;
    }
    const item = { kind, e, run: run as (e: MouseEvent) => void | Promise<void> };
    const last = this.events[this.events.length - 1];
    if (kind === "move" && last?.kind === "move") this.events[this.events.length - 1] = item;
    else this.events.push(item);
    if (!this.draining) void this.drain();
  }

  private async drain() {
    this.draining = true;
    try {
      while (this.events.length) {
        const ev = this.events.shift()!;
        const need = ev.e ? this.pickNeed(ev.kind, ev.e) : null;
        this.eventHit = need && ev.e ? await this.nativePick(ev.e, need) : null;
        try {
          await ev.run(ev.e as MouseEvent);
        } catch (err) {
          console.error(err);
        }
        if (this.eventHit) this.lastHit = { hit: this.eventHit, cam: this.camKey(), mesh: this.currentMesh };
        this.eventHit = null;
      }
    } finally {
      this.draining = false;
    }
  }

  /** Native app: resolves once every queued pointer event has been handled (tests). */
  async settled() {
    while (this.draining || this.events.length) await new Promise((r) => setTimeout(r, 5));
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
    if (this.native) {
      // oc-render's pick and ray cast, made when the right button came up there.
      const h = this.contextHit;
      if (!c || !mesh || !h) return [];
      const out: SelItem[] = [];
      if (h.item && h.item.kind !== "face") out.push(h.item);
      for (const f of h.faces) out.push({ kind: "face", id: f.id });
      return out;
    }
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

  /** Insert Components: the pointer's point in the plane through the view's centre facing
   * the viewer (where the new component's centre goes). */
  private placingPoint(sx: number, sy: number): THREE.Vector3 | null {
    const plane = new THREE.Plane().setFromNormalAndCoplanarPoint(this.cam.viewDir(), this.cam.target);
    return this.cam.ray(sx, sy).intersectPlane(plane, new THREE.Vector3());
  }

  /** 3D point on the body under the cursor (for rotate-about). */
  private hitPoint(sx: number, sy: number): THREE.Vector3 | null {
    if (this.native) {
      const f = this.nativeAt(sx, sy)?.faces[0];
      return f ? new THREE.Vector3(...f.point) : null;
    }
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
    const ab = this.app.asm?.box();
    if (ab) {
      b.expandByPoint(new THREE.Vector3(...ab[0]));
      b.expandByPoint(new THREE.Vector3(...ab[1]));
    }
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

  private modelSnapCache: { mesh: BodyMesh; frame: Frame; v: ModelSnaps } | null = null;

  /**
   * Sketch snap to the model edge or vertex under the pointer (GPU pick, so it is what the
   * user sees there): a vertex, an edge's midpoint (within the snap radius on screen) or
   * the point of the edge nearest the pointer on screen, projected onto the sketch plane.
   */
  private modelSnapAt(sx: number, sy: number): { point: Vec2; vertex?: number; edge?: number; mid?: boolean } | null {
    const s = this.app.sketch, mesh = this.app.result?.mesh;
    if (!s || !mesh) return null;
    const hit = this.pick(sx, sy);
    if (hit?.kind === "vertex") {
      const p = mesh.vertices[hit.id as number];
      return p ? { point: worldToFrame(s.frame, p), vertex: hit.id as number } : null;
    }
    if (hit?.kind !== "edge") return null;
    const id = hit.id as number;
    const info = mesh.edges[id];
    if (!info) return null;
    const sig = info.sig;
    const closed = Math.hypot(sig.a[0] - sig.b[0], sig.a[1] - sig.b[1], sig.a[2] - sig.b[2]) < 1e-7 * Math.max(1, sig.length);
    if (!closed) {
      const mid: Vec3 = info.linear ? [(sig.a[0] + sig.b[0]) / 2, (sig.a[1] + sig.b[1]) / 2, (sig.a[2] + sig.b[2]) / 2] : sig.mid;
      const [mx, my] = this.cam.project(new THREE.Vector3(...mid));
      if (Math.hypot(mx - sx, my - sy) < 9) return { point: worldToFrame(s.frame, mid), edge: id, mid: true };
    }
    const P = mesh.edgePositions;
    let best: Vec3 | null = null, bd = Infinity;
    for (const g of mesh.edgeGroups) {
      if (g.edgeId !== id) continue;
      for (let i = g.start; i + 1 < g.start + g.count; i += 2) {
        const a = new THREE.Vector3(P[i * 3], P[i * 3 + 1], P[i * 3 + 2]), b = new THREE.Vector3(P[i * 3 + 3], P[i * 3 + 4], P[i * 3 + 5]);
        const [ax, ay] = this.cam.project(a), [bx, by] = this.cam.project(b);
        const dx = bx - ax, dy = by - ay, l2 = dx * dx + dy * dy;
        const t = l2 > 0 ? Math.max(0, Math.min(1, ((sx - ax) * dx + (sy - ay) * dy) / l2)) : 0;
        const d = Math.hypot(ax + dx * t - sx, ay + dy * t - sy);
        if (d < bd) [bd, best] = [d, a.lerp(b, t).toArray() as Vec3];
      }
    }
    return best ? { point: worldToFrame(s.frame, best), edge: id } : null;
  }

  /**
   * The body's vertices and edges lying in the open sketch's plane (the face being sketched
   * on, coplanar faces), in sketch coordinates, for sketch snapping. Cached per mesh and plane.
   */
  private modelSnaps(): ModelSnaps | null {
    const s = this.app.sketch, mesh = this.app.result?.mesh;
    if (!s || !mesh) return null;
    const c = this.modelSnapCache;
    if (c && c.mesh === mesh && c.frame === s.frame) return c.v;
    const f = s.frame;
    const diag = mesh.bbox ? new THREE.Vector3(...mesh.bbox[1]).sub(new THREE.Vector3(...mesh.bbox[0])).length() : 100;
    // Mesh coordinates are float32: allow for their rounding.
    const eps = 1e-6 + 2e-6 * Math.max(diag, Math.hypot(...f.origin));
    const off = (x: number, y: number, z: number) => Math.abs((x - f.origin[0]) * f.normal[0] + (y - f.origin[1]) * f.normal[1] + (z - f.origin[2]) * f.normal[2]);
    const v: ModelSnaps = { points: [], edges: [] };
    mesh.vertices.forEach((p, id) => {
      if (off(...p) < eps) v.points.push({ id, p: worldToFrame(f, p) });
    });
    const P = mesh.edgePositions;
    for (const g of mesh.edgeGroups) {
      const info = mesh.edges[g.edgeId];
      if (g.edgeId < 0 || !info || info.seam) continue;
      const segs: [Vec2, Vec2][] = [];
      let inPlane = true;
      for (let i = g.start; i + 1 < g.start + g.count; i += 2) {
        const a: Vec3 = [P[i * 3], P[i * 3 + 1], P[i * 3 + 2]], b: Vec3 = [P[i * 3 + 3], P[i * 3 + 4], P[i * 3 + 5]];
        if (off(...a) > eps || off(...b) > eps) {
          inPlane = false;
          break;
        }
        segs.push([worldToFrame(f, a), worldToFrame(f, b)]);
      }
      if (!inPlane || !segs.length) continue;
      const sig = info.sig;
      const closed = Math.hypot(sig.a[0] - sig.b[0], sig.a[1] - sig.b[1], sig.a[2] - sig.b[2]) < eps;
      v.edges.push({ id: g.edgeId, segs, mid: closed ? null : worldToFrame(f, info.linear ? [(sig.a[0] + sig.b[0]) / 2, (sig.a[1] + sig.b[1]) / 2, (sig.a[2] + sig.b[2]) / 2] : sig.mid) });
    }
    this.modelSnapCache = { mesh, frame: f, v };
    return v;
  }

  /**
   * What planes size themselves to: the body, else the sketches. A lone point (an empty
   * sketch's origin) is no content, so planes keep their default size.
   */
  private planeSizeBox(): THREE.Box3 | null {
    const body = this.bodyBox();
    if (body) return body;
    const c = this.contentBox();
    return c.isEmpty() || c.getSize(new THREE.Vector3()).length() < 1e-6 ? null : c;
  }

  /**
   * View box when entering a sketch: the sketch plane as drawn plus the model, so the
   * view is focused on the plane (an empty part fills the view with the plane instead of
   * a default box around the origin).
   */
  private sketchFitBox(f: Frame): THREE.Box3 {
    const content = this.planeSizeBox();
    const b = content ? content.clone() : new THREE.Box3();
    const id = this.app.sketch ? sketchPlaneId(this.app) : null;
    const [u0, u1, v0, v1] = (id && this.app.doc.display?.planeExtents?.[id]) || autoExtents(f, content);
    for (const [u, v] of [[u0, v0], [u1, v0], [u0, v1], [u1, v1]]) b.expandByPoint(new THREE.Vector3(...frameToWorld(f, [u, v])));
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
    el.addEventListener("pointerdown", (e) => {
      // At once, also in the native app's queue: focus, capture, no middle-button autoscroll.
      this.overlay.focus();
      this.overlay.setPointerCapture(e.pointerId);
      if (e.button === 1) e.preventDefault();
      this.dispatch("down", e, this.onPointerDown);
    });
    // Focus is set explicitly on pointerdown; the default mousedown focus change would take
    // it back from inputs opened by the click (the Smart Dimension Modify box).
    el.addEventListener("mousedown", (e) => e.preventDefault());
    el.addEventListener("pointermove", (e) => this.dispatch("move", e, this.onPointerMove));
    el.addEventListener("pointerup", (e) => this.dispatch("up", e, this.onPointerUp));
    el.addEventListener("pointerleave", () =>
      this.dispatch("leave", null, () => {
        if (!this.nav) this.app.setHover(null);
      }),
    );
    el.addEventListener("dblclick", (e) => this.dispatch("dblclick", e, this.onDblClick));
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
    if (this.editor) this.dispatch("editorMove", e, () => this.editor?.onMove(p));
  };

  private onPointerDown = (e: PointerEvent) => {
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
      const f = this.selectorAt(p.sx, p.sy);
      if (f) {
        const zoom = this.app.opt<boolean>("view.zoomToFitOnStandard");
        this.cam.lookFrom(f.normal, f.up, this.app.transition("view.selectorTransition"), zoom ? this.modelBox() : undefined, f.name);
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
    // Insert Components: the click drops the component that follows the pointer.
    if (this.app.asm?.placing) {
      this.placingDown = true;
      return;
    }
    this.leftDown = { x: p.sx, y: p.sy };
    // Assemblies: pressing on a component that is free to move starts dragging it (it
    // moves within the freedom its mates leave, or as the Move/Rotate Component
    // PropertyManager says); a click without moving still selects. Alt-dragging a face,
    // edge or vertex onto another component's is SmartMates.
    const asm = this.app.asm;
    if (asm && !asm.matePM && !this.app.pm) {
      const hit = this.pick(p.sx, p.sy);
      const onBody = !!hit && "body" in hit && hit.body !== undefined;
      if (onBody && e.altKey && !asm.movePM?.pickingEntity && (hit!.kind === "face" || hit!.kind === "edge" || hit!.kind === "vertex")) {
        this.smartDrag = { from: hit!, x: p.sx, y: p.sy };
        this.leftDown = null;
        return;
      }
      // Options > Assemblies > "Move components by dragging" (off: only in Move Component).
      const dragOk = asm.movePM || this.app.opt("asm.dragMove") !== false;
      const at = onBody && dragOk ? this.hitPoint(p.sx, p.sy) : null;
      const vd = this.cam.viewDir();
      if (onBody && at && asm.dragStart((hit as { body: number }).body, [at.x, at.y, at.z], [vd.x, vd.y, vd.z])) {
        this.compDrag = { at: at.clone(), x: p.sx, y: p.sy, moved: false };
      }
    }
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
      const f = this.selectorAt(p.sx, p.sy);
      if (this.selectorHover(f?.key ?? null)) this.invalidate();
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
    if (this.app.asm?.placing) {
      const w = this.placingPoint(p.sx, p.sy);
      if (w) this.app.asm.placeAt([w.x, w.y, w.z]);
      return;
    }
    if (this.smartDrag) return;
    if (this.compDrag) {
      const c = this.compDrag;
      if (!c.moved && Math.hypot(p.sx - c.x, p.sy - c.y) <= 4) return;
      c.moved = true;
      // The grabbed point follows the pointer in the plane through it facing the viewer.
      const plane = new THREE.Plane().setFromNormalAndCoplanarPoint(this.cam.viewDir(), c.at);
      const target = this.cam.ray(p.sx, p.sy).intersectPlane(plane, new THREE.Vector3());
      if (target) this.app.asm?.dragTo([target.x, target.y, target.z]);
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
        this.contextHit = this.eventHit;
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
    if (this.placingDown && e.button === 0) {
      this.placingDown = false;
      const w = this.placingPoint(p.sx, p.sy);
      if (w) void this.app.asm?.dropAt([w.x, w.y, w.z]);
      this.invalidate();
      return;
    }
    if (this.smartDrag && e.button === 0) {
      // SmartMates: dropped on a face, edge or vertex of another component, the two are
      // mated as the Mate PropertyManager proposes (its pop-up toolbar to confirm).
      const sd = this.smartDrag;
      this.smartDrag = null;
      const hit = this.pick(p.sx, p.sy);
      if (Math.hypot(p.sx - sd.x, p.sy - sd.y) > 4 && hit && "body" in hit && hit.body !== undefined && (hit.kind === "face" || hit.kind === "edge" || hit.kind === "vertex")) {
        void this.app.asm?.smartMate(sd.from, hit);
      }
      this.invalidate();
      return;
    }
    const down = this.leftDown;
    this.leftDown = null;
    if (this.compDrag && e.button === 0) {
      const c = this.compDrag;
      this.compDrag = null;
      if (c.moved) {
        this.app.asm?.dragEnd();
        this.invalidate();
        return;
      }
      this.app.asm?.dragCancel();
    }
    if (this.box) {
      const b = this.box;
      this.box = null;
      this.invalidate();
      return this.boxSelectModel(b, e.ctrlKey);
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
    // Instances to Skip: clicking an instance dot skips or restores it.
    const markers = this.skipMarkers();
    if (markers.length) {
      let best: (typeof markers)[number] | null = null, bd = 10;
      for (const m of markers) {
        const d = Math.hypot(m.x - p.sx, m.y - p.sy);
        if (d < bd) (bd = d), (best = m);
      }
      if (best) this.app.toggleSkip(best.key);
      this.invalidate();
      return;
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
    if (this.app.doc.display?.color) return this.app.doc.display.color;
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

  /** View Selector facet under the pointer: its view direction, up vector and name. */
  private selectorAt(sx: number, sy: number): { normal: THREE.Vector3; up: THREE.Vector3; name: string | null; key: unknown } | null {
    if (this.native) {
      const f = this.nativeAt(sx, sy)?.selector;
      return f ? { normal: new THREE.Vector3(...f.normal), up: new THREE.Vector3(...f.up), name: f.name, key: f.index } : null;
    }
    const sel = this.selector as ViewSelector;
    const f = sel.pick(sx, sy, this.size.w, this.size.h);
    return f ? { normal: f.normal.clone(), up: ViewSelector.upFor(f.normal), name: f.name, key: f } : null;
  }

  /** Sets the hovered facet (a `selectorAt` key); whether it changed. */
  private selectorHover(key: unknown): boolean {
    if (this.selector instanceof NativeSelector) return this.selector.setHover(key as number | null);
    return this.selector.setHover(key as Facet | null);
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

  private boxSelectModel(b: { x0: number; y0: number; x1: number; y1: number }, ctrl: boolean): void | Promise<void> {
    const mesh = this.currentMesh;
    if (!mesh) return;
    if (this.native) {
      return this.native.boxEdges(this.cam, b, b.x1 < b.x0).then((ids) => {
        if (!ctrl) this.app.selection = [];
        for (const id of ids) this.app.select({ kind: "edge", id }, "add");
        this.app.changed();
      });
    }
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
