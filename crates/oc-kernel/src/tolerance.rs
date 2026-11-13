//! The one tolerance policy (`docs/OVERHAUL_PLAN.md` section 2): every tolerance the
//! kernel uses comes from here; implementations take no literals of their own.
//!
//! Model units are millimetres. Defaults reproduce the phase-0 oracle (the TypeScript app
//! on replicad/OCCT) wherever it set a value, so golden results stay comparable.
//!
//! A kernel with a fixed internal precision (Parasolid: 1e-8 m linear, 1e-11 rad
//! angular, sized for a ±500 m box) reports its own values from
//! [`crate::Kernel::tolerances`] and ignores fields it cannot honour.

use crate::spec::MeshParams;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerances {
    /// Points closer than this are the same point (OCCT `Precision::Confusion`, 1e-7 mm).
    pub confusion: f64,
    /// Directions closer than this angle are parallel (OCCT `Precision::Angular`).
    pub angular: f64,
    /// Fuzzy value for booleans: gaps or overlaps below it are closed. 0 = exact
    /// arithmetic with the shapes' own tolerances, which is what the oracle used and what
    /// passes the coplanar and tangent cases of the failure inventory.
    pub boolean_fuzzy: f64,
    /// Let booleans use several threads. OCCT's parallel boolean gives the same result
    /// as the serial one (checked by `tests/determinism.rs`).
    pub boolean_parallel: bool,
    /// After a boolean, merge faces on the same surface and edges on the same curve when
    /// their normals/tangents agree within this angle (radians). replicad's
    /// `SimplifyResult(true, true, 1e-3)`; SOLIDWORKS also shows such faces as one.
    pub unify_angular: f64,
    /// Coincidence tolerance of offset algorithms (shell, offset): replicad's shell value.
    pub offset: f64,
    /// 3D tolerance of loft surfaces (replicad `ThruSections(…, 1e-6)`).
    pub loft: f64,
    /// Maximum deviation of a spline segment from its sample points.
    pub spline_fit: f64,
    /// Precision given to shape healing (STEP import, repair of invalid results).
    pub heal: f64,
    /// A healed result is accepted only if its volume and area are within this relative
    /// difference of the unhealed one and it has the same number of faces: healing must
    /// not change the part (0.4 follow-up in section 8: a "fixed" shell that kept its
    /// removed face as a zero-thickness lid).
    pub heal_acceptance: f64,
}

impl Tolerances {
    pub const DEFAULT: Tolerances = Tolerances {
        confusion: 1e-7,
        angular: 1e-12,
        boolean_fuzzy: 0.0,
        boolean_parallel: true,
        unify_angular: 1e-3,
        offset: 1e-3,
        loft: 1e-6,
        spline_fit: 1e-5,
        heal: 1e-7,
        heal_acceptance: 1e-6,
    };

    /// Display tessellation for SOLIDWORKS' Image Quality slider (`quality` 0..=10,
    /// default 3) on a body whose bounding box diagonal is `size` mm. Each step roughly
    /// halves (or doubles) the chordal deviation. Same formula as the current viewer
    /// (`engine.ts` `meshShape`).
    pub fn display_mesh(&self, quality: f64, size: f64) -> MeshParams {
        let q = quality.clamp(0.0, 10.0);
        let size = if size > 0.0 && size.is_finite() { size } else { 1.0 };
        MeshParams {
            linear: (size * 4e-4 * 2f64.powf((3.0 - q) * 0.8)).max(1e-4),
            angular: (0.15 * 2f64.powf((3.0 - q) * 0.35)).clamp(0.03, 0.5),
        }
    }
}

impl Default for Tolerances {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_mesh_matches_the_viewer() {
        let t = Tolerances::DEFAULT;
        let m = t.display_mesh(3.0, 100.0);
        assert!((m.linear - 0.04).abs() < 1e-12 && (m.angular - 0.15).abs() < 1e-12);
        assert!(t.display_mesh(10.0, 100.0).linear < m.linear);
        assert_eq!(t.display_mesh(10.0, 0.01).linear, 1e-4);
        assert!((t.display_mesh(0.0, 1e9).angular - 0.15 * 2f64.powf(1.05)).abs() < 1e-12);
    }
}
