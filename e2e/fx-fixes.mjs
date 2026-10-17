// The review fixes, end to end: export progress stays with its song, a new export name is
// settled when it's done, stale part files are swept, "Exports" is reserved, nameless names,
// long names in the top bar, the engine's states in the status pill, an export's problems said plainly,
// the plugins example's message, the Empty template's steps and the Starter's bass knob.
// Started with no project and a scratch HOME.
// node run.mjs fx-fixes, without CLAP_PATH   (node fx-fixes.mjs <root> <home>)
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const [S, H] = process.argv.slice(2);
const songs = `${H}/Music/music2`;
const exportsDir = `${songs}/Exports`;
const sess = JSON.parse(fs.readFileSync(`${H}/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
import { REPO } from "./env.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 2 });
const errors = [];
page.on("console", (m) => m.type() === "error" && !/400 \(Bad Request\)/.test(m.text()) && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, sess.token);
const HDR = { "x-m2-token": sess.token };
const api = async (path, body) => {
  const r = await fetch(`${sess.url}/api/${path}`, body === undefined ? { headers: HDR } : { method: "POST", headers: { ...HDR, "content-type": "application/json" }, body: JSON.stringify(body) });
  return { status: r.status, json: await r.json().catch(() => null) };
};
let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 8000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 50)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const text = () => page.evaluate(() => document.body.innerText);
const exporting = async () => (await api("graph/export", { path: "relative.wav" })).json?.error?.includes("already being exported");
const idle = () => until(async () => !(await exporting()), 60000);

// A song that holds a chord for `minutes`: long enough to do things while it exports.
function longSong(dir, name, minutes) {
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(`${dir}/data/pad`, { recursive: true });
  fs.writeFileSync(
    `${dir}/music2.toml`,
    `[project]\nname = ${JSON.stringify(name)}\nentry = "song.ts"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n"@m2/core" = "1"\nstd = "0.1"\n`,
  );
  fs.writeFileSync(`${dir}/song.ts`, "import { song, track } from '@m2/core';\nimport { pad } from 'std/instruments';\nconst p = track('pad', { name: 'Pad' }, ({ lane }) => pad(lane.notes('chords')));\nexport default song(() => p.out);\n");
  const beats = minutes * 120;
  fs.writeFileSync(`${dir}/data/pad/chords.json`, JSON.stringify({ clips: [{ id: "chords1", start: 0, length: beats, notes: [45, 52, 57, 60, 64].map((pitch) => ({ pitch, start: 0, length: beats })) }] }));
  fs.writeFileSync(`${dir}/data/arrangement.json`, JSON.stringify({ tempo: [{ beat: 0, bpm: 120 }] }));
}

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=start]", { timeout: 8000 });

// ---------------------------------------------------------------- "Exports" is the songs folder's, never a song's
const ex = await api("project/new", { name: "Exports", template: "empty" });
check(ex.status === 200 && ex.json.name === "Exports 2", `a song called Exports becomes “${ex.json?.name}”`);
check(fs.existsSync(`${songs}/Exports 2/song.ts`) && !fs.existsSync(exportsDir), "in Exports 2; Exports itself isn't made");
const lower = await api("project/new", { name: "exports", template: "empty" });
// (“exports 2” is taken too: a song's folder name is taken in any capitals, on every OS.)
check(lower.json?.name === "exports 3" && !fs.existsSync(`${songs}/exports/song.ts`), `any case: “exports” becomes “${lower.json?.name}”`);
// A stale part from an export that was cut off, and one that isn't a render's.
fs.mkdirSync(exportsDir, { recursive: true });
fs.writeFileSync(`${exportsDir}/.Old song.wav.999999.part`, "left by a killed export");
fs.writeFileSync(`${exportsDir}/.notes.part`, "mine");
const e1 = await api("graph/export", {});
check(e1.json?.path === `${exportsDir}/exports 3.wav`, `the export goes to the real Exports folder: ${e1.json?.path}`);
check(await until(async () => (await text()).includes("Exported exports 3.wav")), "and finishes");
check(!fs.existsSync(`${exportsDir}/.Old song.wav.999999.part`), "the stale part file is swept");
check(fs.existsSync(`${exportsDir}/.notes.part`), "a file that isn't a render's part is left alone");
check(!fs.existsSync(`${songs}/exports 3/exports 3.wav`) && fs.existsSync(`${exportsDir}/exports 3.wav`), "it lands there, not in a song's folder");

// ---------------------------------------------------------------- names no folder can have
const dots = await api("project/new", { name: "..", template: "empty" });
check(dots.json?.name === "My song" && dots.json?.dir.endsWith("/My song"), `“..” is “${dots.json?.name}” in ${dots.json?.dir?.split("/").pop()}`);
const dots2 = await api("project/new", { name: ". . ", template: "empty" });
check(dots2.json?.name === "My song 2" && dots2.json?.dir.endsWith("/My song 2"), `“. . ” then is “${dots2.json?.name}” in ${dots2.json?.dir?.split("/").pop()}`);

// ---------------------------------------------------------------- the Empty template's steps work in the app
const empty = dots2.json.dir;
const code = fs.readFileSync(`${empty}/song.ts`, "utf8");
check(code.includes("delete the // in front of the two") && !code.includes("Make a file"), "song.ts says to delete two //s, not to make a file");
fs.writeFileSync(`${empty}/song.ts`, code.replace("// const drums", "const drums").replace("// drums: drums.out", "drums: drums.out"));
check(await until(async () => (await api("graph")).json?.timeline?.tracks?.some((t) => t.track && t.module === "drums")), "with those two lines on, a Drums track shows");
check(await until(async () => (await page.$$eval("[data-track]", (els) => els.map((e) => e.dataset.track))).includes("drums")), "in the timeline");
await sleep(400);
await page.screenshot({ path: `${S}/fx-empty-drums.png` });

// ---------------------------------------------------------------- the Starter's bass knob isn't "shared"
const starter = await api("project/new", { name: "Beat", template: "starter" });
check(starter.status === 200, "a Starter beat opens");
await page.waitForSelector("[data-testid=graph]");
await sleep(800);
check(!(await text()).includes("shared ×"), "no knob on its first screen says “shared ×2”");
check(!(await page.$("[data-shared]")), "and none is drawn as shared");
await page.screenshot({ path: `${S}/fx-starter.png` });

// ---------------------------------------------------------------- an export stays its own song's
const long = `${S}/long-one`;
longSong(long, "Long one", 20);
check((await api("graph/open", { path: long })).status === 200, "a 20-minute song opens");
await until(async () => (await text()).includes("Long one"));
await sleep(400);
await page.click("[data-testid=export]");
check(await until(async () => !!(await page.$("[data-testid=export-progress]"))), "its Export button shows progress");
// Someone else takes the name while it renders: the export gets the next one.
check(await until(() => fs.readdirSync(exportsDir).some((n) => n.startsWith(".Long one.wav."))), "it renders to a hidden part");
check(!fs.existsSync(`${exportsDir}/Long one.wav`), "and its name stays free meanwhile");
fs.writeFileSync(`${exportsDir}/Long one.wav`, "precious, made while rendering");

check((await api("graph/close", {})).status === 200, "the song is closed mid-export");
await page.waitForSelector("[data-testid=start]");
check(await until(async () => !!(await page.$("[data-testid=export-elsewhere]"))), "the start screen shows the export going on");
const pill = await page.$eval("[data-testid=export-elsewhere]", (e) => e.innerText.replace(/\s+/g, " ").trim());
check(/^Exporting “Long one” \d+%$/.test(pill), `naming its song: "${pill}"`);
await sleep(300);
await page.screenshot({ path: `${S}/fx-export-start.png` });

const other = await api("project/new", { name: "Other", template: "empty" });
check(other.json?.name === "Other", "another song is made and opens");
await page.waitForSelector("[data-testid=graph]");
await until(async () => (await page.$eval("[data-testid=bar-song-name]", (e) => e.textContent).catch(() => "")) === "Other");
const label = await page.$eval("[data-testid=export]", (e) => e.innerText.replace(/\s+/g, " ").trim());
check(label === "Export" && !(await page.$("[data-testid=export-progress]")), `its own bar says “${label}”, with no progress of the other song's`);
check(!!(await page.$("[data-testid=export-elsewhere]")), "the other song's export shows in the corner");
await page.screenshot({ path: `${S}/fx-export-other.png` });
await page.click("[data-testid=export]");
check(await until(async () => (await text()).includes("“Long one” is still being exported. Export this song when it's done.")), "exporting it meanwhile says which song is being exported");

// Reopened, the song's own bar shows its export again.
check((await api("graph/open", { path: long })).status === 200, "the long song is opened again");
await until(async () => (await page.$eval("[data-testid=bar-song-name]", (e) => e.textContent).catch(() => "")) === "Long one");
check(await until(async () => !!(await page.$("[data-testid=export-progress]"))), "its bar shows its export's progress");
check(!(await page.$("[data-testid=export-elsewhere]")), "and the corner doesn't");

check(await until(async () => (await text()).includes("Exported Long one 2.wav"), 90000), "it finishes as Long one 2.wav");
check(fs.readFileSync(`${exportsDir}/Long one.wav`, "utf8") === "precious, made while rendering", "the file made meanwhile is untouched");
check(fs.statSync(`${exportsDir}/Long one 2.wav`).size > 300e6, `Long one 2.wav holds the 20 minutes (${(fs.statSync(`${exportsDir}/Long one 2.wav`).size / 1e6).toFixed(0)} MB)`);
check(!fs.readdirSync(exportsDir).some((n) => n.startsWith(".Long one")), "and no part file is left");

// ---------------------------------------------------------------- a long name keeps clear of the top bar's buttons
const longName = "My very first summer song with friends and a much longer name than fits here";
const named = `${S}/long-name`;
longSong(named, longName, 6);
check((await api("graph/open", { path: named })).status === 200, "a song with a 77-character name opens");
const bar = async () =>
  page.evaluate(() => {
    const r = (el) => el.getBoundingClientRect();
    const name = document.querySelector("[data-testid=bar-song-name]");
    const box = name.parentElement;
    const time = document.querySelector("span[title=Time]");
    const exp = document.querySelector("[data-testid=export]");
    // The left side ends at the tempo and meter, after the time (which a small window drops).
    const tempo = document.querySelector("[data-testid=tempo]");
    return { box: [r(box).left, r(box).right, r(box).height], time: Math.max(r(time).right, r(tempo).right), exp: r(exp).left, cut: name.scrollWidth > name.clientWidth, title: name.title, lines: Math.round(r(name).height / 18) };
  });
// The window's smallest is 1100×700 (taste.md §3.2); from 1180px the time readout shows (§3.8).
// The Mac app's bar has 80px on its left for the window's buttons, which a page in a browser
// doesn't: each width is checked both ways, the Mac app's by laying the page out as the app's
// (the `data-app` the app sets on its root, which gives the bar that padding).
const macBar = (on) =>
  page.evaluate((on) => {
    if (on) document.documentElement.dataset.app = "mac";
    else delete document.documentElement.dataset.app;
  }, on);
for (const mac of [false, true]) {
  await macBar(mac);
  for (const [w, h] of [[1100, 700], [1180, 800], [1280, 800], [1440, 900]]) {
    await page.setViewport({ width: w, height: h, deviceScaleFactor: 2 });
    await sleep(300);
    const b = await bar();
    check(b.box[0] >= b.time + 4 && b.box[1] <= b.exp - 4 && b.lines === 1, `${w}px${mac ? " in the Mac app" : ""}: the name (${b.box[0].toFixed(0)}–${b.box[1].toFixed(0)}) clears the time and tempo (…${b.time.toFixed(0)}) and Export (${b.exp.toFixed(0)}…), on one line, cut short: ${b.cut}`);
  }
}
await macBar(false);
check((await bar()).title.startsWith(longName), "its tooltip has the whole name");

// RENDER-1, RENDER-25 (fit-4 of the step 7 review), taste ST-1: what the engine is busy with
// ("Paused for export", "Loading <plugins>…") is a neutral state of the top bar's one status pill,
// ranked below every problem. It stays on one line, clear of the name, at the app's smallest
// window (1100), at 1180 (where the time shows) and at 1280, in the browser's bar and the Mac
// app's. Without plugins here, the status the page reads is made to say so.
await page.evaluate(() => {
  const real = window.fetch.bind(window);
  window.__status = null;
  window.fetch = async (url, init) => {
    const r = await real(url, init);
    if (!window.__status || !String(url).includes("/api/graph/status")) return r;
    return new Response(JSON.stringify({ ...(await r.json()), ...window.__status }), { status: r.status, headers: r.headers });
  };
});
const statusPill = () =>
  page.evaluate(() => {
    const p = document.querySelector("[data-status]");
    if (!p) return null;
    const rect = (el) => { const b = el.getBoundingClientRect(); return { l: b.left, r: b.right, t: b.top, b: b.bottom }; };
    const name = document.querySelector("[data-testid=bar-song-name]").parentElement;
    return { ids: p.dataset.status.split(" "), text: p.textContent, title: p.title, pill: rect(p), name: rect(name), undo: rect(document.querySelector("button[aria-label=Undo]")) };
  });
const busy = [
  [{ paused: "exporting" }, "paused", "Paused for export"],
  [{ paused: null, loading: ["Surge XT", "Airwindows Consolidated"] }, "loading", "Loading Surge XT, Airwindows Consolidated…"],
];
for (const [status, id, said] of busy) {
  await page.evaluate((s) => { window.__status = s; }, status);
  for (const mac of [false, true]) {
    await macBar(mac);
    for (const w of [1100, 1180, 1280]) {
      await page.setViewport({ width: w, height: 800, deviceScaleFactor: 2 });
      const shown = await until(async () => (await statusPill())?.ids.includes(id), 3000);
      await sleep(100);
      const r = await statusPill();
      const shows = r && r.name.r > r.name.l;
      const clear = r && r.pill.b - r.pill.t <= 28 && r.pill.r <= r.undo.l && (!shows || r.name.r + 4 <= r.pill.l);
      check(
        shown && r.title === said && r.ids.join(" ") === id && r.text === said && clear,
        `${w}px${mac ? " in the Mac app" : ""}: the pill says “${r?.text}” (${r?.pill.l.toFixed(0)}–${r?.pill.r.toFixed(0)}), on one line, clear of the name (…${r?.name.r.toFixed(0)}) and Undo`,
      );
      await page.screenshot({ path: `${S}/fx-pill-${w}${mac ? "-mac" : ""}-${id}.png`, clip: { x: 0, y: 0, width: w, height: 44 } });
    }
  }
  await macBar(false);
  const p = await page.$("[data-status]");
  await p.click();
  const told = await until(async () => (await page.$eval("[data-testid=status-popover]", (e) => e.innerText).catch(() => "")).includes(id === "paused" ? "plays on from here" : "The song plays on meanwhile"), 3000);
  check(told, `its popover says what it means: ${(await page.$eval("[data-testid=status-popover]", (e) => e.innerText).catch(() => "")).replace(/\n+/g, " | ")}`);
  await page.keyboard.press("Escape");
}
// A problem shows over it: the code broken while paused.
await page.evaluate(() => { window.__status = { paused: "exporting" }; });
const goodSong = fs.readFileSync(`${named}/song.ts`, "utf8");
fs.writeFileSync(`${named}/song.ts`, goodSong.replace("export default", "export default )"));
const ranked = await until(async () => (await statusPill())?.ids.join(" ") === "compile paused", 8000);
const rp = await statusPill();
check(ranked && rp.text.startsWith("Code doesn't compile") && rp.text.endsWith("+1") && rp.title.includes("Paused for export"), `a problem shows over it, which counts it: “${rp?.text}”, “${rp?.title}”`);
fs.writeFileSync(`${named}/song.ts`, goodSong);
check(await until(async () => (await statusPill())?.ids.join(" ") === "paused", 8000), "and it is back once the code compiles");
await page.evaluate(() => { window.__status = null; });
check(await until(async () => !(await statusPill()), 3000), "the pill goes when the engine is done");
await page.setViewport({ width: 1100, height: 700, deviceScaleFactor: 2 });
await page.click("[data-testid=export]");
check(await until(async () => Number(await page.$eval("[data-testid=export-progress]", (e) => e.getAttribute("aria-valuenow")).catch(() => 0)) >= 10, 20000), "exporting it at the smallest window");
const b = await bar();
check(b.box[0] >= b.time + 4 && b.box[1] <= b.exp - 4, `the name still clears the wider “Exporting” button (${b.box[1].toFixed(0)} < ${b.exp.toFixed(0)})`);
await page.screenshot({ path: `${S}/fx-longname-1100-exporting.png` });
await page.screenshot({ path: `${S}/fx-longname-1100-bar.png`, clip: { x: 0, y: 0, width: 1100, height: 44 } });
check(await until(async () => (await text()).includes("Exported My very first summer song"), 60000), "and it finishes");
await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 2 });
await idle();

