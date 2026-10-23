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
//   --jobs <n>     run up to n suites at once (default 1: one at a time, in the list's order). Each
//                  still gets its own fresh server, scratch HOME, folder and browser; the suites
//                  marked `serial` in SUITES run after the others, alone. Results print in the
//                  list's order whatever the order they finish in, with the same counts, rules and
//                  exit status as at --jobs 1.
//   --tier <pr|nightly>
//                  with `all`: only the runs of that tier. Each run in SUITES is in the PR tier unless
//                  it says tier: "nightly", with why (a speed bound, a known flake). Without --tier,
//                  `all` is every run, as ci/check.sh wants. Nightly runs are never weakened to move
//                  back: their cause is fixed, then the tag goes.
//   --shard <i>/<n>
//                  with `all`: only the i-th of n parts of the runs (after --tier), so n machines
//                  can share them. Each run is in exactly one part: the runs are dealt out longest
//                  first (by their `s`) to the part with the least so far, so the parts take about
//                  as long; within a part they keep the list's order. The parts of one --tier
//                  together are exactly the runs of that --tier without --shard.
//   --no-clap      with `all`: leave out the runs marked `clap` (live3b, ph-window), for a machine
//                  without the test plugins (CI's Mac); said at the start. Without it, a run that
//                  needs them and lacks them is PART, never ok
//   --list         print the runs that would run, one name a line (with --tier, --shard and
//                  --no-clap), and stop: no server, no browser
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
// `serial`: with --jobs above 1, the suite runs after the others, alone, for the reason its comment
// gives (none needs it at present: what suites share was checked, see the README's "In parallel").
// `s`: about how many seconds the run takes, one at a time on the Linux server (2026-10-21), so
// --shard can make parts of about the same length; it needn't be exact. `tier: "nightly"`, with
// `why`: the run is left out of the PR tier (--tier pr) and runs in the nightly one, for one of
// two reasons only. speed: it judges how fast something happens (a bound under 2 s, a rate), so
// a slow or busy machine fails it with correct code; a wait that only fails if something never
// happens isn't one. flake: it failed on correct code in a recorded run, and the cause isn't
// fixed yet (HANDOFF.md's "Known flakes").
export const SUITES = [
  ["graph", "proj", { expect: 19, s: 3 }],
  ["cables", "cables", { expect: 12, s: 5 }],
  ["undo", "cables", { expect: 10, s: 6 }],
  ["nx-undo", "fix", { expect: 18, s: 2 }],
  ["ag-notice", "fix", { expect: 47, s: 20 }],
  ["effects", "proj", { expect: 12, s: 7 }],
  ["nx-effects", "fix", { expect: 47, s: 18, tier: "nightly", why: "flake: a tooltip read before it changed (HANDOFF.md)" }],
  ["layout", "proj", { expect: 39, s: 8 }],
  ["views", "proj", { expect: 30, s: 2 }],
  ["code", "proj", { expect: 58, s: 7 }],
  ["notes", "proj", { expect: 63, s: 9 }],
  ["s8-audition", "fix", { expect: 51, s: 10, tier: "nightly", why: "speed: a closed page lets go of its note within 1 s" }],
  ["nx-piano", "fix", { expect: 39, s: 6 }],
  ["nx-midi", "fix", { expect: 24, s: 3 }],
  ["st-record", "fix", { expect: 66, s: 24, tier: "nightly", why: "speed: R records within 2 s" }],
  ["io-record", "fix", { expect: 38, s: 7 }],
  ["pl-notes", "fix", { expect: 102, s: 16 }],
  ["pl-lanes", "fix", { expect: 28, s: 4 }],
  ["ports", "proj", { expect: 46, s: 5 }],
  ["s8-meters", "fix", { expect: 24, s: 17, tier: "nightly", why: "speed: 20 to 40 meter messages a second" }],
  ["dsp", "proj", { expect: 33, s: 3 }],
  ["start", "proj", { expect: 24, s: 3 }],
  ["start", "empty", { expect: 14, s: 2 }],
  ["st-guide", "none", { expect: 38, s: 7 }],
  ["fixes-graph", "fix", { expect: 42, s: 8 }],
  ["fixes-notes", "fix", { expect: 37, s: 6 }],
  ["attention", "attn", { expect: 60, s: 6, tier: "nightly", why: "flake: first runs failed in three checks, 2026-10-15 to 10-17; cause not recorded" }],
  ["st-errors", "fix", { expect: 34, s: 5, tier: "nightly", why: "flake: waited over 30 s for a Needs attention row, under load (HANDOFF.md)" }],
  ["newsong", "none", { expect: 71, s: 10, tier: "nightly", why: "flake: the New Song dialog didn't open after Cmd-N, at half CPU (2026-10-21)" }],
  ["pl-projects", "none", { expect: 66, s: 10 }],
  ["nx-settings", "none", { expect: 42, s: 12 }],
  ["nx-nodevice", "none", { expect: 11, s: 2 }],
  ["sound-stops", "none", { expect: 13, s: 8 }],
  ["ex-faults", "none", { expect: 10, s: 6 }],
  ["export", "export", { expect: 37, s: 6, tier: "nightly", why: "speed: the server answers within 300 ms while exporting" }],
  ["st-midi", "fix", { expect: 41, s: 4, tier: "nightly", why: "flake: a first run failed (2026-10-18); cause not recorded" }],
  ["st-stems", "export", { expect: 49, s: 6 }],
  ["fx-fixes", "none", { expect: 75, s: 34 }],
  ["s4-clips", "fix", { expect: 28, s: 3 }],
  ["s8-clips", "fix", { expect: 94, s: 11, tier: "nightly", why: "flake: the left edge dragged a bar right read 8+28 (HANDOFF.md)" }],
  ["s8-automation", "fix", { expect: 45, s: 4 }],
  ["s8-leases", "fix", { expect: 55, s: 14, tier: "nightly", why: "speed: a lease closes within 1 s, and 3 or more tempos are seen mid-drag" }],
  ["st-audio", "fix", { expect: 94, s: 8, tier: "nightly", why: "flake: the revision moved after an MP3 was refused (HANDOFF.md)" }],
  ["pl-zoom", "fix", { expect: 62, s: 20 }],
  ["pl-arrange", "fix", { expect: 60, s: 8 }],
  ["nx-nits", "fix", { expect: 27, s: 6 }],
  ["s4-attn", "fix", { expect: 13, s: 2 }],
  ["s4-problems", "fix", { expect: 18, s: 2 }],
  ["s4-meter", "fix", { expect: 8, s: 2 }],
  ["live3a", "fix", { expect: 49, s: 15, tier: "nightly", why: "speed: a ruler click and a seek land within 1 s" }],
  ["live3b", "proj", { clap: true, expect: 60, s: 37, tier: "nightly", why: "speed: previews answer within 150 ms during a compile; flake: its release writes another db() (HANDOFF.md; twice at half CPU, 2026-10-21)" }],
  ["ph-window", "none", { clap: true, expect: 9, s: 3 }],
  ["s5-knobs", "fix", { expect: 120, s: 5, tier: "nightly", why: "flake: the page detached after a double-click on a knob (HANDOFF.md)" }],
  ["s6-builders", "fix", { expect: 56, s: 5 }],
  ["s6-write", "fix", { expect: 36, s: 7 }],
  ["s6-fixes", "fix", { expect: 19, s: 4 }],
  ["taste", "fix", { expect: 39, s: 27, tier: "nightly", why: "flake: the Bass header's click didn't select it within 30 s (HANDOFF.md)" }],
  ["st-tips", "fix", { expect: 45, s: 11 }],
  ["gestures", "fix", { expect: 30, s: 8, tier: "nightly", why: "flake: a first run failed (2026-10-18); cause not recorded" }],
  ["g11-play", "fix", { expect: 24, s: 11 }],
  ["s8-tracks", "fix", { expect: 75, s: 5, tier: "nightly", why: "speed: within 1.5 s; flake: it reads the new track's tab a moment too soon (twice at half CPU, 2026-10-21)" }],
  ["pl-tracks", "fix", { expect: 20, s: 4 }],
  ["s8-mute", "export", { expect: 152, s: 28, tier: "nightly", why: "speed: overlays.json within 1.5 s; flake: once on the Mac at e7518f4 (HANDOFF.md)" }],
  ["s8-songbar", "fix", { expect: 84, s: 13 }],
  ["nx-click", "fix", { expect: 20, s: 8 }],
  ["s8-mix", "fix", { expect: 80, s: 23 }],
  ["pl-mix", "fix", { expect: 47, s: 14, tier: "nightly", why: "speed: the meters floor within a second after a stop; flake: a first run failed (2026-10-18)" }],
  ["bus-mix", "fix", { expect: 37, s: 8, tier: "nightly", why: "flake: with nothing sent, the Reverb's meter rose above the floor in 3 of 31 runs, 2 of them in 28 runs alone on 4 cores (2026-10-21); cause not known" }],
  ["pl-sounds", "fix", { expect: 46, s: 4 }],
  ["pl-ids", "fix", { expect: 39, s: 3, tier: "nightly", why: "flake: a rename took more than one revision, at half CPU (2026-10-21)" }],
  ["pc-keys", "fix", { expect: 37, s: 3 }],
  ["ab-sampler", "export", { expect: 23, s: 4 }],
  ["sx-follow", "fix", { expect: 22, s: 2 }],
  ["br-plugins", "fix", { probe: true, expect: 19, s: 2 }],
  ["br-presets", "fix", { probe: true, expect: 29, s: 2 }],
];
const KINDS = ["proj", "cables", "fix", "attn", "empty", "none", "export"];

