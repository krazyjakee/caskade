// Time candidate hot paths inside a Blitsen export (QuickJS) and in Node (V8).
// usage: node microbench.mjs <blitsen export>
const N = 8429;
const benches = {
  "plain sort 8,429 strings": `(() => { const a = Array.from({length: ${N}}, (_, i) => "Game " + ((i * 7919) % ${N})); const t = performance.now(); a.sort(); return performance.now() - t; })()`,
  "localeCompare sort 8,429": `(() => { const a = Array.from({length: ${N}}, (_, i) => "Game " + ((i * 7919) % ${N})); const t = performance.now(); a.sort((x, y) => x.localeCompare(y)); return performance.now() - t; })()`,
  "Intl.Collator sort 8,429": `(() => { const c = new Intl.Collator("en"); const a = Array.from({length: ${N}}, (_, i) => "Game " + ((i * 7919) % ${N})); const t = performance.now(); a.sort(c.compare); return performance.now() - t; })()`,
  "NumberFormat x 8,429": `(() => { const f = new Intl.NumberFormat("en"); const t = performance.now(); for (let i = 0; i < ${N}; i++) f.format(i * 1.5); return performance.now() - t; })()`,
  "toLowerCase+includes x 8,429": `(() => { const a = Array.from({length: ${N}}, (_, i) => "Some Game Title " + i); const t = performance.now(); a.filter((s) => s.toLowerCase().includes("title 1")); return performance.now() - t; })()`,
  "object map/filter x 8,429": `(() => { const a = Array.from({length: ${N}}, (_, i) => ({ id: String(i), title: "G" + i, installed: i % 5 !== 0, lastPlayed: i * 1000 })); const t = performance.now(); a.filter((g) => g.installed).map((g) => ({ ...g, k: g.title + g.id })).sort((x, y) => y.lastPlayed - x.lastPlayed).slice(0, 100); return performance.now() - t; })()`,
  "JSON round-trip 8,429 rows": `(() => { const a = Array.from({length: ${N}}, (_, i) => ({ id: String(i), title: "Game " + i, platform: "Nintendo 64", tags: ["a", "b"] })); const t = performance.now(); JSON.parse(JSON.stringify(a)); return performance.now() - t; })()`,
};

const node = {};
for (const [k, code] of Object.entries(benches)) node[k] = eval(code);

const { launch } = await import("/mnt/Rusty/caskade-blitz-spike/blitsen-npm/node_modules/blitsen/src/test.mjs");
const app = await launch(process.argv[2], { timeout: 300000, width: 400, height: 300 });
try {
  for (const [k, code] of Object.entries(benches)) {
    const ms = await app.evaluate(code);
    console.log(`${k.padEnd(32)} blitsen ${ms.toFixed(1).padStart(9)} ms   node ${node[k].toFixed(1).padStart(7)} ms   x${(ms / Math.max(node[k], 0.05)).toFixed(0)}`);
  }
} finally {
  await app.close();
}
