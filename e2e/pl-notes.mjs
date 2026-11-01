// Polish lane D, D1 and D2 (the polish plan §6): the roll's right-click menu and its note tools
// (transpose, quantize, legato, humanize, swing and the grooves since track notes, duplicate,
// delete), Q in the roll, ⌥← and ⌥→
// left to the page, a Ctrl-click on a Mac that opens the menu and moves nothing, ⌘= and ⌘− zoom
// the roll while it has the keyboard (X3); the song's key
// in the top bar, written into layout.json, and the roll's rows out of the key shaded. A PC page's
// words, and the taste lint over the menu and the key picker.
//
//   node e2e/pl-notes.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint as lintIn } from "./lintlib.mjs";

const { browser, page, errors, api, state, caughtUp, box, key } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

const FILE = "data/bass/notes.json";
const ROW = 14;
/** The taste lint over what is inside `sel` (e2e/lintlib.mjs, as taste.mjs runs it). */
const lint = (sel) => page.evaluate(lintIn, sel);

/** The Bass clip's notes as the server has them (pitches as numbers). */
const bassClip = async () => (await state()).timeline.tracks.find((t) => t.module === "bass").lanes.find((l) => l.key === "notes").clips[0];
/** Write the Bass clip's notes through the lane route, as an agent would, and wait for the roll to show them. */
async function setBass(notes) {
  // An undo just made may still be landing: planned on the revision before it, the write is
  // refused as stale, and made again on the new one.
  for (let tries = 0; ; tries++) {
    await sleep(200);
    const st = await state();
    const c = st.timeline.tracks.find((t) => t.module === "bass").lanes.find((l) => l.key === "notes").clips[0];
    const r = await post("/lane", { session: st.session, revision: st.view.revision, module: "bass", lane: "notes", clips: [{ id: c.id, start: c.start, length: c.length, notes }] });
    if (r.status === 200) break;
    if (tries === 5 || !/changed since/.test(r.body.error ?? "")) throw new Error(`lane write: ${r.status} ${JSON.stringify(r.body)}`);
  }
  await until(async () => (await page.$$("[data-testid=notes] [data-note]")).length === notes.length);
  await sleep(150);
  // The roll writes the lane as the page shows it: the page shows this write first.
  await caughtUp();
  return read(FILE);
}
const revision = async () => (await state()).view.revision;
/** The roll's notes, as it shows them. */
const shown = () =>
  page.$$eval("[data-testid=notes] [data-note]", (els) =>
    els.map((e) => ({ i: Number(e.dataset.note), pitch: Number(e.dataset.pitch), start: Number(e.dataset.start), length: Number(e.dataset.length), velocity: Number(e.dataset.velocity), selected: e.dataset.selected === "true" })),
  );
/** Where to press note `i`: a few px into it, scrolled into view. */
async function noteAt(i, p = page) {
  const el = await p.$(`[data-testid=notes] [data-note="${i}"]`);
  if (!el) return null;
  await el.evaluate((e) => e.scrollIntoView({ block: "center", inline: "nearest" }));
  await sleep(80);
  const b = await el.boundingBox();
  return { x: b.x + Math.min(5, b.width / 2), y: b.y + b.height / 2 };
}
/** A place on empty grid, in view, where no note is: rows from the middle of the roll out, from the left. */
async function emptyAt(p = page) {
  return p.evaluate((ROW) => {
    const s = document.querySelector("[data-testid=roll-scroller]").getBoundingClientRect();
    const mid = (s.top + 34 + s.bottom) / 2;
    for (let k = 0; k < 40; k++) {
      const y = mid + (k % 2 ? 1 : -1) * Math.ceil(k / 2) * ROW;
      if (y < s.top + 36 || y > s.bottom - 6) continue;
      for (let x = s.left + 64; x < s.right - 10; x += 7) {
        const hit = document.elementFromPoint(x, y);
        if (hit && hit.closest("[data-testid=roll-grid]") && !hit.closest("[data-note]")) return { x, y };
      }
    }
    return null;
  }, ROW);
}
const menu = () => page.$("[data-testid=note-menu]");
const heading = () => page.$eval("[data-testid=note-menu-heading]", (e) => e.textContent).catch(() => null);
/** The menu's rows: id, label, shortcut, disabled, tooltip. */
const rows = (p = page) =>
  p.$$eval("[data-testid=note-menu] [data-menu-item]", (els) =>
    els.map((e) => ({
      id: e.dataset.menuItem,
      label: e.querySelector("span > span")?.textContent ?? e.textContent,
      shortcut: e.querySelector(":scope > span.shrink-0.text-xs")?.textContent ?? "",
      disabled: e.disabled ?? false,
      title: e.title,
    })),
  );
