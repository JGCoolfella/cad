// Bodies straight from oc-mesh buffers: the 20-byte vertex (position, octahedral normal,
// face index), the 32-byte edge segment (start + edge index, end) and model vertices.
// Per-face and per-edge highlight state comes from small storage buffers, so a hover
// change rewrites a few bytes instead of the vertex colours.

struct Object {
  offset: vec4f,      // placed body origin relative to the render origin
  rx: vec4f,          // placement rotation, columns (assembly instances; identity for parts)
  ry: vec4f,
  rz: vec4f,
  color: vec4f,       // linear rgb, alpha
  edge: vec4f,        // edge colour rgb, edge width px (0 = frame default)
  material: vec4f,    // roughness, metalness, unused, unused
  flags: vec4u,       // x: OBJ_* bits, y: body slot (low 24 bits of the pick word), z, w unused
};

const OBJ_UNLIT: u32 = 1u;        // hidden-line styles: flat base colour
const OBJ_TINT: u32 = 2u;         // apply face highlight tints
const OBJ_CLIP: u32 = 4u;         // section plane applies
const OBJ_ONLY_HI_EDGES: u32 = 8u; // Shaded style: draw highlighted edges only
const OBJ_EDGE_STATES: u32 = 16u; // edge highlight states apply

struct Seg { a: vec4f, b: vec4f };

@group(1) @binding(0) var<uniform> obj: Object;
@group(1) @binding(1) var<storage, read> face_state: array<u32>;
@group(1) @binding(2) var<storage, read> edge_state: array<u32>;
@group(1) @binding(3) var<storage, read> segs: array<Seg>;
@group(1) @binding(4) var<storage, read> seg_dist: array<f32>;
@group(1) @binding(5) var<storage, read> points: array<vec4f>;

fn rot() -> mat3x3f {
  return mat3x3f(obj.rx.xyz, obj.ry.xyz, obj.rz.xyz);
}

fn placed(p: vec3f) -> vec3f {
  return rot() * p + obj.offset.xyz;
}

fn clipped(p: vec3f) -> bool {
  return (obj.flags.x & OBJ_CLIP) != 0u && is_cut(p);
}

// ---- faces

struct VIn {
  @location(0) pos: vec3f,
  @location(1) nrm: vec2f,
  @location(2) face: u32,
};

struct FOut {
  @builtin(position) clip_pos: vec4f,
  @location(0) world: vec3f,
  @location(1) nrm: vec3f,
  @location(2) @interpolate(flat) face: u32,
};

@vertex fn vs_face(i: VIn) -> FOut {
  var o: FOut;
  let w = placed(i.pos);
  o.clip_pos = to_clip(w);
  o.world = w;
  o.nrm = rot() * oct_decode(i.nrm);
  o.face = i.face;
  return o;
}

fn face_color(face: u32) -> vec3f {
  var base = obj.color.rgb;
  if ((obj.flags.x & OBJ_TINT) != 0u) {
    let st = face_state[face];
    if (st == 1u) { base = mix(base, fr.hover.rgb, fr.amounts.x); }
    if (st == 2u) { base = mix(base, fr.selected.rgb, fr.amounts.y); }
    if (st == 3u) { base = mix(base, fr.preview.rgb, fr.amounts.z); }
  }
  return base;
}

@fragment fn fs_face(v: FOut, @builtin(front_facing) ff: bool) -> @location(0) vec4f {
  if (clipped(v.world)) { discard; }
  let base = face_color(v.face);
  if ((obj.flags.x & OBJ_UNLIT) != 0u) { return vec4f(base, obj.color.a); }
  var n = normalize(v.nrm);
  if (!ff) { n = -n; }
  return vec4f(shade(base, n, v.world, obj.material.x, obj.material.y), obj.color.a);
}

@fragment fn fs_clip_only(v: FOut) -> @location(0) vec4f {
  if (clipped(v.world)) { discard; }
  return vec4f(0.0);
}

