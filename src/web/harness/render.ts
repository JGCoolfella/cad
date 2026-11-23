/**
 * The render worker's harness (W1c; `node scripts/web-render.mjs smoke` opens it in headless
 * Chromium): the worker started as the app will start it, meshes fed through an engine
 * channel (as the engine worker will feed them, W1b), frames and picks in the shapes
 * `NativeView` sends, with the UI's own camera. Result in `window.__webRenderResult`.
 *
 * Checks: the first frame is drawn; a click and a hover pick find the block's top face;
 * the origin, empty space; a window box takes the block's edges; draw counts come back; a
 * frame that names a mesh still on its way is drawn again when the mesh arrives. Times: a
 * part, and 10 000 instances (two meshes) turning, frame and pick.
 */
import * as THREE from "three";
import { ViewCamera } from "../../render/camera";
import { startViewport, WebRenderClient } from "../render";

type Hit = { k: string; body?: number; index?: number } | null;
type Result = Record<string, unknown> & { ok: boolean; checks: Record<string, boolean> };
const out: Result = { ok: false, checks: {} };
(window as unknown as { __webRenderResult?: Result }).__webRenderResult = undefined;

const MODULE = "/target/web-render/oc_web_render.js";
const OPTIONS = {
  hover: "#ff9a1f", selected: "#2e8bff", edges: "#1a1a1a", sketchInactive: "#5a5a5a", tempShaded: "#ffff80",
  bgMode: "scene", bgTop: "#ffffff", bgBottom: "#c8cdd4", planeFront: "#c6d6ee", planeBack: "#e8dcc2",
  planeTransparency: 82, shadedPlanes: true, hiddenEdgesDashed: true, shadedFaceHighlight: true,
  dynamicHighlight: true, triad: true, msaa: true,
};

const camDto = (c: ViewCamera) => ({ target: c.target.toArray(), quat: [c.quat.x, c.quat.y, c.quat.z, c.quat.w], height: c.height, perspective: c.perspective, width: c.width, heightPx: c.heightPx });
const rect = () => ({ x: 0, y: 0, w: innerWidth, h: innerHeight, scale: devicePixelRatio });
const median = (v: number[]) => [...v].sort((a, b) => a - b)[Math.floor(v.length / 2)];
const round = (x: number) => Math.round(x * 100) / 100;

/** `?fallback`: the browser runs without WebGPU; the viewport must say so (three.js draws). */
async function fallback() {
  const m = await startViewport(document.getElementById("view") as HTMLCanvasElement, MODULE);
  out.mode = m.kind;
  out.reason = m.kind === "three" ? m.reason : null;
  out.checks.fallsBackToThree = m.kind === "three" && !m.assemblies;
  out.ok = Object.values(out.checks).every(Boolean);
}

