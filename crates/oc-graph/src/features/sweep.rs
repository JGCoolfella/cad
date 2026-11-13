//! Swept and lofted boss/cut (`sweep`, `loftFeature` in `engine.ts`).

use oc_doc::{LoftFeature, SweepFeature, SweepOrientation as DocOrientation, SweepProfileType};
use oc_kernel::{Kernel, LoftSpec, Profile, Region, Segment, SweepOrientation, SweepSpec, Wire};

use super::fuse_all;
use crate::eval::{Ctx, FResult, NodeOut, Tool};
use crate::sketch::{path_wire, profile_regions, region_world};

pub fn sweep<K: Kernel>(c: &Ctx<'_, K>, f: &SweepFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    let (path_sk, path_g) = c.sketch(f.path.as_str())?;
    let pf = c.frame(path_sk.id.as_str()).ok_or("The path sketch plane is unresolved")?;
    let path = path_wire(&path_g, &pf)?;
    let orientation = if f.orientation == DocOrientation::KeepNormal { SweepOrientation::KeepNormal } else { SweepOrientation::FollowPath };
    let run = |r: Region| -> FResult<K::Body> {
        Ok(c.k.sweep(&SweepSpec { profile: Profile::new(vec![r]), path: path.wire.clone(), orientation })?.body)
    };
    let tool = if f.profile_type == SweepProfileType::Circular {
        // A circle of the given diameter, normal to the path at its start.
        let circle = Segment::Circle { center: path.start, normal: path.start_dir, radius: f.diameter / 2.0 };
        Some(run(Region::new(Wire::new(vec![circle])))?)
    } else {
        let (sk, g) = c.sketch(f.profile.as_str())?;
        let fr = c.frame(sk.id.as_str()).ok_or("The profile sketch plane is unresolved")?;
        let parts = profile_regions(&g, None, None)?.iter().map(|r| run(region_world(r, &fr))).collect::<FResult<Vec<_>>>()?;
        fuse_all(c, parts)?
    };
    let tool = tool.ok_or("The sweep profile is empty")?;
    out.preview = Some(tool.clone());
    out.tools = Some(vec![Tool { body: tool.clone(), cut: f.cut }]);
    c.set_body(c.combine(&tool, f.cut, f.merge)?);
    Ok(())
}

pub fn loft<K: Kernel>(c: &Ctx<'_, K>, f: &LoftFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    if f.profiles.len() < 2 {
        return Err("Select at least two profiles".into());
    }
    let mut sections = vec![];
    for id in &f.profiles {
        let (sk, g) = c.sketch(id.as_str())?;
        let fr = c.frame(sk.id.as_str()).ok_or_else(|| format!("{}: sketch plane unresolved", sk.name))?;
        let loops = profile_regions(&g, None, None)?;
        if loops.len() != 1 || !loops[0].holes.is_empty() {
            return Err(format!("{}: each loft profile must be a single closed contour", sk.name).into());
        }
        sections.push(region_world(&loops[0], &fr).outer);
    }
    let tool = c.k.loft(&LoftSpec { sections, ruled: false })?.body;
    out.preview = Some(tool.clone());
    out.tools = Some(vec![Tool { body: tool.clone(), cut: f.cut }]);
    c.set_body(c.combine(&tool, f.cut, f.merge)?);
    Ok(())
}
