//! Extruded Boss/Base and Extruded Cut: every end condition, From (offset, surface),
//! Direction 2, draft, Thin Feature and Selected Contours (`extrude` in `engine.ts`).

use oc_doc::{EndCondition, EndConditionType, ExtrudeFeature, ExtrudeFromType};
use oc_kernel::{BooleanOp, Kernel, PrismSpec, Profile, Region, Target, Taper};

use super::fuse_all;
use crate::eval::{Ctx, FResult, NodeOut, Tool};
use crate::math::{frame_for_plane, to_world, v3, P3};
use crate::refgeom;
use crate::sketch::{profile, profile_regions, region_world};

#[derive(Clone, Copy)]
struct Ex {
    /// Start plane origin (after From).
    start: P3,
    big: f64,
    cut: bool,
}

/// One profile region, in world coordinates, with its area centroid.
struct Face {
    region: Region,
    center: P3,
    /// A point on the outer boundary.
    corner: P3,
}

fn area_centroid(r: &profile::Region) -> [f64; 2] {
    let one = |poly: &[[f64; 2]]| {
        let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
        let n = poly.len();
        for i in 0..n {
            let (p, q) = (poly[i], poly[(i + 1) % n]);
            let w = p[0] * q[1] - q[0] * p[1];
            a += w;
            cx += (p[0] + q[0]) * w;
            cy += (p[1] + q[1]) * w;
        }
        (a / 2.0, cx / 6.0, cy / 6.0)
    };
    let (mut a, mut x, mut y) = one(&r.outer.poly);
    for h in &r.holes {
        let (ha, hx, hy) = one(&h.poly);
        // Holes run either way; subtract their absolute contribution.
        let s = if (ha > 0.0) == (a > 0.0) { 1.0 } else { -1.0 };
        a -= s * ha;
        x -= s * hx;
        y -= s * hy;
    }
    if a.abs() < 1e-300 {
        return r.outer.poly[0];
    }
    [x / a, y / a]
}

fn translate(r: &Region, v: P3) -> Region {
    super::transform_region(r, &oc_kernel::Transform::translation(v))
}

