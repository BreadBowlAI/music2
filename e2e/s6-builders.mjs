// The first builders of step 6, in the app: rename a track's label in place (ID-10), automate a knob
// from its menu (HOME-2), and move a renamed track's whole folder from the Needs-attention strip
// (ID-17, DIAG-6). Each is one Tx, checked to change only what it should, and one undo step.
//
//   node e2e/s6-builders.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { exe } from "./chrome.mjs";

const [S, , proj = `${S}/proj`] = process.argv.slice(2);
const sess = JSON.parse(fs.readFileSync(`${S}/testhome/.music2/session.json`, "utf8"));
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1600, height: 1000 });
const consoleErrors = [];
page.on("console", (m) => m.type() === "error" && consoleErrors.push(m.text()));
page.on("pageerror", (e) => consoleErrors.push(String(e)));
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, sess.token);
const api = async (path) => (await fetch(`${sess.url}/api/${path}`, { headers: { "x-m2-token": sess.token } })).json();

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 6000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const read = (f) => (fs.existsSync(`${proj}/${f}`) ? fs.readFileSync(`${proj}/${f}`, "utf8") : null);
const write = (f, text) => fs.writeFileSync(`${proj}/${f}`, text);
const orig = { song: read("song.ts"), kick: read("tracks/kick.ts"), bass: read("tracks/bass.ts"), keys: read("tracks/keys.ts"), hits: read("data/kick/hits.json") };
const graph = () => api("graph");
const revision = async () => (await graph()).view.revision;
const toasts = () => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent));
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 4000);
/** Click the Undo of the toast that says `text`. */
const undoToast = (text) =>
  page.evaluate((text) => {
    const t = [...document.querySelectorAll("[data-testid=toast]")].find((e) => e.textContent.includes(text));
    const b = t && [...t.querySelectorAll("button")].find((b) => b.textContent === "Undo");
    b?.click();
    return !!b;
  }, text);
const key = async (k, mods = []) => {
  for (const m of mods) await page.keyboard.down(m);
  await page.keyboard.press(k);
  for (const m of mods.reverse()) await page.keyboard.up(m);
};
const show = async (name) => {
  await page.click(`[data-module-tab="${name}"]`);
  await page.waitForSelector(`[data-module="${name}"]`);
};
// What needs attention is in the status pill's popover (taste.md ST-1): open it to read it.
const status = async () => {
  if ((await page.$("[data-status]")) && !(await page.$("[data-testid=status-popover]"))) await page.click("[data-status]").catch(() => {});
};
const label = (module) => page.$eval(`[data-track="${module}"] [data-testid=track-label]`, (e) => e.textContent).catch(() => null);
/** A track's colour, as its row's bar draws it. */
const barColor = (module) => page.$eval(`[data-track="${module}"] .absolute.left-0.inset-y-0`, (e) => getComputedStyle(e).backgroundColor);

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph]", { timeout: 8000 });
await page.waitForSelector('[data-track="kick"] [data-testid=track-label]');

// ---------------------------------------------------------------- rename a label (ID-10)

const rev0 = await revision();
await page.click('[data-track="kick"] [data-testid=track-label]', { count: 2 });
const input = await page.waitForSelector("[data-testid=track-rename]", { timeout: 2000 }).catch(() => null);
check(!!input, "double-clicking a track's name in the timeline edits it in place");
check((await page.$eval("[data-testid=track-rename]", (e) => [e.value, e.selectionStart, e.selectionEnd].join())) === "Kick,0,4", "the old name, selected");
await page.keyboard.type("Nope");
await key("Escape");
check(await until(async () => !(await page.$("[data-testid=track-rename]"))), "Escape ends the edit");
check((await label("kick")) === "Kick" && read("tracks/kick.ts") === orig.kick && (await revision()) === rev0, "and keeps the old name: nothing written");

await page.click('[data-track="kick"] [data-testid=track-label]', { count: 2 });
await page.waitForSelector("[data-testid=track-rename]");
await page.keyboard.type("Kick Drum");
await page.screenshot({ path: `${S}/s6-renaming.png` });
await key("Enter");
check(await until(() => read("tracks/kick.ts") === orig.kick.replace('{ name: "Kick" }', '{ name: "Kick Drum" }')), "Enter writes the name into kick.ts, and nothing else");
check(await until(async () => (await label("kick")) === "Kick Drum"), "the timeline shows it");
check(await until(async () => (await page.$eval('[data-module-tab="kick"]', (e) => e.textContent)) === "Kick Drum"), "so does the module's tab");
// The name on screen says it: a rename makes no toast; its Undo is the top bar's (taste.md ST-5).
check(!(await toasts()).some((t) => t.includes("Renamed")) && (await page.$eval('button[aria-label="Undo"]', (b) => !b.disabled)), "no toast says so: the name does, and Undo is offered");
const t = (await graph()).timeline.tracks.find((t) => t.module === "kick");
check(t?.label === "Kick Drum" && t.lanes[0].clips.length > 0, "the key stays: its notes still play");

