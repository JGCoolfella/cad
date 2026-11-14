// World triangles (plane fills, the section cap), the background and the screen-space
// overlay (origin, triad).

struct TIn {
  @location(0) pos: vec3f,
  @location(1) nrm: vec3f,
  @location(2) color: vec4f,
  @location(3) back: vec4f,
  @location(4) id: vec2u,
  @location(5) flags: u32,   // 1: lit (section cap material)
};

struct TOut {
  @builtin(position) clip_pos: vec4f,
  @location(0) world: vec3f,
  @location(1) @interpolate(flat) nrm: vec3f,
  @location(2) @interpolate(flat) color: vec4f,
  @location(3) @interpolate(flat) back: vec4f,
  @location(4) @interpolate(flat) id: vec2u,
  @location(5) @interpolate(flat) flags: u32,
};

@vertex fn vs_tri(i: TIn) -> TOut {
  var o: TOut;
  o.clip_pos = to_clip(i.pos);
  o.world = i.pos;
  o.nrm = i.nrm;
  o.color = i.color;
  o.back = i.back;
  o.id = i.id;
  o.flags = i.flags;
  return o;
}

@fragment fn fs_tri(v: TOut, @builtin(front_facing) ff: bool) -> @location(0) vec4f {
  let c = select(v.back, v.color, ff);
  if ((v.flags & 1u) != 0u) {
    var n = normalize(v.nrm);
    if (!ff) { n = -n; }
    return vec4f(shade(c.rgb, n, v.world, 0.6, 0.05), c.a);
  }
  return c;
}

@fragment fn fs_tri_id(v: TOut) -> @location(0) vec2u {
  return v.id;
}

// ---- background: a full-screen triangle

struct BOut {
  @builtin(position) clip_pos: vec4f,
  @location(0) uv: vec2f,
};

@vertex fn vs_bg(@builtin(vertex_index) vi: u32) -> BOut {
  let x = f32((vi << 1u) & 2u);
  let y = f32(vi & 2u);
  var o: BOut;
  o.clip_pos = vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
  o.uv = vec2f(x, y);
  return o;
}

fn srgb_to_linear(c: vec3f) -> vec3f {
  return select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92, c <= vec3f(0.04045));
}

@fragment fn fs_bg(v: BOut) -> @location(0) vec4f {
  let mode = fr.flags.y;
  if (mode == 3u) { return vec4f(0.0); }
  if (mode == 2u) { return vec4f(srgb_to_linear(fr.bg_bottom.rgb), 1.0); }
  var c: vec3f;
  if (mode == 1u) {
    c = mix(fr.bg_top.rgb, fr.bg_bottom.rgb, v.uv.y);
  } else {
    // CSS radial-gradient(ellipse at 50% 45%, top 0%, mid 45%, bottom 100%): the ellipse
    // has the closest-side aspect (w/2 : 0.45 h), scaled to pass through the farthest
    // corner, a factor sqrt(1 + (0.55 / 0.45)^2).
    let w = fr.viewport.x;
    let h = fr.viewport.y;
    let dx = (v.uv.x - 0.5) * w / (0.5 * w);
    let dy = (v.uv.y - 0.45) * h / (0.45 * h);
    let t = sqrt(dx * dx + dy * dy) / sqrt(1.0 + (0.55 / 0.45) * (0.55 / 0.45));
    if (t < 0.45) {
      c = mix(fr.bg_top.rgb, fr.bg_mid.rgb, t / 0.45);
    } else {
      c = mix(fr.bg_mid.rgb, fr.bg_bottom.rgb, clamp((t - 0.45) / 0.55, 0.0, 1.0));
    }
  }
  return vec4f(srgb_to_linear(c), 1.0);
}

// ---- overlay: triangles in physical pixels, y down

struct OIn {
  @location(0) pos: vec2f,
  @location(1) color: vec4f,
};

struct OOut {
  @builtin(position) clip_pos: vec4f,
  @location(0) color: vec4f,
};

@vertex fn vs_overlay(i: OIn) -> OOut {
  var o: OOut;
  o.clip_pos = vec4f(i.pos.x / fr.viewport.x * 2.0 - 1.0, 1.0 - i.pos.y / fr.viewport.y * 2.0, 1.0, 1.0);
  o.color = i.color;
  return o;
}

@fragment fn fs_overlay(v: OOut) -> @location(0) vec4f {
  return v.color;
}
