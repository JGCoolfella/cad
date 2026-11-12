// P4 spike shaders. All positions reaching the GPU are camera-relative (the camera is at
// the origin); the CPU subtracts the camera position in f64 before converting to f32.

struct Cam {
  view_proj: mat4x4f,
  clip: vec4f,      // plane n.xyz, d: fragments with dot(p, n) > d are cut away
  viewport: vec4f,  // width px, height px, edge width px, 0
  flags: vec4u,     // x: section on
};

struct Inst {
  m0: vec4f, m1: vec4f, m2: vec4f, // rows of the affine model matrix
  color: u32, id: u32, pad0: u32, pad1: u32,
};

struct Seg { a: vec4f, b: vec4f };
struct MeshParam { seg_base: u32, nseg: u32, inst_base: u32, pad: u32 };

@group(0) @binding(0) var<uniform> cam: Cam;
@group(0) @binding(1) var<storage, read> insts: array<Inst>;
@group(0) @binding(2) var<storage, read> segs: array<Seg>;
@group(0) @binding(3) var<storage, read> meshes: array<MeshParam>;

struct VIn {
  @location(0) pos: vec3f,
  @location(1) nrm: vec3f,
  @location(7) face: u32,
  @location(2) m0: vec4f,
  @location(3) m1: vec4f,
  @location(4) m2: vec4f,
  @location(5) color: u32,
  @location(6) id: u32,
};

struct VOut {
  @builtin(position) clip_pos: vec4f,
  @location(0) world: vec3f,
  @location(1) nrm: vec3f,
  @location(2) @interpolate(flat) color: u32,
  @location(3) @interpolate(flat) id: vec2u,
};

fn xf(m0: vec4f, m1: vec4f, m2: vec4f, p: vec4f) -> vec3f {
  return vec3f(dot(m0, p), dot(m1, p), dot(m2, p));
}

fn cut(p: vec3f) -> bool {
  return cam.flags.x != 0u && dot(p, cam.clip.xyz) > cam.clip.w;
}

@vertex fn vs_mesh(i: VIn) -> VOut {
  var o: VOut;
  let w = xf(i.m0, i.m1, i.m2, vec4f(i.pos, 1.0));
  o.clip_pos = cam.view_proj * vec4f(w, 1.0);
  o.world = w;
  o.nrm = xf(i.m0, i.m1, i.m2, vec4f(i.nrm, 0.0)); // uniform scale only
  o.color = i.color;
  o.id = vec2u(i.id + 1u, i.face);
  return o;
}

@fragment fn fs_shaded(v: VOut, @builtin(front_facing) ff: bool) -> @location(0) vec4f {
  if (cut(v.world)) { discard; }
  var n = normalize(v.nrm);
  if (!ff) { n = -n; }
  let l = normalize(-v.world); // headlight: light at the camera
  let d = max(dot(n, l), 0.0);
  let base = unpack4x8unorm(v.color).rgb;
  let c = base * (0.25 + 0.75 * d) + vec3f(0.35) * pow(d, 40.0);
  return vec4f(c, 1.0);
}

@fragment fn fs_clip_only(v: VOut) -> @location(0) vec4f {
  if (cut(v.world)) { discard; }
  return vec4f(0.0);
}

@fragment fn fs_id(v: VOut) -> @location(0) vec2u {
  if (cut(v.world)) { discard; }
  return v.id;
}

// ---- wide edge lines: each segment instance becomes a screen-space quad.

struct LOut {
  @builtin(position) clip_pos: vec4f,
  @location(0) world: vec3f,
  @location(1) @interpolate(flat) id: vec2u,
};

@vertex fn vs_line(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> LOut {
  let mp = meshes[vi / 6u];
  let corner = vi % 6u;
  let obj = insts[mp.inst_base + ii / mp.nseg];
  let si = mp.seg_base + ii % mp.nseg;
  let s = segs[si];
  let wa = xf(obj.m0, obj.m1, obj.m2, vec4f(s.a.xyz, 1.0));
  let wb = xf(obj.m0, obj.m1, obj.m2, vec4f(s.b.xyz, 1.0));
  let a = cam.view_proj * vec4f(wa, 1.0);
  let b = cam.view_proj * vec4f(wb, 1.0);
  let half = cam.viewport.xy * 0.5;
  let sa = a.xy / a.w * half;
  let sb = b.xy / b.w * half;
  var dir = sb - sa;
  if (dot(dir, dir) < 1e-12) { dir = vec2f(1.0, 0.0); }
  let n = normalize(vec2f(-dir.y, dir.x)) * cam.viewport.z * 0.5;
  // corners: (end, side) = (0,-)(1,-)(1,+)(0,-)(1,+)(0,+)
  var t = 0.0; var side = -1.0;
  switch corner {
    case 1u: { t = 1.0; }
    case 2u: { t = 1.0; side = 1.0; }
    case 4u: { t = 1.0; side = 1.0; }
    case 5u: { side = 1.0; }
    default: {}
  }
  let p = select(a, b, t > 0.5);
  var o: LOut;
  o.clip_pos = vec4f(p.xy + n * side / half * p.w, p.z, p.w);
  o.world = select(wa, wb, t > 0.5);
  o.id = vec2u(obj.id + 1u, 0x80000000u | (si - mp.seg_base));
  return o;
}

@fragment fn fs_line(v: LOut) -> @location(0) vec4f {
  if (cut(v.world)) { discard; }
  return vec4f(0.05, 0.05, 0.08, 1.0);
}

@fragment fn fs_line_id(v: LOut) -> @location(0) vec2u {
  if (cut(v.world)) { discard; }
  return v.id;
}

// ---- section cap: a quad on the clip plane, drawn where the stencil parity is odd.

@vertex fn vs_cap(@location(0) p: vec3f) -> @builtin(position) vec4f {
  return cam.view_proj * vec4f(p, 1.0);
}

@fragment fn fs_cap() -> @location(0) vec4f {
  return vec4f(0.80, 0.35, 0.30, 1.0);
}
