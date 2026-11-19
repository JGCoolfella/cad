//! Reference geometry: resolving picked references (`GeomRef`, `PlaneRef`) to points,
//! directions, axes and planes, and the reference plane / axis / point / coordinate
//! system features (`resolvePlane`, `refPlaneFrame`, `refAxisDef`, ... in `engine.ts`).
//!
//! A reference to a face, edge or vertex is resolved to the live body first
//! (`docs/OVERHAUL_PLAN.md` section 3.3); points, directions and planes are taken from
//! what it resolves to, never from the stored signature. The signature's type (`PLANE`,
//! `LINE`, `CIRCLE`) still says what kind of reference was picked.

use oc_doc::{CoordSysFeature, EdgeSig, FaceSig, GeomRef, PlaneRef, RefPlaneFeature};
use oc_kernel::{Curve, EdgeInfo, FaceInfo, Kernel};

use crate::eval::{AxisDef, Ctx, FResult};
use crate::math::{frame_for_plane, offset_frame, rotate_vec, std_frame, to_world, v3, Frame, P3};
use crate::resolve::missing;
use crate::sketch::geo::Kind;

/// A plane as origin and unit normal.
#[derive(Clone, Copy, Debug)]
pub struct PlaneDef {
    pub o: P3,
    pub n: P3,
}

pub fn resolve_plane<K: Kernel>(c: &Ctx<'_, K>, r: &PlaneRef) -> FResult<Frame> {
    resolve_plane_as(c, r, "the plane's face")
}

/// [`resolve_plane`], naming the face in the error when it is missing.
pub fn resolve_plane_as<K: Kernel>(c: &Ctx<'_, K>, r: &PlaneRef, what: &str) -> FResult<Frame> {
    match r {
        PlaneRef::Std { plane } => Ok(std_frame(*plane)),
        PlaneRef::RefPlane { feature_id } => c.node(feature_id.as_str()).and_then(|n| n.plane).ok_or_else(|| "Reference plane is missing".into()),
        PlaneRef::Face { sig } => {
            let f = live_face(c, sig, what)?;
            Ok(frame_for_plane(f.centroid, f.normal))
        }
        // Another component's face, in this part's coordinates (G1d): as it was last updated.
        PlaneRef::External { sig, .. } => Ok(frame_for_plane(sig.center, sig.normal)),
    }
}

/// The live face a reference resolves to.
fn live_face<K: Kernel>(c: &Ctx<'_, K>, sig: &FaceSig, what: &str) -> FResult<FaceInfo> {
    let (s, f) = c.find_face(sig)?.ok_or_else(|| missing(what))?;
    Ok(c.topo(&s)?.faces[f.index()].clone())
}

/// The live edge a reference resolves to, with its start and end swapped if needed so it
/// runs the way the picked edge did (an edge has no direction of its own; the picked one
/// is what "Reverse Direction" is relative to).
fn live_edge<K: Kernel>(c: &Ctx<'_, K>, sig: &EdgeSig, what: &str) -> FResult<EdgeInfo> {
    let (s, e) = c.find_edge(sig)?.ok_or_else(|| missing(what))?;
    let mut e = c.topo(&s)?.edges[e.index()].clone();
    if v3::dot(v3::sub(e.end, e.start), v3::sub(sig.b, sig.a)) < 0.0 {
        std::mem::swap(&mut e.start, &mut e.end);
    }
    Ok(e)
}

fn is_plane(r: &GeomRef) -> bool {
    matches!(r, GeomRef::Plane { .. }) || matches!(r, GeomRef::Face { sig } if sig.surface == "PLANE")
}
fn is_line(r: &GeomRef) -> bool {
    matches!(r, GeomRef::Edge { sig } if sig.curve == "LINE") || matches!(r, GeomRef::Axis { .. } | GeomRef::SketchLine { .. })
}
fn is_point(r: &GeomRef) -> bool {
    matches!(r, GeomRef::Vertex { .. } | GeomRef::Point { .. } | GeomRef::Origin)
}