// In the dock's header too (the canvas's title row is gone, taste.md DK-3); back to the first name, the file is as it was.
await show("kick");
await page.click('[data-testid=dock] [data-testid=module-title]', { count: 2 });
const canvasInput = await page.waitForSelector("[data-testid=track-rename]", { timeout: 2000 }).catch(() => null);
check(!!canvasInput, "double-clicking the dock's title edits it too");
await page.keyboard.type("Kick");
await key("Enter");
check(await until(() => read("tracks/kick.ts") === orig.kick), "renamed back: kick.ts is byte for byte as it was");
check(await until(async () => (await page.$eval('[data-testid=dock] [data-testid=module-title]', (e) => e.textContent)) === "Kick"), "the title shows it");

// Clicking away writes it too; each rename is one undo step.
await page.click('[data-track="hats"] [data-testid=track-label]', { count: 2 });
await page.waitForSelector("[data-testid=track-rename]");
await page.keyboard.type("Hi-hats");
await page.mouse.click(800, 850);
check(await until(() => read("tracks/hats.ts")?.includes('{ name: "Hi-hats" }')), "clicking away writes the name");
check(await until(async () => (await label("hats")) === "Hi-hats"), "and the timeline shows it");
await key("z", ["Meta"]);
check(await until(async () => (await label("hats")) === "Hats" && read("tracks/hats.ts").includes('{ name: "Hats" }')), "⌘Z takes the rename back");
await key("z", ["Meta"]);
check(await until(async () => (await label("kick")) === "Kick Drum"), "and the one before it, one step each");
await key("z", ["Meta", "Shift"]);
check(await until(async () => (await label("kick")) === "Kick" && read("tracks/kick.ts") === orig.kick), "⇧⌘Z redoes it");

// The song has no name to change.
await show("song");
await page.click('[data-testid=dock] [data-testid=module-title]', { count: 2 });
await sleep(300);
check(!(await page.$("[data-testid=track-rename]")), "the song's title isn't renamed");

// ---------------------------------------------------------------- automate a knob (HOME-2)

