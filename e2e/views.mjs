// The panel's views: graph, code and a notes clip opened from the timeline.
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const S = process.argv[2];
const proj = `${S}/proj`;
const sess = JSON.parse(fs.readFileSync(`${S}/testhome/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1600, height: 1000 });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, sess.token);

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const json = (x) => JSON.stringify(x);
const selectedView = () => page.$$eval('[data-view-tab][aria-selected="true"]', (els) => els.map((e) => e.dataset.viewTab));
const notesEnabled = () => page.$eval('[data-view-tab="notes"]', (e) => !e.disabled);
const selectedTab = () => page.$$eval('[data-module-tab][aria-selected="true"]', (els) => els.map((e) => e.dataset.moduleTab));
const highlighted = () => page.$$eval("[data-track][data-selected]", (els) => els.map((e) => e.dataset.track));
const body = () => page.$$eval("[data-testid=graph], [data-testid=code], [data-testid=notes]", (els) =>
  els.filter((e) => e.offsetParent !== null).map((e) => e.dataset.testid));
const text = (sel) => page.$eval(sel, (e) => e.textContent);
const center = async (sel) => { const r = await (await page.$(sel)).boundingBox(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; };
const dblclick = async (sel) => { const c = await center(sel); await page.mouse.click(c.x, c.y, { count: 2 }); };
// Where the open clip is in its lane: its place, as the timeline draws it.
const opened = () => page.$$eval("[data-clip][data-open]", (els) => els.map((e) => `${e.closest("[data-lane]").dataset.lane}#${[...e.parentElement.querySelectorAll(":scope > [data-clip]")].indexOf(e)}`));

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 8000 });

// The switcher: three views, the graph first; notes only once a clip is open.
const views = await page.$$eval("[data-view-tab]", (els) => els.map((e) => e.dataset.viewTab));
check(json(views) === json(["graph", "code", "notes"]), `three views: ${views}`);
check(json(await selectedView()) === json(["graph"]) && json(await body()) === json(["graph"]), "the graph shows first");
check(!(await notesEnabled()), "notes is disabled with no clip open");

// Code follows the selected module.
await page.click('[data-view-tab="code"]');
check(await until(async () => json(await body()) === json(["code"])), "the code tab shows the code view, not the graph");
check(json(await selectedView()) === json(["code"]), "and is selected");
check((await text("[data-testid=code]")).includes("Bass"), "for the selected module (Bass)");
await page.click('[data-module-tab="keys"]');
check(await until(async () => (await text("[data-testid=code]")).includes("Keys")), "selecting Keys keeps the code view, now on Keys");
check(json(await selectedView()) === json(["code"]), "still the code view");
await page.click('[data-view-tab="graph"]');
check(await until(async () => json(await body()) === json(["graph"])) && !!(await page.$('[data-module="keys"]')), "back to Keys' graph");
check((await page.$$("[data-testid=code]")).length === 1, "the code view stays mounted, hidden");

// A single click on a clip selects its track only.
await page.click('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
check(await until(async () => json(await highlighted()) === json(["bass"])), "a click on Bass's clip selects Bass");
check(json(await selectedView()) === json(["graph"]) && !(await notesEnabled()), "and opens nothing");

// Double-clicking a notes clip opens it.
await page.click('[data-module-tab="hats"]');
await dblclick('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
check(await until(async () => json(await body()) === json(["notes"])), "double-clicking Bass's notes clip shows the notes view");
check(json(await selectedView()) === json(["notes"]) && (await notesEnabled()), "notes is enabled and selected");
check(json(await selectedTab()) === json(["bass"]) && json(await highlighted()) === json(["bass"]), "and Bass is selected");
check(json(await opened()) === json(["bass.notes#0"]), `the clip is outlined in the timeline (${await opened()})`);
check(/Bass.*notes.*bars 1–8/.test(await text("[data-testid=notes]")), `the view is for that clip: ${await text("[data-testid=notes]")}`);
await page.screenshot({ path: `${S}/views-notes.png` });

// Other views and back: the clip stays open.
await page.click('[data-view-tab="code"]');
check(await until(async () => json(await body()) === json(["code"])) && (await notesEnabled()), "the code view, with the clip still open");
await page.click('[data-view-tab="notes"]');
check(await until(async () => json(await body()) === json(["notes"])), "notes again");

// Selecting another module closes the clip: its notes view goes back to the graph.
await page.click('[data-module-tab="keys"]');
check(await until(async () => json(await body()) === json(["graph"])) && !!(await page.$('[data-module="keys"]')), "selecting Keys shows Keys' graph");
check(!(await notesEnabled()) && (await opened()).length === 0, "and closes the clip");

// So does a single click on another track while a clip is open.
await dblclick('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await until(async () => json(await body()) === json(["notes"]));
await page.click('[data-track="hats"] [data-lane] > div:first-child');
check(await until(async () => json(await body()) === json(["graph"])) && json(await highlighted()) === json(["hats"]), "clicking Hats in the timeline goes back to the graph, for Hats");

// Closing the clip.
await dblclick('[data-lane="keys.chords"] :nth-child(1 of [data-clip])');
check(await until(async () => json(await body()) === json(["notes"])) && json(await selectedTab()) === json(["keys"]), "a Keys clip opens on Keys");
await page.click('[data-testid=notes] button[aria-label="Close the clip"]');
check(await until(async () => json(await body()) === json(["graph"])) && !(await notesEnabled()), "closing it goes back to the graph");

// An edit that removes the open clip closes it.
await dblclick('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await until(async () => json(await body()) === json(["notes"]));
const notesFile = `${proj}/data/bass/notes.json`;
const lane = JSON.parse(fs.readFileSync(notesFile, "utf8"));
fs.writeFileSync(notesFile, JSON.stringify({ ...lane, clips: [] }));
check(await until(async () => json(await body()) === json(["graph"]), 8000), "removing the clip on disk closes it: the graph shows");
check(!(await notesEnabled()), "notes is disabled again");

await page.click('[data-view-tab="code"]');
await page.screenshot({ path: `${S}/views-code.png` });
await page.click('[data-view-tab="graph"]');
await page.screenshot({ path: `${S}/views-graph.png` });
check(errors.length === 0, `no console errors ${errors.join(" | ")}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
