// The camera (taste.md §3.10, VIEW-1 to VIEW-12; docs/designs/camera/design.md §9): every view moves
// as the hand does, through one camera each. Chrome can't show WebKit's own behaviour, so the suite
// sends the events WebKit would send, at known times, on whatever is under the pointer.
//   library   the camera's functions against known numbers, run in the page (`cullPlan` too); the
//             writer on a box that keeps whole pixels, as WebKit does; its numbers
//   graph     on first-song: a diagonal swipe 1:1; 120 events over a wire render nothing; a swipe that
//             carries a picture's dot under the pointer, and one 60 ms after over it, change no file,
//             one that starts on it is the dot's, to its end; Tab to a bar out of view is taken in
//   beat      a kit made as a musician makes one: ⌘-scroll and a pinch keep the step under the pointer
//   twin      a fresh copy of first-song opened over it (no close), its Graph showing: its own zoom,
//             its Graph at Fit and zooming
//   big song  24 tracks of 64 clips (big-song.mjs), Bass 1's first clip of 256 notes, opened straight
//             after the twin (no close): its own zoom, its clips placed by it, its Graph zooming
//     arrangement  a diagonal swipe 1:1, 5° on its rail, one whose first 16 px go 20° off on it too,
//                  45° and 30° free, a turn breaking free, a long 3° swipe with jitter and its glide on
//                  the rail, ⇧ not turned; the owner's own swipes from the probe of 2026-11-14
//                  (`fixtures/swipes`): the straight ones on their rails, the diagonal ones
//                  free; a swipe WebKit took over moving once; an edge
//                  stops it; content getting smaller stops it (`clamp`); ⌘ in a dropped glide zooms
//                  nothing; a right-click holds nothing; 120 events across frames render nothing
//                  within a step, the rows once at a step; no more clips than the view and its margins
//                  can show, none in tracks far below; a swipe down the tracks and back; 30 ⌘-scroll
//                  events render it at most once a frame; a pinch keeps the beat; a zoom renders no
//                  clip's mini drawing, and its notes have no group opacity; a click's scroll taken
//                  in; agents' `seen` follows; a swipe, a key's scroll and a write to `scrollLeft`
//                  pause Follow, a swipe straight down doesn't, and Follow never takes a swipe back to
//                  the playhead; a drag holds the camera and Follow; the camera's own glide with
//                  Linux's flag, stopped by an edge, and none after a mouse's notches
//     notes        a diagonal swipe 1:1; no more notes than the view and its margins show; 120 events
//                  over a note render nothing; 30 ⌘-scroll events at most once a frame; ⌘-scroll and a
//                  pinch keep the beat under the pointer
//
//   node e2e/camera.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { PROJ, ROOT, check, done, open, read, sess, sleep, until } from "./fixlib.mjs";
import { expand } from "./graphlib.mjs";

const { browser, page, errors, api, state, key, box, show, view } = await open(1440, 900);

const near = (a, b, d = 1) => Math.abs(a - b) <= d;
const json = (o) => JSON.stringify(o);
const has = async (sel) => !!(await page.$(sel));
/** A few frames: what a frame's events asked is written, and a render it made has landed. */
const frames = (n = 3) => page.evaluate((n) => new Promise((r) => { const one = (k) => (k ? requestAnimationFrame(() => one(k - 1)) : r()); one(n); }), n);
/** The camera library's face, where the page gives it (VIEW-10): null where it doesn't. */
const face = () => page.evaluate(() => !!window.m2canvas?.camera);
const cam = (name) => page.evaluate((name) => window.m2canvas?.camera?.now(name) ?? null, name);
/** Each view's renders since the page loaded (`useRenderCount`). */
const renders = () => page.evaluate(() => window.m2canvas?.frames?.().total ?? {});
const since = (a, b) => Object.fromEntries([...new Set([...Object.keys(a), ...Object.keys(b)])].map((k) => [k, (b[k] ?? 0) - (a[k] ?? 0)]).filter(([, n]) => n));
const scroll = (sel) => page.$eval(sel, (e) => ({ x: e.scrollLeft, y: e.scrollTop, w: e.clientWidth, h: e.clientHeight, room: { x: e.scrollWidth - e.clientWidth, y: e.scrollHeight - e.clientHeight } }));
const setScroll = (sel, x, y) => page.$eval(sel, (e, x, y) => { if (x !== null) e.scrollLeft = x; if (y !== null) e.scrollTop = y; }, x ?? null, y ?? null);
/**
 * Wheel events at known times (ms after the first), each dispatched on whatever is under (x, y) then,
 * as a trackpad's: `dx`, `dy`, `mode`, `cancelable`, `ctrl`, `meta`, `shift`. `spread` ms apart in
 * real time (0: all in one task).
 */
const feed = (x, y, list, spread = 0) =>
  page.evaluate(
    async (x, y, list, spread) => {
      // A second after the last feed's last event at least: each feed is a swipe of its own.
      const base = Math.max(performance.now(), (window.__stamp ?? 0) + 1000);
      window.__stamp = base + (list.at(-1)?.t ?? 0);
      for (const w of list) {
        const el = document.elementFromPoint(x, y) ?? document.body;
        const e = new WheelEvent("wheel", {
          deltaX: w.dx ?? 0,
          deltaY: w.dy ?? 0,
          deltaMode: w.mode ?? 0,
          clientX: x,
          clientY: y,
          ctrlKey: !!w.ctrl,
          metaKey: !!w.meta,
          shiftKey: !!w.shift,
          bubbles: true,
          cancelable: w.cancelable ?? true,
        });
        Object.defineProperty(e, "timeStamp", { value: base + w.t });
        el.dispatchEvent(e);
        if (spread) await new Promise((r) => setTimeout(r, spread));
      }
    },
    x,
    y,
    list,
    spread,
  );
/** `n` events of a swipe, `apart` ms apart, each moving (dx, dy). */
const swipe = (n, dx, dy, apart = 16, more = {}) => Array.from({ length: n }, (_, i) => ({ t: i * apart, dx, dy, ...more }));
/** A pinch at (x, y), as WebKit sends it: Safari's gesture events, each scale from the pinch's start. */
const pinch = (x, y, scales) =>
  page.evaluate(
    (x, y, scales) => {
      const el = document.elementFromPoint(x, y);
      const send = (type, scale) => {
        const e = new UIEvent(type, { bubbles: true, cancelable: true });
        for (const [k, v] of Object.entries({ scale, clientX: x, clientY: y })) Object.defineProperty(e, k, { value: v });
        el.dispatchEvent(e);
      };
      send("gesturestart", 1);
      for (const s of scales) send("gesturechange", s);
      send("gestureend", scales.at(-1));
    },
    x,
    y,
    scales,
  );
/**
 * `n` frames of ⌘-scroll at (x, y), two events in each (`dy` each), each event a task of its own, as
 * a trackpad's are: however fast the page draws its frames. Then three frames more.
 */
const zoomFrames = (x, y, n, dy) =>
  page.evaluate(
    (x, y, n, dy) =>
      new Promise((done) => {
        let k = 0;
        const one = () => {
          const send = () => {
            const e = new WheelEvent("wheel", { deltaY: dy, clientX: x, clientY: y, metaKey: true, bubbles: true, cancelable: true });
            (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(e);
          };
          send();
          setTimeout(send, 0);
          if (++k < n) requestAnimationFrame(one);
          else requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(done)));
        };
        requestAnimationFrame(one);
      }),
    x,
    y,
    n,
    dy,
  );

// ================================================================ the library, in the page

