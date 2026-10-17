// 8b.5 The timeline as an editor (design-8b §7, taste.md §3.4, §5).
// Part 1, its grid and snap (U4): the grid chip in the corner (Auto, the menu, triplets), a drag
// that lands on the grid, ⌘ held to place a clip freely, ⌘1 to ⌘4 on the view last pointed at (the
// timeline or Notes), the choice kept over a reload, a click on empty lane space that moves where
// play starts and writes nothing.
// Part 2, clip gestures (U5): each edit one `clips` gesture and one undo step, nothing else on
// disk, no toast; either edge resized with the notes kept in time; ⌥-drag copies; a clip dragged
// to another track's lane, with the bubble, the guide and the lit lane; lanes that can't take it
// say why; ⇧-click and ⌘-click select, a drag moves every selected clip, ⌫ deletes them; a clip
// changed elsewhere while held is refused in words.
// And a PC page's words.
//
//   node e2e/s8-clips.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, posts, api, state, toasts, key, box, drag } = await open(1440, 900);
const status = async () => (await api("/status")).body;
const clips = (path) => JSON.parse(read(path)).clips;
const near = (a, b) => Math.abs(a - b) < 1e-3;
const PPB = 24;
/** The grid chip's label and the snap toggle's state, in the timeline's corner or (`notes`) the roll. */
const chip = (view = "song") =>
  page.evaluate((ids) => {
    const g = document.querySelector(`[data-testid=${ids[0]}]`), s = document.querySelector(`[data-testid=${ids[1]}]`);
    return g && s ? { grid: g.dataset.grid, title: g.title, snap: s.getAttribute("aria-pressed") === "true", snapTitle: s.title } : null;
  }, view === "song" ? ["song-grid", "song-snap"] : ["grid", "snap"]);
/** Drag the hats clip `dx` px along its lane (holding ⌘ when `free`); resolves once its file shows a new start. */
const dragHats = async (dx, { free = false } = {}) => {
  const before = clips("data/hats/hits.json")[0].start;
  const c = await box('[data-lane="hats.hits"] [data-clip]');
  if (free) await page.keyboard.down("Meta");
  await drag({ x: c.x + 30, y: c.y + c.height / 2 }, { x: c.x + 30 + dx, y: c.y + c.height / 2 });
  if (free) await page.keyboard.up("Meta");
  await until(() => clips("data/hats/hits.json")[0].start !== before);
  return clips("data/hats/hits.json")[0].start;
};

/** A click on the kick's lane at `beat`, past its clip: empty lane space. */
const clickLane = async (beat) => {
  const l = await box('[data-lane="kick.hits"] > div:last-child');
  await page.mouse.click(l.x + beat * PPB, l.y + l.height / 2);
};

// ---------------------------------------------------------------- the chip: Auto, its lines
await page.waitForSelector('[data-lane="hats.hits"] [data-clip]');
await until(() => page.$("[data-testid=song-grid]"));
const c0 = (await chip()) ?? { title: "", snapTitle: "" };
check(c0.grid === "Auto" && c0.snap, `the timeline's corner has the grid chip: Auto, snap on (${JSON.stringify({ grid: c0.grid, snap: c0.snap })})`);
check(c0.title.includes("Auto is 1/8 at this zoom") && c0.title.includes("⌘1") && c0.snapTitle.includes("⌘4") && c0.snapTitle.includes("hold ⌘"), `its tooltips say what Auto is and the keys: ${c0.title.replace(/\n/g, " | ")}`);
const corner = await page.evaluate(() => {
  const r = (s) => document.querySelector(s)?.getBoundingClientRect() ?? { left: NaN, right: NaN };
  return { add: r("[data-testid=add-track]").right, grid: r("[data-testid=song-grid]").left, fold: r("[data-testid=fold-all]").left, snap: r("[data-testid=song-snap]").right };
});
check(corner.add < corner.grid && corner.snap <= corner.fold, `between + Track and the fold-all chevron: ${JSON.stringify(corner)}`);
check(!!(await page.$('[data-lane="kick.hits"] pattern[width="12"]')), "at 24 px a beat Auto draws a line every eighth (12 px)");

// ---------------------------------------------------------------- a drag lands on the grid; ⌘ skips it
const id = clips("data/hats/hits.json")[0].id;
check((await dragHats(32)) === 1.5, `a drag of 32 px (1⅓ beats) lands on the nearest eighth: ${clips("data/hats/hits.json")[0].start}`);
check(clips("data/hats/hits.json")[0].id === id, "keeping its id");
const free = await dragHats(20, { free: true });
check(near(free, 1.5 + 20 / PPB) && !Number.isInteger(free * 8), `holding ⌘ places it off the grid: ${free}`);

