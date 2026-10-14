// The project layout: the timeline on top, one module's graph in the panel below.
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
const api = async (path) => (await fetch(`${sess.url}/api/${path}`, { headers: { "x-m2-token": sess.token } })).json();

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const shown = () => page.$$eval("[data-module]", (els) => els.map((e) => e.dataset.module));
const selectedTab = () => page.$$eval('[data-module-tab][aria-selected="true"]', (els) => els.map((e) => e.dataset.moduleTab));
const highlighted = () => page.$$eval("[data-track][data-selected]", (els) => els.map((e) => e.dataset.track));
const playheadBeat = async () => Number(await page.$eval("[data-testid=playhead]", (e) => e.dataset.beat));
const center = async (sel) => { const r = await (await page.$(sel)).boundingBox(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; };

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.evaluate(() => { try { localStorage.removeItem("m2.graph.panel"); } catch {} });
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=timeline] [data-track]", { timeout: 8000 });
await page.waitForSelector("[data-testid=graph] [data-module]");

// The timeline: every track, its lanes in order, and as many clips as the server says; the
// Song's own row is pinned at the foot (taste.md §3.1).
const { timeline } = await api("graph");
const tracks = await page.$$eval("[data-testid=timeline] [data-track]", (els) => els.map((e) => e.dataset.track));
check(JSON.stringify(tracks) === JSON.stringify(["bass", "hats", "keys", "kick", "song"]), `timeline tracks, then the Song: ${tracks}`);
check(JSON.stringify(timeline.tracks.map((t) => t.module).filter((m) => m !== "song")) === JSON.stringify(tracks.slice(0, -1)), "in the server's order");
// Automation folds under its track, folded at first (taste.md §3.4): unfold the bass's.
const bassFolded = await page.$$eval('[data-track="bass"] [data-lane]', (els) => els.map((e) => e.dataset.lane));
await page.click('[data-fold="bass"]');
await page.waitForSelector('[data-lane="bass.cutoff"]');
const bassLanes = await page.$$eval('[data-track="bass"] [data-lane]', (els) => els.map((e) => e.dataset.lane));
// The track's row draws its notes; its automation follows in the order the code declares it
// (bass.ts declares cutoff and drop before notes).
const serverLanes = timeline.tracks.find((t) => t.module === "bass").lanes.map((l) => `bass.${l.key}`);
check(JSON.stringify(bassFolded) === JSON.stringify(["bass.notes"]) && JSON.stringify([...bassLanes].sort()) === JSON.stringify(["bass.cutoff", "bass.drop", "bass.notes"]), `Bass has notes, and cutoff and drop folded under them: ${bassFolded} → ${bassLanes}`);
check(JSON.stringify(bassLanes) === JSON.stringify(["bass.notes", ...serverLanes.filter((l) => l !== "bass.notes")]), `the notes in the track's row, its automation in the server's (declaration) order: ${serverLanes}`);
const counts = [];
let allMatch = true, total = 0;
for (const t of timeline.tracks)
  for (const lane of t.lanes) {
    const drawn = await page.$$eval(`[data-lane="${t.module}.${lane.key}"] [data-clip]`, (els) => els.length);
    counts.push(`${t.module}.${lane.key}=${drawn}/${lane.clips.length}`);
    allMatch &&= drawn === lane.clips.length;
    total += lane.clips.length;
  }
check(allMatch && total > 0, `clips per lane match GET /api/graph (${counts.join(", ")})`);
check(!!(await page.$('[data-lane="bass.notes"] [data-clip] rect')) && !!(await page.$('[data-lane="bass.cutoff"] [data-clip] polyline')), "notes and the cutoff curve are drawn");

// One module in the panel, the first track by default.
check(JSON.stringify(await shown()) === JSON.stringify(["bass"]), `the panel shows only Bass at first: ${await shown()}`);
check(JSON.stringify(await selectedTab()) === JSON.stringify(["bass"]) && JSON.stringify(await highlighted()) === JSON.stringify(["bass"]), "Bass's tab and rows are selected");

// Timeline → panel: clicking Keys' header selects it.
await page.click('[data-track="keys"] [data-lane] > div:first-child');
check(await until(async () => JSON.stringify(await shown()) === JSON.stringify(["keys"])), "clicking Keys in the timeline shows Keys' graph");
check(JSON.stringify(await selectedTab()) === JSON.stringify(["keys"]) && JSON.stringify(await highlighted()) === JSON.stringify(["keys"]), "and selects its tab and rows");
// Panel → timeline.
await page.click('[data-module-tab="hats"]');
check(await until(async () => JSON.stringify(await highlighted()) === JSON.stringify(["hats"])), "the Hats tab highlights Hats in the timeline");
check(JSON.stringify(await shown()) === JSON.stringify(["hats"]), "and shows Hats' graph");
await page.click('[data-module-tab="song"]');
check(await until(async () => JSON.stringify(await shown()) === JSON.stringify(["song"])) && JSON.stringify(await highlighted()) === JSON.stringify(["song"]), "the Song's row shows the song graph, and only it is highlighted");
await page.click('[data-module-tab="bass"]');
await page.waitForSelector('[data-module="bass"]');

