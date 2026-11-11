// The camera's probe (taste.md VIEW-10, track 0 of docs/designs/camera/design.md): the command
// palette (⇧⌘P) offers Show frame rate and Swipe probe. The frame meter shows frames a second, the
// slowest frame and each view's redraws, the same numbers `window.m2canvas.frames()` gives (all the
// page gives suites), and its own updates redraw no view; `useRenderCount` counts a zoom's redraw of
// the arrangement and no other view's. The probe asks about the Mac first and prints it first; it
// makes room in the timeline and the Graph, and puts their zoom back after a pinch and on closing.
// Round A only listens: Chrome's own scrolling moves the arrangement, a control under the pointer
// sees the wheel, and the probe hears it. Round B cancels and stops the wheel over the arrangement
// and the Graph before any control, and moves each itself, once; a pinch's scrolls stay out of its
// count. Known synthetic swipes, a diagonal flick and a swipe WebKit took over are read back from
// the summary, and a long step's raw events are cut. The report is copied or, where the clipboard
// refuses, shown in a field selected for ⌘C. Closed, it hears nothing, its frame loop stops and the
// web view scrolls again; the song's files never changed.
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
await key("Meta+Shift+KeyP");
await page.waitForSelector("[data-testid=palette] input");
await key("Meta+Shift+KeyP");
check(await until(async () => !(await has("[data-testid=palette]"))), "and so does ⇧⌘P again");

// ---------------------------------------------------------------- the frame meter

check(
  (await page.evaluate(() => Object.keys(window.m2canvas ?? {}).join())) === "frames" && (await frames())?.fps === null,
  `the page gives suites only frames(), and before the meter shows it measures no frames: ${JSON.stringify(await frames())}`,
);
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
    `frames() gives the meter's numbers: ${JSON.stringify(f)} against ${JSON.stringify(m)}`,
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

const tl = "[data-testid=timeline]", gr = "[data-testid=graph]", panel = "[data-testid=swipe-probe]";
/** The timeline's zoom, as Bass's 32-beat clip's width. */
const clipW = async () => (await box('[data-lane="bass.notes"] [data-clip]'))?.width ?? 0;
/** The track the Graph shows, and its zoom. */
const graphShows = () => page.$eval(`${gr} [data-module]`, (e) => e.dataset.module).catch(() => null);
const graphZoom = () => page.$eval("[data-zoom]", (e) => e.dataset.zoom).catch(() => null);
const roomOf = (sel) => page.$eval(sel, (e) => ({ x: e.scrollWidth - e.clientWidth, y: e.scrollHeight - e.clientHeight, w: e.clientWidth, h: e.clientHeight }));
/** The step the panel shows ("A3"), and whether the probe has made room for it. */
const atNow = () => page.$eval(panel, (e) => e.dataset.at ?? "").catch(() => "");
const roomed = () =>
  until(async () => {
    const a = await atNow();
    return !!a && (await page.$eval(panel, (e) => e.dataset.room ?? "")).startsWith(`${a} `);
  }, 8000);
/** On to the next step: Next, or the scrollbar answer. */
const advance = async (answer = "no") => {
  const a = await atNow();
  await page.click((await has("[data-probe=next]")) ? "[data-probe=next]" : `[data-probe=${answer}]`);
  await until(async () => (await atNow()) !== a);
};
const goTo = async (to) => {
  for (let i = 0; i < 40 && (await atNow()) !== to; i++) await advance();
  await roomed();
};
/**
 * Wheel events at known times (ms after the first), dispatched at once on `sel`, as a trackpad's
 * would come; untrusted, so only the probe moves anything by them.
 */
const feed = (sel, list) =>
  page.evaluate(
    (sel, list) => {
      const el = document.querySelector(sel);
      const base = performance.now() - 5000;
      for (const w of list) {
        const e = new WheelEvent("wheel", { deltaX: w.dx ?? 0, deltaY: w.dy ?? 0, bubbles: true, cancelable: w.cancelable ?? true });
        Object.defineProperty(e, "timeStamp", { value: base + w.t });
        el.dispatchEvent(e);
      }
    },
    sel,
    list,
  );
