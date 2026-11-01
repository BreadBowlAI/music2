// Track notes (gaps #6, #16, #15, #3; NOTE-1 to NOTE-9): the roll's help with the song's key and
// its tools. Lock to key: a note drawn between two notes of C major goes down to the lower, a
// dragged one lands on the key's note nearest the pointer, ↑ steps to the next one; ⌘ places a
// note off the grid, drawn, dragged or resized. A chord stamp: the chord's name and outline under
// the pointer, its notes placed in one click (or a double-click, or a drag that sets its length)
// as one undo step, I for the next inversion. In a pentatonic key only chords all in the key are
// placed, and Lock to key leaves a stamped chord as it is; over the blues' blue note the pointer
// says why there is none. Notes copied in Keys' roll paste into Bass's clip at the playhead (or at
// its start), one undo step, selected, naming the clip by the hash the window drew. A drum machine's
// roll folds to the drums its clip uses (the kit's rows while it has none), no row goes while the
// roll is open, ↑ moves to the next row shown, and Fold shows every row. ⌘↓ makes rows taller.
// The view settings land in .music2/view.json, never in the song.
//
//   node e2e/nt-notes.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, posts, api, state, box, key, drag } = await open(1440, 900);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });

/** The song's own view settings on this machine. */
const view = () => {
  try {
    return JSON.parse(read(".music2/view.json"));
  } catch {
    return {};
  }
};
const lane = async (m, l) => (await state()).timeline.tracks.find((t) => t.module === m).lanes.find((x) => x.key === l);
const revision = async () => (await state()).view.revision;
/** Write a lane's first clip's notes through the lane route, as an agent would, keeping its id. */
async function setNotes(m, l, notes) {
  for (let tries = 0; ; tries++) {
    await sleep(200);
    const st = await state();
    const c = st.timeline.tracks.find((t) => t.module === m).lanes.find((x) => x.key === l).clips[0];
    const r = await post("/lane", { session: st.session, revision: st.view.revision, module: m, lane: l, clips: [{ id: c.id, start: c.start, length: c.length, notes }] });
    if (r.status === 200) return;
    if (tries === 5 || !/changed since/.test(r.body.error ?? "")) throw new Error(`lane write: ${r.status} ${JSON.stringify(r.body)}`);
  }
}
/** The roll's notes, as it shows them. */
const shown = () =>
  page.$$eval("[data-testid=notes] [data-note]", (els) => els.map((e) => ({ pitch: Number(e.dataset.pitch), start: Number(e.dataset.start), selected: e.dataset.selected === "true" })));
/** Open a lane's first clip in Notes, by a double-click in the timeline. */
async function openClip(sel, n) {
  const b = await box(`[data-lane="${sel}"] :nth-child(1 of [data-clip])`);
  await page.mouse.click(b.x + Math.min(40, b.width / 2), b.y + b.height / 2, { count: 2 });
  return until(async () => (await page.$$("[data-testid=notes] [data-note]")).length === n, 5000);
}
/** Where row `pitch` at beat `beat` of the clip is on screen, the row scrolled into view. */
const cell = (pitch, beat) =>
  page.evaluate(
    (pitch, beat) => {
      const row = document.querySelector(`[data-testid=roll-keys] [data-key="${pitch}"]`);
      row.scrollIntoView({ block: "center" });
      const grid = document.querySelector("[data-testid=roll-grid]");
      const g = grid.getBoundingClientRect(), r = row.getBoundingClientRect();
      return { x: g.left + beat * Number(grid.dataset.ppb) + 2, y: r.top + r.height / 2 };
    },
    pitch,
    beat,
  );
const notesOf = async (m, l) => (await lane(m, l)).clips[0].notes.map((n) => [n.pitch, n.start]);
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);

// ---------------------------------------------------------------- lock to key (NOTE-3)

