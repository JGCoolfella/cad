//! Rib (`rib` in `engine.ts`): a slab along each sketch line, grown (in the sketch plane
//! or normal to it) until it meets the body; the part of the slab between the sketch and
//! the body is added.

use oc_doc::{RibDirection, RibFeature, RibSide};
use oc_kernel::{BooleanOp, Kernel, KernelError, PrismSpec, Profile, Target, Wire};

use super::fuse_all;
use crate::eval::{Ctx, FResult, NodeOut, Tool};
use crate::math::{to_world, v3, P2, P3};
use crate::sketch::geo::Kind;

pub fn rib<K: Kernel>(c: &Ctx<'_, K>, f: &RibFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    let shape = c.shape().ok_or("A rib needs an existing body")?;
    let (sk, g) = c.sketch(f.sketch_id.as_str())?;
    let fr = c.frame(sk.id.as_str()).ok_or("The rib sketch plane is unresolved")?;
    let lines: Vec<(P2, P2)> = g
        .entities
        .values()
        .filter(|e| e.solid())
        .filter_map(|e| if let Kind::Line { p1, p2 } = &e.kind { Some((g.p(p1), g.p(p2))) } else { None })
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
    let prism = |q: [P2; 4], h: f64, v: P3| -> FResult<K::Body> {
        let pts: Vec<P3> = q.iter().map(|p| w(*p, h)).collect();
        Ok(c.k.prism(&PrismSpec { profile: Profile::single(Wire::polygon(&pts)), vector: v, taper: None })?.body)
    };
    let slab = |side: f64| -> FResult<Option<K::Body>> {
        let mut parts = vec![];
        for &(a, b) in &lines {
            let u = [b[0] - a[0], b[1] - a[1]];
            let len = u[0].hypot(u[1]);
            if len < 1e-9 {
                continue;
            }
            let s = if f.direction == RibDirection::Parallel {
                // Rectangle from the line towards the body (in the sketch plane), thickened along the normal.
                let m = [-u[1] / len * side, u[0] / len * side];
                let q = [a, b, [b[0] + m[0] * big, b[1] + m[1] * big], [a[0] + m[0] * big, a[1] + m[1] * big]];
                prism(q, n0, v3::scale(fr.normal, n1 - n0))?
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
                prism(q, 0.0, v3::scale(fr.normal, big * side))?
            };
            parts.push(s);
        }
        let Some(tool) = fuse_all(c, parts)? else { return Ok(None) };
        // Keep only the pieces touching the sketch (between the sketch and the body).
        let rest = match c.k.boolean(BooleanOp::Cut, &tool, &[&shape.body]) {
            Ok(r) => r.body,
            Err(KernelError::Empty { .. }) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut touch = vec![];
        for piece in c.k.solids(&rest)? {
            let mut hit = false;
            for &(a, b) in &lines {
                if c.k.distance(Target::Body(&piece.body), Target::Segment(w(a, 0.0), w(b, 0.0)))?.distance < 1e-4 {
                    hit = true;
                    break;
                }
            }
            if hit {
                touch.push(piece.body);
            }
        }
        let Some(keep) = fuse_all(c, touch)? else { return Ok(None) };
        // A piece reaching the far end never met the body: not a rib on that side.
        if c.k.bounding_box(&keep)?.diagonal() > big * 0.9 {
            return Ok(None);
        }
        Ok(Some(keep))
    };
    let (first, second) = if f.flip { (-1.0, 1.0) } else { (1.0, -1.0) };
    let tool = match slab(first)? {
        Some(t) => t,
        None => slab(second)?.ok_or("The rib does not meet the body. Draw the rib sketch on a plane through the part.")?,
    };
    out.preview = Some(tool.clone());
    out.tools = Some(vec![Tool { body: tool.clone(), cut: false }]);
    c.set_body(c.combine(&tool, false, true)?);
    Ok(())
}
