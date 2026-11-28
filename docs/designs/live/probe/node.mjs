// The web engine's probe under Node's WebAssembly (V8): `node node.mjs <out dir>`, after
// wasmtime.rs wrote <out dir>/cases.json. Writes <out dir>/node.json.
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { flushes, runCase } from "./host.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../../../..");
const dir = process.argv[2];
const { blocks, cases } = JSON.parse(readFileSync(join(dir, "cases.json"), "utf8"));
const modules = new Map();
const sha = (b) => createHash("sha256").update(b).digest("hex");
const results = [];
for (const c of cases) {
  if (!modules.has(c.wasm)) modules.set(c.wasm, new WebAssembly.Module(readFileSync(join(root, c.wasm))));
  const id = `${c.package}:${c.kernel}`;
  try {
    const r = runCase(modules.get(c.wasm), c, blocks, () => performance.now());
    results.push({ id, out: sha(r.out), in: sha(r.ins), bytes: r.out.length, nonfinite: r.nonfinite, sounding: r.sounding, us_per_block: r.us });
  } catch (e) {
    results.push({ id, error: String(e) });
  }
}
const host = `node ${process.version} (V8 ${process.versions.v8}), ${process.arch}`;
writeFileSync(join(dir, "node.json"), JSON.stringify({ host, flushes: flushes(2 ** -1022), results }, null, 2));
console.log(`${results.length} kernels: ${join(dir, "node.json")}`);