function usage(msg) {
  if (msg) console.error(`run.mjs: ${msg}`);
  console.error("usage: M2_HEADLESS=<bin> node e2e/run.mjs <suite|all> [kind] [--repo DIR] [--out DIR] [--keep] [--quiet] [--jobs N] [--tier pr|nightly] [--shard I/N] [--no-clap] [--list]");
  console.error(`suites: ${[...new Set(SUITES.map(([s]) => s))].join(" ")}`);
  process.exit(2);
}

const argv = process.argv.slice(2);
const opts = { repo: path.resolve(E2E, ".."), out: null, keep: false, quiet: false, jobs: 1, tier: null, shard: null, list: false, noClap: false };
const pos = [];
for (let i = 0; i < argv.length; i++) {
  const a = argv[i];
  if (a === "--repo") opts.repo = path.resolve(argv[++i] ?? usage("--repo needs a folder"));
  else if (a === "--out") opts.out = path.resolve(argv[++i] ?? usage("--out needs a folder"));
  else if (a === "--keep") opts.keep = true;
  else if (a === "--quiet") opts.quiet = true;
  else if (a === "--jobs" || a.startsWith("--jobs=")) {
    const n = a === "--jobs" ? argv[++i] : a.slice("--jobs=".length);
    if (!/^[1-9][0-9]*$/.test(n ?? "")) usage("--jobs needs a whole number, 1 or more");
    opts.jobs = Number(n);
  } else if (a === "--tier" || a.startsWith("--tier=")) {
    const t = a === "--tier" ? argv[++i] : a.slice("--tier=".length);
    if (t !== "pr" && t !== "nightly") usage("--tier is pr or nightly");
    opts.tier = t;
  } else if (a === "--shard" || a.startsWith("--shard=")) {
    const v = a === "--shard" ? argv[++i] : a.slice("--shard=".length);
    const m = /^([1-9][0-9]*)\/([1-9][0-9]*)$/.exec(v ?? "");
    if (!m || Number(m[1]) > Number(m[2])) usage("--shard needs i/n, with i from 1 to n");
    opts.shard = [Number(m[1]), Number(m[2])];
  } else if (a === "--list") opts.list = true;
  else if (a === "--no-clap") opts.noClap = true;
  else if (a === "-h" || a === "--help") usage();
  else if (a.startsWith("--")) usage(`unknown option ${a}`);
  else pos.push(a);
}
const [which, kindArg] = pos;
if (!which) usage();
if (kindArg && !KINDS.includes(kindArg)) usage(`unknown kind ${kindArg} (${KINDS.join(", ")})`);
if ((opts.tier || opts.shard || opts.noClap) && which !== "all") usage("--tier, --shard and --no-clap go with all");

