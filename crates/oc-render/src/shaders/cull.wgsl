// G2 culling (`cull.rs`). One invocation per instance tests its placed bounding box
// against the view frustum (with a margin for wide lines), the section plane and the
// hierarchical depth buffer (Hi-Z), chooses its level of detail and writes a code: the
// draw kinds it joins and its level. A second pass (`compact`) turns the codes into one
// list per draw slot and kind, in scene order (deterministic: coplanar faces of different
// instances resolve the same way every frame), and writes the instance counts of the
// indirect draws.

struct Object {
  offset: vec4f,
  rx: vec4f,
  ry: vec4f,
  rz: vec4f,
  color: vec4f,
  edge: vec4f,
  material: vec4f,
  flags: vec4u,   // x: OBJ_* bits, y: slot, z: face state base, w: edge state base
  cull: vec4u,    // x: scene mesh, y: kind bits, z: LOD mode (0 finest, 1 auto, 2 level), w: level
  extra: vec4u,   // x: first model vertex, y: LOD max error px (f32 bits)
};

struct SceneMesh {
  lo: vec4f,
  hi: vec4f,
  info: vec4u,    // first slot, levels, first position in order, instances
};

struct Slot {
  info: vec4u,    // first position in order, instances, list base, level
  linear: vec4f,  // chordal deviation of the level (mm)
};

struct CullU {
  view_proj: mat4x4f,
  eye: vec4f,     // eye relative to the render origin; w = 1 perspective
  lod: vec4f,     // x: mm per px (orthographic) or mm per px per mm of distance (perspective)
  clip: vec4f,    // section plane: n, d (cut away where dot(p, n) > d)
  screen: vec4f,  // target width, height (px), margin px, smallest occluder (px across)
  mode: vec4u,    // x: phase (0 prepass, 1 main, 2 pick), y: Hi-Z levels (0: none), z: section on, w: instances
  opts: vec4u,    // x: culling on, y: kinds of this phase
};

const OBJ_CLIP: u32 = 4u;
const NONE: u32 = 0xffffffffu;
const B_PRE: u32 = 1u;
// Opaque faces, transparent faces and edges of an occluded instance are not drawn. Hidden
// edges (HLV) and the section parity are: they show what is behind.
const OCCLUDABLE: u32 = 2u | 4u | 8u;

@group(0) @binding(0) var<uniform> cu: CullU;
@group(0) @binding(1) var<storage, read> objects: array<Object>;
@group(0) @binding(2) var<storage, read> meshes: array<SceneMesh>;
@group(0) @binding(3) var<storage, read> slots: array<Slot>;
@group(0) @binding(4) var<storage, read> order: array<u32>;
@group(0) @binding(5) var<storage, read_write> codes: array<u32>;
@group(0) @binding(6) var<storage, read_write> visible: array<u32>;
@group(0) @binding(7) var hiz: texture_2d<f32>;

fn level_of(o: Object, m: SceneMesh, center: vec3f, radius: f32) -> u32 {
  let n = m.info.y;
  if (n <= 1u || o.cull.z == 0u) { return 0u; }
  if (o.cull.z == 2u) { return min(o.cull.w, n - 1u); }
  var mm = cu.lod.x;
  if (cu.eye.w > 0.5) { mm = cu.lod.x * max(length(center - cu.eye.xyz) - radius, 1e-3); }
  let limit = mm * bitcast<f32>(o.extra.y);
  var best = 0u;
  for (var l = 0u; l < n; l++) {
    if (slots[m.info.x + l].linear.x <= limit) { best = l; }
  }
  return best;
}

