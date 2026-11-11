// A diagonal two-finger swipe scrolls a view that scrolls both ways, both ways at once (taste.md
// §3.8), on a fresh copy of examples/first-song in a small window: the arrangement and the Graph.
// WebKit on the Mac holds a swipe to the way it mostly goes, and Chrome here never does, so the
// swipes are dispatched as plain events, which no web view scrolls by on its own: what moves is
// the view moving itself.
//   diagonal   a wheel 30 px across and 20 down moves the view 30 across and 20 down
//   straight   a wheel only across, or only down, is left to the web view: the view doesn't move it
//   zoom       a ⌘ wheel still zooms, and doesn't scroll
//   node run.mjs scroll fix
import { check, done, open, until } from "./fixlib.mjs";

const { browser, page, errors, view } = await open(1000, 600);

/** Send `sel` a wheel event, as a trackpad's swipe: pixels across and down. */
const wheel = (sel, deltaX, deltaY, mods = {}) =>
  page.$eval(sel, (e, dx, dy, m) => e.dispatchEvent(new WheelEvent("wheel", { deltaX: dx, deltaY: dy, deltaMode: 0, bubbles: true, cancelable: true, ...m })), deltaX, deltaY, mods);
const at = (sel) => page.$eval(sel, (e) => ({ left: e.scrollLeft, top: e.scrollTop, w: e.scrollWidth - e.clientWidth, h: e.scrollHeight - e.clientHeight }));

/** The checks for one view, whose content is bigger than it both ways. */
const swipes = async (name, sel) => {
  const room = await at(sel);
  check(room.w > 60 && room.h > 40, `${name}: room to scroll both ways (${room.w} across, ${room.h} down)`);
  const a = await at(sel);
  await wheel(sel, 30, 20);
  const b = await at(sel);
  check(b.left - a.left === 30 && b.top - a.top === 20, `${name} diagonal: moved ${b.left - a.left} across and ${b.top - a.top} down, for 30 and 20`);
  await wheel(sel, 30, 0);
  await wheel(sel, 0, 20);
  const c = await at(sel);
  check(c.left === b.left && c.top === b.top, `${name} straight: left to the web view (moved ${c.left - b.left}, ${c.top - b.top})`);
};

// ---------------------------------------------------------------- the arrangement
const timeline = "[data-testid=timeline]";
await page.waitForSelector(timeline);
// Zoomed in, so the song is wider than the view; every track taller, so the tracks are taller.
for (let i = 0; i < 6; i++) await wheel(timeline, 0, -100, { ctrlKey: true });
await page.click(`${timeline} [data-testid=ruler], ${timeline}`).catch(() => {});
for (let i = 0; i < 4; i++) {
  await page.keyboard.down("Control");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.up("Control");
}
await until(async () => (await at(timeline)).h > 40 && (await at(timeline)).w > 60).catch(() => {});
await swipes("timeline", timeline);
const z = await at(timeline);
// Zoomed in as far as it goes above, so this zooms out.
await wheel(timeline, 30, 100, { ctrlKey: true });
const z2 = await at(timeline);
check(z2.w < z.w && z2.top === z.top, `timeline zoom: ⌘ wheel zooms (${z.w} → ${z2.w} wide) and doesn't scroll down`);

// ---------------------------------------------------------------- the Graph
await view("graph");
const graph = "[data-testid=graph]";
await page.waitForSelector(`${graph} [data-knob]`);
for (let i = 0; i < 4; i++) await wheel(graph, 0, -200, { ctrlKey: true });
await until(async () => (await at(graph)).h > 40 && (await at(graph)).w > 60).catch(() => {});
await swipes("graph", graph);

done(errors);
await browser.close();