// ---------------------------------------------------------------- ⌘1 to ⌘4
await clickLane(45);
await key("Meta+4");
check(await until(async () => (await chip()).snap === false), "⌘4 turns snap off: the magnet greys");
const off = await dragHats(10);
check(near(off, free + 10 / PPB), `with snap off a clip goes where it is put: ${off}`);
await key("Meta+4");
check(await until(async () => (await chip()).snap === true), "⌘4 turns it back on");
const seen = [];
for (const k of ["Meta+1", "Meta+2", "Meta+2", "Meta+3", "Meta+1", "Meta+3"]) {
  await key(k);
  await sleep(60);
  seen.push((await chip()).grid);
}
check(seen.join(" ") === "1/16 1/8 1/4 1/4T 1/8T 1/8", `⌘1 narrower, ⌘2 wider, ⌘3 triplets: ${seen.join(" → ")}`);

// ---------------------------------------------------------------- the menu
await page.click("[data-testid=song-grid]");
await page.waitForSelector('[role=menu][aria-label=Grid]');
const rows = await page.$$eval('[role=menu][aria-label=Grid] [role^=menuitem]', (els) => els.map((e) => `${e.textContent}${e.getAttribute("aria-checked") === "true" ? "✓" : ""}`));
await page.screenshot({ path: `${ROOT}/s8-clips-grid.png`, clip: { x: 0, y: 0, width: 720, height: 420 }, captureBeyondViewport: false });
check(rows.join(" ") === "Auto Bar 1/2 1/4 1/8✓ 1/16 1/32 1/64 Triplets⌘3", `the menu lists Auto to 1/64 and Triplets, the grid checked: ${rows.join(" ")}`);
await page.click('[role=menu][aria-label=Grid] [data-grid="Bar"]');
check(await until(async () => (await chip()).grid === "Bar"), "picking Bar sets it");
check((await dragHats(30)) === 4, `a drag then lands on a bar line: ${clips("data/hats/hits.json")[0].start}`);

// ---------------------------------------------------------------- the view last pointed at
const hats = await box('[data-lane="hats.hits"] [data-clip]');
await page.mouse.click(hats.x + 40, hats.y + hats.height / 2, { clickCount: 2 });
await page.waitForSelector("[data-testid=roll-grid]");
const roll = await box("[data-testid=roll-grid]");
await page.mouse.click(roll.x + 30, roll.y + 30);
await key("Meta+1");
check(await until(async () => (await chip("notes")).grid === "1/32") && (await chip()).grid === "Bar", `after a click in Notes, ⌘1 changes the roll's grid, not the timeline's: ${(await chip("notes")).grid}, ${(await chip()).grid}`);
await clickLane(45);
await key("Meta+1");
// The kick's lane selects the kick, so the roll has gone: its grid is read where it is kept.
const rollGrid = () => page.evaluate(() => localStorage.getItem("m2.notes.grid"));
check(await until(async () => (await chip())?.grid === "1/2") && (await rollGrid()) === "0.125", `after a click in the timeline, ⌘1 is the timeline's again: ${(await chip())?.grid}, the roll's ${await rollGrid()}`);

// ---------------------------------------------------------------- kept over a reload (in a browser; the app starts over)
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=song-grid]");
check((await chip()).grid === "1/2", `a reload keeps the timeline's grid: ${(await chip()).grid}`);

// ---------------------------------------------------------------- a click on empty lane space: the start point, no write
await page.click("[data-testid=song-grid]");
await page.click('[role=menu][aria-label=Grid] [data-grid="Auto"]');
await until(async () => (await chip()).grid === "Auto");
const rev = (await state()).view.revision;
const kickFile = read("data/kick/hits.json");
const writes = posts.length;
await clickLane(40.3);
check(await until(async () => near((await status()).beat, 40.5)), `a click at beat 40.3 on empty lane space moves the playhead to the eighth there: ${(await status()).beat}`);
await sleep(300);
check(posts.slice(writes).every((p) => p.url === "/api/graph/seek") && read("data/kick/hits.json") === kickFile && (await state()).view.revision === rev, `and writes nothing: ${posts.slice(writes).map((p) => p.url)}`);

