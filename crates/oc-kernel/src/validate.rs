//! Argument checks shared by kernel implementations, so every kernel rejects the same
//! unusable inputs with the same [`KernelError::InvalidInput`] before touching geometry.

use crate::error::{KResult, KernelError, OpKind};
use crate::geom::{v3, Profile, Segment, Wire, P3};
use crate::tolerance::Tolerances;

fn finite(p: P3) -> bool {
    p.iter().all(|x| x.is_finite())
}

/// `x` is finite and `> 0`.
pub fn positive(op: OpKind, what: &str, x: f64) -> KResult<()> {
    if x.is_finite() && x > 0.0 {
        Ok(())
    } else {
        Err(KernelError::input(op, format!("{what} must be positive (got {x})")))
    }
}

/// A non-zero, finite vector.
pub fn direction(op: OpKind, what: &str, v: P3) -> KResult<()> {
    if finite(v) && v3::len(v) > 1e-12 {
        Ok(())
    } else {
        Err(KernelError::input(op, format!("{what} is zero or not finite")))
    }
}

/// Every index is below `count` and none repeats.
pub fn indices(op: OpKind, what: &str, ids: impl IntoIterator<Item = u32>, count: usize) -> KResult<()> {
    let mut seen = std::collections::HashSet::new();
    for i in ids {
        if i as usize >= count {
            return Err(KernelError::input(op, format!("{what} {i} does not exist (the body has {count})")));
        }
        if !seen.insert(i) {
            return Err(KernelError::input(op, format!("{what} {i} is given twice")));
        }
    }
    Ok(())
}

fn segment(op: OpKind, s: &Segment, tol: f64) -> KResult<()> {
    let bad = |m: &str| Err(KernelError::input(op, m.to_string()));
    match s {
        Segment::Line { a, b } => {
            if !finite(*a) || !finite(*b) || v3::dist(*a, *b) <= tol {
                return bad("a line segment has zero length");
            }
        }
        Segment::Arc { a, mid, b } => {
            if ![*a, *mid, *b].iter().all(|p| finite(*p)) {
                return bad("an arc is not finite");
            }
            let n = v3::cross(v3::sub(*mid, *a), v3::sub(*b, *a));
            if v3::len(n) <= tol * (v3::dist(*a, *mid) + v3::dist(*mid, *b)) {
                return bad("an arc's three points are on one line");
            }
        }
        Segment::Circle { center, normal, radius } => {
            positive(op, "circle radius", *radius)?;
            direction(op, "circle normal", *normal)?;
            if !finite(*center) {
                return bad("a circle centre is not finite");
            }
        }
        Segment::Ellipse { center, normal, major_dir, major, minor } => {
            positive(op, "ellipse minor radius", *minor)?;
            if major < minor || !major.is_finite() {
                return bad("an ellipse's major radius is smaller than its minor radius");
            }
            direction(op, "ellipse normal", *normal)?;
            direction(op, "ellipse axis", *major_dir)?;
            if !finite(*center) || v3::len(v3::cross(*normal, *major_dir)) < 1e-9 * v3::len(*normal) * v3::len(*major_dir) {
                return bad("an ellipse's axis is parallel to its normal");
            }
        }
        Segment::Spline { points } => {
            if points.len() < 2 || !points.iter().all(|p| finite(*p)) {
                return bad("a spline needs at least two finite points");
            }
            if v3::dist(points[0], points[points.len() - 1]) <= tol && points.len() < 4 {
                return bad("a closed spline needs at least four points");
            }
        }
    }
    Ok(())
}

/// Segments are valid and each starts where the previous one ends (within `tol`).
pub fn wire(op: OpKind, w: &Wire, tol: f64) -> KResult<()> {
    if w.segments.is_empty() {
        return Err(KernelError::input(op, "a wire has no segments"));
    }
    let mut prev_end: Option<P3> = None;
    for s in &w.segments {
        segment(op, s, tol)?;
        match (s.ends(), prev_end) {
            (None, _) if w.segments.len() > 1 => {
                return Err(KernelError::input(op, "a circle or ellipse must be a wire on its own"));
            }
            (Some((start, _)), Some(end)) if v3::dist(start, end) > tol => {
                return Err(KernelError::input(op, "a wire's segments are not connected end to start"));
            }
            _ => {}
        }
        prev_end = s.ends().map(|e| e.1);
    }
    Ok(())
}

/// A closed, connected wire.
pub fn closed_wire(op: OpKind, w: &Wire, tol: f64) -> KResult<()> {
    wire(op, w, tol)?;
    if !w.is_closed(tol) {
        return Err(KernelError::input(op, "a profile contour is open"));
    }
    Ok(())
}

/// At least one region; every wire closed.
pub fn profile(op: OpKind, p: &Profile, tol: &Tolerances) -> KResult<()> {
    if p.regions.is_empty() {
        return Err(KernelError::input(op, "the profile has no regions"));
    }
    for r in &p.regions {
        closed_wire(op, &r.outer, tol.confusion)?;
        for h in &r.holes {
            closed_wire(op, h, tol.confusion)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Region;

    const OP: OpKind = OpKind::Prism;

    #[test]
    fn profiles() {
        let t = Tolerances::DEFAULT;
        let rect = Wire::rect_xy(0.0, 0.0, 1.0, 1.0, 0.0);
        assert!(profile(OP, &Profile::single(rect.clone()), &t).is_ok());
        assert!(profile(OP, &Profile::default(), &t).is_err());
        let open = Wire::new(rect.segments[..3].to_vec());
        assert!(profile(OP, &Profile::single(open), &t).is_err());
        let mut gap = rect.clone();
        gap.segments[1] = Segment::Line { a: [1.0, 0.1, 0.0], b: [1.0, 1.0, 0.0] };
        assert!(matches!(profile(OP, &Profile::single(gap), &t), Err(KernelError::InvalidInput { .. })));
        let circle = Segment::Circle { center: [0.0; 3], normal: [0.0, 0.0, 1.0], radius: 0.0 };
        assert!(profile(OP, &Profile::single(Wire::new(vec![circle])), &t).is_err());
        let arc = Segment::Arc { a: [0.0; 3], mid: [1.0, 0.0, 0.0], b: [2.0, 0.0, 0.0] };
        assert!(segment(OP, &arc, 1e-7).is_err());
        let region = Region::new(rect).with_hole(Wire::new(vec![Segment::Line { a: [0.0; 3], b: [0.0; 3] }]));
        assert!(profile(OP, &Profile::new(vec![region]), &t).is_err());
    }

    #[test]
    fn scalars_and_indices() {
        assert!(positive(OP, "r", 1.0).is_ok());
        assert!(positive(OP, "r", 0.0).is_err() && positive(OP, "r", f64::NAN).is_err());
        assert!(direction(OP, "v", [0.0, 0.0, 1e-20]).is_err());
        assert!(indices(OP, "edge", [0, 2], 3).is_ok());
        assert!(indices(OP, "edge", [3], 3).is_err());
        assert!(indices(OP, "edge", [1, 1], 3).is_err());
    }
}