await setNotes("keys", "chords", []);
check(await openClip("keys.chords", 0), "Keys' clip, emptied, opens in Notes");
check(!(await page.$("[data-testid=lock-key]")) && !(await page.$("[data-testid=chord-tool]")), "with no key set, the roll offers no Lock to key and no chords");
const st = await state();
await post("/song-key", { session: st.session, key: { root: "C", scale: "major" } });
check(await until(async () => (await page.$eval("[data-testid=lock-key]", (e) => e.textContent).catch(() => null)) === "Lock to key"), "C major set: the roll's header shows Lock to key");
const lockTitle = await page.$eval("[data-testid=lock-key]", (e) => e.title);
check(lockTitle.startsWith("Lock to key (C major): off.") && lockTitle.includes("nearest note of the key"), `its tooltip says what it does: ${lockTitle}`);
await page.click("[data-testid=lock-key]");
check(await until(async () => (await page.$eval("[data-testid=lock-key]", (e) => e.getAttribute("aria-pressed"))) === "true"), "a click turns it on");
check(await until(() => view().roll?.lock === true), `kept in .music2/view.json as roll.lock: ${JSON.stringify(view().roll)}`);
check(!read("layout.json").includes("lock"), "and never in the song's files");

await page.click('[data-tool="draw"]');
let rev = await revision();
let at = await cell(61, 0.5);
await page.mouse.click(at.x, at.y);
check(await until(async () => JSON.stringify(await notesOf("keys", "chords")) === "[[60,0.5]]"), `a click on C♯4's row, between C4 and D4, draws C4: a tie goes down (${JSON.stringify(await notesOf("keys", "chords"))})`);
at = await cell(63, 1);
await page.mouse.click(at.x, at.y);
check(await until(async () => JSON.stringify(await notesOf("keys", "chords")) === "[[60,0.5],[62,1]]"), "D♯4's row draws D4");
check((await revision()) === rev + 2, "each note one undo step");

// ⌘ held: off the grid (NOTE-6).
at = await cell(64, 2.3);
await page.keyboard.down("Meta");
await page.mouse.click(at.x, at.y);
await page.keyboard.up("Meta");
const offGrid = (x) => Math.abs(x / 0.25 - Math.round(x / 0.25)) > 0.01;
check(
  await until(async () => (await notesOf("keys", "chords")).some(([p, s]) => p === 64 && Math.abs(s - 2.3) < 0.1 && offGrid(s))),
  `with ⌘ held, E4 lands where the pointer is, off the 1/16 grid: ${JSON.stringify(await notesOf("keys", "chords"))}`,
);

/** Where to press note `pitch` at `start` (a few px into it), or its right edge. */
const noteBox = async (pitch, start, edge = false) => {
  const b = await page.$eval(`[data-testid=notes] [data-note][data-pitch="${pitch}"][data-start="${start}"]`, (e) => {
    e.scrollIntoView({ block: "center", inline: "nearest" });
    const r = e.getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height };
  });
  // The left of a short note: its right part is the edge that resizes it.
  return { x: edge ? b.x + b.w - 2 : b.x + 2, y: b.y + b.h / 2 };
};
// A drag with Lock to key: C4 dragged up three rows, to D♯4's, lands on D4, the key's nearest
// (a tie goes down), and ↑ then moves it to E4, the key's next note (NOTE-3).
let from = await noteBox(60, 0.5);
await drag(from, { x: from.x, y: from.y - 3 * 14 });
check(await until(async () => JSON.stringify((await notesOf("keys", "chords")).slice(0, 2)) === "[[62,0.5],[62,1]]"), `dragged onto D♯4's row, C4 lands on D4: ${JSON.stringify(await notesOf("keys", "chords"))}`);
await page.keyboard.press("ArrowUp");
check(await until(async () => (await notesOf("keys", "chords"))[0]?.[0] === 64), `↑ moves it to E4, the key's next note: ${JSON.stringify(await notesOf("keys", "chords"))}`);
await page.keyboard.press("ArrowDown");
await until(async () => (await notesOf("keys", "chords"))[0]?.[0] === 62);
// ⌘ held while dragging and resizing: off the grid too (NOTE-6).
from = await noteBox(62, 1);
const ppb = await page.$eval("[data-testid=roll-grid]", (e) => Number(e.dataset.ppb));
await page.keyboard.down("Meta");
await drag(from, { x: from.x + 0.37 * ppb, y: from.y });
await page.keyboard.up("Meta");
check(await until(async () => (await notesOf("keys", "chords")).some(([p, s]) => p === 62 && s > 1.2 && s < 1.5 && offGrid(s))), `with ⌘ held, a dragged note lands off the grid: ${JSON.stringify(await notesOf("keys", "chords"))}`);
const moved = (await lane("keys", "chords")).clips[0].notes.find((n) => n.pitch === 62 && n.start > 1.2);
from = await noteBox(62, moved.start, true);
await page.keyboard.down("Meta");
await drag(from, { x: from.x + 0.3 * ppb, y: from.y });
await page.keyboard.up("Meta");
check(
  await until(async () => (await lane("keys", "chords")).clips[0].notes.some((n) => n.pitch === 62 && n.start === moved.start && Math.abs(n.length - moved.length) > 0.1 && offGrid(n.start + n.length))),
  "and a resized one ends off the grid",
);