// ---------------------------------------------------------------- the ⌥K sheet
await key("Alt+k");
await page.waitForSelector("[data-testid=shortcuts]");
const sheet = await page.$eval("[data-testid=shortcuts]", (e) => e.innerText);
check(sheet.includes("Narrower / wider grid") && sheet.includes("⌘1 / ⌘2") && sheet.includes("Triplet grid") && sheet.includes("hold ⌘ while dragging"), "the shortcuts sheet lists the grid's keys");
check(sheet.includes("Copy a clip") && sheet.includes("⌥-drag") && sheet.includes("⇧-click / ⌘-click"), "and the clips' gestures");
await key("Escape");

// ================================================================ part 2: clip gestures (U5)
const lanes = (path) => JSON.parse(read(path)).clips;
const sent = (from) => posts.slice(from);
const gestureOf = (p) => JSON.parse(p.body).gesture;
/** Every file of the song but `.music2/`'s, as text. */
const snapshot = () =>
  Object.fromEntries(
    fs
      .readdirSync(PROJ, { recursive: true })
      .filter((f) => !f.startsWith(".music2") && fs.statSync(`${PROJ}/${f}`).isFile())
      .map((f) => [f, fs.readFileSync(`${PROJ}/${f}`, "utf8")]),
  );
const changed = (a, b) => Object.keys({ ...a, ...b }).filter((f) => a[f] !== b[f]);
const clipBox = (lane, id) => box(`[data-lane="${lane}"] [data-clip="${id}"]`);
const mid = (b) => ({ x: b.x + 30, y: b.y + b.height / 2 });
const rowOf = (lane) => box(`[data-lane="${lane}"] > div:last-child`);
const pickedIn = (lane) => page.$$eval(`[data-lane="${lane}"] [data-clip][data-picked]`, (els) => els.map((e) => e.dataset.clip));
/** Press at `from`, move to `to` (holding `mods`), look with `during`, let go; answers what `during` saw. */
const hold = async (from, to, { mods = [], during } = {}) => {
  for (const m of mods) await page.keyboard.down(m);
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(to.x, to.y, { steps: 10 });
  const seen = during ? await during() : null;
  await page.mouse.up();
  for (const m of [...mods].reverse()) await page.keyboard.up(m);
  return seen;
};
const looks = async (lane) => ({
  bubble: await page.$eval("[data-testid=clip-bubble]", (e) => e.textContent).catch(() => null),
  guide: await box("[data-testid=clip-guide]"),
  lit: lane ? await page.$eval(`[data-lane="${lane}"] > div:last-child`, (e) => e.className.includes("bg-accent-wash")) : null,
});

// ---------------------------------------------------------------- a move: one gesture, nothing else
const hatsId = lanes("data/hats/hits.json")[0].id;
let n = posts.length;
let before = snapshot();
let b = await clipBox("hats.hits", hatsId);
const moved = await hold(mid(b), { x: mid(b).x + 4 * PPB, y: mid(b).y }, { during: () => looks() });
check(await until(() => lanes("data/hats/hits.json")[0].start === 8), `a drag moves the hats clip a bar right: ${lanes("data/hats/hits.json")[0].start}`);
const lane0 = await rowOf("hats.hits");
check(moved.bubble === "3.1.1" && Math.abs(moved.guide?.x - (lane0.x + 8 * PPB)) <= 1, `while dragged, the bubble says where it lands and a guide marks it: ${moved.bubble}, ${moved.guide?.x}`);
const g1 = sent(n);
const move1 = g1[0] && gestureOf(g1[0]);
check(g1.length === 1 && g1[0].url === "/api/graph/clips" && move1.op === "move" && move1.by === 4, `as one move gesture, and nothing else sent: ${g1.map((p) => `${p.url} ${p.body.slice(0, 80)}`)}`);
check(move1?.clips[0].id === hatsId && /^[0-9a-f]{16}$/.test(move1.clips[0].sha ?? ""), `naming the clip by its id, with the hash the window drew: ${JSON.stringify(move1?.clips[0])}`);
await sleep(300);
check(changed(before, snapshot()).join() === "data/hats/hits.json", `only the hats lane changed on disk: ${changed(before, snapshot())}`);
check((await toasts()).length === 0, "no toast: the clip shows where it went");
const undoTitle = await page.$eval('button[aria-label="Undo"]', (e) => e.title);
check(undoTitle.startsWith("Undo moving a Hats clip"), `Undo names it: ${undoTitle}`);

