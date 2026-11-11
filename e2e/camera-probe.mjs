// The camera's probe (taste.md VIEW-10, track 0 of docs/designs/camera/design.md): the command
// palette (⇧⌘P) offers Show frame rate and Swipe probe. The frame meter shows frames a second, the
// slowest frame and each view's redraws, the same numbers `canvas.frames()` gives (the page's
// `window.m2canvas`), and its own updates redraw no view; `useRenderCount` counts a zoom's redraw of
// the arrangement and no other view's. The probe's round A only listens: Chrome's own scrolling moves
// the arrangement, and the probe hears the wheel and the scrolls. Round B cancels the wheel over the
// arrangement and the Graph and moves each itself, once, so neither moves twice; gesture events are
// heard; the report holds each part of its summary and the raw events, copied or, where the clipboard
// refuses, in a field selected for ⌘C. Closed, it hears nothing and the browser scrolls again; the
// song's files never changed.
//
//   node e2e/camera-probe.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { PROJ, check, done, open, sess, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";
import { tips } from "./tiplib.mjs";

const { browser, page, errors, key, box, hold } = await open(1440, 900);
await browser.defaultBrowserContext().overridePermissions(sess.url, ["clipboard-read", "clipboard-write", "clipboard-sanitized-write"]);

/** The song's own files (all but `.music2/`, the window's), each by its hash. */
const files = (dir = PROJ, out = {}) => {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) e.name !== ".music2" && files(p, out);
    else out[path.relative(PROJ, p)] = crypto.createHash("sha256").update(fs.readFileSync(p)).digest("hex");
  }
  return out;
};
const before = JSON.stringify(files());

const frames = () => page.evaluate(() => window.m2canvas?.frames?.() ?? null);
const has = async (sel) => !!(await page.$(sel));
const text = (sel) => page.$eval(sel, (e) => e.textContent ?? "").catch(() => "");
const scrollOf = (sel) => page.$eval(sel, (e) => ({ left: e.scrollLeft, top: e.scrollTop }));
const setScroll = (sel, left, top) => page.$eval(sel, (e, left, top) => void (e.scrollLeft = left, e.scrollTop = top), left, top);
const lintIn = async (sel, what) => {
  const bad = [...new Set(await page.evaluate(lint, sel))];
  const tipless = await page.evaluate(tips, sel);
  check((await has(sel)) && bad.length === 0 && tipless.length === 0, `${what}: nothing outside the design system, every control with its tooltip${[...bad, ...tipless].length ? ` (${[...bad, ...tipless].slice(0, 5).join("; ")})` : ""}`);
};
/** The palette's rows: each one's command and words. */
const rows = () => page.$$eval("[data-testid=palette] [data-command]", (els) => els.map((e) => `${e.dataset.command}: ${e.textContent?.trim()}`));
/** Open the palette and run the row whose words hold `words`. */
const run = async (words) => {
  await key("Meta+Shift+KeyP");
  await page.waitForSelector("[data-testid=palette] input");
  await page.type("[data-testid=palette] input", words);
  await until(async () => (await rows()).length === 1);
  await key("Enter");
  await until(async () => !(await has("[data-testid=palette]")));
};
/** What the probe has heard, as its panel counts it. */
const heard = () => page.$eval("[data-testid=probe-counts]", (e) => ({ ...e.dataset })).catch(() => ({}));

// ---------------------------------------------------------------- the palette

await key("Meta+Shift+KeyP");
check(await until(() => has("[data-testid=palette]"), 3000), "⇧⌘P opens the command palette");
const all = await rows();
check(all.some((r) => r.startsWith("frames: Show frame rate")) && all.some((r) => r.startsWith("probe: Swipe probe")), `it offers Show frame rate and Swipe probe: ${all.join(" | ")}`);
await lintIn("[data-testid=palette]", "the palette");
await page.type("[data-testid=palette] input", "swipe");
check(await until(async () => (await rows()).join() === (await rows()).filter((r) => r.startsWith("probe:")).join() && (await rows()).length === 1), `typing "swipe" leaves Swipe probe alone: ${(await rows()).join(" | ")}`);
await key("Escape");
check(await until(async () => !(await has("[data-testid=palette]"))), "Esc closes it");

// ---------------------------------------------------------------- the frame meter

