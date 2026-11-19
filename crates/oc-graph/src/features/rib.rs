//! Rib (`rib` in `engine.ts`): a slab along each sketch line, grown (in the sketch plane
//! or normal to it) until it meets the body; the part of the slab between the sketch and
//! the body is added.

use std::rc::Rc;

use oc_doc::{RibDirection, RibFeature, RibSide};
use oc_kernel::{BooleanOp, CapEnd, Kernel, KernelError, PrismSpec, Profile, Source, Target, Wire};
use oc_naming::Tag;

use crate::eval::{Ctx, FResult, NodeOut, Shape, Tool};
use crate::math::{to_world, v3, P2, P3};
use crate::sketch::geo::Kind;

/// Tags of a rib slab's faces, named after its sketch line `line`: the face along the
/// line is `side/<line>`, the two flanks `start/<line>` and `end/<line>`, the faces at the
/// line's ends `side/<line>.a` and `.b`, and the far face (always cut away) `far/<line>`.
/// `roles` gives the role and key suffix of the quadrilateral's four segments and of the
/// start and end cap, in that order. The flanks carry the number of lines (`regions`), as
/// the caps of an extrusion carry its region count.
fn slab_tags(fid: &str, line: &str, roles: [(&str, &str); 6], regions: usize) -> Vec<(Source, Tag)> {
    let tag = |(role, suffix): (&str, &str)| {
        let t = Tag::birth(fid, role, &format!("{line}{suffix}"));
        if matches!(role, "start" | "end") {
            t.with_regions(regions)
        } else {
            t
        }
    };
    let mut out: Vec<(Source, Tag)> = (0..4).map(|s| (Source::Segment { region: 0, wire: 0, segment: s as u32 }, tag(roles[s]))).collect();
    out.push((Source::Cap { region: 0, end: CapEnd::Start }, tag(roles[4])));
    out.push((Source::Cap { region: 0, end: CapEnd::End }, tag(roles[5])));
    out
}

pub fn rib<K: Kernel>(c: &Ctx<'_, K>, f: &RibFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    let shape = c.shape().ok_or("A rib needs an existing body")?;
    let (sk, g) = c.sketch(f.sketch_id.as_str())?;
    let fr = c.frame(sk.id.as_str()).ok_or("The rib sketch plane is unresolved")?;
    let lines: Vec<(&str, P2, P2)> = g
        .entities
        .values()
        .filter(|e| e.solid())
        .filter_map(|e| if let Kind::Line { p1, p2 } = &e.kind { Some((e.id.as_str(), g.p(p1), g.p(p2))) } else { None })
        .collect();
    if lines.is_empty() {
        return Err("The rib sketch needs at least one line".into());
    }
    let big = c.big_depth(&fr)? * 2.0;
    let t = f.thickness;
    let (n0, n1) = match f.side {
        RibSide::Both => (-t / 2.0, t / 2.0),
        RibSide::Side1 => (0.0, t),
        RibSide::Side2 => (-t, 0.0),
    };
    let w = |p: P2, h: f64| v3::add(to_world(&fr, p), v3::scale(fr.normal, h));
    let prism = |q: [P2; 4], h: f64, v: P3, tags: &[(Source, Tag)]| -> FResult<Rc<Shape<K::Body>>> {
        let pts: Vec<P3> = q.iter().map(|p| w(*p, h)).collect();
        c.prism(&PrismSpec { profile: Profile::single(Wire::polygon(&pts)), vector: v, taper: None }, tags)
    };
    let slab = |side: f64| -> FResult<Option<Rc<Shape<K::Body>>>> {
        let mut parts = vec![];
        for &(id, a, b) in &lines {
            let u = [b[0] - a[0], b[1] - a[1]];
            let len = u[0].hypot(u[1]);
            if len < 1e-9 {
                continue;
            }
            let s = if f.direction == RibDirection::Parallel {
                // Rectangle from the line towards the body (in the sketch plane), thickened along the normal.
                let m = [-u[1] / len * side, u[0] / len * side];
                let q = [a, b, [b[0] + m[0] * big, b[1] + m[1] * big], [a[0] + m[0] * big, a[1] + m[1] * big]];
                let tags = slab_tags(c.fid(), id, [("side", ""), ("side", ".b"), ("far", ""), ("side", ".a"), ("start", ""), ("end", "")], lines.len());
                prism(q, n0, v3::scale(fr.normal, n1 - n0), &tags)?
            } else {
                // Thin strip around the line (in the plane), extruded along the normal towards the body.
                let w0 = match f.side {
                    RibSide::Both => -t / 2.0,
                    RibSide::Side1 => 0.0,
                    RibSide::Side2 => -t,
                };
                let w1 = w0 + t;
                let nn = [-u[1] / len, u[0] / len];
                let q = [
                    [a[0] + nn[0] * w0, a[1] + nn[1] * w0],
                    [b[0] + nn[0] * w0, b[1] + nn[1] * w0],
                    [b[0] + nn[0] * w1, b[1] + nn[1] * w1],
                    [a[0] + nn[0] * w1, a[1] + nn[1] * w1],
                ];
                let tags = slab_tags(c.fid(), id, [("start", ""), ("side", ".b"), ("end", ""), ("side", ".a"), ("side", ""), ("far", "")], lines.len());
                prism(q, 0.0, v3::scale(fr.normal, big * side), &tags)?
            };
            parts.push(s);
        }
        let Some(tool) = c.fuse_all(parts)? else { return Ok(None) };
        // Keep only the pieces touching the sketch (between the sketch and the body).
        let rest = match c.try_boolean(BooleanOp::Cut, &tool, &[&shape]) {
            Ok(r) => r,
            Err(KernelError::Empty { .. }) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut touch = vec![];
        for piece in c.solids(&rest)? {
            let mut hit = false;
            for &(_, a, b) in &lines {
                if c.k.distance(Target::Body(&piece.body), Target::Segment(w(a, 0.0), w(b, 0.0)))?.distance < 1e-4 {
                    hit = true;
                    break;
                }
            }
            if hit {
                touch.push(piece);
            }
        }
        let Some(keep) = c.fuse_all(touch)? else { return Ok(None) };
        // A piece reaching the far end never met the body: not a rib on that side.
        if keep.bbox(c.k)?.diagonal() > big * 0.9 {
            return Ok(None);
        }
        Ok(Some(keep))
    };
    let (first, second) = if f.flip { (-1.0, 1.0) } else { (1.0, -1.0) };
    let tool = match slab(first)? {
        Some(t) => t,
        None => slab(second)?.ok_or("The rib does not meet the body. Draw the rib sketch on a plane through the part.")?,
    };
    out.preview = Some(tool.body.clone());
    out.tools = Some(vec![Tool { shape: tool.clone(), cut: false }]);
    c.set_body(c.combine(&tool, false, true)?);
    Ok(())
}
