// 8b.5 The timeline as an editor (design-8b §7, taste.md §3.4, §5).
// Part 1, its grid and snap (U4): the grid chip in the corner (Auto, the menu, triplets), a drag
// that lands on the grid, ⌘ held to place a clip freely, ⌘1 to ⌘4 on the view last pointed at (the
// timeline or Notes; the timeline once Notes has gone), the choice kept over a reload, a click on empty lane space that moves where
// play starts and writes nothing.
// Part 2, clip gestures (U5): each edit one `clips` gesture and one undo step, nothing else on
// disk, no toast; either edge resized with the notes kept in time; ⌥-drag copies; a clip dragged
// to another track's lane, with the bubble, the guide and the lit lane; lanes that can't take it
// say why; ⇧-click and ⌘-click select, a drag moves every selected clip, ⌫ deletes them; a clip
// changed elsewhere while held is refused in words.
// Part 3, clip commands (U6): the clip menu and the lane menu; ⌘D (a verse ending on beat 4 of bar 8
// lands at bar 9); a marquee whose ⌘D keeps its empty bars; ⌘E and ⌘J; ⌘C ⌘X ⌘V at the playhead,
// onto the selected track, and the copy events the app's menu runs; Q; ← → and ⇧→; ⌘E in the code
// editor stays its own; the ⌥K sheet.
// And a PC page's words.
//
//   node e2e/s8-clips.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, posts, api, state, toasts, key, box, drag, view } = await open(1440, 900);
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
// The kick's lane selects the kick, so the roll has gone: its grid is read where it is kept, the
// song's view settings (polish C1).
const rollGrid = async () => (await state()).local?.grid?.notes?.pick;
// The settings are sent a moment after the last change (lib/local): wait for the roll's to land.
check(await until(async () => (await chip())?.grid === "1/2" && (await rollGrid()) === 0.125), `after a click in the timeline, ⌘1 is the timeline's again: ${(await chip())?.grid}, the roll's ${await rollGrid()}`);

// Notes gone from the screen, its grid isn't the keys' any more: the timeline's is the only one shown.
const rollSnap = async () => (await state()).local?.grid?.notes?.on;
const tabWas = await page.$eval("[role=tablist][aria-label=Views] [aria-selected=true]", (e) => e.dataset.viewTab).catch(() => null);
for (const [how, away] of [["E hides the dock", "e"], ["⌥3 shows Mix there", "Alt+3"]]) {
  const bc = await box('[data-lane="bass.notes"] [data-clip]');
  await page.mouse.click(bc.x + 20, bc.y + bc.height / 2, { clickCount: 2 });
  await page.waitForSelector("[data-testid=roll-grid]");
  check(await until(() => page.evaluate(() => !!document.activeElement?.closest("[data-grid-view=notes]")), 3000), `a double-click opens Bass's clip in Notes, which takes the keys (${how})`);
  const was = await rollSnap();
  await key(away);
  await until(async () => !(await page.$("[data-testid=roll-grid]")), 3000);
  await key("Meta+4");
  check(await until(async () => (await chip()).snap === false, 2000) && (await rollSnap()) === was, `then ${how}: ⌘4 turns the timeline's snap off, and the roll's stays as it was (${was})`);
  await key("Meta+4");
  await until(async () => (await chip()).snap === true);
}
await clickLane(45);
if (tabWas) await page.click(`[data-view-tab="${tabWas}"]`).catch(() => {});

// ---------------------------------------------------------------- kept over a reload (with the song's view settings)
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=song-grid]");
check((await chip()).grid === "1/2", `a reload keeps the timeline's grid: ${(await chip()).grid}`);

// ---------------------------------------------------------------- a click on empty lane space: the start point, no write
await page.click("[data-testid=song-grid]");
await page.click('[role=menu][aria-label=Grid] [data-grid="Auto"]');
await until(async () => (await chip()).grid === "Auto");
// Kept with the song's view settings a moment later: not a post of the click below.
await until(async () => (await state()).local?.grid?.song?.pick === "auto");
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
/** What was sent since `from`, once this script has heard of it: the browser tells it of a request
 * on its own channel, which under load can arrive after the server has written the file. */