// The split: dragging it resizes the panel, within limits, and the height is kept.
const panelH = () => page.$eval("[data-testid=graph]", (e) => e.parentElement.getBoundingClientRect().height);
const timelineH = () => page.$eval("[data-testid=timeline]", (e) => e.getBoundingClientRect().height);
const drag = async (dy) => {
  const s = await center("[data-testid=split]");
  await page.mouse.move(s.x, s.y);
  await page.mouse.down();
  await page.mouse.move(s.x, s.y + dy, { steps: 6 });
  await page.mouse.up();
};
const h0 = await panelH(), t0 = await timelineH();
await drag(-150);
const h1 = await panelH(), t1 = await timelineH();
check(Math.abs(h1 - h0 - 150) < 2 && Math.abs(t0 - t1 - 150) < 2, `dragging the split up 150px grows the panel (${h0} → ${h1}) and shrinks the timeline (${t0} → ${t1})`);
await drag(2000);
check(Math.abs((await panelH()) - 180) < 1, `it stops at 180px (${await panelH()})`);
await drag(-200);
const kept = await panelH();
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]");
check(Math.abs((await panelH()) - kept) < 1, `the height survives a reload (${kept})`);

// The ruler seeks: a click at bar 5 is beat 16 in 4/4.
await page.$eval("[data-testid=timeline]", (e) => { e.scrollLeft = 0; });
const bar5 = await page.$$eval("[data-testid=ruler] span", (els) => { const r = els.find((e) => e.textContent === "5").getBoundingClientRect(); return { x: r.left, y: r.top }; });
// Labels sit a few pixels right of their bar line.
await page.mouse.click(bar5.x - 4, bar5.y + 4);
check(await until(async () => Math.abs((await api("graph/status")).beat - 16) < 1), `clicking the ruler at bar 5 seeks to beat 16 (status beat ${(await api("graph/status")).beat})`);
check(await until(async () => Math.abs((await playheadBeat()) - 16) < 1), `the playhead follows (data-beat ${await playheadBeat()})`);
check(await until(async () => (await page.$eval("[data-testid=position]", (e) => e.textContent)) === "5.1.1"), "the bar shows 5.1.1");
check((await api("graph/status")).playing === false, "seeking while stopped stays stopped");

// Playing moves the playhead on, on screen too.
await sleep(300);
const x0 = (await (await page.$("[data-testid=playhead]")).boundingBox()).x;
await page.click('button[title="Play / stop (Space)"]');
check(await until(async () => (await playheadBeat()) > 17), `playing advances the playhead (data-beat ${await playheadBeat()})`);
await sleep(300);
const x1 = (await (await page.$("[data-testid=playhead]")).boundingBox()).x;
check(x1 > x0 + 20, `and it moves right on screen (${x0.toFixed(0)} → ${x1.toFixed(0)}px)`);
await page.click('button[title="Play / stop (Space)"]');
check(await until(async () => !(await api("graph/status")).playing), "stopped");
await page.click('button[aria-label="Back to start"]');
check(await until(async () => (await playheadBeat()) === 0), "back to start puts the playhead at 0");

