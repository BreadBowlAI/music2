// The piano roll and notes clips in the timeline: the roll's edits are one lane write, the
// timeline's clip edits one `clips` gesture, each checked on disk.
// Runs with `node run.mjs notes` (kind proj: a private HOME and project).
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { createHash } from "node:crypto";

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
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; try { localStorage.setItem("m2.graph.panel", "450"); } catch {} }, sess.page_token);

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
const revision = () => page.evaluate(async (token) => (await (await fetch("/api/graph", { headers: { "x-m2-token": token } })).json()).view.revision, sess.page_token);
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
// The server writes the lane's file before it publishes the revision the write made (its settle
// comes between), so the file can be new while /api/graph still says the old revision: the count
// is read once the revision has moved, as for the drag below.
check((await until(async () => (await revision()) > rev)) && (await sleep(300), (await revision()) === rev + 1), "one write");

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

// Draw mode (B), as in GarageBand: a press adds a note, a drag sets its length (its end on the grid)
// and moving up doesn't change its pitch: one note, one write, one undo.
await page.keyboard.press("b");
check(await until(async () => (await page.$eval("[data-tool=draw]", (e) => e.getAttribute("aria-checked"))) === "true"), "B picks the Draw tool");
rev = await revision();
const d0 = cell(60, 1.1), d1 = cell(62, 2.6);
const sounding = () => page.$$eval("[data-testid=notes] [data-key][data-sounding]", (els) => els.map((e) => Number(e.dataset.key)));
await page.mouse.move(d0.x, d0.y);
await page.mouse.down();
await page.mouse.move(d1.x, d1.y, { steps: 12 });
check(await until(async () => (await sounding()).join() === "60"), `C4 sounds while the mouse is held: ${await sounding()}`);
await page.mouse.up();
check(await until(async () => (await sounding()).length === 0), "and stops on release");
check(await until(async () => json(onDisk(60)) === json([{ pitch: "C4", start: 1, length: 1.5, velocity: 0.8 }])), `a drag in Draw mode from beat 1 to 2.5 draws one C4, its end on the grid: ${json(disk().clips[0].notes.filter((n) => ["C4", "C#4", "D4"].includes(n.pitch)))}`);
check(onDisk(61).length === 0 && onDisk(62).length === 0 && disk().clips[0].notes.length === 49, "the drag up two rows left its pitch alone, and nothing else was added");
check((await sleep(300), (await revision()) === rev + 1), "one write");
await undo();
check(await until(async () => fs.readFileSync(FILE, "utf8") === original), "one undo takes it back");

// ⌥-drag in Draw mode paints a note in each grid cell it crosses.
await until(async () => (await at(60)).length === 0);
rev = await revision();
await page.keyboard.down("Alt");
await drag(cell(60, 1.1), cell(60, 2.1));
await page.keyboard.up("Alt");
check(await until(async () => json(onDisk(60).map((n) => [n.start, n.length])) === json([[1, 0.25], [1.25, 0.25], [1.5, 0.25], [1.75, 0.25], [2, 0.25]])), `an ⌥-drag across five cells paints five 1/16 notes: ${json(onDisk(60))}`);
check((await sleep(300), (await revision()) === rev + 1), "one write");
await undo();
check(await until(async () => fs.readFileSync(FILE, "utf8") === original), "one undo takes them back");
await page.keyboard.press("b");
check(await until(async () => (await page.$eval("[data-tool=select]", (e) => e.getAttribute("aria-checked"))) === "true" && (await at(60)).length === 0), "B again: the Select tool");

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
await lengthInput.click({ count: 3 });
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

