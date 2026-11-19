//! The scene as the GPU sees it (G2). Every body has an [`ObjectU`] in one storage
//! buffer, indexed by its place in [`crate::Scene::bodies`]. Part bodies (instances) are
//! also grouped by mesh into *draw slots*, one per (mesh, level of detail): the culling
//! pass (`cull.rs`) writes each slot's visible instances into a list, in scene order, and
//! the slot's instance count into an indirect draw. A draw reads its object through the
//! list (`list[instance_index]`); bodies drawn one by one use the identity range at the
//! end of the list.
//!
//! What the CPU rebuilds per frame is the plain scene description (objects, highlight
//! states, the grouping); each buffer is written only when its contents changed.
use crate::batch::ObjectU;
use crate::mesh::MeshGpu;
use crate::pool::Grow;
use crate::scene::MeshId;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;

/// Draw kinds: the lists the culling pass fills. A body joins a kind by its bit in
/// `ObjectU::cull[1]`.
pub(crate) const K_PRE: u32 = 0; // depth prepass: opaque faces visible last frame
pub(crate) const K_OPAQUE: u32 = 1;
pub(crate) const K_TRANSP: u32 = 2;
pub(crate) const K_EDGE: u32 = 3;
pub(crate) const K_HIDDEN: u32 = 4; // Hidden Lines Visible: edges behind faces
pub(crate) const K_PARITY: u32 = 5; // section cap stencil
pub(crate) const K_PICK_FACE: u32 = 6;
pub(crate) const K_PICK_EDGE: u32 = 7;
pub(crate) const K_PICK_VERTEX: u32 = 8;
pub(crate) const KINDS: u32 = 9;
/// No mesh, no state range.
pub(crate) const NONE: u32 = u32::MAX;

pub(crate) fn is_face_kind(k: u32) -> bool {
    matches!(k, K_PRE | K_OPAQUE | K_TRANSP | K_PARITY | K_PICK_FACE)
}

/// A mesh used by the scene's instances.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
pub(crate) struct SceneMeshG {
    pub lo: [f32; 4],
    pub hi: [f32; 4],
    /// First slot, level count, first position in `order`, instance count.
    pub info: [u32; 4],
}

/// A draw slot: one level of one scene mesh.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
pub(crate) struct SlotG {
    /// First position in `order`, instance count, list base (per kind), level.
    pub info: [u32; 4],
    /// Chordal deviation of the level, mm.
    pub linear: [f32; 4],
}

/// The grouping of the scene's instances into draw slots.
#[derive(Default, PartialEq)]
pub(crate) struct Tables {
    /// Scene indices of the instances, grouped by scene mesh, scene order within a group.
    pub order: Vec<u32>,
    pub meshes: Vec<SceneMeshG>,
    pub slots: Vec<SlotG>,
    /// List entries per kind (the sum of every slot's capacity).
    pub cap: u32,
    /// Per kind and slot: index count, instance count, first index, base vertex, first instance.
    pub face_args: Vec<[u32; 5]>,
    /// Per kind and slot: vertex count, instance count, first vertex, first instance.
    pub edge_args: Vec<[u32; 4]>,
    /// Scene index → scene mesh (or [`NONE`]: drawn one by one, or not at all).
    pub mesh_of: Vec<u32>,
    /// Bodies in the scene (the identity range's length).
    pub bodies: u32,
}

impl Tables {
    pub fn identity_base(&self) -> u32 {
        KINDS * self.cap
    }
    pub fn slot_count(&self) -> u32 {
        self.slots.len() as u32
    }
}

