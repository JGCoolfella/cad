// G2 compaction (`cull.rs`): the culling codes into per-slot lists, in scene order.

struct Slot {
  info: vec4u,    // first position in order, instances, list base, level
  linear: vec4f,  // chordal deviation of the level (mm)
};

// One workgroup per draw slot.

struct CompactU {
  kinds: vec4u,   // first kind, end kind, slots, list entries per kind
};

@group(0) @binding(0) var<uniform> ku: CompactU;
@group(0) @binding(1) var<storage, read> c_slots: array<Slot>;
@group(0) @binding(2) var<storage, read> c_order: array<u32>;
@group(0) @binding(3) var<storage, read> c_codes: array<u32>;
@group(0) @binding(4) var<storage, read_write> list: array<u32>;
@group(0) @binding(5) var<storage, read_write> face_args: array<u32>;
@group(0) @binding(6) var<storage, read_write> edge_args: array<u32>;

const MAXK: u32 = 5u;
var<workgroup> counts: array<u32, 320>;   // 64 invocations × MAXK kinds

fn face_kind(k: u32) -> bool {
  return k == 0u || k == 1u || k == 2u || k == 5u || k == 6u;
}

@compute @workgroup_size(64)
fn compact(@builtin(workgroup_id) wg: vec3u, @builtin(local_invocation_index) t: u32) {
  let s = wg.x + wg.y * 65535u;
  if (s >= ku.kinds.z) { return; }
  let sl = c_slots[s].info;
  let start = sl.x;
  let n = sl.y;
  let base = sl.z;
  let level = sl.w;
  let k0 = ku.kinds.x;
  let nk = ku.kinds.y - k0;
  let per = (n + 63u) / 64u;
  let a = min(t * per, n);
  let b = min(a + per, n);
  var c: array<u32, 5>;
  for (var r = a; r < b; r++) {
    let code = c_codes[start + r];
    if ((code >> 16u) == level) {
      for (var q = 0u; q < nk; q++) {
        if ((code & (1u << (k0 + q))) != 0u) { c[q] += 1u; }
      }
    }
  }
  for (var q = 0u; q < MAXK; q++) { counts[t * MAXK + q] = c[q]; }
  workgroupBarrier();
  if (t == 0u) {
    for (var q = 0u; q < nk; q++) {
      var run = 0u;
      for (var u = 0u; u < 64u; u++) {
        let v = counts[u * MAXK + q];
        counts[u * MAXK + q] = run;
        run += v;
      }
      let k = k0 + q;
      let d = k * ku.kinds.z + s;
      if (face_kind(k)) { face_args[d * 5u + 1u] = run; } else { edge_args[d * 4u + 1u] = run; }
    }
  }
  workgroupBarrier();
  for (var q = 0u; q < MAXK; q++) { c[q] = counts[t * MAXK + q]; }
  for (var r = a; r < b; r++) {
    let code = c_codes[start + r];
    if ((code >> 16u) == level) {
      for (var q = 0u; q < nk; q++) {
        if ((code & (1u << (k0 + q))) != 0u) {
          list[(k0 + q) * ku.kinds.w + base + c[q]] = c_order[start + r];
          c[q] += 1u;
        }
      }
    }
  }
}

