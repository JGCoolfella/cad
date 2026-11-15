/**
 * The native viewport (`crates/oc-app`): `oc-render` draws the graphics area under this
 * transparent webview. The `Viewport` keeps navigation and the 2D overlay; instead of
 * drawing its scene with three.js it describes it here and sends it, with the camera and
 * the graphics area's position, to the renderer. Picking (hover, clicks, Select Other, the
 * rotate-about point, box selection) and the View Selector are answered by `oc-render`
 * too (F2b): there is no WebGL context in the native app.
 *
 * The webview's camera stays the source of truth: the renderer's `Camera` has the same
 * fields (target, orientation, visible height, projection), so overlay projections from
 * `ViewCamera` match the renderer's pixels.
 *
 * Frames are numbered (`seq`). A pick names the newest frame sent; the render thread
 * answers it with that frame's scene (or a newer one) and says which, so plane and
 * feature indices are mapped back with the lists of the frame that answered.
 */
import type { App, SelItem } from "../app/app";
import type { Frame, Vec3 } from "../core/types";
import type { ViewCamera } from "../render/camera";
import { LABEL_COLOR, labelCanvas, type Extents } from "../render/planes";
import type { NativeKernelClient } from "./engine";
import { invoke } from "./tauri";

type Hit =
  | { k: "origin" }
  | { k: "face" | "edge" | "vertex"; body: number; index: number }
  | { k: "plane"; i: number }
  | { k: "feature"; i: number };

interface Body {
  id: number;
  mesh: number;
  role: "part" | "preview" | "previewCut";
  color: string;
  transparent?: boolean;
  previewFaces?: number[];
}

/** Scene items the viewport collects while it builds its pick scene. */
export class NativeScene {
  planes: { id: number; frame: Frame; extents: Extents; label: number | null }[] = [];
  axes: { id: number; point: Vec3; dir: Vec3; length: number }[] = [];
  points: { id: number; point: Vec3 }[] = [];
  csys: { id: number; frame: Frame }[] = [];
  sketches: { id: number; segments: number[]; profile: boolean }[] = [];
  /** Plane ids and feature ids (sketches, axes, points, coordinate systems) by index. */
  planeIds: string[] = [];
  featureIds: string[] = [];

  private featureIndex(id: string) {
    let i = this.featureIds.indexOf(id);
    if (i < 0) i = this.featureIds.push(id) - 1;
    return i;
  }
  plane(id: string, frame: Frame, extents: Extents, label: number | null) {
    this.planes.push({ id: this.planeIds.push(id) - 1, frame, extents, label });
  }
  sketch(id: string, segments: number[], profile: boolean) {
    this.sketches.push({ id: this.featureIndex(id), segments, profile });
  }
  axis(id: string, point: Vec3, dir: Vec3, length: number) {
    this.axes.push({ id: this.featureIndex(id), point, dir, length });
  }
  point(id: string, point: Vec3) {
    this.points.push({ id: this.featureIndex(id), point });
  }
  frame(id: string, frame: Frame) {
    this.csys.push({ id: this.featureIndex(id), frame });
  }

  /** The selection item of a hit from the renderer (the inverse of `hit`). */
  item(h: Hit): SelItem | null {
    switch (h.k) {
      case "origin":
        return { kind: "origin" };
      case "face":
      case "edge":
      case "vertex":
        // Body 0 is the part; previews (body 1) are not pickable.
        return h.body === 0 ? { kind: h.k, id: h.index } : null;
      case "plane":
        return h.i < this.planeIds.length ? { kind: "plane", id: this.planeIds[h.i] } : null;
      case "feature":
        return h.i < this.featureIds.length ? { kind: "feature", id: this.featureIds[h.i] } : null;
    }
  }

  hit(s: SelItem): Hit | null {
    switch (s.kind) {
      case "origin":
        return { k: "origin" };
      case "face":
      case "edge":
      case "vertex":
        return { k: s.kind, body: 0, index: s.id };
      case "plane": {
        const i = this.planeIds.indexOf(s.id);
        return i < 0 ? null : { k: "plane", i };
      }
      case "feature": {
        const i = this.featureIds.indexOf(s.id);
        return i < 0 ? null : { k: "feature", i };
      }
      default:
        return null;
    }
  }
}

/** View Selector state in the native app: `oc-render` draws the cube and finds its facets. */
export class NativeSelector {
  visible = false;
  /** Index of the hovered facet (`oc_render::selector::facets`). */
  hover: number | null = null;
  setHover(i: number | null) {
    if (i === this.hover) return false;
    this.hover = i;
    return true;
  }
}

/** A View Selector facet the renderer found under the pointer. */
export interface NativeFacet {
  index: number;
  normal: Vec3;
  /** Camera up when looking from it (`ViewSelector.upFor`). */
  up: Vec3;
  name: string | null;
}

