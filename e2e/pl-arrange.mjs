// Polish B5 and B4, arranging (Q12 to Q14, X19): markers on the ruler, added from its right-click
// menu at a bar line, named in a field with a short list (written once, when the name is kept),
// moved by a drag on the grid (⌘ held: freely), renamed by a double-click, deleted and looped to
// from their menu; ⌥← and ⌥→ go from one to the next; a click goes there (a Ctrl-click on a Mac
// is a right-click, review PL-R4); each change one undo step named for Undo, a move inside one bar
// too (review PL-R3); a hand-written layout.json shows its markers, an unreadable one refuses in
// words. Repeat… on a clip makes copies one after another, as one undo step.
//
//   node e2e/pl-arrange.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { PROJ, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";
import fs from "node:fs";

const { browser, page, errors, api, key, box, drag, toasts } = await open(1440, 900);
const PPB = 24;
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const status = async () => (await api("/status")).body;
const state = async () => (await api("")).body;
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const at = (b, x) => Math.abs(b - x) < 1e-3;
const loopIs = (l, start, end, on) => !!l && l.start === start && l.end === end && l.on === on;
const layout = () => {
  try {
    return JSON.parse(read("layout.json"));
  } catch {
    return {};
  }
};
const markers = () => layout().markers ?? [];
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
const undo = () => key("Meta+z");
const redo = () => key("Meta+Shift+z");
const lintNow = async (what) => {
  await sleep(250);
  const bad = [...new Set(await page.evaluate(lint))];
  check(bad.length === 0, `lint, ${what}: nothing outside the design system${bad.length ? ` (${bad.slice(0, 5).join("; ")})` : ""}`);
};
const ruler = () => box("[data-testid=ruler]");
/** Right-click the ruler's lower half at beat `beat`, and the menu's rows. */
const rulerMenu = async (beat) => {
  const r = await ruler();
  await page.mouse.click(r.x + beat * PPB + 2, r.y + r.height - 6, { button: "right" });
  await page.waitForSelector("[data-testid=ruler-menu]", { timeout: 2000 }).catch(() => null);
  return page.$$eval("[data-testid=ruler-menu] [role=menuitem]", (els) => els.map((e) => `${e.textContent}${e.disabled ? `(off: ${e.title})` : ""}`));
};
const addAt = async (beat) => {
  await rulerMenu(beat);
  await page.click("[data-testid=ruler-menu] [data-menu-item=add-marker]");
  return page.waitForSelector("[data-testid=marker-field]", { timeout: 2000 }).catch(() => null);
};
const flag = (name) => box(`[data-marker-name="${name}"]`);
const fieldOpen = async () => !!(await page.$("[data-testid=marker-field]"));

// ---------------------------------------------------------------- B5: add a marker from the ruler's menu
const rows = await rulerMenu(16);
check(rows.includes("Add marker at bar 5"), `the ruler's menu has Add marker at bar 5: ${rows.join(" | ")}`);
await page.keyboard.press("Escape");
const field = await addAt(16);
check(!!field, "it opens a name field at bar 5");
const name = await page.$eval("[data-testid=marker-name]", (e) => ({ value: e.value, all: e.selectionStart === 0 && e.selectionEnd === e.value.length, focused: document.activeElement === e }));
check(name.value === "Marker 1" && name.all && name.focused, `called Marker 1, selected, to type over: ${JSON.stringify(name)}`);
const picks = await page.$$eval("[data-testid=marker-field] [data-pick]", (els) => els.map((e) => e.textContent));
check(same(picks, ["Intro", "Verse", "Pre-chorus", "Chorus", "Bridge", "Drop", "Outro"]), `with a short list of names: ${picks.join(", ")}`);
check(markers().length === 0, "nothing is written while it is named");
const ghost = await flag("Marker 1");
check(!!ghost && Math.abs(ghost.x - ((await ruler()).x + 16 * PPB)) <= 1, "it shows on the ruler where it goes");
await lintNow("the name field with its list");
await page.keyboard.type("Chorus");
await page.keyboard.press("Enter");
check(await until(() => same(markers(), [{ beat: 16, name: "Chorus" }])), `typing Chorus and Return writes it into layout.json: ${JSON.stringify(markers())}`);
check(!(await fieldOpen()), "and closes the field");
check(await until(async () => !!(await flag("Chorus"))), "the ruler shows Chorus");
const fc = await flag("Chorus");
const r0 = await ruler();
check(Math.abs(fc.x - (r0.x + 16 * PPB)) <= 1 && fc.y >= r0.y && fc.y + fc.height <= r0.y + r0.height / 2 + 1, `at bar 5, on the ruler's top half: ${JSON.stringify(fc)}`);
check(await until(async () => (await undoTitle()).includes("Undo Chorus at bar 5"), 2000), `Undo says what it takes back: ${await undoTitle()}`);
await undo();
check(await until(() => markers().length === 0), `one Undo takes the marker away: ${JSON.stringify(markers())}`);
await redo();
check(await until(() => markers().length === 1), "and Redo brings it back");
await lintNow("a marker on the ruler");

