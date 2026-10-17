// 8b.5 The timeline as an editor (design-8b §7, taste.md §3.4, §5): part 1, its grid and snap (U4):
// the grid chip in the corner (Auto, the menu, triplets), a drag that lands on the grid, ⌘ held to
// place a clip freely, ⌘1 to ⌘4 on the view last pointed at (the timeline or Notes), the choice kept
// over a reload, a click on empty lane space that moves where play starts and writes nothing, and
// a PC page's words.
//
//   node e2e/s8-clips.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { ROOT, check, done, open, read, sleep, until } from "./fixlib.mjs";

const { browser, page, errors, posts, api, state, key, box, drag } = await open(1440, 900);
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
await key("Escape");

// ---------------------------------------------------------------- a PC page says Ctrl
const pc = await open(1440, 900, { pc: true });
await pc.page.waitForSelector("[data-testid=song-grid]");
const pcWords = await pc.page.evaluate(() => [document.querySelector("[data-testid=song-grid]").title, document.querySelector("[data-testid=song-snap]").title].join(" | "));
check(pcWords.includes("Ctrl+1") && pcWords.includes("Ctrl+4") && pcWords.includes("hold Ctrl") && !pcWords.includes("⌘"), `a PC page's tooltips say Ctrl: ${pcWords.replace(/\n/g, " ")}`);
await pc.browser.close();

await browser.close();
done([...errors, ...pc.errors]);