// ---------------------------------------------------------------- either edge; the notes stay in time
const hatsNow = () => lanes("data/hats/hits.json").find((c) => c.id === hatsId);
const heardAt = () => hatsNow().notes.map((x) => Number((hatsNow().start + x.start).toFixed(6))).join();
const times = heardAt();
const edgeCursor = await page.$eval(`[data-lane="hats.hits"] [data-clip="${hatsId}"] [data-edge=right]`, (e) => getComputedStyle(e).cursor);
check(edgeCursor === "ew-resize", `each edge has a resize cursor: ${edgeCursor}`);
b = await clipBox("hats.hits", hatsId);
const right = { x: b.x + b.width - 3, y: b.y + b.height / 2 };
const r = await hold(right, { x: right.x - 4 * PPB, y: right.y }, { during: () => looks() });
check(await until(() => hatsNow().length === 28) && hatsNow().start === 8, `the right edge dragged a bar left: ${hatsNow().start}+${hatsNow().length}; the bubble said ${r.bubble}`);
check(heardAt() === times && r.bubble === "10.1.1", "every note stays where it plays");
b = await clipBox("hats.hits", hatsId);
const trimmed = read("data/hats/hits.json");
const left = { x: b.x + 3, y: b.y + b.height / 2 };
await hold(left, { x: left.x + 4 * PPB, y: left.y });
check(await until(() => hatsNow().start === 12) && hatsNow().length === 24, `the left edge dragged a bar right: ${hatsNow().start}+${hatsNow().length}`);
check(heardAt() === times && hatsNow().notes.length === 32, "the notes keep their place in time; those before the edge stay in the file, silent");
await key("Meta+z");
check(await until(() => read("data/hats/hits.json") === trimmed), "one undo puts the left edge back, byte for byte");

// ---------------------------------------------------------------- ⌥-drag copies
const kick0 = lanes("data/kick/hits.json")[0];
b = await clipBox("kick.hits", kick0.id);
await hold(mid(b), { x: mid(b).x + 40 * PPB, y: mid(b).y }, { mods: ["Alt"] });
check(await until(() => lanes("data/kick/hits.json").length === 2), "⌥-drag copies the kick clip");
const copy = lanes("data/kick/hits.json").find((c) => c.id !== kick0.id);
const orig = lanes("data/kick/hits.json").find((c) => c.id === kick0.id);
check(copy?.start === 40 && /^[0-9a-z]{8}$/.test(copy.id) && JSON.stringify(copy.notes) === JSON.stringify(kick0.notes) && orig?.start === 0, `with a new id at bar 11, the original where it was: ${copy?.id} at ${copy?.start}`);
check(await until(async () => (await pickedIn("kick.hits")).join() === copy.id), "the copy is selected, not the original");

// ---------------------------------------------------------------- to another track's lane
b = await clipBox("kick.hits", copy.id);
const hatsRow = await rowOf("hats.hits");
const shot = (name) => page.screenshot({ path: `${ROOT}/${name}.png`, captureBeyondViewport: false });
const across = await hold(mid(b), { x: mid(b).x, y: hatsRow.y + hatsRow.height / 2 }, { during: async () => (await shot("s8-clips-across"), looks("hats.hits")) });
check(across.bubble === "11.1.1 · Hats" && across.lit && Math.abs(across.guide?.x - (hatsRow.x + 40 * PPB)) <= 1, `dragged over Hats: the bubble names it, its lane is lit, the guide at bar 11: ${JSON.stringify({ ...across, guide: across.guide?.x })}`);
check(await until(() => lanes("data/kick/hits.json").length === 1 && lanes("data/hats/hits.json").some((c) => c.id === copy.id && c.start === 40)), "let go, the clip moves from Kick's lane to Hats', keeping its id");
check(await until(async () => (await pickedIn("hats.hits")).join() === copy.id), "and stays selected there");

// ---------------------------------------------------------------- lanes that can't take it
await page.click('[data-fold="bass"]');
await page.waitForSelector('[data-lane="bass.cutoff"]');
n = posts.length;
before = snapshot();
const cutoff = await rowOf("bass.cutoff");
b = await clipBox("hats.hits", hatsId);
const onto = await hold(mid(b), { x: mid(b).x, y: cutoff.y + cutoff.height / 2 }, { during: () => looks("bass.cutoff") });
check(onto.bubble === "Automation only" && onto.lit === false, `over an automation lane the bubble says why, and the lane isn't lit: ${onto.bubble}`);
const cut = lanes("data/bass/cutoff.json")[0];
b = await clipBox("bass.cutoff", cut.id);
const drop = await rowOf("bass.drop");
const units = await hold(mid(b), { x: mid(b).x, y: drop.y + drop.height / 2 }, { during: () => looks("bass.drop") });
check(units.bubble === "Only 0–1 automation", `an automation clip over a lane of another unit: ${units.bubble}`);
await sleep(400);
check(sent(n).length === 0 && changed(before, snapshot()).length === 0, "let go there, nothing is sent or changed");