// ---------------------------------------------------------------- an export's problems are said plainly
// FMT-18a, RENDER-1 (fit-5 of the step 7 review): a song whose plugin isn't installed exports, and
// the done toast says plainly, after the file, that it plays without it: in that one toast, since
// a second would replace it (one at a time).
const nop = `${S}/no-plugin`;
fs.rmSync(nop, { recursive: true, force: true });
fs.mkdirSync(nop, { recursive: true });
fs.writeFileSync(`${nop}/music2.toml`, `[project]\nname = "No plugin"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n"@m2/core" = "1"\nstd = "0.1"\n`);
fs.writeFileSync(`${nop}/song.ts`, "import { hz, plugin, song } from '@m2/core';\nimport { osc } from 'std/dsp';\nexport default song(() => plugin('clap:com.example.not-installed', { in: osc.sine(hz(440)) }));\n");
const opened = await api("graph/open", { path: nop });
check(opened.status === 200 && (opened.json?.problems ?? []).some((p) => p.code === "plugin.missing"), "a song whose plugin isn't installed opens, saying so");
await until(async () => (await page.$eval("[data-testid=bar-song-name]", (e) => e.textContent).catch(() => "")) === "No plugin");
await sleep(300);
await page.click("[data-testid=export]");
const without = "A plugin isn't installed, so the export plays without it (the problems list says which).";
const toasts = () => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.innerText.replace(/\s+/g, " ").trim()));
check(await until(async () => { const t = await toasts(); return t.length === 1 && t[0].includes("Exported No plugin.wav") && t[0].includes(without); }, 30000), `its one export toast says it after the file: ${(await toasts()).join(" | ")}`);
await idle();

