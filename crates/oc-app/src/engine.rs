//! The engine thread: owns the OCCT kernel and the `oc-graph` regenerator (neither is
//! `Send`), serves the requests `src/kernel/client.ts` sent to the WebAssembly worker, and
//! hands display meshes to the renderer through the [`MeshStore`].
//!
//! Every request carries the feature list, as the worker protocol does; the regenerator's
//! cache makes repeated lists cheap.
use crate::bodymesh;
use crate::pack::Packer;
use oc_doc::{EdgeSig, FaceSig, Feature, RefSlot};
use oc_graph::{CancelToken, Regenerator, Shape};
use oc_kernel::{BooleanOp, Curve, EdgeId, Kernel, MeshParams, Target, Topology, P3};
use oc_kernel_occt::{Body, OcctKernel};
use oc_mesh::{MeshOptions, RenderMesh};
use oc_naming::{resolve, Live, Rules};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Display meshes made by the engine, by handle, until the renderer drops them.
#[derive(Default)]
pub struct MeshStore {
    pub meshes: Mutex<HashMap<u32, Arc<RenderMesh>>>,
    next: AtomicU32,
}

impl MeshStore {
    pub fn add(&self, m: RenderMesh) -> u32 {
        let h = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.meshes.lock().expect("mesh store").insert(h, Arc::new(m));
        h
    }
}

pub type Reply = Result<Value, String>;
pub type BytesReply = Result<Vec<u8>, String>;

/// Measure tool entity (`MeasureRef` in `src/kernel/protocol.ts`).
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum MeasureRef {
    Face { sig: FaceSig },
    Edge { sig: EdgeSig },
    Vertex { p: P3 },
}

#[derive(Deserialize, Clone, Copy)]
pub struct Frame {
    pub origin: P3,
    pub x: P3,
    pub y: P3,
    pub normal: P3,
}

pub enum Job {
    Rebuild { features: Vec<Arc<Feature>>, preview: Option<Arc<Feature>>, quality: f64, reply: Sender<BytesReply> },
    Export { features: Vec<Arc<Feature>>, format: String, quality: f64, reply: Sender<BytesReply> },
    MassProps { features: Vec<Arc<Feature>>, reply: Sender<Reply> },
    Measure { features: Vec<Arc<Feature>>, refs: Vec<MeasureRef>, reply: Sender<Reply> },
    Convert { features: Vec<Arc<Feature>>, frame: Frame, edges: Vec<EdgeSig>, faces: Vec<FaceSig>, reply: Sender<Reply> },
    ClearCache,
}

/// Starts the engine thread; jobs run one at a time in arrival order.
pub fn spawn(store: Arc<MeshStore>) -> Sender<Job> {
    let (tx, rx) = channel::<Job>();
    std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || {
            let mut e = Engine { regen: Regenerator::new(OcctKernel::default()), store };
            for job in rx {
                e.run(job);
            }
        })
        .expect("engine thread");
    tx
}

struct Engine {
    regen: Regenerator<OcctKernel>,
    store: Arc<MeshStore>,
}

type Sh = Rc<Shape<Body>>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Feature types whose preview is the part before the feature plus the body it adds or
/// removes (`rebuild` in `src/kernel/engine.ts`).
const TOOL_PREVIEWS: [&str; 9] = ["extrude", "revolve", "sweep", "loft", "hole", "rib", "linearPattern", "circularPattern", "mirror"];

