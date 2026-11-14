//! Every feature type, built on the [`oc_kernel::Kernel`] trait the way `engine.ts`
//! builds it (`applyFeature`). Error and warning texts are the TypeScript engine's.

mod dressup;
mod extrude;
mod hole;
mod pattern;
mod revolve;
mod rib;
mod sweep;

use oc_doc::{DraftFeature, Feature, FeatureKind, ShellFeature};
use oc_kernel::{DraftSpec, Kernel, Plane, Region, Segment, ShellSpec, Transform, Wire};

use crate::eval::{Ctx, FResult, NodeOut};
use crate::math::v3;
use crate::refgeom;

pub use dressup::apply_dressup;

/// Evaluate one (unsuppressed) feature. The body change goes through [`Ctx::set_body`].
pub(crate) fn apply<K: Kernel>(c: &Ctx<'_, K>, f: &Feature) -> FResult<NodeOut<K::Body>> {
    let mut out = NodeOut::default();
    match &f.kind {
        FeatureKind::Sketch(s) => {
            let frame = refgeom::resolve_plane_as(c, &s.plane, "the face this sketch is on")?;
            out.frame = Some(frame);
            if s.sketch.refs.as_ref().is_some_and(|r| !r.is_empty()) {
                let (sk, missing) = crate::sketch::refs::resolve(c, &s.sketch, &frame)?;
                out.sketch = Some(std::sync::Arc::new(sk));
                out.warning = crate::sketch::refs::warning(missing);
            }
        }
        FeatureKind::RefPlane(p) => out.plane = Some(refgeom::ref_plane(c, p)?),
        FeatureKind::RefAxis(a) => out.axis = Some(refgeom::ref_axis(c, &a.refs)?),
        FeatureKind::RefPoint(p) => out.point = Some(refgeom::ref_point(c, &p.refs, p.xyz)?),
        FeatureKind::CoordSys(cs) => out.csys = Some(refgeom::coord_sys(c, cs)?),
        FeatureKind::Extrude(e) => extrude::extrude(c, e, &mut out)?,
        FeatureKind::Revolve(r) => revolve::revolve(c, r, &mut out)?,
        FeatureKind::Sweep(s) => sweep::sweep(c, s, &mut out)?,
        FeatureKind::Loft(l) => sweep::loft(c, l, &mut out)?,
        FeatureKind::Fillet(x) => out.dress = Some(vec![apply_dressup(c, &dressup::of_fillet(x))?]),
        FeatureKind::Chamfer(x) => out.dress = Some(vec![apply_dressup(c, &dressup::of_chamfer(x))?]),
        FeatureKind::Hole(h) => hole::hole(c, h, &mut out)?,
        FeatureKind::Rib(r) => rib::rib(c, r, &mut out)?,
        FeatureKind::Draft(d) => draft(c, d)?,
        FeatureKind::Shell(s) => shell(c, s)?,
        FeatureKind::Imported(i) => {
            let body = c.import(i.data.as_bytes()).map_err(|e| crate::FeatureError {
                message: "The imported file could not be read".into(),
                detail: Some(e.to_string()),
            })?;
            let next = match c.shape() {
                Some(s) => c.fuse(&s, &body)?,
                None => body,
            };
            c.set_body(next);
        }
        FeatureKind::LinearPattern(p) => pattern::linear(c, p, &mut out)?,
        FeatureKind::CircularPattern(p) => pattern::circular(c, p, &mut out)?,
        FeatureKind::Mirror(m) => pattern::mirror(c, m, &mut out)?,
    }
    Ok(out)
}

fn draft<K: Kernel>(c: &Ctx<'_, K>, f: &DraftFeature) -> FResult<()> {
    let s = c.shape().ok_or("There is no body to draft")?;
    let pl = refgeom::plane_of(c, f.neutral.as_ref())?;
    if f.faces.is_empty() {
        return Err("Select the faces to draft".into());
    }
    let faces = c.find_faces(&f.faces, "faces to draft")?;
    let n = if f.reverse { v3::neg(pl.n) } else { pl.n };
    let spec = DraftSpec { faces, neutral: Plane { origin: pl.o, normal: n }, angle: f.angle };
    c.set_body(c.draft(&s, &spec)?);
    Ok(())
}

fn shell<K: Kernel>(c: &Ctx<'_, K>, f: &ShellFeature) -> FResult<()> {
    let s = c.shape().ok_or("There is no body to shell")?;
    if f.faces.is_empty() {
        return Err("Select at least one face to remove".into());
    }
    let open = c.find_faces(&f.faces, "faces to remove")?;
    let spec = ShellSpec { open, thickness: f.thickness, outward: f.outward };
    c.set_body(c.shell(&s, &spec)?);
    Ok(())
}

/// A region moved by a rigid transform (used to turn a revolve profile back by half its
/// angle).
pub(crate) fn transform_region(r: &Region, t: &Transform) -> Region {
    let dir = |d: [f64; 3]| v3::sub(t.apply(d), t.apply([0.0; 3]));
    let seg = |s: &Segment| match s {
        Segment::Line { a, b } => Segment::Line { a: t.apply(*a), b: t.apply(*b) },
        Segment::Arc { a, mid, b } => Segment::Arc { a: t.apply(*a), mid: t.apply(*mid), b: t.apply(*b) },
        Segment::Circle { center, normal, radius } => Segment::Circle { center: t.apply(*center), normal: dir(*normal), radius: *radius },
        Segment::Ellipse { center, normal, major_dir, major, minor } => {
            Segment::Ellipse { center: t.apply(*center), normal: dir(*normal), major_dir: dir(*major_dir), major: *major, minor: *minor }
        }
        Segment::Spline { points } => Segment::Spline { points: points.iter().map(|p| t.apply(*p)).collect() },
    };
    let wire = |w: &Wire| Wire::new(w.segments.iter().map(seg).collect());
    Region { outer: wire(&r.outer), holes: r.holes.iter().map(wire).collect() }
}

