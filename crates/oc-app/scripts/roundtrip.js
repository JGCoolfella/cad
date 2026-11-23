// Usage: opencad --script=crates/oc-app/scripts/roundtrip.js --smoke=native-roundtrip.png (from the checkout).
// W1b measurements in the desktop app: the same requests as src/web/check.ts, over Tauri IPC.
const inv = window.__TAURI_INTERNALS__.invoke;
const med = (x) => { const s = x.slice().sort((p, q) => p - q); return +s[s.length >> 1].toFixed(3); };
const empty = [];
for (let i = 0; i < 200; i++) { const t = performance.now(); await inv('engine_mass_props', { features: [] }); empty.push(performance.now() - t); }
const names = ['block-hole','curved-profiles','draft','fillet-chamfer','holes','imported','pad-pocket','patterns','refgeom','revolve','rib','shell-outward','shell','sweep-loft','thin-contours','through-mid-dir2','up-to'];
await inv('engine_clear_cache');
let total = 0; const per = {};
for (const n of names) {
  const doc = JSON.parse(await inv('app_read_text', { path: cwd + '/corpus/' + n + '.ocpart' }));
  const t = performance.now(); await a.kernel.rebuild(doc.features); const ms = performance.now() - t; total += ms; per[n] = +ms.toFixed(1);
}
const block = JSON.parse(await inv('app_read_text', { path: cwd + '/corpus/block-hole.ocpart' })).features;
const cached = [];
for (let i = 0; i < 100; i++) { const t = performance.now(); await inv('engine_rebuild', { features: block, preview: null, quality: 3, config: null }); cached.push(performance.now() - t); }
const all = { format: 'opencad-assembly', version: 2, name: 'all', units: 'MMGS',
  components: names.map((n, i) => ({ id: 'c' + i, kind: 'part', path: n + '.ocpart', config: 'Default' })),
  instances: names.map((n, i) => ({ id: 'i' + i, component: 'c' + i, name: n + '<1>', fixed: true, transform: { position: [i * 150, 0, 0], rotation: [1, 0, 0, 0] } })), mates: [] };
const times = []; let head;
for (let i = 0; i < 4; i++) {
  if (i) await inv('engine_clear_cache');
  const t = performance.now();
  const b = await inv('asm_build', { doc: all, path: cwd + '/corpus/all.ocasm', lightweight: false, quality: 3, overrides: {}, probe: false, base: null });
  times.push(+(performance.now() - t).toFixed(1));
  const n = new DataView(b).getUint32(4, true);
  head = JSON.parse(new TextDecoder().decode(new Uint8Array(b, 8, n)));
}
return { empty: med(empty), cachedRebuild: med(cached), corpusMs: +total.toFixed(1), per, all: { times, regenerated: head.regenerated, workers: head.workers, hosts: head.hosts } };
