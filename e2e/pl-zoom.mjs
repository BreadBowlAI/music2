// Polish B1 to B3, the timeline's view (Q8 to Q11, Q31, X24, X25, X33 to X35): zoom with ⌘= and
// ⌘− around the playhead, with ⌘-scroll and a pinch (Ctrl-scroll) around the pointer, and with the
// ruler's buttons; Z fits the selected clip or the song, and Z again goes back; the ruler numbers
// thin out; the grid's Auto follows the zoom. The view follows the playhead while it plays, pages
// at 85%, pauses when scrolled by hand, and can be turned off. Track heights: a header's bottom
// edge, ⌘↓ and ⌘↑, a double-click back to 44; an automation lane's edge. Each is kept per song in
// `.music2/view.json`. And a PC page's words.
//
//   node e2e/pl-zoom.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, check, done, open, read, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { browser, page, errors, api, key, box, drag } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const status = async () => (await api("/status")).body;
const state = async () => (await api("")).body;
const near = (a, b, d = 1) => Math.abs(a - b) <= d;
const view = () => {
  try {
    return JSON.parse(read(".music2/view.json"));
  } catch {
    return {};
  }
};
const scroll = () => page.$eval("[data-testid=timeline]", (e) => e.scrollLeft);
const setScroll = (x) => page.$eval("[data-testid=timeline]", (e, x) => void (e.scrollLeft = x), x);
const clip = (lane) => box(`[data-lane="${lane}"] [data-clip]`);
const playheadX = async () => (await box("[data-testid=playhead]")).x;
/** The px a beat the timeline draws Bass's 32-beat clip at. */
const ppb = async () => ((await clip("bass.notes")).width + 2) / 32;
/** The lanes' visible width: the timeline's, less the headers'. */
const visible = async () => page.$eval("[data-testid=timeline]", (e) => e.clientWidth - 240);
const lintNow = async (what) => {
  await sleep(250);
  const bad = [...new Set(await page.evaluate(lint))];
  check(bad.length === 0, `lint, ${what}: nothing outside the design system${bad.length ? ` (${bad.slice(0, 5).join("; ")})` : ""}`);
};
/** Close the song, write `.music2/view.json`, and open it again: what the page shows when it does. */
const reopen = async (v) => {
  await post("/close", {});
  if (v) fs.writeFileSync(`${PROJ}/.music2/view.json`, JSON.stringify(v));
  const opened = await post("/open", { path: PROJ });
  await page.waitForSelector('[data-lane="bass.notes"] [data-clip]');
  await sleep(300);
  return opened.status === 200;
};

// ---------------------------------------------------------------- B1: the ruler's tools
check(!!(await page.$("[data-testid=view-tools] [data-testid=zoom-in]")) && !!(await page.$("[data-testid=view-tools] [data-testid=zoom-out]")), "the ruler has Zoom in and Zoom out");
const tools = await box("[data-testid=view-tools]");
const tl = await box("[data-testid=timeline]");
check(!!tools && near(tools.x + tools.width, tl.x + tl.width, 1) && near(tools.height, 28, 1), `at the right of the ruler's row, 28 px tall: ${tools && JSON.stringify(tools)}`);
const titles = await page.$$eval("[data-testid=view-tools] button", (els) => els.map((e) => e.title));
check(titles.join(" | ") === "Follow the playhead (⇧⌘F) | Zoom out (⌘−) | Zoom in (⌘=)", `their tooltips: ${titles.join(" | ")}`);

// ---------------------------------------------------------------- ⌘= and ⌘− around the playhead
const w0 = (await clip("bass.notes")).width;
await post("/seek", { beat: 8 });
await until(() => page.$eval("[data-testid=playhead]", (e) => e.dataset.beat === "8"));
const p0 = await playheadX();
await key("Meta+Equal");
await until(async () => (await clip("bass.notes")).width > w0 + 10, 3000);
const w1 = (await clip("bass.notes")).width;
check(near(w1 + 2, (w0 + 2) * 1.5), `⌘= zooms in: clip width ${w0} → ${w1}`);
const p1 = await playheadX();
check(near(p1, p0), `around the playhead: it stays at x ${p0.toFixed(1)} (${p1.toFixed(1)})`);
await key("Meta+Minus");
await until(async () => (await clip("bass.notes")).width < w1 - 10, 3000);
check(near((await clip("bass.notes")).width, w0) && near(await playheadX(), p0), `⌘− zooms back out, the playhead where it was: ${(await clip("bass.notes")).width}`);
await page.click("[data-testid=zoom-in]");
await until(async () => (await clip("bass.notes")).width > w0 + 10, 3000);
check(near((await clip("bass.notes")).width, w1), "the Zoom in button does what ⌘= does");
await page.click("[data-testid=zoom-out]");
await until(async () => (await clip("bass.notes")).width < w1 - 10, 3000);
check(near((await clip("bass.notes")).width, w0), "and Zoom out what ⌘− does");
check((await page.evaluate(() => document.activeElement?.closest("[data-testid=view-tools]") === null)), "the buttons never take the keyboard (Space still plays)");

