// ci/own-main.mjs: ci/pr.sh rust-build's copy of the tests rust's part 1 runs with cargo test.
//
//   node ci/own-main.mjs <cargo's JSON messages> <checkout> <folder> <test names...>
//
// In short: reads what `cargo test --no-run --message-format json-render-diagnostics` printed,
// copies each named test's program into <folder> under its name alone (no hash), and writes
// <folder>/list.tsv: each test's name, a tab, and its package's folder from the top of <checkout>
// (cargo test runs a test from there, and ci/pr.sh rust --archive does the same). Exits 1, saying
// which, when a named test isn't in the messages, or is there twice.

import { copyFileSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";

const [messages, checkout, folder, ...names] = process.argv.slice(2);
if (!messages || !checkout || !folder || names.length === 0) {
  console.error("usage: node ci/own-main.mjs <cargo's JSON messages> <checkout> <folder> <test names...>");
  process.exit(2);
}
const found = new Map(names.map((n) => [n, []]));
for (const line of readFileSync(messages, "utf8").split("\n")) {
  if (!line.startsWith("{")) continue;
  const m = JSON.parse(line);
  if (m.reason !== "compiler-artifact" || !m.executable || !m.target?.kind?.includes("test")) continue;
  found.get(m.target.name)?.push({ exe: m.executable, dir: relative(checkout, dirname(m.manifest_path)) });
}
let bad = false;
const rows = [];
for (const [name, seen] of found) {
  if (seen.length !== 1) {
    console.log(`${name}: ${seen.length} test programs of that name in cargo's messages, not one`);
    bad = true;
    continue;
  }
  copyFileSync(seen[0].exe, join(folder, name));
  rows.push(`${name}\t${seen[0].dir}\n`);
  console.log(`${name}: ${seen[0].exe}, from ${seen[0].dir}`);
}
writeFileSync(join(folder, "list.tsv"), rows.join(""));
process.exit(bad ? 1 : 0);