// ---------------------------------------------------------------- chord stamps (NOTE-4)

await page.click("[data-testid=chord-tool]");
check(await until(() => page.$("[data-testid=chord-menu]").then(Boolean), 2000), "Chord ▾ opens the stamp's menu");
const kinds = await page.$$eval("[data-testid=chord-menu] [data-chord]", (els) => els.map((e) => e.dataset.chord));
check(kinds.join() === "none,triad,seventh,sus2,sus4,add9", `it lists single notes, then the server's chords: ${kinds}`);
await sleep(300);
await page.screenshot({ path: `${ROOT}/nt-notes-chord-menu.png` });
const badMenu = await page.evaluate(lint, "[data-testid=chord-menu]");
check(badMenu.length === 0, `the menu passes the taste lint${badMenu.length ? `: ${badMenu.slice(0, 3).join(" | ")}` : ""}`);
await page.click('[data-testid=chord-menu] [data-chord="triad"]');
check(await until(async () => (await page.$eval("[data-testid=chord-tool]", (e) => e.textContent)) === "Triad"), "Triad is picked");
at = await cell(62, 4);
await page.mouse.move(at.x, at.y);
check(await until(async () => (await page.$eval("[data-testid=stamp-name]", (e) => e.textContent).catch(() => null)) === "Dm · ii"), `over D4's row the pointer shows the chord it would place: ${await page.$eval("[data-testid=stamp-name]", (e) => e.textContent).catch(() => null)}`);
const ghosts = await page.$$eval("[data-ghost]", (els) => els.map((e) => Number(e.dataset.ghost)));
check(ghosts.join() === "62,65,69", `its notes outlined: D4 F4 A4 (${ghosts})`);
rev = await revision();
await page.mouse.click(at.x, at.y);
check(await until(async () => (await notesOf("keys", "chords")).filter(([, s]) => s === 4).map(([p]) => p).join() === "62,65,69"), "a click places D minor's three notes at once");
check(await until(async () => (await revision()) === rev + 1), "as one undo step");
await page.focus("[data-testid=notes]");
await page.keyboard.press("i");
check(await until(async () => (await page.$eval("[data-testid=chord-tool]", (e) => e.textContent)) === "Triad, 1st inversion"), "I: the first inversion");
at = await cell(60, 6);
await page.mouse.click(at.x, at.y);
check(await until(async () => (await notesOf("keys", "chords")).filter(([, s]) => s === 6).map(([p]) => p).join() === "64,67,72"), "C major's first inversion on C4's row: E4 G4 C5");
const withBoth = read("data/keys/chords.json");
await key("Meta+z");
check(await until(async () => (await notesOf("keys", "chords")).every(([, s]) => s !== 6)), "one ⌘Z takes the whole chord back");
check(read("data/keys/chords.json") !== withBoth && (await notesOf("keys", "chords")).length === 6, "and only it");
// A drag in Draw sets its length; a double-click in Select places one too (NOTE-4).
await page.focus("[data-testid=notes]");
await page.keyboard.press("i");
await page.keyboard.press("i");
at = await cell(65, 8);
await drag(at, { x: at.x + 1.9 * ppb, y: at.y });
check(await until(async () => JSON.stringify((await lane("keys", "chords")).clips[0].notes.filter((n) => n.start === 8).map((n) => [n.pitch, n.length])) === "[[65,2],[69,2],[72,2]]"), "a drag in Draw places F major and sets its length: two beats");
await page.click('[data-tool="select"]');
at = await cell(67, 12);
await page.mouse.click(at.x, at.y, { count: 2 });
check(await until(async () => (await notesOf("keys", "chords")).filter(([, s]) => s === 12).map(([p]) => p).join() === "67,71,74"), "a double-click in Select places G major");
await key("Meta+z");
await key("Meta+z");
check(await until(async () => (await notesOf("keys", "chords")).length === 6), "two ⌘Z take both back");

