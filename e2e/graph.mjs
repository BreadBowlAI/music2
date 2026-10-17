// End-to-end check of the graph canvas against an isolated test instance of the app.
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const S = process.argv[2];
const proj = `${S}/proj`;
const sess = JSON.parse(fs.readFileSync(`${S}/testhome/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
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
const until = async (f, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph]", { timeout: 8000 });

// Every module has a header to select it (the module tabs' part, taste.md §3.7), the Song's
// pinned at the foot; the dock shows the selected one only.
const tabs = await page.$$eval("[data-module-tab]", (els) => els.map((e) => e.dataset.moduleTab));
// In the mixer's order, as the song mixes them, never alphabetical (D8, taste.md §3.4).
check(JSON.stringify(tabs) === JSON.stringify(["kick", "hats", "bass", "keys", "song"]), `module headers, by key: ${tabs}`);
// The name, not the whole header: its M and S buttons have letters too.
const named = await page.$$eval("[data-module-tab] [data-testid=track-label]", (els) => els.map((e) => e.textContent));
check(JSON.stringify(named) === JSON.stringify(["Kick", "Hats", "Bass", "Keys", "Song"]), `named by label: ${named}`);
const show = async (name) => {
  await page.click(`[data-module-tab="${name}"]`);
  await page.waitForSelector(`[data-module="${name}"]`);
  const shown = await page.$$eval("[data-module]", (els) => els.map((e) => e.dataset.module));
  if (shown.length !== 1) throw new Error(`more than one module shown: ${shown}`);
};
// Scroll a knob into the panel's view before dragging it.
const knobBox = async (sel) => {
  await page.$eval(`${sel} svg`, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
  return (await page.$(`${sel} svg`)).boundingBox();
};
await show("bass");
const bassNodes = await page.$$eval('[data-module="bass"] [data-node]', (els) => els.map((e) => e.dataset.node));
check(["lfo", "xrange", "blend", "bass", "notes", "cutoff"].every((l) => bassNodes.includes(l)), `Bass nodes: ${bassNodes.join(", ")}`);
const edges = await page.$$eval('[data-module="bass"] svg.absolute > path', (els) => els.length);
check(edges === 8, `Bass edges drawn: ${edges}`);

// Levels and pans are the Mix view's strips (MX-6).
await page.click('[data-view-tab="mix"]');
await page.waitForSelector('[data-testid=mix] [data-knob="hats.pan"]');
const panKnob = '[data-testid=mix] [data-knob="hats.pan"]';
check((await page.$eval(panKnob, (e) => e.dataset.value)) === "0.3", "hats.pan knob reads 0.3");

// Drag the knob up 60px: on a -1..1 range that is 0.3 → 0.7 (300px is a knob's whole range, taste.md KN-1).
const box = await knobBox(panKnob);
const cx = box.x + box.width / 2, cy = box.y + box.height / 2;
await page.mouse.move(cx, cy);
await page.mouse.down();
for (let i = 1; i <= 8; i++) await page.mouse.move(cx, cy - i * 7.5);
await page.mouse.up();
const songTs = () => fs.readFileSync(`${proj}/song.ts`, "utf8");
check(await until(() => songTs().includes("pan: 0.7 }")), "dragging wrote `pan: 0.7` into song.ts");
check(songTs() === fs.readFileSync(`${S}/orig-song.ts`, "utf8").replace("pan: 0.3 }", "pan: 0.7 }"), "and changed nothing else");
// The knob shows the edit; a knob change makes no toast (taste.md §2.14).
check(await until(async () => (await page.$eval(panKnob, (e) => e.dataset.value)) === "0.7") && (await page.$$("[data-testid=toast]")).length === 0, "the knob shows the edit, with no toast");
check(await until(async () => (await api("graph")).view.revision === 2), "the revision is now 2");

// A fast drag (one move, immediate release) commits exactly the value shown at release.
const keysKnob = '[data-testid=mix] [data-knob="keys.pan"]';
const kb = await knobBox(keysKnob);
await page.mouse.move(kb.x + kb.width / 2, kb.y + kb.height / 2);
await page.mouse.down();
await page.mouse.move(kb.x + kb.width / 2, kb.y + kb.height / 2 - 23);
await page.mouse.up();
const shown = await page.$eval(keysKnob, (e) => e.dataset.value);
check(await until(() => songTs().includes(`pan: ${shown} }`)), `a fast drag commits the value it shows (${shown})`);

// An edit made elsewhere shows up on the canvas.
await page.click('[data-view-tab="graph"]');
await show("bass");
fs.writeFileSync(`${proj}/tracks/bass.ts`, fs.readFileSync(`${proj}/tracks/bass.ts`, "utf8").replace("khz(2.5)", "khz(3)"));
const hi = '[data-module="bass"] [data-node="xrange"] [data-knob="hi"]';
check(await until(async () => (await page.$eval(hi, (e) => e.dataset.value)) === "3000"), "an outside edit to bass.ts appears (xrange hi = 3000)");

// A broken save keeps the last good graph and says why; fixing it clears that.
await show("song");
const good = songTs();
fs.writeFileSync(`${proj}/song.ts`, good.replace("master(", "master(("));
// The status pill says so (taste.md ST-1); its popover holds the banner's words and the way to the code.
check(await until(async () => (await page.$eval("[data-status]", (e) => e.textContent).catch(() => "")).includes("Code doesn't compile")), "a broken save shows the status pill");
check((await page.$$('[data-module="song"] [data-node]')).length > 0, "and the last good graph stays on screen");
await page.click("[data-status]");
await page.waitForSelector("[data-testid=compile-banner]");
const bannerText = await page.$eval("[data-testid=compile-banner]", (e) => e.innerText).catch(() => "");
check(/song\.ts:\d+/.test(bannerText) && !bannerText.includes(proj), `naming the place relative to the project (${bannerText.replace(/\n/g, " | ")})`);
await page.click("[data-testid=compile-banner] button");
check(
  await until(async () => (await page.$eval('[data-view-tab="code"]', (e) => e.getAttribute("aria-selected"))) === "true" &&
    (await page.$eval("[data-testid=code] [data-file][aria-current=true]", (e) => e.dataset.file).catch(() => null)) === "song.ts"),
  "Show in code opens song.ts in the code view",
);
await page.click('[data-view-tab="graph"]');
await page.waitForSelector('[data-module="song"]');
fs.writeFileSync(`${proj}/song.ts`, good);
check(await until(async () => !(await page.$("[data-status]"))), "fixing it clears the pill");

// Transport.
await page.click('[data-testid=play]');
await new Promise((r) => setTimeout(r, 700));
const st = await api("graph/status");
check(st.playing && st.seconds > 0.3, `playing (${st.seconds.toFixed(2)} s in)`);
await page.click('[data-testid=play]');
check(await until(async () => !(await api("graph/status")).playing), "stopped");

await page.screenshot({ path: `${S}/graph.png` });
check(consoleErrors.length === 0, `no console errors ${consoleErrors.length ? JSON.stringify(consoleErrors) : ""}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
