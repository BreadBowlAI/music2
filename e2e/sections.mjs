// Sections (gap #23, ARR-9): a drag along the ruler's top band, where the markers are, selects
// whole bars (each end at the nearest bar line) and shows them across every track, as a marquee's
// box; a marker's Select to next marker selects its bars. A drag on the rest of the ruler loops and
// selects nothing. One selection at a time: bars let go of the track, a track lets go of the bars.
// ⌘C copies a chorus (bars 1 to 8, the server's copy) and ⌘V pastes it at the playhead's bar, bar
// 17: every track's clips there, the marker where it was, one undo step named for Undo that takes
// back every track. The bars' menu (Empty bars); ⌘D duplicates them right after, ⌘I inserts as many
// empty bars where they start, ⇧⌘⌫ deletes them and what follows moves earlier, ⌫ empties them;
// each one gesture of `POST /api/graph/sections`, one undo step. Esc lets go.
//
//   node e2e/sections.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";
import { tips } from "./tiplib.mjs";

const { browser, page, errors, posts, api, key, box, drag, caughtUp } = await open(1440, 900);
const PPB = 24;
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const status = async () => (await api("/status")).body;
const session = (await api("")).body.session;
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
/** Each clip of a lane as `[start, length]`. */
const spans = (lane) => JSON.parse(read(`data/${lane}.json`)).clips.map((c) => [c.start, c.length]);
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
const undo = () => key("Meta+z");
/** The selected bars as the timeline draws them, `[start, end]` in beats, or null. */
const shown = () => page.$eval("[data-testid=section]", (e) => [Number(e.dataset.start), Number(e.dataset.end)]).catch(() => null);
const selectedTrack = () => page.$$eval("[data-track][data-selected]", (els) => els.map((e) => e.dataset.track));
const lintNow = async (what) => {
  await sleep(250);
  const bad = [...new Set(await page.evaluate(lint))];
  check(bad.length === 0, `lint, ${what}: nothing outside the design system${bad.length ? ` (${bad.slice(0, 5).join("; ")})` : ""}`);
};
/** Drag along the ruler's top band (the markers' band) from beat `a` to beat `b`: bars. */
const along = async (a, b) => {
  const r = await box("[data-testid=ruler]");
  await drag({ x: r.x + a * PPB, y: r.y + 7 }, { x: r.x + b * PPB, y: r.y + 7 });
};
/** Drag across the ruler's lower half from beat `a` to beat `b`: a loop. */
const across = async (a, b) => {
  const r = await box("[data-testid=ruler]");
  await drag({ x: r.x + a * PPB, y: r.y + r.height - 6 }, { x: r.x + b * PPB, y: r.y + r.height - 6 });
};
const LANES = ["kick/hits", "hats/hits", "bass/notes", "bass/cutoff", "bass/drop", "keys/chords"];
const files = () => Object.fromEntries(LANES.map((l) => [l, read(`data/${l}.json`)]));

await page.waitForSelector('[data-lane="kick.hits"] [data-clip]');
// The chorus: bars 1 to 8, where every track plays, marked.
await post("/markers", { session, markers: [{ beat: 0, name: "Chorus" }] });
await until(() => page.$('[data-marker-name="Chorus"]'));
const before = files();
const layout = read("layout.json");

// ---------------------------------------------------------------- a loop drag selects nothing
await across(32.3, 47.6);
const loopIs = (l, start, end) => !!l && l.start === start && l.end === end && l.on === true;
check(await until(async () => loopIs((await status()).loop, 32, 48)), `a drag across the ruler loops beats 32–48, as before: ${JSON.stringify((await status()).loop)}`);
check((await shown()) === null, "and selects no bars");