@fragment fn fs_face_id(v: FOut) -> @location(0) vec2u {
  if (clipped(v.world)) { discard; }
  return vec2u((1u << 24u) | obj.flags.y, v.face);
}

// ---- edges

struct EOut {
  @builtin(position) clip_pos: vec4f,
  @location(0) world: vec3f,
  @location(1) @interpolate(linear) along: f32,
  @location(2) @interpolate(flat) color: vec4f,
  @location(3) @interpolate(flat) id: vec2u,
};

fn edge_out(si: u32, k: u32, width: f32, color: vec4f) -> EOut {
  let s = segs[si];
  let wa = placed(s.a.xyz);
  let wb = placed(s.b.xyz);
  let e = expand_line(wa, wb, width, k);
  var o: EOut;
  o.clip_pos = e.pos;
  o.world = select(wa, wb, k == 1u || k == 2u || k == 4u);
  // Dash phase continues along the edge: distance of the segment's start in px.
  o.along = e.along + seg_dist[si] / fr.viewport.w * fr.viewport.z;
  o.color = color;
  o.id = vec2u((2u << 24u) | obj.flags.y, bitcast<u32>(s.a.w));
  return o;
}

@vertex fn vs_edge(@builtin(vertex_index) vi: u32, @builtin(instance_index) si: u32) -> EOut {
  let edge = bitcast<u32>(segs[si].a.w);
  var st = 0u;
  if ((obj.flags.x & OBJ_EDGE_STATES) != 0u) { st = edge_state[edge]; }
  var width = select(fr.widths.x, obj.edge.w, obj.edge.w > 0.0);
  var color = vec4f(obj.edge.rgb, 1.0);
  if (st == 1u) { color = fr.hover; width = fr.widths.y; }
  if (st == 2u) { color = fr.selected; width = fr.widths.y; }
  var o = edge_out(si, vi, width, color);
  if (st == 0u && (obj.flags.x & OBJ_ONLY_HI_EDGES) != 0u) { o.clip_pos = vec4f(0.0, 0.0, -1.0, 1.0); }
  return o;
}

@vertex fn vs_edge_hidden(@builtin(vertex_index) vi: u32, @builtin(instance_index) si: u32) -> EOut {
  return edge_out(si, vi, fr.widths.x, fr.hidden_edge);
}

@vertex fn vs_edge_pick(@builtin(vertex_index) vi: u32, @builtin(instance_index) si: u32) -> EOut {
  return edge_out(si, vi, 1.0, vec4f(0.0));
}

@fragment fn fs_edge(v: EOut) -> @location(0) vec4f {
  if (clipped(v.world)) { discard; }
  return v.color;
}

@fragment fn fs_edge_hidden(v: EOut) -> @location(0) vec4f {
  if (clipped(v.world)) { discard; }
  if (dashed_out(v.along, fr.dash.x, fr.dash.y)) { discard; }
  return v.color;
}

@fragment fn fs_edge_id(v: EOut) -> @location(0) vec2u {
  if (clipped(v.world)) { discard; }
  return v.id;
}

// ---- model vertices (pick targets)

struct POut {
  @builtin(position) clip_pos: vec4f,
  @location(0) world: vec3f,
  @location(1) @interpolate(flat) id: vec2u,
};

@vertex fn vs_vertex_pick(@builtin(vertex_index) vi: u32, @builtin(instance_index) pi: u32) -> POut {
  let p = points[pi];
  let w = placed(p.xyz);
  var o: POut;
  o.clip_pos = expand_point(w, fr.widths.z, vi);
  // POINT_HIDDEN vertices are skipped unless closed-edge vertices are shown.
  if ((bitcast<u32>(p.w) & 1u) != 0u && fr.flags.z == 0u) { o.clip_pos = vec4f(0.0, 0.0, -1.0, 1.0); }
  o.world = w;
  o.id = vec2u((3u << 24u) | obj.flags.y, pi);
  return o;
}

@fragment fn fs_vertex_id(v: POut) -> @location(0) vec2u {
  if (clipped(v.world)) { discard; }
  return v.id;
}