// A knob drag in the panel still writes the code, and the selection stays.
const bassTs = () => fs.readFileSync(`${proj}/tracks/bass.ts`, "utf8");
const before = bassTs();
const rev0 = (await api("graph")).view.revision;
const hi = '[data-module="bass"] [data-node="xrange"] [data-knob="hi"]';
await page.$eval(`${hi} svg`, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
const k = await center(`${hi} svg`);
await page.mouse.move(k.x, k.y);
await page.mouse.down();
for (let i = 1; i <= 8; i++) await page.mouse.move(k.x, k.y - i * 3);
await page.mouse.up();
check(await until(() => bassTs() !== before), "dragging Bass's xrange hi knob rewrites bass.ts");
const changed = bassTs().split("\n").filter((l, i) => l !== before.split("\n")[i]);
check(changed.length === 1 && changed[0].includes("const wobble") && !changed[0].includes("khz(2.5)"), `only that value changed: ${changed.join(" | ")}`);
check(await until(async () => (await api("graph")).view.revision > rev0), "a new revision is accepted");
check(JSON.stringify(await shown()) === JSON.stringify(["bass"]) && JSON.stringify(await highlighted()) === JSON.stringify(["bass"]), "Bass stays selected after the edit");

const post = async (path, body) =>
  (await fetch(`${sess.url}/api/${path}`, { method: "POST", headers: { "x-m2-token": sess.token, "content-type": "application/json" }, body: JSON.stringify(body) })).json();

// Playing on past the song's end (and past the drawn width): the ruler and rows run on under the playhead.
await page.$eval("[data-testid=timeline]", (e) => { e.scrollLeft = 0; });
const drawnBeats = (await page.$eval("[data-testid=ruler]", (e) => e.getBoundingClientRect().width)) / 24;
await post("graph/seek", { beat: Math.floor(drawnBeats) - 2 });
await page.click('button[title="Play / stop (Space)"]');
let inside = true;
const samples = [];
for (let i = 0; i < 8; i++) {
  await sleep(500);
  const m = await page.evaluate(() => {
    const tl = document.querySelector("[data-testid=timeline]");
    const box = tl.getBoundingClientRect();
    const ruler = document.querySelector("[data-testid=ruler]").getBoundingClientRect();
    const ph = document.querySelector("[data-testid=playhead]");
    const header = document.querySelector('[data-track="bass"] [data-lane] > div:first-child').getBoundingClientRect();
    return { beat: Number(ph.dataset.beat), x: ph.getBoundingClientRect().left, rulerRight: ruler.right, right: box.right, header: header.left - box.left, overflow: tl.scrollWidth - (header.width + ruler.width) };
  });
  inside &&= m.x < m.rulerRight - 24 && m.x < m.right && Math.abs(m.header) < 1 && m.overflow <= 1;
  samples.push(`${m.beat.toFixed(1)}: x ${m.x.toFixed(0)}, ruler to ${m.rulerRight.toFixed(0)}`);
}
await page.click('button[title="Play / stop (Space)"]');
check((await playheadBeat()) > drawnBeats + 2 && inside, `past the drawn width (${drawnBeats.toFixed(1)} beats) the playhead stays over the ruler and rows, headers in place (${samples.join("; ")})`);
check(await until(async () => !(await api("graph/status")).playing), "stopped again");

// Notes that never play (before their clip, or of no length) are neither drawn nor counted, and don't stretch the clip's pitch range.
const notesFile = `${proj}/data/bass/notes.json`;
const rects = () => page.$$eval('[data-lane="bass.notes"] [data-clip]:nth-child(1 of [data-clip]) rect', (els) => els.map((e) => Number(e.getAttribute("height"))));
// Clips show no note count (taste.md §2.8): the clip's tooltip counts them.
const label = () => page.$eval('[data-lane="bass.notes"] [data-clip]', (e) => e.title.match(/(\d+) notes/)[1]);
const [r0, l0] = [await rects(), await label()];
const notesJson = JSON.parse(fs.readFileSync(notesFile, "utf8"));
const written = notesJson.clips[0].notes.length + 2;
notesJson.clips[0].notes.push({ pitch: 96, start: -3, length: 1 }, { pitch: 10, start: 2, length: 0 });
fs.writeFileSync(notesFile, JSON.stringify(notesJson));
const bassNotes = async () => (await api("graph")).timeline.tracks.find((t) => t.module === "bass").lanes.find((l) => l.key === "notes").clips[0].notes.length;
check(await until(async () => (await bassNotes()) === written, 8000), "the server shows the two silent notes as written");
await sleep(500);
const [r1, l1] = [await rects(), await label()];
check(r1.length === r0.length && Math.min(...r1) === Math.min(...r0), `the clip draws the same ${r0.length} notes at the same height (${Math.min(...r0).toFixed(2)} → ${Math.min(...r1).toFixed(2)})`);
check(l1 === l0, `and the lane still counts ${l0} notes (${l1})`);

// A knob released just before another module is selected still saves, even while the server is busy
// (a code save compiles while holding the project; since step 3a a seek no longer does, as it
// replays nothing).
const saved = bassTs();
const rev1 = (await api("graph")).view.revision;
const songTs = fs.readFileSync(`${proj}/song.ts`, "utf8");
const busy = fetch(`${sess.url}/api/graph/file`, {
  method: "PUT",
  headers: { "x-m2-token": sess.token, "content-type": "application/json" },
  body: JSON.stringify({ path: "song.ts", base: songTs, text: `${songTs}// saved while a knob moves\n` }),
}).then((r) => r.json());
await sleep(20);
const lo = '[data-module="bass"] [data-node="xrange"] [data-knob="lo"]';
await page.$eval(`${lo} svg`, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
const l = await center(`${lo} svg`);
await page.mouse.move(l.x, l.y);
await page.mouse.down();
for (let i = 1; i <= 8; i++) await page.mouse.move(l.x, l.y - i * 3);
await page.mouse.up();
await page.click('[data-module-tab="keys"]');
const switched = JSON.stringify(await shown()) === JSON.stringify(["keys"]);
const saveResult = await busy;
check(switched && !saveResult.error, `the panel switched to Keys while the save compiled (${saveResult.error ?? "saved"})`);
check(await until(() => bassTs() !== saved, 15000), "releasing Bass's xrange lo knob, then selecting Keys, still rewrites bass.ts");
const loLine = bassTs().split("\n").find((x) => x.includes("const wobble"));
check(!loLine.includes("hz(200)"), `with the new value: ${loLine.trim()}`);
check(await until(async () => (await api("graph")).view.revision > rev1), "as a new revision");

await page.screenshot({ path: `${S}/layout.png` });
check(errors.length === 0, `no console errors ${errors.join(" | ")}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