// ---------------------------------------------------------------- selecting several; a drag moves them all; ⌫
const keysId = lanes("data/keys/chords.json")[0].id;
await page.click(`[data-lane="hats.hits"] [data-clip="${hatsId}"]`);
await page.keyboard.down("Shift");
await page.click(`[data-lane="keys.chords"] [data-clip="${keysId}"]`);
await page.keyboard.up("Shift");
const both = async () => [...(await pickedIn("hats.hits")), ...(await pickedIn("keys.chords"))].join();
check((await both()) === `${hatsId},${keysId}`, `⇧-click adds a clip of another track: ${await both()}`);
await shot("s8-clips-select");
await page.keyboard.down("Meta");
await page.click(`[data-lane="keys.chords"] [data-clip="${keysId}"]`);
const toggled = await both();
await page.click(`[data-lane="keys.chords"] [data-clip="${keysId}"]`);
await page.keyboard.up("Meta");
check(toggled === hatsId && (await both()) === `${hatsId},${keysId}`, `⌘-click takes one out, and puts it back: ${toggled}`);
n = posts.length;
b = await clipBox("hats.hits", hatsId);
await hold(mid(b), { x: mid(b).x + 2 * PPB, y: mid(b).y });
check(await until(() => hatsNow().start === 10 && lanes("data/keys/chords.json")[0].start === 2), `a drag moves both by 2 beats: ${hatsNow().start}, ${lanes("data/keys/chords.json")[0].start}`);
check(sent(n).length === 1 && gestureOf(sent(n)[0]).clips.length === 2, "in one gesture");
const kept = { hats: read("data/hats/hits.json"), keys: read("data/keys/chords.json") };
n = posts.length;
await key("Backspace");
check(await until(() => !lanes("data/hats/hits.json").some((c) => c.id === hatsId) && lanes("data/keys/chords.json").length === 0), "⌫ deletes both");
check(sent(n).length === 1 && gestureOf(sent(n)[0]).op === "delete" && gestureOf(sent(n)[0]).clips.length === 2, "in one gesture");
await key("Meta+z");
check(await until(() => read("data/hats/hits.json") === kept.hats && read("data/keys/chords.json") === kept.keys), "one undo brings both back");

// ---------------------------------------------------------------- a clip changed elsewhere while held
await until(async () => !!(await clipBox("hats.hits", hatsId)));
b = await clipBox("hats.hits", hatsId);
await page.mouse.move(mid(b).x, mid(b).y);
await page.mouse.down();
await page.mouse.move(mid(b).x + 2 * PPB, mid(b).y, { steps: 6 });
const file = JSON.parse(read("data/hats/hits.json"));
file.clips.find((c) => c.id === hatsId).notes[0].velocity = 0.31;
write("data/hats/hits.json", JSON.stringify(file, null, 2));
const seenIt = await until(async () => (await state()).timeline.tracks.find((t) => t.module === "hats").lanes[0].clips.find((c) => c.id === hatsId)?.notes[0].velocity === 0.31);
const outside = read("data/hats/hits.json");
await page.mouse.move(mid(b).x + 4 * PPB, mid(b).y, { steps: 4 });
await page.mouse.up();
check(seenIt && (await until(async () => (await toasts()).some((t) => t.includes("changed in another window or by an agent")))), `an outside edit while it was held: the drop is refused in words: ${(await toasts()).join(" | ")}`);
await sleep(300);
check(read("data/hats/hits.json") === outside, "and the file keeps the outside edit");

// ---------------------------------------------------------------- a PC page says Ctrl
const pc = await open(1440, 900, { pc: true });
await pc.page.waitForSelector("[data-testid=song-grid]");
const pcWords = await pc.page.evaluate(() => [document.querySelector("[data-testid=song-grid]").title, document.querySelector("[data-testid=song-snap]").title].join(" | "));
check(pcWords.includes("Ctrl+1") && pcWords.includes("Ctrl+4") && pcWords.includes("hold Ctrl") && !pcWords.includes("⌘"), `a PC page's tooltips say Ctrl: ${pcWords.replace(/\n/g, " ")}`);
const clipWords = await pc.page.$eval('[data-lane="kick.hits"] [data-clip]', (e) => e.title);
check(clipWords.includes("Alt-drag to copy") && clipWords.includes("drag an edge to resize"), `and a clip's tooltip says Alt-drag: ${clipWords.replace(/\n/g, " | ")}`);
await pc.browser.close();

await browser.close();
done([...errors, ...pc.errors]);