// ---------------------------------------------------------------- select bars 1 to 8 along the top band
// From just before bar 9 back to just inside bar 1 (a drag that starts on the Chorus flag moves the
// marker): each end goes to the nearest bar line.
await page.click('[data-track="bass"] [data-module-tab]');
await until(async () => same(await selectedTrack(), ["bass"]));
await along(31.6, 0.3);
check(await until(async () => same(await shown(), [0, 32])), `a drag along the ruler's top selects bars 1–8: ${JSON.stringify(await shown())}`);
check(loopIs((await status()).loop, 32, 48), "and leaves the loop as it was");
check(same(await selectedTrack(), []), `one selection at a time: the Bass track lets go (${await selectedTrack()})`);
const sel = await box("[data-testid=section]"), kick = await box('[data-lane="kick.hits"]'), keys = await box('[data-lane="keys.chords"]');
check(!!sel && sel.y <= kick.y && sel.y + sel.height >= keys.y + keys.height && Math.abs(sel.width - 32 * PPB) <= 1, `shown across every track: ${JSON.stringify(sel)}`);
check(!!(await page.$("[data-testid=section-band]")), "and lit in the ruler's top band");
await lintNow("bars selected");
const untipped = await page.evaluate(tips, "[data-testid=timeline]");
check(untipped.length === 0, `every control of the timeline has its tooltip${untipped.length ? `: ${untipped.slice(0, 3).join("; ")}` : ""}`);

// ---------------------------------------------------------------- ⌘C, then ⌘V at bar 17
await key("Meta+c");
await until(() => posts.some((p) => p.url === "/api/graph/sections/copy"));
await post("/seek", { beat: 64 });
// Once the page's own playhead is there: ⌘V pastes where it shows it.
await until(() => page.$eval("[data-testid=playhead]", (e) => Math.abs(Number(e.dataset.beat) - 64) < 1e-3));
const sent = posts.length;
await key("Meta+v");
const pasted = await until(() => spans("kick/hits").some(([s, l]) => s === 64 && l === 32));
check(pasted, `⌘V pastes the chorus at the playhead's bar, bar 17: ${JSON.stringify(spans("kick/hits"))}`);
const want = {
  "kick/hits": [[0, 32], [64, 32]],
  "hats/hits": [[0, 32], [64, 32]],
  "bass/notes": [[0, 32], [64, 32]],
  "bass/cutoff": [[0, 16], [64, 16]],
  "bass/drop": [[16, 16], [80, 16]],
  "keys/chords": [[0, 32], [64, 32]],
};
const got = Object.fromEntries(LANES.map((l) => [l, spans(l)]));
check(same(got, want), `every track's clips, the automation's too: ${JSON.stringify(got)}`);
const gestures = posts.slice(sent).filter((p) => p.url === "/api/graph/sections").map((p) => JSON.parse(p.body).gesture);
const kinds = gestures[0]?.section.lanes.map((l) => l.kind) ?? [];
check(
  gestures.length === 1 && gestures[0].op === "paste" && gestures[0].at === 64 && gestures[0].section.start === 0 && gestures[0].section.end === 32 && kinds.includes("control"),
  `as one gesture, the server's copy with each lane's kind: ${JSON.stringify(gestures.map((g) => ({ op: g.op, at: g.at })))} ${kinds}`,
);
check(read("layout.json") === layout, "the Chorus marker stays where it was (ARR-8)");
check(await until(async () => (await undoTitle()).includes("Undo pasting bars 1–8 at bar 17")), `one undo step, named: ${await undoTitle()}`);
check(await until(async () => same(await shown(), [64, 96])), `the pasted bars are the selection: ${JSON.stringify(await shown())}`);
await undo();
check(await until(() => same(files(), before)), "one Undo takes the paste back, on every track, byte for byte");