/** What the renderer found at a pointer position. */
export interface NativeHit {
  sx: number;
  sy: number;
  item: SelItem | null;
  /** Faces of the part along the pointer ray, nearest first, where they were crossed (only when asked). */
  faces: { id: number; point: Vec3 }[];
  selector: NativeFacet | null;
}

export interface PickOptions {
  x: number;
  y: number;
  /** `app.selFilter` while the filter is on; the Chamfer PropertyManager takes edges only. */
  filter: { only: string[] | null; edgesOnly: boolean };
  /** A click (the renderer waits for the GPU) rather than hover. */
  wait: boolean;
  /** Faces along the ray: the first (rotate-about, Hole Wizard) or all (Select Other). */
  ray: "none" | "first" | "all";
  /** Try the View Selector's facets first. */
  selector: boolean;
}

interface PickReply {
  seq: number;
  hit: Hit | null;
  ray: { hit: Hit; t: number; point: Vec3 }[];
  selector: NativeFacet | null;
}

/** How long the webview waits for a pick before treating it as a miss (the event queue must not stall). */
const PICK_TIMEOUT_MS = 1500;

export class NativeView {
  scene = new NativeScene();
  /** Number of the newest frame sent. */
  seq = 0;
  /** Scenes of recent frames, by number (picks are answered against one of them). */
  private sent = new Map<number, NativeScene>();
  private labels = new Map<string, number | "pending">();
  private nextLabel = 1;
  private inFlight = false;
  private queued: unknown = null;

  constructor(
    private app: App,
    private redraw: () => void,
  ) {}

  /** Starts collecting a new scene. */
  reset() {
    this.scene = new NativeScene();
  }

  /** Key of a plane name image, uploading it on first use (null until it is there). */
  label(text: string): number | null {
    const k = this.labels.get(text);
    if (typeof k === "number") return k;
    if (k === "pending") return null;
    this.labels.set(text, "pending");
    const key = this.nextLabel++;
    const c = labelCanvas(text, LABEL_COLOR);
    const px = c.getContext("2d")!.getImageData(0, 0, c.width, c.height).data;
    invoke("view_add_label", new Uint8Array(px.buffer, px.byteOffset, px.byteLength), {
      headers: { "x-key": String(key), "x-width": String(c.width), "x-height": String(c.height) },
    })
      .then(() => {
        this.labels.set(text, key);
        this.redraw();
      })
      .catch((e) => console.error("plane label", e));
    return null;
  }

  /** The camera as the renderer takes it. */
  static camera(c: ViewCamera) {
    return {
      target: c.target.toArray(),
      quat: [c.quat.x, c.quat.y, c.quat.z, c.quat.w],
      height: c.height,
      perspective: c.perspective,
      width: c.width,
      heightPx: c.heightPx,
    };
  }