// ---------------------------------------------------------------- a pinch and ⌘-scroll: around the pointer
/** The beat under screen x, from where Bass's clip (at beat 0) is drawn. */
const beatAt = async (x) => {
  const c = await clip("bass.notes");
  return (x - (c.x - 1)) / ((c.width + 2) / 32);
};
const row = await box('[data-lane="bass.notes"]');
const px = row.x + 240 + 12 * 24 + 7;
const py = row.y + row.height / 2;
await page.mouse.move(px, py);
const b0 = await beatAt(px);
await page.keyboard.down("Meta");
await page.mouse.wheel({ deltaY: -100 });
await page.keyboard.up("Meta");
await until(async () => (await clip("bass.notes")).width > w0 + 10, 3000);
const b1 = await beatAt(px);
check((await ppb()) > 30 && Math.abs(b1 - b0) * (await ppb()) <= 2, `⌘-scroll zooms in around the pointer: beat ${b0.toFixed(2)} stays under it (${b1.toFixed(2)}), ${(await ppb()).toFixed(1)} px a beat`);
await page.keyboard.down("Control");
await page.mouse.wheel({ deltaY: 40 });
await page.keyboard.up("Control");
await sleep(300);
const b2 = await beatAt(px);
check((await ppb()) < 36 && Math.abs(b2 - b0) * (await ppb()) <= 2, `a pinch (Chrome's: a wheel with Ctrl) zooms out around it too: beat ${b2.toFixed(2)}, ${(await ppb()).toFixed(1)} px a beat`);
check((await page.evaluate(() => window.visualViewport?.scale ?? 1)) === 1, "the page itself never zooms");

// ---------------------------------------------------------------- Z: fit the selected clip, or the song; again, back
await key("Meta+Minus");
await key("Meta+Equal");
await sleep(200);
const before = { ppb: await ppb(), scroll: await scroll() };
const kc = await clip("keys.chords");
await page.mouse.click(kc.x + 30, kc.y + kc.height / 2);
await until(() => page.$('[data-lane="keys.chords"] [data-clip][data-picked]'));
await key("z");
await sleep(300);
const fit = await clip("keys.chords");
const vis = await visible();
check(fit.width > vis * 0.9 && fit.x >= tl.x + 240 && fit.x + fit.width <= tl.x + 240 + vis, `Z with Keys' clip selected: it fills the view (${fit.width.toFixed(0)} of ${vis} px, from x ${fit.x.toFixed(0)})`);
await key("z");
await sleep(300);
check(near(await ppb(), before.ppb, 0.01) && near(await scroll(), before.scroll), `Z again: back to the zoom and scroll before (${(await ppb()).toFixed(2)} px a beat, scrolled ${await scroll()})`);
await key("Escape");
await key("Escape");
await key("z");
await sleep(300);
const all = await clip("bass.notes");
check(all.x >= tl.x + 240 && all.x + all.width <= tl.x + tl.width && all.width > vis * 0.9, `Z with nothing selected: the whole song fits (${all.width.toFixed(0)} px)`);
await key("z");
await sleep(300);

// ---------------------------------------------------------------- the grid's Auto follows the zoom
check(await reopen({ ...view(), zoom: 24 }), "opened again at 24 px a beat");
const autoTitle = (await page.$eval("[data-testid=song-grid]", (e) => e.title)).split("\n")[0];
check(near(await ppb(), 24, 0.01) && autoTitle.includes("Auto is 1/8 at this zoom"), `at 24 px a beat, Auto is 1/8: ${autoTitle}`);
await key("Meta+Equal");
await key("Meta+Equal");
await until(async () => near(await ppb(), 54, 0.05), 3000);
const auto54 = (await page.$eval("[data-testid=song-grid]", (e) => e.title)).split("\n")[0];
check(auto54.includes("Auto is 1/16 at this zoom"), `two ⌘= later (54 px a beat), Auto is 1/16: ${auto54}`);
const kick0 = await clip("kick.hits");
await drag({ x: kick0.x + 60, y: kick0.y + kick0.height / 2 }, { x: kick0.x + 60 + 40, y: kick0.y + kick0.height / 2 });
check(await until(async () => (await state()).timeline.tracks.find((t) => t.module === "kick").lanes[0].clips[0].start === 0.75), `a clip dragged 40 px lands on a sixteenth: beat ${(await state()).timeline.tracks.find((t) => t.module === "kick").lanes[0].clips[0].start}`);
await key("Meta+z");
await until(async () => (await state()).timeline.tracks.find((t) => t.module === "kick").lanes[0].clips[0].start === 0);

