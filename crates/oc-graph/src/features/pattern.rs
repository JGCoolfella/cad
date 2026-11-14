//! Linear and circular patterns and mirror (`repeatSeeds` in `engine.ts`): features that
//! add or remove material repeat their body; fillets and chamfers are re-applied to the
//! matching edges of each instance, so they pattern with their parent feature.
//!
//! Names: instance `n` of a seed face tagged `t` is `<fid>/i<n>/t`, where `n` is the
//! instance's place in the pattern (`2`, `3`, ... along a circular pattern, `i,j` in a
//! linear one; the seed is 1 and keeps its own name; a mirror's copy is `2`). The place,
//! not a running count, so skipping an instance or changing the count renames nothing
//! else. A fillet or chamfer re-applied to an instance is named from the instance's own
//! edge (`<fid>/fillet/<key>`), whose faces carry the instance tags.

use oc_doc::{CircularPatternFeature, LinearPatternFeature, MirrorFeature};
use oc_kernel::{Axis, Kernel, Plane, Transform};

use super::apply_dressup;
use crate::eval::{Ctx, Dressup, FResult, Instance, NodeOut, Tool};
use crate::math::{v3, P3};
use crate::refgeom;

/// The one copy a mirror makes (the seed is instance 1).
const MIRRORED: &str = "2";

/// One instance: its transform, key ("i,j" / "i") and whether it is skipped.
struct Xf {
    t: Transform,
    key: Option<String>,
    skipped: bool,
}

fn dir_of(t: &Transform, d: P3) -> P3 {
    v3::sub(t.apply(d), t.apply([0.0; 3]))
}