// Picking from the list; Esc; a click away.
await addAt(40);
await page.click('[data-testid=marker-field] [data-pick="Verse"]');
check(await until(() => same(markers(), [{ beat: 16, name: "Chorus" }, { beat: 40, name: "Verse" }])), `picking Verse from the list names the next, at bar 11: ${JSON.stringify(markers())}`);
await addAt(8);
await page.keyboard.press("Escape");
await sleep(400);
check(!(await fieldOpen()) && markers().length === 2 && !(await flag("Marker 1")), "Esc adds nothing");
await addAt(8);
const lane = await box('[data-lane="kick.hits"] > div:last-child');
await page.mouse.click(lane.x + 60 * PPB, lane.y + lane.height / 2);
check(await until(() => markers().some((m) => m.beat === 8 && m.name === "Marker 1")), `a click away keeps it as Marker 1: ${JSON.stringify(markers())}`);
check((await rulerMenu(16)).includes("Add marker at bar 5(off: A marker is at bar 5 already)"), "a bar with a marker can't take another: the row says why");
await page.keyboard.press("Escape");

// Its menu: Rename, Delete, Loop to next marker.
const f8 = await flag("Marker 1");
await page.mouse.click(f8.x + 4, f8.y + f8.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
const mrows = await page.$$eval("[data-testid=clip-menu] [role=menuitem]", (els) => els.map((e) => e.textContent));
check(same(mrows, ["Rename", "Delete", "Loop to next marker"]), `a marker's menu: ${mrows.join(" | ")}`);
await page.click('[data-testid=clip-menu] [data-menu-item="delete"]');
check(await until(() => markers().length === 2), "Delete takes it out");
check(await until(async () => (await undoTitle()).includes("Undo deleting Marker 1"), 2000), `Undo says so: ${await undoTitle()}`);
await undo();
check(await until(() => markers().length === 3), "Undo brings it back");
await undo();
await until(() => markers().length === 2);

// A click goes there; ⌥→ and ⌥← go from one to the next.
await post("/seek", { beat: 0 });
await until(async () => at((await status()).beat, 0));
const fv = await flag("Verse");
await page.mouse.click(fv.x + 6, fv.y + fv.height / 2);
check(await until(async () => at((await status()).beat, 40)), `a click on Verse puts the playhead there: ${(await status()).beat}`);
await until(() => page.$eval("[data-testid=playhead]", (e) => e.dataset.beat === "40"));
await post("/seek", { beat: 0 });
await until(() => page.$eval("[data-testid=playhead]", (e) => e.dataset.beat === "0"));
await key("Alt+ArrowRight");
check(await until(async () => at((await status()).beat, 16)), `⌥→ goes to the next marker: ${(await status()).beat}`);
await key("Alt+ArrowRight");
check(await until(async () => at((await status()).beat, 40)), `again, to the one after: ${(await status()).beat}`);
await key("Alt+ArrowLeft");
check(await until(async () => at((await status()).beat, 16)), `⌥← goes back: ${(await status()).beat}`);
check(await page.evaluate(() => history.length) <= 2, "and the page doesn't go back");

// A Ctrl-click on a Mac is a right-click (X7): the menu opens and the playhead stays. WebKit sends
// the press with Ctrl, then the menu's event, then the release. A Ctrl-drag moves nothing (review PL-R4).
await post("/seek", { beat: 0 });
await until(async () => at((await status()).beat, 0));
const fx = await flag("Verse");
const [cx, cy] = [fx.x + 6, fx.y + fx.height / 2];
await page.keyboard.down("Control");
await page.mouse.move(cx, cy);
await page.mouse.down();
await page.$eval('[data-marker-name="Verse"]', (e, x, y) => e.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: x, clientY: y, ctrlKey: true })), cx, cy);
await page.mouse.up();
await page.keyboard.up("Control");
await sleep(500);
const ctrlMenu = !!(await page.$("[data-testid=clip-menu]"));
check(ctrlMenu && at((await status()).beat, 0), `a Ctrl-click on a marker on a Mac opens its menu (${ctrlMenu}) and leaves the playhead where it was: beat ${(await status()).beat}`);
await page.keyboard.press("Escape");
await until(async () => !(await page.$("[data-testid=clip-menu]")), 2000);
const ctrlBefore = JSON.stringify(markers());
await page.keyboard.down("Control");
await drag({ x: cx, y: cy }, { x: cx + 4 * PPB, y: cy });
await page.keyboard.up("Control");
await sleep(600);
check(JSON.stringify(markers()) === ctrlBefore && at((await status()).beat, 0), `and a Ctrl-drag on a Mac moves no marker: ${JSON.stringify(markers())}`);
await page.keyboard.press("Escape");