/// A point from a reference: vertex, reference point, arc centre, face centre.
pub fn point_of<K: Kernel>(c: &Ctx<'_, K>, r: &GeomRef) -> FResult<P3> {
    match r {
        GeomRef::Vertex { p, name } => Ok(c.find_vertex(*p, name.as_ref())?.ok_or_else(|| missing("vertex"))?.0),
        GeomRef::Origin => Ok([0.0; 3]),
        GeomRef::Point { feature_id } => c.node(feature_id.as_str()).and_then(|n| n.point).ok_or_else(|| "The reference point no longer exists".into()),
        GeomRef::Edge { sig } if sig.curve == "CIRCLE" => Ok(axis_of(c, Some(r))?.p),
        GeomRef::Edge { sig } => Ok(live_edge(c, sig, "edge")?.mid),
        GeomRef::Face { sig } => Ok(live_face(c, sig, "face")?.centroid),
        GeomRef::Plane { plane } => Ok(resolve_plane(c, plane)?.origin),
        _ => Ok(axis_of(c, Some(r))?.p),
    }
}

/// Plane (origin + unit normal) of a reference: planar face, standard or reference plane.
pub fn plane_of<K: Kernel>(c: &Ctx<'_, K>, r: Option<&GeomRef>) -> FResult<PlaneDef> {
    match r {
        None => Err("Select a plane or planar face to mirror about".into()),
        Some(GeomRef::Plane { plane }) => {
            let f = resolve_plane(c, plane)?;
            Ok(PlaneDef { o: f.origin, n: f.normal })
        }
        Some(GeomRef::Face { sig }) => {
            let f = live_face(c, sig, "face")?;
            Ok(PlaneDef { o: f.centroid, n: f.normal })
        }
        Some(_) => Err("Select a plane or planar face".into()),
    }
}

/// Unit direction of a reference (linear edge, plane/face normal, sketch line).
pub fn direction_of<K: Kernel>(c: &Ctx<'_, K>, r: Option<&GeomRef>) -> FResult<P3> {
    match r {
        None => Err("Select a direction (a linear edge, face, plane or sketch line)".into()),
        Some(GeomRef::Axis { .. }) => Ok(axis_of(c, r)?.d),
        Some(GeomRef::Vertex { .. } | GeomRef::Point { .. } | GeomRef::Origin) => Err("A point does not define a direction".into()),
        Some(GeomRef::Edge { sig }) => {
            let e = live_edge(c, sig, "direction edge")?;
            Ok(v3::norm(v3::sub(e.end, e.start)))
        }
        Some(GeomRef::Face { sig }) => Ok(live_face(c, sig, "direction face")?.normal),
        Some(GeomRef::Plane { plane }) => Ok(resolve_plane(c, plane)?.normal),
        Some(_) => Ok(axis_of(c, r)?.d),
    }
}

/// Centre and axis of a circular edge (the circle's own axis, as the TypeScript engine
/// derives it from three points in parameter order).
fn circle_axis(e: &EdgeInfo) -> FResult<AxisDef> {
    match e.curve {
        Curve::Circle { center, normal, .. } => Ok(AxisDef { p: center, d: v3::norm(normal) }),
        _ => Err("The edge is not circular".into()),
    }
}

