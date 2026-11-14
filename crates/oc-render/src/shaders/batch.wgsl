// Per-frame batches of world lines and square points: sketches, reference axes, coordinate
// systems, plane borders, reference points and vertex markers.

struct Line {
  a: vec4f,      // start (relative to the render origin), width px
  b: vec4f,      // end, dash px (0 = solid)
  color: vec4f,  // linear rgba
  id: vec4u,     // pick word, pick index, gap px (f32 bits), unused
};

struct Pt {
  p: vec4f,      // position, size px
  color: vec4f,
  id: vec4u,     // pick word, pick index, unused, unused
};

@group(1) @binding(0) var<storage, read> lines: array<Line>;
@group(1) @binding(1) var<storage, read> pts: array<Pt>;

struct LOut {
  @builtin(position) clip_pos: vec4f,
  @location(0) @interpolate(linear) along: f32,
  @location(1) @interpolate(flat) color: vec4f,
  @location(2) @interpolate(flat) id: vec2u,
  @location(3) @interpolate(flat) dash: vec2f,
};

@vertex fn vs_line(@builtin(vertex_index) vi: u32, @builtin(instance_index) li: u32) -> LOut {
  let l = lines[li];
  let e = expand_line(l.a.xyz, l.b.xyz, l.a.w, vi);
  var o: LOut;
  o.clip_pos = e.pos;
  o.along = e.along;
  o.color = l.color;
  o.id = l.id.xy;
  o.dash = vec2f(l.b.w, bitcast<f32>(l.id.z));
  return o;
}

@vertex fn vs_line_pick(@builtin(vertex_index) vi: u32, @builtin(instance_index) li: u32) -> LOut {
  let l = lines[li];
  let e = expand_line(l.a.xyz, l.b.xyz, 1.0, vi);
  var o: LOut;
  o.clip_pos = e.pos;
  o.along = 0.0;
  o.color = vec4f(0.0);
  o.id = l.id.xy;
  o.dash = vec2f(0.0);
  return o;
}

@fragment fn fs_line(v: LOut) -> @location(0) vec4f {
  if (dashed_out(v.along, v.dash.x, v.dash.y)) { discard; }
  return v.color;
}

@fragment fn fs_line_id(v: LOut) -> @location(0) vec2u {
  return v.id;
}

struct PtOut {
  @builtin(position) clip_pos: vec4f,
  @location(0) @interpolate(flat) color: vec4f,
  @location(1) @interpolate(flat) id: vec2u,
};

@vertex fn vs_point(@builtin(vertex_index) vi: u32, @builtin(instance_index) pi: u32) -> PtOut {
  let p = pts[pi];
  var o: PtOut;
  o.clip_pos = expand_point(p.p.xyz, p.p.w, vi);
  o.color = p.color;
  o.id = p.id.xy;
  return o;
}

@fragment fn fs_point(v: PtOut) -> @location(0) vec4f {
  return v.color;
}

@fragment fn fs_point_id(v: PtOut) -> @location(0) vec2u {
  return v.id;
}