const bassColor = await barColor("bass");
const panKnob = '[data-module="song"] [data-knob="hats.pan"]';
await page.$eval(panKnob, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
const openMenu = async (sel) => {
  // The scroll that brought it into view lands first: the menu closes on a scroll under it.
  await sleep(150);
  const b = await (await page.$(sel)).boundingBox();
  await page.mouse.click(b.x + 30, b.y + b.height / 2, { button: "right" });
  return page.waitForSelector("[data-testid=knob-menu]", { timeout: 2000 }).catch(() => null);
};
check(!!(await openMenu(panKnob)), "right-clicking a knob opens its menu");
const item = (id) => page.$eval(`[data-testid=knob-menu] [data-menu-item="${id}"]`, (e) => ({ disabled: e.getAttribute("aria-disabled") === "true", detail: e.querySelector("[data-detail]").textContent })).catch(() => null);
const automateItem = await item("automate");
check(automateItem && !automateItem.disabled && automateItem.detail.includes("new lane"), `it offers Automate (${automateItem?.detail})`);
// A pan reads "C" at the centre (taste.md, Words and numbers).
check((await item("reset"))?.detail === "C", "and Reset to its default, C");
await page.screenshot({ path: `${S}/s6-menu.png` });
await key("Escape");
check(await until(async () => !(await page.$("[data-testid=knob-menu]"))), "Escape closes it");

const revA = await revision();
await openMenu(panKnob);
await page.click('[data-testid=knob-menu] [data-menu-item="automate"]');
const automated = orig.song.replace("song(() =>", "song(({ lane }) =>").replace("pan: 0.3", 'pan: lane.control("hats-pan", "bipolar", 0.3)');
check(await until(() => read("song.ts") === automated), "Automate writes the value as a lane's fallback, and the song takes its lanes");
check(read("data/song/hats-pan.json") === '{"unit": "bipolar", "clips": []}\n', "with an empty lane file beside the song's data");
// In words: never the lane's key (taste.md §6.22).
check(await toasted("Automating hats pan in a new lane"), "a toast says so, with Undo");
check(await until(async () => !!(await page.$('[data-lane="song.hats-pan"]'))), "the timeline shows the lane");
const lane = (await graph()).timeline.tracks.find((t) => t.module === "song")?.lanes.find((l) => l.key === "hats-pan");
check(lane?.kind === "control" && lane.default === 0.3 && lane.clips.length === 0 && lane.unit === "bipolar", "empty, drawing the fallback (0.3)");
check((await revision()) === revA + 1, "one revision");
check(await until(async () => !(await page.$(panKnob)) && !!(await page.$('[data-module="song"] [data-node="hats-pan"]'))), "on the canvas the knob is a lane now");
check((await barColor("bass")) === bassColor, "a lane on the song recolours no track");
await page.screenshot({ path: `${S}/s6-automated.png` });

check(await undoToast("Automating hats pan"), "the toast's Undo");
check(await until(() => read("song.ts") === orig.song && read("data/song/hats-pan.json") === null), "puts the literal back and takes the lane file away, in one step");
check(await until(async () => !!(await page.$(panKnob)) && !(await page.$('[data-lane="song.hats-pan"]'))), "and the knob is back");
await key("z", ["Meta", "Shift"]);
check(await until(() => read("song.ts") === automated), "redo automates it again");
await key("z", ["Meta"]);
check(await until(() => read("song.ts") === orig.song), "undone again");

// From the keyboard, in a track that takes its lanes already: only the value changes there.
await show("bass");
const hi = '[data-module="bass"] [data-node="xrange"] [data-knob="hi"]';
await page.$eval(hi, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
await openMenu(hi);
await key("Enter");
check(await until(() => read("tracks/bass.ts") === orig.bass.replace("khz(2.5)", 'lane.control("hi", "hz", khz(2.5))')), "Enter in the menu automates; ({ lane, time }) is left as it is");
check(await until(async () => !!(await page.$('[data-track="bass"] [data-lane="bass.hi"]'))), "the lane shows under its track");
await key("z", ["Meta"]);
check(await until(() => read("tracks/bass.ts") === orig.bass && read("data/bass/hi.json") === null), "undone");

// What can't be automated says why, in place of what it would do.
write("tracks/keys.ts", orig.keys.replace('export default track("keys", { name: "Keys" }, ({ lane }) => pluck(lane.notes("chords")));',
  'const play = (n: any) => pluck(n, { brightness: hz(4000) });\nexport default track("keys", { name: "Keys" }, ({ lane }) => play(lane.notes("chords")));').replace('import { track } from "@m2/core";', 'import { hz, track } from "@m2/core";'));
await show("keys");
const bright = '[data-module="keys"] [data-knob="brightness"]';
// The call's brightness is inherited (5000 Hz) until the edit is taken in.
const brightness = () => page.$eval(bright, (e) => e.dataset.value).catch(() => null);
check(await until(async () => (await brightness()) === "4000"), "an outside edit puts the knob in a helper outside the track's function");
await page.$eval(bright, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
await openMenu(bright);
const refused = await item("automate");
check(refused?.disabled && refused.detail.includes("isn't inside Keys's function"), `Automate is offered no more, saying why (${refused?.detail})`);
await page.click('[data-testid=knob-menu] [data-menu-item="automate"]');
await sleep(300);
check(read("tracks/keys.ts").includes("hz(4000)") && fs.readdirSync(`${proj}/data/keys`).join() === "chords.json", "and clicking it does nothing");
await key("Escape");
write("tracks/keys.ts", orig.keys);
check(await until(async () => (await brightness()) === "5000"), "the edit taken back");

// ---------------------------------------------------------------- a renamed track's folder (ID-17)

write("tracks/kick.ts", orig.kick.replace('track("kick"', 'track("kicks"'));
const row = '[data-testid=attention] [data-problem="data.unclaimed"][data-src="data/kick"]';
check(await until(async () => (await status(), !!(await page.$(row)))), "a track key renamed by hand: Needs attention names its folder");
const likely = await page.$eval(`${row} [data-fix="relink"][data-likely]`, (e) => ({ text: e.textContent, to: e.dataset.to })).catch(() => null);
check(likely?.text === "Move all to Kick (kicks)" && likely.to === "data/kicks", `its one-click fix moves the whole folder (${likely?.text} → ${likely?.to})`);
const notes = async (module) => (await graph()).timeline.tracks.find((t) => t.module === module)?.lanes[0]?.clips.length ?? -1;
check((await notes("kicks")) === 0, "the renamed track plays nothing yet");
await page.screenshot({ path: `${S}/s6-strip.png` });
const revF = await revision();
await page.click(`${row} [data-fix="relink"][data-likely]`);
check(await until(() => read("data/kicks/hits.json") === orig.hits && !fs.existsSync(`${proj}/data/kick`)), "Move all moves data/kick to data/kicks, byte for byte, as one rename");
check(await until(async () => !(await page.$(row)) && (await notes("kicks")) > 0), "the strip clears and the track plays its notes again");
check((await revision()) === revF + 1, "one revision");
check(await toasted("Moved data/kick to data/kicks."), "a toast says so, with Undo");
check(await undoToast("Moved data/kick"), "its Undo");
check(await until(() => read("data/kick/hits.json") === orig.hits && !fs.existsSync(`${proj}/data/kicks`)), "moves the folder back");
check(await until(async () => (await status(), !!(await page.$(row)))), "and the strip names it again");
write("tracks/kick.ts", orig.kick);
check(await until(async () => !(await page.$("[data-testid=attention]")) && (await notes("kick")) > 0), "the key put back by hand: nothing needs attention");

check(read("song.ts") === orig.song && read("tracks/bass.ts") === orig.bass && read("tracks/keys.ts") === orig.keys, "every code file is as it was");
check(consoleErrors.length === 0, `no console errors ${consoleErrors.length ? JSON.stringify(consoleErrors) : ""}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
