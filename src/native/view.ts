/**
 * The native viewport (`crates/oc-app`): `oc-render` draws the graphics area under this
 * transparent webview. The three.js `Viewport` keeps doing everything else (navigation,
 * picking, the 2D overlay, the View Selector); instead of drawing its scene it describes it
 * here and sends it, with the camera and the graphics area's position, to the renderer.
 *
 * The webview's camera stays the source of truth: the renderer's `Camera` has the same
 * fields (target, orientation, visible height, projection), so overlay projections from
 * `ViewCamera` match the renderer's pixels.
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

export class NativeView {
  scene = new NativeScene();
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

  /** Sends the frame (the newest one wins while one is on its way). */
  send(opts: { cam: ViewCamera; host: HTMLElement; style: string; modelColor: string }) {
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
    };
    const c = opts.cam;
    const camera = {
      target: c.target.toArray(),
      quat: [c.quat.x, c.quat.y, c.quat.z, c.quat.w],
      height: c.height,
      perspective: c.perspective,
      width: c.width,
      heightPx: c.heightPx,
    };
    const r = opts.host.getBoundingClientRect();
    const rect = { x: r.left, y: r.top, w: r.width, h: r.height, scale: window.devicePixelRatio };
    this.post({ scene, camera, rect });
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