pub fn linear<K: Kernel>(c: &Ctx<'_, K>, f: &LinearPatternFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    let sign = |r: bool| if r { -1.0 } else { 1.0 };
    let d1 = v3::scale(refgeom::direction_of(c, f.dir1.r#ref.as_ref())?, sign(f.dir1.reverse));
    let d2 = match &f.dir2 {
        Some(d) if d.r#ref.is_some() => Some((v3::scale(refgeom::direction_of(c, d.r#ref.as_ref())?, sign(d.reverse)), d)),
        _ => None,
    };
    let skip = f.skip.clone().unwrap_or_default();
    let mut xfs = vec![];
    for i in 0..f.dir1.count.max(1) {
        for j in 0..d2.map_or(1, |(_, d)| d.count.max(1)) {
            if i == 0 && j == 0 {
                continue;
            }
            let mut v = v3::scale(d1, f.dir1.spacing * i as f64);
            if let Some((d, p)) = d2 {
                v = v3::add(v, v3::scale(d, p.spacing * j as f64));
            }
            xfs.push(Xf { t: Transform::translation(v), key: Some(format!("{},{}", i + 1, j + 1)), skipped: skip.contains(&[i + 1, j + 1]) });
        }
    }
    repeat_seeds(c, &f.seeds, xfs, out)
}

pub fn circular<K: Kernel>(c: &Ctx<'_, K>, f: &CircularPatternFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    let ax = refgeom::axis_of(c, f.axis.as_ref())?;
    let n = f.count.max(1);
    let deg = f.angle.to_degrees();
    let full = (deg.abs() - 360.0).abs() < 1e-6;
    let step = if f.equal_spacing { deg / if full { n } else { (n - 1).max(1) } as f64 } else { deg };
    let s = if f.reverse { -1.0 } else { 1.0 };
    let k = v3::norm(ax.d);
    let skip = f.skip.clone().unwrap_or_default();
    let xfs = (1..n)
        .map(|i| Xf {
            t: Transform::rotation(Axis { origin: ax.p, dir: k }, (s * step * i as f64).to_radians()),
            key: Some((i + 1).to_string()),
            skipped: skip.contains(&(i + 1)),
        })
        .collect();
    repeat_seeds(c, &f.seeds, xfs, out)
}

pub fn mirror<K: Kernel>(c: &Ctx<'_, K>, f: &MirrorFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    let pl = refgeom::plane_of(c, f.plane.as_ref())?;
    let t = Transform::mirror(Plane { origin: pl.o, normal: pl.n });
    if f.body == Some(true) || f.seeds.is_empty() {
        let s = c.shape().ok_or("There is no body to mirror")?;
        let m = c.transform(&s, &t, Some(MIRRORED))?;
        out.preview = Some(m.body.clone());
        out.tools = Some(vec![Tool { shape: m.clone(), cut: false }]);
        c.set_body(c.fuse(&s, &m)?);
        return Ok(());
    }
    repeat_seeds(c, &f.seeds, vec![Xf { t, key: None, skipped: false }], out)
}

/// Pattern/mirror instances of `seeds`, in FeatureManager order (as SOLIDWORKS does,
/// whatever order they were picked in). An instance whose fillet/chamfer edges are
/// missing is skipped with a warning.
fn repeat_seeds<K: Kernel>(c: &Ctx<'_, K>, seeds: &[oc_doc::Id], all: Vec<Xf>, out: &mut NodeOut<K::Body>) -> FResult<()> {
    if seeds.is_empty() {
        return Err("Select the features to pattern".into());
    }
    let mut sorted: Vec<&str> = seeds.iter().map(|s| s.as_str()).collect();
    sorted.sort_by_key(|id| c.index_of(id).map_or(-1, |i| i as i64));
    // Instance markers sit where each instance puts the first seed's centre.
    if all.iter().any(|x| x.key.is_some()) {
        if let Some(anchor) = seed_anchor(c, sorted[0])? {
            out.instances = Some(all.iter().map(|x| Instance { key: x.key.clone().unwrap_or_default(), p: x.t.apply(anchor), skipped: x.skipped }).collect());
        }
    }
    let xfs: Vec<&Xf> = all.iter().filter(|x| !x.skipped).collect();
    if xfs.is_empty() {
        out.tools = Some(vec![]);
        out.dress = Some(vec![]);
        return Ok(());
    }
    let mut tools_out = vec![];
    let mut dress_out = vec![];
    let mut failed = 0;
    for sid in sorted {
        let node = c.node(sid);
        let tools = node.and_then(|n| n.tools.as_ref()).filter(|t| !t.is_empty());
        let dress = node.and_then(|n| n.dress.as_ref()).filter(|d| !d.is_empty());
        if tools.is_none() && dress.is_none() {
            let name = c.name_of(sid).unwrap_or("A feature");
            return Err(format!("{name} cannot be patterned. Select features that add or remove material, fillets or chamfers.").into());
        }
        for t in tools.into_iter().flatten() {
            let copies = xfs.iter().map(|x| c.transform(&t.shape, &x.t, Some(x.key.as_deref().unwrap_or(MIRRORED)))).collect::<FResult<Vec<_>>>()?;
            let Some(acc) = c.fuse_all(copies)? else { continue };
            c.set_body(c.combine(&acc, t.cut, true)?);
            tools_out.push(Tool { shape: acc, cut: t.cut });
        }
        for d in dress.into_iter().flatten() {
            for x in &xfs {
                let inst = Dressup {
                    kind: d.kind.clone(),
                    edges: d
                        .edges
                        .iter()
                        .map(|e| oc_doc::EdgeSig { a: x.t.apply(e.a), b: x.t.apply(e.b), mid: x.t.apply(e.mid), name: None, ..e.clone() })
                        .collect(),
                    faces: d.faces.iter().map(|s| oc_doc::FaceSig { center: x.t.apply(s.center), normal: dir_of(&x.t, s.normal), name: None, ..s.clone() }).collect(),
                    exact: true,
                };
                match apply_dressup(c, &inst) {
                    Ok(applied) => dress_out.push(applied),
                    Err(_) => failed += 1,
                }
            }
        }
    }
    if failed > 0 {
        out.warning = Some(if failed == 1 {
            "One fillet/chamfer instance could not be created (pattern its parent feature too)".into()
        } else {
            format!("{failed} fillet/chamfer instances could not be created (pattern their parent features too)")
        });
    }
    // With fillet/chamfer instances the preview shows the whole result, so they are seen too.
    out.preview = (tools_out.len() == 1 && dress_out.is_empty()).then(|| tools_out[0].shape.body.clone());
    out.tools = Some(tools_out);
    out.dress = Some(dress_out);
    Ok(())
}

/// Centre of what a seed feature added or removed (or of its first fillet/chamfer edge).
fn seed_anchor<K: Kernel>(c: &Ctx<'_, K>, id: &str) -> FResult<Option<P3>> {
    let Some(n) = c.node(id) else { return Ok(None) };
    if let Some(t) = n.tools.as_ref().and_then(|t| t.first()) {
        let b = t.shape.bbox(c.k)?;
        return Ok(Some(v3::mid(b.min, b.max)));
    }
    Ok(n.dress.as_ref().and_then(|d| d.first()).and_then(|d| d.edges.first()).map(|e| e.mid))
}
