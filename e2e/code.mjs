// The code editor: open it, autocomplete, edit and save with ⌘S, compile errors as markers,
// files changed outside it, and the files API's guards.
//   node code.mjs <scratchpad>   (the server's HOME and project: $M2_HOME / $M2_PROJ, else testhome / proj)
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const S = process.argv[2];
const home = process.env.M2_HOME ?? `${S}/testhome`;
const proj = process.env.M2_PROJ ?? `${S}/proj`;
const sess = JSON.parse(fs.readFileSync(`${home}/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1600, height: 1000 });
const errors = [];
// The API checks below get refusals on purpose; the browser logs those as failed loads.
page.on("console", (m) => m.type() === "error" && !/status of (400|409)/.test(m.text()) && errors.push(m.text()));
page.on("pageerror", (e) => { console.log("PAGEERROR", String(e), e.stack?.slice(0, 300)); errors.push(String(e)); });
// The dock as tall as the panel it replaced was at this size (45%): the editor shows the whole of bass.ts.
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; try { localStorage.setItem("m2.graph.panel", "450"); } catch {} }, sess.token);

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 6000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const bassFile = `${proj}/tracks/bass.ts`;
const disk = () => fs.readFileSync(bassFile, "utf8");
/** The editor's rendered lines (only those on screen), with Monaco's spaces made plain. */
const shown = () => page.$eval("[data-testid=code] .view-lines", (e) => e.innerText.replace(/ /g, " "));
const files = () => page.$$eval("[data-testid=code] [data-file]", (els) => els.map((e) => e.dataset.file));
const openFile = () => page.$eval("[data-testid=code] [data-file][aria-current=true]", (e) => e.dataset.file).catch(() => null);
const dirty = () => page.$$eval("[data-testid=code] [data-file][data-dirty]", (els) => els.map((e) => e.dataset.file));
const strip = () => page.$eval("[data-testid=problems]", (e) => ({ state: e.dataset.state, text: e.innerText }));
const api = (method, path, body) =>
  page.evaluate(async (method, path, body, token) => {
    const r = await fetch(`/api/graph${path}`, { method, headers: { "content-type": "application/json", "x-m2-token": token }, body: body && JSON.stringify(body) });
    return { status: r.status, data: await r.json().catch(() => null) };
  }, method, path, body, sess.token);
const key = async (combo) => {
  const keys = combo.split("+");
  for (const k of keys.slice(0, -1)) await page.keyboard.down(k);
  await page.keyboard.press(keys.at(-1));
  for (const k of keys.slice(0, -1).reverse()) await page.keyboard.up(k);
};
/** Select the first match of `text` with the editor's find widget. */
const find = async (text) => {
  await page.click("[data-testid=code] .view-lines");
  await key("Meta+f");
  await page.waitForSelector("[data-testid=code] .find-widget.visible");
  await page.keyboard.type(text);
  await sleep(150);
  await key("Escape");
  await sleep(100);
};

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 8000 });

// ---------------------------------------------------------------- the API's guards
const listed = await api("GET", "/files");
check(listed.status === 200 && listed.data.files.some((f) => f.path === "tracks/bass.ts"), "GET /files lists the project's TypeScript");
check(listed.data.modules.find((m) => m.key === "bass")?.file === "tracks/bass.ts", "and which file each module is in");
const types = await api("GET", "/types");
check(["@m2/core/index.ts", "std/dsp.ts", "std/fx.ts"].every((p) => types.data.files.some((f) => f.path === p)), "GET /types gives the SDK and std sources");
const original = disk();
const stale = await api("PUT", "/file", { path: "tracks/bass.ts", base: "not what it says", text: "// lost" });
check(stale.status === 409 && stale.data.current === original && disk() === original, "a save over a changed file is refused with 409 and what it says now");
for (const path of ["../song.ts", "/etc/hosts", "music2.toml", "tracks/../song.ts", "data/bass/notes.json"]) {
  const r = await api("PUT", "/file", { path, base: "", text: "x" });
  check(r.status === 400, `${path} can't be written (${r.status}: ${r.data?.error})`);
}

// ---------------------------------------------------------------- opening
await page.click('[data-view-tab="code"]');
check(await until(async () => !!(await page.$("[data-testid=code] .monaco-editor .view-lines"))), "the code view opens a Monaco editor");
// The first track is the mixer's first, the kick (D8).
check(await until(async () => (await openFile()) === "tracks/kick.ts"), `on the selected module's file, tracks/kick.ts (${await openFile()})`);
check(await until(async () => (await shown()).includes('track("kick"')), "showing its code");
check(JSON.stringify(await files()) === JSON.stringify(["song.ts", "tracks/bass.ts", "tracks/hats.ts", "tracks/keys.ts", "tracks/kick.ts"]), `song first, then the tracks: ${await files()}`);
check((await strip()).state === "ok", `the strip says it compiles: ${(await strip()).text}`);
await page.screenshot({ path: `${S}/code-open.png` });

// Selecting another module follows it to its file.
await page.click('[data-module-tab="keys"]');
check(await until(async () => (await openFile()) === "tracks/keys.ts" && (await shown()).includes("pluck(")), "selecting Keys opens tracks/keys.ts");
await page.click('[data-file="song.ts"]');
check(await until(async () => (await openFile()) === "song.ts" && (await shown()).includes("master(")), "the file list opens song.ts");
await page.click('[data-module-tab="bass"]');
check(await until(async () => (await openFile()) === "tracks/bass.ts"), "and Bass goes back to tracks/bass.ts");

// ---------------------------------------------------------------- autocomplete
await page.click("[data-testid=code] .view-lines");
await key("Meta+ArrowDown");
await page.keyboard.press("Enter");
await page.keyboard.type('import { osc } from "std/dsp";');
await key("Escape");
await page.keyboard.press("Enter");
await page.keyboard.type("osc.");
const suggested = await until(async () =>
  page.$eval("[data-testid=code] .suggest-widget", (e) => e.classList.contains("visible") && e.innerText).then((t) => t && /sine/.test(t) && /saw/.test(t)).catch(() => false),
);
check(suggested, "typing osc. suggests sine, saw, … from std/dsp");
await page.screenshot({ path: `${S}/code-suggest.png` });
await page.keyboard.type("sin");
await sleep(400);
const hover = await page.$eval("[data-testid=code] .suggest-widget", (e) => e.innerText).catch(() => "");
check(/sine/.test(hover), "narrowing to sine");
await key("Escape");
check(await until(async () => (await dirty()).includes("tracks/bass.ts")), "the typing marks bass.ts unsaved");
check((await page.$eval("[data-testid=code]", (e) => e.innerText)).includes("Edited"), "and the toolbar says Edited");
for (let i = 0; i < 20 && (await dirty()).length; i++) {
  await key("Meta+z");
  await sleep(60);
}
check((await dirty()).length === 0 && disk() === original, "undoing it all makes it clean again; nothing was written");

// ---------------------------------------------------------------- edit a number and save
await find("hz(200)");
await page.keyboard.type("hz(300)");
check(await until(async () => (await dirty()).includes("tracks/bass.ts")), "edited: unsaved");
const saveEnabled = await page.$eval('[data-testid=code] button[aria-label="Save"]', (e) => !e.disabled);
check(saveEnabled, "the Save button is enabled");
await key("Meta+s");
check(await until(() => disk().includes("hz(300)")), "⌘S writes the file");
check(disk() === original.replace("hz(200)", "hz(300)"), "with exactly that change");
check(await until(async () => (await dirty()).length === 0), "and it is clean again");
await page.click('[data-view-tab="graph"]');
check(await until(async () => !!(await page.$('[data-testid=graph] [data-module="bass"] [data-knob][data-value="300"]'))), "the graph's knob shows 300");
await page.click('[data-view-tab="code"]');
await sleep(300);

// ---------------------------------------------------------------- a change made elsewhere
// Clean: the buffer follows the disk in place.
const before = disk();
fs.writeFileSync(bassFile, before.replace("hz(500)", "hz(520)"));
check(await until(async () => (await shown()).includes("hz(520)")), "a change on disk shows in the clean buffer");
check((await dirty()).length === 0, "and it stays clean");
check(!(await page.$("[data-testid=code-changed]")), "no banner for a clean file");

// A knob turned in the graph: the clean buffer follows in place, the cursor stays put.
await find("return bass");
const cursorTop = () => page.$eval("[data-testid=code] .cursors-layer .cursor", (e) => e.style.top);
const top0 = await cursorTop();
const state = (await api("GET", "")).data;
const knob = state.view.modules.find((m) => m.key === "bass").nodes.flatMap((n) => n.knobs).find((k) => k.literal === "300");
const set = await api("POST", "/set", { session: state.session, revision: state.view.revision, id: knob.id, gesture: "e2e", value: 310, expect: 300 });
check(set.status === 200 && disk().includes("hz(310)"), `a knob set in the graph writes the file (${set.status})`);
check(await until(async () => (await shown()).includes("hz(310)")), "the clean buffer shows the knob's new value");
check((await cursorTop()) === top0 && (await dirty()).length === 0, `with the cursor where it was (${top0}), still clean`);

// Unsaved edits: they are kept, and a banner asks.
await find("period: 0.5");
await page.keyboard.type("period: 0.25");
await until(async () => (await dirty()).includes("tracks/bass.ts"));
fs.writeFileSync(bassFile, disk().replace("hz(520)", "hz(540)"));
check(await until(async () => !!(await page.$("[data-testid=code-changed]"))), "a change on disk while edited shows a banner");
check((await shown()).includes("period: 0.25") && !(await shown()).includes("hz(540)"), "and keeps the edits");
await page.screenshot({ path: `${S}/code-changed.png` });
const [keep] = await page.$$("xpath/.//button[contains(., 'Keep my edits')]");
await keep.click();
check(await until(async () => !(await page.$("[data-testid=code-changed]"))), "Keep my edits closes the banner");
await key("Meta+s");
check(await until(() => disk().includes("period: 0.25")), "and a save writes them");
check(disk().includes("hz(520)") && !disk().includes("hz(540)"), "over the other change, as chosen");

// ---------------------------------------------------------------- code that doesn't compile
const good = disk();
await page.click("[data-testid=code] .view-lines");
await key("Meta+ArrowDown");
await page.keyboard.press("Enter");
await page.keyboard.type("const broken = ;");
await key("Escape");
await key("Meta+s");
check(await until(() => disk().includes("const broken = ;")), "broken code is saved too");
check(await until(async () => (await strip()).state === "error"), "the strip says it doesn't compile");
const problem = await strip();
check(/doesn't compile/.test(problem.text) && /tracks\/bass\.ts:\d+/.test(problem.text), `naming the place: ${problem.text.replace(/\n/g, " | ")}`);
check(await until(async () => !!(await page.$("[data-testid=code] .squiggly-error"))), "a marker appears in the editor");
// The top bar's status pill says it once (taste.md ST-1); its popover holds what the banner said.
const pill = await page.$eval("[data-status]", (e) => e.textContent).catch(() => "");
check(/Code doesn't compile/.test(pill), `the status pill says the code doesn't compile (${pill})`);
await page.click("[data-status]");
await page.waitForSelector("[data-testid=compile-banner]");
const bannerText = await page.$eval("[data-testid=compile-banner]", (e) => e.innerText).catch(() => "");
check(/tracks\/bass\.ts:\d+/.test(bannerText) && !bannerText.includes(proj), `and names the place relative to the project (${bannerText.replace(/\n/g, " | ")})`);
await key("Escape");
await page.click("[data-testid=code] .view-lines");
check((await dirty()).length === 0, "and the file is saved (clean)");
await page.screenshot({ path: `${S}/code-error.png` });
const goodLines = good.split("\n").length;
const marker = await page.$eval("[data-testid=problems] button", (e) => e.innerText.replace(/\n/g, " "));
check(marker.includes(`tracks/bass.ts:${goodLines + 1}`), `on the broken line (${marker})`);

// Fix it: undo the line and save.
const lines = () => page.$$eval("[data-testid=code] .view-line", (els) => els.length);
for (let i = 0; i < 10 && ((await shown()).includes("broken") || (await lines()) > goodLines); i++) {
  await key("Meta+z");
  await sleep(60);
}
await key("Meta+s");
check(await until(() => disk() === good), "fixing it and saving writes the good code");
check(await until(async () => (await strip()).state === "ok"), "the strip says it compiles");
check(await until(async () => !(await page.$("[data-testid=code] .squiggly-error"))), "the marker is gone");
check(!(await page.$("[data-status]")), "and so is the status pill");

// ---------------------------------------------------------------- go to definition
await find("xrange(lfo");
await key("ArrowLeft");
await key("F12");
check(await until(async () => (await page.$eval("[data-testid=code]", (e) => e.innerText)).includes("Read only")), "go to definition opens std's source, read only");
check(await until(async () => (await shown()).includes("xrange")), "at xrange");
await page.screenshot({ path: `${S}/code-definition.png` });
await page.click('[data-testid=code] button[aria-label="Back"]');
check(await until(async () => (await openFile()) === "tracks/bass.ts"), "Back returns to bass.ts");

// Closing the project with unsaved code asks first, and keeps it open.
await page.click("[data-testid=code] .view-lines");
await key("Meta+ArrowDown");
await page.keyboard.type("\n// not saved yet");
check(await until(async () => (await dirty()).includes("tracks/bass.ts")), "an unsaved edit");
// The close button is gone from the top bar (taste.md TB-11): ⌘W closes the song.
await page.click("[data-testid=timeline]");
await key("Meta+w");
check(await until(async () => (await page.evaluate(() => document.body.innerText)).includes("unsaved edits in the code editor")), "closing the project says a file has unsaved edits");
await sleep(300);
check((await api("GET", "")).status === 200 && !!(await page.$("[data-testid=code]")), "and the project stays open");

check(errors.length === 0, `no console errors ${errors.join(" | ")}`);
await page.screenshot({ path: `${S}/code-final.png` });
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
