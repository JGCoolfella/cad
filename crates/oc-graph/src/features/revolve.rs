//! Revolved Boss/Base and Revolved Cut: one direction, mid plane, Direction 2, thin
//! feature, selected contours, axis from a sketch line or a picked reference.

use std::rc::Rc;

use oc_doc::{RevolveFeature, RevolveType};
use oc_kernel::{Axis, Kernel, Profile, RevolveSpec, Transform};

use super::transform_region;
use crate::eval::{Ctx, FResult, NodeOut, Shape, Tool};
use crate::math::{to_world, v3};
use crate::refgeom;
use crate::sketch::geo::Kind;
use crate::sketch::{profile_regions, region_tags, region_world};

pub fn revolve<K: Kernel>(c: &Ctx<'_, K>, f: &RevolveFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    let (sk, mut g) = c.sketch(f.sketch_id.as_str())?;
    let frame = c.frame(sk.id.as_str()).ok_or("Sketch plane unresolved")?;
    let axis_ref = f.axis_ref.as_ref().and_then(|r| r.as_ref());
    let (a, dir) = match axis_ref {
        Some(r) => {
            let ax = refgeom::axis_of(c, Some(r))?;
            (ax.p, ax.d)
        }
        None => {
            let Some(Kind::Line { p1, p2 }) = g.entities.get(f.axis.as_str()).map(|e| &e.kind) else {
                return Err("Select a line, centerline, edge or axis as the axis of revolution".into());
            };
            let a = to_world(&frame, g.p(p1));
            (a, v3::norm(v3::sub(to_world(&frame, g.p(p2)), a)))
        }
    };
    // A solid line used as the axis is not part of the profile.
    if axis_ref.is_none() && g.entities.get(f.axis.as_str()).is_some_and(|e| e.construction != Some(true)) {
        g.entities.shift_remove(f.axis.as_str());
    }
    let regions = profile_regions(&g, f.thin.as_ref(), f.contours.as_deref())?;
    let deg = f.angle.to_degrees();
    let sgn = if f.reverse { -1.0 } else { 1.0 };
    let tau = std::f64::consts::TAU;
    let a2 = f.angle2.unwrap_or(0.0);
    let two_way = a2 != 0.0 && f.rev_type != RevolveType::MidPlane && deg < 360.0;
    let mut parts = vec![];
    for r in &regions {
        let run = |face: &oc_kernel::Region, d: [f64; 3], angle: f64, caps: [&str; 2]| -> FResult<Rc<Shape<K::Body>>> {
            let spec = RevolveSpec { profile: Profile::new(vec![face.clone()]), axis: Axis { origin: a, dir: d }, angle };
            c.revolve(&spec, &region_tags(c.fid(), r, caps, regions.len()))
        };
        let face = region_world(r, &frame);
        let fc = if f.rev_type == RevolveType::MidPlane {
            transform_region(&face, &Transform::rotation(Axis { origin: a, dir }, -sgn * f.angle / 2.0))
        } else {
            face.clone()
        };
        // Caps as for extrusions: with a second direction the caps on the sketch plane meet
        // inside the body, direction 1 ends in `end` and direction 2 in `start`.
        let mut s = run(&fc, v3::scale(dir, sgn), f.angle.min(tau), if two_way { ["mid", "end"] } else { ["start", "end"] })?;
        if two_way {
            let back = run(&face, v3::scale(dir, -sgn), (tau - f.angle).min(a2), ["mid", "start"])?;
            s = c.fuse(&s, &back)?;
        }
        parts.push(s);
    }
    let Some(tool) = c.fuse_all(parts)? else { return Ok(()) };
    out.preview = Some(tool.body.clone());
    out.tools = Some(vec![Tool { shape: tool.clone(), cut: f.cut }]);
    c.set_body(c.combine(&tool, f.cut, f.merge)?);
    Ok(())
}
