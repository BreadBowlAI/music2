#!/usr/bin/env node
// ci/notices.mjs: THIRD-PARTY-NOTICES.txt, the licences of the code inside music2's downloads that
// others wrote (PLAT-8, the release review's L3). apps/music2-desktop/bundle-m2.sh runs it as
// `tauri build` bundles, so every bundle carries the notices of exactly what it was built from.
//
//   node ci/notices.mjs <rust target> <out file>
//
// In short: the Rust crates the app and m2 are built from (cargo metadata of this workspace, for
// that target: the crates music2-desktop and m2 reach by normal dependencies, not build or dev
// ones), the crates the kernel modules are built from (std/kernels-src's workspace: std.wasm and music2's
// own packs', for wasm32), the npm packages of the
// UI that aren't dev ones (web/package-lock.json, read from web/node_modules, which the UI's
// build installs), and the Inter font (web/public/fonts/OFL.txt). For each: its name, version
// and licence, then every licence file it carries (LICENSE*, COPYING*, NOTICE*, ...), each text
// printed once and named by every package that has it. A package that carries none gets the
// standard text of each licence it names, from ci/licenses/ (SPDX's license-list-data, v3.27.0),
// headed by its authors. A licence with no text there is an error, so nothing is left out.
// Needs: cargo (it may fetch crates' manifests) and Node.

import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [target, out] = process.argv.slice(2);
if (!target || !out) {
  console.error("usage: node ci/notices.mjs <rust target> <out file>");
  process.exit(2);
}

const LICENCE_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense|authors)/i;
const texts = new Map(); // hash → { text, users: [] }
const listed = []; // { section, line }
const missing = [];

function addText(text, user) {
  const t = text.replace(/\r\n/g, "\n").trim();
  const h = crypto.createHash("sha256").update(t).digest("hex");
  if (!texts.has(h)) texts.set(h, { text: t, users: [] });
  texts.get(h).users.push(user);
}

// The SPDX ids in an expression ("MIT OR Apache-2.0", "Apache-2.0 WITH LLVM-exception").
function ids(expr) {
  return (expr || "").replace(/[()]/g, " ").split(/\s+/).filter((w) => w && !["OR", "AND", "WITH"].includes(w)).map((w) => w.replace(/\/.*/, ""));
}

function standard(expr, user, authors) {
  for (const id of ids(expr)) {
    const file = path.join(ROOT, "ci/licenses", `${id}.txt`);
    if (!fs.existsSync(file)) {
      missing.push(`${user}: no file of its own, and ci/licenses has no ${id}.txt`);
      continue;
    }
    let text = fs.readFileSync(file, "utf8");
    if (authors) text = `${id}, as ${user}'s manifest names it; copyright its authors: ${authors}\n\n${text}`;
    addText(text, user);
  }
}

function files(dir, user, expr, authors, own) {
  const found = fs.readdirSync(dir).filter((f) => LICENCE_FILE.test(f) && fs.statSync(path.join(dir, f)).isFile());
  if (own) found.push(path.relative(dir, path.resolve(dir, own)));
  for (const f of [...new Set(found)]) addText(fs.readFileSync(path.join(dir, f), "utf8"), user);
  if (!found.length) standard(expr, user, authors);
}

function crates(manifestDir, triple, roots, section) {
  const meta = JSON.parse(
    execFileSync("cargo", ["metadata", "--locked", "--format-version", "1", "--filter-platform", triple], {
      cwd: manifestDir,
      maxBuffer: 1 << 30,
      encoding: "utf8",
    }),
  );
  const ours = new Set(meta.workspace_members);
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const start = meta.packages.filter((p) => ours.has(p.id) && (roots === null || roots.includes(p.name))).map((p) => p.id);
  const seen = new Set();
  const todo = [...start];
  while (todo.length) {
    const id = todo.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const d of nodes.get(id).deps) if (d.dep_kinds.some((k) => k.kind === null)) todo.push(d.pkg);
  }
  const pkgs = [...seen].filter((id) => !ours.has(id)).map((id) => byId.get(id));
  pkgs.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
  for (const p of pkgs) {
    const user = `${p.name} ${p.version}`;
    listed.push({ section, line: `${user}  ${p.license || `see ${p.license_file}`}${p.repository ? `  ${p.repository}` : ""}` });
    files(path.dirname(p.manifest_path), user, p.license, (p.authors || []).join(", "), p.license_file);
  }
  return pkgs.length;
}

const rust = crates(ROOT, target, ["music2-desktop", "m2"], "Rust crates in the app and m2");
const kernels = crates(path.join(ROOT, "std/kernels-src"), "wasm32-unknown-unknown", null, "Rust crates in the kernels' wasm");

const lock = JSON.parse(fs.readFileSync(path.join(ROOT, "web/package-lock.json"), "utf8"));
let npm = 0;
for (const [where, p] of Object.entries(lock.packages)) {
  if (!where || p.dev) continue;
  const dir = path.join(ROOT, "web", where);
  if (!fs.existsSync(dir)) {
    console.error(`notices: web/${where} isn't installed: run npm ci in web first`);
    process.exit(1);
  }
  const name = where.replace(/.*node_modules\//, "");
  const user = `${name} ${p.version}`;
  listed.push({ section: "npm packages in the UI", line: `${user}  ${p.license}` });
  files(dir, user, p.license, "", null);
  npm++;
}

const rustc = execFileSync("rustc", ["-V"], { cwd: ROOT, encoding: "utf8" }).trim();
listed.push({ section: "Rust's standard library, in every program", line: `${rustc}  MIT OR Apache-2.0  https://github.com/rust-lang/rust` });
standard("MIT OR Apache-2.0", "Rust's standard library", "The Rust Project Developers");

listed.push({ section: "Fonts in the UI", line: "Inter 4.1 (rsms/inter, web/public/fonts/InterVariable.woff2)  OFL-1.1" });
addText(fs.readFileSync(path.join(ROOT, "web/public/fonts/OFL.txt"), "utf8"), "Inter 4.1");

if (missing.length) {
  console.error(`notices: ${missing.length} licence texts missing:\n${missing.join("\n")}`);
  process.exit(1);
}

let doc = `music2: notices of the code inside it that others wrote

music2 itself is under the MIT licence or the Apache License 2.0, at your choice (LICENSE-MIT,
LICENSE-APACHE beside this file). It is built from the packages below, each under its own
licence, whose texts follow. Made by ci/notices.mjs for ${target}: ${rust} crates in the app and
m2, ${kernels} in the kernels' wasm, ${npm} npm packages in the UI, Rust's standard library, and one
font.
`;
let section = "";
for (const l of listed) {
  if (l.section !== section) {
    section = l.section;
    doc += `\n== ${section} ==\n\n`;
  }
  doc += `${l.line}\n`;
}
doc += "\n== Licence texts ==\n";
for (const { text, users } of texts.values()) {
  doc += `\n---- Used by: ${users.join(", ")}\n\n${text}\n`;
}
fs.mkdirSync(path.dirname(path.resolve(out)), { recursive: true });
fs.writeFileSync(out, doc);
console.log(`notices: ${out}: ${rust} + ${kernels} crates, ${npm} npm packages, a font, ${texts.size} licence texts`);