/** `n` swipes of `each` events `apart` ms apart, `between` ms from one's last to the next's first. */
const swipesOf = (n, each, apart, between, d, f = () => ({})) => {
  const out = [];
  let t = 0;
  for (let i = 0; i < n; i++)
    for (let k = 0; k < each; k++) {
      out.push({ t, ...d, ...f(i, k) });
      t += k < each - 1 ? apart : between;
    }
  return out;
};
/** A pinch, as WebKit sends it: Safari's gesture events. */
const pinch = (sel, events) =>
  page.evaluate(
    (sel, events) => {
      const el = document.querySelector(sel);
      for (const [type, scale] of events) {
        const e = new UIEvent(type, { bubbles: true, cancelable: true });
        Object.defineProperty(e, "scale", { value: scale });
        Object.defineProperty(e, "clientX", { value: 700 });
        el.dispatchEvent(e);
      }
    },
    sel,
    events,
  );
const lanes = async () => {
  const t = await box(tl);
  return { x: t.x + 600, y: t.y + 120 };
};

// The views as they are before the probe: it puts back what it changes.
const was = { width: await clipW(), module: await graphShows(), zoom: await graphZoom() };

await run("swipe");
check(await until(() => has(panel), 3000), "Swipe probe opens its panel");
const intro = await text(panel);
check(
  /big-song\.mjs/.test(intro) && /changes nothing in your song/i.test(intro) && /Any song works/.test(intro) && !/browser/i.test(intro),
  `its first page says what it does, that any song works, and how to make the big song: ${intro}`,
);
await lintIn(panel, "the probe's first page");
await page.click("[data-probe=start]");

// About the Mac: what the page can't see.
const mac = await text(panel);
check(
  /About your Mac/.test(mac) && /About This Mac/.test(mac) && /Refresh rate/.test(mac) && (await has('[data-mac-model="MacBook Air"]')) && (await has('[data-mac-rate="ProMotion"]')),
  `then it asks about the Mac: ${mac}`,
);
await lintIn(panel, "the Mac's page");
await page.click('[data-mac-model="MacBook Air"]');
await page.type("[data-mac=more]", "M2, 2022");
await page.type("[data-mac=macos]", "15.1");
await page.click('[data-mac-rate="60 Hz"]');
await page.click("[data-probe=next]");

// Round A's first step: the timeline, zoomed in by the probe until it has room to move.
check(await roomed(), "round A's first step shows once the probe has made room");
const stepA = await text(panel);
check(
  /Round A/.test(stepA) && /as it does today/i.test(stepA) && /three times/.test(stepA) && /lifting your fingers/.test(stepA) && /1 of 14/.test(stepA) && /tracks at the top/.test(stepA) && /thin bar at the right or bottom edge/.test(stepA) && (await has("[data-probe=yes]")),
  `it asks for three swipes, says where, and asks about the scrollbar, saying what it is: ${stepA}`,
);
await lintIn(panel, "round A's step");
{
  const r = await roomOf(tl);
  check(r.x >= r.w && (await clipW()) > was.width, `the probe zoomed the timeline in until it has a view's width of room: ${JSON.stringify(r)}`);
}
// A control under the pointer: a wheel listener of a lane's own, as a knob's or a dot's.
await page.evaluate(() => {
  window.__seen = 0;
  document.querySelector("[data-testid=timeline] [data-lane]").addEventListener("wheel", () => window.__seen++);
});
{
  const s0 = await scrollOf(tl);
  const at = await lanes();
  await page.mouse.move(at.x, at.y);
  await page.mouse.wheel({ deltaX: 240 });
  check(await until(async () => (await scrollOf(tl)).left > s0.left, 3000), `round A: the wheel scrolls the arrangement, as today: ${JSON.stringify([s0, await scrollOf(tl)])}`);
  check(
    await until(async () => {
      const h = await heard();
      return Number(h.wheels) >= 1 && Number(h.scrolls) >= 1;
    }, 3000),
    `the probe hears the wheel over it and the box's scrolls: ${JSON.stringify(await heard())}`,
  );
  check(Number((await heard()).cancelled) === 0 && (await page.evaluate(() => window.__seen)) >= 1, `it cancels none, and a control under the pointer sees it: ${JSON.stringify(await heard())}`);
}
await pinch(tl, [["gesturestart", 1], ["gesturechange", 1.25], ["gestureend", 1.25]]);
check(await until(async () => Number((await heard()).gestures) === 3, 3000), `it hears gesturestart, gesturechange and gestureend: ${JSON.stringify(await heard())}`);
await advance("yes");