impl Engine {
    fn run(&mut self, job: Job) {
        // A panic in one request must not take the engine down with it.
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match job {
            Job::Rebuild { features, preview, quality, reply } => {
                let _ = reply.send(self.rebuild(&features, preview, quality));
            }
            Job::Export { features, format, quality, reply } => {
                let _ = reply.send(self.export(&features, &format, quality));
            }
            Job::MassProps { features, reply } => {
                let _ = reply.send(self.mass_props(&features));
            }
            Job::Measure { features, refs, reply } => {
                let _ = reply.send(self.measure(&features, &refs));
            }
            Job::Convert { features, frame, edges, faces, reply } => {
                let _ = reply.send(self.convert(&features, frame, &edges, &faces));
            }
            Job::ClearCache => self.regen.clear_cache(),
        }));
        if r.is_err() {
            eprintln!("engine: request panicked; cache cleared");
            self.regen.clear_cache();
        }
    }

    fn final_body(&mut self, features: &[Arc<Feature>]) -> Result<Option<Sh>, String> {
        let r = self.regen.regenerate(features, &CancelToken::new()).map_err(|_| "cancelled".to_string())?;
        Ok(r.body)
    }

    fn mesh(&self, body: &Body, quality: f64) -> Result<RenderMesh, String> {
        oc_mesh::build(self.regen.kernel(), body, &MeshOptions { quality, max_lods: 4, threads: 0 }).map_err(err)
    }

    fn rebuild(&mut self, features: &[Arc<Feature>], preview: Option<Arc<Feature>>, quality: f64) -> BytesReply {
        let t0 = Instant::now();
        let mut all = features.to_vec();
        all.extend(preview.iter().cloned());
        let r = self.regen.regenerate(&all, &CancelToken::new()).map_err(|_| "cancelled".to_string())?;
        let k = self.regen.kernel();
        let order: HashMap<&str, usize> = all.iter().enumerate().map(|(i, f)| (f.id.as_str(), i)).collect();
        let order_fn = |id: &str| order.get(id).copied();

        // Preview: the part before the feature and the body it adds (or, for a cut, the
        // part of the old body it removes).
        let mut base = r.body.clone();
        let mut tool: Option<Body> = None;
        let mut instances = Value::Null;
        if let Some(p) = &preview {
            let i = all.len() - 1;
            let pv = serde_json::to_value(&**p).map_err(err)?;
            let ty = pv["type"].as_str().unwrap_or("");
            if let Some(x) = r.instances.get(p.id.as_str()) {
                instances = Value::Array(x.iter().map(|x| json!({ "key": x.key, "p": x.p, "skipped": x.skipped })).collect());
            }
            let out = r.outs.get(p.id.as_str());
            let tools = out.and_then(|o| o.tools.as_ref()).filter(|t| !t.is_empty());
            if let (true, true, Some(tools)) = (r.steps[i].status.error.is_none(), TOOL_PREVIEWS.contains(&ty), tools) {
                let before = if i > 0 { r.steps[i - 1].body.clone() } else { None };
                let cut = match pv.get("cut") {
                    Some(c) => c.as_bool().unwrap_or(false),
                    None if ty.ends_with("Pattern") || ty == "mirror" => tools.iter().all(|t| t.cut),
                    None => false,
                };
                let bodies: Vec<&Body> = tools.iter().map(|t| &t.shape.body).collect();
                if let Ok(c) = k.compound(&bodies) {
                    let mut t = c.body;
                    if let (true, Some(b)) = (cut, &before) {
                        if let Ok(x) = k.boolean(BooleanOp::Common, &b.body, &[&t]) {
                            t = x.body;
                        }
                    }
                    tool = Some(t);
                    base = before;
                }
            }
        }

        let mut packer = Packer::default();
        let mesh = match &base {
            Some(s) => {
                let topo = s.topo(k).map_err(|e| e.message.clone())?;
                let rm = self.mesh(&s.body, quality)?;
                let h = self.store.add(rm.clone());
                bodymesh::encode(&mut packer, bodymesh::Input { handle: h, mesh: &rm, topo, names: Some(&s.names), order: &order_fn })
            }
            None => Value::Null,
        };
        let tool_mesh = match &tool {
            Some(b) => match (k.topology(b), self.mesh(b, quality)) {
                (Ok(topo), Ok(rm)) if !topo.faces.is_empty() => {
                    let h = self.store.add(rm.clone());
                    bodymesh::encode(&mut packer, bodymesh::Input { handle: h, mesh: &rm, topo: &topo, names: None, order: &order_fn })
                }
                _ => Value::Null,
            },
            None => Value::Null,
        };
        let frame = |f: &oc_graph::math::Frame| json!({ "origin": f.origin, "x": f.x, "y": f.y, "normal": f.normal });
        let statuses: Vec<Value> = r
            .steps
            .iter()
            .map(|s| {
                let mut v = json!({ "id": s.status.id });
                if let Some(e) = &s.status.error {
                    v["error"] = json!(e);
                }
                if let Some(w) = &s.status.warning {
                    v["warning"] = json!(w);
                }
                v
            })
            .collect();
        let missing = missing_refs(k, &all, &r.steps);
        let heals: Vec<Value> = r.heals.iter().map(|h| json!({ "id": h.to.id.as_str(), "from": &*h.from, "feature": &*h.to })).collect();
        let header = json!({
            "mesh": mesh,
            "toolMesh": tool_mesh,
            "statuses": statuses,
            "frames": r.frames.iter().map(|(k, f)| (k.clone(), frame(f))).collect::<serde_json::Map<_, _>>(),
            "planes": r.planes.iter().map(|(k, f)| (k.clone(), json!({ "frame": frame(f), "size": 60 }))).collect::<serde_json::Map<_, _>>(),
            "axes": r.axes.iter().map(|(k, a)| (k.clone(), json!({ "p": a.p, "d": a.d }))).collect::<serde_json::Map<_, _>>(),
            "points": r.points.iter().map(|(k, p)| (k.clone(), json!(p))).collect::<serde_json::Map<_, _>>(),
            "csys": r.csys.iter().map(|(k, f)| (k.clone(), frame(f))).collect::<serde_json::Map<_, _>>(),
            "sketches": r.sketches.iter().map(|(k, s)| (k.clone(), json!(&**s))).collect::<serde_json::Map<_, _>>(),
            "instances": instances,
            "heals": heals,
            "missing": missing,
            "untracked": r.untracked,
            "timeMs": t0.elapsed().as_secs_f64() * 1e3,
        });
        Ok(packer.finish(&header))
    }

    fn export(&mut self, features: &[Arc<Feature>], format: &str, quality: f64) -> BytesReply {
        let body = self.final_body(features)?.ok_or("Nothing to export")?;
        let k = self.regen.kernel();
        match format {
            "step" => k.write_step(&[&body.body]).map_err(err),
            "stl" => {
                let size = body.bbox(k).map(|b| b.diagonal()).unwrap_or(1.0);
                let m = k.tessellate(&body.body, &k.tolerances().display_mesh(quality, size)).map_err(err)?;
                Ok(stl(&m.positions, &m.triangles))
            }
            f => Err(format!("unknown export format {f}")),
        }
    }

    fn mass_props(&mut self, features: &[Arc<Feature>]) -> Reply {
        let Some(body) = self.final_body(features)? else { return Ok(Value::Null) };
        let m = self.regen.kernel().mass_properties(&body.body).map_err(err)?;
        Ok(json!({ "volume": m.volume, "area": m.area, "centerOfMass": m.center }))
    }

    fn measure(&mut self, features: &[Arc<Feature>], refs: &[MeasureRef]) -> Reply {
        let body = self.final_body(features)?;
        let k = self.regen.kernel();
        let rules = Rules::default();
        let topo = match &body {
            Some(b) => Some(b.topo(k).map_err(|e| e.message.clone())?),
            None => None,
        };
        let mut targets = vec![];
        for r in refs.iter().take(2) {
            let t = match (r, &body, topo) {
                (MeasureRef::Vertex { p }, _, _) => Target::Point(*p),
                (MeasureRef::Face { sig }, Some(b), Some(t)) => match resolve::face(&Live::new(t, &b.names), sig, &rules) {
                    Some(f) => Target::Face(&b.body, f.id),
                    None => return Ok(Value::Null),
                },
                (MeasureRef::Edge { sig }, Some(b), Some(t)) => match resolve::edge(&Live::new(t, &b.names), sig, &rules) {
                    Some(e) => Target::Edge(&b.body, e.id),
                    None => return Ok(Value::Null),
                },
                _ => return Ok(Value::Null),
            };
            targets.push(t);
        }
        let mut it = targets.into_iter();
        let (Some(a), Some(b)) = (it.next(), it.next()) else { return Ok(Value::Null) };
        let d = k.distance(a, b).map_err(err)?;
        Ok(json!({ "distance": d.distance, "p1": d.a, "p2": d.b }))
    }

    /// Convert Entities: model edges (and the boundary edges of faces) projected onto the
    /// sketch plane, in sketch coordinates (`ConvertedCurve`).
    fn convert(&mut self, features: &[Arc<Feature>], frame: Frame, edges: &[EdgeSig], faces: &[FaceSig]) -> Reply {
        let Some(body) = self.final_body(features)? else { return Ok(json!([])) };
        let k = self.regen.kernel();
        let topo = body.topo(k).map_err(|e| e.message.clone())?;
        let live = Live::new(topo, &body.names);
        let rules = Rules::default();
        let mut ids: Vec<EdgeId> = vec![];
        for s in edges {
            if let Some(e) = resolve::edge(&live, s, &rules) {
                ids.push(e.id);
            }
        }
        for s in faces {
            if let Some(f) = resolve::face(&live, s, &rules) {
                ids.extend(topo.faces[f.id.index()].edges.iter().copied());
            }
        }
        let mut seen = std::collections::HashSet::new();
        ids.retain(|e| seen.insert(*e) && !topo.edges[e.index()].seam);
        let mut polylines: Option<Vec<Vec<P3>>> = None;
        let mut out = vec![];
        for e in ids {
            match curve_2d(topo, e, &frame) {
                Some(c) => out.push(c),
                None => {
                    if polylines.is_none() {
                        let size = body.bbox(k).map(|b| b.diagonal()).unwrap_or(1.0);
                        let p: MeshParams = k.tolerances().display_mesh(3.0, size);
                        polylines = Some(k.tessellate(&body.body, &p).map_err(err)?.edges);
                    }
                    let pts = polylines.as_ref().and_then(|p| p.get(e.index())).cloned().unwrap_or_default();
                    if pts.len() > 1 {
                        out.push(json!({ "kind": "poly", "pts": pts.iter().map(|p| to_frame(&frame, *p)).collect::<Vec<_>>() }));
                    }
                }
            }
        }
        Ok(Value::Array(out))
    }
}

