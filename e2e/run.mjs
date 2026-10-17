#!/usr/bin/env node
// Runs the browser suites against the headless music2 server, each on its own fresh server.
//
//   M2_HEADLESS=<headless binary> node e2e/run.mjs <suite|all> [kind] [options]
//
//   suite    one of the names in SUITES below (or "all": every suite, each on a fresh server)
//   kind     the fixture the suite gets (default: the suite's own, see SUITES):
//              proj    examples/first-song open        node suite <root>
//              cables  a one-file song (fixtures/)     node suite <root>
//              fix     examples/first-song open        node suite <root> <home> <project>
//              attn    same as fix                     node suite <root> <home> <project>
//              empty   no project open                 node suite <root> empty
//              none    no project open                 node suite <root> <home>
//              export  examples/first-song open        node suite <root> <home> <project> <m2 cli>
//   --repo <dir>   the checkout examples/first-song is copied from (the <root>/proj of every kind),
//                  passed on as E2E_REPO to the suites that copy an example themselves (live3b:
//                  examples/with-plugins; fx-fixes: examples/first-song). Default: the repo this
//                  e2e/ folder lives in. It does not change what the server serves: web/dist, std,
//                  templates/ and the projects behind "Open an Example" come from the source tree
//                  the headless binary was compiled in (crates/server's CARGO_MANIFEST_DIR), unless
//                  M2_STD / M2_TEMPLATES / M2_EXAMPLES are set. So use the checkout the binaries
//                  were built from; a note says so when the binary wasn't built in --repo.
//   --out <dir>    where each run's scratch folder and log go (default: a new temp folder)
//   --keep         keep every scratch folder (logs are always kept, and so is the folder of a run
//                  that wasn't ok)
//   --quiet        don't stream a single suite's output (all mode never streams)
//
// Environment:
//   M2_HEADLESS  the server: `cargo build -p m2-server --example headless` → target/<profile>/examples/headless
//   M2_CLI       the m2 command line, for the export suite (default: <M2_HEADLESS dir>/../m2)
//   E2E_CHROME   chrome-headless-shell (default: puppeteer's cache, version 133.0.6943.141, for this OS;
//                off the Mac the suites run it through chrome-as-mac.sh: chrome.mjs)
//   CLAP_PATH    where hosted CLAP plugins are found. Only suites marked `clap` get it (live3b, whose
//                plugin part needs Surge XT and Airwindows Consolidated there), and only when you set
//                it: there is no default, so your own plugin folder is never used unasked
//                (CLAP_PATH=~/Library/Audio/Plug-Ins/CLAP uses it). Without it live3b skips that
//                part, and the run is PART, not ok. Every other suite runs WITHOUT CLAP_PATH,
//                fx-fixes in particular.
//   E2E_TIMEOUT  seconds a suite may take (default 600)
//
// Every run gets a fresh scratch HOME (<root>/testhome) for both the server and the suite, so
// nothing reads or writes the real ~/.music2 or ~/Music.
//
// A run is ok only when the suite exited 0, printed no FAIL and no SKIP line, and passed exactly
// the number of checks SUITES expects of it. One that passed but printed a SKIP, or passed another
// number of checks, is PART, with the reason: part of it didn't run, or the suite changed and its
// `expect` here (and the README's table) must follow. Exit status: 0 when every run was ok, 1 when
// any failed, 3 when none failed but some were PART, 2 for a usage error.

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { real as chrome } from "./chrome.mjs";

const E2E = path.dirname(fileURLToPath(import.meta.url));

