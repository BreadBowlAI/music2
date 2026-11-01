// Clips by id in the timeline and the piano roll (FMT-11, FMT-12), on a fresh s4 server with the
// converted first-song:  node s4-clips.mjs <root> <home> <project>
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, state, toasts, show, view, key, box, drag } = await open();
const lane = (path) => JSON.parse(read(path)).clips;
const clipEls = (l) => page.$$eval(`[data-lane="${l}"] [data-clip]`, (els) => els.map((e) => ({ id: e.dataset.clip, start: Number(e.dataset.start), open: !!e.dataset.open, picked: !!e.dataset.picked })));
const kick0 = lane("data/kick/hits.json");
check(kick0.length === 1 && /^[0-9a-z]{8}$/.test(kick0[0].id), `the kick's clip has an id (${kick0[0].id})`);

// ---------------------------------------------------------------- open by id, edit, the id stays
const k = await box(`[data-lane="kick.hits"] [data-clip="${kick0[0].id}"]`);
await page.mouse.click(k.x + 40, k.y + k.height / 2, { count: 2 });
check(await until(() => page.$("[data-testid=roll-grid]")), "double-clicking the clip opens the piano roll");
// The roll's place is in the dock's header (taste.md §3.5): the track, then after the switcher the
// bars; the Kick's one notes lane needs no name beside the Notes tab.
const header = await page.$eval("[data-testid=dock-header]", (e) => e.innerText.replace(/\s+/g, " "));
check(/^Kick Device Notes Mix Code bars 1–8/.test(header), `the roll names the clip by labels and bars: ${header.slice(0, 40)}`);
check((await clipEls("kick.hits"))[0].open, "and the timeline outlines it");
/** A point in the roll's grid at `beat`, halfway down what the scroller shows. */
const inRoll = async (beat) => {
  const g = await box("[data-testid=roll-grid]"), sc = await box("[data-testid=roll-scroller]");
  const ppb = await page.$eval("[data-testid=roll-grid]", (e) => e.getBoundingClientRect().width);
  return { x: g.x + beat * 48 + 5, y: sc.y + sc.height / 2 + 7 };
};
const before = read("data/kick/hits.json");
let p = await inRoll(0.5);
await page.mouse.click(p.x, p.y, { count: 2 });
check(await until(() => read("data/kick/hits.json") !== before), "a double-click in the roll adds a note: the lane is written");
const kick1 = lane("data/kick/hits.json");
check(kick1.length === 1 && kick1[0].id === kick0[0].id && kick1[0].notes.length === kick0[0].notes.length + 1, "the clip keeps its id, with one note more");
await sleep(300);
check(!!(await page.$("[data-testid=notes]")) && (await page.$$("[data-testid=notes] [data-note]")).length === kick1[0].notes.length, "the roll stays open on it");

// ---------------------------------------------------------------- move a clip: same id, new start
await page.click('[data-view-tab="graph"]');
const hats0 = lane("data/hats/hits.json");
const h = await box(`[data-lane="hats.hits"] [data-clip="${hats0[0].id}"]`);
await drag({ x: h.x + 30, y: h.y + h.height / 2 }, { x: h.x + 30 + 24 * 4, y: h.y + h.height / 2 });
check(await until(() => lane("data/hats/hits.json")[0].start === 4), "dragging the hats clip a bar right moves it to beat 4");
check(lane("data/hats/hits.json")[0].id === hats0[0].id, "keeping its id");
check(await until(async () => (await clipEls("hats.hits"))[0]?.picked), "and it stays picked");

// ---------------------------------------------------------------- make a clip: a new id, picked and open
const kr = await box('[data-lane="kick.hits"]');
await page.mouse.click(kr.x + 240 + 24 * 34 + 5, kr.y + kr.height / 2, { count: 2 });
check(await until(() => lane("data/kick/hits.json").length === 2), "double-clicking after the clip makes one");
const made = lane("data/kick/hits.json")[1];
check(made.start === 32 && made.length === 4 && /^[0-9a-z]{8}$/.test(made.id) && made.id !== kick0[0].id, `a bar long, with a new id (${made.id})`);
check(await until(async () => (await clipEls("kick.hits")).find((c) => c.id === made.id)?.open === true), "it opens in the roll");
check(await until(async () => (await page.$eval("[data-testid=dock-header]", (e) => e.innerText).catch(() => "")).includes("bar 9")), "at bar 9");

// ---------------------------------------------------------------- delete it, undo
await page.click(`[data-lane="kick.hits"] [data-clip="${made.id}"]`);
await key("Backspace");
check(await until(() => lane("data/kick/hits.json").length === 1), "Delete removes the picked clip");
// Notes stays the view, saying how to pick a clip (taste.md §3.7).
check(await until(async () => !(await page.$("[data-testid=roll-grid]")) && !!(await page.$("[data-testid=notes][data-empty]"))), "and its roll closes");
await page.click('button[aria-label="Undo"]');
check(await until(() => lane("data/kick/hits.json").length === 2 && lane("data/kick/hits.json")[1].id === made.id), "undo brings it back with its id");

// ---------------------------------------------------------------- an anonymous clip
const anon = { clips: [{ start: 0, length: 16, notes: [{ pitch: "C3", start: 0, length: 1 }] }, { id: "x2", start: 16, length: 16, notes: [] }] };
write("data/keys/chords.json", JSON.stringify(anon));
check(await until(async () => (await clipEls("keys.chords")).length === 2), "a clip written without an id shows");
const els = await clipEls("keys.chords");
check(els[0].id === "" && els[1].id === "x2", `named by id only when it has one (${els.map((e) => e.id || "-")})`);
check(await until(async () => (await state()).problems?.some((p) => p.code === "clip.anonymous")), "the state carries clip.anonymous");
const a = await box('[data-lane="keys.chords"] [data-clip=""]');
await page.mouse.click(a.x + 20, a.y + a.height / 2, { count: 2 });
check(await until(() => page.$("[data-testid=roll-grid]")), "the anonymous clip opens");
p = await inRoll(2);
await page.mouse.click(p.x, p.y, { count: 2 });
check(await until(() => lane("data/keys/chords.json")[0].id !== undefined), "an edit writes it with an id");
const named = lane("data/keys/chords.json");
check(/^[0-9a-z]{8}$/.test(named[0].id) && named[1].id === "x2" && named[0].notes.length === 2, `the app chose it, kept the other's (${named.map((c) => c.id)})`);
await sleep(400);
check(!!(await page.$("[data-testid=notes]")), "the roll stays open on it");
check(await until(async () => (await clipEls("keys.chords"))[0].id === named[0].id && (await clipEls("keys.chords"))[0].open), "now named by its id");
p = await inRoll(3);
await page.mouse.click(p.x, p.y, { count: 2 });
check(await until(() => lane("data/keys/chords.json")[0].notes.length === 3), "a second edit lands on the same clip");
check(lane("data/keys/chords.json")[0].id === named[0].id, "with the same id");
await page.screenshot({ path: `${ROOT}/s4-clips.png` });
await browser.close();
done(errors);
