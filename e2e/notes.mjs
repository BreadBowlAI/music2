// The piano roll and notes clips in the timeline: every edit is one lane write, checked on disk.
// Runs with `node run.mjs notes` (kind proj: a private HOME and project).
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const S = process.argv[2];
const H = process.env.M2_E2E_HOME ?? `${S}/testhome`;
const proj = process.env.M2_E2E_PROJ ?? `${S}/proj`;
const sess = JSON.parse(fs.readFileSync(`${H}/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1600, height: 1000 });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
// The dock as tall as the panel it replaced was at this size (45%), so the roll shows as many rows.
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; try { localStorage.setItem("m2.graph.panel", "450"); } catch {} }, sess.token);

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 6000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 80)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const json = (x) => JSON.stringify(x);

const FILE = `${proj}/data/bass/notes.json`;
const original = fs.readFileSync(FILE, "utf8");
const disk = () => JSON.parse(fs.readFileSync(FILE, "utf8"));
const NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const name = (p) => `${NAMES[p % 12]}${Math.floor(p / 12) - 1}`;
/** The notes on disk at `pitch` in clip `c`. */
const onDisk = (pitch, c = 0) => (disk().clips[c]?.notes ?? []).filter((n) => n.pitch === name(pitch));
const notes = () => page.$$eval("[data-testid=notes] [data-note]", (els) => els.map((e) => ({
  i: Number(e.dataset.note), pitch: Number(e.dataset.pitch), start: Number(e.dataset.start), length: Number(e.dataset.length),
  velocity: Number(e.dataset.velocity), selected: e.dataset.selected === "true",
})));
const at = async (pitch) => (await notes()).filter((n) => n.pitch === pitch);
const box = async (sel) => { const el = await page.$(sel); return el && el.boundingBox(); };
// The roll's place and tools are in the dock's header (taste.md §3.5, NT-4).
const header = () => page.$eval("[data-testid=dock-header]", (e) => e.textContent);
/** The open clip, by its lane and its place there (FMT-12: the timeline names it by id). */
const openClip = () => page.$$eval("[data-clip][data-open]", (els) => els.map((e) => `${e.closest("[data-lane]").dataset.lane}#${[...e.parentElement.querySelectorAll(":scope > [data-clip]")].indexOf(e)}`));
const ID = /^[0-9a-z]{8}$/;
const undo = async () => { await page.keyboard.down("Meta"); await page.keyboard.press("z"); await page.keyboard.up("Meta"); };
const revision = () => page.evaluate(async (token) => (await (await fetch("/api/graph", { headers: { "x-m2-token": token } })).json()).view.revision, sess.token);
const drag = async (from, to, steps = 12) => {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(to.x, to.y, { steps });
  await page.mouse.up();
};

// The roll's rows and its ruler (taste.md §3.5: 14px rows, the arrangement's 28px ruler).
const ROW = 14, RULER = 28;
await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 8000 });

// ---------------------------------------------------------------- the piano roll

