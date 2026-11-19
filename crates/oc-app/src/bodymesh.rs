//! A body as the webview's `BodyMesh` (`src/core/types.ts`): level 0 of the `oc-mesh`
//! display mesh (the one `oc-render` draws, so picks and pixels agree) with each face's and
//! edge's signature and persistent name, for selection, references, snapping and Measure.
//! The renderer gets the same mesh through the mesh store under `handle`.
use crate::pack::Packer;
use oc_kernel::{Curve, FaceId, EdgeId, Surface, Topology, VertexId};
use oc_mesh::{RenderMesh, EDGE_SEAM, POINT_HIDDEN};
use oc_naming::resolve::Live;
use oc_naming::signature::{edge_sig, face_sig};
use oc_naming::BodyNames;
use serde_json::{json, Value};

/// The feature that made a face (Edit Feature from the graphics area, fillet previews):
/// among its tags' features, the one latest in the tree. `order` maps feature id → position.
pub fn owner(names: &BodyNames, f: usize, order: &dyn Fn(&str) -> Option<usize>) -> Option<String> {
    let name = names.faces.get(f)?;
    name.tags()
        .iter()
        .filter_map(|t| {
            let fid = t.as_str().split('/').next()?;
            order(fid).map(|p| (p, fid))
        })
        .max_by_key(|(p, _)| *p)
        .map(|(_, fid)| fid.to_string())
}

pub struct Input<'a> {
    pub handle: u32,
    pub mesh: &'a RenderMesh,
    pub topo: &'a Topology,
    pub names: Option<&'a BodyNames>,
    pub order: &'a dyn Fn(&str) -> Option<usize>,
}

pub fn encode(p: &mut Packer, x: Input<'_>) -> Value {
    let m = x.mesh;
    let o = m.origin;
    let at = |v: [f32; 3]| [(o[0] + v[0] as f64) as f32, (o[1] + v[1] as f64) as f32, (o[2] + v[2] as f64) as f32];
    let Some(l) = m.lods.first() else {
        return Value::Null;
    };
    let nv = l.vertex_count as usize;
    let (mut pos, mut nor) = (Vec::with_capacity(nv * 3), Vec::with_capacity(nv * 3));
    for i in 0..nv {
        let v = l.vertex(i);
        pos.extend_from_slice(&at(v.position));
        nor.extend_from_slice(&v.normal);
    }
    let idx: Vec<u32> = (0..l.index_count as usize).map(|i| l.index(i)).collect();
    let face_groups: Vec<Value> = l
        .faces
        .iter()
        .enumerate()
        .map(|(f, r)| json!({ "start": r.first_index, "count": r.index_count, "faceId": f }))
        .collect();
    let mut epos: Vec<f32> = Vec::with_capacity(l.segment_count as usize * 6);
    let mut edge_groups = Vec::with_capacity(l.edges.len());
    for (e, r) in l.edges.iter().enumerate() {
        let start = epos.len() / 3;
        for s in r.first_segment..r.first_segment + r.segment_count {
            let s = l.segment(s as usize);
            epos.extend_from_slice(&at(s.a));
            epos.extend_from_slice(&at(s.b));
        }
        edge_groups.push(json!({ "start": start, "count": epos.len() / 3 - start, "edgeId": e }));
    }
    let t = x.topo;
    let faces: Vec<Value> = (0..t.faces.len())
        .map(|f| {
            let sig = face_sig(t, x.names, FaceId(f as u32));
            let planar = matches!(t.faces[f].surface, Surface::Plane { .. });
            let owner = x.names.and_then(|n| owner(n, f, x.order));
            json!({ "id": f, "sig": sig, "planar": planar, "owner": owner })
        })
        .collect();
    let edges: Vec<Value> = (0..t.edges.len())
        .map(|e| {
            let sig = edge_sig(t, x.names, EdgeId(e as u32));
            let seam = m.edge_flags.get(e).is_some_and(|f| f & EDGE_SEAM != 0);
            json!({ "id": e, "sig": sig, "seam": seam, "linear": matches!(t.edges[e].curve, Curve::Line) })
        })
        .collect();
    let hidden: Vec<usize> = m
        .point_flags
        .iter()
        .enumerate()
        .filter(|(_, f)| *f & POINT_HIDDEN != 0)
        .map(|(i, _)| i)
        .collect();
    let vertex_names: Option<Vec<Value>> = x.names.map(|n| {
        let live = Live::new(t, n);
        (0..t.vertices.len())
            .map(|v| json!(live.vertex_name(VertexId(v as u32)).to_stored()))
            .collect()
    });
    let lo = at(m.bounds[0]);
    let hi = at(m.bounds[1]);
    json!({
        "handle": x.handle,
        "positions": p.f32(&pos),
        "normals": p.f32(&nor),
        "indices": p.u32(&idx),
        "faceGroups": face_groups,
        "edgePositions": p.f32(&epos),
        "edgeGroups": edge_groups,
        "faces": faces,
        "edges": edges,
        "vertices": t.vertices,
        "vertexNames": vertex_names,
        "hiddenVertices": hidden,
        "bbox": [lo, hi],
        // Solid bodies (G2b: Level of detail applies to multibody parts, as in SOLIDWORKS).
        "solids": t.solids,
    })
}