// A2: three swipes down of ten events 16 ms apart, 300 ms between them.
await roomed();
await feed(tl, swipesOf(3, 10, 16, 300, { dy: 10 }));
await advance();
// A3: one swipe of 200 events 5 ms apart: more than a step keeps raw.
await roomed();
await feed(tl, swipesOf(1, 200, 5, 0, { dx: 1, dy: 1 }));
// A5: a diagonal flick let go: its push and its glide both at 45°.
await goTo("A5");
await feed(tl, [...[2, 4, 6, 8, 10].map((v, k) => ({ t: k * 16, dx: v, dy: v })), ...Array.from({ length: 20 }, (_, k) => ({ t: (5 + k) * 16, dx: 9 - k * 0.45, dy: 9 - k * 0.45 }))]);

// The Graph's first step: the probe shows the Song's graph, zoomed in until it overflows both ways.
await goTo("A7");
{
  const r = await roomOf(gr);
  check(r.x >= 200 && r.y >= 200 && (await graphShows()) === "song", `at the Graph's first step the probe shows the Song's graph, with room both ways: ${JSON.stringify(r)}, ${await graphShows()}`);
  const step = await text(panel);
  check(/panel at the bottom/.test(step) && /dotted background/.test(step) && !/browser/i.test(step), `and says where the Graph is, and to swipe over its dotted background: ${step}`);
}

// The pinches come last in a round; the probe puts the zoom back after each.
await goTo("A13");
{
  const w0 = await clipW();
  await pinch(tl, [["gesturestart", 1], ["gesturechange", 1.5], ["gestureend", 1.5]]);
  check(await until(async () => (await clipW()) > w0 * 1.3, 3000), `A13: a pinch over the timeline zooms it (${w0} → ${await clipW()})`);
  await advance();
  check(await until(async () => Math.abs((await clipW()) - w0) < 1, 3000), `after the pinch step the probe puts its zoom back: ${w0} → ${await clipW()}`);
}