// ---------------------------------------------------------------- kept per song
check(await until(() => view().zoom === 54), `.music2/view.json keeps the zoom: ${JSON.stringify(view())}`);
check(await reopen(), "the song is closed and opened again");
check(near(await ppb(), 54, 0.05), `its zoom is back: ${(await ppb()).toFixed(2)} px a beat`);
await lintNow("zoomed in");

check(await reopen({ ...view(), zoom: 4 }), "opened again at 4 px a beat");
const numbers = await page.$$eval("[data-testid=ruler] [data-bar]", (els) => els.map((e) => e.textContent));
check(numbers.slice(0, 4).join() === "1,5,9,13", `the ruler numbers every 4th bar: ${numbers.slice(0, 6).join(", ")}`);
check(await page.$eval("[data-testid=zoom-out]", (e) => !e.disabled), "Zoom out is still on at 4 px a beat");
await lintNow("zoomed out");
for (let i = 0; i < 3; i++) await key("Meta+Minus");
await sleep(200);
check(near(await ppb(), 2, 0.01) && (await page.$eval("[data-testid=zoom-out]", (e) => e.disabled && e.title === "Zoomed out all the way")), `at 2 px a beat Zoom out is off: "Zoomed out all the way" (${(await ppb()).toFixed(2)})`);

// ---------------------------------------------------------------- B3: follow the playhead
check(await reopen({ ...view(), zoom: 48 }), "opened again at 48 px a beat");
const follow = () => page.$eval("[data-testid=follow]", (e) => e.getAttribute("aria-pressed"));
check((await follow()) === "true", "the Follow button is there, on at first");
await setScroll(0);
await post("/seek", { beat: 60 });
await key("Space");
await until(async () => (await status()).playing);
check(await until(async () => (await scroll()) > 2000, 3000), `play from beat 60: the view pages to the playhead (scrolled ${await scroll()})`);
const at15 = (await playheadX()) - (tl.x + 240);
check(at15 > 0 && at15 < (await visible()) * 0.3, `which sits near the view's left: ${at15.toFixed(0)} px in`);
await post("/seek", { beat: 120 });
check(await until(async () => (await scroll()) > 5000, 3000), `the playhead past the view's right: it pages again (scrolled ${await scroll()})`);
// A scroll by hand while playing pauses it, until play starts again.
await setScroll(0);
await post("/seek", { beat: 60 });
await sleep(800);
check((await scroll()) < 50, `a scroll by hand while playing: the view stays where it was put (${await scroll()})`);
await key("Space");
await until(async () => !(await status()).playing);
await key("Space");
check(await until(async () => (await scroll()) > 2000, 3000), `play again: it follows again (${await scroll()})`);
await page.click("[data-testid=follow]");
check((await follow()) === "false" && (await page.$eval("[data-testid=follow]", (e) => e.title)) === "Not following the playhead (⇧⌘F)", "the Follow button turns it off, and says so");
const kept = await scroll();
await post("/seek", { beat: 140 });
await sleep(800);
check(near(await scroll(), kept), `off: the view stays (${await scroll()})`);
await key("Meta+Shift+KeyF");
check(await until(async () => (await follow()) === "true"), "⇧⌘F turns it on");
check(await until(async () => (await scroll()) > kept + 500, 3000), `and the view pages to the playhead (${await scroll()})`);
await key("Meta+Shift+KeyF");
await key("Space");
await until(async () => !(await status()).playing);
check(await until(() => view().follow === false), `off is kept in view.json: ${JSON.stringify(view())}`);
await reopen();
check((await follow()) === "false", "and is still off when the song opens again");
await page.click("[data-testid=follow]");
await until(() => view().follow === true);

