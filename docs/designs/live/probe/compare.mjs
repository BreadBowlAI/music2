// The web engine's probe's table: `node compare.mjs <out dir>`. Each host's output hash against
// wasmtime's, kernel by kernel, and the time per 128-frame block.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2];
const hosts = ["wasmtime", "wasmtime-flush", "node", "chrome-main", "chrome-worker", "chrome-worklet", "chrome-realtime"].filter((h) => existsSync(join(dir, `${h}.json`)));
const runs = Object.fromEntries(hosts.map((h) => [h, JSON.parse(readFileSync(join(dir, `${h}.json`), "utf8"))]));
const ref = new Map(runs.wasmtime.results.map((r) => [r.id, r]));
console.log("| host | same output bits as wasmtime | same inputs | flushes subnormals | µs per block, all kernels summed |");
console.log("|---|---|---|---|---|");
for (const h of hosts) {
  const run = runs[h];
  const rs = run.results;
  const same = rs.filter((r) => r.out && r.out === ref.get(r.id)?.out).length;
  const ins = rs.filter((r) => r.in && r.in === ref.get(r.id)?.in).length;
  const us = rs.reduce((a, r) => a + (r.us_per_block ?? 0), 0);
  const fl = run.flushes ?? rs.find((r) => r.flushes !== undefined)?.flushes;
  console.log(`| ${run.host} | ${same} of ${rs.length} | ${ins} of ${rs.length} | ${fl ?? "-"} | ${us.toFixed(1)} |`);
  const differ = rs.filter((r) => r.out !== ref.get(r.id)?.out).map((r) => r.id + (r.error ? ` (${r.error})` : ""));
  if (differ.length) console.log(`|  differ: ${differ.join(", ")} | | | | |`);
}
console.log("\n| kernel | " + hosts.join(" | ") + " |");
console.log("|---|" + hosts.map(() => "---:").join("|") + "|");
for (const r of runs.wasmtime.results) {
  const cells = hosts.map((h) => {
    const x = runs[h].results.find((y) => y.id === r.id);
    return x ? `${x.us_per_block ?? "-"}${x.out === r.out ? "" : " ≠"}` : "-";
  });
  console.log(`| ${r.id} | ${cells.join(" | ")} |`);
}
for (const h of ["chrome-worklet", "chrome-realtime"]) if (runs[h]) console.log(`\n${h}: scope ${JSON.stringify(runs[h].scope)}, state ${runs[h].state}`);