// shard(list, i, n): the i-th of n parts of list, dealt out longest first (by `s`, ties in the
// list's order) to the part with the least so far (ties: the first), in the list's order.
function shard(list, i, n) {
  const secs = (k) => list[k][2]?.s ?? 10;
  const order = list.map((_, k) => k).sort((a, b) => secs(b) - secs(a) || a - b);
  const load = new Array(n).fill(0);
  const part = new Array(list.length);
  for (const k of order) {
    let best = 0;
    for (let j = 1; j < n; j++) if (load[j] < load[best]) best = j;
    part[k] = best;
    load[best] += secs(k);
  }
  return list.filter((_, k) => part[k] === i - 1);
}

let runs;
// What --tier and --shard chose, said at the start of the run.
let chose = "";
if (which === "all") {
  if (kindArg) usage("all takes no kind");
  runs = SUITES;
  // Two runs of one name would share a folder and a log.
  const names = SUITES.map(([s, k]) => `${s}-${k}`);
  const twice = names.find((n, i) => names.indexOf(n) !== i);
  if (twice) usage(`SUITES lists ${twice} twice`);
  const untagged = SUITES.find(([, , o]) => o.tier && (o.tier !== "nightly" || !o.why));
  if (untagged) usage(`SUITES: ${untagged[0]}-${untagged[1]} needs tier: "nightly" and a why`);
  if (opts.tier) {
    runs = runs.filter(([, , o]) => (o.tier ?? "pr") === opts.tier);
    chose = `tier ${opts.tier}: ${runs.length} of ${SUITES.length} runs`;
  }
  if (opts.noClap) {
    const gone = runs.filter(([, , o]) => o.clap).map(([s, k]) => `${s}-${k}`);
    runs = runs.filter(([, , o]) => !o.clap);
    chose += `${chose ? "; " : ""}--no-clap: ${gone.length ? `${gone.join(", ")} left out` : "nothing to leave out"}`;
  }
  if (opts.shard) {
    const [i, n] = opts.shard;
    const of = runs.length;
    runs = shard(runs, i, n);
    const secs = runs.reduce((a, [, , o]) => a + (o.s ?? 10), 0);
    chose += `${chose ? "; " : ""}shard ${i}/${n}: ${runs.length} of ${of} runs (about ${secs} s one at a time)`;
  }
} else {
  const known = SUITES.filter(([s]) => s === which);
  if (!known.length) usage(`unknown suite ${which}`);
  if (!kindArg) runs = known.slice(0, 1);
  // A kind SUITES doesn't list for this suite: its options, but no count to expect.
  else runs = [known.find(([, k]) => k === kindArg) ?? [which, kindArg, { ...known[0][2], expect: undefined }]];
}