// A drag moves it on the grid; ⌘ held, freely.
let f = await flag("Chorus");
await drag({ x: f.x + 6, y: f.y + f.height / 2 }, { x: f.x + 6 + 16 * PPB + 5, y: f.y + f.height / 2 });
check(await until(() => same(markers(), [{ beat: 32, name: "Chorus" }, { beat: 40, name: "Verse" }])), `a drag to bar 9 writes beat 32: ${JSON.stringify(markers())}`);
check(await until(async () => (await undoTitle()).includes("Undo moving Chorus"), 2000), `Undo says: ${await undoTitle()}`);
f = await flag("Chorus");
await page.keyboard.down("Meta");
await drag({ x: f.x + 6, y: f.y + f.height / 2 }, { x: f.x + 6 + 1.5 * PPB, y: f.y + f.height / 2 });
await page.keyboard.up("Meta");
check(await until(() => markers()[0]?.beat === 33.5), `with ⌘ held it lands off the grid: beat ${markers()[0]?.beat}`);
await undo();
check(await until(() => markers()[0]?.beat === 32), "Undo puts it back on bar 9");
// That move stayed in bar 9, and was still named as a step of its own: Undo now names the drag
// before it, not the step before that (review PL-R3).
check(await until(async () => (await undoTitle()).includes("Undo moving Chorus"), 2000), `then Undo names the drag to bar 9: ${await undoTitle()}`);
f = await flag("Verse");
await drag({ x: f.x + 6, y: f.y + f.height / 2 }, { x: f.x + 6 - 8 * PPB, y: f.y + f.height / 2 });
await sleep(500);
check(markers()[1]?.beat === 40 && (await toasts()).some((t) => t.includes("A marker is at bar 9 already")), `a drag onto another marker is refused: ${(await toasts()).join(" | ")}`);

