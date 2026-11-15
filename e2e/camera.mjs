// The camera (taste.md §3.10, VIEW-1 to VIEW-12; docs/designs/camera/design.md §9): every view moves
// as the hand does, through one camera each. Chrome can't show WebKit's own behaviour, so the suite
// sends the events WebKit would send, at known times, on whatever is under the pointer.
//   library   the camera's functions against known numbers, run in the page (`cullPlan` too); the
//             writer on a box that keeps whole pixels, as WebKit does; its numbers
//   graph     on first-song: a diagonal swipe 1:1; 120 events over a wire render nothing; a pinch
//             renders no node; a swipe that carries a picture's dot under the pointer, and one 60 ms
//             after over it, change no file, one that starts on it is the dot's, to its end; Tab to a
//             bar out of view is taken in
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
//                  events render it at most once a frame, and its grid and take once, not once a
//                  track; a pinch keeps the beat; a zoom renders no clip's mini drawing; each clip's
//                  canvas drawn at the zoom, sharp, its notes each at its own alpha (its pixels read),
//                  the 2 px marks only where a note is under 2 px; a zoom by the hand stretching the
//                  canvases and drawing each once as it settles, ⌘= at once; a long clip's canvas
//                  bounded around what shows at 192 px a beat, drawn again only past it; a click's
//                  scroll taken in; agents' `seen` follows; a swipe, a key's scroll and a write to `scrollLeft`
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
import { drawings, rgb } from "./drawlib.mjs";
import { expand } from "./graphlib.mjs";

// At 2 device px a px, as the owner's Mac draws: the canvases' checks read the screen's own pixels (VIEW-12).
const { browser, page, errors, posts, api, state, key, box, show, view } = await open(1440, 900, { dpr: 2 });

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
 * real time (0: all in one task). With `watch` ({ name, frames, late }): then, for that many frames,
 * each frame's time and camera `name`'s x, read after the camera's own frame (its first event asked
 * for one before this did); frame `late` runs 100 ms long, so the one after it comes late, as frames
 * do on a busy machine.
 */
const feed = (x, y, list, spread = 0, watch = null) =>
  page.evaluate(
    async (x, y, list, spread, watch) => {
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
      if (!watch) return null;
      const seen = [];
      await new Promise((done) => {
        const one = (t) => {
          seen.push({ t, x: window.m2canvas?.camera?.now(watch.name)?.x ?? null });
          if (seen.length === watch.late) for (const end = performance.now() + 100; performance.now() < end; );
          if (seen.length < watch.frames) requestAnimationFrame(one);
          else done();
        };
        requestAnimationFrame(one);
      });
      return seen;
    },
    x,
    y,
    list,
    spread,
    watch,
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
/** A pinch at (x, y) across frames, as WebKit sends one: a `gesturechange` a frame, each scale from its start. Then three frames more. */
const pinchFrames = (x, y, scales) =>
  page.evaluate(
    (x, y, scales) =>
      new Promise((done) => {
        const el = document.elementFromPoint(x, y);
        const send = (type, scale) => {
          const e = new UIEvent(type, { bubbles: true, cancelable: true });
          for (const [k, v] of Object.entries({ scale, clientX: x, clientY: y })) Object.defineProperty(e, k, { value: v });
          el.dispatchEvent(e);
        };
        send("gesturestart", 1);
        let i = 0;
        const one = () => {
          send("gesturechange", scales[i]);
          if (++i < scales.length) return requestAnimationFrame(one);
          send("gestureend", scales.at(-1));
          requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(done)));
        };
        requestAnimationFrame(one);
      }),
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
/**
 * A run of `n` ⌘-scroll events at (x, y), one a frame (`dy` each), where right after event `late` the
 * page is busy 80 ms, and the next event, made 16 ms after that one, is heard only after the task
 * that ran long, behind a timer due 60 ms after it (VIEW-5): as on a busy machine, or a big song's
 * layout, where the trackpad's events wait in a queue. Before each event after the first, whether
 * the camera `name` was laid out already, its zoom ended (`ended`, by the event's number); and how
 * many ms after the last event it was laid out (`end`).
 */