if (opts.list) {
  for (const [s, k] of runs) console.log(`${s}-${k ?? "proj"}`);
  process.exit(0);
}

const headless = process.env.M2_HEADLESS && path.resolve(process.env.M2_HEADLESS);
if (!headless || !fs.existsSync(headless)) usage("set M2_HEADLESS to the headless server binary");
const cli = path.resolve(process.env.M2_CLI ?? path.join(path.dirname(headless), "..", "m2"));
if (!fs.existsSync(chrome)) usage(`no chrome-headless-shell at ${chrome} (set E2E_CHROME)`);
// No default: a suite gets the plugins you name, never your own folder unasked.
const clapPath = process.env.CLAP_PATH ? path.resolve(process.env.CLAP_PATH) : null;
const timeout = Number(process.env.E2E_TIMEOUT ?? 600) * 1000;
if (!fs.existsSync(path.join(opts.repo, "examples/first-song"))) usage(`${opts.repo} has no examples/first-song`);
if (!fs.existsSync(path.join(E2E, "node_modules/puppeteer-core"))) usage(`run \`npm ci\` in ${E2E} first`);

const made = opts.out ?? fs.mkdtempSync(path.join(os.tmpdir(), "m2-e2e-"));
fs.mkdirSync(made, { recursive: true });
// By its real path: the app names folders by theirs (a Mac's temporary folder is a link, /var to
// /private/var), so the suites' paths, HOME's among them, say the same.
const root = fs.realpathSync(made);

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
if (chose) console.log(chose);
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

function envFor(home, clap, probe) {
  const env = { ...process.env, HOME: home, M2_AUDIO: "none", M2_MIDI: "inject", M2_AUDIO_INPUT: "inject", M2_AUDIO_INPUTS: "Test mic,USB mic", E2E_CHROME: chrome, E2E_REPO: opts.repo, E2E_HEADLESS: headless };
  delete env.CLAP_PATH;
  delete env.M2_PROBE;
  if (clap && clapPath) env.CLAP_PATH = clapPath;
  // The probe plugin, which a debug server installs (br-plugins).
  if (probe) env.M2_PROBE = "1";
  return env;
}