// ---------------------------------------------------------------- copy and paste between clips (NOTE-5)

await page.click('[data-testid=chord-tool]');
await until(() => page.$("[data-testid=chord-menu]").then(Boolean), 2000);
await page.click('[data-testid=chord-menu] [data-chord="none"]');
await page.focus("[data-testid=notes]");
await key("Meta+a");
await key("Meta+c");
const keysBefore = read("data/keys/chords.json");
await post("/seek", { beat: 8 });
// The page reads the playhead from its next poll.
await sleep(600);
check(await openClip("bass.notes", 48), "Bass's clip opens in Notes");
await page.focus("[data-testid=notes]");
rev = await revision();
await key("Meta+v");
// Keys' six notes (its three drawn, dragged and stretched, and D minor), the earliest at beat 8.
const copied = await notesOf("keys", "chords");
const first = Math.min(...copied.map(([, t]) => t));
const pasted = copied.map(([p, t]) => [p, 8 + t - first]);
check(
  await until(async () => {
    const now = await notesOf("bass", "notes");
    return now.length === 54 && pasted.every(([p, s]) => now.some(([q, t]) => q === p && Math.abs(t - s) < 0.1));
  }),
  `⌘V pastes Keys' six notes into Bass's clip, the first at the playhead (bar 3), keeping their distances: ${JSON.stringify(pasted)}`,
);
check(await until(async () => (await revision()) === rev + 1) && read("data/keys/chords.json") === keysBefore, "one undo step, and Keys' clip as it was");
check(await until(async () => (await shown()).filter((n) => n.selected).length === 6), "the pasted notes are selected");
check(await until(async () => (await undoTitle()).startsWith("Undo pasting Bass notes")), `Undo names it: ${await undoTitle()}`);
const sent = posts.filter((p) => p.url === "/api/graph/clips" && p.body.includes('"op":"paste"')).at(-1);
check(!!sent && JSON.parse(sent.body).gesture.into.sha?.length > 0, `it names Bass's clip by the hash the window drew, so a change it didn't see is refused: ${sent?.body.slice(0, 160)}`);
// With the playhead outside the clip, at its start.
await key("Meta+z");
await until(async () => (await notesOf("bass", "notes")).length === 48);
await post("/seek", { beat: 100 });
await sleep(600);
await page.focus("[data-testid=notes]");
await key("Meta+v");
check(
  await until(async () => {
    const now = await notesOf("bass", "notes");
    return now.length === 54 && pasted.every(([p, s]) => now.some(([q, t]) => q === p && Math.abs(t - (s - 8)) < 0.1));
  }),
  "with the playhead past the clip, ⌘V pastes at the clip's start",
);
await key("Meta+z");
await until(async () => (await notesOf("bass", "notes")).length === 48);

// ---------------------------------------------------------------- a pentatonic key and the blues (NOTE-4, review N1)