/// References of features with an error or warning that do not resolve on the body before them, per
/// feature (the stored signatures, or `{p}` for vertices): the UI shows them as Missing
/// in the feature's selection boxes (plan section 3.3, step 3).
fn missing_refs(k: &OcctKernel, all: &[Arc<Feature>], steps: &[oc_graph::Step<Body>]) -> serde_json::Map<String, Value> {
    let rules = Rules::default();
    let mut out = serde_json::Map::new();
    for (i, s) in steps.iter().enumerate() {
        if s.status.error.is_none() && s.status.warning.is_none() {
            continue;
        }
        let before = if i > 0 { steps[i - 1].body.clone() } else { None };
        let topo = before.as_ref().and_then(|b| b.topo(k).ok());
        let mut f = (*all[i]).clone();
        let mut list = vec![];
        f.refs_mut(&mut |slot| {
            let found = match (&before, topo) {
                (Some(b), Some(t)) => {
                    let live = Live::new(t, &b.names);
                    match &slot {
                        RefSlot::Face(sig) => resolve::face(&live, sig, &rules).is_some(),
                        RefSlot::Edge(sig) => resolve::edge(&live, sig, &rules).is_some(),
                        RefSlot::Vertex { p, name } => resolve::vertex(&live, **p, name.as_deref(), &rules).is_some(),
                    }
                }
                _ => false,
            };
            if !found {
                list.push(match slot {
                    RefSlot::Face(sig) => json!(sig),
                    RefSlot::Edge(sig) => json!(sig),
                    RefSlot::Vertex { p, .. } => json!({ "p": p }),
                });
            }
        });
        if !list.is_empty() {
            out.insert(s.status.id.clone(), Value::Array(list));
        }
    }
    out
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn to_frame(f: &Frame, p: P3) -> [f64; 2] {
    let d = sub(p, f.origin);
    [dot(d, f.x), dot(d, f.y)]
}

/// Lines, and circles and arcs in a plane parallel to the sketch, map to sketch entities;
/// anything else is sampled (`None`).
fn curve_2d(t: &Topology, e: EdgeId, f: &Frame) -> Option<Value> {
    let x = &t.edges[e.index()];
    match x.curve {
        Curve::Line => Some(json!({ "kind": "line", "a": to_frame(f, x.start), "b": to_frame(f, x.end) })),
        Curve::Circle { center, normal, radius } if dot(normal, f.normal).abs() > 1.0 - 1e-9 => {
            let closed = dot(sub(x.start, x.end), sub(x.start, x.end)).sqrt() < 1e-7;
            Some(if closed {
                json!({ "kind": "circle", "c": to_frame(f, center), "r": radius })
            } else {
                json!({ "kind": "arc", "a": to_frame(f, x.start), "m": to_frame(f, x.mid), "b": to_frame(f, x.end) })
            })
        }
        _ => None,
    }
}

/// Binary STL.
fn stl(pos: &[P3], tris: &[[u32; 3]]) -> Vec<u8> {
    let mut out = vec![0u8; 80];
    out[..14].copy_from_slice(b"OpenCAD binary");
    out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for t in tris {
        let [a, b, c] = t.map(|i| pos[i as usize]);
        let (u, v) = (sub(b, a), sub(c, a));
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = dot(n, n).sqrt().max(1e-300);
        for x in [n.map(|q| q / l), a, b, c] {
            for q in x {
                out.extend_from_slice(&(q as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    out
}
