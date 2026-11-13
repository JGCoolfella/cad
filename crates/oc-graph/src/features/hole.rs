//! Hole Wizard (`hole` in `engine.ts`, sizes from `src/core/holes.ts`): a revolved hole
//! body at each position, cut from the part.

use oc_doc::{HoleEndType, HoleFeature, HoleFit, HoleType};
use oc_kernel::{Axis, Kernel, Profile, RevolveSpec, Wire};

use super::fuse_all;
use crate::eval::{Ctx, FResult, NodeOut, Tool};
use crate::math::{frame_for_plane, to_world, v3, P2, P3};
use crate::sketch::geo::Kind;

struct Size {
    name: &'static str,
    nominal: f64,
    /// close, normal, loose
    clearance: [f64; 3],
    cbore_dia: f64,
    cbore_depth: f64,
    csk_dia: f64,
    tap_drill: f64,
}

const fn sz(name: &'static str, nominal: f64, clearance: [f64; 3], cbore_dia: f64, cbore_depth: f64, csk_dia: f64, tap_drill: f64) -> Size {
    Size { name, nominal, clearance, cbore_dia, cbore_depth, csk_dia, tap_drill }
}

/// ISO metric sizes: clearance ISO 273, counterbores for ISO 4762, countersinks for
/// ISO 10642 (90°), coarse-thread tap drills.
const ISO: [Size; 13] = [
    sz("M2", 2.0, [2.2, 2.4, 2.6], 4.3, 2.3, 4.4, 1.6),
    sz("M2.5", 2.5, [2.7, 2.9, 3.1], 5.0, 2.8, 5.5, 2.05),
    sz("M3", 3.0, [3.2, 3.4, 3.6], 6.5, 3.3, 6.72, 2.5),
    sz("M4", 4.0, [4.3, 4.5, 4.8], 8.0, 4.4, 8.96, 3.3),
    sz("M5", 5.0, [5.3, 5.5, 5.8], 10.0, 5.4, 11.2, 4.2),
    sz("M6", 6.0, [6.4, 6.6, 7.0], 11.0, 6.5, 13.44, 5.0),
    sz("M8", 8.0, [8.4, 9.0, 10.0], 15.0, 8.6, 17.92, 6.8),
    sz("M10", 10.0, [10.5, 11.0, 12.0], 18.0, 10.8, 22.4, 8.5),
    sz("M12", 12.0, [13.0, 13.5, 14.5], 20.0, 13.0, 26.88, 10.2),
    sz("M14", 14.0, [15.0, 15.5, 16.5], 24.0, 15.0, 30.8, 12.0),
    sz("M16", 16.0, [17.0, 17.5, 18.5], 26.0, 17.5, 33.6, 14.0),
    sz("M20", 20.0, [21.0, 22.0, 24.0], 33.0, 21.5, 40.32, 17.5),
    sz("M24", 24.0, [25.0, 26.0, 28.0], 40.0, 25.5, 48.0, 21.0),
];

/// Hole geometry for a size, type and fit (`holeDims`): diameter, counterbore diameter
/// and depth, countersink diameter and angle. Unknown sizes fall back to M6.
pub fn hole_dims(t: HoleType, size: &str, fit: HoleFit) -> (f64, f64, f64, f64, f64) {
    let s = ISO.iter().find(|x| x.name == size).unwrap_or(&ISO[5]);
    let fit = match fit {
        HoleFit::Close => 0,
        HoleFit::Normal => 1,
        HoleFit::Loose => 2,
    };
    let dia = match t {
        HoleType::Tap => s.tap_drill,
        HoleType::Hole => s.nominal,
        _ => s.clearance[fit],
    };
    let cbore = if t == HoleType::Cbore { (s.cbore_dia, s.cbore_depth) } else { (0.0, 0.0) };
    let csk = if t == HoleType::Csk { s.csk_dia } else { 0.0 };
    (dia, cbore.0, cbore.1, csk, std::f64::consts::FRAC_PI_2)
}