// [suite, default kind, options]. Order is the order of `all`. `expect`: the checks the suite
// passes on that kind at 933f4de (the README's table). `clap`: the suite gets CLAP_PATH. live3b's
// 51 include the 11 of its plugin part: without CLAP_PATH it passes 40 and prints a SKIP.
export const SUITES = [
  ["graph", "proj", { expect: 19 }],
  ["cables", "cables", { expect: 12 }],
  ["undo", "cables", { expect: 10 }],
  ["effects", "proj", { expect: 12 }],
  ["layout", "proj", { expect: 39 }],
  ["views", "proj", { expect: 30 }],
  ["code", "proj", { expect: 58 }],
  ["notes", "proj", { expect: 52 }],
  ["s8-audition", "fix", { expect: 51 }],
  ["ports", "proj", { expect: 46 }],
  ["s8-meters", "fix", { expect: 24 }],
  ["dsp", "proj", { expect: 33 }],
  ["start", "proj", { expect: 24 }],
  ["start", "empty", { expect: 14 }],
  ["fixes-graph", "fix", { expect: 42 }],
  ["fixes-notes", "fix", { expect: 37 }],
  ["attention", "attn", { expect: 57 }],
  ["newsong", "none", { expect: 71 }],
  ["export", "export", { expect: 37 }],
  ["fx-fixes", "none", { expect: 75 }],
  ["s4-clips", "fix", { expect: 28 }],
  ["s8-clips", "fix", { expect: 94 }],
  ["s8-automation", "fix", { expect: 45 }],
  ["s4-attn", "fix", { expect: 13 }],
  ["s4-problems", "fix", { expect: 18 }],
  ["s4-meter", "fix", { expect: 8 }],
  ["live3a", "fix", { expect: 49 }],
  ["live3b", "proj", { clap: true, expect: 60 }],
  ["s5-knobs", "fix", { expect: 120 }],
  ["s6-builders", "fix", { expect: 56 }],
  ["s6-write", "fix", { expect: 36 }],
  ["s6-fixes", "fix", { expect: 19 }],
  ["taste", "fix", { expect: 39 }],
  ["gestures", "fix", { expect: 30 }],
  ["g11-play", "fix", { expect: 24 }],
  ["s8-tracks", "fix", { expect: 75 }],
  ["pl-tracks", "fix", { expect: 20 }],
  ["s8-mute", "export", { expect: 152 }],
  ["s8-songbar", "fix", { expect: 82 }],
  ["s8-mix", "fix", { expect: 80 }],
  ["pc-keys", "fix", { expect: 37 }],
];
const KINDS = ["proj", "cables", "fix", "attn", "empty", "none", "export"];

function usage(msg) {
  if (msg) console.error(`run.mjs: ${msg}`);
  console.error("usage: M2_HEADLESS=<bin> node e2e/run.mjs <suite|all> [kind] [--repo DIR] [--out DIR] [--keep] [--quiet]");
  console.error(`suites: ${[...new Set(SUITES.map(([s]) => s))].join(" ")}`);
  process.exit(2);
}

const argv = process.argv.slice(2);
const opts = { repo: path.resolve(E2E, ".."), out: null, keep: false, quiet: false };
const pos = [];
for (let i = 0; i < argv.length; i++) {
  const a = argv[i];
  if (a === "--repo") opts.repo = path.resolve(argv[++i] ?? usage("--repo needs a folder"));
  else if (a === "--out") opts.out = path.resolve(argv[++i] ?? usage("--out needs a folder"));
  else if (a === "--keep") opts.keep = true;
  else if (a === "--quiet") opts.quiet = true;
  else if (a === "-h" || a === "--help") usage();
  else if (a.startsWith("--")) usage(`unknown option ${a}`);
  else pos.push(a);
}
const [which, kindArg] = pos;
if (!which) usage();
if (kindArg && !KINDS.includes(kindArg)) usage(`unknown kind ${kindArg} (${KINDS.join(", ")})`);

const headless = process.env.M2_HEADLESS && path.resolve(process.env.M2_HEADLESS);
if (!headless || !fs.existsSync(headless)) usage("set M2_HEADLESS to the headless server binary");
const cli = path.resolve(process.env.M2_CLI ?? path.join(path.dirname(headless), "..", "m2"));
if (!fs.existsSync(chrome)) usage(`no chrome-headless-shell at ${chrome} (set E2E_CHROME)`);
// No default: a suite gets the plugins you name, never your own folder unasked.
const clapPath = process.env.CLAP_PATH ? path.resolve(process.env.CLAP_PATH) : null;
const timeout = Number(process.env.E2E_TIMEOUT ?? 600) * 1000;
if (!fs.existsSync(path.join(opts.repo, "examples/first-song"))) usage(`${opts.repo} has no examples/first-song`);
if (!fs.existsSync(path.join(E2E, "node_modules/puppeteer-core"))) usage(`run \`npm ci\` in ${E2E} first`);

