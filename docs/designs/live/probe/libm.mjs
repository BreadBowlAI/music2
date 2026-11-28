// Song code's Math in the app is the libm crate (PLAT-4); is a JavaScript engine's own Math the
// same? `node libm.mjs <out dir>` after `wasmtime.rs <out dir> --libm`: Node's V8 (chrome.mjs checks
// the page's).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { check } from "./math.mjs";

if (process.argv[2]) {
  const table = JSON.parse(readFileSync(join(process.argv[2], "libm.json"), "utf8"));
  console.log(`node ${process.version} (V8 ${process.versions.v8}): Math against libm`);
  console.log("| function | inputs | differ | most ulps apart |\n|---|---:|---:|---:|");
  for (const r of check(table, Math)) console.log(`| ${r.name} | ${r.rows} | ${r.differ} | ${r.most_ulps} |`);
}