await post("/song-key", { session: (await state()).session, key: { root: "C", scale: "major-pentatonic" } });
await until(async () => (await state()).timeline.key?.scale === "major-pentatonic");
await page.click('[data-tool="draw"]');
await page.click("[data-testid=chord-tool]");
await until(() => page.$("[data-testid=chord-menu]").then(Boolean), 2000);
await page.click('[data-testid=chord-menu] [data-chord="triad"]');
const stampSays = () => page.$eval("[data-testid=stamp-name]", (e) => e.textContent).catch(() => null);
at = await cell(62, 20);
await page.mouse.move(at.x, at.y);
check(
  await until(async () => (await stampSays()) === "Dm has F, which isn't in C major pentatonic: a stamp places only chords whose notes are all in the key"),
  `over D4's row, D minor isn't offered, and the pointer says why: ${await stampSays()}`,
);
rev = await revision();
await page.mouse.click(at.x, at.y);
await sleep(500);
check((await revision()) === rev && (await notesOf("bass", "notes")).length === 48, "a click there places nothing");
await page.click("[data-testid=chord-tool]");
await until(() => page.$("[data-testid=chord-menu]").then(Boolean), 2000);
await page.click('[data-testid=chord-menu] [data-chord="sus2"]');
await page.mouse.move(at.x, at.y);
check(await until(async () => (await stampSays()) === "Dsus2 · ii"), `Sus2 on D is all in the key: ${await stampSays()}`);
await page.mouse.click(at.x, at.y);
const sus = () => notesOf("bass", "notes").then((n) => n.filter(([p]) => p >= 60));
check(await until(async () => JSON.stringify(await sus()) === "[[62,20],[64,20],[69,20]]"), `D E A placed: ${JSON.stringify(await sus())}`);
// Lock to key is on: a drag sideways leaves the chord as it is.
from = await noteBox(62, 20);
await drag(from, { x: from.x + ppb, y: from.y });
check(await until(async () => JSON.stringify(await sus()) === "[[62,21],[64,21],[69,21]]"), `dragged a beat later with Lock to key on, it keeps every note: ${JSON.stringify(await sus())}`);
await post("/song-key", { session: (await state()).session, key: { root: "A", scale: "blues" } });
await until(async () => (await state()).timeline.key?.scale === "blues");
at = await cell(63, 24);
await page.mouse.move(at.x, at.y - 2);
await page.mouse.move(at.x, at.y);
check(await until(async () => (await stampSays()) === "A blues has no chord on E♭, its blue note"), `over the blues' blue note, the pointer says there is no chord: ${await stampSays()}`);
const gridTitle = await page.$eval("[data-testid=roll-grid]", (e) => e.title);
check(gridTitle === "A blues has no chord on E♭, its blue note", `and so does the grid's tooltip: ${gridTitle}`);
await page.click('[data-tool="select"]');
await page.click("[data-testid=chord-tool]");
await until(() => page.$("[data-testid=chord-menu]").then(Boolean), 2000);
await page.click('[data-testid=chord-menu] [data-chord="none"]');

// ---------------------------------------------------------------- folded drum rows (NOTE-8)