// ---------------------------------------------------------------- a marker's bars, and the bars' menu
const flag = await box('[data-marker-name="Chorus"]');
await page.mouse.click(flag.x + 6, flag.y + flag.height / 2, { button: "right" });
await page.waitForSelector('[data-testid=clip-menu] [data-menu-item="select"]', { timeout: 2000 }).catch(() => null);
await page.click('[data-testid=clip-menu] [data-menu-item="select"]').catch(() => {});
check(await until(async () => same(await shown(), [0, 32])), `Select to next marker selects the Chorus's bars, to the end of the last bar that plays: ${JSON.stringify(await shown())}`);
const lane = await box('[data-lane="kick.hits"] > div:last-child');
await page.mouse.click(lane.x + 4 * PPB, lane.y + lane.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]", { timeout: 2000 }).catch(() => null);
const rows = await page.$$eval("[data-testid=clip-menu] [data-menu-item]", (els) => els.map((e) => e.dataset.menuItem));
const label = await page.$eval("[data-testid=clip-menu]", (e) => e.getAttribute("aria-label")).catch(() => null);
check(
  same(rows, ["cut", "copy", "paste", "duplicate", "delete", "insert-bars", "delete-bars", "loop"]) && label === "Menu of bars 1–8",
  `a right-click inside them opens their menu: ${label}: ${rows.join(", ")}`,
);
const rowText = (id) => page.$eval(`[data-menu-item="${id}"]`, (e) => e.textContent).catch(() => "");
const [emptyRow, insertRow] = [await rowText("delete"), await rowText("insert-bars")];
check(emptyRow.startsWith("Empty bars") && insertRow.startsWith("Insert 8 bars") && insertRow.includes("⌘I"), `its rows say what they do, with their keys: ${emptyRow} | ${insertRow}`);
await lintNow("the bars' menu");
await page.keyboard.press("Escape");
await until(async () => !(await page.$("[data-testid=clip-menu]")));

// ---------------------------------------------------------------- ⌘D, ⌘I, ⇧⌘⌫ and ⌫
await key("Meta+d");
check(await until(() => same(spans("kick/hits"), [[0, 32], [32, 32]])), `⌘D duplicates the bars right after them: ${JSON.stringify(spans("kick/hits"))}`);
check(await until(async () => same(await shown(), [32, 64])), "and the copy is the selection, so ⌘D again goes on");
await until(async () => (await undoTitle()).includes("Undo duplicating bars 1–8"));
// The timeline takes a key once it shows its last edit (one pressed sooner does nothing).
await caughtUp();
await key("Meta+i");
check(await until(() => same(spans("kick/hits"), [[0, 32], [64, 32]])), `⌘I inserts 8 empty bars where they start, and the copy moves on: ${JSON.stringify(spans("kick/hits"))}`);
check(await until(async () => (await undoTitle()).includes("Undo inserting 8 bars at bar 9")), `named: ${await undoTitle()}`);
await caughtUp();
await key("Meta+Shift+Backspace");
check(await until(() => same(spans("kick/hits"), [[0, 32], [32, 32]])), `⇧⌘⌫ deletes the selected bars, and what follows moves earlier: ${JSON.stringify(spans("kick/hits"))}`);
check(await until(async () => (await shown()) === null), "and lets go of them");
await until(async () => (await undoTitle()).includes("Undo deleting bars 9–16"));
await along(32.3, 47.6);
await until(async () => same(await shown(), [32, 48]));
await caughtUp();
await page.keyboard.press("Backspace");
check(await until(() => same(spans("kick/hits"), [[0, 32], [48, 16]])), `⌫ empties bars 9–12; the clip across their end is split there: ${JSON.stringify(spans("kick/hits"))}`);
check(await until(async () => (await undoTitle()).includes("Undo emptying bars 9–12")), `named: ${await undoTitle()}`);
// One selection at a time: a track picked lets go of the bars.
await page.click('[data-track="keys"] [data-module-tab]');
check(await until(async () => (await shown()) === null && same(await selectedTrack(), ["keys"])), "picking a track lets go of the bars");
await along(32.3, 47.6);
await until(async () => same(await shown(), [32, 48]));
await page.keyboard.press("Escape");
check(await until(async () => (await shown()) === null), "Esc lets go of the bars");
for (let i = 0; i < 4; i++) {
  const t = await undoTitle();
  await undo();
  await until(async () => (await undoTitle()) !== t);
}
check(await until(() => same(files(), before)), "four Undos take the four gestures back, byte for byte");

await browser.close();
done(errors);