check(await face(), "the page gives the camera's face beside frames() (VIEW-10)");
const lib = await page.evaluate(() => {
  const c = window.m2canvas?.camera;
  if (!c) return null;
  const n = c.numbers;
  // An arrangement's shape: 240 px of names at the left, a 36 px ruler, the zoom stretching time across.
  const arr = { view: { w: 1240, h: 600 }, content: { w: 5000, h: 2000 }, margins: { left: 240, top: 36, right: 0, bottom: 50 }, origin: { x: 240, y: 36 }, stretch: { x: true, y: false } };
  // A graph's: 16 px of padding, stretched both ways.
  const gr = { view: { w: 800, h: 400 }, content: { w: 2000, h: 1000 }, margins: { left: 0, top: 0, right: 0, bottom: 0 }, origin: { x: 16, y: 16 }, stretch: { x: true, y: true } };
  const at = { x: 100, y: 50, zoom: 24 };
  const out = {};
  out.numbers = [n.SWIPE_GAP, n.RAIL_ANGLE, n.RAIL_AFTER, n.RAIL_TURN, n.RAIL_RECENT, n.GLIDE_RATE, n.SCROLLBARS, n.EDGES, n.CULL_MARGIN, n.CULL_STEP, n.SYSTEM_GLIDE.mac, n.SYSTEM_GLIDE.other];
  out.move = [c.move(at, 30, -20, arr), c.move(at, -500, -500, arr), c.move(at, 9000, 9000, arr), c.to(at, 10.25, 20.5, arr)];
  // 300 px into the lanes at beat (100 + 300) / 24; at 48 px a beat it stays under the pointer.
  out.zoom = [c.zoomAround(at, { x: 540, y: 300 }, 48, arr), c.zoomAround(at, { x: 100, y: 300 }, 48, arr), c.zoomAround({ x: 0, y: 0, zoom: 1 }, { x: 216, y: 116 }, 2, gr)];
  out.reveal = [
    c.reveal(at, { x: [2000, 2100] }, arr),
    c.reveal(at, { x: [400, 500] }, arr),
    c.reveal(at, { x: [100, 200] }, arr),
    c.reveal(at, { x: [400, 3000] }, arr),
    c.reveal(at, { y: [800, 840] }, arr),
    c.reveal(at, { x: [2000, 2100] }, arr, { align: "center" }),
    c.reveal(at, { x: [2000, 2100] }, arr, { pad: 10 }),
  ];
  out.fit = [c.fit(at, [8, 16], arr, { pad: 16 }), c.fit(at, [8, 16], arr, { pad: 16, zoomOf: (z) => Math.min(z, 60) })];
  const u = c.toContent(at, { x: 540, y: 300 }, arr);
  const r6 = (p) => ({ x: Math.round(p.x * 1e6) / 1e6, y: Math.round(p.y * 1e6) / 1e6 });
  out.convert = [r6(u), r6(c.toBox(at, u, arr)), c.toContent({ x: 50, y: 20, zoom: 2 }, { x: 116, y: 96 }, gr)];
  // A window is kept while it holds what shows and half a view more each side, and isn't twice as
  // wide as a new one: so a pan moves it every half a view, a zoom only every ×1.5 out or ×2 in.
  const rw = (w) => [Math.round(w.from * 1e6) / 1e6, Math.round(w.to * 1e6) / 1e6];
  const fresh = c.cull({ ...at, x: 0 }, arr);
  const after = (cam) => c.cull({ ...at, ...cam }, arr, {}, fresh).x;
  out.cull = [
    rw(fresh.x),
    after({ x: 499 }) === fresh.x,
    rw(after({ x: 520 })),
    after({ x: 0, zoom: 48 }) === fresh.x,
    rw(after({ x: 0, zoom: 60 })),
    rw(after({ x: 0, zoom: 16 })),
    rw(c.cull({ ...at, y: 0 }, arr).y),
    rw(c.cull(at, { ...arr, view: { w: 0, h: 0 } }).x),
    rw(c.cull({ ...at, x: 0 }, arr, { x: { margin: 4, step: 2 } }).x),
  ];
  // What a view does with its window (VIEW-12): keep it, ask for a new one between frames, or draw
  // one at once, judged by the window on screen (committed), not the one asked for last.
  const plan = (x, committed, asked) => {
    const p = c.cullPlan?.({ ...at, x }, arr, {}, committed, asked);
    return p ? [p.now, rw(p.next.x)] : null;
  };
  const W0 = fresh, W1 = c.cull({ ...at, x: 520 }, arr);
  out.plan = [plan(100, W0, W0), plan(520, W0, W0), plan(560, W0, W1), plan(1010, W0, W1), plan(1100, W0, W1), plan(1100, W1, W1)];
  // A box that keeps whole pixels, as WebKit's does (`long scrollLeft`).
  const box = { left: 0, top: 0, scrollWidth: 2000, scrollHeight: 1000, clientWidth: 500, clientHeight: 300 };
  const whole = {
    get scrollLeft() { return box.left; },
    set scrollLeft(v) { box.left = Math.max(0, Math.min(this.scrollWidth - this.clientWidth, Math.trunc(v))); },
    get scrollTop() { return box.top; },
    set scrollTop(v) { box.top = Math.max(0, Math.min(this.scrollHeight - this.clientHeight, Math.trunc(v))); },
    scrollWidth: 2000, scrollHeight: 1000, clientWidth: 500, clientHeight: 300,
  };
  const wr = c.writer(whole);
  let cam = { x: 0, y: 0, zoom: 1 };
  // A slow swipe: 30 events of 0.3 px. Reading the box and adding would never leave 0.
  for (let i = 0; i < 30; i++) cam = wr.write({ ...cam, x: cam.x + 0.3 });
  out.slow = [Math.round(cam.x * 1000) / 1000, box.left, wr.theirs()];
  box.left += 1;
  out.slack = wr.theirs();
  box.left += 40;
  out.theirs = wr.theirs();
  out.clamped = wr.write({ x: 5000, y: -10, zoom: 1 });
  return out;
});
check(
  json(lib?.numbers) === json([50, 25, 16, 25, 100, 0.998, "browser", "stop", 1, 0.5, true, false]),
  `its numbers, from the owner's swipe probe of 2026-11-14: G 50 ms, rails within 25° after 16 px, broken when the last 100 px turn 25° off, its glide 0.998 a ms and only where the system gives none, the browser's scrollbars, edges stop, a view's margin in steps of half a view: ${json(lib?.numbers)}`,
);
check(
  json(lib?.move) === json([{ x: 130, y: 30, zoom: 24 }, { x: 0, y: 0, zoom: 24 }, { x: 3760, y: 1400, zoom: 24 }, { x: 10.25, y: 20.5, zoom: 24 }]),
  `move and to: by and to a point, with fractions, stopped at the content's edges: ${json(lib?.move)}`,
);
check(
  json(lib?.zoom) === json([{ x: 500, y: 50, zoom: 48 }, { x: 200, y: 50, zoom: 48 }, { x: 200, y: 100, zoom: 2 }]),
  `zoomAround: the beat under the pointer stays (over the names: at their edge), and the graph's point both ways: ${json(lib?.zoom)}`,
);
check(
  json(lib?.reveal?.map((r) => [r.x, r.y])) === json([[860, 50], [100, 50], [0, 50], [160, 50], [100, 290], [1310, 50], [870, 50]]),
  `reveal: the least move clear of the names and the ruler, none when it shows, its start when too big, the middle, a pad: ${json(lib?.reveal?.map((r) => [r.x, r.y]))}`,
);
check(
  json(lib?.fit) === json([{ x: 952, y: 50, zoom: 121 }, { x: 464, y: 50, zoom: 60 }]),
  `fit: beats 8 to 16 fill the view 16 px in from each side, the zoom clamped as asked: ${json(lib?.fit)}`,
);
check(
  json(lib?.convert) === json([{ x: 16.666667, y: 314 }, { x: 540, y: 300 }, { x: 75, y: 50 }]),
  `toContent and back: the box's point as beats and px, and the graph's both ways: ${json(lib?.convert)}`,
);
check(
  json(lib?.cull) === json([[-41.666667, 83.333333], true, [-20, 105], true, [-16.666667, 33.333333], [-62.5, 125], [-514, 1028], [0, 0], [-4, 45.666667]]),
  `cull: what shows and a view each side; kept within half a view of a pan, a new one past it; kept zooming in ×2, new at ×2.5 in and ×1.5 out; down too; nothing before the box is laid out; a margin of its own: ${json(lib?.cull)}`,
);
check(
  json(lib?.plan) ===
    json([
      ["keep", [-41.666667, 83.333333]],
      ["wait", [-20, 105]],
      ["keep", [-20, 105]],
      ["urgent", [-20, 105]],
      ["urgent", [4.166667, 129.166667]],
      ["wait", [4.166667, 129.166667]],
    ]),
  `cullPlan: a pan within the window keeps it; past a step it asks for a new one between frames; while that waits, a view that leaves the window on screen draws it at once: ${json(lib?.plan)}`,
);
check(
  json(lib?.slow) === json([9, 9, null]) && lib?.slack === null && json(lib?.theirs) === json({ x: 50, y: 0 }) && json(lib?.clamped) === json({ x: 1500, y: 0, zoom: 1 }),
  `the writer on a box that keeps whole pixels: 30 moves of 0.3 px reach 9 (${json(lib?.slow)}), 1 px off is its own (${json(lib?.slack)}), 40 more the browser's (${json(lib?.theirs)}), stopped at the box's edges (${json(lib?.clamped)})`,
);

// ================================================================ the Graph, on first-song

const graph = "[data-testid=graph]";
await view("graph");
await page.waitForSelector(`${graph} [data-node-id]`);
const graphCam = async () => {
  const names = await page.evaluate(() => window.m2canvas?.camera?.names() ?? []);
  const name = names.find((n) => n.startsWith("graph:"));
  return name ? { name, ...(await cam(name)) } : null;
};
// Room both ways: zoomed in at the graph's corner.
{
  const n = await box(`${graph} [data-node-id]`);
  for (let i = 0; i < 2; i++) {
    await feed(n.x + 2, n.y + 2, [{ t: 0, dy: -100, ctrl: true }]);
    await frames();
  }
}
{
  const g = await box(graph);
  const s0 = await scroll(graph);
  const at = { x: g.x + g.width * 0.5, y: g.y + g.height * 0.5 };
  await feed(at.x, at.y, swipe(20, 6, 4));
  await frames();
  const s1 = await scroll(graph);
  const c = await graphCam();
  check(
    s0.room.x > 150 && s0.room.y > 100 && s1.x - s0.x === 120 && s1.y - s0.y === 80 && !!c && near(c.x, s1.x, 0.5) && near(c.y, s1.y, 0.5),
    `the Graph: a diagonal swipe moves it 1:1 both ways (120 × 80): ${json({ s0, s1, cam: c })}`,
  );
  // A canvas has no rails (Figma, Freeform, tldraw): 5° off straight is 5° off.
  await sleep(120);
  await feed(at.x, at.y, swipe(20, 10, 0.8749));
  await frames();
  const s2 = await scroll(graph);
  const c2 = await graphCam();
  check(s2.x - s1.x === 200 && !!c2 && near(c2.y - c.y, 17.498, 0.01), `the Graph has no rails: a swipe 5° off straight moves it 1:1 both ways (${s2.x - s1.x} × ${(c2?.y ?? 0) - (c?.y ?? 0)})`);
}
// 120 events of a swipe with the pointer over a wire: no view renders (VIEW-8), its hover muted.
{
  const wire = await page.$$eval(`${graph} svg path[data-from]`, (els) => {
    for (const p of els) {
      const r = p.getBoundingClientRect();
      if (r.width > 40) return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
    }
    return null;
  });
  if (wire) await page.mouse.move(wire.x, wire.y);
  await sleep(300);
  const r0 = await renders();
  const s0 = await scroll(graph);
  await feed(wire?.x ?? 0, wire?.y ?? 0, swipe(120, 0.5, -0.25, 8), 8);
  await frames(6);
  await sleep(400);
  const r = since(r0, await renders());
  const s1 = await scroll(graph);
  check(!!wire && s1.x !== s0.x && Object.keys(r).length === 0, `120 events of a swipe over a wire move the Graph and render nothing: ${json({ moved: [s1.x - s0.x, s1.y - s0.y], renders: r })}`);
}
// Tab to a bar out of view: the browser scrolls it into view, and the camera takes that in (VIEW-9).
{
  await expand(page, `${graph} [data-node-id]`).catch(() => {});
  await setScroll(graph, 0, 0);
  await frames();
  const bars = await page.$$(`${graph} [role=slider][tabindex="0"]`);
  const g = await box(graph);
  let target = null;
  for (const b of bars) {
    const r = await b.boundingBox();
    if (r && (r.x > g.x + g.width || r.y > g.y + g.height)) {
      target = b;
      break;
    }
  }
  if (target) await target.evaluate((e) => e.focus());
  await frames();
  const s = await scroll(graph);
  const c = await graphCam();
  check(!!target && (s.x > 0 || s.y > 0) && !!c && near(c.x, s.x, 1) && near(c.y, s.y, 1), `focus on a bar out of view scrolls the Graph, and its camera takes it in: ${json({ s, cam: c && { x: c.x, y: c.y } })}`);
}