// The drum machine names its rows; no list offers it now (KIT-2), so it is written in code, over a
// Drum + Track adds.
const added = await post("/track/add", { session: (await state()).session, component: "@m2/drums:drum", key: "drum-kit", label: "Drum machine" });
check(added.status === 200, `a drum machine track is added (${added.status})`);
await until(() => fs.existsSync(`${PROJ}/tracks/drum-kit.ts`));
write("tracks/drum-kit.ts", read("tracks/drum-kit.ts").replace("drums.drum(", "drums.kit("));
await until(async () => Object.keys((await state()).timeline.tracks.find((t) => t.module === "drum-kit")?.lanes?.[0]?.names ?? {}).length > 0);
const writeKit = async (notes) => {
  const st = await state();
  return post("/lane", { session: st.session, revision: st.view.revision, module: "drum-kit", lane: "hits", clips: [{ id: "kitbeat1", start: 0, length: 4, notes }] });
};
await writeKit([]);
await until(async () => !!(await page.$('[data-lane="drum-kit.hits"] [data-clip]')), 5000);
check(await openClip("drum-kit.hits", 0), "its empty clip opens in Notes");
const rowsNow = () => page.$eval("[data-testid=roll-grid]", (e) => e.dataset.rows ?? null);
const named = (await lane("drum-kit", "hits")).names;
const kit = Object.keys(named).map(Number).sort((a, b) => b - a).join();
check((await rowsNow()) === kit && !(await page.$('[data-testid=roll-keys] [data-key="61"]')), `folded, with no notes: the kit's ${Object.keys(named).length} named rows (${await rowsNow()})`);
check(!(await page.$("[data-testid=lock-key]")) && (await page.$eval("[data-testid=fold-rows]", (e) => e.getAttribute("aria-pressed"))) === "true", "a kit has Fold, on, and no Lock to key");
await writeKit([36, 42, 38, 42].map((pitch, i) => ({ pitch, start: i, length: 0.5, velocity: 0.8 })));
await until(async () => (await page.$$("[data-testid=notes] [data-note]")).length === 4);
check((await rowsNow()) === kit, "a beat written in it keeps the kit's rows: none goes while the roll is open");
await page.click("[data-testid=fold-rows]");
check(await until(async () => (await rowsNow()) === null && !!(await page.$('[data-testid=roll-keys] [data-key="61"]'))), "Fold off: every row");
check(await until(() => view().roll?.fold === false), `kept as roll.fold: ${JSON.stringify(view().roll)}`);
await page.click("[data-testid=fold-rows]");
check(await until(async () => (await rowsNow()) === "42,38,36"), `folded again: the three drums the beat uses, highest first (${await rowsNow()})`);
// Folded, each drum's name fits on its key, and the roll passes the lint.
const misfits = await page.evaluate(() =>
  [...document.querySelectorAll("[data-testid=roll-keys] [data-name]")]
    .filter((e) => {
      const s = e.getBoundingClientRect(), k = e.parentElement.getBoundingClientRect();
      return s.left < k.left || s.right > k.right || s.top < k.top || s.bottom > k.bottom;
    })
    .map((e) => e.textContent),
);
check(misfits.length === 0 && (await page.$$("[data-testid=roll-keys] [data-name]")).length === 3, `folded, every name fits on its key (${misfits})`);
const badFolded = await page.evaluate(lint, "[data-testid=notes]");
check(badFolded.length === 0, `the folded roll passes the taste lint${badFolded.length ? `: ${badFolded.slice(0, 3).join(" | ")}` : ""}`);
// ↑ on the Kick moves it to the next row shown, the Snare; the Kick's row stays.
const kick = await noteBox(36, 0);
await page.mouse.click(kick.x, kick.y);
await page.keyboard.press("ArrowUp");
check(await until(async () => JSON.stringify(await notesOf("drum-kit", "hits")) === "[[38,0],[42,1],[38,2],[42,3]]"), `↑ moves the Kick to the next row shown, the Snare: ${JSON.stringify(await notesOf("drum-kit", "hits"))}`);
check((await rowsNow()) === "42,38,36", "and the Kick's row stays");
// A row an agent's note lands on is shown, and stays after the note goes.
await writeKit([49, 42, 38, 42].map((pitch, i) => ({ pitch, start: i, length: 0.5, velocity: 0.8 })));
check(await until(async () => (await rowsNow()) === "49,42,38,36"), `a Crash written by an agent adds its row: ${await rowsNow()}`);
await writeKit([36, 42, 38, 42].map((pitch, i) => ({ pitch, start: i, length: 0.5, velocity: 0.8 })));
await until(async () => JSON.stringify(await notesOf("drum-kit", "hits")) === "[[36,0],[42,1],[38,2],[42,3]]");
check((await rowsNow()) === "49,42,38,36", "and keeps it when the Crash goes");

// ---------------------------------------------------------------- taller rows (NOTE-9)

await page.focus("[data-testid=notes]");
const height = () => page.$eval("[data-testid=roll-grid]", (e) => Number(e.dataset.rowHeight));
await key("Meta+ArrowDown");
check(await until(async () => (await height()) === 16), `⌘↓: rows 2 px taller (${await height()})`);
check(await until(() => view().roll?.rows === 16), "kept as roll.rows");
const keyRow = await box('[data-testid=roll-keys] [data-key="38"]');
check(Math.round(keyRow.height) === 16, `the keys follow: ${keyRow.height}`);
await key("Meta+ArrowUp");
check(await until(async () => (await height()) === 14), "⌘↑: back to 14");

done(errors);