const sentBy = async (from) => {
  await until(() => sent(from).length > 0, 3000);
  return sent(from);
};
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
const g1 = await sentBy(n);
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
// The window draws the trimmed clip once it has heard of it: the next drag names the clip as it is
// now (its hash), not as it was.
await until(async () => (await clipBox("hats.hits", hatsId)).width < 31 * PPB);
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
// Grabbed by its top strip, away from the curve (on the curve a drag moves a point, U7).
const grab = { x: b.x + 30, y: b.y + 2 };
const units = await hold(grab, { x: grab.x, y: drop.y + drop.height / 2 }, { during: () => looks("bass.drop") });
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
const g2 = await sentBy(n);
check(g2.length === 1 && gestureOf(g2[0]).clips.length === 2, "in one gesture");
const kept = { hats: read("data/hats/hits.json"), keys: read("data/keys/chords.json") };
n = posts.length;
await key("Backspace");
check(await until(() => !lanes("data/hats/hits.json").some((c) => c.id === hatsId) && lanes("data/keys/chords.json").length === 0), "⌫ deletes both");
const g3 = await sentBy(n);
check(g3.length === 1 && gestureOf(g3[0]).op === "delete" && gestureOf(g3[0]).clips.length === 2, "in one gesture");
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

// ================================================================ part 3: clip commands (U6)
/** The example's own file, as the fixture had it. */
const fixture = (f) => fs.readFileSync(`${process.env.E2E_REPO}/examples/first-song/${f}`, "utf8");
/** A lane's clips as the server reads them now (pitches as numbers). */
const shown = async (module, lane) => (await state()).timeline.tracks.find((t) => t.module === module).lanes.find((l) => l.key === lane).clips;
const starts = (c) => c.map((x) => x.start).join();
/** The playhead at `beat`, by a click on the ruler (whole beats), once the page's clock shows it. */
const playAt = async (beat) => {
  const r = await box("[data-testid=ruler]");
  await page.mouse.click(r.x + beat * PPB + 1, r.y + r.height / 2);
  return until(async () => Number(await page.$eval("[data-testid=playhead]", (e) => e.dataset.beat)) === beat);
};
const clickClip = async (lane, id, mods = []) => {
  for (const m of mods) await page.keyboard.down(m);
  await page.click(`[data-lane="${lane}"] [data-clip="${id}"]`);
  for (const m of [...mods].reverse()) await page.keyboard.up(m);
};
const menuRows = () =>
  page.$$eval("[data-testid=clip-menu] [role=menuitem]", (els) => els.map((e) => `${e.textContent}${e.disabled ? "(off)" : ""}`));
const lastGesture = (from) => sent(from).filter((p) => p.url === "/api/graph/clips").map(gestureOf);
const undoNow = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);

// A known start: Kick's verse ends on beat 4 of bar 8 (0 to 31), Hats and Keys whole bars 1 to 8.
const kickClip = JSON.parse(fixture("data/kick/hits.json"));
kickClip.clips[0].length = 31;
write("data/kick/hits.json", JSON.stringify(kickClip, null, 2));
write("data/hats/hits.json", fixture("data/hats/hits.json"));
write("data/keys/chords.json", fixture("data/keys/chords.json"));
const K = kickClip.clips[0].id, H = JSON.parse(fixture("data/hats/hits.json")).clips[0].id;
await until(async () => starts(await shown("hats", "hits")) === "0" && (await shown("kick", "hits"))[0]?.length === 31 && (await shown("keys", "chords")).length === 1);
await key("Escape");
await key("Escape");
await playAt(0);

