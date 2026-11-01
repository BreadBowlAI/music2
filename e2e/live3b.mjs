// Step 3b in the app: knob drags are gesture leases (WRITE-22), the session is an actor, and plugin
// instances are carried across rebuilds (HOST-5). Run on a fresh headless server with a copy of
// examples/first-song (node run.mjs live3b):
//   slow     a knob dragged while the code editor's save compiles for over a second stays live:
//            every preview answers at once and the server plays each value as it comes, and the
//            drag outlives the save (the step-3 gate "previews never wait on a compile")
//   through  a drag goes on through an outside save to another file, and its release writes it
//   held     a knob held still past the server's 10 s TTL is kept alive by the page (C6)
//   closed   a window that closes ends the drag it left open (C6)
//   broken   a save that breaks the code mid-drag doesn't end it; the release is saved once the
//            code compiles again (C5)
//   renumbered  an outside save that inserts a call above the dragged knob in its own file keeps
//            the drag: the knob stays mounted under its new id, and its release writes its line
//   plugins  examples/with-plugins on a second headless server of the suite's own, whose player
//            plays into a file (M2_AUDIO_CAPTURE) that the suite listens to; Surge XT holds one long
//            chord into Airwindows. Needs CLAP_PATH naming the folder with both (never the user's
//            own by default: run.mjs has no default and passes CLAP_PATH on only when it is set).
//            Without it, these 20 checks are skipped with one SKIP line (40 of the suite's 60 run),
//            and run.mjs reports the run PART, not ok:
//            - an effect added from the UI while the chord sounds activates a renderer that
//              carries the instances, and the captured sound goes on with no gap or dip (HOST-5,
//              RENDER-25, RENDER-25a);
//            - the lead's level dragged while a save of tracks/kick.ts compiles for over a second
//              is heard in the capture before the save returns (WRITE-22, RENDER-29);
//            - the negative control: a seek mid-chord ends its notes (RENDER-9), and the same
//              check sees the sound fall;
//            - Export borrows the plugins (RENDER-1, step 7): with the lead back at its own level,
//              playback pauses, the top bar says "Paused for export", the capture is silent and
//              the playhead still for as long as the pause lasts (sampled until it ends, however
//              short the export), and play goes on from there once the file is written.
// The second server is E2E_HEADLESS and examples/ come from E2E_REPO (run.mjs sets both, see env.mjs).
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { spawn } from "node:child_process";