/// Groups the instances: `instances[i]` is body `i`'s mesh when it is drawn by the GPU
/// lists (a part body with an uploaded mesh).
pub(crate) fn tables(instances: &[Option<MeshId>], meshes: &HashMap<MeshId, MeshGpu>) -> Tables {
    let mut index: HashMap<MeshId, usize> = HashMap::new();
    let mut groups: Vec<(MeshId, Vec<u32>)> = vec![];
    let mut mesh_of = vec![NONE; instances.len()];
    for (i, m) in instances.iter().enumerate() {
        let Some(m) = m else { continue };
        if !meshes.contains_key(m) {
            continue;
        }
        let g = *index.entry(*m).or_insert_with(|| {
            groups.push((*m, vec![]));
            groups.len() - 1
        });
        groups[g].1.push(i as u32);
        mesh_of[i] = g as u32;
    }
    let mut t = Tables { mesh_of, bodies: instances.len() as u32, ..Default::default() };
    let mut list_base = 0u32;
    for (m, inst) in &groups {
        let mg = &meshes[m];
        let n = inst.len() as u32;
        let start = t.order.len() as u32;
        t.meshes.push(SceneMeshG {
            lo: [mg.bounds[0][0], mg.bounds[0][1], mg.bounds[0][2], 0.0],
            hi: [mg.bounds[1][0], mg.bounds[1][1], mg.bounds[1][2], 0.0],
            info: [t.slots.len() as u32, mg.levels.len() as u32, start, n],
        });
        for (l, lv) in mg.levels.iter().enumerate() {
            t.slots.push(SlotG { info: [start, n, list_base, l as u32], linear: [lv.linear as f32, 0.0, 0.0, 0.0] });
            list_base += n;
        }
        t.order.extend_from_slice(inst);
    }
    t.cap = list_base;
    // Indirect arguments with zero instances; the compaction pass fills the counts.
    let mut slot_mesh = vec![];
    for (m, _) in &groups {
        for l in 0..meshes[m].levels.len() {
            slot_mesh.push((&meshes[m], l));
        }
    }
    for k in 0..KINDS {
        for (s, (mg, l)) in slot_mesh.iter().enumerate() {
            let r = mg.levels[*l].range;
            let first = k * t.cap + t.slots[s].info[2];
            t.face_args.push(if is_face_kind(k) { [r.index_count, 0, r.first_index, r.base_vertex as u32, first] } else { [0; 5] });
            t.edge_args.push(match k {
                K_EDGE | K_HIDDEN | K_PICK_EDGE => [6 * r.seg_count, 0, 6 * r.seg_base, first],
                K_PICK_VERTEX => [6 * mg.point_count, 0, 6 * mg.point_base, first],
                _ => [0; 4],
            });
        }
    }
    t
}

/// GPU buffers of the scene, rewritten when their contents change.
pub(crate) struct GpuScene {
    pub objects: Grow,
    pub states: Grow,
    pub list: Grow,
    pub order: Grow,
    pub codes: Grow,
    pub visible: Grow,
    pub meshes: Grow,
    pub slots: Grow,
    pub face_args: Grow,
    pub edge_args: Grow,
    last_objects: Vec<ObjectU>,
    last_states: Vec<u32>,
    pub tables: Tables,
    /// Changes whenever a buffer is replaced (bind groups are rebuilt).
    pub version: u64,
}

impl GpuScene {
    pub fn new(device: &wgpu::Device) -> GpuScene {
        use wgpu::BufferUsages as U;
        let s = U::STORAGE;
        let args = U::STORAGE | U::INDIRECT | U::COPY_SRC;
        GpuScene {
            objects: Grow::new(device, "objects", s),
            states: Grow::new(device, "states", s),
            list: Grow::new(device, "instance lists", s),
            order: Grow::new(device, "instance order", s),
            codes: Grow::new(device, "cull codes", s),
            visible: Grow::new(device, "visible last frame", s),
            meshes: Grow::new(device, "scene meshes", s),
            slots: Grow::new(device, "draw slots", s),
            face_args: Grow::new(device, "face draws", args),
            edge_args: Grow::new(device, "edge draws", args),
            last_objects: vec![],
            last_states: vec![],
            tables: Tables { order: vec![NONE], ..Default::default() },
            version: 0,
        }
    }

    /// Writes what changed. `tables` is moved in when it differs from the current one.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, objects: Vec<ObjectU>, states: Vec<u32>, tables: Tables) {
        let mut grown = false;
        if tables != self.tables {
            let bodies_changed = tables.bodies != self.tables.bodies;
            grown |= self.order.write(device, queue, bytemuck::cast_slice(&tables.order));
            grown |= self.meshes.write(device, queue, bytemuck::cast_slice(&tables.meshes));
            grown |= self.slots.write(device, queue, bytemuck::cast_slice(&tables.slots));
            grown |= self.face_args.write(device, queue, bytemuck::cast_slice(&tables.face_args));
            grown |= self.edge_args.write(device, queue, bytemuck::cast_slice(&tables.edge_args));
            grown |= self.codes.fit(device, (tables.order.len().max(4) * 4) as u64);
            let id_base = tables.identity_base();
            grown |= self.list.fit(device, ((id_base + tables.bodies).max(4) as u64) * 4);
            let identity: Vec<u32> = (0..tables.bodies).collect();
            if !identity.is_empty() {
                queue.write_buffer(&self.list.buf, id_base as u64 * 4, bytemuck::cast_slice(&identity));
            }
            // Visibility from the last frame is kept while the bodies stay the same.
            let vis_grown = self.visible.fit(device, (tables.bodies.max(4) as u64) * 4);
            if vis_grown || bodies_changed {
                queue.write_buffer(&self.visible.buf, 0, &vec![0u8; tables.bodies.max(4) as usize * 4]);
            }
            grown |= vis_grown;
            self.tables = tables;
        }
        if objects != self.last_objects {
            grown |= self.objects.write(device, queue, bytemuck::cast_slice(&objects));
            self.last_objects = objects;
        }
        if states != self.last_states {
            grown |= self.states.write(device, queue, bytemuck::cast_slice(&states));
            self.last_states = states;
        }
        if grown {
            self.version += 1;
        }
    }
}