// What a slow machine can do: the answers to requests for `urls` sent from now on are held until
// `release`, which lets them through in the order they came. Each is paused as it is sent (requests
// sent before never are), and again with its answer, which is held. Interception can take a moment
// to reach the browser's network: a request of the suite's own is paused before it counts as on.
const holder = async (urls) => {
  const cdp = await page.createCDPSession();
  const held = [];
  let on = () => {};
  const armed = new Promise((resolve) => (on = resolve));
  cdp.on("Fetch.requestPaused", (e) => {
    const probe = e.request.url.endsWith("?armed");
    if (probe) on();
    if (probe || e.responseStatusCode === undefined) {
      cdp.send("Fetch.continueRequest", { requestId: e.requestId, interceptResponse: !probe }).catch(() => {});
    } else held.push(e);
  });
  await cdp.send("Fetch.enable", { patterns: [...urls, "/api/graph?armed"].map((u) => ({ urlPattern: `*${u}`, requestStage: "Request" })) });
  await page.evaluate((token) => fetch("/api/graph?armed", { headers: { "x-m2-token": token } }).then((r) => r.text()), sess.page_token);
  await armed;
  return {
    held,
    release: async () => {
      for (const e of held.splice(0)) {
        const body = await cdp.send("Fetch.getResponseBody", { requestId: e.requestId });
        const base64 = body.base64Encoded ? body.body : Buffer.from(body.body).toString("base64");
        await cdp.send("Fetch.fulfillRequest", { requestId: e.requestId, responseCode: e.responseStatusCode, responseHeaders: e.responseHeaders, body: base64 });
      }
      await cdp.send("Fetch.disable");
      await cdp.detach();
    },
  };
};
const refusals = [];
page.on("response", (r) => r.status() === 409 && refusals.push(r.url()));
/** Until the window shows the revision the server is at: each case below starts from a window caught up. */
const caughtUp = () => until(async () => Number(await page.$eval("[data-revision]", (e) => e.dataset.revision)) === (await revision()));

// ⌘Z pressed after an edit made its step but before its answer came back to the window takes back
// that edit: the window waits for its edits on their way, never naming the step below, which the
// server refuses as not the newest (AGENT-2). Here the delete's answer, and the state the window
// reads after it, are held until after ⌘Z.
await caughtUp();
await page.mouse.click(cutoff.x + 30, cutoff.y + 3);
let hold = await holder(["/api/graph/clips", "/api/graph"]);
await page.keyboard.press("Delete");
const deleted = await until(() => hold.held.some((e) => e.request.url.endsWith("/clips")) && JSON.parse(fs.readFileSync(cutoffFile, "utf8")).clips.length === 0);
await undo();
await sleep(500);
await hold.release();
const undone = await until(() => fs.readFileSync(cutoffFile, "utf8") === cutoffWas);
check(deleted && undone && refusals.length === 0, `⌘Z pressed before the delete's answer came back takes the delete back, never refused (${deleted}, ${undone}, ${refusals.length} refused)`);

// A clip dragged in the timeline right after a note is added in the roll, before the window has
// read the state that write made: the drag waits for it, and moves the clip as it is now, the note
// with it (the window's own edit, not a change made elsewhere). Here that state is held until after
// the drag is let go.
await caughtUp();
await page.mouse.click(clipBox.x + 60, clipBox.y + clipBox.height / 2, { count: 2 });
await until(async () => /bars 1–8/.test(await header()) && (await notes()).length === 48);
await page.$eval("[data-testid=roll-scroller]", (el, row, ruler) => { el.scrollTop = ruler + (127 - 60) * row - el.clientHeight / 2; el.scrollLeft = 0; }, ROW, RULER);
await sleep(150);
const g4 = await box("[data-testid=roll-grid]");
hold = await holder(["/api/graph"]);
await page.mouse.click(g4.x + 1.1 * ppb, g4.y + (127 - 60) * ROW + ROW / 2, { count: 2 });
const noted = await until(() => onDisk(60).length === 1);
await drag({ x: clipBox.x + 60, y: clipBox.y + clipBox.height / 2 }, { x: clipBox.x + 60 + 4 * PPB, y: clipBox.y + clipBox.height / 2 }, 10);
await sleep(500);
await hold.release();
const carried = await until(() => disk().clips[0].start === 4 && onDisk(60).length === 1);
await undo();
await sleep(300);
await undo();
const unwound = await until(() => fs.readFileSync(FILE, "utf8") === original);
check(noted && carried && unwound && refusals.length === 0, `a clip dragged right after a note is added in the roll, before the window reads the state that made, moves with the note; two undos put it back (${noted}, ${carried}, ${unwound}, ${refusals.length} refused)`);

