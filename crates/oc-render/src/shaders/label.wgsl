// Plane name labels: a textured quad lying in the plane (caller-rasterised image).

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct LIn {
  @location(0) pos: vec3f,
  @location(1) uv: vec2f,
};

struct LOut {
  @builtin(position) clip_pos: vec4f,
  @location(0) uv: vec2f,
};

@vertex fn vs_label(i: LIn) -> LOut {
  var o: LOut;
  o.clip_pos = to_clip(i.pos);
  o.uv = i.uv;
  return o;
}

@fragment fn fs_label(v: LOut) -> @location(0) vec4f {
  let c = textureSample(tex, samp, v.uv);
  if (c.a <= 0.0) { discard; }
  return c;
}
