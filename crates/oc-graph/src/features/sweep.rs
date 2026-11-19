//! Swept and lofted boss/cut (`sweep`, `loftFeature` in `engine.ts`).

use std::rc::Rc;

use oc_doc::{LoftFeature, SweepFeature, SweepOrientation as DocOrientation, SweepProfileType};
use oc_kernel::{CapEnd, Kernel, LoftSpec, Profile, Region, Segment, Source, SweepOrientation, SweepSpec, Wire};
use oc_naming::hash::key_of_set;
use oc_naming::Tag;

use crate::eval::{Ctx, FResult, NodeOut, Shape, Tool};
use crate::sketch::{path_wire, profile_regions, region_tags, region_world};

pub fn sweep<K: Kernel>(c: &Ctx<'_, K>, f: &SweepFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    let (path_sk, path_g) = c.sketch(f.path.as_str())?;
    let pf = c.frame(path_sk.id.as_str()).ok_or("The path sketch plane is unresolved")?;
    let path = path_wire(&path_g, &pf)?;
    let orientation = if f.orientation == DocOrientation::KeepNormal { SweepOrientation::KeepNormal } else { SweepOrientation::FollowPath };
    // A profile segment swept along a path of several curves makes one face per curve; they
    // share the segment's tag and are told apart by position.
    let run = |r: Region, tags: &[(Source, Tag)]| -> FResult<Rc<Shape<K::Body>>> {
        c.sweep(&SweepSpec { profile: Profile::new(vec![r]), path: path.wire.clone(), orientation }, tags)
    };
    let tool = if f.profile_type == SweepProfileType::Circular {
        // A circle of the given diameter, normal to the path at its start.
        let circle = Segment::Circle { center: path.start, normal: path.start_dir, radius: f.diameter / 2.0 };
        let fid = c.fid();
        let tags = [
            (Source::Segment { region: 0, wire: 0, segment: 0 }, Tag::birth(fid, "side", "profile")),
            (Source::Cap { region: 0, end: CapEnd::Start }, Tag::cap(fid, "start", "profile", 1)),
            (Source::Cap { region: 0, end: CapEnd::End }, Tag::cap(fid, "end", "profile", 1)),
        ];
        Some(run(Region::new(Wire::new(vec![circle])), &tags)?)
    } else {
        let (sk, g) = c.sketch(f.profile.as_str())?;
        let fr = c.frame(sk.id.as_str()).ok_or("The profile sketch plane is unresolved")?;
        let regions = profile_regions(&g, None, None)?;
        let parts = regions
            .iter()
            .map(|r| run(region_world(r, &fr), &region_tags(c.fid(), r, ["start", "end"], regions.len())))
            .collect::<FResult<Vec<_>>>()?;
        c.fuse_all(parts)?
    };
    let tool = tool.ok_or("The sweep profile is empty")?;
    out.preview = Some(tool.body.clone());
    out.tools = Some(vec![Tool { shape: tool.clone(), cut: f.cut }]);
    c.set_body(c.combine(&tool, f.cut, f.merge)?);
    Ok(())
}

pub fn loft<K: Kernel>(c: &Ctx<'_, K>, f: &LoftFeature, out: &mut NodeOut<K::Body>) -> FResult<()> {
    if f.profiles.len() < 2 {
        return Err("Select at least two profiles".into());
    }
    let fid = c.fid();
    let mut sections = vec![];
    // A side face runs through one segment of every profile and carries a tag for each
    // (`<fid>/side/<sketch>.<entity>`: entity ids are only unique within a sketch), so it
    // is still found when a profile is redrawn or removed. The caps are the first and the
    // last profile.
    let mut tags = vec![];
    let mut loop_keys = vec![];
    for (i, id) in f.profiles.iter().enumerate() {
        let (sk, g) = c.sketch(id.as_str())?;
        let fr = c.frame(sk.id.as_str()).ok_or_else(|| format!("{}: sketch plane unresolved", sk.name))?;
        let loops = profile_regions(&g, None, None)?;
        if loops.len() != 1 || !loops[0].holes.is_empty() {
            return Err(format!("{}: each loft profile must be a single closed contour", sk.name).into());
        }
        let keys: Vec<String> = loops[0].outer.ents.iter().map(|e| format!("{}.{e}", sk.id.as_str())).collect();
        for (s, k) in keys.iter().enumerate() {
            tags.push((Source::Segment { region: i as u32, wire: 0, segment: s as u32 }, Tag::birth(fid, "side", k)));
        }
        loop_keys.push(key_of_set(&keys));
        sections.push(region_world(&loops[0], &fr).outer);
    }
    // One region per profile (checked above).
    tags.push((Source::Cap { region: 0, end: CapEnd::Start }, Tag::cap(fid, "start", &loop_keys[0], 1)));
    tags.push((Source::Cap { region: 0, end: CapEnd::End }, Tag::cap(fid, "end", &loop_keys[loop_keys.len() - 1], 1)));
    let tool = c.loft(&LoftSpec { sections, ruled: false }, &tags)?;
    out.preview = Some(tool.body.clone());
    out.tools = Some(vec![Tool { shape: tool.clone(), cut: f.cut }]);
    c.set_body(c.combine(&tool, f.cut, f.merge)?);
    Ok(())
}