// The cases below (the review of notesflake) need an agent beside the window: a Tx with the tools key.
const tools = { "x-m2-token": sess.token, "content-type": "application/json" };
const sessionId = (await (await fetch(`${sess.url}/api/graph`, { headers: tools })).json()).session;
/** An agent's Tx putting `put` into lane `lane` of file `rel`: its answer, with the step it made. */
const agentPut = async (rel, lane, put) => {
  const now = fs.readFileSync(`${proj}/${rel}`, "utf8");
  const tx = {
    id: `tx-agent-${Date.now()}`,
    base: { [rel]: `sha256:${createHash("sha256").update(now).digest("hex")}` },
    ops: [{ op: "clips", lane, put, delete: [] }],
    expect: { confined: { ranges: {}, lanes: [lane], modules: [] } },
  };
  const res = await fetch(`${sess.url}/api/tx/commit`, { method: "POST", headers: tools, body: JSON.stringify({ tx, as: "Claude", why: "a test" }) });
  return res.json();
};
/** The agent takes its step back. */
const agentUndo = (step) => fetch(`${sess.url}/api/graph/undo`, { method: "POST", headers: tools, body: JSON.stringify({ session: sessionId, step, as: "Claude" }) }).then((r) => r.status);
/** The console errors a refusal the case expects leaves, `n` of them, taken out once said. */
const expected409 = async (n) => {
  await until(() => errors.filter((e) => /status of 409/.test(e)).length >= n, 2000);
  for (let k = 0; k < n; k++) {
    const i = errors.findIndex((e) => /status of 409/.test(e));
    if (i >= 0) errors.splice(i, 1);
  }
  refusals.length = 0;
};
/** The roll on Bass's first clip, middle C in view: its grid's box. */
const rollAtC4 = async () => {
  await page.mouse.click(clipBox.x + 60, clipBox.y + clipBox.height / 2, { count: 2 });
  await until(async () => /bars 1–8/.test(await header()) && (await notes()).length >= 48);
  await page.$eval("[data-testid=roll-scroller]", (el, row, ruler) => { el.scrollTop = ruler + (127 - 60) * row - el.clientHeight / 2; el.scrollLeft = 0; }, ROW, RULER);
  await sleep(150);
  return box("[data-testid=roll-grid]");
};
const keysFile = `${proj}/data/keys/chords.json`;
const keysWas = fs.readFileSync(keysFile, "utf8");
const cutoffNow = () => JSON.parse(fs.readFileSync(cutoffFile, "utf8")).clips;

// N1: ⌘Z pressed while a drag, let go, waits for the state of the note added before it takes back
// the drag once it lands, not the note: the waiting drag is an edit on its way too.
await caughtUp();
let g5 = await rollAtC4();
let rev5 = await revision();
hold = await holder(["/api/graph"]);
await page.mouse.click(g5.x + 1.1 * ppb, g5.y + (127 - 60) * ROW + ROW / 2, { count: 2 });
const noted5 = await until(() => onDisk(60).length === 1);
await drag({ x: clipBox.x + 60, y: clipBox.y + clipBox.height / 2 }, { x: clipBox.x + 60 + 4 * PPB, y: clipBox.y + clipBox.height / 2 }, 10);
await undo();
await sleep(500);
await hold.release();
const dragUndone = await until(async () => (await revision()) === rev5 + 3 && disk().clips[0].start === 0 && onDisk(60).length === 1);
await caughtUp();
await undo();
const clean5 = await until(() => fs.readFileSync(FILE, "utf8") === original);
check(noted5 && dragUndone && clean5 && refusals.length === 0, `⌘Z pressed while a drag let go waits takes back the drag once it lands, and the note added before it stays (${noted5}, ${dragUndone}, ${clean5}, ${refusals.length} refused)`);