/// Axis (point + unit direction) of a reference.
pub fn axis_of<K: Kernel>(c: &Ctx<'_, K>, r: Option<&GeomRef>) -> FResult<AxisDef> {
    let r = r.ok_or("Select an axis (a circular or straight edge, a cylindrical face or a sketch line)")?;
    match r {
        GeomRef::Axis { feature_id } => c.node(feature_id.as_str()).and_then(|n| n.axis).ok_or_else(|| "The reference axis no longer exists".into()),
        GeomRef::Vertex { .. } | GeomRef::Point { .. } | GeomRef::Origin => Err("A point does not define an axis".into()),
        GeomRef::SketchLine { sketch_id, entity } => {
            let (sk, g) = c.sketch(sketch_id.as_str())?;
            let frame = c.frame(sk.id.as_str());
            let (Some(e), Some(frame)) = (g.entities.get(entity.as_str()), frame) else {
                return Err("The axis line could not be found".into());
            };
            let Kind::Line { p1, p2 } = &e.kind else { return Err("The axis line could not be found".into()) };
            let a = to_world(&frame, g.p(p1));
            let b = to_world(&frame, g.p(p2));
            Ok(AxisDef { p: a, d: v3::norm(v3::sub(b, a)) })
        }
        GeomRef::Plane { plane } => {
            let f = resolve_plane(c, plane)?;
            Ok(AxisDef { p: f.origin, d: f.normal })
        }
        GeomRef::Edge { sig } if sig.curve == "LINE" => {
            let e = live_edge(c, sig, "axis edge")?;
            Ok(AxisDef { p: e.start, d: v3::norm(v3::sub(e.end, e.start)) })
        }
        GeomRef::Edge { sig } => circle_axis(&live_edge(c, sig, "axis edge")?),
        GeomRef::Face { sig } => {
            let (s, f) = c.find_face(sig)?.ok_or_else(|| missing("axis face"))?;
            let t = c.topo(&s)?;
            let face = &t.faces[f.index()];
            let circle = face.edges.iter().map(|e| &t.edges[e.index()]).find(|e| matches!(e.curve, Curve::Circle { .. }));
            match circle {
                Some(e) => circle_axis(e),
                None => Ok(AxisDef { p: face.centroid, d: face.normal }),
            }
        }
    }
}

/// Reference plane from its references; the plane type follows from what was picked.
pub fn ref_plane<K: Kernel>(c: &Ctx<'_, K>, f: &RefPlaneFeature) -> FResult<Frame> {
    let refs = match &f.refs {
        Some(r) if !r.is_empty() => r,
        _ => return Ok(offset_frame(&resolve_plane(c, &f.base)?, f.offset, f.flip)),
    };
    let planes: Vec<&GeomRef> = refs.iter().filter(|r| is_plane(r)).collect();
    let lines: Vec<&GeomRef> = refs.iter().filter(|r| is_line(r)).collect();
    let pts: Vec<&GeomRef> = refs.iter().filter(|r| is_point(r)).collect();
    let curves: Vec<&GeomRef> = refs.iter().filter(|r| matches!(r, GeomRef::Edge { sig } if sig.curve != "LINE")).collect();
    let flip = |n: P3| if f.flip { v3::neg(n) } else { n };
    if pts.len() >= 3 {
        let a = point_of(c, pts[0])?;
        let b = point_of(c, pts[1])?;
        let p = point_of(c, pts[2])?;
        let n = v3::norm(v3::cross(v3::sub(b, a), v3::sub(p, a)));
        if v3::len(n) < 1e-9 {
            return Err("The three points are on one line".into());
        }
        return Ok(frame_for_plane(a, flip(n)));
    }
    if planes.len() >= 2 {
        // Mid plane between two (parallel) planes or faces.
        let a = plane_of(c, Some(planes[0]))?;
        let b = plane_of(c, Some(planes[1]))?;
        let nb = if v3::dot(a.n, b.n) < 0.0 { v3::neg(b.n) } else { b.n };
        let n = v3::norm(v3::add(a.n, nb));
        // Halfway between the planes. (The TypeScript engine averages A's origin with B's
        // origin projected onto A, which lands on plane A: a bug, section 8.)
        let o = v3::add(b.o, v3::scale(n, v3::dot(v3::sub(a.o, b.o), n) / 2.0));
        return Ok(frame_for_plane(o, flip(n)));
    }
    if planes.len() == 1 && !lines.is_empty() {
        // At angle: rotate the plane about the line.
        let p = plane_of(c, Some(planes[0]))?;
        let l = axis_of(c, Some(lines[0]))?;
        let ang = f.angle.unwrap_or(0.0) * if f.flip { -1.0 } else { 1.0 };
        return Ok(frame_for_plane(l.p, rotate_vec(p.n, l.d, ang)));
    }
    if planes.len() == 1 && !pts.is_empty() {
        let p = plane_of(c, Some(planes[0]))?;
        return Ok(frame_for_plane(point_of(c, pts[0])?, flip(p.n)));
    }
    if (!curves.is_empty() || !lines.is_empty()) && !pts.is_empty() {
        // Normal to a curve at a point.
        let e = curves.first().or(lines.first()).copied().expect("checked above");
        let p = point_of(c, pts[0])?;
        let t = match e {
            GeomRef::Edge { sig } if sig.curve == "CIRCLE" => {
                let ax = axis_of(c, Some(e))?;
                v3::norm(v3::cross(ax.d, v3::sub(p, ax.p)))
            }
            _ => axis_of(c, Some(e))?.d,
        };
        return Ok(frame_for_plane(p, flip(t)));
    }
    if planes.len() == 1 {
        let p = plane_of(c, Some(planes[0]))?;
        return Ok(offset_frame(&frame_for_plane(p.o, p.n), f.offset, f.flip));
    }
    Err("Select references that define a plane (a plane or face, plus a line, point or second plane)".into())
}