const zoomLate = (name, x, y, n, dy, late) =>
  page.evaluate(
    (name, x, y, n, dy, late) =>
      new Promise((done) => {
        const face = window.m2canvas.camera;
        const ended = [];
        let k = 0, made = 0;
        const one = (queued) => {
          if (k > 0 && face.now(name)?.laid === null) ended.push(k + 1);
          const e = new WheelEvent("wheel", { deltaY: dy, clientX: x, clientY: y, metaKey: true, bubbles: true, cancelable: true });
          // Its own time: when the trackpad made it, 16 ms after the one before it, however late it is heard.
          made = queued === true ? made + 16 : performance.now();
          Object.defineProperty(e, "timeStamp", { value: made });
          (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(e);
          if (++k === late) {
            setTimeout(() => one(true), 60);
            for (const end = performance.now() + 80; performance.now() < end; );
            return;
          }
          if (k < n) return requestAnimationFrame(one);
          const last = performance.now();
          const wait = () => {
            const end = performance.now() - last;
            if (face.now(name)?.laid === null || end > 3000) done({ ended, end: Math.round(end) });
            else requestAnimationFrame(wait);
          };
          requestAnimationFrame(wait);
        };
        requestAnimationFrame(one);
      }),
    name,
    x,
    y,
    n,
    dy,
    late,
  );
/**
 * A pinch at (x, y) over time, as WebKit sends one: `gesturestart`, a `gesturechange` every 8 ms with
 * each of `scales`, and (with `end`) its `gestureend` 8 ms after, each event a task of its own on
 * whatever is under (x, y) then. Resolves once the last is sent.
 */
const pinchOver = (x, y, scales, end = true) =>
  page.evaluate(
    async (x, y, scales, end) => {
      const send = (type, scale) => {
        const e = new UIEvent(type, { bubbles: true, cancelable: true });
        for (const [k, v] of Object.entries({ scale, clientX: x, clientY: y })) Object.defineProperty(e, k, { value: v });
        (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(e);
      };
      const wait = () => new Promise((r) => setTimeout(r, 8));
      send("gesturestart", 1);
      for (const s of scales) {
        await wait();
        send("gesturechange", s);
      }
      if (!end) return;
      await wait();
      send("gestureend", scales.at(-1));
    },
    x,
    y,
    scales,
    end,
  );
/** A pinch's `gestureend` at (x, y), alone. */
const pinchEnd = (x, y, scale) =>
  page.evaluate(
    (x, y, scale) => {
      const e = new UIEvent("gestureend", { bubbles: true, cancelable: true });
      for (const [k, v] of Object.entries({ scale, clientX: x, clientY: y })) Object.defineProperty(e, k, { value: v });
      (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(e);
    },
    x,
    y,
    scale,
  );
/**
 * What the camera `name` stretches (VIEW-5): each marked element's transform. A stretch scales
 * (`scale(…)`); after one, an element keeps a transform that moves nothing a moment, with its layer.
 */
const stretched = (name) => page.evaluate((name) => window.m2canvas?.camera?.stretched?.(name) ?? [], name);
/**
 * Until the camera `name` keeps one zoom again: its live zoom laid out (VIEW-5). Two frames first: a
 * zoom's events start it in the frame after them.
 */
const laidOut = async (name, ms = 3000) => {
  await frames(2);
  return until(async () => (await cam(name))?.laid === null, ms);
};
/**
 * Every frame as it is painted (a ResizeObserver hears each, after its layout), until the function
 * this returns is called, which gives them: whether the camera `name` stretched its view then (its
 * `laid`, and the zoom laid out), how many of what it marks carried a transform, how many renders it
 * put on the page, how far the middles of the items `sel` in its box `boxSel` (each with
 * `data-start` and `data-length`, in beats) were, at most, from where the zoom shown puts them
 * (items too thin to tell, under 8 px, or out of the clear view don't count), and whether the items
 * drawn reach both sides of the clear view (`full`).
 * - `hold: { x, kind }`: the beat under screen x `x`, read from what is drawn (`kind`: "clips" by their
 *   middles; "notes" by their starts, and the roll's grid's left edge, its beat 0; "steps" by the beat
 *   grid's first row's middles), from the two farthest apart, and `off`: how
 *   many px it is from the beat there in the first frame. Judged against the start, never against
 *   each frame's own camera, so a frame where the camera itself jumps shows it.
 * - `fill`: an element that must reach the clear view's right edge (`covers`): nothing blank past it.
 * - `canvases`: how many of the lanes' canvases holding any clip were not drawn at the zoom laid out,
 *   or are not as wide on screen as that drawing stretched by the live zoom's stretch (`cv`): drawn
 *   anew in the frame a zoom is laid out, and stretched once, by the camera, never twice (VIEW-5).
 *   Under a drawn zoom's picture, nothing is stretched: each as wide as it drew.
 *
 * Where the view draws its zoom (the arrangement, VIEW-5): whether its picture showed (`shown`) and
 * painted at the zoom shown in that frame (`painted`); the items are then what it drew (each clip's
 * box, in its list), their middles judged against the zoom shown as the page's are, and `full` and
 * the held point read from them too; `page` is how far the page's own items under it were, at most,
 * from where the zoom laid out puts them: the page laid out as it was, not moved, not stretched.
 */
const watchFrames = async (name, boxSel, sel, { hold = null, fill = null, canvases = false } = {}) => {
  await page.evaluate(
    (name, boxSel, sel, hold, fill, canvases) => {
      const face = window.m2canvas.camera;
      const probe = document.createElement("div");
      probe.style.cssText = "position: fixed; left: 0; top: 0; width: 1px; height: 1px; opacity: 0; pointer-events: none; contain: strict";
      document.body.appendChild(probe);
      const seen = [];
      const renders = () => Object.values(window.m2canvas.frames().total).reduce((a, n) => a + n, 0);
      let rendered = renders();
      let beat0 = null;
      /** [beat, screen x] of what is drawn in the clear part [lo, hi], by `hold.kind`. */
      const anchors = (box, lo, hi) => {
        const out = [];
        if (hold.kind === "steps") {
          const row = box.querySelector("[data-testid=beat-row]");
          const els = [...(row?.querySelectorAll("[data-step]") ?? [])].map((e) => ({ u: Number(e.dataset.step), r: e.getBoundingClientRect() })).sort((a, b) => a.u - b.u);
          for (let i = 0; i + 1 < els.length; i++) out.push([(els[i].u + els[i + 1].u) / 2, els[i].r.left + els[i].r.width / 2]);
        } else if (hold.kind === "notes") {
          // Drawn notes in view or out of it: a note's left is its start, exactly, stretched or not.
          for (const e of box.querySelectorAll("[data-note]")) out.push([Number(e.dataset.start), e.getBoundingClientRect().left]);
          const grid = box.querySelector("[data-testid=roll-grid]");
          if (grid) out.push([0, grid.getBoundingClientRect().left]);
          return out.sort((a, b) => a[1] - b[1]);
        } else
          for (const e of box.querySelectorAll("[data-clip]")) {
            const r = e.getBoundingClientRect();
            if (r.width > 6) out.push([Number(e.dataset.start) + Number(e.dataset.length) / 2, r.left + r.width / 2]);
          }
        return out.filter(([, px]) => px >= lo && px <= hi).sort((a, b) => a[1] - b[1]);
      };
      const ro = new ResizeObserver(() => {
        const c = face.now(name), box = document.querySelector(boxSel);
        if (!c || !box) return;
        const b = box.getBoundingClientRect();
        const lo = b.left + box.clientLeft + c.margins.left, hi = b.left + box.clientLeft + c.view.w;
        const zero = b.left + box.clientLeft + c.origin.x - Math.round(c.x);
        const pic = face.picture?.(name) ?? null;
        // What the picture drew in this frame, where it shows: its clips' boxes, on screen.
        const drawn = pic?.shown ? pic.drawn.clips.map((d) => ({ start: d.start, length: d.length, r: { left: lo + d.x, right: lo + d.x + d.w, width: d.w } })) : null;
        const items = drawn ?? [...box.querySelectorAll(sel)].map((e) => ({ start: Number(e.dataset.start), length: Number(e.dataset.length), r: e.getBoundingClientRect() }));
        let worst = 0, n = 0, left = Infinity, right = -Infinity;
        for (const { start, length, r } of items) {
          if (r.width > 0) (left = Math.min(left, r.left)), (right = Math.max(right, r.right));
          if (r.width < 8 || r.left < lo || r.right > hi) continue;
          worst = Math.max(worst, Math.abs(r.left + r.width / 2 - (zero + (start + length / 2) * c.zoom)));
          n++;
        }
        // The page under a picture: its items where the zoom laid out puts them.
        let page = null;
        if (drawn) {
          const l = c.laid ?? c;
          const zeroLaid = b.left + box.clientLeft + c.origin.x - Math.round(l.x);
          page = 0;
          for (const e of box.querySelectorAll(sel)) {
            const r = e.getBoundingClientRect();
            if (r.width < 8 || r.left < lo || r.right > hi) continue;
            page = Math.max(page, Math.abs(r.left + r.width / 2 - (zeroLaid + (Number(e.dataset.start) + Number(e.dataset.length) / 2) * l.zoom)));
          }
          page = Math.round(page * 100) / 100;
        }
        let off = null;
        if (hold) {
          const a = drawn
            ? drawn.filter((d) => d.r.width > 6 && d.r.left + d.r.width / 2 >= lo && d.r.left + d.r.width / 2 <= hi).map((d) => [d.start + d.length / 2, d.r.left + d.r.width / 2]).sort((p, q) => p[1] - q[1])
            : anchors(box, lo, hi);
          if (a.length >= 2 && a.at(-1)[0] !== a[0][0]) {
            const ppb = (a.at(-1)[1] - a[0][1]) / (a.at(-1)[0] - a[0][0]);
            const beat = a[0][0] + (hold.x - a[0][1]) / ppb;
            beat0 ??= beat;
            off = Math.round(Math.abs(beat - beat0) * ppb * 100) / 100;
          }
        }
        const covers = fill ? (document.querySelector(fill)?.getBoundingClientRect().right ?? -Infinity) >= hi - 1 : null;
        let cv = null;
        if (canvases) {
          const laid = c.laid?.zoom ?? c.zoom;
          // Stretched with the view by a live zoom; under a picture, as it drew.
          const k = pic ? 1 : c.zoom / laid;
          cv = [...box.querySelectorAll("canvas[data-drawing=lane]")].filter((e) => e.drawn && e.drawn.w > 0 && e.getClientRects().length).filter((e) => e.drawn.zoom !== laid || Math.abs(e.getBoundingClientRect().width - e.drawn.w * k) > 1).length;
        }
        const st = face.stretched?.(name) ?? [];
        const now = renders();
        seen.push({
          live: !!c.laid,
          laid: c.laid?.zoom ?? c.zoom,
          stretched: st.filter((s) => s.transform.includes("scale")).length,
          drew: now - rendered,
          worst: Math.round(worst * 100) / 100,
          n,
          full: left <= lo + 2 && right >= hi - 2,
          off,
          covers,
          cv,
          zoom: c.zoom,
          shown: pic ? pic.shown : null,
          painted: pic ? pic.shown && pic.drawn?.zoom === c.zoom : null,
          page,
        });
        rendered = now;
      });
      ro.observe(probe);
      let on = true, wide = false;
      const loop = () => {
        if (!on) return;
        wide = !wide;
        probe.style.width = wide ? "2px" : "1px";
        requestAnimationFrame(loop);
      };
      requestAnimationFrame(loop);
      window.__watch = () => {
        on = false;
        ro.disconnect();
        probe.remove();
        return seen;
      };
    },
    name,
    boxSel,
    sel,
    hold,
    fill,
    canvases,
  );
  return () => page.evaluate(() => window.__watch());
};
/**
 * Whether the frames from `watchFrames` render only where a live zoom may (VIEW-8): at a frame laid
 * out (its end, or an early layout), or in a stretched frame after an early layout in that gesture,
 * where the new window it asked for lands between frames (VIEW-12); never in another stretched frame.
 */
const rendersAllowed = (fs) => {
  let first = null, early = false;
  for (const f of fs) {
    if (f.live) {
      if (first === null) first = f.laid;
      else if (f.laid !== first) early = true;
      if (f.drew && !early) return false;
    } else if (first !== null) early = true;
  }
  return true;
};
/** Whether every frame from `watchFrames` with a held point kept it within a px of where it was at the start. */
const pointHeld = (fs) => fs.length > 0 && fs.every((f) => f.off !== null && f.off <= 1);
/**
 * What frames from `watchFrames` prove of a live zoom (VIEW-5, VIEW-8): some showed it stretched,
 * every stretched one carried the transform and rendered only where VIEW-8 allows (`rendersAllowed`),
 * every other carried none (no frame shows both the stretch and the new layout), the last was laid
 * out, and in each the items sat where the zoom shown puts them (unless `items` is false: a view whose
 * items have no length, as the beat grid's steps); with the lanes' canvases watched, each drawn at the
 * zoom laid out and stretched once, in every frame; with a held point, it stayed put in
 * every frame, the end's too (`pointHeld`).
 */
const liveFrames = (fs, { items = true } = {}) => {
  const live = fs.filter((f) => f.live);
  return (
    live.length >= 2 &&
    live.every((f) => f.stretched > 0) &&
    rendersAllowed(fs) &&
    fs.filter((f) => !f.live).every((f) => f.stretched === 0) &&
    !!fs.length &&
    !fs.at(-1).live &&
    (!items || fs.every((f) => f.n > 0 && f.worst <= 1)) &&
    (fs[0].off === null || pointHeld(fs)) &&
    fs.every((f) => !f.cv)
  );
};
/**
 * What frames from `watchFrames` prove of a drawn zoom (VIEW-5, VIEW-8, VIEW-13), the arrangement's:
 * some were zoomed by the hand; in each of those the picture showed, painted at the zoom shown, nothing
 * on the page stretched, nothing rendered, and the page stayed laid out at the zoom the gesture began
 * at (never laid out before the end), its items where that zoom puts them; in every other the
 * picture was hidden; the last was laid out; in every frame each item (the picture's, or the page's)
 * sat within a px of where the zoom shown puts it, each lane's canvas was drawn at the zoom laid out
 * and not stretched, and the held point stayed put (`pointHeld`); and (`full`) every frame drew
 * clips across the whole view.
 */
const drawnFrames = (fs, { full = false } = {}) => {
  const live = fs.filter((f) => f.live);
  const first = live[0]?.laid;
  return (
    live.length >= 2 &&
    live.every((f) => f.shown && f.painted && f.stretched === 0 && !f.drew && f.laid === first && f.page !== null && f.page <= 1) &&
    fs.filter((f) => !f.live).every((f) => f.shown === false && f.stretched === 0) &&
    !fs.at(-1).live &&
    fs.every((f) => f.n > 0 && f.worst <= 1 && !f.cv && (!full || f.full)) &&
    (fs[0].off === null || pointHeld(fs))
  );
};
/** Frames from `watchFrames`, briefly: the picture (P), stretched (S) or laid out (L), its transforms, its renders (+n), its items and their worst px off, the page's under a picture (^), and the held point's px off (@). */
const brief = (fs) =>
  fs
    .map((f) => `${f.shown ? "P" : f.live ? "S" : "L"}${f.stretched}${f.drew ? `+${f.drew}` : ""}/${f.n}/${f.worst}${f.page ? `^${f.page}` : ""}${f.off === null ? "" : `@${f.off}`}${f.covers === false ? "!" : ""}${f.cv ? `~${f.cv}` : ""}${f.shown && !f.painted ? "?" : ""}`)
    .join(" ");

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
  // A live zoom (VIEW-5): a view laid out at 24 px a beat and scrolled 100 px, shown at 36 px a beat
  // from 130.6 px. Stretched ×1.5 across around its origin, and moved to where its end will scroll it.
  const shown = { ...arr, x: 130.6, y: 40, zoom: 36, laid: { x: 100, y: 40, zoom: 24 } };
  out.stretch = c.stretchOf
    ? [c.stretchOf(shown), c.stretchOf({ ...shown, laid: null }), c.shownAt(shown, 10), c.shownAt({ ...shown, laid: null, zoom: 24 }, 10), c.laidOf(shown), c.laidOf({ ...shown, laid: null }).zoom]
    : null;
  // Where the box shows the content while it is stretched, in the zoom shown's px: the box scrolled
  // to 100 shows from 131 (the stretch moves it 31 px); with no live zoom, from where it is scrolled.
  out.scrolled = c.shownScroll ? [c.shownScroll(shown, { x: 100, y: 40 }), c.shownScroll({ ...shown, laid: null }, { x: 100, y: 40 })] : null;
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
check(
  json(lib?.stretch) === json([{ scale: { x: 1.5, y: 1 }, shift: { x: -31, y: 0 } }, null, 329, 240, { x: 100, y: 40, zoom: 24 }, 36]),
  `stretchOf, shownAt and laidOf: laid out at 24 px a beat, shown at 36, it is stretched ×1.5 across and moved 31 px; beat 10 shows 329 px from the origin (240 with no live zoom); the camera laid out is its laid, else itself: ${json(lib?.stretch)}`,
);
check(
  json(lib?.scrolled) === json([{ x: 131, y: 40 }, { x: 100, y: 40 }]),
  `shownScroll: a box scrolled to 100 under that stretch shows the content from 131 px of the zoom shown (reveal reads one zoom), and from 100 with none: ${json(lib?.scrolled)}`,
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
// A pinch over the Graph renders the Graph at most once a frame, and none of its nodes: the Graph
// scales them as one (VIEW-8).
{
  const g = await box(graph);
  const at = { x: g.x + g.width * 0.5, y: g.y + g.height * 0.5 };
  const z0 = (await graphCam())?.zoom;
  const nodes = (await page.$$(`${graph} [data-node-id]`)).length;
  const scales = [0.95, 0.9, 0.85, 0.8, 0.75, 0.8, 0.85, 0.9, 0.95, 1];
  const r0 = await renders();
  await pinchFrames(at.x, at.y, scales);
  await sleep(300);
  const r = since(r0, await renders());
  const z1 = (await graphCam())?.zoom;
  check(
    nodes > 0 && (r.graph ?? 0) >= 1 && (r.graph ?? 0) <= scales.length + 3 && !r["graph.nodes"] && z0 !== undefined && near(z1, z0, 1e-6),
    `a pinch over the Graph (${scales.length} frames, out and back to ${z0}) renders it at most once a frame and none of its ${nodes} nodes: ${json(r)}`,
  );
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
  // A live zoom (VIEW-5, VIEW-8): a pinch over time stretches the grid, renders nothing, keeps the
  // step under the fingers, and is laid out once when it ends.
  await laidOut("beat");
  await sleep(300);
  const z0 = (await cam("beat"))?.zoom;
  const a2 = await beatX(at.x);
  const r0 = await renders();
  await pinchOver(at.x, at.y, [0.95, 0.9], false);
  await frames(2);
  const mid = await cam("beat");
  const st = await stretched("beat");
  const m = await beatX(at.x);
  const r = since(r0, await renders());
  check(
    !!z0 && mid?.laid?.zoom === z0 && near(mid.zoom, z0 * 0.9, 0.01) && st.length > 0 && st.every((s) => s.transform.includes("scale")) && Object.keys(r).length === 0 && near(m.ppb, z0 * 0.9, 0.5) && near(m.beat * m.ppb, a2.beat * m.ppb, 1.5),
    `the beat grid mid-pinch: stretched (${st.length} layers), nothing rendered (${json(r)}), beat ${a2.beat.toFixed(3)} still under the fingers (${m.beat.toFixed(3)}), its camera showing ${mid?.zoom?.toFixed(2)} px a beat, laid out at ${mid?.laid?.zoom?.toFixed(2)}`,
  );
  await pinchEnd(at.x, at.y, 0.9);
  await laidOut("beat");
  await frames();
  const e = since(r0, await renders());
  const st2 = await stretched("beat");
  const p2 = await beatX(at.x);
  check(
    e.beat === 1 && st2.length > 0 && st2.every((s) => !s.transform.includes("scale")) && near(p2.ppb, z0 * 0.9, 0.5) && near(p2.beat * p2.ppb, a2.beat * p2.ppb, 1.5),
    `its gestureend lays it out once (${json(e)}), the stretch gone, the beat still under the fingers (${p2.beat.toFixed(3)})`,
  );
  // A run of ⌘-scroll with the page busy 80 ms in its middle, an event waiting behind it (VIEW-5):
  // its end is counted in the frames after its last event, each at most G/2, never by a timer, so
  // the late frame never ends it early.
  {
    const G = await page.evaluate(() => window.m2canvas.camera.numbers.SWIPE_GAP);
    await laidOut("beat");
    await sleep(300);
    const z0 = (await cam("beat"))?.zoom;
    const l = await zoomLate("beat", at.x, at.y, 12, -3, 5);
    await frames(2);
    // Back to the zoom it had, laid out at once (all in one task), for what comes after.
    await laidOut("beat");
    const z1 = (await cam("beat"))?.zoom;
    if (z0 && z1) await pinch(at.x, at.y, [z0 / z1]);
    await frames(2);
    check(
      l.ended.length === 0 && l.end >= G - 1 && l.end <= G + 1000,
      `the beat grid: a run of 12 ⌘-scroll events, one a frame, the page busy 80 ms after the fifth and the sixth heard after a timer due 60 ms on, is never laid out before its last event (laid out before events ${json(l.ended)}), and laid out ${l.end} ms after it (G ${G} ms)`,
    );
  }
  // Near its right edge (VIEW-5, VIEW-6): at its least zoom, from its start, its content no wider than
  // the view. A pinch in ×1.6, held at the middle and near the right, keeps the step under the
  // fingers in every frame, the end's too: the grid keeps room where the zoom looked.
  for (const fx of [0.5, 0.97]) {
    // To the least zoom around the clear view's left edge, laid out at once: from the start, so the
    // room a zoom looked at is the view's own.
    await setScroll(beat, 0, null);
    await frames(2);
    const c0 = await cam("beat");
    await pinch(Math.round(b.x + (c0?.margins.left ?? 0) + 1), b.y + 60, [16 / (c0?.zoom ?? 88)]);
    await frames(2);
    await setScroll(beat, 0, null);
    await frames(2);
    await sleep(300);
    const c = await cam("beat");
    const room = (await scroll(beat)).room.x;
    const q = { x: Math.round(b.x + c.margins.left + (c.view.w - c.margins.left) * fx), y: b.y + 60 };
    const stop = await watchFrames("beat", beat, "[data-none]", { hold: { x: q.x, kind: "steps" } });
    await pinchOver(q.x, q.y, [1.1, 1.2, 1.3, 1.4, 1.5, 1.6]);
    await laidOut("beat");
    await frames(3);
    const fs = await stop();
    check(
      c?.zoom === 16 && room < 100 && liveFrames(fs, { items: false }),
      `the beat grid at its least zoom (${c?.zoom}), its content as wide as the view (${room} px of room): a pinch in ×1.6 at ${fx} across keeps the step under the fingers in every frame, the end's too (${brief(fs)})`,
    );
  }
}

// ================================================================ the big song

const big = path.join(ROOT, "big");
/** Bass 1's empty clip and its clip of one bar, as the big song is made. */
let bassClips = null;
execFileSync(process.execPath, [path.join(path.dirname(fileURLToPath(import.meta.url)), "big-song.mjs"), big]);
// Bass 1's first clip: 64 beats of 256 notes over four octaves, for Notes (its other clips after it).
{
  const file = path.join(big, "data/bass-1/notes.json");
  const lane = JSON.parse(fs.readFileSync(file, "utf8"));
  const notes = Array.from({ length: 256 }, (_, i) => ({ pitch: 36 + ((i * 7) % 48), start: i / 4, length: 0.25, velocity: 0.8 }));
  lane.clips = [{ ...lane.clips[0], start: 0, length: 64, notes }, ...lane.clips.slice(16)];
  // Its next clip empty (Notes' hint), the one after it a bar of six notes (Notes on a short clip).
  lane.clips[1] = { ...lane.clips[1], notes: [] };
  fs.writeFileSync(file, JSON.stringify(lane));
  bassClips = { empty: lane.clips[1], bar: lane.clips[2] };
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
  // A view that marks nothing zooms as before the live zoom (VIEW-5): the Graph. ⌘-scroll over ten
  // frames lands each frame's zoom in that frame, laid out, with nothing stretched.
  {
    const name = (await graphCam())?.name;
    await sleep(200);
    const stop = await watchFrames(name, graph, "[data-none]");
    await zoomFrames(n.x + 4, n.y + 4, 10, 4);
    await sleep(200);
    const fs = await stop();
    const zooms = new Set(fs.map((f) => f.zoom));
    check(
      !!name && fs.length >= 10 && fs.every((f) => !f.live && f.stretched === 0) && zooms.size >= 8 && (await stretched(name)).length === 0,
      `a view that marks nothing (the Graph) zooms as before: ${zooms.size} zooms over ${fs.length} frames, each laid out in its frame, nothing stretched (${json([...zooms].map((z) => Math.round(z * 1000) / 1000))})`,
    );
  }
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
  // A live zoom: laid out once, G after its last event (VIEW-5).
  await laidOut("arrangement");
  await frames(4);
  const r = since(r0, await renders());
  check((r.arrangement ?? 0) >= 1 && (r.arrangement ?? 0) <= 1, `30 ⌘-scroll events in one go render the arrangement once (${r.arrangement ?? 0})`);
  const r1 = await renders();
  await zoomFrames(at.x, at.y, 15, 3);
  await laidOut("arrangement");
  await frames();
  const q = since(r1, await renders());
  check((q.arrangement ?? 0) >= 1 && (q.arrangement ?? 0) <= 18, `30 more, two in each of 15 frames, render it at most once a frame (18 frames): ${q.arrangement ?? 0} renders`);
  check((r["arrangement.drawings"] === undefined || r["arrangement.drawings"] === 0) && (await page.evaluate(() => "arrangement.drawings" in (window.m2canvas?.frames?.().total ?? {}))), `a zoom in renders no clip's mini drawing: ${r["arrangement.drawings"] ?? 0} (each counted)`);
  // The grid and the recording take render with the zoom once, never once a track (VIEW-8): the
  // grid's lines (`arrangement.grid`) each time the zoom renders the arrangement, no row's grid
  // (`arrangement.grid.rows`), and no lane's take while none records.
  const counted = await page.evaluate(() => ["arrangement.grid", "arrangement.grid.rows", "arrangement.take"].every((k) => k in (window.m2canvas?.frames?.().total ?? {})));
  const rows = (await page.$$(`${tl} [data-track]`)).length;
  check(
    counted && [r, q].every((x) => (x["arrangement.grid"] ?? 0) >= 1 && (x["arrangement.grid"] ?? 0) <= (x.arrangement ?? 0) && !x["arrangement.grid.rows"] && !x["arrangement.take"]),
    `a zoom renders the grid's lines once each time it renders the arrangement, and no row's grid or take of the ${rows} tracks (each counted): ${json({ "in one go": r, "a frame each": q })}`,
  );
}
// A clip's notes on its lane's canvas (VIEW-12; review Q9 before it): drawn at the camera's zoom once a
// zoom settles, sharp at the screen's own pixels (its bitmap its box at devicePixelRatio, not
// stretched), each note inside its clip's box as painted, at its own alpha (its middle pixel the
// track's ink at that alpha; no opacity on the canvas, which WebKit would paint through a layer of
// its own), its 2 px marks only in a clip whose shortest note is under 2 px at that zoom, each
// centred on its note. At this zoom and six steps out.
{
  const look = async () => {
    await sleep(200);
    await frames(2);
    const zoom = (await arr())?.zoom ?? 0;
    const dpr = await page.evaluate(() => devicePixelRatio);
    const all = (await page.$$eval(`${tl} [data-clip]`, drawings)).filter((d) => d?.kind === "lane" && d.notes.length > 0);
    const wrong = [];
    for (const d of all) {
      const fine = (Math.min(...d.notes.map((n) => n.w)) / d.k) * zoom < 2;
      const ink = rgb(d.ink);
      const why = [];
      if (d.zoom !== zoom) why.push(`drawn at ${d.zoom}`);
      if (d.stretch !== 1 || d.scale !== dpr || d.bitmap[0] !== Math.round(d.drawnBox[0] * dpr) || d.bitmap[1] !== Math.round(d.drawnBox[1] * dpr)) why.push(`not sharp: ${json({ stretch: d.stretch, scale: d.scale, bitmap: d.bitmap, box: d.drawnBox })}`);
      if (d.opacity !== "1") why.push(`opacity ${d.opacity}`);
      // The box on the screen's own pixels, as the canvas rounds it (one function rounds both), and
      // each note inside it, within its 1 px border and 4 px more at its top and foot.
      const onPx = (v) => near(v * dpr, Math.round(v * dpr), 0.01);
      if (!onPx(d.clip.left) || !onPx(d.clip.right)) why.push(`the box ${json(d.clip)} is off the screen's pixels`);
      const [l, r] = [d.clip.left + 1, d.clip.right - 1];
      const out = d.notes.filter((n) => n.x < l - 0.01 || n.x + n.w > r + 0.01 || n.y < d.clip.top + 5 - 0.01 || n.y + n.h > d.clip.bottom - 5 + 0.01);
      if (out.length) why.push(`${out.length} notes outside the box ${json(d.clip)}: ${json(out[0])}`);
      if (d.marks.length !== (fine ? d.notes.length : 0)) why.push(`${d.marks.length} marks for ${d.notes.length} notes, fine ${fine}`);
      d.marks.forEach((m, i) => {
        const n = d.notes[i];
        if (!near(m.w, 2, 0.01) || !near(m.x + m.w / 2, n.x + n.w / 2, 0.01) || !near(m.h, n.h, 0.01) || m.alpha !== n.alpha) why.push(`mark ${i} ${json(m)} for ${json(n)}`);
      });
      for (const n of d.notes) {
        if (n.w < 3 || n.h < 3) continue;
        const a = fine ? 1 - (1 - n.alpha) ** 2 : n.alpha;
        const p = n.pixel;
        if (!p || Math.abs(p[3] - a * 255) > 4 || p.slice(0, 3).some((v, j) => Math.abs(v - ink[j]) > 3)) why.push(`note ${json(n)}: its pixel ${json(p)}, not ${json([...ink, Math.round(a * 255)])}`);
      }
      if (why.length) wrong.push({ start: d.start, why: why.slice(0, 3) });
    }
    // Every clip whose box shows notes in the beats its lane's canvas holds is in that canvas's list,
    // with each of its audible notes there: a lane's canvas that left a clip out would draw an empty box.
    const st = await state();
    const lanes = await page.$$eval(`${tl} [data-lane]`, (els) =>
      els.map((e) => {
        const c = e.querySelector("canvas[data-drawing=lane]");
        const d = c?.drawn;
        return {
          lane: e.dataset.lane,
          held: d ? [d.from, d.to] : null,
          clips: [...e.querySelectorAll("[data-clip]")]
            .filter((x) => !x.querySelector("canvas[data-drawing]"))
            .map((x) => {
              const start = Number(x.dataset.start), length = Number(x.dataset.length);
              const got = d?.clips.find((y) => y.start === start && y.length === length);
              return { start, length, notes: got ? got.notes.length : null, k: got?.k ?? null };
            }),
        };
      }),
    );
    const missing = [];
    let listed = 0;
    for (const l of lanes) {
      const [module, key] = l.lane.split(".");
      const data = st.timeline.tracks.find((t) => t.module === module)?.lanes.find((x) => x.key === key);
      if (data?.kind !== "notes") continue;
      for (const c of l.clips) {
        const clip = data.clips.find((x) => x.start === c.start && x.length === c.length);
        const audible = (clip?.notes ?? []).map((n) => [Math.max(n.start, 0), Math.min(n.start + n.length, c.length)]).filter(([a, b]) => b > a);
        if (!l.held || c.start + c.length < l.held[0] || c.start > l.held[1] || !audible.length) continue;
        // A thin note's 2 px mark reaches past it: those notes that near the part held count too.
        const slack = 2 / (c.k ?? zoom);
        const want = audible.filter(([a, b]) => b >= l.held[0] - c.start - slack && a <= l.held[1] - c.start + slack).length;
        listed++;
        if (c.notes !== want) missing.push({ lane: l.lane, start: c.start, drew: c.notes, want });
      }
    }
    const alphas = new Set(all.flatMap((d) => d.notes.map((n) => n.alpha)));
    return { zoom, clips: all.length, listed, marked: all.filter((d) => d.marks.length).length, alphas: alphas.size, wrong: wrong.slice(0, 4), wrongs: wrong.length, missing: missing.slice(0, 4), missed: missing.length };
  };
  const a = await look();
  for (let i = 0; i < 6; i++) await key("Meta+Minus");
  await frames(3);
  const b = await look();
  for (let i = 0; i < 6; i++) await key("Meta+Equal");
  await frames(3);
  check(
    a.clips > 0 && b.clips > 0 && a.listed >= a.clips && b.listed >= b.clips && a.wrongs + b.wrongs + a.missed + b.missed === 0 && b.marked > 0 && a.alphas > 0,
    `a clip's notes on its lane's canvas: every clip in the beats it holds, each with all its notes there; at the zoom, sharp, each clip's box on the screen's pixels and its notes inside it, each at its own alpha in the track's ink, the 2 px marks only where the shortest note is under 2 px (${json({ a, b })})`,
  );
}
// A zoom by the hand shows the arrangement's picture, painted at the zoom shown each frame, and leaves
// each lane's canvas under it as it drew, not stretched, drawing none again until the zoom is laid out
// (no ⌘-scroll for G); then it draws each once, at the new zoom, in the frame it is laid out in, and
// the picture hides. A zoom by the app (⌘=) draws them in the frame it lands in (VIEW-5, VIEW-12).
{
  await setScroll(tl, 0, 0);
  await frames();
  await sleep(300);
  const at = await lanes();
  const z0 = (await arr())?.zoom;
  const n0 = await page.$$eval(`${tl} canvas[data-drawing]`, (els) => (window.__canvases = els).length);
  const r0 = await renders();
  const watch = await watchFrames("arrangement", tl, "[data-clip]", { canvases: true });
  // 10 frames of ⌘-scroll in, read in the frame after the last event: well before G.
  const mid = await page.evaluate(
    (x, y) =>
      new Promise((done) => {
        let k = 0;
        const send = () => (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(new WheelEvent("wheel", { deltaY: -3, clientX: x, clientY: y, metaKey: true, bubbles: true, cancelable: true }));
        const one = () => {
          send();
          send();
          if (++k < 10) return requestAnimationFrame(one);
          requestAnimationFrame(() =>
            setTimeout(() => {
              const els = window.__canvases.filter((e) => e.isConnected);
              const pic = window.m2canvas.camera.picture?.("arrangement");
              done({
                zoom: window.m2canvas.camera.now("arrangement").zoom,
                picture: pic && { shown: pic.shown, zoom: pic.drawn?.zoom ?? null, paints: pic.paints },
                kept: els.length,
                // Those holding any of their clip (one far from the view may hold none of it).
                drawn: els.filter((e) => e.drawn.w > 0).length,
                stretched: els.filter((e) => e.drawn.w > 0 && Math.abs(e.getBoundingClientRect().width - e.drawn.w) > 0.5).length,
                at: [...new Set(els.map((e) => e.drawn.zoom))],
                total: window.m2canvas.frames().total,
              });
            }, 0),
          );
        };
        requestAnimationFrame(one);
      }),
    at.x,
    at.y,
  );
  const r1 = since(r0, mid.total);
  await sleep(300);
  await frames(2);
  const fs = await watch();
  const r2 = since(r0, await renders());
  const end = await page.evaluate(() => {
    const z = window.m2canvas.camera.now("arrangement").zoom;
    const els = window.__canvases.filter((e) => e.isConnected);
    return { zoom: z, n: els.length, at: els.filter((e) => e.drawn.zoom === z && Math.abs(e.getBoundingClientRect().width - e.drawn.w) < 0.5).length };
  });
  check(
    n0 > 0 && mid.zoom > z0 * 1.2 && !!mid.picture?.shown && mid.picture.zoom === mid.zoom && mid.kept === n0 && json(mid.at) === json([z0]) && mid.drawn > 0 && mid.stretched === 0 && !r1["arrangement.canvas"],
    `while 20 ⌘-scroll events zoom in (${z0} → ${mid.zoom?.toFixed(2)} px a beat), the arrangement's picture shows, painted at the zoom shown (${json(mid.picture)}); the ${n0} lanes' canvases under it stay as drawn (${mid.stretched} of the ${mid.drawn} holding any of their lane stretched), each still at ${json(mid.at)}, none drawn again: ${json(r1)}`,
  );
  check(
    n0 > 0 && end.zoom === mid.zoom && end.at === end.n && end.n === n0 && (r2["arrangement.canvas"] ?? 0) === n0 && fs.some((f) => f.live) && !fs.at(-1).live && fs.every((f) => !f.cv && f.shown === f.live && (!f.live || f.painted)),
    `once it is laid out, each is drawn again once (${r2["arrangement.canvas"] ?? 0} of ${n0}), at the new zoom and its box (${end.at}): in every frame, each lane's canvas drawn at the zoom laid out and as wide as it drew, the picture showing exactly while the zoom was not laid out, painted at the zoom shown, the frame it is laid out in too (${brief(fs)})`,
  );
  const r3 = await renders();
  await key("Meta+Equal");
  await frames(1);
  const app = await page.evaluate(() => {
    const z = window.m2canvas.camera.now("arrangement").zoom;
    const els = window.__canvases.filter((e) => e.isConnected);
    return { zoom: z, n: els.length, at: els.filter((e) => e.drawn.zoom === z && Math.abs(e.getBoundingClientRect().width - e.drawn.w) < 0.5).length };
  });
  const r4 = since(r3, await renders());
  check(
    near(app.zoom, end.zoom * 1.5, 0.01) && app.n > 0 && app.at === app.n && (r4["arrangement.canvas"] ?? 0) >= app.n,
    `⌘= draws every canvas again in the frame it lands in, at its zoom (${app.at} of ${app.n}, ${app.zoom.toFixed(2)} px a beat): ${json(r4)}`,
  );
  // And the grid's lines follow it: a row's beat and bar lines are the new px a beat and a bar (4/4) apart.
  const lines = await page.evaluate(() =>
    [...document.querySelectorAll('[data-testid=timeline] [data-lane="kick-1.hits"] svg rect[fill^="url(#"]')].slice(0, 3).map((r) => {
      const p = document.getElementById(r.getAttribute("fill").slice(5, -1));
      return p ? { every: Number(p.getAttribute("width")), drawn: !!p.querySelector("rect") } : null;
    }),
  );
  check(
    lines.length === 3 && lines[1]?.drawn && lines[2]?.drawn && near(lines[1].every, app.zoom, 0.001) && near(lines[2].every, app.zoom * 4, 0.001),
    `the grid's lines follow the zoom: a beat ${lines[1]?.every} px and a bar ${lines[2]?.every} px apart at ${app.zoom.toFixed(3)} px a beat`,
  );
  await key("Meta+Minus");
  await frames(2);
}
// Bounded: at 192 px a beat, the canvas of Bass 1's lane, whose first clip is 64 beats (12,288 px),
// holds only what shows and up to a view more each side, in a bitmap of at most 8192 × 512 device px,
// sharp; a pan within that draws it not again; a pan past it draws it again at once, around what
// shows then (VIEW-12).
{
  const bass = `${tl} [data-track="bass-1"] [data-clip]`;
  for (let i = 0; i < 8; i++) await key("Meta+Equal");
  await until(async () => (await arr())?.zoom === 192, 3000);
  await frames(2);
  const view = async () => {
    const c = await arr();
    const w = c.view.w - c.margins.left - c.margins.right;
    return { from: c.x / c.zoom, to: (c.x + w) / c.zoom, w };
  };
  // The view's middle at beat 32 of the clip.
  const v0 = await view();
  await setScroll(tl, Math.round(32 * 192 - v0.w / 2), null);
  await frames(3);
  await sleep(300);
  const dpr = await page.evaluate(() => devicePixelRatio);
  const holds = (d, v) => !!d && d.from <= v.from - d.start + 1e-6 && d.to >= Math.min(d.length, v.to - d.start) - 1e-6;
  const a = (await page.$$eval(bass, drawings))[0];
  const va = await view();
  const sharp = (d) => d.scale === dpr && d.bitmap[0] === Math.round(d.drawnBox[0] * dpr) && d.stretch === 1;
  check(
    !!a && a.kind === "lane" && a.start === 0 && a.length === 64 && a.from > 0 && a.to < 64 && a.to - a.from <= 3 * (va.to - va.from) + 1e-6 && holds(a, va) && a.bitmap[0] <= 8192 && a.bitmap[1] <= 512 && sharp(a),
    `at 192 px a beat its lane's canvas holds beats ${a?.from.toFixed(2)} to ${a?.to.toFixed(2)} of its 64, around what shows (${va.from.toFixed(2)} to ${va.to.toFixed(2)}), in ${json(a?.bitmap)} device px, sharp`,
  );
  const r0 = await renders();
  await setScroll(tl, Math.round(32 * 192 - va.w / 2 + va.w * 0.4), null);
  await frames(3);
  const r1 = since(r0, await renders());
  const b = (await page.$$eval(bass, drawings))[0];
  check(!!a && !!b && !r1["arrangement.canvas"] && b.from === a.from && b.to === a.to && holds(b, await view()), `a pan of 0.4 views within what it holds draws it not again: ${json(r1)}`);
  await setScroll(tl, Math.round(32 * 192 - va.w / 2 + va.w * 1.6), null);
  await frames(3);
  const r2 = since(r0, await renders());
  const c = (await page.$$eval(bass, drawings))[0];
  const vc = await view();
  check(
    (r2["arrangement.canvas"] ?? 0) >= 1 && holds(c, vc) && c.from > a.from && c.bitmap[0] <= 8192 && sharp(c),
    `a pan of 1.6 views draws it again at once, around what shows then: beats ${c?.from.toFixed(2)} to ${c?.to.toFixed(2)} for ${vc.from.toFixed(2)} to ${vc.to.toFixed(2)} (${json(r2)})`,
  );
  for (let i = 0; i < 8; i++) await key("Meta+Minus");
  await frames(2);
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

// ---------------------------------------------------------------- the drawn zoom (VIEW-5, VIEW-8, VIEW-13)
// A pinch over time: the arrangement's picture shows, painted at the zoom shown; the page under it
// stays laid out as it was (nothing stretched, nothing rendered); the beat under the fingers holds
// and the camera says the zoom shown; what the picture draws over the tracks lines up with it. Its
// gestureend lays the page out once, in the frame the picture hides, as a direct zoom to the same zoom
// and place does.
/** Each overlay of the arrangement, how many px it is off where the zoom shown puts it (null: not drawn). */
const overlays = (marker) =>
  page.evaluate(
    (tl, marker) => {
      const c = window.m2canvas.camera.now("arrangement");
      const box = document.querySelector(tl);
      const b = box.getBoundingClientRect();
      const zero = b.left + box.clientLeft + c.origin.x - Math.round(c.x);
      const off = {};
      const one = (what, el, from, to) => {
        if (!el) return void (off[what] = null);
        const r = el.getBoundingClientRect();
        off[what] = Math.round(Math.max(Math.abs(r.left - (zero + from * c.zoom)), to === undefined ? 0 : Math.abs(r.width - (to - from) * c.zoom)) * 100) / 100;
      };
      const range = box.querySelector("[data-testid=loop-range]");
      const loop = range && [Number(range.dataset.start), Number(range.dataset.end)];
      const sel = box.querySelector("[data-testid=section]");
      const bars = sel && [Number(sel.dataset.start), Number(sel.dataset.end)];
      const head = box.querySelector("[data-testid=playhead]");
      one("loop's shade", box.querySelector("[data-testid=loop-shade]"), ...(loop ?? [0, 0]));
      one("loop in the ruler", range, ...(loop ?? [0, 0]));
      one("Song row's loop", box.querySelector("[data-song-row] > .mix-blend-multiply"), ...(loop ?? [0, 0]));
      one("bars selected", sel, ...(bars ?? [0, 0]));
      one("bars in the ruler", box.querySelector("[data-testid=section-band]"), ...(bars ?? [0, 0]));
      one("Song row's bars", box.querySelector("[data-song-row] > .rounded-xs"), ...(bars ?? [0, 0]));
      one("marker", box.querySelector(`[data-marker-name="${marker.name}"]`), marker.beat);
      one("playhead", head, Number(head?.dataset.beat));
      // The grid: a lane's lines from beat 0.
      one("grid", box.querySelector('[data-track="kick-1"] [data-lane] > :nth-child(2) > :first-child'), 0);
      return off;
    },
    tl,
    marker,
  );
/**
 * What the arrangement's picture drew over the tracks, each how many px off where the zoom shown puts
 * it (null: not drawn): its last paint's list, the camera's `x` its box's scroll at the zoom's end.
 */
const drawnOverlays = (marker) =>
  page.evaluate((marker) => {
    const face = window.m2canvas.camera;
    const c = face.now("arrangement"), d = face.picture?.("arrangement")?.drawn;
    if (!d) return null;
    const X = Math.max(0, Math.round(c.x)), z = c.zoom;
    const loop = window.__loop, bars = window.__bars;
    const off = {};
    const one = (what, got, from, to) => {
      if (!got) return void (off[what] = null);
      const [x, w] = typeof got === "number" ? [got, null] : [got.x, got.w];
      off[what] = Math.round(Math.max(Math.abs(x - (from * z - X)), w === null || to === undefined ? 0 : Math.abs(w - (to - from) * z)) * 100) / 100;
    };
    one("loop's shade", d.loop, ...loop);
    one("loop in the ruler", d.brace, ...loop);
    one("Song row's loop", d.songLoop, ...loop);
    one("bars selected", d.section, ...bars);
    one("bars in the ruler", d.band, ...bars);
    one("Song row's bars", d.songSection, ...bars);
    one("marker", d.marks.find((m) => m.name === marker.name)?.x, marker.beat);
    const beat = Number(document.querySelector("[data-testid=playhead]")?.dataset.beat);
    one("playhead", d.playhead, beat);
    one("Song row's playhead", d.songPlayhead, beat);
    one("playhead's cap", d.cap, beat);
    one("grid", d.grid, 0);
    return off;
  }, marker);
/** The beat under screen x from the middles of two of Kick 1's clips the arrangement's picture drew in view, and its px a beat. */
const beatDrawn = (x) =>
  page.evaluate(
    (tl, x) => {
      const face = window.m2canvas.camera;
      const c = face.now("arrangement"), d = face.picture?.("arrangement")?.drawn;
      const e = document.querySelector(tl), b = e.getBoundingClientRect();
      const lo = b.left + e.clientLeft + c.margins.left;
      const cs = (d?.clips ?? []).filter((k) => k.lane === "kick-1.hits" && k.w > 0 && k.x > 0 && lo + k.x + k.w < b.right);
      if (cs.length < 2) return { beat: NaN, ppb: NaN };
      const [p, q] = [cs[0], cs.at(-1)];
      const at = (k) => lo + k.x + k.w / 2;
      const ppb = (at(q) - at(p)) / (q.start + q.length / 2 - (p.start + p.length / 2));
      return { beat: p.start + p.length / 2 + (x - at(p)) / ppb, ppb };
    },
    tl,
    x,
  );
/** Every clip drawn, by its lane and id: its left in the timeline's box and its width. */
const clipBoxes = () =>
  page.$$eval(
    `${tl} [data-clip]`,
    (els, tl) => {
      const b = document.querySelector(tl).getBoundingClientRect();
      return Object.fromEntries(els.map((e) => [`${e.closest("[data-lane]")?.dataset.lane}/${e.dataset.clip}`, [e.getBoundingClientRect().left - b.left, e.getBoundingClientRect().width]]));
    },
    tl,
  );
/**
 * The beat under screen x, from the middles of two of Kick 1's clips drawn in view: as true of a view
 * stretched by a live zoom as of one laid out (a clip's 1 px inset stretches with it; its middle not).
 */
const beatMid = (x) =>
  page.$eval(
    tl,
    (e, x) => {
      const b = e.getBoundingClientRect();
      const cs = [...e.querySelectorAll('[data-track="kick-1"] [data-clip]')]
        .map((d) => ({ mid: Number(d.dataset.start) + Number(d.dataset.length) / 2, r: d.getBoundingClientRect() }))
        .filter((d) => d.r.width > 0 && d.r.left > b.left + 240 && d.r.right < b.right);
      if (cs.length < 2) return { beat: NaN, ppb: NaN };
      const [p, q] = [cs[0], cs.at(-1)];
      const at = (d) => d.r.left + d.r.width / 2;
      const ppb = (at(q) - at(p)) / (q.mid - p.mid);
      return { beat: p.mid + (x - at(p)) / ppb, ppb };
    },
    x,
  );
{
  const at = await lanes();
  // At 24 px a beat (a pinch laid out at once, as it all comes in one task), 1200 px in.
  const z = (await arr())?.zoom ?? 24;
  if (z !== 24) await pinch(at.x, at.y, [24 / z]);
  await frames();
  await setScroll(tl, 1200, 0);
  await frames();
  const first = Math.ceil(1200 / 24);
  // In view: a loop, the playhead, a marker and bars selected along the ruler's top band.
  await api("/loop", { method: "POST", body: json({ start: first + 8, end: first + 16, on: true }) });
  await api("/seek", { method: "POST", body: json({ beat: first + 20 }) });
  const marker = { name: "Live", beat: first + 24 };
  await api("/markers", { method: "POST", body: json({ session: (await state()).session, markers: [marker] }) });
  const ruler = await box("[data-testid=ruler]");
  await page.mouse.move(ruler.x + (first + 28.3) * 24, ruler.y + 7);
  await page.mouse.down();
  await page.mouse.move(ruler.x + (first + 33) * 24, ruler.y + 7, { steps: 4 });
  await page.mouse.move(ruler.x + (first + 35.6) * 24, ruler.y + 7, { steps: 4 });
  await page.mouse.up();
  await until(async () => (await has("[data-testid=loop-shade]")) && (await has("[data-testid=section]")) && (await has('[data-marker-name="Live"]')), 5000);
  await page.mouse.move(at.x, at.y);
  await sleep(400);
  const before = await overlays(marker);
  await page.evaluate((loop, bars) => ((window.__loop = loop), (window.__bars = bars)), [first + 8, first + 16], await page.$eval("[data-testid=section]", (e) => [Number(e.dataset.start), Number(e.dataset.end)]));
  const a = await beatMid(at.x);
  const boxes0 = await clipBoxes();
  const r0 = await renders();
  const stop = await watchFrames("arrangement", tl, "[data-clip]", { hold: { x: at.x, kind: "clips" }, canvases: true });
  await pinchOver(at.x, at.y, [1.1, 1.2, 1.3, 1.4, 1.5], false);
  await frames(2);
  const r = since(r0, await renders());
  const mid = await arr();
  const st = await stretched("arrangement");
  const pic = await page.evaluate(() => window.m2canvas.camera.picture?.("arrangement"));
  const b = await beatDrawn(at.x);
  const boxes1 = await clipBoxes();
  const kept = Object.keys(boxes0).filter((k) => json(boxes0[k]) === json(boxes1[k])).length;
  check(
    Object.keys(r).length === 0 && mid?.laid?.zoom === 24 && mid?.zoom === 36 && !!pic?.shown && pic.drawn?.zoom === 36 && st.every((s) => !s.transform) && kept === Object.keys(boxes0).length && kept > 50 && near(b.ppb, 36, 0.05) && near(b.beat * 36, a.beat * 36, 1),
    `mid-pinch the arrangement's picture shows, painted at the zoom shown (${pic?.drawn?.zoom}); the page under it is as it was laid out: nothing stretched (${st.filter((s) => s.transform).length} transforms), ${kept} of ${Object.keys(boxes0).length} clip boxes unmoved, nothing rendered (${json(r)}); beat ${a.beat.toFixed(3)} still under the fingers in the picture (${Number(b.beat).toFixed(3)}); its camera says the zoom shown, ${mid?.zoom}, laid out at ${mid?.laid?.zoom}`,
  );
  const o = await drawnOverlays(marker);
  check(
    Object.values(before).every((v) => v !== null && v <= 1) && !!o && Object.values(o).every((v) => v !== null && v <= 1),
    `and what the picture draws over the tracks lines up with it, each within a px of where the zoom shown puts it: ${json(o)} (the page's before: ${json(before)})`,
  );
  // Agents see the zoom shown too (their `seen` is sent 200 ms after a change).
  await sleep(500);
  const said = (await (await fetch(`${sess.url}/api/focus`, { headers: { "x-m2-token": sess.token } })).json()).seen;
  const shown = await arr();
  const want = shown && [shown.x / shown.zoom, (shown.x + shown.view.w - shown.margins.left) / shown.zoom].map((v) => Math.round(v * 1000) / 1000);
  check(!!shown?.laid && json(said) === json(want), `agents' seen says what the zoom shown shows: ${json(said)} (${json(want)})`);
  await pinchEnd(at.x, at.y, 1.5);
  await laidOut("arrangement");
  await frames(3);
  const fs = await stop();
  const e = since(r0, await renders());
  check(
    drawnFrames(fs) && e.arrangement === 1 && (await page.evaluate(() => window.m2canvas.camera.picture?.("arrangement")?.shown)) === false,
    `its gestureend lays it out once (${json(e)}), the picture hidden in that frame: every frame, the picture (P) or laid out (L), showed each clip within a px of where the zoom shown put it, the page under the picture where the zoom laid out put it (^), and the beat under the fingers within a px of where it was at the start (@) (${brief(fs)})`,
  );
  // The same pinch laid out at once (all its events in one task): the same boxes.
  await sleep(500);
  const live = { boxes: await clipBoxes(), at: (await scroll(tl)).x, cam: await arr() };
  await pinch(at.x, at.y, [1 / 1.5]);
  await frames();
  await setScroll(tl, 1200, 0);
  await frames();
  await sleep(300);
  await pinch(at.x, at.y, [1.5]);
  await frames();
  await sleep(500);
  const direct = { boxes: await clipBoxes(), at: (await scroll(tl)).x, cam: await arr() };
  const both = Object.keys(live.boxes).filter((k) => k in direct.boxes);
  const worst = Math.max(...both.map((k) => Math.max(Math.abs(live.boxes[k][0] - direct.boxes[k][0]), Math.abs(live.boxes[k][1] - direct.boxes[k][1]))));
  check(
    both.length > 50 && worst <= 0.5 && live.at === direct.at && live.cam?.zoom === direct.cam?.zoom,
    `laid out, it is what a direct zoom to the same zoom and place draws: ${both.length} clip boxes within ${Math.round(worst * 100) / 100} px, scrolled ${live.at} and ${direct.at}, at ${live.cam?.zoom} and ${direct.cam?.zoom} px a beat`,
  );
  await api("/loop", { method: "POST", body: json({ start: first + 8, end: first + 16, on: false }) });
  await page.keyboard.press("Escape");
}
// Never laid out before the end (VIEW-5): a pinch out to a quarter, far past what the page's window
// holds, is drawn whole by the picture in every frame, the page under it laid out only at its end;
// so is a pinch in as far.
{
  await sleep(300);
  const at = await lanes();
  const z0 = (await arr())?.zoom;
  const out = await watchFrames("arrangement", tl, "[data-clip]", { hold: { x: at.x, kind: "clips" }, canvases: true });
  await pinchOver(at.x, at.y, Array.from({ length: 24 }, (_, i) => 1 - (0.75 * (i + 1)) / 24), false);
  await frames(3);
  await pinchEnd(at.x, at.y, 0.25);
  await laidOut("arrangement");
  await frames(3);
  const fo = await out();
  // The zooms the frames under the picture were laid out at: the start's alone.
  const laidOut0 = [...new Set(fo.filter((f) => f.live).map((f) => f.laid))];
  const zEnd = (await arr())?.zoom ?? 0;
  check(
    json(laidOut0) === json([z0]) && zEnd < (z0 ?? 0) * 0.3 && drawnFrames(fo, { full: true }),
    `a pinch out to a quarter (${z0} → ${zEnd.toFixed(2)} px a beat) is never laid out before its end: under the picture, laid out at ${json(laidOut0)} alone; every frame drawn across the whole view, each clip within a px of the zoom shown, the beat under the fingers held (@), nothing stretched, nothing rendered but at the end (${brief(fo)})`,
  );
  await sleep(300);
  const z1 = (await arr())?.zoom;
  const inn = await watchFrames("arrangement", tl, "[data-clip]", { hold: { x: at.x, kind: "clips" }, canvases: true });
  await pinchOver(at.x, at.y, Array.from({ length: 24 }, (_, i) => 1 + (3 * (i + 1)) / 24));
  await laidOut("arrangement");
  await frames(3);
  const fi = await inn();
  const laidIn = new Set(fi.filter((f) => f.live).map((f) => f.laid));
  check(
    laidIn.size === 1 && [...laidIn][0] === z1 && drawnFrames(fi, { full: true }),
    `a pinch in ×4 is laid out only at its end: under the picture, laid out at ${json([...laidIn])} (${z1} before) (${brief(fi)})`,
  );
}
// A pinch out at the song's start, and back in (VIEW-5, VIEW-6): the picture stops at the start, as
// the page does, never showing past it; so the way back in starts from what showed, the clips in
// every frame where the zoom shown puts them, and the camera never looks before the start.
{
  await sleep(300);
  await setScroll(tl, 0, null);
  await frames(2);
  await sleep(300);
  const at = await lanes();
  const z0 = (await arr())?.zoom ?? 0;
  const xs = [];
  const watch = await watchFrames("arrangement", tl, "[data-clip]");
  const seen = page.evaluate(
    () =>
      new Promise((done) => {
        const xs = [];
        const one = () => {
          xs.push(window.m2canvas.camera.now("arrangement").x);
          if (xs.length < 90) requestAnimationFrame(one);
          else done(xs);
        };
        requestAnimationFrame(one);
      }),
  );
  await pinchOver(at.x, at.y, [...Array.from({ length: 12 }, (_, i) => 1 - (0.5 * (i + 1)) / 12), ...Array.from({ length: 12 }, (_, i) => 0.5 + (0.5 * (i + 1)) / 12)]);
  await laidOut("arrangement");
  xs.push(...(await seen));
  await frames(3);
  const fs = await watch();
  const end = await arr();
  check(
    drawnFrames(fs) && Math.min(...xs) >= 0 && near(end?.zoom ?? 0, z0, 0.01),
    `a pinch out at the song's start and back in stops at the start, as the page does: the camera never before it (its least x ${Math.min(...xs).toFixed(2)}), each clip in every frame where the zoom shown puts it (${brief(fs)})`,
  );
}
// A pinch out at the song's end and back in to the zoom it began at (VIEW-5, VIEW-6): back in, the
// zoom looks further right than the song's room went at that zoom, and the view keeps room where it
// looked, so its end shows what the last picture did: the beat under the fingers held in every frame,
// the end's too.
{
  await sleep(300);
  const s0 = await scroll(tl);
  await setScroll(tl, s0.room.x, null);
  await frames(2);
  await sleep(300);
  const at = await lanes();
  const z0 = (await arr())?.zoom ?? 0;
  const watch = await watchFrames("arrangement", tl, "[data-clip]", { hold: { x: at.x, kind: "clips" } });
  await pinchOver(at.x, at.y, [...Array.from({ length: 12 }, (_, i) => 1 - (0.5 * (i + 1)) / 12), ...Array.from({ length: 12 }, (_, i) => 0.5 + (0.5 * (i + 1)) / 12)]);
  await laidOut("arrangement");
  await frames(3);
  const fs = await watch();
  const end = await arr();
  check(
    drawnFrames(fs) && near(end?.zoom ?? 0, z0, 0.01),
    `a pinch out at the song's end and back in to its zoom (${z0}) ends where its last picture showed, the view keeping room where the zoom looked: each clip in every frame where the zoom shown puts it, the beat under the fingers held (${brief(fs)})`,
  );
}
// A press while the picture shows ends the zoom and does nothing else (VIEW-5), as a touch stops a
// glide: the zoom is laid out at the zoom shown, the picture hides, and the clip shown under the press
// isn't picked, the playhead doesn't move, nothing is written. A press after it, on the page laid
// out, picks that clip.
{
  await sleep(300);
  const at = await lanes();
  await pinchOver(at.x, at.y, [1.1, 1.2, 1.3], false);
  await frames(2);
  const mid = await arr();
  const target = await page.evaluate((tl) => {
    const face = window.m2canvas.camera;
    const c = face.now("arrangement"), d = face.picture?.("arrangement")?.drawn;
    const e = document.querySelector(tl), b = e.getBoundingClientRect();
    const lo = b.left + e.clientLeft + c.margins.left;
    const row = e.querySelector('[data-track="kick-1"] [data-row="first"]')?.getBoundingClientRect();
    const k = (d?.clips ?? []).find((k) => k.lane === "kick-1.hits" && lo + k.x > b.left + 400 && lo + k.x + k.w < b.right - 100 && k.w > 20);
    return k && row ? { id: k.id, x: lo + k.x + k.w / 2, y: row.top + row.height / 2 } : null;
  }, tl);
  const beat0 = (await api("/status")).body.beat;
  const posts0 = posts.length;
  if (target) await page.mouse.click(target.x, target.y);
  await frames(2);
  const after = await arr();
  const shown = await page.evaluate(() => window.m2canvas.camera.picture?.("arrangement")?.shown);
  const picked = await page.$$eval(`${tl} [data-clip][data-picked]`, (els) => els.map((e) => e.dataset.clip));
  const beat1 = (await api("/status")).body.beat;
  const wrote = posts.slice(posts0).map((p) => p.url);
  check(
    !!mid?.laid && !!target && after?.laid === null && near(after.zoom, mid.zoom, 0.001) && shown === false && picked.length === 0 && beat1 === beat0 && wrote.length === 0,
    `a press while the picture shows ends the zoom, laid out at the zoom shown (${mid?.zoom?.toFixed(2)}, laid out at ${after?.zoom?.toFixed(2)}, the picture ${shown ? "showing" : "hidden"}), and does nothing else: the clip shown under it (${target?.id}) not picked (${json(picked)}), the playhead still at ${beat1} (${beat0}), nothing written (${json(wrote)})`,
  );
  await pinchEnd(at.x, at.y, 1.3);
  await frames(2);
  const again = target && (await page.evaluate((tl, id) => {
    const el = document.querySelector(`${tl} [data-track="kick-1"] [data-clip="${id}"]`)?.getBoundingClientRect();
    return el ? { x: el.left + el.width / 2, y: el.top + el.height / 2 } : null;
  }, tl, target.id));
  if (again) await page.mouse.click(again.x, again.y);
  await frames(2);
  const picked2 = await page.$$eval(`${tl} [data-clip][data-picked]`, (els) => els.map((e) => e.dataset.clip));
  check(!!again && json(picked2) === json([target.id]), `the next press, on the page laid out, picks that clip: ${json(picked2)}`);
  await page.keyboard.press("Escape");
}
// ⌘-scroll over time: the picture until no ⌘-scroll for G, then laid out once.
{
  await sleep(300);
  const at = await lanes();
  const r0 = await renders();
  const stop = await watchFrames("arrangement", tl, "[data-clip]", { hold: { x: at.x, kind: "clips" }, canvases: true });
  const t = await page.evaluate(
    (x, y) =>
      new Promise((done) => {
        const face = window.m2canvas.camera;
        const send = () => (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(new WheelEvent("wheel", { deltaY: -3, clientX: x, clientY: y, metaKey: true, bubbles: true, cancelable: true }));
        let k = 0;
        const one = () => {
          send();
          if (++k < 12) return requestAnimationFrame(one);
          // Read in the same task as the last event, so no frame or timer comes between: the view
          // stretched by the frames before it, and nothing rendered. Then when it is laid out.
          const last = performance.now();
          const pic = face.picture?.("arrangement");
          const mid = { laid: face.now("arrangement")?.laid ?? null, renders: window.m2canvas.frames().total, picture: !!pic?.shown && pic.drawn?.zoom === face.now("arrangement")?.zoom };
          const wait = () => {
            const end = performance.now() - last;
            if (face.now("arrangement")?.laid === null || end > 3000) done({ mid, end });
            else requestAnimationFrame(wait);
          };
          requestAnimationFrame(wait);
        };
        requestAnimationFrame(one);
      }),
    at.x,
    at.y,
  );
  await frames(3);
  const fs = await stop();
  const e = since(r0, await renders());
  const G = (await page.evaluate(() => window.m2canvas.camera.numbers.SWIPE_GAP)) ?? 50;
  const quiet = since(r0, t.mid.renders);
  check(
    !!t.mid.laid && t.mid.picture && Object.keys(quiet).length === 0 && t.end >= G - 1 && t.end <= G + 1000,
    `⌘-scroll shows the picture at its last event, painted at the zoom shown (${t.mid.picture}; laid out at ${t.mid.laid?.zoom}), nothing rendered (${json(quiet)}), and is laid out ${Math.round(t.end)} ms after it: never before G (${G} ms)`,
  );
  check(drawnFrames(fs) && e.arrangement === 1, `laid out once (${json(e)}), every frame showing each clip within a px of the zoom shown, the beat under the pointer held (@) (${brief(fs)})`);
  // A run of ⌘-scroll with the page busy 80 ms in its middle, an event waiting behind it (VIEW-5):
  // its end is counted in the frames after its last event, each at most G/2, never by a timer, so
  // the late frame never ends it early.
  {
    const G = await page.evaluate(() => window.m2canvas.camera.numbers.SWIPE_GAP);
    await laidOut("arrangement");
    await sleep(300);
    const z0 = (await cam("arrangement"))?.zoom;
    const l = await zoomLate("arrangement", at.x, at.y, 12, -3, 5);
    await frames(2);
    // Back to the zoom it had, laid out at once (all in one task), for what comes after.
    await laidOut("arrangement");
    const z1 = (await cam("arrangement"))?.zoom;
    if (z0 && z1) await pinch(at.x, at.y, [z0 / z1]);
    await frames(2);
    check(
      l.ended.length === 0 && l.end >= G - 1 && l.end <= G + 1000,
      `the arrangement: a run of 12 ⌘-scroll events, one a frame, the page busy 80 ms after the fifth and the sixth heard after a timer due 60 ms on, is never laid out before its last event (laid out before events ${json(l.ended)}), and laid out ${l.end} ms after it (G ${G} ms)`,
    );
  }
}
// The browser's own move of the box while the picture shows (VIEW-9): a write to scrollLeft, as a
// scrollbar's drag or a key's scroll makes, is taken in from where the picture shows the view, as
// with no zoom by the hand: the zoom shown is laid out, 120 px on.
{
  await sleep(300);
  const at = await lanes();
  await pinchOver(at.x, at.y, [1.1, 1.2, 1.3], false);
  await frames(2);
  const mid = await arr();
  const s0 = await scroll(tl);
  await setScroll(tl, s0.x + 120, null);
  await frames(3);
  const c = await arr();
  const s1 = await scroll(tl);
  check(
    !!mid?.laid && c?.laid === null && near(c.zoom, mid.zoom, 0.001) && near(c.x, mid.x + 120, 1) && near(s1.x, Math.round(mid.x + 120), 1),
    `a write to scrollLeft while the picture shows moves the view 120 px from where the picture showed it, laid out at the zoom shown: ${json({ shown: mid && [mid.x, mid.zoom], laid: mid?.laid, now: c && [c.x, c.zoom], box: [s0.x, s1.x] })}`,
  );
  await pinchEnd(at.x, at.y, 1.3);
  // The zoom as it was, for what comes after.
  await laidOut("arrangement");
  await pinch(at.x, at.y, [1 / 1.3]);
  await frames(2);
}
// A picture that can't paint (it throws: a browser without a canvas feature it uses) never freezes the
// view (VIEW-5): the zoom is laid out each frame instead, the clips where the zoom shown puts them in
// every frame, and the picture keeps why, for the swipe probe's report.
{
  await sleep(300);
  const at = await lanes();
  const z0 = (await arr())?.zoom ?? 0;
  await page.evaluate(() => window.m2canvas.camera.pictureFails?.("arrangement", true));
  const stop = await watchFrames("arrangement", tl, "[data-clip]", { hold: { x: at.x, kind: "clips" } });
  await pinchOver(at.x, at.y, [1.1, 1.2, 1.3, 1.4]);
  await laidOut("arrangement");
  await frames(3);
  const fs = await stop();
  const pic = await page.evaluate(() => window.m2canvas.camera.picture?.("arrangement"));
  await page.evaluate(() => window.m2canvas.camera.pictureFails?.("arrangement", false));
  const z1 = (await arr())?.zoom ?? 0;
  const zooms = new Set(fs.map((f) => f.zoom));
  check(
    near(z1, z0 * 1.4, 0.01) && fs.length > 3 && zooms.size >= 4 && fs.every((f) => f.shown === false && !f.live && f.n > 0 && f.worst <= 1) && pointHeld(fs) && /throws/.test(pic?.failed?.why ?? ""),
    `a picture that can't paint (it throws) never freezes the view: every frame laid out at the zoom shown (${zooms.size} zooms, ${z0} → ${z1.toFixed(2)}), each clip where it puts it, the beat under the fingers held (${brief(fs)}); the picture says why: ${json(pic?.failed?.why)}`,
  );
  await pinch(at.x, at.y, [z0 / z1]);
  await frames(2);
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
  // The swipe (10 events of 10 px, 16 ms apart), then its next 10 frames: the third runs 100 ms
  // long, so the fourth comes late, as frames do on a busy machine (GitHub's Mac).
  const seen = (await feed(at.x, at.y, swipe(10, 10, 0), 0, { name: "arrangement", frames: 10, late: 3 })) ?? [];
  // VIEW-4, by the clock: from the frame that took the swipe's last event (its 100 px), at the speed
  // of its last 50 ms (30 px in 48 ms), keeping 0.998 of it each ms. So t ms after that frame the
  // camera is at 100 + v (1 − 0.998^t) / −ln 0.998, however late its frames come, and each frame's
  // speed (its move over its time) is less than the one before.
  const v = 30 / 48, k = -Math.log(0.998), t0 = seen[0]?.t ?? 0;
  const off = Math.max(...seen.map((s) => Math.abs((s.x ?? 0) - (100 + (v * (1 - 0.998 ** (s.t - t0))) / k))));
  const speeds = seen.slice(1).map((s, i) => (s.x - seen[i].x) / (s.t - seen[i].t));
  const slowing = speeds.every((s, i) => s > 0 && (i === 0 || s < speeds[i - 1]));
  check(
    seen.length === 10 && seen[0].x === 100 && seen.at(-1).x > 100 + 20 && slowing && off < 0.5,
    `with Linux's flag, a swipe's glide carries on after it, slowing by the clock, a late frame too: ${off.toFixed(2)} px from where the clock puts it (${seen.map((s) => `${(s.t - t0).toFixed(0)} ms ${s.x?.toFixed(1)}`).join(", ")})`,
  );
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
  await laidOut("notes");
  await frames();
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
  // Read from what the grid draws (its `data-ppb`), so once the live zoom is laid out (VIEW-5).
  await laidOut("notes");
  await frames();
  const b = await rollBeat(at.x);
  check(b.ppb > a.ppb * 1.4 && near(b.beat * b.ppb, a.beat * b.ppb, 1.5), `Notes: ⌘-scroll zooms around the pointer, beat ${a.beat.toFixed(3)} staying (${b.beat.toFixed(3)}, ${a.ppb.toFixed(1)} → ${b.ppb.toFixed(1)})`);
  await pinch(at.x, at.y, [0.9, 0.75]);
  await frames();
  const p = await rollBeat(at.x);
  check(near(p.ppb, b.ppb * 0.75, 0.5) && near(p.beat * p.ppb, a.beat * p.ppb, 1.5), `and a pinch out, around it too (${p.beat.toFixed(3)}, ${p.ppb.toFixed(1)} px a beat)`);
}
// Notes' live zoom (VIEW-5, VIEW-8): mid-pinch its grid and ruler are stretched and nothing renders;
// its notes, and their velocity bars under the roll, sit where the zoom shown puts them; its
// gestureend lays it out once.
{
  const r = await box(roll);
  const at = { x: r.x + r.width * 0.5, y: r.y + r.height * 0.5 };
  await laidOut("notes");
  await sleep(300);
  const z0 = (await cam("notes"))?.zoom;
  const r0 = await renders();
  const stop = await watchFrames("notes", roll, "[data-note]", { hold: { x: at.x, kind: "notes" } });
  await pinchOver(at.x, at.y, [1.1, 1.2, 1.3], false);
  await frames(2);
  const q = since(r0, await renders());
  const mid = await cam("notes");
  const st = await stretched("notes");
  const lined = await page.evaluate((roll) => {
    const c = window.m2canvas.camera.now("notes");
    const box = document.querySelector(roll), b = box.getBoundingClientRect();
    const s = c.zoom / (c.laid?.zoom ?? c.zoom);
    let worst = 0, n = 0;
    for (const e of box.querySelectorAll("[data-note]")) {
      const r = e.getBoundingClientRect();
      const bar = document.querySelector(`[data-testid=velocity] [data-velocity="${e.dataset.note}"]`);
      if (!bar || r.left < b.left + c.margins.left || r.right > b.right) continue;
      const want = b.left + box.clientLeft + c.origin.x - Math.round(c.x) + Number(e.dataset.start) * c.zoom;
      // A velocity bar's box starts 5 px before its note, at the zoom drawn: stretched, 5 px times the stretch.
      worst = Math.max(worst, Math.abs(r.left - want), Math.abs(bar.getBoundingClientRect().left + 5 * s - want));
      n++;
    }
    return { n, worst: Math.round(worst * 100) / 100 };
  }, roll);
  check(
    Object.keys(q).length === 0 && !!z0 && mid?.laid?.zoom === z0 && near(mid.zoom, z0 * 1.3, 0.01) && st.length > 0 && st.every((s) => s.transform.includes("scale")) && lined.n > 0 && lined.worst <= 1,
    `Notes mid-pinch: stretched (${st.length} layers), nothing rendered (${json(q)}), ${lined.n} notes and their velocity bars within ${lined.worst} px of where the zoom shown puts them (${mid?.zoom?.toFixed(2)} px a beat, laid out at ${mid?.laid?.zoom?.toFixed(2)})`,
  );
  await pinchEnd(at.x, at.y, 1.3);
  await laidOut("notes");
  await frames(3);
  const fs = await stop();
  const e = since(r0, await renders());
  check(
    liveFrames(fs) && e.notes === 1 && (await stretched("notes")).every((s) => !s.transform.includes("scale")),
    `its gestureend lays Notes out once (${json(e)}), the stretch gone in that frame, every frame showing each note within a px of the zoom shown, the beat under the fingers held (@) (${brief(fs)})`,
  );
  // A run of ⌘-scroll with the page busy 80 ms in its middle, an event waiting behind it (VIEW-5):
  // its end is counted in the frames after its last event, each at most G/2, never by a timer, so
  // the late frame never ends it early.
  {
    const G = await page.evaluate(() => window.m2canvas.camera.numbers.SWIPE_GAP);
    await laidOut("notes");
    await sleep(300);
    const z0 = (await cam("notes"))?.zoom;
    const l = await zoomLate("notes", at.x, at.y, 12, -3, 5);
    await frames(2);
    // Back to the zoom it had, laid out at once (all in one task), for what comes after.
    await laidOut("notes");
    const z1 = (await cam("notes"))?.zoom;
    if (z0 && z1) await pinch(at.x, at.y, [z0 / z1]);
    await frames(2);
    check(
      l.ended.length === 0 && l.end >= G - 1 && l.end <= G + 1000,
      `Notes: a run of 12 ⌘-scroll events, one a frame, the page busy 80 ms after the fifth and the sixth heard after a timer due 60 ms on, is never laid out before its last event (laid out before events ${json(l.ended)}), and laid out ${l.end} ms after it (G ${G} ms)`,
    );
  }
}
/** Open Bass 1's clip `clip` in Notes, by a double-click in the timeline; its zoom as it opens. */
const openBass = async (clip) => {
  const c = await box(`${tl} [data-track="bass-1"] [data-clip="${clip.id}"]`);
  await page.mouse.click(c.x + c.width / 2, c.y + c.height / 2, { clickCount: 2 });
  await until(async () => (await page.$$eval(`${roll} [data-note]`, (els) => els.length)) === clip.notes.length && (await page.$eval("[data-testid=roll-grid]", (e) => e.dataset.ppb).catch(() => null)) !== null, 10000);
  await laidOut("notes");
  await sleep(400);
  return (await cam("notes"))?.zoom;
};
// Notes on a clip of one bar, as it opens: fitted, its content no wider than the view (VIEW-5,
// VIEW-6). A pinch in ×1.6, held at the middle and near the right, keeps the beat under the fingers
// in every frame, the end's too: the roll keeps room where the zoom looked. Then a pinch out shows
// past the content's end at once: laid out each frame, never a blank strip past what is drawn.
{
  const fit = await openBass(bassClips.bar);
  const r = await box(roll);
  // Back to its fitted zoom around the clear view's left edge, laid out at once: from the start, so the
  // room a zoom looked at is the view's own.
  const refit = async () => {
    await setScroll(roll, 0, null);
    await frames(2);
    const c0 = await cam("notes");
    if (c0 && c0.zoom !== fit) await pinch(Math.round(r.x + c0.margins.left + 1), r.y + r.height / 2, [fit / c0.zoom]);
  };
  for (const fx of [0.5, 0.9, 0.97]) {
    await refit();
    await frames(2);
    await setScroll(roll, 0, null);
    await frames(2);
    await sleep(300);
    const c = await cam("notes");
    const room = (await scroll(roll)).room.x;
    const at = { x: Math.round(r.x + c.margins.left + (c.view.w - c.margins.left) * fx), y: Math.round(r.y + r.height / 2) };
    const stop = await watchFrames("notes", roll, "[data-note]", { hold: { x: at.x, kind: "notes" } });
    await pinchOver(at.x, at.y, [1.1, 1.2, 1.3, 1.4, 1.5, 1.6]);
    await laidOut("notes");
    await frames(3);
    const fs = await stop();
    check(
      near(c?.zoom ?? 0, fit, 0.001) && room < 100 && liveFrames(fs),
      `Notes on a bar fitted (${fit} px a beat, ${room} px of room): a pinch in ×1.6 at ${fx} across keeps the beat under the fingers in every frame, the end's too (${brief(fs)})`,
    );
  }
  await refit();
  await frames(2);
  await setScroll(roll, 0, null);
  await frames(2);
  await sleep(300);
  const room = (await scroll(roll)).room.x;
  const at = { x: Math.round(r.x + r.width * 0.5), y: Math.round(r.y + r.height / 2) };
  const stop = await watchFrames("notes", roll, "[data-note]", { fill: "[data-testid=roll-grid]" });
  await pinchOver(at.x, at.y, [0.9, 0.8, 0.7, 0.6]);
  await laidOut("notes");
  await frames(3);
  const fs = await stop();
  check(
    room < 100 && fs.length > 4 && fs.every((f) => f.covers && !f.live),
    `with its content as wide as the view (${room} px of room), a pinch out shows past the content's end, so it is laid out each frame: the grid reaches the view's right edge in every frame, none stretched (${brief(fs)})`,
  );
}
// Notes' hint on an empty clip follows the zoom shown (VIEW-5): mid-pinch it is where the zoom's end
// puts it, so it doesn't jump when the zoom is laid out.
{
  await openBass(bassClips.empty);
  const r = await box(roll);
  const at = { x: Math.round(r.x + r.width * 0.5), y: Math.round(r.y + r.height / 2) };
  await pinchOver(at.x, at.y, [1.1, 1.2, 1.3], false);
  await frames(2);
  const mid = await page.evaluate(
    (roll, length) => {
      const c = window.m2canvas.camera.now("notes");
      const hint = [...document.querySelectorAll("[data-testid=notes] *")].find((e) => /to add a note/.test(e.textContent ?? "") && e.children.length === 0)?.closest(".absolute");
      const b = document.querySelector(roll).getBoundingClientRect();
      if (!hint || !c) return null;
      const h = hint.getBoundingClientRect();
      return { laid: !!c.laid, at: Math.round((h.left + h.width / 2 - b.left) * 10) / 10, want: Math.round((c.margins.left + Math.min(length * c.zoom, c.view.w - c.margins.left) / 2) * 10) / 10 };
    },
    roll,
    bassClips.empty.length,
  );
  await pinchEnd(at.x, at.y, 1.3);
  await laidOut("notes");
  check(!!mid?.laid && near(mid.at, mid.want, 1), `mid-pinch, the hint on an empty clip is where the zoom shown puts it: ${json(mid)}`);
}

// ================================================================ clips laid over each other (VIEW-12)
// A copy of first-song whose Bass has clips over each other, as a drag or a MIDI file leaves them:
// the later clip covers the earlier one, its notes, its curve and the curve's points, as each box
// covered the one before it when each clip drew its own SVG. Then one file changed outside the app
// draws only its own lane again.
{
  const ovl = path.join(ROOT, "ovl");
  fs.cpSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "../examples/first-song"), ovl, { recursive: true });
  const notesFile = path.join(ovl, "data/bass/notes.json");
  const bassNotes = JSON.parse(fs.readFileSync(notesFile, "utf8"));
  const first = bassNotes.clips[0];
  // A: beats 0 to 8; B: 4 to 12, a note at its start and at beats 10.5 and 11.25 of the song; C: 16 to 24.
  const a = { ...first, length: 8, notes: first.notes.filter((n) => n.start < 8) };
  const b = { id: "c0000001", start: 4, length: 8, notes: [0, 6.5, 7.25].map((t) => ({ pitch: "C3", start: t, length: 0.25, velocity: 0.9 })) };
  const c = { id: "c0000002", start: 16, length: 8, notes: first.notes.filter((n) => n.start < 8) };
  fs.writeFileSync(notesFile, JSON.stringify({ ...bassNotes, clips: [a, b, c] }));
  // The cutoff: E from 0 to 16 through 1 kHz at beat 8; L from 6 to 10, flat at 5 kHz, over E's point.
  const cutFile = path.join(ovl, "data/bass/cutoff.json");
  const cut = JSON.parse(fs.readFileSync(cutFile, "utf8"));
  const e = { ...cut.clips[0], start: 0, length: 16, points: [[0, 300], [8, 1000], [16, 2200]] };
  const l = { id: "c0000003", start: 6, length: 4, points: [[0, 5000], [4, 5000]] };
  fs.writeFileSync(cutFile, JSON.stringify({ ...cut, clips: [e, l] }));
  const opened = (await api("/open", { method: "POST", body: json({ path: ovl }) })).status === 200;
  await until(async () => (await page.$$('[data-lane="bass.notes"] [data-clip]')).length === 3, 30000);
  await setScroll(tl, 0, 0);
  if (!(await has('[data-lane="bass.cutoff"] [data-clip]'))) await page.click('[data-fold="bass"]');
  await until(async () => (await page.$$('[data-lane="bass.cutoff"] [data-clip]')).length === 2, 10000);
  await sleep(500);
  await frames(3);

  /** The screen's colour at (x, y) as a picture of the page shows it: [r, g, b]. */
  const screenAt = async (x, y) => {
    const shot = await page.screenshot({ encoding: "base64", clip: { x: Math.floor(x), y: Math.floor(y), width: 1, height: 1 } });
    return page.evaluate(async (shot) => {
      const img = new Image();
      img.src = `data:image/png;base64,${shot}`;
      await img.decode();
      const cv = document.createElement("canvas");
      cv.width = img.width;
      cv.height = img.height;
      const g = cv.getContext("2d");
      g.drawImage(img, 0, 0);
      return [...g.getImageData(0, 0, 1, 1).data].slice(0, 3);
    }, shot);
  };
  const same = (p, q) => p.every((v, i) => Math.abs(v - q[i]) <= 6);
  const boxOf = (sel) => page.$eval(sel, (x) => x.getBoundingClientRect().toJSON());
  /**
   * Where `under`'s notes (or its curve's line and points) lie inside `over`'s box, clear of
   * `over`'s own notes: each must show only `over`: its body, as its colour in the strip above its
   * drawing, under the shade of its curve (its ink at 0.1) below the curve's line.
   */
  const covered = async (lane, under, over) => {
    const sel = (start) => `[data-lane="${lane}"] [data-clip][data-start="${start}"]`;
    const [du] = await page.$$eval(sel(under), drawings);
    const [dv] = await page.$$eval(sel(over), drawings);
    const ov = await boxOf(sel(over));
    const inside = (x) => x > ov.left + 2 && x < ov.right - 2;
    // Clear of what `over` draws itself there: its notes, and its curve's points (6 px dots).
    const dots = await page.$$eval(`${sel(over)} [data-point]`, (els) => els.map((p) => { const r = p.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; }));
    const clear = (x, y) => !(dv?.notes ?? []).some((n) => x >= n.x - 1 && x <= n.x + n.w + 1 && y >= n.y - 1 && y <= n.y + n.h + 1) && !dots.some(([px, py]) => Math.hypot(x - px, y - py) < 5);
    const spots = [];
    for (const n of du?.notes ?? []) {
      const [x, y] = [n.x + n.w / 2, n.y + n.h / 2];
      if (n.w >= 3 && inside(x) && clear(x, y)) spots.push({ what: "note", x, y });
    }
    const line = du?.line ?? [];
    for (let i = 1; i < line.length; i++) {
      const [[x0, y0], [x1, y1]] = [line[i - 1], line[i]];
      const x = x0 + (x1 - x0) * 0.75;
      if (inside(x) && clear(x, y0)) spots.push({ what: "line", x, y: y0 + (y1 - y0) * 0.75 });
    }
    for (const pt of await page.$$eval(`${sel(under)} [data-point]`, (els) => els.map((p) => { const r = p.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })))
      if (inside(pt[0]) && clear(pt[0], pt[1])) spots.push({ what: "point", x: pt[0], y: pt[1] });
    const lineY = dv?.line?.length ? dv.line[0][1] : null;
    const ink = dv ? rgb(dv.ink) : null;
    const wrong = [];
    for (const sp of spots) {
      const [seen, body] = [await screenAt(sp.x, sp.y), await screenAt(sp.x, ov.top + 2)];
      const want = lineY !== null && sp.y > lineY + 2 ? body.map((v, i) => Math.round(v * 0.9 + ink[i] * 0.1)) : body;
      if (!same(seen, want)) wrong.push({ ...sp, seen, want });
    }
    return { spots: spots.length, kinds: [...new Set(spots.map((sp) => sp.what))], wrong: wrong.slice(0, 3), wrongs: wrong.length };
  };
  const n1 = await covered("bass.notes", 0, 4);
  check(opened && n1.spots > 0 && n1.wrongs === 0, `a notes clip laid over the one before covers its notes, as its box did: ${json(n1)}`);
  const c1 = await covered("bass.cutoff", 0, 6);
  check(c1.kinds.includes("point") && c1.kinds.includes("line") && c1.wrongs === 0, `an automation clip laid over the one before covers its curve and its points, as the later one plays there: ${json(c1)}`);
  // A drag that drops C onto B: from beat 16 to 10, so C covers B's notes at beats 10.5 and 11.25.
  const cb = await boxOf('[data-lane="bass.notes"] [data-clip][data-start="16"]');
  const ppb = (await arr())?.zoom ?? 24;
  await page.mouse.move(cb.left + cb.width / 2, cb.top + cb.height / 2);
  await page.mouse.down();
  await page.mouse.move(cb.left + cb.width / 2 - 6 * ppb, cb.top + cb.height / 2, { steps: 8 });
  await page.mouse.up();
  const dropped = await until(() => JSON.parse(fs.readFileSync(notesFile, "utf8")).clips.some((x) => x.start === 10), 8000);
  await until(async () => !!(await page.$('[data-lane="bass.notes"] [data-clip][data-start="10"]')), 8000);
  await sleep(300);
  await frames(3);
  const d1 = await covered("bass.notes", 4, 10);
  check(dropped && d1.spots >= 2 && d1.wrongs === 0, `a clip dragged onto the next covers the notes it lands on: ${json(d1)}`);

  // One note's loudness changed in Kick's file, outside the app, as an agent would: Kick's lane's
  // canvas is drawn again, and no other lane's (VIEW-12: only when what it draws changes).
  const drawnN = () => page.$$eval(`${tl} [data-lane]`, (els) => Object.fromEntries(els.flatMap((x) => { const cv = x.querySelector("canvas[data-drawing=lane]"); return cv?.drawn ? [[x.dataset.lane, cv.drawn.n]] : []; })));
  const n0 = await drawnN();
  const kickFile = path.join(ovl, "data/kick/hits.json");
  const kick = JSON.parse(fs.readFileSync(kickFile, "utf8"));
  kick.clips[0].notes[3].velocity = kick.clips[0].notes[3].velocity === 0.5 ? 0.6 : 0.5;
  fs.writeFileSync(kickFile, JSON.stringify(kick));
  const want = kick.clips[0].notes[3].velocity;
  const heard = await until(async () => (await state()).timeline.tracks.find((t) => t.module === "kick")?.lanes.find((x) => x.key === "hits")?.clips[0].notes[3].velocity === want, 10000);
  await sleep(500);
  await frames(3);
  const nz = await drawnN();
  const again = Object.keys(n0).filter((k) => nz[k] !== n0[k]);
  check(heard && Object.keys(n0).length >= 4 && json(again) === json(["kick.hits"]), `one note's loudness changed in Kick's file draws Kick's lane's canvas again and none of the other ${Object.keys(n0).length - 1}: ${json({ again, before: n0, after: nz })}`);
}

check(read("song.ts").length > 0, "first-song's files are still there");
done(errors);
await browser.close();