// N2: an agent's step that lands while the window's edit is on its way is never what ⌘Z takes back:
// it meant the window's own edit, which isn't the newest now, so nothing is undone and the agent's
// step is named (AGENT-2).
await caughtUp();
await page.mouse.click(cutoff.x + 30, cutoff.y + 3);
hold = await holder(["/api/graph/clips"]);
await page.keyboard.press("Delete");
const deleted6 = await until(() => hold.held.length === 1 && cutoffNow().length === 0);
const byAgent = await agentPut("data/keys/chords.json", ["keys", "chords"], [{ id: "agentn2a", start: 64, length: 4, notes: [{ pitch: "C4", start: 0, length: 1 }] }]);
const keysByAgent = fs.readFileSync(keysFile, "utf8");
await caughtUp();
await undo();
await sleep(500);
await hold.release();
const refused6 = await until(() => refusals.some((u) => u.endsWith("/graph/undo")));
await sleep(300);
const kept6 = fs.readFileSync(keysFile, "utf8") === keysByAgent && cutoffNow().length === 0;
const told6 = (await page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent ?? ""))).some((t) => /isn't the newest/.test(t));
await expected409(1);
const agentBack = (await agentUndo(byAgent.step)) === 200 && (await until(() => fs.readFileSync(keysFile, "utf8") === keysWas));
await caughtUp();
await undo();
const clean6 = await until(() => fs.readFileSync(cutoffFile, "utf8") === cutoffWas);
check(deleted6 && byAgent.step !== undefined && refused6 && kept6 && told6 && agentBack && clean6, `an agent's step landing while the window's delete is on its way isn't taken back by ⌘Z, which is refused, naming it (${deleted6}, ${byAgent.step}, ${refused6}, ${kept6}, ${told6}, ${agentBack}, ${clean6})`);

// N3: the wait is bounded. An edit whose answer doesn't come within 2 s: ⌘Z takes back nothing and
// says so; a second ⌘Z pressed while the first waits is dropped, never sent later.
await caughtUp();
await page.mouse.click(cutoff.x + 30, cutoff.y + 3);
hold = await holder(["/api/graph/clips", "/api/graph"]);
await page.keyboard.press("Delete");
const deleted7 = await until(() => hold.held.some((e) => e.request.url.endsWith("/clips")) && cutoffNow().length === 0);
await undo();
await sleep(200);
await undo();
await sleep(2600);
const told7 = (await page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent ?? ""))).some((t) => /isn't answered yet/.test(t));
const none7 = cutoffNow().length === 0;
await hold.release();
await sleep(800);
const still7 = cutoffNow().length === 0 && fs.readFileSync(FILE, "utf8") === original;
await caughtUp();
await undo();
const clean7 = await until(() => fs.readFileSync(cutoffFile, "utf8") === cutoffWas);
check(deleted7 && told7 && none7 && still7 && clean7 && refusals.length === 0, `⌘Z waits at most 2 s for an edit's answer, then says so and takes back nothing; a ⌘Z pressed meanwhile is dropped (${deleted7}, ${told7}, ${none7}, ${still7}, ${clean7}, ${refusals.length} refused)`);

// N4: a drag pressed while the timeline draws an older picture (a note just added in the roll) still
// names the clips of other lanes by the hash it drew: an agent's change to the dragged clip, which
// the window hadn't shown, refuses the drag (WRITE-12), as when the timeline is caught up.
await caughtUp();
g5 = await rollAtC4();
hold = await holder(["/api/graph"]);
const changed8 = await agentPut("data/bass/cutoff.json", ["bass", "cutoff"], [{ id: "adp0sw76", start: 0, length: 16, points: [[0, 300], [16, 2000]] }]);
const cutoffByAgent = fs.readFileSync(cutoffFile, "utf8");
await page.mouse.click(g5.x + 1.1 * ppb, g5.y + (127 - 60) * ROW + ROW / 2, { count: 2 });
const noted8 = await until(() => onDisk(60).length === 1);
await page.mouse.move(cutoff.x + 30, cutoff.y + 3);
await page.mouse.down();
await page.mouse.move(cutoff.x + 30 + 4 * PPB, cutoff.y + 3, { steps: 10 });
await page.mouse.up();
await sleep(300);
await hold.release();
const refused8 = await until(() => refusals.some((u) => u.endsWith("/graph/clips")));
await sleep(300);
const kept8 = fs.readFileSync(cutoffFile, "utf8") === cutoffByAgent;
await expected409(1);
await caughtUp();
await undo();
const clean8 = await until(() => fs.readFileSync(FILE, "utf8") === original);
const agentBack8 = (await agentUndo(changed8.step)) === 200 && (await until(() => fs.readFileSync(cutoffFile, "utf8") === cutoffWas));
check(noted8 && changed8.step !== undefined && refused8 && kept8 && agentBack8 && clean8, `a drag pressed on an older picture is refused when an agent changed the clip of another lane meanwhile (${noted8}, ${changed8.step}, ${refused8}, ${kept8}, ${agentBack8}, ${clean8})`);

check(errors.length === 0, `no console errors ${errors.join(" | ")}`);
void ppb3;
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