const S = process.argv[2];
const proj = `${S}/proj`;
const sess = JSON.parse(fs.readFileSync(`${S}/testhome/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
import { following } from "./caughtup.mjs";
import { HEADLESS, REPO as FIX } from "./env.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const consoleErrors = [];
const newPage = async (s = sess) => {
  const page = await browser.newPage();
  await page.setViewport({ width: 1600, height: 1000 });
  page.on("console", (m) => m.type() === "error" && consoleErrors.push(m.text()));
  page.on("pageerror", (e) => consoleErrors.push(String(e)));
  await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, s.page_token);
  await page.goto(s.url, { waitUntil: "domcontentloaded" });
  // The dock opens on the view this browser chose last: Device, or Mix.
  await page.waitForSelector("[data-testid=graph], [data-testid=mix]", { timeout: 8000 });
  return page;
};
const apiOf = (s) => async (path, opts = {}) =>
  (await fetch(`${s.url}/api/${path}`, { ...opts, headers: { "x-m2-token": s.page_token, "content-type": "application/json" } })).json();
const api = apiOf(sess);
const leases = () => api("graph/leases");
/** The code editor's save: answers once the session took the file in, its compile included. */
const putFile = (s, path, base, text) =>
  fetch(`${s.url}/api/graph/file`, { method: "PUT", headers: { "x-m2-token": s.page_token, "content-type": "application/json" }, body: JSON.stringify({ path, base, text }) });
/** A build that spins for about a second and a half (QuickJS has no clock; a counted loop is the way). */
const slowly = (text) => `for (let i = 0; i < 150000000; i++) {}\n${text}`;

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const skip = (what, why) => console.log(`SKIP  ${what}: ${why}`);
const until = async (f, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const songTs = () => fs.readFileSync(`${proj}/song.ts`, "utf8");
const bassTs = () => fs.readFileSync(`${proj}/tracks/bass.ts`, "utf8");
const bass = bassTs();

const page = await newPage();
const caughtUp = following(page, () => api("graph"), check);
const show = async (p, name) => {
  await p.click(`[data-module-tab="${name}"]`);
  await p.waitForSelector(`[data-module="${name}"]`);
};
// A knob's centre (a fader's thumb), scrolled into the panel's view.
const knobAt = async (p, sel) => {
  const grip = `${sel} svg, ${sel} [data-testid=fader-thumb]`;
  await p.$eval(grip, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
  const b = await (await p.$(grip)).boundingBox();
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
};
const value = (p, sel) => p.$eval(sel, (e) => e.dataset.value);
/** The Mix view: levels and pans are its strips' (MX-6). */
const mix = async (p) => {
  await p.click('[data-view-tab="mix"]');
  await p.waitForSelector("[data-testid=mix] [data-strip]");
};
const hats = '[data-testid=mix] [data-knob="hats.pan"]';
const keys = '[data-testid=mix] [data-knob="keys.pan"]';
const keysDb = '[data-testid=mix] [data-knob="keys.db"]';
await mix(page);

// ---- slow: a drag while the code editor's save compiles for over a second.
// Each preview is timed from its request to its answer.
const previews = [];
const asked = new Map();
page.on("request", (r) => r.url().endsWith("/api/graph/preview") && asked.set(r, Date.now()));
page.on("requestfinished", (r) => {
  const t = asked.get(r);
  if (t !== undefined) previews.push({ start: t, ms: Date.now() - t });
  asked.delete(r);
});
let at = await knobAt(page, keysDb);
await page.mouse.move(at.x, at.y);
await page.mouse.down();
await page.mouse.move(at.x, at.y + 3);
check(await until(async () => (await leases()).length === 1), "a drag on keys.db holds a lease");
const slowLease = (await leases())[0]?.lease;
const t0 = Date.now();
let saved = null;
const saving = putFile(sess, "tracks/bass.ts", bass, slowly(bass)).then((r) => { saved = Date.now(); return r.status; });
// While it compiles: move, then ask the server what it plays.
const followed = [];
for (let step = 2; saved === null && Date.now() - t0 < 10_000; step++) {
  await page.mouse.move(at.x, at.y + step * 3);
  await sleep(80);
  const shown = Number(await value(page, keysDb));
  const playing = (await leases())[0]?.value;
  if (saved === null) followed.push({ shown, playing });
}
const savedStatus = await saving;
check(savedStatus === 200 && saved - t0 > 1000, `the save compiled for over a second (${saved - t0} ms, status ${savedStatus})`);
const during = previews.filter((p) => p.start >= t0 && p.start + p.ms <= saved);
const slowest = Math.max(0, ...during.map((p) => p.ms));
check(during.length >= 5 && slowest < 150, `every preview sent while it compiled answered at once (${during.length} of them, the slowest in ${slowest} ms)`);
const same = followed.filter((f) => Math.abs(f.shown - f.playing) < 1e-6).length;
check(followed.length >= 5 && same >= followed.length - 1, `meanwhile the server played what the knob showed (${same} of ${followed.length} reads)`);
check((await leases())[0]?.lease === slowLease, "the drag outlived the save, on the same lease");
await page.mouse.move(at.x, at.y + 60);
const slowShown = await value(page, keysDb);
await page.mouse.up();
check(await until(() => songTs().includes(`db: db(${slowShown})`)), `its release writes db(${slowShown}) into song.ts`);
check(await until(async () => (await leases()).length === 0), "and ends the lease");
check((await putFile(sess, "tracks/bass.ts", slowly(bass), bass)).status === 200 && bassTs() === bass, "bass.ts saved back");

// ---- through: an outside save to another file mid-drag. The mixer is one call, and Keys' level is in
// it: the drag goes from the revision the page shows, so the page shows the release's first.
await caughtUp();
at = await knobAt(page, hats);
await page.mouse.move(at.x, at.y);
await page.mouse.down();
// 60px in all, of a knob's 300 (taste.md KN-1): 0.3 → 0.7 on the pan's -1..1.
for (let i = 1; i <= 4; i++) await page.mouse.move(at.x, at.y - i * 7.5);
check(await until(async () => (await leases()).length === 1), "dragging a knob holds a lease on the server");
const own = (await leases())[0];
check(own?.client?.length > 0 && own?.file === "song.ts", `the lease is the window's, on song.ts (${JSON.stringify(own)})`);
const before = (await api("graph")).view.revision;
fs.writeFileSync(`${proj}/tracks/bass.ts`, bass.replace("khz(2.5)", "khz(3)"));
check(await until(async () => (await api("graph")).view.revision > before), "the outside save to tracks/bass.ts is taken in");
check((await leases()).length === 1 && (await leases())[0].lease === own.lease, "the drag goes on through it, on the same lease");
for (let i = 5; i <= 8; i++) await page.mouse.move(at.x, at.y - i * 7.5);
await page.mouse.up();
check(await until(() => songTs().includes("pan: 0.7 }")), "releasing wrote `pan: 0.7` into song.ts");
check(bassTs().includes("khz(3)"), "and the outside edit to bass.ts stays");
check(await until(async () => (await leases()).length === 0), "the commit ended the lease");

// ---- held: still for 12 s, longer than the server's TTL; the page's keepalive keeps it.
await caughtUp();
at = await knobAt(page, keys);
await page.mouse.move(at.x, at.y);
await page.mouse.down();
await page.mouse.move(at.x, at.y - 8);
check(await until(async () => (await leases()).length === 1), "a second drag holds a lease");
await sleep(12_500);
check((await leases()).length === 1, "held still for 12.5 s, the drag is still alive (the server ends idle ones after 10 s)");
await page.mouse.move(at.x, at.y - 16);
await page.mouse.up();
const shown = await value(page, keys);
check(await until(() => songTs().includes(`pan: ${shown} }`)), `and its release writes the value it shows (${shown})`);

// ---- closed: a second window drags, then closes; its drag ends, and nothing is written. It opens
// once the release's revision is published, so its drag goes from that one.
await caughtUp();
const other = await newPage();
await mix(other);
const kept = songTs();
at = await knobAt(other, hats);
await other.mouse.move(at.x, at.y);
await other.mouse.down();
for (let i = 1; i <= 4; i++) await other.mouse.move(at.x, at.y + i * 4);
check(await until(async () => (await leases()).length === 1), "the other window's drag holds a lease");
const theirs = (await leases())[0];
check(theirs.client !== own.client, "a lease of its own window");
await other.close();
check(await until(async () => (await leases()).length === 0), "closing that window ended its drag");
check(songTs() === kept, "and wrote nothing");

// ---- broken: a save that breaks the code mid-drag; the release waits for the code to compile.
at = await knobAt(page, hats);
await page.mouse.move(at.x, at.y);
await page.mouse.down();
for (let i = 1; i <= 2; i++) await page.mouse.move(at.x, at.y + i * 4);
check(await until(async () => (await leases()).length === 1), "a drag holds a lease");
fs.writeFileSync(`${proj}/tracks/bass.ts`, "export default (;\n");
check(await until(async () => (await api("graph")).diagnostics.length > 0), "a broken save to tracks/bass.ts is reported");
check((await leases()).length === 1, "the drag goes on through the broken save");
for (let i = 3; i <= 4; i++) await page.mouse.move(at.x, at.y + i * 4);
const released = await value(page, hats);
await page.mouse.up();
check(await until(async () => (await leases())[0]?.state === "pending"), "the release waits, pending, for the code to compile");
check(await until(async () => (await page.content()).includes("saved once the code compiles again")), "and the page says so");
check(!songTs().includes(`pan: ${released} }`), "nothing is written yet");
fs.writeFileSync(`${proj}/tracks/bass.ts`, bass);
check(await until(() => songTs().includes(`pan: ${released} }`)), `fixed, the code compiles and the release is written (pan: ${released})`);
check(await until(async () => (await leases()).length === 0), "and the lease is gone");

// ---- renumbered: an outside save inserts a call above the dragged knob in its own file, which
// renumbers it (WRITE-22, WRITE-12). The server carries the drag to the knob's new id, and the page
// keeps the knob mounted, sending no cancel: the drag goes on, and its release writes the knob's
// own line.
const cancels = [];
const onCancel = (r) => r.url().endsWith("/api/graph/preview/cancel") && cancels.push(r.postData());
page.on("request", onCancel);
const songBefore = songTs();
at = await knobAt(page, keysDb);
const start = await value(page, keysDb);
await page.mouse.move(at.x, at.y);
await page.mouse.down();
for (let i = 1; i <= 4; i++) { await page.mouse.move(at.x, at.y - i * 4); await sleep(60); }
check(await until(async () => (await leases()).length === 1), "a drag on keys.db holds a lease");
const first = (await leases())[0];
const idBefore = await page.$eval(keysDb, (e) => { e.__mounted = true; return e.dataset.knobId; });
const rev0 = (await api("graph")).view.revision;
fs.writeFileSync(`${proj}/song.ts`, songBefore.replace("kick: kick.out,", "kick: duck(kick.out, kick.env, { depth: db(-3) }),"));
check(await until(async () => (await api("graph")).view.revision > rev0 && (await page.$eval(keysDb, (e) => e.dataset.knobId)) !== idBefore),
  "the save renumbers the knob, and the page shows its new id");
const carried = (await leases())[0];
check(carried?.lease === first.lease && carried.state === "live" && carried.id !== first.id,
  `the drag goes on, on the same lease, carried from ${first.id} to ${carried?.id}`);
check(await page.$eval(keysDb, (e) => e.__mounted === true), "the knob stayed mounted");
for (let i = 5; i <= 12; i++) { await page.mouse.move(at.x, at.y - i * 4); await sleep(60); }
const dragged = await value(page, keysDb);
check(await until(async () => (await leases())[0]?.value === Number(dragged)), `the server plays what the knob shows (${dragged})`);
await page.mouse.up();
check(dragged !== start && await until(() => songTs().includes(`keys: { in: keys.out, db: db(${dragged}),`)),
  `its release writes db(${dragged}) on the keys line (it was db(${start}))`);
check(songTs().includes("kick: duck(kick.out, kick.env, { depth: db(-3) }),"), "and the inserted call stays");
check(await until(async () => (await leases()).length === 0), "the commit ended the lease");
check(cancels.length === 0, `the page sent no cancel ${cancels.join(" ")}`);
page.off("request", onCancel);

// ---- plugins: examples/with-plugins, played into a file the suite listens to.
await withPlugins();

const unexpected = consoleErrors.filter((e) => !e.includes("400"));
check(unexpected.length === 0, `no console errors ${unexpected.join(" | ")}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);

async function withPlugins() {
  const clap = process.env.CLAP_PATH;
  if (!clap || !fs.existsSync(`${clap}/Surge XT.clap`) || !fs.existsSync(`${clap}/Airwindows Consolidated.clap`)) {
    skip("plugins", "CLAP_PATH must name a folder with Surge XT and Airwindows Consolidated");
    return;
  }
  const root = `${S}/plugins`;
  fs.rmSync(root, { recursive: true, force: true });
  fs.mkdirSync(`${root}/home/.music2`, { recursive: true });
  const dir = `${root}/proj`;
  fs.cpSync(`${FIX}/examples/with-plugins`, dir, { recursive: true });
  // One long chord, the kick silent and the cutoff still: a sound that should never dip.
  const chord = ["A3", "C4", "E4"].map((pitch) => ({ pitch, start: 0, length: 256, velocity: 0.8 }));
  fs.writeFileSync(`${dir}/data/lead/notes.json`, JSON.stringify({ clips: [{ id: "wnnf4t7q", start: 0, length: 256, notes: chord }] }));
  fs.writeFileSync(`${dir}/data/kick/hits.json`, JSON.stringify({ clips: [] }));
  fs.writeFileSync(`${dir}/data/lead/cutoff.json`, JSON.stringify({ unit: "scalar", clips: [{ id: "rt0wbb3b", start: 0, length: 256, points: [[0, 0.6], [256, 0.6]] }] }));
  const capture = `${root}/played.f32`;
  const sessFile = `${root}/home/.music2/session.json`;
  const env = { ...process.env, M2_AUDIO: "none", M2_MIDI: "none", M2_AUDIO_CAPTURE: capture, M2_AUDIO_FRAMES: "512", HOME: `${root}/home`, CLAP_PATH: clap };
  const server = spawn(HEADLESS, [dir, sessFile, `p${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(`${root}/headless.log`, "w")] });
  try {
    if (!(await until(() => fs.existsSync(sessFile), 20_000))) {
      check(false, `the with-plugins server started (${fs.readFileSync(`${root}/headless.log`, "utf8").slice(0, 300)})`);
      return;
    }
    await sleep(300);
    const s2 = JSON.parse(fs.readFileSync(sessFile, "utf8"));
    const api2 = apiOf(s2);
    const state = await api2("graph");
    const missing = (state.problems ?? []).filter((p) => p.code === "plugin.missing");
    check(missing.length === 0, `with-plugins opens with its plugins (${missing.map((p) => p.message).join("; ") || "none missing"})`);

    // The capture: interleaved stereo f32 at 48 kHz. Frames played so far, and the level (RMS, dB)
    // of each 100 ms window (or `win` frames) between two frames.
    const RATE = 48_000, WIN = 4_800;
    const frames = () => Math.floor(fs.statSync(capture).size / 8);
    const levels = (from, to, win = WIN) => {
      const buf = fs.readFileSync(capture);
      const x = new Float32Array(buf.buffer, buf.byteOffset, Math.floor(buf.length / 4));
      const out = [];
      for (let f = Math.max(0, from); f + win <= Math.min(to, x.length / 2); f += win) {
        let sum = 0;
        for (let i = 2 * f; i < 2 * (f + win); i++) sum += x[i] * x[i];
        out.push(10 * Math.log10(sum / (2 * win) + 1e-24));
      }
      return out;
    };
    const median = (v) => [...v].sort((a, b) => a - b)[Math.floor(v.length / 2)];
    const fmt = (v) => v.map((d) => d.toFixed(0)).join(" ");

    const p2 = await newPage(s2);
    await p2.click('[data-testid=play]');
    check(await until(async () => (await api2("graph/status")).playing, 4000), "with-plugins plays");
    await sleep(1500);
    const sounding = levels(frames() - RATE, frames());
    check(sounding.length >= 9 && median(sounding) > -45, `Surge XT's chord sounds through Airwindows (${fmt(sounding)} dB)`);

    // An effect added from the UI while the chord sounds: a new renderer, carrying the instances.
    await show(p2, "kick");
    const kickTs = () => fs.readFileSync(`${dir}/tracks/kick.ts`, "utf8");
    const rev = (await api2("graph")).view.revision;
    const clicked = frames();
    const ref = median(levels(clicked - RATE, clicked));
    await p2.click('[data-module="kick"] button[aria-label="Add an effect after drums.drum"]');
    await p2.waitForSelector('[data-module="kick"] [data-effect="delay"]');
    await p2.click('[data-module="kick"] [data-effect="delay"]');
    check(await until(() => kickTs().includes("delay(drums.drum("), 5000), "a delay goes after the kick while the chord sounds");
    check(await until(async () => (await api2("graph")).view.revision > rev, 5000), "and a new revision plays");
    const taken = frames();
    await sleep(1200);
    const across = levels(clicked - RATE / 2, taken + RATE);
    const worst = Math.min(...across), loudest = Math.max(...across);
    check(
      across.length >= 10 && worst > ref - 6 && loudest < ref + 6,
      `the plugins' sound goes on across the activation: every 100 ms within 6 dB of ${ref.toFixed(1)} dB (${fmt(across)})`,
    );

    // The negative control: a seek mid-chord ends its notes (RENDER-9), and the same check sees the
    // sound fall. Then back to the start, where the chord begins again.
    const loud = median(levels(frames() - RATE / 2, frames()));
    const sought = frames();
    await api2("graph/seek", { method: "POST", body: JSON.stringify({ beat: 100 }) });
    await sleep(1600);
    const after = levels(sought, frames());
    check(Math.min(...after) < loud - 10, `negative control: a seek mid-chord ends it, and the same check sees the level fall (${loud.toFixed(1)} dB, then ${fmt(after)})`);
    await api2("graph/seek", { method: "POST", body: JSON.stringify({ beat: 0 }) });
    await sleep(1200);
    check(median(levels(frames() - RATE / 2, frames())) > loud - 6, "from the start, the chord sounds again");

    // The lead's level dragged down while a save of tracks/kick.ts compiles: heard before it returns.
    await mix(p2);
    const lead = '[data-testid=mix] [data-knob="lead.db"]';
    const song0 = fs.readFileSync(`${dir}/song.ts`, "utf8");
    const kick = kickTs();
    at = await knobAt(p2, lead);
    await p2.mouse.move(at.x, at.y);
    await p2.mouse.down();
    await p2.mouse.move(at.x, at.y + 2);
    const level0 = median(levels(frames() - RATE / 2, frames()));
    let back = null;
    const t1 = Date.now();
    const compiling = putFile(s2, "tracks/kick.ts", kick, slowly(kick)).then((r) => { back = { status: r.status, at: Date.now(), frame: frames() }; });
    await sleep(150);
    // Down the fader into its foot, and no further than the window's edge.
    for (let i = 1; i <= 20 && back === null; i++) {
      await p2.mouse.move(at.x, Math.min(at.y + 2 + i * 8, 990));
      await sleep(20);
    }
    const draggedTo = Number(await value(p2, lead));
    const moved = frames();
    await sleep(400);
    const heard = back === null ? median(levels(moved, frames())) : NaN;
    const stillCompiling = back === null;
    await compiling;
    check(back.status === 200 && back.at - t1 > 1000, `the save of tracks/kick.ts compiled for over a second (${back.at - t1} ms)`);
    check(
      stillCompiling && heard < level0 - 10,
      `the level dragged to ${draggedTo} dB is heard while it compiles: ${level0.toFixed(1)} → ${heard.toFixed(1)} dB, before the save returned`,
    );
    const releasedAt = await value(p2, lead);
    await p2.mouse.up();
    check(await until(() => fs.readFileSync(`${dir}/song.ts`, "utf8").includes(`db: db(${releasedAt})`), 5000), `its release writes db(${releasedAt})`);

    // RENDER-1 (step 7): Export borrows the plugins that play. Playback pauses (a tape pause: it
    // fades out, holds, and resumes where it was), the top bar says so, and the file is written.
    // The export resets the plugins' voices, its stated cost: from the start the chord sounds again.
    const exports = `${root}/home/Music/music2/Exports`;
    // What the engine is busy with is a state of the top bar's status pill (taste ST-1): its title lists every state.
    const note = () => p2.evaluate(() => document.querySelector("[data-status]")?.title ?? null);
    // The lead back at its own level (the drag above left it at the fader's foot), so the checks below hear
    // the chord well above silence.
    const dragged = fs.readFileSync(`${dir}/song.ts`, "utf8");
    await putFile(s2, "song.ts", dragged, song0);
    await sleep(800);
    const playing = median(levels(frames() - RATE / 2, frames()));
    await p2.click('[data-testid="export"]');
    // The Export dialog (studio C3): Song is chosen and Export… has the focus.
    await p2.waitForSelector("[data-testid=export-dialog]", { timeout: 5000 });
    await p2.keyboard.press("Enter");
    check(await until(async () => (await api2("graph/status")).paused === "exporting", 5000), "an export of with-plugins pauses playback");
    // Sampled until it resumes, however long the export takes: each frame count read before the
    // status that says it still holds, so every one is from the pause.
    const heldFrom = frames();
    const seen = [];
    let noted = false;
    for (const t0 = Date.now(); Date.now() - t0 < 120_000; ) {
      const frame = frames();
      const status = await api2("graph/status");
      if (status.paused !== "exporting") break;
      seen.push({ frame, beat: status.beat });
      noted ||= ((await note()) ?? "").split(" · ").includes("Paused for export");
      await sleep(20);
    }
    check(noted, "the top bar's status pill says Paused for export");
    // In 25 ms windows, from 50 ms in (the 5 ms fade out, and the hold's callback) to 50 ms short of
    // the last frame seen holding, so a short pause is judged too.
    const held = levels(heldFrom + 2400, (seen.at(-1)?.frame ?? heldFrom) - 2400, 1200);
    const pausedFor = seen.length ? ((seen.at(-1).frame - heldFrom) / RATE).toFixed(2) : "0";
    check(
      held.length >= 2 && playing > -45 && Math.max(...held) < Math.min(-100, playing - 20),
      `silent while paused (${fmt(held)} dB over ${pausedFor} s; ${playing.toFixed(1)} dB playing)`,
    );
    const beatHeld = seen[0]?.beat ?? NaN;
    check(seen.length >= 2 && seen.every((x) => x.beat === beatHeld), `the playhead holds still while paused (beat ${beatHeld.toFixed(2)}, ${seen.length} samples)`);
    check(await until(async () => (await api2("graph/status")).paused == null, 1000), "playback resumes once the file is written");
    check(await until(() => fs.existsSync(exports) && fs.readdirSync(exports).some((f) => f.endsWith(".wav")), 5000), "the export is written");
    check(await until(async () => (await note()) === null, 3000), "the top bar no longer says paused");
    const resumed = await api2("graph/status");
    check(resumed.playing && resumed.beat >= beatHeld, `it plays on from where it paused (beat ${beatHeld.toFixed(2)}, then ${resumed.beat.toFixed(2)})`);
    await api2("graph/seek", { method: "POST", body: JSON.stringify({ beat: 0 }) });
    await sleep(1200);
    const again = median(levels(frames() - RATE / 2, frames()));
    check(again > playing - 6, `from the start, the chord sounds again after the export (${again.toFixed(1)} dB; ${playing.toFixed(1)} before it)`);

    await p2.close();
  } finally {
    server.kill();
  }
}