pub fn extrude<K: Kernel>(c: &Ctx<'_, K>, f: &ExtrudeFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    let (sk, g) = c.sketch(f.sketch_id.as_str())?;
    let frame = c.frame(sk.id.as_str()).ok_or("Sketch plane unresolved")?;
    let n = frame.normal;
    // Cuts go away from the material by default (into the part), like SOLIDWORKS.
    let sign = if f.reverse { -1.0 } else { 1.0 } * if f.cut { -1.0 } else { 1.0 };
    let dir = v3::scale(n, sign);
    let mut shift = 0.0;
    match &f.from {
        Some(fr) if fr.kind == ExtrudeFromType::Offset => {
            shift = fr.offset.unwrap_or(0.0) * if fr.reverse == Some(true) { -1.0 } else { 1.0 };
        }
        Some(fr) if fr.kind == ExtrudeFromType::Surface => {
            let pl = refgeom::plane_of(c, fr.r#ref.as_ref().and_then(|r| r.as_ref()))?;
            let denom = v3::dot(n, pl.n);
            if denom.abs() < 1e-6 {
                return Err("The From surface is perpendicular to the sketch".into());
            }
            shift = v3::dot(v3::sub(pl.o, frame.origin), pl.n) / denom;
        }
        _ => {}
    }
    let regions = profile_regions(&g, f.thin.as_ref(), f.contours.as_deref())?;
    let moved = shift.abs() > 1e-12;
    let faces: Vec<Face> = regions
        .iter()
        .map(|r| {
            let mut region = region_world(r, &frame);
            let mut center = to_world(&frame, area_centroid(r));
            let mut corner = to_world(&frame, r.outer.poly[0]);
            if moved {
                let v = v3::scale(n, shift);
                region = translate(&region, v);
                center = v3::add(center, v);
                corner = v3::add(corner, v);
            }
            Face { region, center, corner }
        })
        .collect();
    let ex = Ex { start: v3::add(frame.origin, v3::scale(n, shift)), big: c.big_depth(&frame)?, cut: f.cut };
    let mut solids = vec![];
    for face in &faces {
        if f.end1.kind == EndConditionType::MidPlane {
            let d = f.end1.depth;
            let back = v3::scale(dir, -d / 2.0);
            let moved = Face { region: translate(&face.region, back), center: v3::add(face.center, back), corner: v3::add(face.corner, back) };
            let ec = EndCondition { kind: EndConditionType::Blind, ..f.end1.clone() };
            solids.push(one(c, &moved, dir, &ec, &Ex { start: v3::add(ex.start, back), ..ex })?);
        } else {
            solids.push(one(c, face, dir, &f.end1, &ex)?);
            let both = if f.end1.kind == EndConditionType::ThroughAllBoth {
                Some(EndCondition { kind: EndConditionType::ThroughAll, ..EndCondition::blind(0.0) })
            } else {
                f.dir2.clone()
            };
            if let Some(b) = both {
                solids.push(one(c, face, v3::neg(dir), &b, &ex)?);
            }
        }
    }
    let tool = fuse_all(c, solids)?.ok_or("The sketch has no closed contours")?;
    out.preview = Some(tool.clone());
    out.tools = Some(vec![Tool { body: tool.clone(), cut: f.cut }]);
    c.set_body(c.combine(&tool, f.cut, f.merge)?);
    Ok(())
}

/// Depth along `dir` for an end condition, or a clipping half-space when it ends on a
/// plane that is not perpendicular to `dir`.
fn end_depth<K: Kernel>(c: &Ctx<'_, K>, ec: &EndCondition, dir: P3, face: &Face, x: &Ex) -> FResult<(f64, Option<(P3, P3)>)> {
    use EndConditionType::*;
    match ec.kind {
        Blind | MidPlane => Ok((ec.depth, None)),
        ThroughAll | ThroughAllBoth | UpToBody => Ok((x.big, None)),
        UpToVertex => {
            let v = ec.vertex.flatten().ok_or("Select a vertex for Up To Vertex")?;
            Ok((v3::dot(v3::sub(v, x.start), dir), None))
        }
        UpToSurface | OffsetFromSurface => {
            let pl = refgeom::plane_of(c, ec.r#ref.as_ref().and_then(|r| r.as_ref()))?;
            let off = if ec.kind == OffsetFromSurface { ec.depth * if ec.reverse_offset == Some(true) { -1.0 } else { 1.0 } } else { 0.0 };
            let denom = v3::dot(dir, pl.n);
            if denom.abs() < 1e-6 {
                return Err("The surface is parallel to the extrude direction".into());
            }
            let d = v3::dot(v3::sub(pl.o, x.start), pl.n) / denom - off;
            if (denom.abs() - 1.0).abs() < 1e-9 {
                return Ok((d, None));
            }
            // Inclined plane: extrude past it and cut away what lies beyond.
            Ok((x.big, Some((v3::sub(pl.o, v3::scale(dir, off)), v3::scale(pl.n, denom.signum())))))
        }
        UpToNext => Ok((next_face_depth(c, face, dir, x)?, None)),
    }
}

/// Up To Next: the nearest planar face, parallel to the sketch, that covers the profile
/// centre. Bosses grow until they meet a face looking back at them; cuts until material ends.
fn next_face_depth<K: Kernel>(c: &Ctx<'_, K>, face: &Face, dir: P3, x: &Ex) -> FResult<f64> {
    let s = c.shape().ok_or("Up To Next needs an existing body")?;
    let t = c.topo(&s)?;
    let mut best = f64::INFINITY;
    for f in &t.faces {
        if !matches!(f.surface, oc_kernel::Surface::Plane { .. }) {
            continue;
        }
        let k = v3::dot(f.normal, dir);
        if if x.cut { k < 0.999 } else { k > -0.999 } {
            continue;
        }
        let d = v3::dot(v3::sub(f.centroid, x.start), dir);
        if d <= 1e-6 || d >= best {
            continue;
        }
        let hit = v3::add(face.center, v3::scale(dir, d));
        if (0..3).all(|i| hit[i] >= f.bbox.min[i] - 1e-4 && hit[i] <= f.bbox.max[i] + 1e-4) {
            best = d;
        }
    }
    if !best.is_finite() {
        return Err("Up To Next: no face found in the extrude direction".into());
    }
    Ok(best)
}

/// Box on the +n side of a plane (clips extrusions at inclined surfaces).
fn half_space<K: Kernel>(c: &Ctx<'_, K>, o: P3, n: P3, size: f64) -> FResult<K::Body> {
    let fr = frame_for_plane(o, n);
    let pts: Vec<P3> = [[-size, -size], [size, -size], [size, size], [-size, size]]
        .iter()
        .map(|[u, v]| v3::add(o, v3::add(v3::scale(fr.x, *u), v3::scale(fr.y, *v))))
        .collect();
    let spec = PrismSpec { profile: Profile::single(oc_kernel::Wire::polygon(&pts)), vector: v3::scale(n, size), taper: None };
    Ok(c.k.prism(&spec)?.body)
}

fn one<K: Kernel>(c: &Ctx<'_, K>, face: &Face, dir: P3, ec: &EndCondition, x: &Ex) -> FResult<K::Body> {
    let (depth, clip) = end_depth(c, ec, dir, face, x)?;
    if depth <= 1e-6 {
        return Err("The end condition lies behind the sketch plane. Try Reverse Direction.".into());
    }
    let taper = ec.draft.filter(|d| *d > 1e-9).map(|angle| Taper { angle, outward: ec.draft_outward == Some(true) });
    let spec = PrismSpec { profile: Profile::new(vec![face.region.clone()]), vector: v3::scale(dir, depth), taper };
    let mut s = c.k.prism(&spec)?.body;
    if let Some((o, n)) = clip {
        let h = half_space(c, o, n, x.big * 4.0)?;
        s = c.k.boolean(BooleanOp::Cut, &s, &[&h])?.body;
    }
    if ec.kind == EndConditionType::UpToBody {
        s = up_to_body(c, s, face, x)?;
    }
    Ok(s)
}

/// Up To Body: SOLIDWORKS stops the extrusion where it meets the body. The document does
/// not store which body was picked (`EndCondition` has no body field), so the part's body
/// is used: the through-all prism minus the body, keeping the pieces that start at the
/// profile. Without a body this is Through All.
fn up_to_body<K: Kernel>(c: &Ctx<'_, K>, prism: K::Body, face: &Face, x: &Ex) -> FResult<K::Body> {
    let Some(s) = c.shape() else { return Ok(prism) };
    let rest = match c.k.boolean(BooleanOp::Cut, &prism, &[&s.body]) {
        Ok(r) => r.body,
        Err(oc_kernel::KernelError::Empty { .. }) => return Err("The extrusion does not reach the body".into()),
        Err(e) => return Err(e.into()),
    };
    let mut keep = vec![];
    for piece in c.k.solids(&rest)? {
        let d = c.k.distance(Target::Body(&piece.body), Target::Point(face.corner))?;
        if d.distance < 1e-4 {
            keep.push(piece.body);
        }
    }
    let tool = fuse_all(c, keep)?.ok_or("The extrusion does not reach the body")?;
    if c.k.bounding_box(&tool)?.diagonal() > x.big * 0.9 {
        return Err("The extrusion does not reach the body".into());
    }
    Ok(tool)
}
