// G2 Hi-Z (`cull.rs`): level 0 is half the depth buffer's size (rounded up), each texel
// the farthest (smallest, reverse Z) depth of its 2 × 2 pixels and all their samples; each
// further level halves again. Reads are clamped at the edge.

@group(0) @binding(0) var dst: texture_storage_2d<r32float, write>;
@group(0) @binding(1) var src_ms: texture_depth_multisampled_2d;
@group(0) @binding(2) var src_one: texture_depth_2d;
@group(0) @binding(3) var src_level: texture_2d<f32>;

@compute @workgroup_size(8, 8)
fn hiz_ms(@builtin(global_invocation_id) g: vec3u) {
  let d = textureDimensions(dst);
  if (g.x >= d.x || g.y >= d.y) { return; }
  let sd = textureDimensions(src_ms) - 1u;
  let ns = textureNumSamples(src_ms);
  var m = 1.0;
  for (var k = 0u; k < 4u; k++) {
    let p = min(g.xy * 2u + vec2u(k & 1u, k >> 1u), sd);
    for (var s = 0u; s < ns; s++) { m = min(m, textureLoad(src_ms, p, i32(s))); }
  }
  textureStore(dst, g.xy, vec4f(m, 0.0, 0.0, 0.0));
}

@compute @workgroup_size(8, 8)
fn hiz_one(@builtin(global_invocation_id) g: vec3u) {
  let d = textureDimensions(dst);
  if (g.x >= d.x || g.y >= d.y) { return; }
  let sd = textureDimensions(src_one) - 1u;
  var m = 1.0;
  for (var k = 0u; k < 4u; k++) {
    m = min(m, textureLoad(src_one, min(g.xy * 2u + vec2u(k & 1u, k >> 1u), sd), 0));
  }
  textureStore(dst, g.xy, vec4f(m, 0.0, 0.0, 0.0));
}

@compute @workgroup_size(8, 8)
fn hiz_reduce(@builtin(global_invocation_id) g: vec3u) {
  let d = textureDimensions(dst);
  if (g.x >= d.x || g.y >= d.y) { return; }
  // Mip sizes round down, so 3 × 3 texels: an odd last row or column is still covered.
  let sd = textureDimensions(src_level) - 1u;
  var m = 1.0;
  for (var k = 0u; k < 9u; k++) {
    m = min(m, textureLoad(src_level, min(g.xy * 2u + vec2u(k % 3u, k / 3u), sd), 0).x);
  }
  textureStore(dst, g.xy, vec4f(m, 0.0, 0.0, 0.0));
}