check((await frames())?.fps === null, `before the meter shows, canvas.frames() measures no frames: ${JSON.stringify(await frames())}`);
await run("frame");
check(await until(() => has("[data-testid=frame-meter]"), 3000), "Show frame rate shows the meter");
check(
  await until(async () => Number(await page.$eval("[data-testid=frame-meter]", (e) => e.dataset.fps)) > 0, 3000),
  `it counts frames a second: ${await text("[data-testid=frame-meter]")}`,
);
{
  const m = await page.$eval("[data-testid=frame-meter]", (e) => ({ fps: Number(e.dataset.fps), slowest: Number(e.dataset.slowest), text: e.textContent ?? "" }));
  const f = await frames();
  check(
    f && f.fps > 0 && f.slowest > 0 && Math.abs(f.fps - m.fps) <= Math.max(6, m.fps * 0.2) && m.slowest > 0 && /frames a second/.test(m.text) && /slowest/i.test(m.text),
    `canvas.frames() gives the meter's numbers: ${JSON.stringify(f)} against ${JSON.stringify(m)}`,
  );
  check(
    /Timeline/.test(m.text) && /Graph/.test(m.text) && /Notes/.test(m.text) && /Beat/.test(m.text) && f && "arrangement" in f.total && "arrangement.rows" in f.total && "graph" in f.total && "graph.nodes" in f.total,
    `it names each view's redraws, and frames() counts the arrangement, its rows, the Graph and its nodes: ${m.text} / ${JSON.stringify(f?.total)}`,
  );
}
const meter = await box("[data-testid=frame-meter]");
const ruler = await box("[data-testid=timeline] [data-testid=ruler]");
const tools = await box("[data-testid=view-tools]");
check(
  !!meter && !!ruler && !!tools && meter.y >= ruler.y + ruler.height && meter.x + meter.width > tools.x && (await page.$eval("[data-testid=frame-meter]", (e) => getComputedStyle(e).pointerEvents)) === "none",
  `the meter sits at the right under the ruler, clear of the top bar's controls and the ruler's tools, and takes no clicks: ${JSON.stringify({ meter, ruler, tools })}`,
);
await lintIn("[data-testid=frame-meter]", "the meter");

// The window's own redraws stop while its status isn't answered (the 200 ms poll draws it): then
// the meter's updates alone go on, and they redraw no view.
let release = null;
const statusHeld = new Promise((r) => (release = r));
const unhold = await hold((r) => /\/api\/graph\/status$/.test(new URL(r.url()).pathname) && { until: statusHeld });
await sleep(600);
{
  const t0 = (await frames()).total;
  const at0 = await page.$eval("[data-testid=frame-meter]", (e) => e.dataset.at);
  await sleep(1200);
  const t1 = (await frames()).total;
  const at1 = await page.$eval("[data-testid=frame-meter]", (e) => e.dataset.at);
  const views = ["arrangement", "arrangement.rows", "graph", "graph.nodes"];
  check(at1 !== at0 && views.every((v) => t1[v] === t0[v]), `a second of the meter's updates redraws no view: ${JSON.stringify({ at0, at1, t0, t1 })}`);

  // ⌘= zooms the arrangement: it is redrawn, and no other view is.
  await key("Meta+Equal");
  await sleep(500);
  const t2 = (await frames()).total;
  const others = ["graph", "graph.nodes", "notes", "notes.keys", "beat", "beat.rows"];
  check(t2.arrangement > t1.arrangement && others.every((v) => (t2[v] ?? 0) === (t1[v] ?? 0)), `a zoom redraws the arrangement, and no other view: ${JSON.stringify({ t1, t2 })}`);
  const f = await frames();
  check(f.renders.arrangement >= 1, `and the meter's last second counts it: ${JSON.stringify(f.renders)}`);
}
release();
await unhold();

// ---------------------------------------------------------------- the probe's panel

// Room to scroll: the arrangement zoomed in, the Graph at 200%; both at their start.
for (let i = 0; i < 3; i++) await key("Meta+Equal");
for (let i = 0; i < 4; i++) await page.click('[data-zoom] [aria-label="Zoom in"]');
await sleep(400);
const roomOf = (sel) => page.$eval(sel, (e) => ({ x: e.scrollWidth - e.clientWidth, y: e.scrollHeight - e.clientHeight }));
const roomT = await roomOf("[data-testid=timeline]"), roomG = await roomOf("[data-testid=graph]");
check(roomT.x > 600 && roomG.y > 100, `the arrangement and the Graph have room to scroll: ${JSON.stringify({ roomT, roomG })}`);
await setScroll("[data-testid=timeline]", 0, 0);
await setScroll("[data-testid=graph]", 0, 0);