// ---------------------------------------------------------------- a song too long for a WAV file is refused plainly
const far = `${S}/far`;
fs.rmSync(far, { recursive: true, force: true });
fs.cpSync(`${REPO}/examples/first-song`, far, { recursive: true });
fs.writeFileSync(`${far}/data/kick/hits.json`, JSON.stringify({ clips: [{ start: 30000, length: 4, notes: [] }] }));
check((await api("graph/open", { path: far })).status === 200, "a song with a clip at beat 30000 opens");
await until(async () => (await page.$eval("[data-testid=bar-song-name]", (e) => e.textContent).catch(() => "")) === "first-song");
await sleep(300);
await page.click("[data-testid=export]");
const tooLong = "Couldn't export the song: its last clip ends in bar 7501, 4 h 2 min in, past the 3 hours music2 can export. Is a clip far past the rest?";
check(await until(async () => (await text()).includes(tooLong)), `exporting it says: ${tooLong}`);
const farToast = await page.evaluate(() => {
  const span = [...document.querySelectorAll("span")].find((s) => s.textContent.startsWith("Couldn't export the song"));
  return span ? span.scrollHeight > span.clientHeight + 1 : null;
});
check(farToast === false, "all of it fits the toast");
check(!(await page.$("[data-testid=export-progress]")) && !(await exporting()), "and nothing is exporting");
await page.screenshot({ path: `${S}/fx-too-long.png` });

// ---------------------------------------------------------------- the plugins example says plainly what's missing
check((await api("graph/close", {})).status === 200, "back to the start screen");
await page.waitForSelector("[data-testid=start]");
await sleep(300);
await page.click("[data-testid=open-example]");
await page.waitForSelector("[data-example=with-plugins]");
await page.click("[data-example=with-plugins]");
const plain = "Couldn't open With plugins: it needs the Airwindows Consolidated and Surge XT plugins, which aren't installed. Install them, then try again.";
check(await until(async () => (await text()).includes(plain)), `the toast says: ${plain}`);
const toastLines = await page.evaluate(() => {
  const span = [...document.querySelectorAll("span")].find((s) => s.textContent.startsWith("Couldn't open With plugins"));
  return span ? { cut: span.scrollHeight > span.clientHeight + 1, height: span.clientHeight } : null;
});
check(toastLines && !toastLines.cut, `all of it fits the toast (${toastLines?.height}px, not cut)`);
check(!fs.existsSync(`${songs}/With plugins`), "and nothing was copied");
await sleep(300);
await page.screenshot({ path: `${S}/fx-plugins.png` });

check(errors.length === 0, `no console errors${errors.length ? `: ${errors.join(" | ")}` : ""}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