// Round B.
await goTo("B1");
const stepB = await text(panel);
check(
  /Round B/.test(stepB) && /Cancel the swipe/.test(stepB) && /1 of 14/.test(stepB) && /window's own scrolling/.test(stepB) && !/browser/i.test(stepB) && (await has("[data-probe=no]")),
  `round B's first step asks about the scrollbar: ${stepB}`,
);
await lintIn(panel, "round B's step");
{
  const c0 = Number((await heard()).cancelled);
  const seen = await page.evaluate(() => window.__seen);
  const s0 = await scrollOf(tl);
  const at = await lanes();
  await page.mouse.move(at.x, at.y);
  await page.mouse.wheel({ deltaX: 240 });
  check(await until(async () => Number((await heard()).cancelled) > c0, 3000), `round B: the wheel over the arrangement is cancelled: ${JSON.stringify(await heard())}`);
  await sleep(400);
  const s = await scrollOf(tl);
  check(s.left === s0.left + 240, `the probe moved it by the wheel's 240 px, and the web view didn't move it too: ${JSON.stringify([s0, s])}`);
  check((await page.evaluate(() => window.__seen)) === seen, "and stopped it before any control under the pointer saw it");
}
await advance();
// B2: three swipes; WebKit takes the third over after its first event.
await roomed();
await feed(tl, swipesOf(3, 10, 16, 300, { dy: 10 }, (i, k) => ({ cancelable: !(i === 2 && k > 0) })));
await goTo("B7");
{
  const c0 = Number((await heard()).cancelled);
  const s0 = await scrollOf(gr);
  const g = await box(gr);
  await page.mouse.move(g.x + g.width - 60, g.y + 40);
  await page.mouse.wheel({ deltaY: 100 });
  check(await until(async () => Number((await heard()).cancelled) > c0, 3000), `over the Graph too: ${JSON.stringify(await heard())}`);
  await sleep(400);
  const s = await scrollOf(gr);
  check(s.top === s0.top + 100, `the probe moved the Graph by the wheel's 100 px, once: ${JSON.stringify([s0, s])}`);
}
// B13: a pinch's scrolls are the app's zoom, kept out of round B's count.
await goTo("B13");
{
  const s0 = (await scrollOf(tl)).left;
  await pinch(tl, [["gesturestart", 1], ["gesturechange", 1.4], ["gesturechange", 1.6], ["gestureend", 1.6]]);
  await until(async () => (await scrollOf(tl)).left !== s0, 3000);
  await sleep(300);
  check(Number((await heard()).theirs) === 0, `round B's scrolls in its swipe and flick steps are all the probe's, the pinch's apart: ${JSON.stringify(await heard())}`);
}

// ---------------------------------------------------------------- the report

await page.click("[data-probe=copy]");
const copied = (await until(async () => /Copied/.test(await text(panel)), 3000)) ? await page.evaluate(() => navigator.clipboard.readText()).catch((e) => `couldn't read: ${e}`) : "";
const parts = {
  "the Mac first, as said": /^music2 swipe probe[^\n]*\n[^\n]*\n\nYour Mac, as you said\n  Model: MacBook Air \(more: M2, 2022\)\n  macOS: 15\.1\n  Screen's refresh rate: 60 Hz\n/,
  "where it ran": /In the app: no/,
  "the platform": /userAgent: .*Chrome/,
  "the pixel ratio and the screen": /devicePixelRatio: [\d.]+[\s\S]*screen: \d+×\d+/,
  "the scrollbars": /Scroll bars: overlay/,
  "the page's frame rate, with its caveat": /The page's frame rate \(the web view may hold it at 60 even on a 120 Hz screen\): median [\d.]+ ms/,
  "A2's swipes at each G, against the 3 asked": /\| A2 timeline, three swipes down \| 3 \| 3 \| 3 \| 3 \| 3 \| 30 \|/,
  "the gaps of 16 and 300 ms": /12–17 ms +78 [\s\S]*250–500 ms +4 /,
  "A3's gaps of 5 ms": /4–8 ms +199 /,
  "each step's room, movement, frames, lateness and answer": /\| A1 timeline, three swipes across \| \d+ × \d+ \| 240 × 0 \| \d+ × \d+ \| \d+ a second, slowest [\d.]+ ms, over 16\.7 ms: \d+, over 8\.3 ms: \d+ \| [\d.]+ ms, [\d.]+ ms \| Yes \|/,
  "the diagonal flick and its glide at 45°": /A5 timeline, two diagonal flicks:\n    1: the flick 45°, 5 events; the glide after it [\d.]+ ms, 20 events, 45°, 50% across and 50% down/,
  "the swipe WebKit took over": /\| B2 timeline, three swipes down \| 3 \| 2 \| 1 \| 0 \|/,
  "round A's take-overs said to be normal": /its later events can't be cancelled/,
  "round B's scrolls": /In swipe and flick steps, not the probe's: 0/,
  "the pinches' scrolls apart": /In the pinch steps, the app's zoom: timeline [1-9]/,
  "the gesture events": /Gesture events: \d+ \(\d+ start, \d+ change, \d+ end\)/,
  "the deltas with fractions": /fractions/,
  "A1's raw events": /\nA1 timeline, three swipes across: 1 wheel, 3 gesture\n(?:[^\n]*\n)*?w [\d.]+ 240 0 c [^\n]*\n(?:[^\n]*\n)*?g [\d.]+ change 1\.25 700\n/,
  "A3's raw cut": /\nA3 timeline, three diagonal swipes: 200 wheel, 0 gesture; cut: 110 wheel\n/,
};
const missing = Object.entries(parts)
  .filter(([, re]) => !re.test(copied))
  .map(([what]) => what);