let runs;
if (which === "all") {
  if (kindArg) usage("all takes no kind");
  runs = SUITES;
} else {
  const known = SUITES.filter(([s]) => s === which);
  if (!known.length) usage(`unknown suite ${which}`);
  if (!kindArg) runs = known.slice(0, 1);
  // A kind SUITES doesn't list for this suite: its options, but no count to expect.
  else runs = [known.find(([, k]) => k === kindArg) ?? [which, kindArg, { ...known[0][2], expect: undefined }]];
}

const root = opts.out ?? fs.mkdtempSync(path.join(os.tmpdir(), "m2-e2e-"));
fs.mkdirSync(root, { recursive: true });

// The server serves web/dist, std, templates/ and the examples behind "Open an Example" from the
// tree it was compiled in (concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/dist") and the like),
// not from --repo. True when that tree is --repo, false when it is another, null when the binary
// doesn't say.
function builtIn(bin, repo) {
  let buf;
  try {
    buf = fs.readFileSync(bin);
  } catch {
    return null;
  }
  const tail = "/crates/server/../../web/dist";
  if (buf.indexOf(tail) < 0) return null;
  const dirs = new Set([repo]);
  try {
    dirs.add(fs.realpathSync(repo));
  } catch {}
  return [...dirs].some((d) => buf.indexOf(`${d}${tail}`) >= 0);
}
if (builtIn(headless, opts.repo) === false) {
  console.log(
    `note: ${headless} wasn't built in ${opts.repo}: the server serves web/dist, std, templates/ and ` +
      `"Open an Example" from the checkout it was compiled in, whatever --repo says. Pass that checkout as --repo.`,
  );
}
if (runs.some(([, , o]) => o?.clap) && !clapPath) {
  console.log(
    "note: CLAP_PATH is not set, so live3b skips its plugin part (11 checks) and is reported PART. " +
      "Set it to a folder with Surge XT and Airwindows Consolidated (CLAP_PATH=~/Library/Audio/Plug-Ins/CLAP for yours).",
  );
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Builds the scratch folder for one run, as the suites expect it.
function prepare(dir, kind) {
  fs.rmSync(dir, { recursive: true, force: true });
  const home = path.join(dir, "testhome");
  fs.mkdirSync(path.join(home, ".music2"), { recursive: true });
  const proj = path.join(dir, "proj");
  fs.cpSync(path.join(opts.repo, "examples/first-song"), proj, { recursive: true });
  // The suites compare edits against the fixture's own files.
  fs.copyFileSync(path.join(proj, "song.ts"), path.join(dir, "orig-song.ts"));
  fs.copyFileSync(path.join(proj, "tracks/keys.ts"), path.join(dir, "keys-orig.ts"));
  let open = proj;
  if (kind === "cables") {
    const cables = path.join(dir, "cables");
    fs.cpSync(path.join(E2E, "fixtures/cables"), cables, { recursive: true });
    fs.copyFileSync(path.join(cables, "song.ts"), path.join(dir, "cables-orig.ts"));
    open = cables;
  } else if (kind === "empty" || kind === "none") {
    open = "-";
  }
  const args = {
    proj: [dir],
    cables: [dir],
    fix: [dir, home, proj],
    attn: [dir, home, proj],
    empty: [dir, "empty"],
    none: [dir, home],
    export: [dir, home, proj, cli],
  }[kind];
  return { home, open, args };
}

function envFor(home, clap) {
  const env = { ...process.env, HOME: home, M2_AUDIO: "none", E2E_CHROME: chrome, E2E_REPO: opts.repo, E2E_HEADLESS: headless };
  delete env.CLAP_PATH;
  if (clap && clapPath) env.CLAP_PATH = clapPath;
  return env;
}

async function runOne(suite, kind, o = {}, stream) {
  const name = `${suite}-${kind}`;
  const dir = path.join(root, name);
  const logFile = path.join(root, `${name}.log`);
  const { home, open, args } = prepare(dir, kind);
  const env = envFor(home, o.clap);
  const session = path.join(home, ".music2/session.json");
  const log = fs.openSync(logFile, "w");
  const say = (s) => {
    fs.writeSync(log, s);
    if (stream) process.stdout.write(s);
  };

  const serverLog = fs.openSync(path.join(dir, "headless.log"), "w");
  const server = spawn(headless, [open, session, `e2e${Date.now()}`], { env, stdio: ["ignore", serverLog, serverLog] });
  let serverExit = null;
  server.on("exit", (code, sig) => (serverExit = sig ?? code));
  const t0 = Date.now();
  while (!fs.existsSync(session) && serverExit === null && Date.now() - t0 < 30000) await sleep(100);
  await sleep(300);
  let code;
  if (!fs.existsSync(session)) {
    say(`FAIL  the headless server didn't start (exit ${serverExit}); see ${path.join(dir, "headless.log")}\n`);
    code = 1;
  } else {
    code = await new Promise((resolve) => {
      const child = spawn(process.execPath, [path.join(E2E, `${suite}.mjs`), ...args], { cwd: E2E, env, stdio: ["ignore", "pipe", "pipe"] });
      child.stdout.on("data", (d) => say(d.toString()));
      child.stderr.on("data", (d) => say(d.toString()));
      // On a timeout, SIGTERM first: puppeteer closes its browser on it (a SIGKILL would leave
      // chrome-headless-shell running, since puppeteer starts it in its own process group).
      let hard;
      const timer = setTimeout(() => {
        say(`FAIL  timed out after ${timeout / 1000}s\n`);
        child.kill("SIGTERM");
        hard = setTimeout(() => child.kill("SIGKILL"), 5000);
      }, timeout);
      child.on("exit", (c, sig) => {
        clearTimeout(timer);
        clearTimeout(hard);
        resolve(sig ? 1 : c);
      });
    });
  }
  if (serverExit === null) {
    server.kill("SIGTERM");
    for (let i = 0; i < 30 && serverExit === null; i++) await sleep(100);
    if (serverExit === null) server.kill("SIGKILL");
  }
  fs.closeSync(serverLog);
  fs.closeSync(log);
  const text = fs.readFileSync(logFile, "utf8");
  const pass = (text.match(/^PASS/gm) ?? []).length;
  const fail = (text.match(/^FAIL/gm) ?? []).length;
  const skips = text.match(/^SKIP.*$/gm) ?? [];
  const { expect } = o;
  // Why a run that failed nothing still isn't ok.
  const why = [];
  if (expect !== undefined && pass < expect) why.push(`${pass} of its ${expect} checks passed, ${expect - pass} didn't run`);
  if (expect !== undefined && pass > expect) why.push(`${pass} checks passed where ${expect} are expected: update its count in run.mjs and the README`);
  why.push(...skips.map((s) => s.replace(/^SKIP\s+/, "skipped ")));
  const status = code !== 0 || fail > 0 ? "FAIL" : why.length ? "PART" : "ok";
  if (!opts.keep && status === "ok") fs.rmSync(dir, { recursive: true, force: true });
  return { name, code, pass, fail, skip: skips.length, expect, why, status, logFile };
}

const results = [];
for (const [suite, kind, o] of runs) {
  const stream = runs.length === 1 && !opts.quiet;
  const r = await runOne(suite, kind ?? "proj", o ?? {}, stream);
  results.push(r);
  if (runs.length > 1) {
    const counts = `${r.pass} pass ${r.fail} fail${r.skip ? ` ${r.skip} skip` : ""}`;
    const short = r.expect !== undefined && r.pass !== r.expect ? ` (expected ${r.expect} pass)` : "";
    console.log(`${r.status.padEnd(4)} ${r.name.padEnd(18)} exit=${r.code} ${counts}${short}`);
  }
}

const failed = results.filter((r) => r.status === "FAIL");
const part = results.filter((r) => r.status === "PART");
const sum = (f) => results.reduce((a, r) => a + f(r), 0);
const expected = results.every((r) => r.expect !== undefined) ? ` of ${sum((r) => r.expect)} expected` : "";
console.log(
  `\n${results.length - failed.length - part.length}/${results.length} suites ok` +
    `${part.length ? `, ${part.length} PART (passed, but not in full)` : ""}${failed.length ? `, ${failed.length} failed` : ""}; ` +
    `${sum((r) => r.pass)}${expected} checks passed, ${sum((r) => r.fail)} failed, ${sum((r) => r.skip)} skipped. Logs: ${root}`,
);
for (const r of part) console.log(`  PART:   ${r.name}: ${r.why.join("; ")}. ${r.logFile}`);
for (const r of failed) console.log(`  failed: ${r.name} (exit ${r.code}) ${r.logFile}`);
process.exit(failed.length ? 1 : part.length ? 3 : 0);