// A picture's dot keeps its wheel (VIEW-2, DV-17): a swipe that carries it under the pointer is still
// the camera's, to its end, and changes no file; one that starts on it is the dot's, and moves no view.
{
  const st = await state();
  const pluck = st.view.modules.find((m) => m.key === "keys").nodes.find((n) => n.kind === "call" && n.audio);
  await (await fetch(`${sess.url}/api/graph/effect/add`, { method: "POST", headers: { "x-m2-token": sess.token, "content-type": "application/json" }, body: json({ session: st.session, revision: st.view.revision, call: pluck.id, component: "@m2/effects:delay" }) })).text();
  await until(() => read("tracks/keys.ts").includes("delay("), 15000);
  await show("keys");
  // The second echo's dot: its wheel turns the feedback.
  const dotSel = `${graph} [data-module="keys"] [data-dot="1"]`;
  await until(() => has(dotSel), 15000);
  await sleep(500);
  // Zoomed in at the graph's corner: room to move.
  const n = await box(`${graph} [data-node-id]`);
  for (let i = 0; i < 2; i++) {
    await feed(n.x + 2, n.y + 2, [{ t: 0, dy: -100, ctrl: true }]);
    await frames();
  }
  const g = await box(graph);
  let d = await box(dotSel);
  // The dot at the view's middle, so it can pass under a pointer 60 px left of it.
  await setScroll(graph, (await scroll(graph)).x + d.x - (g.x + g.width / 2), (await scroll(graph)).y + d.y - (g.y + g.height / 2));
  await frames();
  await sleep(200);
  d = await box(dotSel);
  // Towards the side the Graph has room on.
  const s0 = await scroll(graph);
  const way = s0.room.x - s0.x >= 130 ? 1 : -1;
  const at = { x: d.x + d.width / 2 - 60 * way, y: d.y + d.height / 2 };
  const onDot = () => page.evaluate(({ x, y }) => !!document.elementFromPoint(x, y)?.closest("[data-wheel]"), at);
  const bare = !(await onDot());
  const file = read("tracks/keys.ts");
  // 60 px: the dot under the pointer at its end. Then, 60 ms on (a pause, or a glide coming late:
  // more than G, a swipe of its own), another over the dot: the pointer never moved, so it is the
  // camera's too (review Q10).
  await feed(at.x, at.y, swipe(10, 6 * way, 0), 16);
  await frames(2);
  const carried = await onDot();
  await sleep(20);
  await feed(at.x, at.y, swipe(10, 6 * way, 0), 16);
  await frames();
  await sleep(1500);
  const s1 = await scroll(graph);
  check(
    bare && carried && Math.abs(s1.x - s0.x) === 120 && read("tracks/keys.ts") === file,
    `a swipe that carries a Delay's dot under the pointer, and one 60 ms after it over the dot, move the Graph 120 px and change no file: ${json({ bare, carried, moved: s1.x - s0.x })}`,
  );
  d = await box(dotSel);
  const s2 = await scroll(graph);
  await sleep(120);
  await feed(d.x + d.width / 2, d.y + d.height / 2, [{ t: 0, dy: -100 }]);
  const turned = await until(() => read("tracks/keys.ts") !== file, 8000);
  const s3 = await scroll(graph);
  check(turned && s3.x === s2.x && s3.y === s2.y, `one that starts on the dot is the dot's: it writes its value, and the Graph stays (${json({ turned, s2, s3 })})`);
  // The dot's to its end (review Q14): events after its first that no longer land on it (the dot
  // slid away as it turned) are cancelled, never left to the browser to scroll the Graph by.
  await sleep(1500);
  d = await box(dotSel);
  const kept = await page.evaluate(
    ({ on, off }) => {
      const base = Math.max(performance.now(), (window.__stamp ?? 0) + 1000);
      const send = (p, t) => {
        const e = new WheelEvent("wheel", { deltaY: 2, clientX: p.x, clientY: p.y, bubbles: true, cancelable: true });
        Object.defineProperty(e, "timeStamp", { value: base + t });
        return (document.elementFromPoint(p.x, p.y) ?? document.body).dispatchEvent(e);
      };
      const first = send(on, 0);
      const rest = [1, 2, 3, 4].map((i) => send(off, i * 16));
      window.__stamp = base + 64;
      return { first, rest, offBare: !document.elementFromPoint(off.x, off.y)?.closest("[data-wheel]") };
    },
    { on: { x: d.x + d.width / 2, y: d.y + d.height / 2 }, off: { x: d.x + d.width / 2 + 40, y: d.y + d.height / 2 } },
  );
  check(kept.offBare && kept.rest.every((r) => r === false), `a swipe that starts on the dot and slides off it: the rest cancelled, not the browser's (${json(kept)})`);
}

// An agent's pointer at the Delay's knob follows it each frame as the nodes move aside (DV-13,
// 280 ms) when the Pluck before it expands: no view moves then, the nodes ease.
{
  // Fit: 100%, from the top left, the Delay's knobs in view.
  await page.click('button[aria-label="Fit to panel"]');
  await frames();
  const st = await state();
  const keys = st.view.modules.find((m) => m.key === "keys");
  const delay = keys.nodes.find((n) => n.kind === "call" && /delay/i.test(n.component ?? n.label ?? ""));
  const pluck = keys.nodes.find((n) => n.kind === "call" && n.audio);
  const knob = delay?.knobs?.[0]?.id;
  await (await fetch(`${sess.url}/api/point`, { method: "POST", headers: { "x-m2-token": sess.token, "content-type": "application/json" }, body: json({ session: st.session, target: `knob:${knob}`, words: "this one" }) })).text();
  const marked = await until(() => has("[data-testid=pointer-mark]"), 3000);
  await sleep(300);
  // Each frame, after it is drawn: the ring's box against the knob's (or its +N's), from before the
  // click to well after the nodes have moved.
  await page.evaluate((knob) => {
    window.__off = [];
    window.__sampling = true;
    const one = () =>
      setTimeout(() => {
        const mark = document.querySelector("[data-testid=pointer-mark]")?.getBoundingClientRect();
        const target = (document.querySelector(`[data-knob-id="${CSS.escape(knob)}"]`) ?? document.querySelector(`[data-hides~="${CSS.escape(knob)}"]`))?.getBoundingClientRect();
        if (mark && target) window.__off.push({ x: Math.round(mark.left - target.left), y: Math.round(mark.top - target.top), at: target.left });
        if (window.__sampling) requestAnimationFrame(one);
      }, 0);
    requestAnimationFrame(one);
  }, knob);
  await until(() => page.evaluate(() => window.__off.length >= 2), 3000);
  await page.click(`${graph} [data-expand="${pluck.id}"]`);
  await sleep(700);
  const off = await page.evaluate(() => {
    window.__sampling = false;
    return window.__off;
  });
  const moved = off.length ? Math.abs(off.at(-1).at - off[0].at) : 0;
  const worst = Math.max(0, ...off.map((o) => Math.max(Math.abs(o.x), Math.abs(o.y))));
  check(!!knob && marked && moved > 20 && worst <= 1, `an agent's pointer at a knob follows it each frame while the nodes move aside: the knob moved ${Math.round(moved)} px, the ring at most ${worst} px off it over ${off.length} frames (${marked ? "ringed" : "no ring"})`);
  await page.click("[data-testid=pointer-ok]").catch(() => {});
  await page.click(`${graph} [data-expand="${pluck.id}"]`).catch(() => {});
  await sleep(500);
}

// ================================================================ the beat grid: a kit, as a musician makes one

