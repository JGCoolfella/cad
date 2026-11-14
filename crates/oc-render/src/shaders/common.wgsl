// Shared by every oc-render shader module (prepended to each). Positions reaching the GPU
// are relative to the camera target (the render origin), so f32 only holds small numbers.
// Depth is reversed: near = 1, far = 0.

struct FrameU {
  view_proj: mat4x4f,
  eye: vec4f,        // eye relative to the render origin; w = 1 perspective, 0 orthographic
  view_dir: vec4f,   // unit vector from the target towards the eye
  viewport: vec4f,   // target width px, height px, px per logical px, mm per logical px
  clip: vec4f,       // section plane: fragments with dot(p, n) > d are cut away
  light1: vec4f,     // world direction towards the light, intensity
  light2: vec4f,
  sky: vec4f,        // hemisphere light: sky colour, intensity
  ground: vec4f,
  hover: vec4f,
  selected: vec4f,
  preview: vec4f,
  hidden_edge: vec4f,
  amounts: vec4f,    // face tint: hover, selected, preview; unused
  widths: vec4f,     // edge px, highlighted edge px, vertex pick px, unused
  dash: vec4f,       // hidden edge dash px, gap px, unused, unused
  bg_top: vec4f,     // background colours, sRGB-encoded (CSS interpolates in sRGB)
  bg_mid: vec4f,
  bg_bottom: vec4f,
  flags: vec4u,      // x: section on, y: background mode, z: show closed-edge vertices, w: unused
};

@group(0) @binding(0) var<uniform> fr: FrameU;

const PI: f32 = 3.141592653589793;

fn to_clip(p: vec3f) -> vec4f {
  return fr.view_proj * vec4f(p, 1.0);
}

fn is_cut(p: vec3f) -> bool {
  return fr.flags.x != 0u && dot(p, fr.clip.xyz) > fr.clip.w;
}

fn oct_decode(e: vec2f) -> vec3f {
  var n = vec3f(e, 1.0 - abs(e.x) - abs(e.y));
  let t = max(-n.z, 0.0);
  n.x += select(t, -t, n.x >= 0.0);
  n.y += select(t, -t, n.y >= 0.0);
  return normalize(n);
}

// three.js MeshStandardMaterial with physically based lights (r155+): Lambert diffuse
// over pi, GGX specular (Smith correlated visibility, Schlick Fresnel), a hemisphere light
// in world space and two directional lights fixed to the camera.
fn d_ggx(alpha: f32, nh: f32) -> f32 {
  let a2 = alpha * alpha;
  let d = nh * nh * (a2 - 1.0) + 1.0;
  return a2 / (PI * d * d);
}

fn v_smith(alpha: f32, nl: f32, nv: f32) -> f32 {
  let a2 = alpha * alpha;
  let gv = nl * sqrt(a2 + (1.0 - a2) * nv * nv);
  let gl = nv * sqrt(a2 + (1.0 - a2) * nl * nl);
  return 0.5 / max(gv + gl, 1e-6);
}

fn direct(albedo: vec3f, f0: vec3f, alpha: f32, n: vec3f, v: vec3f, l: vec3f, intensity: f32) -> vec3f {
  let nl = clamp(dot(n, l), 0.0, 1.0);
  if (nl <= 0.0) { return vec3f(0.0); }
  let h = normalize(l + v);
  let nv = clamp(abs(dot(n, v)), 1e-4, 1.0);
  let nh = clamp(dot(n, h), 0.0, 1.0);
  let vh = clamp(dot(v, h), 0.0, 1.0);
  let fres = f0 + (vec3f(1.0) - f0) * pow(1.0 - vh, 5.0);
  let spec = fres * (v_smith(alpha, nl, nv) * d_ggx(alpha, nh));
  return intensity * nl * (albedo / PI + spec);
}

fn shade(base: vec3f, n: vec3f, p: vec3f, roughness: f32, metalness: f32) -> vec3f {
  let albedo = base * (1.0 - metalness);
  let f0 = mix(vec3f(0.04), base, metalness);
  let r = max(roughness, 0.0525);
  let alpha = r * r;
  var v = fr.view_dir.xyz;
  if (fr.eye.w > 0.5) { v = normalize(fr.eye.xyz - p); }
  let w = 0.5 * n.y + 0.5;
  let hemi = mix(fr.ground.rgb, fr.sky.rgb, w) * fr.sky.w;
  var c = hemi * albedo / PI;
  c += direct(albedo, f0, alpha, n, v, fr.light1.xyz, fr.light1.w);
  c += direct(albedo, f0, alpha, n, v, fr.light2.xyz, fr.light2.w);
  return c;
}

// ---- screen-space wide lines and square points

struct Expanded {
  pos: vec4f,
  along: f32,   // physical px from the segment's start
  keep: bool,
};

// Corner k of the quad (two triangles) covering segment a-b, `width` physical px wide,
// with square caps. Segments crossing the eye plane are clipped.
fn expand_line(wa: vec3f, wb: vec3f, width: f32, k: u32) -> Expanded {
  var ca = to_clip(wa);
  var cb = to_clip(wb);
  var o: Expanded;
  o.keep = true;
  let eps = 1e-5;
  if (ca.w < eps && cb.w < eps) { o.keep = false; o.pos = vec4f(0.0, 0.0, -1.0, 1.0); return o; }
  if (ca.w < eps) { ca = mix(ca, cb, (eps - ca.w) / (cb.w - ca.w)); }
  if (cb.w < eps) { cb = mix(cb, ca, (eps - cb.w) / (ca.w - cb.w)); }
  let half = fr.viewport.xy * 0.5;
  let sa = ca.xy / ca.w * half;
  let sb = cb.xy / cb.w * half;
  var d = sb - sa;
  let len = length(d);
  var u = vec2f(1.0, 0.0);
  if (len > 1e-6) { u = d / len; }
  let nrm = vec2f(-u.y, u.x);
  // corners: (end, side) = (0,-)(1,-)(1,+)(0,-)(1,+)(0,+)
  var t = 0.0;
  var side = -1.0;
  switch k {
    case 1u: { t = 1.0; }
    case 2u: { t = 1.0; side = 1.0; }
    case 4u: { t = 1.0; side = 1.0; }
    case 5u: { side = 1.0; }
    default: {}
  }
  let p = select(ca, cb, t > 0.5);
  let off = nrm * side * width * 0.5 + u * (t * 2.0 - 1.0) * width * 0.5;
  o.pos = vec4f(p.xy + off / half * p.w, p.z, p.w);
  o.along = select(-width * 0.5, len + width * 0.5, t > 0.5);
  return o;
}

fn expand_point(wp: vec3f, size: f32, k: u32) -> vec4f {
  let c = to_clip(wp);
  var off = vec2f(-1.0, -1.0);
  switch k {
    case 1u: { off = vec2f(1.0, -1.0); }
    case 2u: { off = vec2f(1.0, 1.0); }
    case 4u: { off = vec2f(1.0, 1.0); }
    case 5u: { off = vec2f(-1.0, 1.0); }
    default: {}
  }
  let half = fr.viewport.xy * 0.5;
  return vec4f(c.xy + off * size * 0.5 / half * c.w, c.z, c.w);
}

fn dashed_out(along: f32, dash: f32, gap: f32) -> bool {
  if (dash <= 0.0) { return false; }
  let period = dash + gap;
  let m = along - floor(along / period) * period;
  return m > dash;
}