// The screen rectangle (NDC) and nearest depth of a box are behind the Hi-Z everywhere.
fn occluded(smin: vec2f, smax: vec2f, zmax: f32) -> bool {
  let size = cu.screen.xy;
  let m = cu.screen.z;
  let lo = clamp(vec2f((smin.x * 0.5 + 0.5) * size.x, (0.5 - smax.y * 0.5) * size.y) - m, vec2f(0.0), size - 1.0);
  let hi = clamp(vec2f((smax.x * 0.5 + 0.5) * size.x, (0.5 - smin.y * 0.5) * size.y) + m, vec2f(0.0), size - 1.0);
  let ext = max(hi.x - lo.x, hi.y - lo.y);
  // A texel of level L covers 2^(L+1) px: at the level where that is at least half the
  // rectangle, it spans at most 3 × 3 texels.
  let lv = min(u32(max(ceil(log2(max(ext, 1.0))) - 2.0, 0.0)), cu.mode.y - 1u);
  let dims = textureDimensions(hiz, lv);
  let sh = vec2u(lv + 1u);
  let t0 = min(vec2u(lo) >> sh, dims - 1u);
  let t1 = min(vec2u(hi) >> sh, dims - 1u);
  var far = 1.0;
  for (var y = t0.y; y <= t1.y; y++) {
    for (var x = t0.x; x <= t1.x; x++) {
      far = min(far, textureLoad(hiz, vec2u(x, y), i32(lv)).x);
    }
  }
  // Reverse depth: the box's nearest point is farther than the farthest occluder.
  return zmax < far - 1e-6;
}

@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) gid: vec3u) {
  let j = gid.x;
  if (j >= cu.mode.w) { return; }
  let i = order[j];
  let o = objects[i];
  let phase = cu.mode.x;
  if (o.cull.x == NONE) {
    codes[j] = 0u;
    return;
  }
  let m = meshes[o.cull.x];
  let r = mat3x3f(o.rx.xyz, o.ry.xyz, o.rz.xyz);
  let lo = m.lo.xyz;
  let hi = m.hi.xyz;
  let mx = 1.0 + 2.0 * cu.screen.z / cu.screen.x;
  let my = 1.0 + 2.0 * cu.screen.z / cu.screen.y;
  // Bits of the planes every corner so far is outside of: left, right, below, above.
  // (Integer masks: `&` on bool vectors miscompiled on D3D12 with optimisation.)
  var out = 15u;
  var all_behind = true;
  var any_behind = false;
  var all_cut = true;
  var zmax = -1.0;
  var smin = vec2f(1e30);
  var smax = vec2f(-1e30);
  for (var k = 0u; k < 8u; k++) {
    let c = vec3f(select(lo.x, hi.x, (k & 1u) != 0u), select(lo.y, hi.y, (k & 2u) != 0u), select(lo.z, hi.z, (k & 4u) != 0u));
    let p = o.offset.xyz + r * c;
    if (!(dot(p, cu.clip.xyz) > cu.clip.w)) { all_cut = false; }
    let h = cu.view_proj * vec4f(p, 1.0);
    out &= select(0u, 1u, h.x < -mx * h.w) | select(0u, 2u, h.x > mx * h.w) | select(0u, 4u, h.y < -my * h.w) | select(0u, 8u, h.y > my * h.w);
    if (h.w <= 1e-6) {
      any_behind = true;
    } else {
      all_behind = false;
      let n = h.xyz / h.w;
      zmax = max(zmax, n.z);
      smin = min(smin, n.xy);
      smax = max(smax, n.xy);
    }
  }
  let on = cu.opts.x != 0u;
  let outside = out != 0u || all_behind;
  let cut = cu.mode.z != 0u && (o.flags.x & OBJ_CLIP) != 0u && all_cut;
  if (on && (outside || cut)) {
    codes[j] = 0u;
    if (phase == 1u) { visible[i] = 0u; }
    return;
  }
  let center = o.offset.xyz + r * ((lo + hi) * 0.5);
  let level = level_of(o, m, center, length(hi - lo) * 0.5);
  var mask = o.cull.y & cu.opts.y;
  // Occluders: instances visible last frame that are large on screen (small ones hide
  // little and would make the prepass cost as much as the frame).
  let ext = (smax - smin) * 0.5 * cu.screen.xy;
  let big = any_behind || max(ext.x, ext.y) >= cu.screen.w;
  if (phase == 0u) {
    if (visible[i] == 0u || !big) { mask = 0u; }
  } else if (phase == 1u) {
    let hz = cu.mode.y > 0u && on;
    var hidden = false;
    if (hz && !any_behind) { hidden = occluded(smin, smax, zmax); }
    // Drawn in the prepass this frame: its depth is in the buffer, so it is drawn.
    let pre = hz && visible[i] != 0u && big && (o.cull.y & B_PRE) != 0u;
    if (hidden && !pre) { mask = mask & ~OCCLUDABLE; }
    visible[i] = select(1u, 0u, hidden);
  }
  codes[j] = (level << 16u) | mask;
}
