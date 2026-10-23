#!/usr/bin/env node
// ci/ci-ok.mjs: the verdict of the "ci-ok" job, the one check a pull request must pass (branch
// protection and the merge queue require it alone, so adding or renaming a job never needs a
// settings change).
//
//   NEEDS='<the needs context as JSON>' node ci/ci-ok.mjs
//
// In short: it reads what every job before it did (GitHub's `needs` context, given by the
// workflow as toJSON(needs)) and passes only when the "changes" job succeeded and then either
//   - the change is documentation only (changes said docs_only=true), and every other job was
//     skipped by that rule or succeeded; or
//   - it isn't, and every other job succeeded.
// A failed or cancelled job fails it; so does a job skipped for any other reason. Prints one line
// per job, then the verdict. Exit 0 or 1.
//
//   node ci/ci-ok.mjs --check-workflow
//
// checks instead that ci-ok's `needs:` in .github/workflows/ci.yml names every other job of it,
// so a new job can't be left out of the verdict (ci/pr.sh quick runs this).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

if (process.argv[2] === "--check-workflow") {
  const file = path.join(path.dirname(fileURLToPath(import.meta.url)), "../.github/workflows/ci.yml");
  const lines = fs.readFileSync(file, "utf8").split("\n");
  const start = lines.findIndex((l) => /^jobs:\s*$/.test(l));
  const jobs = [];
  let needs = null;
  let current = null;
  for (const l of lines.slice(start + 1)) {
    const job = /^  ([A-Za-z0-9_-]+):\s*$/.exec(l);
    if (job) current = job[1], jobs.push(current);
    const n = /^    needs:\s*\[(.*)\]\s*$/.exec(l);
    if (n && current === "ci-ok") needs = n[1].split(",").map((x) => x.trim()).filter(Boolean);
  }
  const want = jobs.filter((j) => j !== "ci-ok");
  if (start < 0 || !jobs.includes("ci-ok") || !needs) {
    console.log("FAIL  .github/workflows/ci.yml has no ci-ok job with needs: [...] on one line");
    process.exit(1);
  }
  const missing = want.filter((j) => !needs.includes(j));
  const extra = needs.filter((j) => !want.includes(j));
  for (const j of missing) console.log(`FAIL  ci-ok doesn't need ${j}: its result would be left out`);
  for (const j of extra) console.log(`FAIL  ci-ok needs ${j}, which isn't a job`);
  console.log(missing.length || extra.length ? "ci-ok's needs: FAIL" : `ci-ok's needs: ok (${want.join(", ")})`);
  process.exit(missing.length || extra.length ? 1 : 0);
}

let needs = null;
try {
  needs = JSON.parse(process.env.NEEDS ?? "null");
} catch {}
if (!needs || typeof needs !== "object" || !needs.changes) {
  console.log("FAIL  no needs context, or no changes job in it: ci-ok can't judge");
  process.exit(1);
}
const docsOnly = needs.changes.result === "success" && needs.changes.outputs?.docs_only === "true";
let ok = true;
for (const [job, { result }] of Object.entries(needs)) {
  let fine;
  let why;
  if (job === "changes") {
    fine = result === "success";
    why = fine ? (docsOnly ? "the change is documentation only" : "the change needs the full tier") : "the change couldn't be judged";
  } else if (result === "success") {
    fine = true;
    why = "passed";
  } else if (result === "skipped" && docsOnly) {
    fine = true;
    why = "skipped: documentation only (ci/docs-only.sh)";
  } else {
    fine = false;
    why = result === "skipped" ? "skipped, but the change isn't documentation only" : result;
  }
  ok &&= fine;
  console.log(`${fine ? "ok  " : "FAIL"}  ${job}: ${why}`);
}
console.log(ok ? "ci-ok: PASS" : "ci-ok: FAIL");
process.exit(ok ? 0 : 1);