// ---------------------------------------------------------------- B2: track heights
check(await reopen({ zoom: 24 }), "opened again at 24 px a beat, every track at its usual height");
const rowH = async (id) => (await box(`[data-lane="${id}"]`)).height;
const selected = () => page.$$eval("[data-module-tab][aria-selected=true]", (els) => els.map((e) => e.dataset.moduleTab));
let edge = await box('[data-height-edge="bass"]');
check(!!edge && near(edge.height, 6) && near(edge.y + edge.height, (await box('[data-lane="bass.notes"]')).y + 44), "Bass's header has an edge to drag: 6 px at its foot");
const edgeTitle = await page.$eval('[data-height-edge="bass"]', (e) => e.title);
check(edgeTitle === "Drag to change Bass's height · double-click for the usual height", `its tooltip: ${edgeTitle}`);
const keysTop = (await box('[data-track="keys"]')).y;
await drag({ x: edge.x + 100, y: edge.y + 3 }, { x: edge.x + 100, y: edge.y + 3 + 28 });
check(await until(async () => near(await rowH("bass.notes"), 72)), `dragged down 28 px, Bass's row is 72 px: ${await rowH("bass.notes")}`);
check(near((await clip("bass.notes")).height, 64), `its clip draws 64 px tall: ${(await clip("bass.notes")).height}`);
check(near((await box('[data-track="keys"]')).y, keysTop + 28), "the next track moved 28 px down");
const top = (await box('[data-lane="bass.notes"]')).y;
const mute = await box('[data-mute="bass"]'), solo = await box('[data-solo="bass"]'), meter = await box('[data-track="bass"] [data-slot="meter"]');
check(near(mute.y - top, 12) && near(solo.y - top, 12) && near(meter.y - top, 8) && near(meter.height, 28), `M, S and the meter stay in its top 44 px: M ${mute.y - top}, S ${solo.y - top}, meter ${meter.y - top} and ${meter.height} tall`);
check(await until(() => view().heights?.bass === 72), `kept in view.json: ${JSON.stringify(view().heights)}`);
edge = await box('[data-height-edge="bass"]');
await page.mouse.click(edge.x + 100, edge.y + 3, { clickCount: 2 });
check(await until(async () => near(await rowH("bass.notes"), 44)), `a double-click puts back 44: ${await rowH("bass.notes")}`);
check(await until(() => view().heights === undefined), "and view.json keeps no height for it");

await page.click('[data-module-tab="kick"]');
await key("Meta+ArrowDown");
check(await until(async () => near(await rowH("kick.hits"), 72)) && near(await rowH("hats.hits"), 44), "⌘↓ with Kick selected: Kick is 72 px, Hats stays 44");
await key("Meta+ArrowUp");
check(await until(async () => near(await rowH("kick.hits"), 44)), "⌘↑: back to 44");
await page.click('[data-module-tab="hats"]');
await key("Meta+ArrowUp");
await until(async () => near(await rowH("hats.hits"), 28));
const htop = (await box('[data-lane="hats.hits"]')).y;
const hm = await box('[data-mute="hats"]'), hmeter = await box('[data-track="hats"] [data-slot="meter"]');
check(near(await rowH("hats.hits"), 28) && near(hm.y - htop, 4) && near(hmeter.height, 20), `⌘↑ on Hats: 28 px, M centred in it (${hm.y - htop}), the meter 20 tall (${hmeter.height})`);
await key("Meta+ArrowDown");
await until(async () => near(await rowH("hats.hits"), 44));
await key("Escape");
await key("Escape");
await until(async () => (await selected()).length === 0);
await key("Meta+ArrowDown");
const tall = await Promise.all(["kick.hits", "hats.hits", "bass.notes", "keys.chords"].map(rowH));
check(tall.every((h) => near(h, 72)), `with no track selected, ⌘↓ makes every track 72 px: ${tall.join(", ")}`);
await key("Meta+ArrowUp");
await until(async () => near(await rowH("keys.chords"), 44));

await page.click('[data-fold="bass"]');
await page.waitForSelector('[data-lane="bass.cutoff"]');
const le = await box('[data-height-edge="bass.cutoff"]');
check(!!le && near(await rowH("bass.cutoff"), 32), "an automation lane has the edge too, at 32 px");
await drag({ x: le.x + 100, y: le.y + 3 }, { x: le.x + 100, y: le.y + 3 + 32 });
check(await until(async () => near(await rowH("bass.cutoff"), 64)), `dragged down 32 px: 64 (${await rowH("bass.cutoff")})`);
await page.click('[data-module-tab="bass"]');
await key("Meta+ArrowDown");
check(await until(() => view().heights?.bass === 72 && view().heights?.["bass.cutoff"] === 64), `view.json keeps both: ${JSON.stringify(view().heights)}`);
await lintNow("a tall track and a tall lane");
await reopen();
await page.click('[data-fold="bass"]');
await page.waitForSelector('[data-lane="bass.cutoff"]');
check(near(await rowH("bass.notes"), 72) && near(await rowH("bass.cutoff"), 64), `opened again, Bass is 72 px and its Cutoff lane 64 (${await rowH("bass.notes")}, ${await rowH("bass.cutoff")})`);

// ---------------------------------------------------------------- a PC page's words
const pc = await open(1440, 900, { pc: true });
await pc.page.waitForSelector("[data-testid=view-tools]");
const pcTitles = await pc.page.$$eval("[data-testid=view-tools] button", (els) => els.map((e) => e.title));
check(pcTitles.join(" | ") === "Follow the playhead (Ctrl+Shift+F) | Zoom out (Ctrl+−) | Zoom in (Ctrl+=)", `on a PC: ${pcTitles.join(" | ")}`);
await pc.browser.close();
errors.push(...pc.errors);

await browser.close();
done(errors);