const pick = async (id) => {
  await page.click(`[data-testid=note-menu] [data-menu-item="${id}"]`);
  await until(async () => !(await menu()), 2000);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
async function undoTo(text, what) {
  await key("Meta+z");
  check(await until(() => read(FILE) === text), `${what}: ⌘Z puts ${FILE} back byte for byte`);
  // The roll's next edit goes from the revision the page shows: the undo's, once it shows it.
  await caughtUp();
}
/** One write: the revision moves by one, and stays. */
async function oneStep(rev) {
  await until(async () => (await revision()) > rev);
  await sleep(400);
  return (await revision()) === rev + 1;
}

// ---------------------------------------------------------------- the menu on a note (D1)

// Open Bass's notes clip in Notes.
const clip = await box('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await page.mouse.click(clip.x + 60, clip.y + clip.height / 2, { count: 2 });
check(await until(async () => (await page.$$("[data-testid=notes] [data-note]")).length === 48), "double-clicking Bass's notes clip opens it in Notes, 48 notes");
const original = read(FILE);
const gridTitle = await page.$eval("[data-testid=roll-grid]", (e) => e.title);
check(gridTitle.includes("right-click for note tools"), `the grid's tooltip says right-click for note tools: ${gridTitle.slice(-60)}`);

let n = await noteAt(3);
const before3 = (await shown())[3];
await page.mouse.click(n.x, n.y, { button: "right" });
check(await until(() => menu().then(Boolean), 2000), "right-click on a note opens its menu");
check((await heading()) === "1 note", `its first line says what it acts on: ${await heading()}`);
check(await until(async () => (await shown()).filter((x) => x.selected).map((x) => x.i).join() === "3"), "the note is selected, alone");
const r1 = await rows();
const want = [
  ["up", "Up a semitone", "↑"],
  ["down", "Down a semitone", "↓"],
  ["octave_up", "Up an octave", "⇧↑"],
  ["octave_down", "Down an octave", "⇧↓"],
  ["quantize", "Quantize to 1/16", "Q"],
  ["legato", "Legato", ""],
  ["humanize", "Humanize", ""],
  ["swing", "Swing", ""],
  ["groove-laid-back", "Laid back", ""],
  ["groove-pushed", "Pushed", ""],
  ["groove-accents", "Accents", ""],
  ["duplicate", "Duplicate", "⌘D"],
  ["delete", "Delete", "⌫"],
];
const got = r1.map((r) => [r.id, r.id === "swing" ? "Swing" : r.label, r.shortcut]);
check(JSON.stringify(got) === JSON.stringify(want), `its rows, in order, with their keys: ${JSON.stringify(got)}`);
const swings = await page.$$eval("[data-testid=note-menu] [data-swing]", (els) => els.map((e) => e.textContent));
check(swings.join() === "50%,55%,60%,66%,71%", `Swing is a row of five amounts, the server's (NOTE-7): ${swings}`);
const tips = Object.fromEntries(r1.map((r) => [r.id, r.title]));
check(
  tips.quantize === "Move the notes onto the grid" &&
    tips.legato === "Make each note last until the next one starts" &&
    tips.humanize === "Change timing and loudness a little, so it sounds played" &&
    tips.swing === "Push every second note a little later, for a bouncy feel",
  `each tool's tooltip says what it does in plain words: ${JSON.stringify([tips.quantize, tips.legato, tips.humanize, tips.swing])}`,
);
await sleep(400);
await page.screenshot({ path: `${ROOT}/pl-notes-menu.png` });
const bad = await lint("[data-testid=note-menu]");
check(bad.length === 0, `the menu passes the taste lint${bad.length ? `: ${[...new Set(bad)].slice(0, 5).join("; ")}` : ""}`);

// Up an octave: that note only, +12, one undo step named for it.
let rev = await revision();
await pick("octave_up");
check(await until(async () => (await bassClip()).notes.filter((x, i) => i !== 3).length === 47 && (await shown())[3]?.pitch === before3.pitch + 12), `Up an octave moves the note from ${before3.pitch} to ${before3.pitch + 12}`);
// The roll draws the move at once; the server's state may land a moment later.
const raised = (c) => c.notes.length === 48 && c.notes.some((x) => x.pitch === before3.pitch + 12 && x.start === before3.start);
check(await until(async () => raised(await bassClip())), "the file has it an octave up");
check(await oneStep(rev), "one write, so one undo step");
check(await until(async () => (await undoTitle()).startsWith("Undo transposing Bass notes")), `Undo names it: ${await undoTitle()}`);
check(await page.evaluate(() => !!document.activeElement?.closest("[data-testid=notes]")), "the roll keeps the keyboard after a row");
await undoTo(original, "Up an octave");

// Escape closes the menu; the roll keeps the keyboard and the selection.
n = await noteAt(3);
await page.mouse.click(n.x, n.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
await page.keyboard.press("Escape");
check(await until(async () => !(await menu()), 2000), "Esc closes the menu");
check(
  (await page.evaluate(() => document.activeElement?.dataset.testid)) === "notes" && (await shown()).some((x) => x.selected),
  "the focus is back in the roll, and the note still selected (Esc closed only the menu)",
);

// ---------------------------------------------------------------- empty grid: every note

await page.keyboard.press("Escape");
await until(async () => !(await shown()).some((x) => x.selected));
let e = await emptyAt();
await page.mouse.click(e.x, e.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
check((await heading()) === "All 48 notes", `right-click on empty grid with nothing selected: "${await heading()}"`);
const r2 = await rows();
check(r2.filter((r) => ["up", "down", "octave_up", "octave_down", "duplicate", "delete"].includes(r.id)).every((r) => r.shortcut === ""), "with no note selected the arrow, ⌘D and ⌫ keys aren't shown: they act only on selected notes");
check(r2.find((r) => r.id === "quantize")?.shortcut === "Q", "Q is: it acts on every note");
rev = await revision();
const all0 = await bassClip();
await pick("down");
check(
  await until(async () => {
    const now = await bassClip();
    return now.notes.length === 48 && now.notes.every((x, i) => x.pitch === all0.notes[i].pitch - 1 && x.start === all0.notes[i].start);
  }),
  "Down a semitone moves every note down one",
);
check(await oneStep(rev), "as one undo step");
await undoTo(original, "Down a semitone on every note");

// ---------------------------------------------------------------- quantize (Q)

const offGrid = [
  { pitch: 33, start: 0.07, length: 0.25, velocity: 0.8 },
  { pitch: 36, start: 0.31, length: 0.25, velocity: 0.8 },
  { pitch: 40, start: 1.12, length: 0.5, velocity: 0.8 },
  { pitch: 38, start: 2.9, length: 0.25, velocity: 0.8 },
];
let base = await setBass(offGrid);
await page.focus("[data-testid=notes]");
rev = await revision();
await page.keyboard.press("q");
const onGrid = (x) => Math.abs(x / 0.25 - Math.round(x / 0.25)) < 1e-6;
check(await until(async () => (await bassClip()).notes.every((x) => onGrid(x.start))), `Q with nothing selected puts every start on 1/16: ${(await bassClip()).notes.map((x) => x.start)}`);
check(JSON.stringify((await bassClip()).notes.map((x) => x.start)) === "[0,0.25,1,3]" && (await bassClip()).notes.every((x, i) => x.length === offGrid[i].length), "each to its nearest line, lengths kept");
check(await oneStep(rev), "one undo step");
check(await until(async () => (await undoTitle()).startsWith("Undo quantizing Bass notes to 1/16")), `Undo names it: ${await undoTitle()}`);
await undoTo(base, "Q");

// ---------------------------------------------------------------- legato

base = await setBass([
  { pitch: 33, start: 0, length: 0.25, velocity: 0.8 },
  { pitch: 40, start: 0, length: 0.5, velocity: 0.8 },
  { pitch: 36, start: 1, length: 0.25, velocity: 0.8 },
  { pitch: 38, start: 2.5, length: 0.25, velocity: 0.8 },
]);
e = await emptyAt();
await page.mouse.click(e.x, e.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
await pick("legato");
check(
  await until(async () => JSON.stringify((await bassClip()).notes.map((x) => [x.start, x.length])) === "[[0,1],[0,1],[1,1.5],[2.5,0.25]]"),
  `Legato: each note lasts until the next starts, a chord's notes together, the last unchanged: ${JSON.stringify((await bassClip()).notes.map((x) => [x.start, x.length]))}`,
);
await undoTo(base, "Legato");

// ---------------------------------------------------------------- humanize

const run = Array.from({ length: 8 }, (_, k) => ({ pitch: 33 + (k % 3), start: k * 0.5, length: 0.25, velocity: 0.8 }));
base = await setBass(run);
const bpm = (await state()).timeline.tempo[0].bpm;
e = await emptyAt();
await page.mouse.click(e.x, e.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
await pick("humanize");
check(await until(() => read(FILE) !== base), "Humanize changes the notes");
const human = (await bassClip()).notes;
const ms = (beats) => (beats * 60000) / bpm;
const moved = human.map((h) => Math.min(...run.filter((r) => r.pitch === h.pitch).map((r) => Math.abs(ms(h.start - r.start)))));
check(human.length === 8 && moved.every((m) => m <= 10.001), `every start moved at most 10 ms at ${bpm} BPM: ${moved.map((m) => m.toFixed(1)).join(" ")}`);
check(human.every((h) => h.velocity >= 0.72 - 1e-9 && h.velocity <= 0.88 + 1e-9), `and every velocity within 8% of 80%: ${human.map((h) => h.velocity).join(" ")}`);
check(human.every((h) => h.length === 0.25), "lengths kept");
await undoTo(base, "Humanize");

// ---------------------------------------------------------------- swing

const sixteenths = Array.from({ length: 8 }, (_, k) => ({ pitch: 33, start: k * 0.25, length: 0.25, velocity: 0.8 }));
base = await setBass(sixteenths);
e = await emptyAt();
await page.mouse.click(e.x, e.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
rev = await revision();
await page.click('[data-testid=note-menu] [data-swing="60"]');
const swungStarts = [0, 0.3, 0.5, 0.8, 1, 1.3, 1.5, 1.8];
check(
  await until(async () => JSON.stringify((await bassClip()).notes.map((x) => x.start)) === JSON.stringify(swungStarts)),
  `Swing 60% on 1/16 notes: the second of each pair 0.05 of a beat later, the first where it was: ${(await bassClip()).notes.map((x) => x.start)}`,
);
check((await bassClip()).notes.every((x, i) => Math.abs(x.start + x.length - sixteenths[i].start - 0.25) < 1e-9), "each moved note keeps its end, so it never runs into the next");
check(await oneStep(rev), "one undo step");
check(await until(async () => (await undoTitle()).startsWith("Undo swinging Bass notes")), `Undo names it: ${await undoTitle()}`);
const once = read(FILE);
e = await emptyAt();
await page.mouse.click(e.x, e.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
rev = await revision();
await page.click('[data-testid=note-menu] [data-swing="60"]');
await sleep(800);
check(read(FILE) === once && (await revision()) === rev, "Swing 60% twice is the same as once: it sets notes to their swung place");
e = await emptyAt();
await page.mouse.click(e.x, e.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
await page.click('[data-testid=note-menu] [data-swing="55"]');
check(
  await until(async () => JSON.stringify((await bassClip()).notes.map((x) => x.start)) === "[0,0.275,0.5,0.775,1,1.275,1.5,1.775]"),
  `55% after 60% puts them at 55%'s place: ${(await bassClip()).notes.map((x) => x.start)}`,
);
await key("Meta+z");
await undoTo(base, "Swing");

// ---------------------------------------------------------------- the selection, duplicate and delete

base = await setBass(sixteenths.slice(0, 4).map((x, k) => ({ ...x, pitch: 33 + k })));
await page.focus("[data-testid=notes]");
await key("Meta+a");
await until(async () => (await shown()).every((x) => x.selected));
// Shift-click two notes out of the selection: notes 0 and 1 stay selected.
for (const i of [2, 3]) {
  const p = await noteAt(i);
  await page.keyboard.down("Shift");
  await page.mouse.click(p.x, p.y);
  await page.keyboard.up("Shift");
}
check(await until(async () => (await shown()).filter((x) => x.selected).map((x) => x.i).join() === "0,1"), "two of four notes selected");
n = await noteAt(1);
await page.mouse.click(n.x, n.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
check((await heading()) === "2 notes", `right-click on a selected note keeps the selection: "${await heading()}"`);
await pick("up");
check(await until(async () => (await bassClip()).notes.map((x) => x.pitch).join() === "34,35,35,36"), `Up a semitone moves only the selected notes: ${(await bassClip()).notes.map((x) => x.pitch)}`);
await undoTo(base, "Up a semitone on two notes");
await until(async () => (await shown()).filter((x) => x.selected).length === 2);
n = await noteAt(0);
await page.mouse.click(n.x, n.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
await pick("duplicate");
check(await until(async () => (await bassClip()).notes.length === 6), "Duplicate copies the selected notes");
await undoTo(base, "Duplicate");
// A click on empty grid selects nothing.
e = await emptyAt();
await page.mouse.click(e.x, e.y);
await until(async () => !(await shown()).some((x) => x.selected));
await page.mouse.click(e.x, e.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
await pick("delete");
check(await until(async () => (await bassClip()).notes.length === 0), "Delete from empty grid, nothing selected: every note goes");
await undoTo(base, "Delete every note");

// ---------------------------------------------------------------- ⌥← and ⌥→ are the page's (X1); Ctrl-click on a Mac (X7)

await page.focus("[data-testid=notes]");
await key("Meta+a");
await until(async () => (await shown()).every((x) => x.selected));
rev = await revision();
await key("Alt+ArrowRight");
await key("Alt+ArrowLeft");
await sleep(600);
check(read(FILE) === base && (await revision()) === rev, "⌥→ and ⌥← with notes selected move no note: they are the markers' keys");
await page.keyboard.press("Escape");
n = await noteAt(2);
rev = await revision();
await page.keyboard.down("Control");
await page.mouse.move(n.x, n.y);
await page.mouse.down();
await page.mouse.move(n.x + 60, n.y - 30, { steps: 8 });
await page.mouse.up();
await page.keyboard.up("Control");
await sleep(600);
check(read(FILE) === base && (await revision()) === rev, "a Ctrl-drag on a note on a Mac moves nothing: Ctrl-click is a right-click there");

// ⌘= and ⌘− zoom the roll while it has the keyboard, and leave the timeline (X3, review PL-T3).
await page.focus("[data-testid=notes]");
const widths = async () => ({
  note: await page.$eval('[data-testid=notes] [data-note="0"]', (e) => e.getBoundingClientRect().width),
  clip: await page.$eval('[data-lane="bass.notes"] [data-clip]', (e) => e.getBoundingClientRect().width),
});
const z0 = await widths();
await key("Meta+Equal");
await sleep(400);
const z1 = await widths();
await key("Meta+Minus");
await sleep(400);
const z2 = await widths();
// A note is drawn 1 px short of its length, so its length is its width + 1.
check(Math.abs((z1.note + 1) / (z0.note + 1) - 1.5) < 0.02 && Math.abs(z1.clip - z0.clip) < 0.5, `with the roll focused, ⌘= makes its notes 1.5 times longer and leaves the timeline: note ${z0.note} → ${z1.note} px, clip ${z0.clip} → ${z1.clip} px`);
check(Math.abs(z2.note - z0.note) < 0.5 && Math.abs(z2.clip - z0.clip) < 0.5, `and ⌘− puts it back: note ${z2.note} px, clip ${z2.clip} px`);

// ---------------------------------------------------------------- read only

const bassTs = read("tracks/bass.ts");
write("tracks/bass.ts", `${bassTs}\n(`);
check(await until(() => page.$("[data-testid=notes-locked]").then(Boolean)), "with the code broken the roll is read only");
n = await noteAt(1);
await page.mouse.click(n.x, n.y, { button: "right" });
check(await until(() => menu().then(Boolean), 2000), "the menu still opens");
const r3 = await rows();
check(r3.length === 13 && r3.filter((r) => r.id !== "swing").every((r) => r.disabled && r.title.includes("compiles")), `its rows are disabled, the tooltip saying why: ${r3[0]?.title}`);
check(await page.$$eval("[data-testid=note-menu] [data-swing]", (els) => els.length === 5 && els.every((b) => b.disabled)), "Swing's five too");
await page.keyboard.press("Escape");
write("tracks/bass.ts", bassTs);
check(await until(async () => !(await page.$("[data-testid=notes-locked]"))), "once it compiles, the roll is editable again");

// ---------------------------------------------------------------- the song's key (D2)

const LAYOUT = "layout.json";
const layoutKey = () => {
  try {
    return JSON.parse(read(LAYOUT)).key ?? null;
  } catch {
    return null;
  }
};
const field = () => page.$eval("[data-testid=key-field]", (e) => ({ text: e.textContent, title: e.title, color: getComputedStyle(e).color })).catch(() => null);
const picker = () => page.$("[data-testid=key-picker]");
const shading = () => page.$eval("[data-testid=roll-grid]", (e) => e.dataset.shaded).catch(() => null);
/** Whether the roll draws row `pitch` shaded: its band in the grid's twelve-row pattern. */
const drawnShaded = (pitch) =>
  page.$eval(
    "[data-testid=roll-grid] svg pattern",
    (pat, pitch, ROW) => {
      const k = (127 - pitch) % 12;
      return [...pat.querySelectorAll("rect")].some((r) => Number(r.getAttribute("y")) === k * ROW && Number(r.getAttribute("height")) === ROW);
    },
    pitch,
    ROW,
  );

const f0 = await field();
check(!!f0, "the top bar has a Key field");
check(f0?.text === "Key" && f0.title.startsWith("The song's key. In Notes, rows not in the key are shaded. Saved in layout.json."), `it says "Key" while none is set, its tooltip what it does: ${f0?.text} · ${f0?.title.split("\n")[0]}`);
const order = await page.evaluate(() => {
  const r = (s) => document.querySelector(s)?.getBoundingClientRect();
  return { meter: r("[data-testid=meter-field]")?.right, key: r("[data-testid=key-field]")?.left };
});
check(order.key > order.meter && order.key - order.meter < 12, `it sits right after the time signature (${order.meter?.toFixed(0)} → ${order.key?.toFixed(0)})`);
check(read(FILE) === base && (await shading()) === "1,3,6,8,10" && (await drawnShaded(66)) && !(await drawnShaded(65)), "with no key, the roll shades the black keys' rows: F♯ (66) shaded, F (65) plain");

await page.click("[data-testid=key-field]");
check(await until(() => picker().then(Boolean), 2000), "a click opens the key picker");
const roots = await page.$$eval("[data-testid=key-picker] [data-root]", (els) => els.map((e) => e.textContent));
check(roots.join(" ") === "C C♯ D E♭ E F F♯ G A♭ A B♭ B", `the twelve roots, spelled with ♯ and ♭: ${roots.join(" ")}`);
const rows2 = await page.$eval("[data-testid=key-picker] [data-root=C]", (c) => {
  const f = document.querySelector("[data-testid=key-picker] [data-root=\"F#\"]");
  return { top: c.getBoundingClientRect().top, ftop: f.getBoundingClientRect().top };
});
check(rows2.ftop > rows2.top, "in two rows of six: F♯ starts the second");
const scales = await page.$$eval("[data-testid=key-picker] [data-scale]", (els) => els.map((e) => `${e.textContent}:${e.getAttribute("aria-checked")}`));
const allScales = ["Major", "Minor", "Dorian", "Phrygian", "Lydian", "Mixolydian", "Locrian", "Harmonic minor", "Melodic minor", "Major pentatonic", "Minor pentatonic", "Blues", "Chromatic"];
check(
  scales.join() === allScales.map((x, i) => `${x}:${i === 0}`).join() && (await page.$("[data-testid=no-key][disabled]")) !== null,
  `the table's thirteen scales (NOTE-1), Major first and picked, and No key, off while there is none: ${scales}`,
);
await sleep(400);
await page.screenshot({ path: `${ROOT}/pl-notes-key-picker.png` });
const badKey = await lint("[data-testid=key-picker]");
check(badKey.length === 0, `the picker passes the taste lint${badKey.length ? `: ${[...new Set(badKey)].slice(0, 5).join("; ")}` : ""}`);
await page.keyboard.press("Escape");
check(await until(async () => !(await picker()), 2000), "Esc closes it");
check(await page.evaluate(() => document.activeElement?.dataset.testid === "notes"), "and the keyboard is where it was: the roll");

// E minor: F♯ is in it, F isn't, the other way round from the black keys.
await page.click("[data-testid=key-field]");
await until(() => picker().then(Boolean), 2000);
await page.click('[data-testid=key-picker] [data-scale="minor"]');
check(!(await layoutKey()), "Minor alone writes nothing while no key is set");
rev = await revision();
await page.click('[data-testid=key-picker] [data-root="E"]');
check(await until(() => JSON.stringify(layoutKey()) === JSON.stringify({ root: "E", scale: "minor" })), `E minor is written into layout.json: ${read(LAYOUT).replace(/\s+/g, " ")}`);
check(await until(async () => !(await picker()), 2000), "a root closes the picker");
check(await until(async () => (await field())?.text === "E minor"), `the field says "${(await field())?.text}"`);
check((await revision()) === rev && read(FILE) === base, "nothing that plays changed: no revision, the notes as they were");
check(await until(async () => (await shading()) === "1,3,5,8,10" && (await drawnShaded(65)) && !(await drawnShaded(66))), `the roll shades the rows out of E minor: F (65) shaded, F♯ (66) plain (${await shading()})`);
check(await until(async () => (await undoTitle()).startsWith("Undo key E minor")), `Undo names it: ${await undoTitle()}`);
await page.screenshot({ path: `${ROOT}/pl-notes-e-minor.png` });

// With a key set, the scale changes at once, and the picker stays open for a root.
await page.click("[data-testid=key-field]");
await until(() => picker().then(Boolean), 2000);
await page.click('[data-testid=key-picker] [data-scale="major"]');
check(await until(() => layoutKey()?.scale === "major" && layoutKey()?.root === "E"), "Major with E minor set writes E major");
check(!!(await picker()), "and leaves the picker open");
await page.click('[data-testid=key-picker] [data-root="Bb"]');
check(await until(async () => (await field())?.text === "B♭ major" && layoutKey()?.root === "Bb"), `B♭ major: the file says Bb, the field ${(await field())?.text}`);

// No key: the black keys' shading again, and the file without it.
await page.click("[data-testid=key-field]");
await until(() => picker().then(Boolean), 2000);
await page.click("[data-testid=no-key]");
check(await until(() => !("key" in JSON.parse(read(LAYOUT)))), "No key takes it out of layout.json");
check(await until(async () => (await shading()) === "1,3,6,8,10" && (await field())?.text === "Key"), "the roll shades the black keys again; the field says Key");
await key("Meta+z");
check(await until(() => layoutKey()?.root === "Bb"), "⌘Z puts B♭ major back");
await key("Meta+z");
await key("Meta+z");
check(await until(() => JSON.stringify(layoutKey()) === JSON.stringify({ root: "E", scale: "minor" })), "two more: E minor");
await key("Meta+z");
check(await until(async () => !layoutKey() && (await field())?.text === "Key"), "and one more, no key");

// A key written into layout.json by hand (a merge, an agent) shows too.
write(LAYOUT, JSON.stringify({ order: [], tracks: {}, key: { root: "C", scale: "major" } }));
check(await until(async () => (await field())?.text === "C major" && (await shading()) === "1,3,6,8,10"), "a key written into layout.json by hand shows: C major, whose rows out of it are the black keys'");

// The name keeps clear of the bar's groups as measured (Q23): a long name, with a key set, at the
// app's smallest window, laid out as the Mac app's.
const st0 = await state();
const longName = "A very long song name that cannot fit in the top bar at all";
check((await post("/title", { session: st0.session, name: longName })).status === 200, "a long name");
write(LAYOUT, JSON.stringify({ order: [], tracks: {}, key: { root: "F#", scale: "minor" } }));
await until(async () => (await field())?.text === "F♯ minor");
const room = async () =>
  page.evaluate(() => {
    const r = (s) => document.querySelector(s)?.getBoundingClientRect();
    const name = document.querySelector("[data-testid=bar-song-name]");
    const box = name?.parentElement?.getBoundingClientRect();
    const time = r("span[title=Time]");
    const left = Math.max(r("[data-testid=tempo]").right, time?.width ? time.right : 0);
    const right = Math.min(r("[data-testid=export]").left, r('button[aria-label="Undo"]').left);
    return { box: box && [box.left, box.right], left, right, shown: !!box && box.width > 0, cut: name.scrollWidth > name.clientWidth };
  });
for (const mac of [false, true]) {
  await page.evaluate((on) => (on ? (document.documentElement.dataset.app = "mac") : delete document.documentElement.dataset.app), mac);
  for (const w of [1100, 1280, 1440]) {
    await page.setViewport({ width: w, height: 900 });
    await sleep(300);
    const b = await room();
    check(!b.shown || (b.box[0] >= b.left + 4 && b.box[1] <= b.right - 4), `${w}px${mac ? " in the Mac app" : ""}: the name (${b.box?.map((x) => x.toFixed(0)).join("–")}) clears the key (…${b.left.toFixed(0)}) and Undo (${b.right.toFixed(0)}…)${b.shown ? ", cut short" : ", or goes"}`);
  }
}
await page.evaluate(() => delete document.documentElement.dataset.app);
await page.setViewport({ width: 1440, height: 900 });
// The name's room is measured again a frame later: typing must not begin in the Mac app's room,
// which the Record button (studio lane B) makes too small for the field at 1440.
await sleep(300);
const wide = await room();
check(wide.shown && wide.cut, "at 1440 it shows, cut short");
// While the name is being typed it stays, even when its room goes for a moment (a status pill
// shows in a small window, say): what was typed half way is never written by it going.
const nb = await box("[data-testid=bar-song-name]");
await page.mouse.click(nb.x + nb.width / 2, nb.y + nb.height / 2, { count: 2 });
await page.waitForSelector("[data-testid=song-title-input]", { timeout: 2000 }).catch(() => null);
await page.keyboard.type("Half typed");
const typedBox = () =>
  page.$eval("[data-testid=song-title-input]", (e) => {
    const r = e.getBoundingClientRect();
    return { w: Math.round(r.width), top: document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2) === e };
  });
const typedAt = await typedBox();
await page.evaluate(() => (document.querySelector("[data-testid=tempo]").style.marginRight = "4000px"));
await sleep(300);
const typing = await page.evaluate(() => document.activeElement?.dataset.testid === "song-title-input");
const typedThen = await typedBox().catch(() => null);
await page.evaluate(() => (document.querySelector("[data-testid=tempo]").style.marginRight = ""));
await page.keyboard.press("Escape");
await sleep(300);
check(typing && (await state()).name === longName, `a name being typed stays while its room goes, and nothing is written: ${(await state()).name}`);
// And it stays as wide as when typing began, on top, so what is typed can be seen (review PL-T1).
check(!!typedThen && typedThen.w >= typedAt.w && typedAt.w >= 120 && typedThen.top, `the field keeps its width, on top of its neighbours: ${JSON.stringify(typedAt)} then ${JSON.stringify(typedThen)}`);
await key("Meta+z");
await until(async () => (await page.$eval("[data-testid=bar-song-name]", (e) => e.textContent).catch(() => "")) !== longName);
fs.rmSync(`${PROJ}/${LAYOUT}`);
check(await until(async () => (await field())?.text === "Key"), "layout.json gone: no key");

// ---------------------------------------------------------------- a PC page

{
  const pc = await open(1440, 900, { pc: true });
  const c2 = await pc.box('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
  await pc.page.mouse.click(c2.x + 60, c2.y + c2.height / 2, { count: 2 });
  await until(async () => (await pc.page.$$("[data-testid=notes] [data-note]")).length > 0);
  const p = await noteAt(1, pc.page);
  await pc.page.mouse.click(p.x, p.y, { button: "right" });
  await until(() => pc.page.$("[data-testid=note-menu]").then(Boolean), 2000);
  const keys = Object.fromEntries((await rows(pc.page)).map((r) => [r.id, r.shortcut]));
  check(keys.octave_up === "Shift+↑" && keys.octave_down === "Shift+↓" && keys.duplicate === "Ctrl+D" && keys.delete === "Backspace", `a PC page spells the keys: ${JSON.stringify(keys)}`);
  await pc.page.keyboard.press("Escape");
  await pc.page.click("[data-testid=key-field]");
  const opened = await until(() => pc.page.$("[data-testid=key-picker]").then(Boolean), 2000);
  const mac = await pc.page.evaluate(() => /[⌘⌥⇧⌫]/.test(document.querySelector("[data-testid=key-picker]")?.innerText + document.querySelector("[data-testid=key-field]").title));
  check(opened && !mac, "on a PC page the key picker opens, with no Mac key symbol in it or the field's tooltip");
  check(pc.errors.length === 0, `no console errors on the PC page ${pc.errors.slice(0, 2).join(" | ")}`);
  await pc.browser.close();
}

// ---------------------------------------------------------------- the end

check(read(FILE) === base, "the Bass notes are as the suite last set them");
await browser.close();
done(errors);
