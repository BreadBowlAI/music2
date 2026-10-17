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

// The dock's switcher (taste.md §3.5): four views, Device first. Its view is one choice for the
// window: a selection changes what it shows, never which view it is (§3.7).
const views = await page.$$eval("[data-view-tab]", (els) => els.map((e) => e.dataset.viewTab));
check(json(views) === json(["graph", "notes", "mix", "code"]), `four views, the Device (graph) first: ${views}`);
check(json(await selectedView()) === json(["graph"]) && json(await body()) === json(["graph"]), "the Device view shows first");
check(await notesEnabled(), "notes is never disabled");
// Where the switcher is: the same, whatever is selected and whichever view shows (principle 8).
const switcherX = () => page.$eval('[data-view-tab="graph"]', (e) => Math.round(e.getBoundingClientRect().left));
const x0 = await switcherX();

// Code follows the selected module.
await page.click('[data-view-tab="code"]');
check(await until(async () => json(await body()) === json(["code"])), "the code tab shows the code view, not the graph");
check(json(await selectedView()) === json(["code"]), "and is selected");
// A window starts on the top track, which is the mixer's first since 8b.1 (D8): Kick.
check(await until(async () => (await text("[data-testid=code]")).includes("kick.ts")), `for the selected module, the top track (Kick): selected ${await selectedTab()}`);
await page.click('[data-module-tab="keys"]');
check(await until(async () => (await text("[data-testid=code]")).includes("keys.ts")), "selecting Keys keeps the code view, now on Keys");
check(json(await selectedView()) === json(["code"]), "still the code view");
await page.click('[data-view-tab="graph"]');
check(await until(async () => json(await body()) === json(["graph"])) && !!(await page.$('[data-module="keys"]')), "back to Keys' graph");
check((await page.$$("[data-testid=code]")).length === 1, "the code view stays mounted, hidden");

// A single click on a clip selects it and its track; the view stays.
await page.click('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
check(await until(async () => json(await highlighted()) === json(["bass"])), "a click on Bass's clip selects Bass");
check(json(await selectedView()) === json(["graph"]) && !!(await page.$('[data-lane="bass.notes"] [data-clip][data-picked]')), "and the clip, with the view still Device");
// Notes then shows that clip: the selection decides what it shows.
await page.click('[data-view-tab="notes"]');
check(await until(async () => json(await body()) === json(["notes"])) && json(await opened()) === json(["bass.notes#0"]), "Notes shows the selected clip");

// Double-clicking a notes clip opens Notes on it, from any view.
await page.click('[data-view-tab="graph"]');
await page.click('[data-module-tab="hats"]');
await dblclick('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
check(await until(async () => json(await body()) === json(["notes"])), "double-clicking Bass's notes clip shows the notes view");
check(json(await selectedView()) === json(["notes"]), "notes is selected");
check(json(await selectedTab()) === json(["bass"]) && json(await highlighted()) === json(["bass"]), "and Bass is selected");
check(json(await opened()) === json(["bass.notes#0"]), `the clip is outlined in the timeline (${await opened()})`);
check(/Bass.*Notes.*bars 1–8/.test(await text("[data-testid=dock]")), `the dock is for that clip: ${(await text("[data-testid=dock]")).slice(0, 60)}`);
check((await switcherX()) === x0, `the switcher hasn't moved, with Hats picked before and a clip open now (x ${await switcherX()} vs ${x0})`);
await page.screenshot({ path: `${S}/views-notes.png` });

// Other views and back: the clip stays.
await page.click('[data-view-tab="code"]');
check(await until(async () => json(await body()) === json(["code"])), "the code view");
await page.click('[data-view-tab="notes"]');
check(await until(async () => json(await body()) === json(["notes"])) && json(await opened()) === json(["bass.notes#0"]), "notes again, on the same clip");

// Selecting another track keeps Notes: it shows that track's clip, or says how to pick one.
await page.click('[data-module-tab="keys"]');
check(await until(async () => json(await selectedView()) === json(["notes"]) && !!(await page.$("[data-testid=notes][data-empty]"))), "selecting Keys keeps the notes view, with nothing picked on Keys");
check((await text("[data-testid=notes]")).includes("Select a clip, or double-click in the timeline to make one."), "it says how to pick one");
await page.click('[data-track="hats"] [data-lane] > div:first-child');
check(await until(async () => json(await highlighted()) === json(["hats"])) && json(await selectedView()) === json(["notes"]), "clicking Hats in the timeline keeps Notes too, for Hats");
await page.click('[data-module-tab="bass"]');
check(await until(async () => json(await opened()) === json(["bass.notes#0"])), "and back on Bass, its clip is still the one shown");

// Escape lets the clip go: Notes says so, never blank.
await page.click("[data-testid=timeline]");
await page.keyboard.press("Escape");
check(await until(async () => !!(await page.$("[data-testid=notes][data-empty]")) && (await opened()).length === 0), "Escape lets the clip go: Notes shows its empty state");

// An edit that removes the shown clip lets it go too.
await dblclick('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await until(async () => json(await opened()) === json(["bass.notes#0"]));
const notesFile = `${proj}/data/bass/notes.json`;
const lane = JSON.parse(fs.readFileSync(notesFile, "utf8"));
fs.writeFileSync(notesFile, JSON.stringify({ ...lane, clips: [] }));
check(await until(async () => !!(await page.$("[data-testid=notes][data-empty]")), 8000), "removing the clip on disk lets it go: Notes shows its empty state");
check(json(await selectedView()) === json(["notes"]), "and the view stays Notes");

// Mix, until the mixer, says what it will hold and where levels are meanwhile.
await page.click('[data-view-tab="mix"]');
check(await until(async () => (await text("[data-testid=mix]")).includes("Song")), "Mix says where levels and pans are until the mixer");

await page.click('[data-view-tab="code"]');
await page.screenshot({ path: `${S}/views-code.png` });
await page.click('[data-view-tab="graph"]');
await page.screenshot({ path: `${S}/views-graph.png` });
check(errors.length === 0, `no console errors ${errors.join(" | ")}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