await run("swipe");
check(await until(() => has("[data-testid=swipe-probe]"), 3000), "Swipe probe opens its panel");
const intro = await text("[data-testid=swipe-probe]");
check(/big-song\.mjs/.test(intro) && /changes nothing in your song/i.test(intro), `its first page says what it does and how to make the big song: ${intro}`);
await lintIn("[data-testid=swipe-probe]", "the probe's first page");
await page.click("[data-probe=start]");
const stepA = await text("[data-testid=swipe-probe]");
check(/Round A/.test(stepA) && /as it does today/i.test(stepA) && /swipe straight across/i.test(stepA) && /1 of 12/.test(stepA), `round A's first step: ${stepA}`);
await lintIn("[data-testid=swipe-probe]", "round A's step");

// Round A: Chrome's own scrolling moves the arrangement, and the probe hears it.
const lanes = async () => {
  const t = await box("[data-testid=timeline]");
  return { x: t.x + 600, y: t.y + 120 };
};
{
  const at = await lanes();
  await page.mouse.move(at.x, at.y);
  await page.mouse.wheel({ deltaX: 240 });
  check(await until(async () => (await scrollOf("[data-testid=timeline]")).left > 0, 3000), `round A: the wheel scrolls the arrangement, as today: ${JSON.stringify(await scrollOf("[data-testid=timeline]"))}`);
  check(
    await until(async () => {
      const h = await heard();
      return Number(h.wheels) >= 1 && Number(h.scrolls) >= 1;
    }, 3000),
    `the probe hears the wheel over it and the box's scrolls: ${JSON.stringify(await heard())}`,
  );
  check(Number((await heard()).cancelled) === 0, `and cancels none: ${JSON.stringify(await heard())}`);
}
// A pinch, as WebKit sends it: Safari's gesture events.
await page.evaluate(() => {
  const el = document.querySelector("[data-testid=timeline]");
  for (const [type, scale] of [["gesturestart", 1], ["gesturechange", 1.25], ["gestureend", 1.25]]) {
    const e = new UIEvent(type, { bubbles: true, cancelable: true });
    Object.defineProperty(e, "scale", { value: scale });
    Object.defineProperty(e, "clientX", { value: 700 });
    el.dispatchEvent(e);
  }
});
check(await until(async () => Number((await heard()).gestures) === 3, 3000), `it hears gesturestart, gesturechange and gestureend: ${JSON.stringify(await heard())}`);

// To round B, through round A's twelve steps, the arrangement back at its start.
await setScroll("[data-testid=timeline]", 0, 0);
await sleep(200);
for (let i = 0; i < 12; i++) await page.click("[data-probe=next]");
const stepB = await text("[data-testid=swipe-probe]");
check(/Round B/.test(stepB) && /cancel the swipe/i.test(stepB) && /1 of 12/.test(stepB) && (await has("[data-probe=yes]")) && (await has("[data-probe=no]")), `round B's first step asks about the scrollbar: ${stepB}`);
await lintIn("[data-testid=swipe-probe]", "round B's step");

// Round B: the wheel is cancelled, and the probe moves the box itself, once.
{
  const c0 = Number((await heard()).cancelled);
  const at = await lanes();
  await page.mouse.move(at.x, at.y);
  await page.mouse.wheel({ deltaX: 240 });
  check(await until(async () => Number((await heard()).cancelled) > c0, 3000), `round B: the wheel over the arrangement is cancelled: ${JSON.stringify(await heard())}`);
  await sleep(400);
  const s = await scrollOf("[data-testid=timeline]");
  check(s.left === 240, `the probe moved it by the wheel's 240 px, and the browser didn't move it too: ${JSON.stringify(s)}`);
}
{
  const c0 = Number((await heard()).cancelled);
  const g = await box("[data-testid=graph]");
  await page.mouse.move(g.x + g.width / 2, g.y + g.height / 2);
  await page.mouse.wheel({ deltaY: 100 });
  check(await until(async () => Number((await heard()).cancelled) > c0, 3000), `over the Graph too: ${JSON.stringify(await heard())}`);
  await sleep(400);
  const s = await scrollOf("[data-testid=graph]");
  check(s.top === 100, `the probe moved the Graph by the wheel's 100 px, once: ${JSON.stringify(s)}`);
}
check(Number((await heard()).theirs) === 0, `round B's scrolls are all the probe's: ${JSON.stringify(await heard())}`);
await page.click("[data-probe=no]");
check(/2 of 12/.test(await text("[data-testid=swipe-probe]")), "No answers the scrollbar question and goes on");