async function main() {
  if (new URLSearchParams(location.search).has("fallback")) return fallback();
  const canvas = document.getElementById("view") as HTMLCanvasElement;
  const t0 = performance.now();
  const c = await WebRenderClient.start(canvas, MODULE);
  if (!(c instanceof WebRenderClient)) throw new Error(`no WebGPU viewport: ${c.reason}`);
  out.startMs = round(performance.now() - t0);
  out.info = c.info;
  out.adapter = await c.command("app_info");

  const drawnSeq = (seq: number) =>
    new Promise<number>((ok, fail) => {
      if (c.drawnSeq >= seq) return ok(0);
      const t = performance.now();
      const timer = setTimeout(() => fail(new Error(`frame ${seq} not drawn`)), 10_000);
      const off = c.onDrawn((s) => {
        if (s >= seq) {
          clearTimeout(timer);
          off();
          ok(performance.now() - t);
        }
      });
    });

  let seq = 0;
  let seen = 0;
  const frame = (cam: ViewCamera, bodies: unknown[] | null, extra: Record<string, unknown> = {}) => {
    seq++;
    const scene = { style: "shadedEdges", options: OPTIONS, bodies, planes: [], axes: [], points: [], csys: [], sketches: [], originShown: true, originPickable: true, section: null, hover: null, selected: [], retain: [1, 2, 3], seen, seq, ...extra };
    void c.command("view_frame", { scene, camera: camDto(cam), rect: rect() });
    return seq;
  };
  const pick = async (cam: ViewCamera, x: number, y: number, wait: boolean) => {
    const t = performance.now();
    const r = (await c.command("view_pick", { req: { seq, camera: camDto(cam), rect: rect(), x, y, wait } })) as { seq: number; hit: Hit };
    return { hit: r.hit, ms: performance.now() - t };
  };

  // A part: the F1 block (20 × 10 × 5, a Ø6 hole), its corner at the origin.
  const engine = c.engineChannel();
  const blob = async (name: string) => (await fetch(`/target/web-render/${name}.ocmesh`)).arrayBuffer();
  const [block, disc] = [await blob("block"), await blob("disc")];
  const block2 = block.slice(0);
  engine.postMessage({ t: "mesh", handle: 1, bytes: block }, [block]);
  seen = 1;
  const cam = new ViewCamera();
  cam.resize(innerWidth, innerHeight);
  cam.setView("isometric", false, 0, new THREE.Box3(new THREE.Vector3(0, 0, 0), new THREE.Vector3(20, 10, 5)));
  const part = [{ id: 0, mesh: 1, role: "part", color: "#c9ccd1" }];
  const first = frame(cam, part);
  out.firstFrameMs = round(await drawnSeq(first));
  out.checks.firstFrameDrawn = true;

  const top = cam.project(new THREE.Vector3(3, 2.5, 5));
  const click = await pick(cam, top[0], top[1], true);
  const hover = await pick(cam, top[0], top[1], false);
  out.checks.clickFindsTopFace = click.hit?.k === "face" && click.hit.body === 0;
  out.checks.hoverFindsTopFace = hover.hit?.k === "face" && hover.hit.body === 0 && hover.hit.index === click.hit?.index;
  const o = cam.project(new THREE.Vector3(0, 0, 0));
  out.checks.originFirst = (await pick(cam, o[0], o[1], false)).hit?.k === "origin";
  out.checks.emptySpace = (await pick(cam, 5, innerHeight / 2, false)).hit === null;
  const corners = [new THREE.Vector3(0, 0, 0), new THREE.Vector3(20, 10, 5), new THREE.Vector3(20, 0, 0), new THREE.Vector3(0, 10, 5), new THREE.Vector3(0, 10, 0), new THREE.Vector3(20, 0, 5)].map((p) => cam.project(p));
  const xs = corners.map((p) => p[0]);
  const ys = corners.map((p) => p[1]);
  const box = (await c.command("view_box", { req: { seq, camera: camDto(cam), x0: Math.min(...xs) - 10, y0: Math.min(...ys) - 10, x1: Math.max(...xs) + 10, y1: Math.max(...ys) + 10, crossing: false } })) as { hits: Hit[] };
  out.boxEdges = box.hits.length;
  out.checks.boxTakesEdges = box.hits.length > 12 && box.hits.every((h) => h?.k === "edge");
  const firstStats = await c.command("view_stats");
  await drawnSeq(frame(cam, null));
  await new Promise((r) => setTimeout(r, 100));
  const stats = (await c.command("view_stats")) as { instances: number; levels: number[] } | null;
  out.stats = stats;
  out.checks.drawCountsComeBack = firstStats === null && stats !== null && stats.instances === 1 && stats.levels.reduce((a, b) => a + b, 0) === 1;

  // A frame naming a mesh that arrives after it (the engine's port and the UI's are not
  // ordered): drawn again once the mesh is there.
  const before = (await c.drawn()).frames;
  const late = [...part, { id: 1, mesh: 3, role: "part", color: "#7d93ad", placement: { position: [0, 0, 10], rotation: [1, 0, 0, 0] } }];
  seen = 3;
  await drawnSeq(frame(cam, late));
  engine.postMessage({ t: "mesh", handle: 3, bytes: block2 }, [block2]);
  for (let i = 0; i < 100 && (await c.drawn()).frames < before + 2; i++) await new Promise((r) => setTimeout(r, 20));
  const lateTop = cam.project(new THREE.Vector3(3, 2.5, 15));
  const lateHit = (await pick(cam, lateTop[0], lateTop[1], true)).hit;
  out.checks.lateMeshDrawn = (await c.drawn()).frames >= before + 2 && lateHit?.k === "face" && lateHit.body === 1;

  // Part times: the view turning.
  const turn = async (n: number, bodies: unknown[]) => {
    const frameMs: number[] = [];
    const gpuMs: number[] = [];
    await drawnSeq(frame(cam, bodies, { moving: true }));
    for (let i = 0; i < n; i++) {
      cam.rotate(4, 1, cam.target.clone());
      frameMs.push(await drawnSeq(frame(cam, null, { moving: true })));
      for (let k = 0; k < 50; k++) {
        const d = await c.drawn();
        if (d.gpuMs !== null) {
          gpuMs.push(d.gpuMs);
          break;
        }
        await new Promise((r) => setTimeout(r, 2));
      }
    }
    await drawnSeq(frame(cam, null));
    const picks: number[] = [];
    for (let i = 0; i < 30; i++) picks.push((await pick(cam, innerWidth * (0.3 + 0.4 * Math.random()), innerHeight * (0.3 + 0.4 * Math.random()), false)).ms);
    return { frameMs: round(median(frameMs)), frameMsMax: round(Math.max(...frameMs)), gpuMs: gpuMs.length ? round(median(gpuMs)) : null, hoverPickMs: round(median(picks)), hoverPickMsMax: round(Math.max(...picks)) };
  };
  out.part = await turn(60, part);

  // 10 000 instances of two meshes (an assembly's GPU lists), turning.
  engine.postMessage({ t: "mesh", handle: 2, bytes: disc }, [disc]);
  seen = 3;
  const many = [];
  for (let k = 0; k < 10_000; k++) {
    const q = new THREE.Quaternion().setFromEuler(new THREE.Euler(k * 0.37, k * 0.11, 0));
    many.push({ id: k, mesh: k % 2 ? 1 : 2, role: "part", color: k % 3 ? "#c9ccd1" : "#7d93ad", placement: { position: [(k % 25) * 45, (Math.floor(k / 25) % 20) * 45, Math.floor(k / 500) * 45], rotation: [q.w, q.x, q.y, q.z] } });
  }
  cam.setView("isometric", false, 0, new THREE.Box3(new THREE.Vector3(-20, -20, -5), new THREE.Vector3(25 * 45, 20 * 45, 20 * 45)));
  const tBig = performance.now();
  await drawnSeq(frame(cam, many, { lod: 10 }));
  out.instancesFirstFrameMs = round(performance.now() - tBig);
  out.instances = await turn(60, many);
  const st = (await c.command("view_stats")) as { instances: number } | null;
  out.checks.tenThousandInstances = st?.instances === 10_000;

  out.ok = Object.values(out.checks).every(Boolean);
  // The smoke's second screenshot: the part again.
  (window as unknown as { __webRenderShowPart?: () => Promise<void> }).__webRenderShowPart = async () => {
    cam.setView("isometric", false, 0, new THREE.Box3(new THREE.Vector3(0, 0, 0), new THREE.Vector3(20, 10, 5)));
    await drawnSeq(frame(cam, part, { hover: { k: "face", body: 0, index: click.hit?.index ?? 0 } }));
    await new Promise((r) => setTimeout(r, 200));
  };
}

main()
  .catch((e) => {
    out.error = String((e as Error)?.stack ?? e);
  })
  .finally(() => {
    (window as unknown as { __webRenderResult?: Result }).__webRenderResult = out;
  });