check(missing.length === 0, `Copy report copies the report, with each part${missing.length ? `; missing: ${missing.join(", ")}\n${copied.slice(0, 8000)}` : ""}`);
check(copied.length > 0 && copied.length < 60_000, `and it stays under 60 KB: ${copied.length} bytes`);
// Where the clipboard refuses (as a web view may), the report shows in a field, selected for ⌘C.
await page.evaluate(() => (navigator.clipboard.writeText = () => Promise.reject(new Error("refused"))));
await page.click("[data-probe=copy]");
check(await until(() => has("[data-testid=probe-report]"), 3000), "where the clipboard refuses, the report shows in a field");
const field = await page.$eval("[data-testid=probe-report]", (e) => ({ head: e.value.split("\n")[0], all: e.selectionStart === 0 && e.selectionEnd === e.value.length && e.value.length > 0, focused: document.activeElement === e }));
check(field.all && field.focused && /^music2 swipe probe/.test(field.head), `its text is selected, for ⌘C: ${JSON.stringify(field)}`);
await lintIn(panel, "the report's field");

// ---------------------------------------------------------------- closed

const before2 = await heard();
await page.click("[data-probe=close]");
check(await until(async () => !(await has(panel))), "× closes the probe");
check(
  await until(async () => Math.abs((await clipW()) - was.width) < 1 && (await graphShows()) === was.module && (await graphZoom()) === was.zoom, 3000),
  `and puts back the timeline's zoom, the track the Graph shows and its zoom: ${JSON.stringify({ was, now: { width: await clipW(), module: await graphShows(), zoom: await graphZoom() } })}`,
);
await run("frame");
check(await until(async () => !(await has("[data-testid=frame-meter]"))), "Hide frame rate hides the meter");
{
  const f0 = await frames();
  await sleep(400);
  const f1 = await frames();
  check(f0.fps === null && typeof f0.measured === "number" && f1.measured === f0.measured, `with both closed, frames() measures nothing and no frame loop runs: ${JSON.stringify([f0.measured, f1.measured])}`);
}
{
  const s0 = await scrollOf(tl);
  const at = await lanes();
  await page.mouse.move(at.x, at.y);
  await page.mouse.wheel({ deltaX: 240 });
  check(await until(async () => (await scrollOf(tl)).left === s0.left + 240, 3000), `closed, the wheel is the web view's again: it scrolls the arrangement once: ${JSON.stringify([s0, await scrollOf(tl)])}`);
}
await pinch(tl, [["gesturestart", 1], ["gestureend", 1]]);
await run("swipe");
await page.waitForSelector("[data-testid=probe-counts]");
await sleep(400);
const after = await heard();
check(after.wheels === before2.wheels && after.gestures === before2.gestures, `and the probe heard no wheel or gesture while closed: ${JSON.stringify({ before2, after })}`);
await page.click("[data-probe=close]");
check(JSON.stringify(files()) === before, "the song's files are unchanged");

await page.screenshot({ path: `${process.argv[2]}/camera-probe.png` });
await browser.close();
done(errors);