/// Reference axis: a line or edge, two planes, two points, a cylindrical face, or a point
/// and a plane.
pub fn ref_axis<K: Kernel>(c: &Ctx<'_, K>, refs: &[GeomRef]) -> FResult<AxisDef> {
    let planes: Vec<&GeomRef> = refs.iter().filter(|r| is_plane(r)).collect();
    let pts: Vec<&GeomRef> = refs.iter().filter(|r| is_point(r)).collect();
    if planes.len() >= 2 {
        let a = plane_of(c, Some(planes[0]))?;
        let b = plane_of(c, Some(planes[1]))?;
        let d = v3::norm(v3::cross(a.n, b.n));
        if v3::len(d) < 1e-9 {
            return Err("The planes are parallel".into());
        }
        // A point on both planes, in the plane spanned by the normals.
        let (h1, h2) = (v3::dot(a.n, a.o), v3::dot(b.n, b.o));
        let k = v3::dot(a.n, b.n);
        let det = 1.0 - k * k;
        let (c1, c2) = ((h1 - h2 * k) / det, (h2 - h1 * k) / det);
        return Ok(AxisDef { p: v3::add(v3::scale(a.n, c1), v3::scale(b.n, c2)), d });
    }
    if pts.len() >= 2 {
        let a = point_of(c, pts[0])?;
        let b = point_of(c, pts[1])?;
        return Ok(AxisDef { p: a, d: v3::norm(v3::sub(b, a)) });
    }
    if planes.len() == 1 && pts.len() == 1 {
        return Ok(AxisDef { p: point_of(c, pts[0])?, d: plane_of(c, Some(planes[0]))?.n });
    }
    match refs.first() {
        Some(r) => axis_of(c, Some(r)),
        None => Err("Select a line or edge, two planes, two points, or a cylindrical face".into()),
    }
}

pub fn ref_point<K: Kernel>(c: &Ctx<'_, K>, refs: &[GeomRef], xyz: Option<P3>) -> FResult<P3> {
    match refs.first() {
        None => Ok(xyz.unwrap_or([0.0; 3])),
        Some(r) => point_of(c, r),
    }
}

pub fn coord_sys<K: Kernel>(c: &Ctx<'_, K>, f: &CoordSysFeature) -> FResult<Frame> {
    let o = match &f.origin {
        Some(r) => point_of(c, r)?,
        None => [0.0; 3],
    };
    let x = match &f.x_axis {
        Some(r) => direction_of(c, Some(r))?,
        None => [1.0, 0.0, 0.0],
    };
    let mut y = match &f.y_axis {
        Some(r) => direction_of(c, Some(r))?,
        None => v3::norm(v3::cross([0.0, 0.0, 1.0], x)),
    };
    if v3::len(y) < 1e-9 {
        y = [0.0, 1.0, 0.0];
    }
    // Y perpendicular to X; Z completes a right-handed frame.
    let y = v3::norm(v3::sub(y, v3::scale(x, v3::dot(y, x))));
    let xn = v3::norm(x);
    Ok(Frame { origin: o, x: xn, y, normal: v3::cross(xn, y) })
}