// Opening the Bass clip: centred on its notes, 48 of them.
const clipBox = await box('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await page.mouse.click(clipBox.x + 60, clipBox.y + clipBox.height / 2, { count: 2 });
check(await until(async () => (await notes()).length === 48), "double-clicking Bass's notes clip opens the piano roll with its 48 notes");
// The chip, the switcher, then the clip's bars: one notes lane, so no "Notes" beside the Notes tab.
check(/^BassDeviceNotesMixCodebars 1–8/.test(await header()) && (await header()).includes("Length"), `the header names the clip, after the switcher: ${await header()}`);
const lengthInput = await page.$('[data-testid=dock] input[aria-label="Clip length in bars"]');
check((await lengthInput.evaluate((e) => e.value)) === "8", "its length is 8 bars");
const rows = await page.$$eval("[data-testid=notes] [data-key]", (els) => els.length);
const labels = await page.$$eval("[data-testid=notes] [data-key] span", (els) => els.map((e) => e.textContent));
check(rows === 128 && labels.includes("C2") && labels.includes("C4") && labels.every((l) => l.startsWith("C")), `a keyboard of 128 keys with C labels (${labels.slice(0, 3)}…)`);
const visiblePitches = await page.evaluate(() => {
  const s = document.querySelector("[data-testid=roll-scroller]").getBoundingClientRect();
  return [...document.querySelectorAll("[data-testid=notes] [data-note]")].filter((e) => { const r = e.getBoundingClientRect(); return r.top >= s.top && r.bottom <= s.bottom; }).length;
});
check(visiblePitches > 40, `it opens centred on the clip's notes (${visiblePitches} of 48 in view)`);
const snapping = () => page.$eval("[data-testid=grid]", (e) => e.dataset.grid).then(async (g) => g === "1/16" && (await page.$eval("[data-testid=snap]", (e) => e.getAttribute("aria-pressed"))) === "true");
check(await snapping(), "snapping to 1/16 by default");

// Bring middle C into view; the Bass clip has nothing there.
await page.$eval("[data-testid=roll-scroller]", (el, row, ruler) => { el.scrollTop = ruler + (127 - 60) * row - el.clientHeight / 2; }, ROW, RULER);
await sleep(100);
const grid = await box("[data-testid=roll-grid]");
const ppb = await page.evaluate(() => {
  const n = [...document.querySelectorAll("[data-testid=notes] [data-note]")].find((e) => Number(e.dataset.start) > 0);
  return (n.getBoundingClientRect().left - document.querySelector("[data-testid=roll-grid]").getBoundingClientRect().left) / Number(n.dataset.start);
});
const cell = (pitch, beat) => ({ x: grid.x + beat * ppb, y: grid.y + (127 - pitch) * ROW + ROW / 2 });

// Add: a click on empty space selects nothing (D4); a double-click adds a 1/16 note in that cell.
let rev = await revision();
const c4 = cell(60, 1.1);
await page.mouse.click(c4.x, c4.y);
await sleep(400);
check(onDisk(60).length === 0 && (await revision()) === rev, "a single click on empty grid adds nothing");
await page.mouse.click(c4.x, c4.y, { count: 2 });
check(await until(async () => json(onDisk(60)) === json([{ pitch: "C4", start: 1, length: 0.25, velocity: 0.8 }])), `a double-click adds C4 at beat 1, a 1/16 long, written as a note name, in the canonical form (FMT-8a): ${json(onDisk(60))}`);
check(await until(async () => (await at(60)).length === 1 && (await at(60))[0].selected), "the new note shows, selected");
check((await revision()) === rev + 1, "one write");

// Move: drag it two beats right and two semitones up, as one write.
rev = await revision();
let n = await box('[data-note][data-pitch="60"]');
await drag({ x: n.x + 3, y: n.y + n.height / 2 }, { x: n.x + 3 + 2 * ppb, y: n.y + n.height / 2 - 2 * ROW });
check(await until(async () => json(onDisk(62)) === json([{ pitch: "D4", start: 3, length: 0.25, velocity: 0.8 }]) && onDisk(60).length === 0), `dragging moves it to D4 at beat 3: ${json(onDisk(62))}`);
check((await until(async () => (await revision()) > rev)) && (await sleep(300), (await revision()) === rev + 1), "the whole drag is one write");

// Resize: drag its right edge half a beat to the right.
n = await box('[data-note][data-pitch="62"]');
await drag({ x: n.x + n.width - 2, y: n.y + n.height / 2 }, { x: n.x + n.width - 2 + 0.5 * ppb, y: n.y + n.height / 2 });
check(await until(async () => json(onDisk(62)) === json([{ pitch: "D4", start: 3, length: 0.75, velocity: 0.8 }])), `dragging its edge makes it 3/16 long: ${json(onDisk(62))}`);
// New notes take the grid's length, as in Live, not the last one resized (taste.md NT-4).
check(!(await page.$("[data-testid=draw-length]")) && (await snapping()), "new notes still take the grid's length");

// Velocity: drag its bar down to about half.
const vi = (await at(62))[0].i;
const bar = await box(`[data-testid=velocity] [data-velocity="${vi}"]`);
const strip = await box("[data-testid=velocity]");
await drag({ x: bar.x + bar.width / 2, y: strip.y + 12 }, { x: bar.x + bar.width / 2, y: strip.y + strip.height / 2 }, 6);
const vel = () => onDisk(62)[0]?.velocity;
check(await until(async () => vel() > 0.4 && vel() < 0.6), `dragging its velocity bar sets about half: ${vel()}`);
check(await until(async () => Math.abs((await at(62))[0].velocity - vel()) < 1e-9), "the note shows it");
const withVelocity = fs.readFileSync(FILE, "utf8");

// Delete: the file is back to the original, byte for byte.
await page.click('[data-note][data-pitch="62"]');
await page.keyboard.press("Delete");
check(await until(async () => fs.readFileSync(FILE, "utf8") === original), "Delete removes it: the file is byte for byte what it was");
check(await until(async () => (await notes()).length === 48), "48 notes again");

// Undo brings it back, exactly as it was.
await undo();
check(await until(async () => fs.readFileSync(FILE, "utf8") === withVelocity), "⌘Z undoes the delete: the file has the note again, velocity and all");
check(await until(async () => (await at(62)).length === 1), "and the roll shows it");
for (let k = 0; k < 4; k++) {
  await undo();
  await sleep(250);
}
check(await until(async () => fs.readFileSync(FILE, "utf8") === original), "four more undos (velocity, resize, move, add) restore the original file");
check(await until(async () => (await notes()).length === 48 && (await at(60)).length === 0), "and the roll");

// Select with a marquee, move with the arrows, duplicate with ⌘D.
await page.$eval("[data-testid=roll-scroller]", (el, row, ruler) => { el.scrollTop = ruler + (127 - 36) * row - el.clientHeight / 2; el.scrollLeft = 0; }, ROW, RULER);
await sleep(150);
const g2 = await box("[data-testid=roll-grid]");
const first = (await notes()).filter((x) => x.start < 2);
const lo = Math.min(...first.map((x) => x.pitch)), hi = Math.max(...first.map((x) => x.pitch));
await drag({ x: g2.x + 2, y: g2.y + (127 - hi) * ROW - 4 }, { x: g2.x + 2 * ppb - 2, y: g2.y + (128 - lo) * ROW + 4 });
const marqueed = (await notes()).filter((x) => x.selected);
check(json(marqueed.map((x) => x.i)) === json(first.map((x) => x.i)), `a marquee over the first two beats selects their ${first.length} notes`);
check(fs.readFileSync(FILE, "utf8") === original, "selecting writes nothing");
await page.keyboard.press("ArrowUp");
check(await until(async () => disk().clips[0].notes.filter((x, i) => i < first.length).every((x, i) => x.pitch === name(first[i].pitch + 1))), "↑ moves them up a semitone, on disk");
await page.keyboard.down("Meta"); await page.keyboard.press("d"); await page.keyboard.up("Meta");
check(await until(async () => disk().clips[0].notes.length === 48 + first.length), `⌘D duplicates them (${disk().clips[0].notes.length} notes)`);
check(await until(async () => (await notes()).filter((x) => x.selected).every((x) => x.start >= 2)), "the copies are selected, right after the originals");
await undo(); await sleep(250); await undo();
// The page too, so the next edit is made from the revision the undos wrote, not one it refuses.
check(await until(async () => fs.readFileSync(FILE, "utf8") === original && json((await notes()).filter((x) => x.start < 2).map((x) => x.pitch)) === json(first.map((x) => x.pitch))), "two undos put them back");

// The clip's length.
await lengthInput.click({ clickCount: 3 });
await page.keyboard.type("4");
await page.keyboard.press("Enter");
check(await until(async () => disk().clips[0].length === 16), "typing 4 bars makes the clip 16 beats long");
check(await until(async () => (await page.$eval("[data-testid=timeline]", (e) => !!e)) && (await box('[data-lane="bass.notes"] :nth-child(1 of [data-clip])')).width < clipBox.width * 0.6), "the timeline shows it shorter");
await undo();
check(await until(async () => fs.readFileSync(FILE, "utf8") === original), "undo restores the length");
await page.screenshot({ path: `${S}/notes-roll.png` });

// ---------------------------------------------------------------- clips in the timeline

// A double-click on an empty spot makes a one-bar clip there and opens it.
const lane = await box('[data-lane="bass.notes"] > div:last-child');
const PPB = 24;
await page.mouse.click(lane.x + (9 * 4 + 1.5) * PPB, lane.y + lane.height / 2, { count: 2 });
check(await until(async () => json({ ...disk().clips[1], id: undefined }) === json({ start: 36, length: 4, notes: [] })), `double-clicking bar 10 of the empty lane makes a one-bar clip there: ${json(disk().clips[1])}`);
const madeId = disk().clips[1]?.id;
check(ID.test(madeId) && madeId !== disk().clips[0].id, `with an id of its own (FMT-12): ${madeId}`);
check(await until(async () => /bar 10/.test(await header()) && (await notes()).length === 0), "and opens it in the piano roll");
check(await until(async () => json(await openClip()) === json(["bass.notes#1"])), "outlined as the open clip");

// A note in the new clip.
const g3 = await box("[data-testid=roll-grid]");
const visible = await page.$eval("[data-testid=roll-scroller]", (el) => ({ top: el.scrollTop, h: el.clientHeight }));
const pitch = 127 - Math.floor((visible.top + visible.h / 2) / ROW);
const ppb3 = await page.evaluate(() => document.querySelector("[data-testid=roll-grid]").getBoundingClientRect().width);
await page.mouse.click(g3.x + 5, g3.y + (127 - pitch) * ROW + ROW / 2, { count: 2 });
check(await until(async () => disk().clips[1].notes.length === 1 && disk().clips[1].notes[0].start === 0), `a double-click adds a note to the new clip (${json(disk().clips[1].notes)})`);

// Drag the clip two bars later.
let made = await box('[data-lane="bass.notes"] :nth-child(2 of [data-clip])');
await drag({ x: made.x + 20, y: made.y + made.height / 2 }, { x: made.x + 20 + 8 * PPB, y: made.y + made.height / 2 }, 10);
check(await until(async () => disk().clips[1]?.start === 44 && disk().clips[1].notes.length === 1), `dragging it two bars right moves it to beat 44: ${json(disk().clips.map((c) => c.start))}`);
check(disk().clips[1]?.id === madeId, "keeping its id");
check(await until(async () => json(await openClip()) === json(["bass.notes#1"]) && /bar 12/.test(await header())), "it stays open in the piano roll");

// A drag before the first clip reorders the lane. Dragging a clip selects it, and Notes shows what is
// selected (taste.md §3.7): it follows that clip by its id, not its place in the lane.
made = await box('[data-lane="bass.notes"] :nth-child(2 of [data-clip])');
const firstClip = await box('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await drag({ x: firstClip.x + 30, y: firstClip.y + firstClip.height / 2 }, { x: firstClip.x + 30 + 48 * PPB, y: firstClip.y + firstClip.height / 2 }, 12);
check(await until(async () => json(disk().clips.map((c) => c.start)) === json([44, 48])), `moving the first clip past it reorders the lane: ${json(disk().clips.map((c) => c.start))}`);
check(await until(async () => /bars 13–20/.test(await header()) && (await notes()).length === 48 && json(await openClip()) === json(["bass.notes#1"])), "Notes shows the clip dragged, now second in the lane");
await undo();
check(await until(async () => json(disk().clips.map((c) => c.start)) === json([0, 44])), "undo moves it back");
check(await until(async () => /bars 1–8/.test(await header()) && (await notes()).length === 48 && json(await openClip()) === json(["bass.notes#0"])), "and the piano roll still shows it, first again");

// Click to pick a clip, Delete to remove it.
made = await box('[data-lane="bass.notes"] :nth-child(2 of [data-clip])');
await page.mouse.click(made.x + 20, made.y + made.height / 2);
check(await until(async () => !!(await page.$('[data-lane="bass.notes"] :nth-child(2 of [data-clip])[data-picked]'))), "a click picks the clip");
await page.keyboard.press("Delete");
check(await until(async () => disk().clips.length === 1), "Delete removes it");
check(await until(async () => (await page.$$('[data-lane="bass.notes"] [data-clip]')).length === 1), "from the timeline");
// Notes stays the view and says how to pick another (taste.md §3.7: the dock never goes blank).
check(await until(async () => !!(await page.$("[data-testid=notes][data-empty]"))), "and its piano roll closes");
await undo();
check(await until(async () => disk().clips.length === 2 && disk().clips[1].start === 44 && disk().clips[1].notes.length === 1), "undo brings the clip back, note and all");
for (let k = 0; k < 3; k++) {
  await undo();
  await sleep(300);
}
check(await until(async () => fs.readFileSync(FILE, "utf8") === original), "undoing the move, the note and the new clip leaves the original file");
check(await until(async () => (await page.$$('[data-lane="bass.notes"] [data-clip]')).length === 1), "and one clip in the lane");

// A control lane's clips are clips too (design-8b §7.2): a click on one away from its curve picks it
// and Delete removes it, one undo step. A double-click on its empty space makes an automation clip
// there (8b.5, U7), one undo step. (Automation is folded under its track.)
await page.click('[data-fold="bass"]');
await page.waitForSelector('[data-lane="bass.cutoff"]');
const cutoffFile = `${proj}/data/bass/cutoff.json`;
const cutoffWas = fs.readFileSync(cutoffFile, "utf8");
const cutoff = await box('[data-lane="bass.cutoff"] :nth-child(1 of [data-clip])');
// Its top edge: the curve runs low (300 Hz to 2.2 kHz), so this is the clip's body, not the curve.
await page.mouse.click(cutoff.x + 30, cutoff.y + 3);
await page.keyboard.press("Delete");
const gone = await until(() => JSON.parse(fs.readFileSync(cutoffFile, "utf8")).clips.length === 0);
await undo();
const back = await until(() => fs.readFileSync(cutoffFile, "utf8") === cutoffWas);
const empty = await box('[data-lane="bass.cutoff"] > div:last-child');
await page.mouse.click(empty.x + 40 * PPB, empty.y + empty.height / 2, { count: 2 });
const madeOne = await until(() => JSON.parse(fs.readFileSync(cutoffFile, "utf8")).clips.some((c) => c.start === 40 && c.length === 4));
await undo();
const unmade = await until(() => fs.readFileSync(cutoffFile, "utf8") === cutoffWas);
check(gone && back && madeOne && unmade, `an automation clip picked is deleted by Delete and undo brings it back; a double-click on its lane makes a one-bar automation clip there, and undo takes it away (${gone}, ${back}, ${madeOne}, ${unmade})`);
check(errors.length === 0, `no console errors ${errors.join(" | ")}`);
void ppb3;
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
