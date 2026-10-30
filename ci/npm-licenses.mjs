#!/usr/bin/env node
// ci/npm-licenses.mjs: the npm side of what deny.toml checks for Rust. Reads the two lockfiles
// (web/package-lock.json, e2e/package-lock.json), so it needs no `npm ci` and no network.
//
//   node ci/npm-licenses.mjs [lockfile ...]
//
// In short: every package in them must come from registry.npmjs.org with an integrity hash, and
// have a licence from ALLOW below (for "A OR B", one of them; for "A AND B", both). A package that
// comes bundled inside another (npm's "inBundle") has neither of its own: the package it ships in
// must, and must say it bundles. Our own package.json files must say MIT OR Apache-2.0. Prints one
// line per lockfile; exit 1 on any problem, naming each. Lockfiles named on the command line (each
// with its package.json beside it) are read instead of the two; ci/npm-licenses-test.sh does that.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const LOCKS = process.argv.length > 2 ? process.argv.slice(2) : ["web/package-lock.json", "e2e/package-lock.json"];
// deny.toml's list, plus 0BSD (tslib). Add one only after reading the licence.
const ALLOW = new Set([
  "MIT",
  "Apache-2.0",
  "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Zlib",
  "Unicode-3.0",
  "BSL-1.0",
  "MPL-2.0",
  "0BSD",
]);
const OURS = "MIT OR Apache-2.0";
const REGISTRY = "https://registry.npmjs.org/";

// allowed(expr): whether an SPDX expression is satisfied by ALLOW. OR needs one side, AND both;
// "X WITH Y" counts as one licence; brackets group.
function allowed(expr) {
  const tokens = expr.replace(/\(/g, " ( ").replace(/\)/g, " ) ").trim().split(/\s+/);
  let i = 0;
  const atom = () => {
    if (tokens[i] === "(") {
      i++;
      const v = or();
      if (tokens[i++] !== ")") throw new Error("a bracket isn't closed");
      return v;
    }
    let id = tokens[i++];
    if (!id || ["AND", "OR", "WITH", ")"].includes(id)) throw new Error(`a licence is missing before "${id ?? "the end"}"`);
    if (tokens[i] === "WITH") {
      id = `${id} WITH ${tokens[i + 1]}`;
      i += 2;
    }
    return ALLOW.has(id);
  };
  const and = () => {
    let v = atom();
    while (tokens[i] === "AND") {
      i++;
      v = atom() && v;
    }
    return v;
  };
  const or = () => {
    let v = and();
    while (tokens[i] === "OR") {
      i++;
      v = and() || v;
    }
    return v;
  };
  const v = or();
  if (i !== tokens.length) throw new Error(`"${tokens[i]}" is out of place`);
  return v;
}

// bundledBy(packages, where): why the package at `where`, marked inBundle, isn't covered by the
// package it ships in, or "" when it is. That package is the nearest folder above it that isn't
// bundled itself; it must be in the lockfile, from the registry, with an integrity hash (which
// then covers the bundled files too), and must say it bundles.
function bundledBy(packages, where) {
  let at = where;
  for (;;) {
    const cut = at.lastIndexOf("/node_modules/");
    if (cut < 0) return "no package above it ships it";
    at = at.slice(0, cut);
    const up = packages[at];
    if (!up) return `${at} isn't in the lockfile`;
    if (up.inBundle) continue;
    if (!up.resolved?.startsWith(REGISTRY) || !up.integrity) return `${at}, which ships it, has no registry address and integrity hash`;
    const says = up.bundleDependencies ?? up.bundledDependencies;
    if (says !== true && !(Array.isArray(says) && says.length)) return `${at}, which ships it, bundles nothing`;
    return "";
  }
}

const problems = [];
for (const lock of LOCKS) {
  const file = path.resolve(ROOT, lock);
  const data = JSON.parse(fs.readFileSync(file, "utf8"));
  const own = JSON.parse(fs.readFileSync(path.join(path.dirname(file), "package.json"), "utf8"));
  if (own.license !== OURS) problems.push(`${path.dirname(lock)}/package.json: license is ${JSON.stringify(own.license)}, not "${OURS}"`);
  let n = 0;
  const seen = new Map();
  const packages = data.packages ?? {};
  for (const [where, p] of Object.entries(packages)) {
    if (where === "") continue;
    n++;
    const name = `${lock}: ${where.replace(/^.*node_modules\//, "")}@${p.version}`;
    if (p.link) {
      problems.push(`${name} is a link to ${p.resolved}, not a package from the registry`);
      continue;
    }
    if (p.inBundle) {
      const why = bundledBy(packages, where);
      if (why) problems.push(`${name} comes bundled, but ${why}`);
    } else {
      if (!p.resolved?.startsWith(REGISTRY)) problems.push(`${name} comes from ${p.resolved ?? "nowhere recorded"}, not ${REGISTRY}`);
      if (!p.integrity) problems.push(`${name} has no integrity hash`);
    }
    const lic = p.license;
    if (typeof lic !== "string" || !lic.trim()) {
      problems.push(`${name} names no licence`);
      continue;
    }
    let ok;
    try {
      ok = allowed(lic);
    } catch (e) {
      problems.push(`${name}: its licence "${lic}" doesn't read as SPDX (${e.message})`);
      continue;
    }
    if (!ok) problems.push(`${name}: its licence "${lic}" isn't on the allowed list (ci/npm-licenses.mjs)`);
    seen.set(lic, (seen.get(lic) ?? 0) + 1);
  }
  const kinds = [...seen].sort((a, b) => b[1] - a[1]).map(([l, c]) => `${l} ${c}`).join(", ");
  console.log(`${lock}: ${n} packages, from ${REGISTRY} (${kinds})`);
}
for (const p of problems) console.log(`FAIL  ${p}`);
console.log(problems.length ? `npm licences: ${problems.length} problems` : "npm licences: ok");
process.exit(problems.length ? 1 : 0);