// ---------------------------------------------------------------- the report

await page.click("[data-probe=copy]");
const copied = (await until(async () => /Copied/.test(await text("[data-testid=swipe-probe]")), 3000))
  ? await page.evaluate(() => navigator.clipboard.readText()).catch((e) => `couldn't read: ${e}`)
  : "";
const parts = {
  "a title": /^music2 swipe probe/m,
  "the platform": /userAgent: .*HeadlessChrome|userAgent: .*Chrome/,
  "navigator.platform": /platform: \S+/,
  "the pixel ratio": /devicePixelRatio: [\d.]+/,
  "the screen": /screen: \d+×\d+/,
  "the frames' median and 95th percentile": /Frames: median [\d.]+ ms .*95th percentile [\d.]+ ms/,
  "a histogram of the gaps": /Gaps between wheel events[\s\S]*?\d+–\d+ ms/,
  "the swipes in each step at several gaps": /Swipes in each step[\s\S]*?G = 50 ms/,
  "what came after a flick": /After each flick/,
  "the events that couldn't be cancelled": /cancelable: false/,
  "round B's scrolls": /Round B's scrolls[\s\S]*?not the probe's: 0/,
  "the events with both deltas": /both deltas/,
  "the deltas' fractions": /fractions/,
  "the gesture events": /Gesture events: 3 \(1 start, 1 change, 1 end\)/,
  "the scrollbar answers": /Scrollbar[\s\S]*?B1 timeline, swipe straight across: No/,
  "the raw events": /Raw events[\s\S]*\nw [\d.]+ A1 [\s\S]*\ng [\d.]+ A1 change 1\.25 700[\s\S]*\nw [\d.]+ B1 /,
};
const missing = Object.entries(parts).filter(([, re]) => !re.test(copied)).map(([what]) => what);
check(missing.length === 0, `Copy report copies the report, with each part${missing.length ? `; missing: ${missing.join(", ")}\n${copied.slice(0, 3000)}` : ""}`);
// Where the clipboard refuses (as a web view may), the report shows in a field, selected for ⌘C.
await page.evaluate(() => (navigator.clipboard.writeText = () => Promise.reject(new Error("refused"))));
await page.click("[data-probe=copy]");
check(await until(() => has("[data-testid=probe-report]"), 3000), "where the clipboard refuses, the report shows in a field");
const field = await page.$eval("[data-testid=probe-report]", (e) => ({ head: e.value.split("\n")[0], all: e.selectionStart === 0 && e.selectionEnd === e.value.length && e.value.length > 0, focused: document.activeElement === e }));
check(field.all && field.focused && /^music2 swipe probe/.test(field.head), `its text is selected, for ⌘C: ${JSON.stringify(field)}`);
await lintIn("[data-testid=swipe-probe]", "the report's field");

// ---------------------------------------------------------------- closed

const was = await heard();
await page.click("[data-probe=close]");
check(await until(async () => !(await has("[data-testid=swipe-probe]"))), "× closes the probe");
{
  const s0 = await scrollOf("[data-testid=timeline]");
  const at = await lanes();
  await page.mouse.move(at.x, at.y);
  await page.mouse.wheel({ deltaX: 240 });
  check(await until(async () => (await scrollOf("[data-testid=timeline]")).left === s0.left + 240, 3000), `closed, the wheel is the browser's again: it scrolls the arrangement once: ${JSON.stringify([s0, await scrollOf("[data-testid=timeline]")])}`);
}
await run("swipe");
await page.waitForSelector("[data-testid=probe-counts]");
await sleep(400);
const now = await heard();
check(now.wheels === was.wheels && now.scrolls === was.scrolls && now.gestures === was.gestures, `and the probe heard nothing while closed: ${JSON.stringify({ was, now })}`);
await page.click("[data-probe=close]");
await run("frame");
check(await until(async () => !(await has("[data-testid=frame-meter]"))), "Hide frame rate hides the meter");
check(JSON.stringify(files()) === before, "the song's files are unchanged");

await page.screenshot({ path: `${process.argv[2]}/camera-probe.png` });
await browser.close();
done(errors);
