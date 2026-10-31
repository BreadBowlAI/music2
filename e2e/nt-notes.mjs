// Track notes (gaps #6, #16, #15, #3; NOTE-1 to NOTE-9): the roll's help with the song's key and
// its tools. Lock to key: a note drawn between two notes of C major goes down to the lower; ⌘
// places one off the grid. A chord stamp: the chord's name and outline under the pointer, its
// notes placed in one click as one undo step, I for the next inversion. Notes copied in Keys'
// roll paste into Bass's clip at the playhead, one undo step, selected. A Drum kit's roll folds to
// the drums its clip uses, and Fold shows every row. ⌘↓ makes rows taller. The view settings land
// in .music2/view.json, never in the song.
//
//   node e2e/nt-notes.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { ROOT, check, done, open, read, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, state, box, key } = await open(1440, 900);
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
check(await until(async () => !!(await page.$("[data-testid=lock-key]"))), "C major set: the roll's header shows In key");
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
check(
  await until(async () => (await notesOf("keys", "chords")).some(([p, s]) => p === 64 && Math.abs(s - 2.3) < 0.1 && Math.abs(s / 0.25 - Math.round(s / 0.25)) > 0.01)),
  `with ⌘ held, E4 lands where the pointer is, off the 1/16 grid: ${JSON.stringify(await notesOf("keys", "chords"))}`,
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
check(await until(async () => (await page.$eval("[data-testid=chord-tool]", (e) => e.textContent)) === "Triad 1st"), "I: the first inversion");
at = await cell(60, 6);
await page.mouse.click(at.x, at.y);
check(await until(async () => (await notesOf("keys", "chords")).filter(([, s]) => s === 6).map(([p]) => p).join() === "64,67,72"), "C major's first inversion on C4's row: E4 G4 C5");
const withBoth = read("data/keys/chords.json");
await key("Meta+z");
check(await until(async () => (await notesOf("keys", "chords")).every(([, s]) => s !== 6)), "one ⌘Z takes the whole chord back");
check(read("data/keys/chords.json") !== withBoth && (await notesOf("keys", "chords")).length === 6, "and only it");

// ---------------------------------------------------------------- copy and paste between clips (NOTE-5)

await page.click('[data-tool="select"]');
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
// From the first copied note (C4 at 0.5): C4 D4 E4 and D minor, the earliest at beat 8.
const pasted = [[60, 8], [62, 8.5], [64, 9.8], [62, 11.5], [65, 11.5], [69, 11.5]];
check(
  await until(async () => {
    const now = await notesOf("bass", "notes");
    return now.length === 54 && pasted.every(([p, s]) => now.some(([q, t]) => q === p && Math.abs(t - s) < 0.1));
  }),
  `⌘V pastes Keys' six notes into Bass's clip, the first at the playhead (bar 3), keeping their distances`,
);
check(await until(async () => (await revision()) === rev + 1) && read("data/keys/chords.json") === keysBefore, "one undo step, and Keys' clip as it was");
check(await until(async () => (await shown()).filter((n) => n.selected).length === 6), "the pasted notes are selected");
check(await until(async () => (await undoTitle()).startsWith("Undo pasting Bass notes")), `Undo names it: ${await undoTitle()}`);

// ---------------------------------------------------------------- folded drum rows (NOTE-8)

const added = await post("/track/add", { session: (await state()).session, component: "std:drum-kit" });
check(added.status === 200, `a Drum kit track is added (${added.status})`);
await until(async () => (await state()).timeline.tracks.some((t) => t.module === "drum-kit"));
const s1 = await state();
const beat = [36, 42, 38, 42].map((pitch, i) => ({ pitch, start: i, length: 0.5, velocity: 0.8 }));
await post("/lane", { session: s1.session, revision: s1.view.revision, module: "drum-kit", lane: "hits", clips: [{ id: "kitbeat1", start: 0, length: 4, notes: beat }] });
await until(async () => !!(await page.$('[data-lane="drum-kit.hits"] [data-clip]')), 5000);
check(await openClip("drum-kit.hits", 4), "its beat opens in Notes");
const rowsNow = () => page.$eval("[data-testid=roll-grid]", (e) => e.dataset.rows ?? null);
check((await rowsNow()) === "42,38,36" && !(await page.$('[data-testid=roll-keys] [data-key="61"]')), `folded: only the three drums the beat uses, highest first (${await rowsNow()})`);
check(!(await page.$("[data-testid=lock-key]")) && (await page.$eval("[data-testid=fold-rows]", (e) => e.getAttribute("aria-pressed"))) === "true", "a kit has Fold, on, and no Lock to key");
await page.click("[data-testid=fold-rows]");
check(await until(async () => (await rowsNow()) === null && !!(await page.$('[data-testid=roll-keys] [data-key="61"]'))), "Fold off: every row");
check(await until(() => view().roll?.fold === false), `kept as roll.fold: ${JSON.stringify(view().roll)}`);
await page.click("[data-testid=fold-rows]");
check(await until(async () => (await rowsNow()) === "42,38,36"), "and folded again");

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
