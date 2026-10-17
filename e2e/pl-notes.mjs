// Polish lane D, D1 and D2 (the polish plan §6): the roll's right-click menu and its note tools
// (transpose, quantize, legato, humanize, swing, duplicate, delete), Q in the roll, ⌥← and ⌥→
// left to the page, a Ctrl-click on a Mac that opens the menu and moves nothing; the song's key
// in the top bar, written into layout.json, and the roll's rows out of the key shaded. A PC page's
// words, and the taste lint over the menu and the key picker.
//
//   node e2e/pl-notes.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, api, state, box, key } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

const FILE = "data/bass/notes.json";
const ROW = 14;
/** The taste lint (e2e/taste.mjs) over what is inside `sel`: font sizes, radii, gradients, backdrop filters, mono text. */
const lint = (sel) =>
  page.evaluate((sel) => {
    const SIZES = new Set([11, 12, 13, 15]);
    const RADII = new Set([0, 2, 4, 8]);
    const bad = [];
    const root = document.querySelector(sel);
    if (!root) return [`nothing at ${sel}`];
    for (const e of [root, ...root.querySelectorAll("*")]) {
      if (!e.getClientRects().length) continue;
      const s = getComputedStyle(e);
      const texty = [...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim()) || /^(INPUT|TEXTAREA|SELECT)$/.test(e.tagName);
      if (texty && e.namespaceURI === "http://www.w3.org/1999/xhtml" && !SIZES.has(parseFloat(s.fontSize))) bad.push(`font-size ${s.fontSize}`);
      if (texty && /mono|menlo|courier|sf mono/i.test(s.fontFamily) && !e.closest("[data-code]")) bad.push("mono font");
      for (const c of ["borderTopLeftRadius", "borderTopRightRadius", "borderBottomRightRadius", "borderBottomLeftRadius"]) {
        const v = s[c], r = parseFloat(v);
        if (v.endsWith("%") ? r !== 0 && r < 50 : !RADII.has(r) && r < 9999) bad.push(`border-radius ${v}`);
      }
      if (/gradient/.test(s.backgroundImage)) bad.push("gradient");
      if (s.backdropFilter && s.backdropFilter !== "none") bad.push("backdrop-filter");
    }
    return bad;
  }, sel);

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
  ["duplicate", "Duplicate", "⌘D"],
  ["delete", "Delete", "⌫"],
];
const got = r1.map((r) => [r.id, r.id === "swing" ? "Swing" : r.label, r.shortcut]);
check(JSON.stringify(got) === JSON.stringify(want), `its rows, in order, with their keys: ${JSON.stringify(got)}`);
const swings = await page.$$eval("[data-testid=note-menu] [data-swing]", (els) => els.map((e) => e.textContent));
check(swings.join() === "Light,Medium,Heavy", `Swing is a row of three: ${swings}`);
const tips = Object.fromEntries(r1.map((r) => [r.id, r.title]));
check(
  tips.quantize === "Move the notes onto the grid" &&
    tips.legato === "Make each note last until the next one starts" &&
    tips.humanize === "Change timing and loudness a little, so it sounds played" &&
    tips.swing === "Push every second note a little later, for a bouncy feel",
  `each tool's tooltip says what it does in plain words: ${JSON.stringify([tips.quantize, tips.legato, tips.humanize, tips.swing])}`,
);
const bad = await lint("[data-testid=note-menu]");
check(bad.length === 0, `the menu passes the taste lint${bad.length ? `: ${[...new Set(bad)].slice(0, 5).join("; ")}` : ""}`);

// Up an octave: that note only, +12, one undo step named for it.
let rev = await revision();
await pick("octave_up");
check(await until(async () => (await bassClip()).notes.filter((x, i) => i !== 3).length === 47 && (await shown())[3]?.pitch === before3.pitch + 12), `Up an octave moves the note from ${before3.pitch} to ${before3.pitch + 12}`);
const after = await bassClip();
check(after.notes.length === 48 && after.notes.some((x) => x.pitch === before3.pitch + 12 && x.start === before3.start), "the file has it an octave up");
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
await page.click('[data-testid=note-menu] [data-swing="medium"]');
const swungStarts = [0, 0.3, 0.5, 0.8, 1, 1.3, 1.5, 1.8];
check(
  await until(async () => JSON.stringify((await bassClip()).notes.map((x) => x.start)) === JSON.stringify(swungStarts)),
  `Swing Medium on 1/16 notes: the second of each pair 0.05 of a beat later (60%), the first where it was: ${(await bassClip()).notes.map((x) => x.start)}`,
);
check((await bassClip()).notes.every((x, i) => Math.abs(x.start + x.length - sixteenths[i].start - 0.25) < 1e-9), "each moved note keeps its end, so it never runs into the next");
check(await oneStep(rev), "one undo step");
check(await until(async () => (await undoTitle()).startsWith("Undo swinging Bass notes")), `Undo names it: ${await undoTitle()}`);
const once = read(FILE);
e = await emptyAt();
await page.mouse.click(e.x, e.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
rev = await revision();
await page.click('[data-testid=note-menu] [data-swing="medium"]');
await sleep(800);
check(read(FILE) === once && (await revision()) === rev, "Swing Medium twice is the same as once: it sets notes to their swung place");
e = await emptyAt();
await page.mouse.click(e.x, e.y, { button: "right" });
await until(() => menu().then(Boolean), 2000);
await page.click('[data-testid=note-menu] [data-swing="light"]');
check(
  await until(async () => JSON.stringify((await bassClip()).notes.map((x) => x.start)) === "[0,0.275,0.5,0.775,1,1.275,1.5,1.775]"),
  `Light after Medium puts them at Light's place (55%): ${(await bassClip()).notes.map((x) => x.start)}`,
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

// ---------------------------------------------------------------- read only

const bassTs = read("tracks/bass.ts");
write("tracks/bass.ts", `${bassTs}\n(`);
check(await until(() => page.$("[data-testid=notes-locked]").then(Boolean)), "with the code broken the roll is read only");
n = await noteAt(1);
await page.mouse.click(n.x, n.y, { button: "right" });
check(await until(() => menu().then(Boolean), 2000), "the menu still opens");
const r3 = await rows();
check(r3.length === 10 && r3.filter((r) => r.id !== "swing").every((r) => r.disabled && r.title.includes("compiles")), `its rows are disabled, the tooltip saying why: ${r3[0]?.title}`);
check(await page.$$eval("[data-testid=note-menu] [data-swing]", (els) => els.every((b) => b.disabled)), "Swing's three too");
await page.keyboard.press("Escape");
write("tracks/bass.ts", bassTs);
check(await until(async () => !(await page.$("[data-testid=notes-locked]"))), "once it compiles, the roll is editable again");

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
  check(pc.errors.length === 0, `no console errors on the PC page ${pc.errors.slice(0, 2).join(" | ")}`);
  await pc.browser.close();
}

// ---------------------------------------------------------------- the end

check(read(FILE) === base, "the Bass notes are as the suite last set them");
await browser.close();
done(errors);