await show("hats");
await key("Meta+t");
await page.waitForSelector("[data-testid=instrument-list]", { timeout: 3000 }).catch(() => null);
await page.keyboard.type("basic kit");
await sleep(150);
await page.keyboard.press("Enter");
await until(async () => ((await state()).view.drum_kits ?? []).length === 1, 15000);
await show("kick-2");
await view("beat");
const beat = "[data-testid=beat-scroller]";
await page.waitForSelector(beat);
await frames();
/** The beat under screen x in the beat grid, from where its step of beat 0 is drawn. */
const beatX = (x) =>
  page.$eval(
    `${beat} [data-testid=beat-row] [data-step="0"]`,
    (e, x) => {
      const r = e.getBoundingClientRect();
      const s1 = e.parentElement.querySelector('[data-step="0.25"]')?.getBoundingClientRect();
      const ppb = s1 ? (s1.left - r.left) / 0.25 : NaN;
      return { beat: (x - r.left) / ppb, ppb };
    },
    x,
  ).catch(() => ({ beat: NaN, ppb: NaN }));
{
  const b = await box(beat);
  const at = { x: b.x + 420, y: b.y + 60 };
  const a = await beatX(at.x);
  await feed(at.x, at.y, [{ t: 0, dy: -100, ctrl: true }]);
  await frames();
  const z = await beatX(at.x);
  check(z.ppb > a.ppb * 1.4 && near(z.beat * z.ppb, a.beat * z.ppb, 1.5), `the beat grid: ⌘-scroll zooms around the pointer, beat ${a.beat.toFixed(3)} staying under it (${z.beat.toFixed(3)}, ${a.ppb.toFixed(1)} → ${z.ppb.toFixed(1)} px a beat)`);
  await pinch(at.x, at.y, [0.9, 0.8]);
  await frames();
  const p = await beatX(at.x);
  check(near(p.ppb, z.ppb * 0.8, 0.5) && near(p.beat * p.ppb, a.beat * p.ppb, 1.5), `and a pinch out (WebKit's gesture events), around it too: beat ${p.beat.toFixed(3)}, ${p.ppb.toFixed(1)} px a beat`);
}

// ================================================================ the big song

const big = path.join(ROOT, "big");
execFileSync(process.execPath, [path.join(path.dirname(fileURLToPath(import.meta.url)), "big-song.mjs"), big]);
// Bass 1's first clip: 64 beats of 256 notes over four octaves, for Notes (its other clips after it).
{
  const file = path.join(big, "data/bass-1/notes.json");
  const lane = JSON.parse(fs.readFileSync(file, "utf8"));
  const notes = Array.from({ length: 256 }, (_, i) => ({ pitch: 36 + ((i * 7) % 48), start: i / 4, length: 0.25, velocity: 0.8 }));
  lane.clips = [{ ...lane.clips[0], start: 0, length: 64, notes }, ...lane.clips.slice(16)];
  fs.writeFileSync(file, JSON.stringify(lane));
}
// First-song zoomed in to 54 px a beat, its Graph showing, then a twin of it and the big song each
// opened straight after the last, with no close in between (Open Recent, Save As): each song's views
// get cameras of their own (review Q1).
await key("Meta+Equal");
await key("Meta+Equal");
await until(async () => (await cam("arrangement"))?.zoom === 54, 3000);
const tabWas = await page.$eval('[data-view-tab][aria-selected="true"]', (e) => e.dataset.viewTab).catch(() => null);
await view("graph");
await page.waitForSelector(`${graph} [data-node-id]`);
// A twin of first-song (a fresh copy: its own view.json), whose Graph shows the same track at once.
{
  const twin = path.join(ROOT, "twin");
  fs.cpSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "../examples/first-song"), twin, { recursive: true });
  const opened = (await api("/open", { method: "POST", body: json({ path: twin }) })).status === 200;
  await until(async () => (await cam("arrangement"))?.zoom !== 54 && (await has(`${graph} [data-node-id]`)), 30000);
  await frames();
  const zoom = (await cam("arrangement"))?.zoom;
  const n = await box(`${graph} [data-node-id]`);
  const before = await page.$eval("[data-zoom]", (e) => e.dataset.zoom);
  await feed(n.x + 4, n.y + 4, [{ t: 0, dy: -100, ctrl: true }]);
  await frames();
  const after = await page.$eval("[data-zoom]", (e) => e.dataset.zoom);
  check(
    opened && zoom === 24 && before === "1.00" && after === "1.50",
    `first-song's twin opened over it, its Graph showing: its own zoom (24 px a beat: ${zoom}), its Graph at Fit and zooming by ⌘-scroll (${before} → ${after})`,
  );
  await key("Meta+Equal");
  await key("Meta+Equal");
  await until(async () => (await cam("arrangement"))?.zoom === 54, 3000);
}
check((await api("/open", { method: "POST", body: json({ path: big }) })).status === 200, "the big song opens straight after the twin: 24 tracks of 64 clips");
const tl = "[data-testid=timeline]";
await page.waitForSelector(`${tl} [data-track="keys-6"]`, { timeout: 60000 });
await until(async () => (await page.$$(`${tl} [data-clip]`)).length > 50, 60000);
await frames();
/** Each clip box drawn, against where its start and length put it at the camera's zoom. */
const clipsOff = () =>
  page.evaluate(() => {
    const zoom = window.m2canvas?.camera?.now("arrangement")?.zoom ?? NaN;
    let worst = 0, n = 0;
    for (const e of document.querySelectorAll("[data-testid=timeline] [data-clip]")) {
      const lane = e.parentElement.getBoundingClientRect(), b = e.getBoundingClientRect();
      const start = Number(e.dataset.start), length = Number(e.dataset.length);
      worst = Math.max(worst, Math.abs(b.left - lane.left - (start * zoom + 1)), Math.abs(b.width - Math.max(length * zoom - 2, 4)));
      n++;
    }
    return { zoom, worst: Math.round(worst * 10) / 10, n };
  });
{
  const o = await clipsOff();
  check(o.zoom === 24 && o.n > 0 && o.worst <= 1, `it opens at its own zoom (24 px a beat, no view.json), not the twin's 54, each clip box where its beats put it: ${json(o)}`);
  await key("Meta+Equal");
  await until(async () => (await cam("arrangement"))?.zoom === 36, 3000);
  await frames();
  const z = await clipsOff();
  check(z.zoom === 36 && z.worst <= 1, `⌘= zooms it, and every clip box follows: ${json(z)}`);
  await key("Meta+Minus");
  await until(async () => (await cam("arrangement"))?.zoom === 24, 3000);
  await page.waitForSelector(`${graph} [data-node-id]`);
  await until(async () => (await page.$eval("[data-zoom]", (e) => e.dataset.zoom).catch(() => null)) === "1.00", 3000);
  const n = await box(`${graph} [data-node-id]`);
  const before = await page.$eval("[data-zoom]", (e) => e.dataset.zoom);
  await feed(n.x + 4, n.y + 4, [{ t: 0, dy: -100, ctrl: true }]);
  await frames();
  const after = await page.$eval("[data-zoom]", (e) => e.dataset.zoom);
  check(before === "1.00" && after === "1.50", `the new song's Graph zooms by ⌘-scroll (${before} → ${after})`);
  // Off the Graph again, as before the switch (first-song's beat grid has no tab in the big song:
  // Notes, with no clip, draws nothing either).
  const back = (await has(`[data-view-tab="${tabWas}"]`)) ? tabWas : "notes";
  if (back && back !== "graph") await view(back);
}
const lanes = async () => {
  const t = await box(tl);
  return { x: t.x + 700, y: t.y + 200 };
};
const arr = () => cam("arrangement");