pub fn hole<K: Kernel>(c: &Ctx<'_, K>, f: &HoleFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    let shape = c.shape().ok_or("There is no body to put holes in")?;
    let (frame, pts): (_, Vec<P2>) = match &f.sketch_id {
        Some(id) => {
            let (sk, g) = c.sketch(id.as_str())?;
            let frame = c.frame(sk.id.as_str()).ok_or("The hole positions sketch is unresolved")?;
            let pts = g.entities.values().filter_map(|e| if let Kind::Point { p } = &e.kind { Some(g.p(p)) } else { None }).collect();
            (frame, pts)
        }
        None => {
            let face = f.face.as_ref().ok_or("Select a planar face for the hole positions")?;
            (frame_for_plane(face.center, face.normal), f.positions.clone().unwrap_or_default())
        }
    };
    if pts.is_empty() {
        return Err("Click on the face to place the holes".into());
    }
    let (dia, cbore_dia, cbore_depth, csk_dia, csk_angle) = hole_dims(f.hole_type, &f.size, f.fit);
    let cu = f.custom.clone().unwrap_or_default();
    let r = cu.dia.unwrap_or(dia) / 2.0;
    let (rcb, cbd) = (cu.cbore_dia.unwrap_or(cbore_dia) / 2.0, cu.cbore_depth.unwrap_or(cbore_depth));
    let (rcsk, cska) = (cu.csk_dia.unwrap_or(csk_dia) / 2.0, cu.csk_angle.unwrap_or(csk_angle));
    let axis = v3::neg(frame.normal);
    let big = c.big_depth(&frame)?;
    let blind = f.end.kind == HoleEndType::Blind;
    let mut bodies = vec![];
    for p in pts {
        let at = to_world(&frame, p);
        let mut l = if blind { f.end.depth } else { big };
        if f.end.kind == HoleEndType::UpToNext {
            l = exit_depth(c, &shape, at, axis)?.unwrap_or(big);
        }
        // Half profile in (radius, depth), starting just above the face.
        let z0 = -0.01;
        let mut prof: Vec<[f64; 2]> = vec![[0.0, z0]];
        if f.hole_type == HoleType::Cbore && rcb > r {
            prof.extend([[rcb, z0], [rcb, cbd], [r, cbd]]);
        } else if f.hole_type == HoleType::Csk && rcsk > r {
            let k = (cska / 2.0).tan();
            prof.extend([[rcsk - z0 * k, z0], [r, (rcsk - r) / k]]);
        } else {
            prof.push([r, z0]);
        }
        prof.push([r, l]);
        // Blind holes end in a 118° drill point, as SOLIDWORKS draws them.
        prof.push([0.0, if blind { l + r / 59f64.to_radians().tan() } else { l }]);
        let pts3: Vec<P3> = prof.iter().map(|[rr, z]| v3::add(at, v3::add(v3::scale(frame.x, *rr), v3::scale(axis, *z)))).collect();
        let spec = RevolveSpec { profile: Profile::single(Wire::polygon(&pts3)), axis: Axis { origin: at, dir: axis }, angle: std::f64::consts::TAU };
        bodies.push(c.k.revolve(&spec)?.body);
    }
    let Some(tool) = fuse_all(c, bodies)? else { return Ok(()) };
    out.preview = Some(tool.clone());
    out.tools = Some(vec![Tool { body: tool.clone(), cut: true }]);
    c.set_body(c.combine(&tool, true, true)?);
    Ok(())
}

/// Depth along `dir` from `p` to where material ends (Up To Next for holes).
fn exit_depth<K: Kernel>(c: &Ctx<'_, K>, s: &crate::eval::Shape<K::Body>, p: P3, dir: P3) -> FResult<Option<f64>> {
    let t = c.topo(s)?;
    let mut best: Option<f64> = None;
    for f in &t.faces {
        if !matches!(f.surface, oc_kernel::Surface::Plane { .. }) || v3::dot(f.normal, dir) < 0.999 {
            continue;
        }
        let d = v3::dot(v3::sub(f.centroid, p), dir);
        if d <= 1e-6 || best.is_some_and(|b| d >= b) {
            continue;
        }
        let hit = v3::add(p, v3::scale(dir, d));
        if (0..3).all(|i| hit[i] >= f.bbox.min[i] - 1e-4 && hit[i] <= f.bbox.max[i] + 1e-4) {
            best = Some(d);
        }
    }
    Ok(best)
}