// A double-click renames it.
f = await flag("Verse");
await page.mouse.click(f.x + 6, f.y + f.height / 2, { clickCount: 2 });
await page.waitForSelector("[data-testid=marker-field]");
check((await page.$eval("[data-testid=marker-name]", (e) => e.value)) === "Verse" && !(await page.$("[data-testid=marker-field] [data-pick]")), "a double-click opens its name, with no list");
await page.keyboard.type("Big Verse");
await page.keyboard.press("Enter");
check(await until(() => markers()[1]?.name === "Big Verse"), `renamed: ${JSON.stringify(markers())}`);
check(await until(async () => (await undoTitle()).includes("Undo renaming Verse"), 2000), `Undo says: ${await undoTitle()}`);

// Loop to next marker.
f = await flag("Chorus");
await page.mouse.click(f.x + 4, f.y + f.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
await page.click('[data-testid=clip-menu] [data-menu-item="loop"]');
check(await until(async () => loopIs((await status()).loop, 32, 40, true)), `Loop to next marker loops bars 9 to 11: ${JSON.stringify((await status()).loop)}`);
f = await flag("Big Verse");
await page.mouse.click(f.x + 4, f.y + f.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
await page.click('[data-testid=clip-menu] [data-menu-item="loop"]');
check(await until(async () => loopIs((await status()).loop, 40, 44, true)), `from the last marker past the song's end: its bar (${JSON.stringify((await status()).loop)})`);
await key("c");

// layout.json by hand: shown; one that doesn't read: refused in words.
const kept = read("layout.json");
write("layout.json", JSON.stringify({ order: [], tracks: {}, markers: [{ beat: 4, name: "Hand" }] }, null, 2));
check(await until(async () => !!(await flag("Hand")), 5000), "a layout.json written by hand shows its marker");
write("layout.json", "{ \"order\": [\n");
check(await until(async () => !(await flag("Hand")), 5000), "one that doesn't read shows none");
await addAt(24);
await page.keyboard.press("Enter");
check(await until(async () => (await toasts()).some((t) => t.includes("layout.json doesn't read: fix it, or reset the layout"))), `adding one is refused in words: ${(await toasts()).join(" | ")}`);
write("layout.json", kept);
check(await until(async () => !!(await flag("Chorus")), 5000), "put back, its markers show again");

// A marker's menu or name field still acts on its marker after the song's markers come back as a
// new list: once the window's own list gave way to the song's while the menu was open, and Delete
// took nothing out (the Mac's and Linux's pl-arrange-fix retries).
const withMarker = (m) => {
  const l = JSON.parse(kept);
  l.markers = [...(l.markers ?? []), m];
  return JSON.stringify(l, null, 2);
};
f = await flag("Chorus");
await page.mouse.click(f.x + 4, f.y + f.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
write("layout.json", withMarker({ beat: 60, name: "Outro" }));
await until(async () => !!(await flag("Outro")), 5000);
await page.click('[data-testid=clip-menu] [data-menu-item="delete"]');
check(await until(() => same(markers(), [{ beat: 40, name: "Big Verse" }, { beat: 60, name: "Outro" }])), `Delete from a menu opened before the list came back new takes out its marker: ${JSON.stringify(markers())}`);
write("layout.json", kept);
await until(async () => !!(await flag("Chorus")) && !(await flag("Outro")), 5000);
f = await flag("Big Verse");
await page.mouse.click(f.x + 6, f.y + f.height / 2, { clickCount: 2 });
await page.waitForSelector("[data-testid=marker-field]");
write("layout.json", withMarker({ beat: 60, name: "Outro" }));
await until(async () => !!(await flag("Outro")), 5000);
await page.$eval("[data-testid=marker-name]", (e) => (e.focus(), e.select()));
await page.keyboard.type("Last Verse");
await page.keyboard.press("Enter");
check(await until(() => markers()[1]?.name === "Last Verse"), `and a rename begun before it renames its marker: ${JSON.stringify(markers())}`);
write("layout.json", kept);
check(await until(async () => !!(await flag("Chorus")) && !(await flag("Outro")), 5000), "put back again");

// ---------------------------------------------------------------- B4: repeat a clip
const bassStarts = () => JSON.parse(read("data/bass/notes.json")).clips.map((c) => c.start);
const repeatFrom = async () => {
  const b = await box('[data-lane="bass.notes"] [data-clip]');
  await page.mouse.click(b.x + 40, b.y + b.height / 2, { button: "right" });
  await page.waitForSelector("[data-testid=clip-menu]");
};
await repeatFrom();
const ids = await page.$$eval("[data-testid=clip-menu] [data-menu-item]", (els) => els.map((e) => e.dataset.menuItem));
check(ids.includes("repeat") && ids.indexOf("repeat") === ids.indexOf("duplicate") + 1, `the clip menu has Repeat… after Duplicate: ${ids.join(", ")}`);
await page.click('[data-testid=clip-menu] [data-menu-item="repeat"]');
await page.waitForSelector("[data-testid=repeat-popover]");
const rv = await page.$eval("[data-testid=repeat-times]", (e) => ({ value: e.value, focused: document.activeElement === e }));
const words = await page.$eval("[data-testid=repeat-popover]", (e) => e.innerText.replace(/\s+/g, " ").trim());
check(rv.value === "3" && rv.focused && words === "Repeat more times Repeat", `it asks "Repeat [3] more times", the 3 ready to type over: ${JSON.stringify(rv)} ${words}`);
await lintNow("the Repeat popover");
await page.keyboard.press("Enter");
check(await until(() => same(bassStarts(), [0, 32, 64, 96])), `Return makes three Bass clips after the first, one after another: ${bassStarts()}`);
check(await until(async () => (await page.$$('[data-lane="bass.notes"] [data-clip][data-picked]')).length === 3), "the copies are selected");
const ids3 = JSON.parse(read("data/bass/notes.json")).clips.map((c) => c.id);
check(new Set(ids3).size === 4, `each with an id of its own: ${ids3.join(", ")}`);
check(await until(async () => (await undoTitle()).includes("Undo repeating a Bass clip"), 2000), `Undo says: ${await undoTitle()}`);
await undo();
check(await until(() => same(bassStarts(), [0])), "one Undo takes all three back");
await repeatFrom();
await page.click('[data-testid=clip-menu] [data-menu-item="repeat"]');
await page.waitForSelector("[data-testid=repeat-popover]");
await page.keyboard.press("Escape");
await sleep(400);
check(!(await page.$("[data-testid=repeat-popover]")) && same(bassStarts(), [0]), "Esc closes it and repeats nothing");
await repeatFrom();
await page.click('[data-testid=clip-menu] [data-menu-item="repeat"]');
await page.waitForSelector("[data-testid=repeat-popover]");
await page.keyboard.type("65");
const bad = await page.$eval("[data-testid=repeat-popover]", (e) => ({ say: e.querySelector("[role=alert]")?.textContent, off: e.querySelector("[data-testid=repeat-go]").disabled }));
await page.keyboard.press("Enter");
await sleep(300);
check(bad.say === "Repeat 1 to 64 times" && bad.off && same(bassStarts(), [0]), `65 is refused in words: ${JSON.stringify(bad)}`);
await page.keyboard.press("Escape");

// ---------------------------------------------------------------- a PC page's words
const pc = await open(1440, 900, { pc: true });
await pc.page.waitForSelector('[data-marker-name="Chorus"]');
const pcTitle = await pc.page.$eval('[data-marker-name="Chorus"]', (e) => e.title);
check(pcTitle.includes("Alt+← and Alt+→") && pcTitle.includes("hold Ctrl"), `on a PC, a marker's tooltip: ${pcTitle.replace(/\n/g, " | ")}`);
await pc.browser.close();
errors.push(...pc.errors);

await browser.close();
done(errors);
