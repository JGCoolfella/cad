//! The geometry pool (G2): every uploaded mesh level lives in five shared GPU buffers
//! (vertices, 32-bit indices, edge segments, segment distances, model vertices), so one
//! `multi_draw_*_indirect` call draws every instance of every mesh in a pass. A mesh is
//! appended when it is added; removing one leaves a hole that is reclaimed by compaction
//! (re-uploading the live meshes from their CPU copies) once holes outweigh live data.

/// One shared buffer of fixed-size elements, grown by doubling (the old contents copied on
/// the GPU).
pub(crate) struct Arena {
    pub buf: wgpu::Buffer,
    /// Elements the buffer holds.
    cap: u64,
    /// Elements in use (live and dead).
    pub len: u64,
    /// Elements of removed meshes.
    pub dead: u64,
    stride: u64,
    usage: wgpu::BufferUsages,
    label: &'static str,
    /// Largest buffer the device allows for this use, bytes.
    max: u64,
}

impl Arena {
    fn new(device: &wgpu::Device, label: &'static str, stride: u64, usage: wgpu::BufferUsages, max: u64) -> Arena {
        let cap = (4096 / stride).max(4);
        Arena { buf: Self::buffer(device, label, cap * stride, usage), cap, len: 0, dead: 0, stride, usage, label, max }
    }

    fn buffer(device: &wgpu::Device, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: usage | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    }

    /// Appends `bytes` (whole elements) and returns the first element's index, or None when
    /// the buffer would exceed the device's limit. Sets `grown` when the buffer was replaced.
    fn push(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8], grown: &mut bool) -> Option<u64> {
        let n = bytes.len() as u64 / self.stride;
        debug_assert_eq!(n * self.stride, bytes.len() as u64);
        if self.len + n > self.cap {
            let cap = (self.cap * 2).max(self.len + n).next_power_of_two();
            let size = (cap * self.stride).min(self.max / self.stride * self.stride);
            if size < (self.len + n) * self.stride {
                return None;
            }
            let buf = Self::buffer(device, self.label, size, self.usage);
            let mut enc = device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(&self.buf, 0, &buf, 0, self.len * self.stride);
            queue.submit([enc.finish()]);
            self.buf = buf;
            self.cap = size / self.stride;
            *grown = true;
        }
        let first = self.len;
        if !bytes.is_empty() {
            queue.write_buffer(&self.buf, first * self.stride, bytes);
        }
        self.len += n;
        Some(first)
    }
}

pub(crate) const SEG_STRIDE: u64 = oc_mesh::SEGMENT_STRIDE as u64;
pub(crate) const VERT_STRIDE: u64 = oc_mesh::VERTEX_STRIDE as u64;

pub(crate) struct Pool {
    pub vertices: Arena,
    pub indices: Arena,
    pub segs: Arena,
    pub seg_dist: Arena,
    pub points: Arena,
    /// Changes whenever a buffer is replaced (bind groups that name them are rebuilt).
    pub generation: u64,
}

/// A level's place in the pool.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LevelRange {
    pub first_index: u32,
    pub index_count: u32,
    pub base_vertex: i32,
    pub vertex_count: u32,
    pub seg_base: u32,
    pub seg_count: u32,
}

impl Pool {
    pub fn new(device: &wgpu::Device) -> Pool {
        let l = device.limits();
        let storage = l.max_storage_buffer_binding_size.min(l.max_buffer_size);
        let s = wgpu::BufferUsages::STORAGE;
        Pool {
            vertices: Arena::new(device, "pool vertices", VERT_STRIDE, wgpu::BufferUsages::VERTEX, l.max_buffer_size),
            indices: Arena::new(device, "pool indices", 4, wgpu::BufferUsages::INDEX, l.max_buffer_size),
            segs: Arena::new(device, "pool segments", SEG_STRIDE, s, storage),
            seg_dist: Arena::new(device, "pool segment distances", 4, s, storage),
            points: Arena::new(device, "pool points", 16, s, storage),
            generation: 0,
        }
    }

    /// Uploads one level. None when the pool is full (the device's buffer limits).
    pub fn push_level(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, l: &oc_mesh::Lod) -> Option<LevelRange> {
        let mut grown = false;
        let idx: Vec<u32> = (0..l.index_count as usize).map(|i| l.index(i)).collect();
        let dist = crate::mesh::segment_distances(l);
        let seg_bytes = &l.segments[..l.segment_count as usize * oc_mesh::SEGMENT_STRIDE];
        let vert_bytes = &l.vertices[..l.vertex_count as usize * oc_mesh::VERTEX_STRIDE];
        let r = (|| {
            let r = LevelRange {
                base_vertex: self.vertices.push(device, queue, vert_bytes, &mut grown)? as i32,
                vertex_count: l.vertex_count,
                first_index: self.indices.push(device, queue, bytemuck::cast_slice(&idx), &mut grown)? as u32,
                index_count: l.index_count,
                seg_base: self.segs.push(device, queue, seg_bytes, &mut grown)? as u32,
                seg_count: l.segment_count,
            };
            self.seg_dist.push(device, queue, bytemuck::cast_slice(&dist), &mut grown)?;
            Some(r)
        })();
        if grown {
            self.generation += 1;
        }
        r
    }

    /// Uploads a mesh's model vertices (position, flags) and returns the first one.
    pub fn push_points(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pts: &[[f32; 4]]) -> Option<u32> {
        let mut grown = false;
        let r = self.points.push(device, queue, bytemuck::cast_slice(pts), &mut grown);
        if grown {
            self.generation += 1;
        }
        r.map(|x| x as u32)
    }

    /// Marks a removed mesh's elements as dead.
    pub fn release(&mut self, levels: &[LevelRange], points: u32) {
        for l in levels {
            self.vertices.dead += l.vertex_count as u64;
            self.indices.dead += l.index_count as u64;
            self.segs.dead += l.seg_count as u64;
            self.seg_dist.dead += l.seg_count as u64;
        }
        self.points.dead += points as u64;
    }

    /// Bytes held by removed meshes and in use overall.
    pub fn waste(&self) -> (u64, u64) {
        let a = [&self.vertices, &self.indices, &self.segs, &self.seg_dist, &self.points];
        (a.iter().map(|x| x.dead * x.stride).sum(), a.iter().map(|x| x.len * x.stride).sum())
    }
}

/// A storage or indirect buffer that grows to fit (contents are rewritten by the caller).
pub(crate) struct Grow {
    pub buf: wgpu::Buffer,
    size: u64,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl Grow {
    pub fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages) -> Grow {
        let usage = usage | wgpu::BufferUsages::COPY_DST;
        Grow { buf: device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: 256, usage, mapped_at_creation: false }), size: 256, usage, label }
    }

    /// Makes room for `bytes`; true when the buffer was replaced (its contents are lost).
    pub fn fit(&mut self, device: &wgpu::Device, bytes: u64) -> bool {
        if bytes <= self.size {
            return false;
        }
        self.size = bytes.next_power_of_two().max(256);
        self.buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some(self.label), size: self.size, usage: self.usage, mapped_at_creation: false });
        true
    }

    /// Replaces the contents with `bytes` (growing first). True when the buffer was replaced.
    pub fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) -> bool {
        let grown = self.fit(device, bytes.len().max(16) as u64);
        if !bytes.is_empty() {
            queue.write_buffer(&self.buf, 0, bytes);
        }
        grown
    }
}