  /** The graphics area in CSS px within the window, and the device pixel ratio. */
  static rect(host: HTMLElement) {
    const r = host.getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height, scale: window.devicePixelRatio };
  }

  /** What `oc-render` finds at (x, y) of the graphics area, with the newest frame's scene. */
  async pick(cam: ViewCamera, host: HTMLElement, o: PickOptions): Promise<NativeHit> {
    const miss: NativeHit = { sx: o.x, sy: o.y, item: null, faces: [], selector: null };
    const req = { seq: this.seq, camera: NativeView.camera(cam), rect: NativeView.rect(host), ...o };
    let r: PickReply | null;
    try {
      r = await withTimeout(invoke<PickReply>("view_pick", { req }), PICK_TIMEOUT_MS);
    } catch (e) {
      console.error("view_pick", e);
      return miss;
    }
    if (!r) return miss;
    const scene = this.sent.get(r.seq);
    const item = r.hit && scene ? scene.item(r.hit) : null;
    const faces: NativeHit["faces"] = [];
    for (const x of r.ray) if (x.hit.k === "face" && x.hit.body === 0) faces.push({ id: x.hit.index, point: x.point });
    return { sx: o.x, sy: o.y, item, faces, selector: r.selector };
  }

  /** Model edges in a selection box (logical px, drag order): `oc-render`'s box selection. */
  async boxEdges(cam: ViewCamera, b: { x0: number; y0: number; x1: number; y1: number }, crossing: boolean): Promise<number[]> {
    const req = { seq: this.seq, camera: NativeView.camera(cam), ...b, crossing };
    try {
      const r = await withTimeout(invoke<{ seq: number; hits: Hit[] }>("view_box", { req }), PICK_TIMEOUT_MS);
      return (r?.hits ?? []).flatMap((h) => (h.k === "edge" && h.body === 0 ? [h.index] : []));
    } catch (e) {
      console.error("view_box", e);
      return [];
    }
  }

  /** Sends the frame (the newest one wins while one is on its way). */
  send(opts: { cam: ViewCamera; host: HTMLElement; style: string; modelColor: string; selector: { hover: number | null } | null }) {
    const app = this.app;
    const C = app.options.colors;
    const s = this.scene;
    const disp = app.doc.display ?? {};
    const pm = app.pm;
    const inPM = !!(pm && "feature" in pm);
    const mesh = (inPM && app.previewResult?.mesh) || app.result?.mesh || null;
    const tool = (inPM && app.previewResult?.toolMesh) || null;
    const bodies: Body[] = [];
    if (mesh?.handle && !disp.bodyHidden) {
      // Fillet/chamfer previews: the faces the feature makes, in preview yellow.
      const pmId = pm && "feature" in pm && app.previewResult?.mesh && !app.previewResult.toolMesh ? pm.feature.id : null;
      const previewFaces = pmId ? mesh.faces.filter((f) => f.owner === pmId).map((f) => f.id) : [];
      bodies.push({ id: 0, mesh: mesh.handle, role: "part", color: opts.modelColor, transparent: !!disp.transparent, previewFaces });
    }
    if (tool?.handle) {
      const cut = !!(pm && (pm.kind === "extrude" || pm.kind === "revolve") && pm.feature.cut);
      bodies.push({ id: 1, mesh: tool.handle, role: cut ? "previewCut" : "preview", color: "#ffff80" });
    }
    const retain = [app.result?.mesh, app.result?.toolMesh, app.previewResult?.mesh, app.previewResult?.toolMesh]
      .map((m) => m?.handle)
      .filter((h): h is number => !!h);
    const sp = app.sectionPlane();
    const originOn = app.opt("view.origins") !== false;
    const origin = { kind: "origin" } as const;
    const hits = (items: (SelItem | null)[]) => items.map((x) => (x ? s.hit(x) : null)).filter((x): x is Hit => !!x);
    const bg = C.background;
    const scene = {
      style: opts.style,
      options: {
        hover: C.hover,
        selected: C.selected,
        edges: C.edges,
        sketchInactive: C.sketchInactive,
        tempShaded: app.opt<string>("color.tempShaded") || "#ffff80",
        bgMode: app.opt<string>("ui.bgMode") ?? "scene",
        bgTop: bg[0],
        bgBottom: bg[1],
        planeFront: app.docProp<string>("plane.front") ?? "#c6d6ee",
        planeBack: app.docProp<string>("plane.back") ?? "#e8dcc2",
        planeTransparency: Number(app.docProp<number>("plane.transparency") ?? 82),
        shadedPlanes: app.opt("display.shadedPlanes") !== false,
        hiddenEdgesDashed: app.opt<string>("display.hiddenEdges") !== "solid",
        shadedFaceHighlight: app.opt("display.shadedFaceHighlight") !== false,
        dynamicHighlight: app.opt("display.dynamicHighlight") !== false,
        triad: app.opt("display.triad") !== false,
        msaa: app.options.performance.msaa !== false,
      },
      bodies,
      planes: s.planes,
      axes: s.axes,
      points: s.points,
      csys: s.csys,
      sketches: s.sketches,
      originShown: !app.sketch && (originOn || app.hover?.kind === "origin" || app.isSelected(origin)),
      originPickable: !app.sketch && originOn,
      section: sp ? { origin: sp.o, normal: sp.n } : null,
      hover: hits([app.hover])[0] ?? null,
      selected: hits(app.selection),
      retain,
      seen: (app.kernel as NativeKernelClient).seen ?? 0,
      seq: ++this.seq,
      selector: opts.selector,
    };
    this.sent.set(scene.seq, s);
    // Frames are coalesced and answered within moments; a few dozen cover any pick in flight.
    for (const k of this.sent.keys()) if (k <= scene.seq - 64) this.sent.delete(k);
    this.post({ scene, camera: NativeView.camera(opts.cam), rect: NativeView.rect(opts.host) });
  }

  private post(frame: unknown) {
    if (this.inFlight) {
      this.queued = frame;
      return;
    }
    this.inFlight = true;
    invoke("view_frame", frame)
      .catch((e) => console.error("view_frame", e))
      .finally(() => {
        this.inFlight = false;
        const q = this.queued;
        this.queued = null;
        if (q) this.post(q);
      });
  }
}

/** The promise's value, or null once `ms` have passed. */
function withTimeout<T>(p: Promise<T>, ms: number): Promise<T | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<null>((r) => (timer = setTimeout(() => r(null), ms)));
  return Promise.race([p, late]).finally(() => clearTimeout(timer));
}