// ---------------------------------------------------------------- swipes: 1:1, rails, ⇧, edges
await setScroll(tl, 600, 300);
await frames();
{
  const at = await lanes();
  const s0 = await scroll(tl);
  await feed(at.x, at.y, swipe(20, 6, 4));
  await frames();
  const s1 = await scroll(tl);
  const c = await arr();
  check(s1.x - s0.x === 120 && s1.y - s0.y === 80 && !!c && near(c.x, s1.x, 0.5) && near(c.y, s1.y, 0.5), `the arrangement: a diagonal swipe moves it 1:1 both ways (120 × 80): ${json({ s0, s1, cam: c && { x: c.x, y: c.y } })}`);
  await sleep(120);
  // 5° off: within 25° at its start, and its last 100 px never more than 5° off its rail.
  await feed(at.x, at.y, swipe(20, 10, 0.8749));
  await frames();
  const s2 = await scroll(tl);
  check(s2.x - s1.x === 200 && s2.y === s1.y, `a swipe 5° off straight keeps to its rail: 200 across, none down (${s2.x - s1.x} × ${s2.y - s1.y})`);
  await sleep(120);
  // A hand's start is rough (the owner's first 8 to 20 px often go 15 to 25° off): four events of
  // 4.26 px at 20° (17 px), then straight back across. Its first 16 px decide its rail.
  await feed(at.x, at.y, [...swipe(4, -4, -1.4559), ...swipe(16, -10, 0).map((w) => ({ ...w, t: w.t + 64 }))]);
  await frames();
  const s2b = await scroll(tl);
  check(s2b.x - s2.x === -176 && s2b.y === s2.y, `a swipe whose first 16 px go 20° off still gets its rail: 176 across, none up (${s2b.x - s2.x} × ${s2b.y - s2.y})`);
  await sleep(120);
  await feed(at.x, at.y, swipe(20, -5, -5));
  await frames();
  const s3 = await scroll(tl);
  check(s3.x - s2b.x === -100 && s3.y - s2b.y === -100, `a 45° one moves both ways (${s3.x - s2b.x} × ${s3.y - s2b.y})`);
  await sleep(120);
  await feed(at.x, at.y, swipe(20, 10, 5.7735));
  await frames();
  const s3b = await scroll(tl);
  check(s3b.x - s3.x === 200 && near(s3b.y - s3.y, 115.47, 0.5), `a 30° one never rails: 1:1 both ways (${s3b.x - s3.x} × ${s3b.y - s3.y})`);
  await sleep(120);
  // Straight across, then turning to 45°: on its rail until its last 100 px go more than 25° off it
  // (the 7th turned event: 28.8°; the 6th is 24.5°), then free both ways from there; the 42 px held
  // back aren't moved.
  await feed(at.x, at.y, [...swipe(10, 10, 0), ...swipe(20, 7, 7).map((w) => ({ ...w, t: w.t + 160 }))]);
  await frames();
  const s3c = await scroll(tl);
  check(s3c.x - s3b.x === 240 && s3c.y - s3b.y === 98, `one that starts straight and turns 45° breaks free once its last 100 px turn 25° off its rail, both ways from there: 240 × 98 (${s3c.x - s3b.x} × ${s3c.y - s3b.y})`);
  await sleep(120);
  // 1500 px across, 3° off, with ±1 px of jitter down each event (a hand's raw deltas), then a
  // glide of the same angle: on its rail to the end (review Q3).
  const jitter = (n, d, from = 0) => Array.from({ length: n }, (_, i) => ({ t: from + i * 16, dx: d(i), dy: d(i) * Math.tan((3 * Math.PI) / 180) + (i % 2 ? 1 : -1) }));
  await feed(at.x, at.y, [...jitter(150, () => 10), ...jitter(40, (i) => 10 * 0.93 ** i, 150 * 16)]);
  await frames();
  const s3d = await scroll(tl);
  const glided = 10 * (1 - 0.93 ** 40) / (1 - 0.93);
  check(near(s3d.x - s3c.x, 1500 + glided, 1) && s3d.y === s3c.y, `a 1500 px swipe 3° off straight, ±1 px of jitter off it, and its glide keep to the rail: ${(s3d.x - s3c.x).toFixed(0)} across (${(1500 + glided).toFixed(0)}), ${s3d.y - s3c.y} down`);
  await sleep(120);
  await feed(at.x, at.y, swipe(10, 0, 5, 16, { shift: true }));
  await frames();
  const s4 = await scroll(tl);
  check(s4.x === s3d.x && s4.y - s3d.y === 50, `⇧ + wheel isn't turned by the camera: the Mac and WebKitGTK turn it already (${s4.x - s3d.x} × ${s4.y - s3d.y})`);
  await sleep(120);
  await feed(at.x, at.y, swipe(30, -100, -100));
  await frames();
  const s5 = await scroll(tl);
  const c5 = await arr();
  check(s5.x === 0 && s5.y === 0 && !!c5 && c5.x === 0 && c5.y === 0, `at the edges it stops, and so does the camera: ${json({ s5, cam: c5 && { x: c5.x, y: c5.y } })}`);
}
// The owner's own swipes (VIEW-3): their wheel events from the swipe probe of 2026-11-14 on their
// Mac (`fixtures/swipes`), over the timeline and the Graph in both rounds, all one hand. Each swipe
// (split at G) is fed at its own times over the arrangement, its tracks at their tallest, from where
// it has room for all of it. Only a flick down further than the tracks go meets their foot, which
// changes nothing across. One under 30 px tells no angle. A straight one (under 12° off an axis, all
// told) moves at most 2 px off it; a diagonal one (over 20°) keeps at least 90% of its move across.
{
  const owner = JSON.parse(fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures/swipes/owner-2026-11-14.json"), "utf8")).steps;
  const G = await page.evaluate(() => window.m2canvas?.camera?.numbers.SWIPE_GAP);
  await key("Escape");
  const tracksWere = (await scroll(tl)).room.y;
  await key("Meta+ArrowDown");
  await key("Meta+ArrowDown");
  await until(async () => (await scroll(tl)).room.y > tracksWere * 2, 3000);
  await frames();
  const at = await lanes();
  const room = (await scroll(tl)).room;
  const seen = { straight: { n: 0, asked: 0, moved: 0, wrong: [] }, diagonal: { n: 0, asked: 0, moved: 0, wrong: [] } };
  const cramped = [];
  for (const [step, events] of Object.entries(owner)) {
    const swipes = [];
    for (const [t, dx, dy] of events) {
      const last = swipes.at(-1)?.at(-1);
      if (!last || t - last.t >= G) swipes.push([]);
      swipes.at(-1).push({ t, x: dx, y: dy });
    }
    for (const sw of swipes) {
      const sum = (f) => sw.reduce((a, e) => a + f(e), 0);
      const all = { x: sum((e) => e.x), y: sum((e) => e.y) };
      if (sum((e) => Math.hypot(e.x, e.y)) < 30) continue;
      const deg = (Math.atan2(Math.abs(all.y), Math.abs(all.x)) * 180) / Math.PI;
      const off = Math.min(deg, 90 - deg);
      const kind = off < 12 ? "straight" : off > 20 ? "diagonal" : null;
      if (!kind) continue;
      const across = Math.abs(all.x) >= Math.abs(all.y) ? "y" : "x";
      // Each way: from what it goes back to what it goes on, in the middle of the room; where that
      // is more than the room, from the end it moves away from.
      const fits = {};
      const start = (way) => {
        const on = sum((e) => Math.max(0, e[way])), back = -sum((e) => Math.min(0, e[way]));
        fits[way] = on + back <= room[way];
        return fits[way] ? Math.round(back + (room[way] - on - back) / 2) : all[way] < 0 ? room[way] : 0;
      };
      await setScroll(tl, start("x"), start("y"));
      await frames();
      const c0 = await arr();
      await feed(at.x, at.y, sw.map((e) => ({ t: e.t - sw[0].t, dx: e.x, dy: e.y })));
      await frames();
      const c1 = await arr();
      const asked = Math.abs(all[across]), moved = Math.abs(c1[across] - c0[across]);
      const s = seen[kind];
      s.n++;
      s.asked += asked;
      s.moved += moved;
      const said = { step, events: sw.length, off: Math.round(off * 10) / 10, asked, moved: Math.round(moved * 10) / 10 };
      if (!fits[across]) cramped.push(said);
      if (kind === "straight" ? moved > 2 : moved < asked * 0.9) s.wrong.push(said);
    }
  }
  const { straight: st, diagonal: dg } = seen;
  check(
    st.n > 0 && !cramped.length && !st.wrong.length,
    `the owner's ${st.n} straight swipes each move at most 2 px off their axis: ${Math.round(st.moved)} of the ${st.asked} px they went off it, all told (room ${json(room)})${st.wrong.length ? `; ${st.wrong.length} more: ${json(st.wrong)}` : ""}${cramped.length ? `; no room across: ${json(cramped)}` : ""}`,
  );
  check(
    dg.n > 0 && !cramped.length && !dg.wrong.length,
    `the owner's ${dg.n} diagonal swipes each keep at least 90% of their move across: ${Math.round(dg.moved)} of ${dg.asked} px, all told (${Math.round((dg.moved / dg.asked) * 1000) / 10}%)${dg.wrong.length ? `; ${dg.wrong.length} less: ${json(dg.wrong)}` : ""}`,
  );
  await key("Escape");
  await key("Meta+ArrowUp");
  await key("Meta+ArrowUp");
  await until(async () => (await scroll(tl)).room.y === tracksWere, 3000);
  await frames();
}
// A swipe WebKit took over (VIEW-2): the page answered its first event too late, so WebKit scrolled
// by it anyway and marks every later one not cancelable; it scrolls the box by all ten itself. The
// camera doesn't move by any of them, the first included, and takes the box's move in (review Q15).
{
  await setScroll(tl, 600, 0);
  await frames();
  await sleep(120);
  const at = await lanes();
  const s0 = await scroll(tl);
  await feed(at.x, at.y, [{ t: 0, dx: 10 }, ...swipe(9, 10, 0, 16, { cancelable: false }).map((w) => ({ ...w, t: w.t + 16 }))]);
  // What WebKit does with all ten: it scrolls the box itself.
  await page.$eval(tl, (e) => void (e.scrollLeft += 100));
  await frames();
  await sleep(100);
  const s1 = await scroll(tl);
  const c = await arr();
  check(s1.x - s0.x === 100 && !!c && near(c.x, s1.x, 0.5), `a swipe WebKit took over moves 100, not 110: the camera drops the first event's move and takes the box's in (${s1.x - s0.x}, camera at ${c?.x})`);
}

// The content getting smaller under the camera (every track a step shorter, the view at the foot):
// the box stops at its new edge, and the camera with it, moved by no one: `clamp` (review Q12).
{
  await key("Escape");
  await setScroll(tl, 600, 100000);
  await frames();
  await sleep(150);
  const s0 = await scroll(tl);
  await key("Meta+ArrowUp");
  const shrank = await until(async () => (await scroll(tl)).room.y < s0.room.y, 3000);
  await frames(3);
  const s1 = await scroll(tl);
  const c = await arr();
  check(
    shrank && s1.y === s1.room.y && s1.y < s0.y && !!c && near(c.y, s1.y, 0.5) && c.by === "clamp",
    `tracks a step shorter at the foot: the camera stops where the box does, moved by its edge (${json({ was: s0.y, box: s1.y, cam: c && { y: c.y, by: c.by } })})`,
  );
  await key("Meta+ArrowDown");
  await until(async () => (await scroll(tl)).room.y >= s0.room.y, 3000);
  await frames();
}

// A swipe's first event decides it (VIEW-2): ⌘ held later, after a key dropped it, zooms nothing (review Q5).
{
  await setScroll(tl, 600, 0);
  await frames();
  await sleep(120);
  const at = await lanes();
  const z0 = (await arr())?.zoom;
  const s0 = await scroll(tl);
  await page.evaluate(({ x, y }) => {
    const base = Math.max(performance.now(), (window.__stamp ?? 0) + 1000);
    const send = (t, meta) => {
      const e = new WheelEvent("wheel", { deltaY: 6, clientX: x, clientY: y, metaKey: meta, bubbles: true, cancelable: true });
      Object.defineProperty(e, "timeStamp", { value: base + t });
      (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(e);
    };
    for (let i = 0; i < 5; i++) send(i * 16, false);
    // ⌘Z, say: a key, as the glide still comes, now with ⌘ held.
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    for (let i = 5; i < 25; i++) send(i * 16, true);
    window.__stamp = base + 24 * 16;
  }, at);
  await frames();
  await sleep(100);
  const s1 = await scroll(tl);
  check((await arr())?.zoom === z0 && s1.y - s0.y === 30, `⌘ held in a swipe's glide, after a key dropped it, zooms nothing and moves nothing more: ${json({ zoom: [z0, (await arr())?.zoom], down: s1.y - s0.y })}`);
}

// A right-click's press holds nothing (review Q6): its menu may eat the release, and a swipe after still moves the view.
{
  const at = await lanes();
  await page.evaluate(() => {
    document.body.dispatchEvent(new PointerEvent("pointerdown", { button: 2, buttons: 2, bubbles: true }));
    document.body.dispatchEvent(new MouseEvent("contextmenu", { button: 2, bubbles: true, cancelable: true }));
    document.body.dispatchEvent(new PointerEvent("pointerdown", { button: 0, buttons: 1, ctrlKey: true, bubbles: true }));
    document.body.dispatchEvent(new MouseEvent("contextmenu", { button: 0, ctrlKey: true, bubbles: true, cancelable: true }));
  });
  const s0 = await scroll(tl);
  await feed(at.x, at.y, swipe(10, 10, 0));
  await frames();
  const s1 = await scroll(tl);
  check(s1.x - s0.x === 100, `after a right-click and a ⌃-click whose release never came (a context menu took it), a swipe still moves the view (${s1.x - s0.x})`);
}

// ---------------------------------------------------------------- moving renders nothing; the rows at a step
{
  const c = await arr();
  const step = c ? (c.view.w - c.margins.left - c.margins.right) / 2 : 600;
  // A jump far from where it was: a new window, around where it lands.
  await setScroll(tl, 0, 0);
  await frames();
  await setScroll(tl, Math.round(step * 2 + 20), 0);
  await frames();
  await sleep(300);
  const at = await lanes();
  await page.mouse.move(at.x, at.y);
  await sleep(300);
  const r0 = await renders();
  const s0 = await scroll(tl);
  await feed(at.x, at.y, swipe(120, 1.5, 0, 8), 8);
  await frames(6);
  await sleep(400);
  const s1 = await scroll(tl);
  const r = since(r0, await renders());
  check(s1.x - s0.x === 180 && Object.keys(r).length === 0, `120 events of a swipe within a step move the arrangement 180 px and render nothing: ${json(r)}`);
  const rows = (await page.$$(`${tl} [data-track]`)).length;
  const r1 = await renders();
  await sleep(120);
  // A step and 60 px more: across one step, never two.
  await feed(at.x, at.y, swipe(120, (step + 60) / 120, 0, 8), 8);
  await frames(6);
  await sleep(400);
  const s2 = await scroll(tl);
  const crossed = Math.floor(s2.x / step) - Math.floor(s1.x / step);
  const q = since(r1, await renders());
  check(crossed === 1 && (q["arrangement.rows"] ?? 0) > 0 && (q["arrangement.rows"] ?? 0) <= rows && !q.arrangement, `120 more that cross one step render each row once at most (${rows} rows), and the arrangement not: ${json(q)}`);
}

// ---------------------------------------------------------------- only what is in view
const clipsShown = () =>
  page.$$eval(`${tl} [data-clip]`, (els) =>
    els.map((e) => ({ track: e.closest("[data-track]")?.dataset.track, start: Number(e.dataset.start), length: Number(e.dataset.length) })),
  );
{
  // At the foot: the tracks at the top are more than a view above.
  await setScroll(tl, 3000, 10000);
  await frames();
  await sleep(500);
  const c = await arr();
  const shown = await clipsShown();
  const t = await box(tl);
  const tracks = await page.$$eval(`${tl} [data-track]`, (els) => els.map((e) => ({ track: e.dataset.track, top: e.getBoundingClientRect().top, bottom: e.getBoundingClientRect().bottom })));
  const clear = c ? c.view.w - c.margins.left : 0;
  // The window across: what shows, a view each side, and up to a step more.
  const from = c ? (c.x - clear) / c.zoom : 0, to = c ? (c.x + clear * 2.5) / c.zoom : 0;
  const nearTracks = tracks.filter((x) => x.bottom >= t.y - t.height && x.top <= t.y + t.height * 2).map((x) => x.track);
  const outside = shown.filter((s) => s.start + s.length < from - 1e-6 || s.start > to + 1e-6);
  const far = shown.filter((s) => !nearTracks.includes(s.track));
  const all = 24 * 64;
  check(
    !!c && shown.length > 0 && shown.length < all && outside.length === 0 && far.length === 0 && nearTracks.length < 25,
    `scrolled to 3000 px and the foot: ${shown.length} of ${all} clips drawn, none outside beats ${from.toFixed(1)} to ${to.toFixed(1)} (${outside.length}), none in a track more than a view above (${far.length}; ${nearTracks.length} of ${tracks.length} rows near)`,
  );
  const rowsKept = await page.$$eval(`${tl} [data-track="kick-1"] [data-lane]`, (els) => els.map((e) => ({ h: e.getBoundingClientRect().height, clips: e.querySelectorAll("[data-clip]").length })));
  check(rowsKept.length > 0 && rowsKept.every((r) => r.h > 20 && r.clips === 0), `a track far above keeps its row box, drawn without its clips: ${json(rowsKept)}`);
}

// ---------------------------------------------------------------- down the tracks: rows near and in view
// A swipe down the 24 tracks and back up, across frames (review Q8): each row draws its clips as it
// comes near (between frames) and lets them go as it leaves, at most twice a row each way; the
// arrangement itself never renders; every row in view at the end has its clips. Its slowest frame
// here is only told: the judge of speed is the owner's Mac.
{
  await setScroll(tl, 600, 0);
  await frames();
  await sleep(400);
  const at = await lanes();
  const rows = (await page.$$(`${tl} [data-track]`)).length;
  const r0 = await renders();
  await page.evaluate(() => {
    window.__gaps = [];
    let last = performance.now();
    const tick = (t) => {
      window.__gaps.push(t - last);
      last = t;
      window.__raf = requestAnimationFrame(tick);
    };
    window.__raf = requestAnimationFrame(tick);
  });
  await feed(at.x, at.y, swipe(60, 0, 40, 8), 8);
  await sleep(120);
  await feed(at.x, at.y, swipe(60, 0, -40, 8), 8);
  await frames(6);
  await sleep(500);
  const gaps = await page.evaluate(() => {
    cancelAnimationFrame(window.__raf);
    return window.__gaps;
  });
  const q = since(r0, await renders());
  const t = await box(tl);
  const c = await arr();
  const blank = await page.$$eval(
    `${tl} [data-track]`,
    (els, { top, bottom }) =>
      els
        .filter((e) => {
          const r = e.getBoundingClientRect();
          return r.bottom > top && r.top < bottom && e.querySelector("[data-lane]");
        })
        .filter((e) => !e.querySelector("[data-clip]"))
        .map((e) => e.dataset.track),
    { top: t.y + (c?.margins.top ?? 0), bottom: t.y + t.height - (c?.margins.bottom ?? 0) },
  );
  const slowest = Math.max(...gaps.slice(1));
  check(
    (q["arrangement.rows"] ?? 0) <= 4 * rows && !q.arrangement && blank.length === 0,
    `a swipe down the tracks and back: rows drawn as they come near and go, ${q["arrangement.rows"] ?? 0} renders (at most ${4 * rows}), the arrangement ${q.arrangement ?? 0}, every row in view drawn (${json(blank)}); its slowest frame ${slowest.toFixed(1)} ms`,
  );
}

// ---------------------------------------------------------------- zoom: once a frame, around the pointer, no mini drawing
{
  await setScroll(tl, 1200, 0);
  await frames();
  await sleep(300);
  const at = await lanes();
  const r0 = await renders();
  await feed(at.x, at.y, Array.from({ length: 30 }, (_, i) => ({ t: i * 4, dy: -3, meta: true })));
  await frames(4);
  const r = since(r0, await renders());
  check((r.arrangement ?? 0) >= 1 && (r.arrangement ?? 0) <= 1, `30 ⌘-scroll events in one go render the arrangement once (${r.arrangement ?? 0})`);
  const r1 = await renders();
  await zoomFrames(at.x, at.y, 15, 3);
  const q = since(r1, await renders());
  check((q.arrangement ?? 0) >= 1 && (q.arrangement ?? 0) <= 18, `30 more, two in each of 15 frames, render it at most once a frame (18 frames): ${q.arrangement ?? 0} renders`);
  check((r["arrangement.drawings"] === undefined || r["arrangement.drawings"] === 0) && (await page.evaluate(() => "arrangement.drawings" in (window.m2canvas?.frames?.().total ?? {}))), `a zoom in renders no clip's mini drawing: ${r["arrangement.drawings"] ?? 0} (each counted)`);
}
// A clip's mini drawing (review Q9): each note at its own opacity, never a group's (WebKit paints a
// group with opacity through a layer of its own); its 2 px marks shown only in a clip whose shortest
// note is under 2 px at the zoom. At this zoom and six steps out.
{
  const drawing = () =>
    page.evaluate(() => {
      const zoom = window.m2canvas?.camera?.now("arrangement")?.zoom ?? 0;
      const clips = [...document.querySelectorAll("[data-testid=timeline] [data-clip]")].filter((e) => e.querySelector("svg rect"));
      const wrong = clips.filter((e) => {
        const shortest = Math.min(...[...e.querySelectorAll("svg rect")].map((r) => Number(r.getAttribute("width"))));
        const line = e.querySelector("svg line[data-note-mark]");
        const shown = !!line && getComputedStyle(line).display !== "none";
        return shown !== shortest * zoom < 2;
      });
      const marked = clips.filter((e) => {
        const line = e.querySelector("svg line[data-note-mark]");
        return !!line && getComputedStyle(line).display !== "none";
      }).length;
      return { zoom, clips: clips.length, layers: document.querySelectorAll("[data-testid=timeline] [data-clip] svg [opacity]").length, wrong: wrong.length, marked };
    });
  const a = await drawing();
  for (let i = 0; i < 6; i++) await key("Meta+Minus");
  await frames(3);
  const b = await drawing();
  for (let i = 0; i < 6; i++) await key("Meta+Equal");
  await frames(3);
  check(
    a.clips > 0 && b.clips > 0 && a.layers + b.layers === 0 && a.wrong + b.wrong === 0 && b.marked > 0,
    `a clip's notes: no group opacity, the 2 px marks only where the shortest note is under 2 px (${json({ a, b })})`,
  );
}
// A zoom renders no clip's box, only those entering the window drawn; the arrangement still once a frame.
{
  await setScroll(tl, 0, 0);
  await frames();
  await setScroll(tl, 2400, 0);
  await frames();
  await sleep(300);
  const at = await lanes();
  for (const [what, dy] of [["in", -6], ["out", 6]]) {
    await page.$$eval(`${tl} [data-clip]`, (els) => void (window.__clips = new Set(els)));
    const r0 = await renders();
    await zoomFrames(at.x, at.y, 15, dy);
    // The window's new render is a transition: done once the page is idle.
    await sleep(500);
    const entered = await page.$$eval(`${tl} [data-clip]`, (els) => els.filter((e) => !window.__clips.has(e)).length);
    const r = since(r0, await renders());
    const counted = await page.evaluate(() => "arrangement.clips.new" in (window.m2canvas?.frames?.().total ?? {}));
    // A clip entering the window is drawn for the first time (`.new`); a clip box already drawn never again.
    check(
      counted && !r["arrangement.clips"] && (r["arrangement.clips.new"] ?? 0) === entered && (r.arrangement ?? 0) >= 1 && (r.arrangement ?? 0) <= 18,
      `30 ⌘-scroll events zooming ${what} render no clip box drawn before (${r["arrangement.clips"] ?? 0}), only the ${entered} entering the window (each counted), and the arrangement ${r.arrangement ?? 0} times in 18 frames: ${json(r)}`,
    );
  }
}
/** The beat under screen x, from where Kick 1's clip at beat 0… is drawn: any clip with its start. */
const beatAt = async (x) =>
  page.$eval(
    tl,
    (e, x) => {
      const c = [...e.querySelectorAll('[data-track="kick-1"] [data-clip]')].map((d) => ({ s: Number(d.dataset.start), l: Number(d.dataset.length), r: d.getBoundingClientRect() })).find((d) => d.r.width > 0);
      if (!c) return { beat: NaN, ppb: NaN };
      const ppb = (c.r.width + 2) / c.l;
      return { beat: c.s + (x - (c.r.left - 1)) / ppb, ppb };
    },
    x,
  );
{
  const at = await lanes();
  const a = await beatAt(at.x);
  await pinch(at.x, at.y, [1.2, 1.5]);
  await frames();
  const b = await beatAt(at.x);
  check(near(b.ppb, a.ppb * 1.5, 0.05) && near(b.beat * b.ppb, a.beat * b.ppb, 1.5), `a pinch over the arrangement (WebKit's gesture events) zooms around the pointer: beat ${a.beat.toFixed(3)} stays (${b.beat.toFixed(3)}, ${a.ppb.toFixed(2)} → ${b.ppb.toFixed(2)})`);
  await pinch(at.x, at.y, [1 / 1.5]);
  await frames();
}

// ---------------------------------------------------------------- a click's scroll, and agents' seen
{
  await setScroll(tl, 800, 0);
  await frames();
  await page.click(`${tl} [data-module-tab="keys-6"]`);
  await frames();
  const s = await scroll(tl);
  const c = await arr();
  check(s.y > 0 && !!c && near(c.y, s.y, 1) && near(c.x, s.x, 1), `a click on a track's name out of view scrolls to it, and the camera takes it in: ${json({ s, cam: c && { x: c.x, y: c.y } })}`);
  const at = await lanes();
  await sleep(120);
  await feed(at.x, at.y, swipe(10, 13, 0));
  await frames();
  await sleep(600);
  const now = await arr();
  const said = (await (await fetch(`${sess.url}/api/focus`, { headers: { "x-m2-token": sess.token } })).json()).seen;
  const want = now && [now.x / now.zoom, (now.x + now.view.w - now.margins.left) / now.zoom].map((b) => Math.round(b * 1000) / 1000);
  check(!!want && json(said) === json(want), `agents' seen follows the camera: ${json(said)} (${json(want)})`);
}

// ---------------------------------------------------------------- an agent's pointer moves with the view, rendering nothing
{
  await setScroll(tl, 0, 0);
  await frames();
  await setScroll(tl, 1500, 0);
  await frames();
  await sleep(300);
  const t = await box(tl);
  const id = await page.$$eval(`${tl} [data-track="kick-1"] [data-clip]`, (els, x) => els.find((e) => { const r = e.getBoundingClientRect(); return r.left > x + 300 && r.right < x + 700; })?.dataset.clip ?? null, t.x + 240);
  const { session } = await state();
  const said = await (await fetch(`${sess.url}/api/point`, { method: "POST", headers: { "x-m2-token": sess.token, "content-type": "application/json" }, body: json({ session, target: `clip:kick-1/hits:${id}`, words: "this one" }) })).json().catch(() => null);
  await until(() => has("[data-testid=pointer-mark]"), 3000);
  await sleep(300);
  const at = await lanes();
  const r0 = await renders();
  // 8 ms apart in time too: the view moves over many frames.
  await feed(at.x, at.y, swipe(120, 1, 0, 8), 8);
  await frames(6);
  await sleep(400);
  const r = since(r0, await renders());
  const mark = await box("[data-testid=pointer-mark]");
  const clip = await box(`${tl} [data-clip="${id}"]`);
  const counted = await page.evaluate(() => "pointer.note" in (window.m2canvas?.frames?.().total ?? {}));
  check(
    !!id && !!mark && !!clip && counted && !r["pointer.note"] && near(mark.x, clip.x, 1) && near(mark.width, clip.width, 1),
    `with an agent's pointer at a clip, 120 events of a swipe render its note 0 times (${counted ? (r["pointer.note"] ?? 0) : "uncounted"}), and its mark still sits on the clip: ${json({ said: !!said, mark, clip })}`,
  );
  await page.click("[data-testid=pointer-ok]").catch(() => {});
}

// ---------------------------------------------------------------- Follow pauses on any move not the app's
const status = async () => (await api("/status")).body;
const post = (p, body) => api(p, { method: "POST", body: json(body) });
{
  for (let i = 0; i < 6; i++) await page.click("[data-testid=zoom-in]");
  await setScroll(tl, 0, 0);
  await post("/seek", { beat: 8 });
  await key("Space");
  await until(async () => (await status()).playing);
  // A seek to `beat`, and Follow's page to it: the playhead past it, in the view's left half.
  // (Any page is no proof: playing again pages to where the playhead stopped first.)
  const pages = async (beat) => {
    await post("/seek", { beat });
    return until(async () => {
      const c = await arr();
      const b = (await status()).beat;
      const at = b * c.zoom - c.x;
      return b >= beat && at > 0 && at < (c.view.w - c.margins.left) / 2;
    }, 2500);
  };
  check(await pages(40), `playing at 192 px a beat, Follow pages to the playhead (${(await scroll(tl)).x})`);
  const at = await lanes();
  // Each moves the view well left of the playhead: Follow pauses until play starts again or the
  // playhead is back in view (Q11), so a seek further on must leave the view where it was put.
  const tries = [
    ["a swipe", () => feed(at.x, at.y, swipe(20, -100, 0))],
    [
      "a key's scroll",
      async () => {
        await page.$eval(tl, (e) => e.focus({ preventScroll: true }));
        const x0 = (await scroll(tl)).x;
        // ← scrolls 40 px a press, smoothly: until it has gone 1500 px.
        for (let i = 0; i < 80 && x0 - (await scroll(tl)).x < 1500; i++) await page.keyboard.press("ArrowLeft");
        await until(async () => x0 - (await scroll(tl)).x >= 1500, 2000);
      },
    ],
    ["a write to scrollLeft", () => page.$eval(tl, (e) => void (e.scrollLeft -= 2000))],
  ];
  let beatTo = 60;
  for (const [what, act] of tries) {
    // Play again: Follow follows again.
    await key("Space");
    await until(async () => !(await status()).playing);
    await key("Space");
    await until(async () => (await status()).playing);
    await pages((beatTo += 20));
    await act();
    await frames();
    await sleep(300);
    const kept = (await scroll(tl)).x;
    await post("/seek", { beat: (beatTo += 40) });
    await sleep(900);
    check(Math.abs((await scroll(tl)).x - kept) < 2, `${what} while playing pauses Follow: the view stays at ${kept} (${(await scroll(tl)).x})`);
  }
  // A swipe straight down while playing is no move across: Follow still pages (review Q2).
  await key("Space");
  await until(async () => !(await status()).playing);
  await key("Space");
  await until(async () => (await status()).playing);
  await pages((beatTo += 20));
  {
    await sleep(120);
    await feed(at.x, at.y, swipe(30, 0, 10));
    await frames();
    await sleep(100);
    const c = await arr();
    const kept = (await scroll(tl)).x;
    const w = c.view.w - c.margins.left;
    await post("/seek", { beat: (c.x + w * 0.9) / c.zoom });
    check(await until(async () => Math.abs((await scroll(tl)).x - kept) > 300, 2500), `a swipe straight down (300 px) while playing doesn't pause Follow: past 85% it pages (${kept} → ${(await scroll(tl)).x})`);
  }
  // Follow coming back while the hand swipes doesn't take the swipe from it (review Q4): scrolled
  // far left of the playhead, a swipe back right that brings it into view moves 1:1 to its end.
  await key("Space");
  await until(async () => !(await status()).playing);
  await key("Space");
  await until(async () => (await status()).playing);
  await pages((beatTo += 20));
  {
    await sleep(120);
    await feed(at.x, at.y, swipe(20, -100, 0));
    await frames();
    await sleep(200);
    const s0 = await scroll(tl);
    await feed(at.x, at.y, swipe(30, 60, 0, 8), 8);
    await frames(2);
    const s1 = await scroll(tl);
    check(s1.x - s0.x === 1800, `a swipe back to the playhead moves 1:1 to its end (1800 px): Follow neither pages in it nor drops it (${s1.x - s0.x})`);
  }
  // A hand's scroll with the playhead still in view keeps Follow paused (Q11: until the playhead
  // comes back into view after being out of it): its next page never takes the view back.
  await key("Space");
  await until(async () => !(await status()).playing);
  await key("Space");
  await until(async () => (await status()).playing);
  await pages((beatTo += 20));
  {
    await feed(at.x, at.y, swipe(10, -20, 0));
    await frames();
    await sleep(100);
    const c = await arr();
    const kept = (await scroll(tl)).x;
    // The playhead to 90% of the view: past Follow's 85%, still in view.
    const w = c.view.w - c.margins.left;
    await post("/seek", { beat: (c.x + w * 0.9) / c.zoom });
    await sleep(600);
    check(Math.abs((await scroll(tl)).x - kept) < 2, `a swipe of 200 px with the playhead in view keeps Follow paused: past 85% it doesn't page (${kept} → ${(await scroll(tl)).x})`);
  }
  // A drag holds the camera: a swipe and Follow move nothing until it is let go.
  await key("Space");
  await until(async () => !(await status()).playing);
  await key("Space");
  await until(async () => (await status()).playing);
  await pages((beatTo += 20));
  const t = await box(tl);
  const lane = await box(`${tl} [data-track="hats-1"] [data-lane]`);
  await page.mouse.move(t.x + 400, lane.y + lane.height / 2);
  await page.mouse.down();
  await page.mouse.move(t.x + 460, lane.y + lane.height / 2, { steps: 4 });
  const held = (await scroll(tl)).x;
  await feed(t.x + 700, lane.y + 10, swipe(10, 30, 0));
  await post("/seek", { beat: (beatTo += 40) });
  await sleep(900);
  check(Math.abs((await scroll(tl)).x - held) < 2, `during a drag, a swipe and Follow move nothing (${held} → ${(await scroll(tl)).x})`);
  await page.mouse.up();
  check(await until(async () => Math.abs((await scroll(tl)).x - held) > 300, 2500), `let go, Follow catches up (${(await scroll(tl)).x})`);
  await key("Escape");
  await key("Space");
  await until(async () => !(await status()).playing);
}

// ---------------------------------------------------------------- the camera's own glide, with Linux's flag
{
  await page.evaluate(() => window.m2canvas?.camera?.glide(true));
  await setScroll(tl, 0, 0);
  await frames();
  await sleep(150);
  const at = await lanes();
  await feed(at.x, at.y, swipe(10, 10, 0));
  const xs = [];
  for (let i = 0; i < 8; i++) {
    await frames(1);
    xs.push((await arr())?.x ?? 0);
  }
  const steps = xs.slice(1).map((x, i) => x - xs[i]);
  const slowing = steps.slice(2).every((s, i) => s > 0 && s <= steps[i + 1] + 1e-9);
  check(xs.at(-1) > 100 + 20 && slowing, `with Linux's flag, a swipe's glide carries on after it, slowing (${xs.map((x) => x.toFixed(1)).join(", ")})`);
  await feed(at.x, at.y, [{ t: 0, dx: 1 }]);
  await frames(2);
  const x0 = (await arr())?.x ?? 0;
  await frames(6);
  const x1 = (await arr())?.x ?? 0;
  check(near(x1, x0, 0.01), `the next event stops it (${x0.toFixed(2)} → ${x1.toFixed(2)})`);
  // Towards the left edge 60 px off: the glide meets it and stops there (VIEW-6, review Q13), so
  // the camera is still again a QUIET after, long before such a glide would have slowed to nothing.
  await setScroll(tl, 60, 0);
  await frames();
  await sleep(400);
  await feed(at.x, at.y, swipe(10, -10, 0));
  await sleep(450);
  const edge = await page.evaluate(() => ({ x: window.m2canvas?.camera?.now("arrangement")?.x, moving: window.m2canvas?.camera?.moving?.() }));
  check(edge.x === 0 && edge.moving === false, `a glide that meets the edge stops there: the view still 450 ms on (${json(edge)})`);
  // A mouse's wheel (lines, not pixels): two notches 20 ms apart move two lines, and never glide (review Q16).
  await setScroll(tl, 0, 0);
  await frames();
  await sleep(400);
  const y0 = (await scroll(tl)).y;
  await feed(at.x, at.y, [{ t: 0, dy: 1, mode: 1 }, { t: 20, dy: 1, mode: 1 }], 20);
  await frames(2);
  const y1 = (await scroll(tl)).y;
  await sleep(300);
  const y2 = (await scroll(tl)).y;
  check(y1 - y0 === 66 && y2 === y1, `with Linux's flag, two notches of a mouse's wheel move 66 px and don't glide (${y0} → ${y1} → ${y2})`);
  await page.evaluate(() => window.m2canvas?.camera?.glide(null));
}

// ================================================================ Notes, on a clip of 256 notes
await page.keyboard.press("Escape");
{
  for (let i = 0; i < 6; i++) await key("Meta+Minus");
  await setScroll(tl, 0, 0);
  await frames();
  const clip = await box(`${tl} [data-track="bass-1"] [data-clip]`);
  await page.mouse.click(clip.x + 20, clip.y + clip.height / 2, { clickCount: 2 });
}
const roll = "[data-testid=roll-scroller]";
await page.waitForSelector(`${roll} [data-note]`, { timeout: 10000 }).catch(() => null);
await frames();
{
  const notes = await page.$$eval(`${roll} [data-note]`, (els) => els.length);
  const c = await cam("notes");
  check(notes > 0 && notes < 256, `Notes draws only the notes in view and a margin of the 256: ${notes}${c ? ` (${c.view.w} × ${c.view.h} px)` : ""}`);
  const r = await box(roll);
  const at = { x: r.x + r.width * 0.6, y: r.y + r.height * 0.6 };
  // Zoomed in, so it has room across: the clip opens fitted to the view.
  await feed(r.x + 100, at.y, [{ t: 0, dy: -100, ctrl: true }]);
  await frames();
  await sleep(120);
  await feed(r.x + 100, at.y, [{ t: 0, dy: -100, ctrl: true }]);
  await frames();
  await sleep(120);
  const s0 = await scroll(roll);
  await feed(at.x, at.y, swipe(20, 6, 4));
  await frames();
  const s1 = await scroll(roll);
  check(s0.room.x - s0.x >= 120 && s1.x - s0.x === 120 && s1.y - s0.y === 80, `Notes: a diagonal swipe moves it 1:1 both ways (${s1.x - s0.x} × ${s1.y - s0.y}; room ${s0.room.x - s0.x} across)`);
  // Over a note: 120 events render nothing. First a jump far from where it was (more than half a
  // view): a new window, around where it lands.
  await setScroll(roll, (await scroll(roll)).room.x, null);
  await frames();
  await setScroll(roll, 10, null);
  await frames();
  await sleep(300);
  const note = await page.$$eval(`${roll} [data-note]`, (els, r) => {
    for (const e of els) {
      const b = e.getBoundingClientRect();
      if (b.left > r.x + 100 && b.right < r.x + r.width - 100 && b.top > r.y + 40 && b.bottom < r.y + r.height - 40) return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
    }
    return null;
  }, r);
  if (note) await page.mouse.move(note.x, note.y);
  await sleep(300);
  const r0 = await renders();
  await feed(note?.x ?? at.x, note?.y ?? at.y, swipe(120, 0.5, 0, 8), 8);
  await frames(6);
  await sleep(400);
  const q = since(r0, await renders());
  check(!!note && Object.keys(q).length === 0, `120 events of a swipe over a note render nothing: ${json(q)}`);
  const r1 = await renders();
  await zoomFrames(at.x, at.y, 15, -3);
  const z = since(r1, await renders());
  check((z.notes ?? 0) >= 1 && (z.notes ?? 0) <= 18, `30 ⌘-scroll events, two in each of 15 frames, render Notes at most once a frame (18 frames): ${z.notes ?? 0} renders`);
}
/** The beat under screen x in Notes, from the ruler's bar 1. */
const rollBeat = (x) =>
  page.$eval(`${roll} [data-ppb]`, (e, x) => {
    const ppb = Number(e.dataset.ppb);
    return { beat: (x - e.getBoundingClientRect().left) / ppb, ppb };
  }, x);
{
  const r = await box(roll);
  const at = { x: r.x + r.width * 0.5, y: r.y + r.height * 0.5 };
  const a = await rollBeat(at.x);
  await sleep(120);
  await feed(at.x, at.y, [{ t: 0, dy: -100, ctrl: true }]);
  await frames();
  const b = await rollBeat(at.x);
  check(b.ppb > a.ppb * 1.4 && near(b.beat * b.ppb, a.beat * b.ppb, 1.5), `Notes: ⌘-scroll zooms around the pointer, beat ${a.beat.toFixed(3)} staying (${b.beat.toFixed(3)}, ${a.ppb.toFixed(1)} → ${b.ppb.toFixed(1)})`);
  await pinch(at.x, at.y, [0.9, 0.75]);
  await frames();
  const p = await rollBeat(at.x);
  check(near(p.ppb, b.ppb * 0.75, 0.5) && near(p.beat * p.ppb, a.beat * p.ppb, 1.5), `and a pinch out, around it too (${p.beat.toFixed(3)}, ${p.ppb.toFixed(1)} px a beat)`);
}

check(read("song.ts").length > 0, "first-song's files are still there");
done(errors);
await browser.close();