// ---------------------------------------------------------------- the clip menu
b = await clipBox("kick.hits", K);
await page.mouse.click(b.x + 40, b.y + b.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
const kickMenu = await menuRows();
await page.screenshot({ path: `${ROOT}/s8-clips-menu.png`, clip: { x: 0, y: 0, width: 720, height: 560 }, captureBeyondViewport: false });
check(
  // Part 1 left the roll's grid at 1/32: Quantize says so.
  kickMenu.join(" | ") === "Cut⌘X | Copy⌘C | Paste⌘V(off) | Duplicate⌘D | Repeat… | Export as MIDI… | Split at playhead⌘E(off) | Quantize to 1/32Q | Delete⌫ | Loop this clip⌘L | Open in Notes",
  `a right-click on a clip opens its menu, each row with its key; Paste waits for a copy, Split for the playhead inside: ${kickMenu.join(" | ")}`,
);
check((await pickedIn("kick.hits")).join() === K, "and the clip is the selection");
await key("Escape");
check(await until(async () => !(await page.$("[data-testid=clip-menu]"))), "Escape closes it");

// ---------------------------------------------------------------- ⌘D: a verse ending on beat 4 of bar 8 lands at bar 9
n = posts.length;
await key("Meta+d");
check(await until(async () => starts(await shown("kick", "hits")) === "0,32"), `⌘D copies the Kick verse (0 to 31) onto bar 9, not beat 31: ${starts(await shown("kick", "hits"))}`);
const dup = lastGesture(n);
check(dup.length === 1 && dup[0].op === "duplicate" && dup[0].clips.length === 1 && !dup[0].range, `one duplicate gesture: ${JSON.stringify(dup)}`);
const kickCopy = (await shown("kick", "hits")).find((c) => c.id !== K);
check(await until(async () => (await pickedIn("kick.hits")).join() === kickCopy.id), "the copy is the selection, so ⌘D again goes on");
check(await until(async () => (await undoNow()).startsWith("Undo duplicating a Kick clip")), `Undo names it: ${await undoNow()}`);
await key("Meta+z");
await until(async () => starts(await shown("kick", "hits")) === "0");

// ---------------------------------------------------------------- the marquee keeps its empty bars
const kickRow = await rowOf("kick.hits");
const hatsLane = await rowOf("hats.hits");
const beat0 = (await status()).beat;
const mq = await hold({ x: kickRow.x + 39 * PPB + 6, y: kickRow.y + kickRow.height / 2 }, { x: hatsLane.x + 3 * PPB, y: hatsLane.y + hatsLane.height / 2 }, {
  during: async () => ({ bubble: await page.$eval("[data-testid=clip-bubble]", (e) => e.textContent).catch(() => null), box: await box("[data-testid=marquee]") }),
});
check(mq.bubble === "Bars 1–10" && Math.abs(mq.box?.x - kickRow.x) <= 1 && Math.abs(mq.box?.width - 40 * PPB) <= 2, `a drag on empty lane space draws a marquee on bar lines, its bubble naming the bars: ${mq.bubble}, ${mq.box?.x} + ${mq.box?.width}`);
check((await pickedIn("kick.hits")).join() === K && (await pickedIn("hats.hits")).join() === H && (await pickedIn("keys.chords")).length === 0, "the clips it covers, in the lanes it crosses, are the selection");
await sleep(250);
check((await status()).beat === beat0 && !(await page.$("[data-testid=marquee]")), `let go, the marquee goes and play's start stays (${(await status()).beat})`);
n = posts.length;
await key("Meta+d");
check(await until(async () => starts(await shown("kick", "hits")) === "0,40" && starts(await shown("hats", "hits")) === "0,40"), `⌘D copies bars 1 to 10, empty bars kept: both copies at bar 11 (${starts(await shown("kick", "hits"))}; ${starts(await shown("hats", "hits"))})`);
const mdup = lastGesture(n)[0];
check(mdup?.op === "duplicate" && mdup.range?.join() === "0,40" && mdup.clips.length === 2, `as one gesture with the marquee's bars: ${JSON.stringify(mdup?.range)}`);
await key("Meta+z");
await until(async () => starts(await shown("kick", "hits")) === "0" && starts(await shown("hats", "hits")) === "0");

// ---------------------------------------------------------------- ⌘E splits at the playhead, ⌘J joins
const hatsBefore = JSON.stringify((await shown("hats", "hits"))[0].notes);
await playAt(16);
await clickClip("hats.hits", H);
n = posts.length;
await key("Meta+e");
check(await until(async () => starts(await shown("hats", "hits")) === "0,16"), `⌘E splits the Hats clip at the playhead (bar 5): ${starts(await shown("hats", "hits"))}`);
const halves = await shown("hats", "hits");
check(halves[0].id === H && halves[0].length === 16 && halves[1].length === 16 && halves[1].id !== H, "the first half keeps the id, the second gets its own");
check(await until(async () => (await pickedIn("hats.hits")).length === 2), "both halves are the selection");
check(await until(async () => (await undoNow()).startsWith("Undo splitting a Hats clip")), `Undo names it: ${await undoNow()}`);
n = posts.length;
await key("Meta+j");
check(await until(async () => starts(await shown("hats", "hits")) === "0"), "⌘J joins them into one");
const joined = (await shown("hats", "hits"))[0];
check(joined.id === H && joined.length === 32 && JSON.stringify(joined.notes) === hatsBefore, `keeping the first's id and every note in time: ${joined.id}, ${joined.length}`);
check(lastGesture(n).map((g) => g.op).join() === "join", "one join gesture");

// ---------------------------------------------------------------- ⌘C ⌘V and ⌘X at the playhead
await playAt(40);
await clickClip("kick.hits", K);
await key("Meta+c");
n = posts.length;
await key("Meta+v");
check(await until(async () => starts(await shown("kick", "hits")) === "0,40"), `⌘C then ⌘V puts a copy at the playhead, on the clip's own track: ${starts(await shown("kick", "hits"))}`);
const pasted = (await shown("kick", "hits")).find((c) => c.id !== K);
check(lastGesture(n).map((g) => g.op).join() === "paste" && JSON.stringify(pasted.notes) === JSON.stringify((await shown("kick", "hits"))[0].notes), "as one paste gesture, with the same notes and a new id");
await until(async () => (await pickedIn("kick.hits")).join() === pasted.id);
await key("Meta+x");
check(await until(async () => starts(await shown("kick", "hits")) === "0"), "⌘X cuts the pasted clip");
await page.click('[data-module-tab="hats"]');
await key("Meta+v");
check(await until(async () => starts(await shown("hats", "hits")) === "0,40"), `with Hats selected, ⌘V pastes the Kick clip onto the Hats lane: ${starts(await shown("hats", "hits"))}`);
// In the app the menu bar's Copy runs the web view's command: the page takes its events (M7).
await clickClip("keys.chords", JSON.parse(fixture("data/keys/chords.json")).clips[0].id);
const events = await page.evaluate(() => {
  const fire = (type) => {
    const e = new Event(type, { bubbles: true, cancelable: true });
    document.body.dispatchEvent(e);
    return e.defaultPrevented;
  };
  return { before: fire("beforecopy"), copy: fire("copy"), beforepaste: fire("beforepaste") };
});
check(events.before && events.copy && events.beforepaste, `with a clip selected the page takes beforecopy, copy and beforepaste, so the app's Copy and Paste are on: ${JSON.stringify(events)}`);
await playAt(36);
await page.click('[data-module-tab="keys"]');
await key("Meta+v");
check(await until(async () => starts(await shown("keys", "chords")) === "0,36"), `and that copy event copied the Keys clip: ⌘V pastes it at bar 10 (${starts(await shown("keys", "chords"))})`);

// ---------------------------------------------------------------- Q quantizes; ← → nudge
const loose = JSON.parse(fixture("data/kick/hits.json"));
loose.clips[0].notes = loose.clips[0].notes.map((x, i) => ({ ...x, start: x.start + (i % 2 ? 0.06 : -0.04 * (x.start > 0)) }));
write("data/kick/hits.json", JSON.stringify(loose, null, 2));
await until(async () => Math.abs((await shown("kick", "hits"))[0].notes[1]?.start - 1.06) < 1e-9);
await clickClip("kick.hits", K);
n = posts.length;
await key("q");
check(await until(async () => (await shown("kick", "hits"))[0].notes.every((x) => Number.isInteger(x.start))), "Q quantizes the selected clip's notes onto the roll's grid (1/32)");
const q = lastGesture(n)[0];
check(q?.op === "quantize" && q.grid === 0.125 && (await until(async () => (await undoNow()).startsWith("Undo quantizing a Kick clip to 1/32"))), `as one gesture, named in Undo: ${JSON.stringify({ op: q?.op, grid: q?.grid })}; ${await undoNow()}`);
await clickClip("hats.hits", H);
await key("ArrowRight");
check(await until(async () => (await shown("hats", "hits")).find((c) => c.id === H)?.start === 0.5), "→ moves the selected clip by the grid (an eighth at this zoom)");
await key("Shift+ArrowRight");
check(await until(async () => (await shown("hats", "hits")).find((c) => c.id === H)?.start === 4.5), "⇧→ by a bar");
await key("ArrowLeft");
await key("ArrowLeft");
await key("ArrowLeft");
check(await until(async () => (await shown("hats", "hits")).find((c) => c.id === H)?.start === 3), `three quick ← move it on from where each left it: ${(await shown("hats", "hits")).find((c) => c.id === H)?.start}`);

// ---------------------------------------------------------------- the lane menu
await page.mouse.click(kickRow.x + 34.3 * PPB, kickRow.y + kickRow.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
const laneMenu = await menuRows();
check(laneMenu.join(" | ") === "Add clip | Paste here", `a right-click on a lane's empty space: ${laneMenu.join(" | ")}`);
await page.click('[data-testid=clip-menu] [data-menu-item="paste"]');
check(await until(async () => starts(await shown("kick", "hits")) === "0,34.5"), `Paste here puts the copied Keys clip on the Kick lane where the click was, on the grid: ${starts(await shown("kick", "hits"))}`);

// ---------------------------------------------------------------- ⌘E in the code editor stays the editor's
// The Kick clip selected, under the playhead: ⌘E would split it anywhere else.
await playAt(1);
await clickClip("kick.hits", K);
await view("code");
await page.waitForSelector("[data-testid=code] .monaco-editor .view-lines");
await page.click("[data-testid=code] .monaco-editor .view-lines");
n = posts.length;
const kickNow = read("data/kick/hits.json");
await key("Meta+e");
await sleep(400);
check(lastGesture(n).length === 0 && read("data/kick/hits.json") === kickNow, "in the code editor ⌘E is the editor's: the selected clip isn't split");
await view("notes");

// ---------------------------------------------------------------- the ⌥K sheet
await page.click("[data-testid=timeline]");
await key("Alt+k");
await page.waitForSelector("[data-testid=shortcuts]");
const sheet3 = Object.fromEntries(await page.$$eval("[data-testid=shortcuts] dl > div", (els) => els.map((e) => [e.querySelector("dt").textContent, e.querySelector("dd").textContent])));
const want3 = { "Split clips at the playhead": "⌘E", "Join clips": "⌘J", "Quantize the selected clips": "Q", "Cut / copy / paste clips": "⌘X / ⌘C / ⌘V", "Move clips by the grid / by a bar": "← → / ⇧← ⇧→" };
check(Object.entries(want3).every(([k, v]) => sheet3[k] === v), `the shortcuts sheet lists the clip commands: ${Object.keys(want3).map((k) => `${k} = ${sheet3[k]}`).join("; ")}`);
await key("Escape");

// ---------------------------------------------------------------- a PC page says Ctrl
const pc = await open(1440, 900, { pc: true });
await pc.page.waitForSelector("[data-testid=song-grid]");
const pcWords = await pc.page.evaluate(() => [document.querySelector("[data-testid=song-grid]").title, document.querySelector("[data-testid=song-snap]").title].join(" | "));
check(pcWords.includes("Ctrl+1") && pcWords.includes("Ctrl+4") && pcWords.includes("hold Ctrl") && !pcWords.includes("⌘"), `a PC page's tooltips say Ctrl: ${pcWords.replace(/\n/g, " ")}`);
const clipWords = await pc.page.$eval('[data-lane="kick.hits"] [data-clip]', (e) => e.title);
check(clipWords.includes("Alt-drag to copy") && clipWords.includes("drag an edge to resize"), `and a clip's tooltip says Alt-drag: ${clipWords.replace(/\n/g, " | ")}`);
const pcClip = await pc.page.$('[data-lane="kick.hits"] [data-clip]').then((e) => e.boundingBox());
await pc.page.mouse.click(pcClip.x + 20, pcClip.y + pcClip.height / 2, { button: "right" });
await pc.page.waitForSelector("[data-testid=clip-menu]");
const pcMenu = await pc.page.$$eval("[data-testid=clip-menu] [role=menuitem]", (els) => els.map((e) => e.textContent).join(" | "));
check(pcMenu.includes("Split at playheadCtrl+E") && pcMenu.includes("CutCtrl+X") && pcMenu.includes("DeleteBackspace") && !pcMenu.includes("⌘"), `and its clip menu says Ctrl+E: ${pcMenu}`);
await pc.browser.close();

await browser.close();
done([...errors, ...pc.errors]);