async function runOne(suite, kind, o = {}, stream) {
  const name = `${suite}-${kind}`;
  const dir = path.join(root, name);
  const logFile = path.join(root, `${name}.log`);
  const { home, open, args } = prepare(dir, kind);
  const env = envFor(home, o.clap, o.probe);
  const session = path.join(home, ".music2/session.json");
  const log = fs.openSync(logFile, "w");
  // Once the log is closed, a late line goes to it by name: its old descriptor may belong to
  // another run's file by then.
  let closed = false;
  const say = (s) => {
    if (closed) fs.appendFileSync(logFile, s);
    else fs.writeSync(log, s);
    if (stream) process.stdout.write(s);
  };

  const serverLog = fs.openSync(path.join(dir, "headless.log"), "w");
  const server = spawn(headless, [open, session, `e2e${Date.now()}`], { env, stdio: ["ignore", serverLog, serverLog] });
  let serverExit = null;
  server.on("exit", (code, sig) => (serverExit = sig ?? code));
  // It couldn't be started: say so in its run, not as a crash of every run.
  server.on("error", (e) => (serverExit ??= e.code ?? String(e)));
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
      // Its output can still be on the way when it exits: wait for the end of both (at most 2 s,
      // in case something it started holds them open), so every line it printed is counted.
      const ends = Promise.all([child.stdout, child.stderr].map((s) => new Promise((r) => s.once("end", r).once("error", r))));
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
        Promise.race([ends, sleep(2000)]).then(() => resolve(sig ? 1 : c));
      });
      // node couldn't be started: that run fails, and the others go on.
      child.on("error", (e) => {
        clearTimeout(timer);
        clearTimeout(hard);
        say(`FAIL  the suite didn't start: ${e.message}\n`);
        resolve(1);
      });
    });
  }
  if (serverExit === null) {
    server.kill("SIGTERM");
    for (let i = 0; i < 30 && serverExit === null; i++) await sleep(100);
    if (serverExit === null) server.kill("SIGKILL");
  }
  fs.closeSync(serverLog);
  closed = true;
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

// One run, never a crash of all of them: anything runOne throws is that run's FAIL, in its log.
async function runSafe(suite, kind, o, stream) {
  const t0 = Date.now();
  try {
    return { ...(await runOne(suite, kind, o, stream)), ms: Date.now() - t0 };
  } catch (e) {
    const name = `${suite}-${kind}`;
    const logFile = path.join(root, `${name}.log`);
    fs.appendFileSync(logFile, `FAIL  run.mjs couldn't run it: ${e?.stack ?? e}\n`);
    return { name, code: 1, pass: 0, fail: 1, skip: 0, expect: o.expect, why: [], status: "FAIL", logFile, ms: Date.now() - t0 };
  }
}

// Every run, up to opts.jobs at once, the `serial` ones after the others and alone (at --jobs 1, all
// of them one at a time in the list's order, as before). Each result prints once every run before
// it in the list has printed, so the lines come in the list's order.
const started = Date.now();
const results = new Array(runs.length);
let printed = 0;
function finished(i, r) {
  results[i] = r;
  for (; printed < runs.length && results[printed]; printed++) {
    if (runs.length === 1) continue;
    const r = results[printed];
    const counts = `${r.pass} pass ${r.fail} fail${r.skip ? ` ${r.skip} skip` : ""}`;
    const short = r.expect !== undefined && r.pass !== r.expect ? ` (expected ${r.expect} pass)` : "";
    console.log(`${r.status.padEnd(4)} ${r.name.padEnd(18)} exit=${r.code} ${counts}${short}  ${Math.round(r.ms / 1000)} s`);
  }
}
const one = async (i) => {
  const [suite, kind, o] = runs[i];
  finished(i, await runSafe(suite, kind ?? "proj", o ?? {}, runs.length === 1 && !opts.quiet));
};
const all = runs.map((_, i) => i);
const together = opts.jobs > 1 ? all.filter((i) => !runs[i][2]?.serial) : all;
const alone = opts.jobs > 1 ? all.filter((i) => runs[i][2]?.serial) : [];
let next = 0;
await Promise.all(
  Array.from({ length: Math.min(opts.jobs, together.length) }, async () => {
    while (next < together.length) await one(together[next++]);
  }),
);
for (const i of alone) await one(i);

const failed = results.filter((r) => r.status === "FAIL");
const part = results.filter((r) => r.status === "PART");
const sum = (f) => results.reduce((a, r) => a + f(r), 0);
const expected = results.every((r) => r.expect !== undefined) ? ` of ${sum((r) => r.expect)} expected` : "";
console.log(
  `\n${results.length - failed.length - part.length}/${results.length} suites ok` +
    `${part.length ? `, ${part.length} PART (passed, but not in full)` : ""}${failed.length ? `, ${failed.length} failed` : ""}; ` +
    `${sum((r) => r.pass)}${expected} checks passed, ${sum((r) => r.fail)} failed, ${sum((r) => r.skip)} skipped. Logs: ${root}`,
);
console.log(`time: ${((Date.now() - started) / 60000).toFixed(1)} min, ${opts.jobs === 1 ? "one suite at a time" : `up to ${opts.jobs} suites at once${alone.length ? `, then ${alone.length} alone` : ""}`}`);
for (const r of part) console.log(`  PART:   ${r.name}: ${r.why.join("; ")}. ${r.logFile}`);
for (const r of failed) console.log(`  failed: ${r.name} (exit ${r.code}) ${r.logFile}`);
process.exit(failed.length ? 1 : part.length ? 3 : 0);
