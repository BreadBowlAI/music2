// The zoom bench (taste.md VIEW-10; the zoom note, docs/designs/camera/zoom.md, §1 and §6 step 1):
// it replays a pinch or a run of ⌘-scroll over a point, as WebKit sends them, and measures each
// frame meanwhile, split into its parts. The swipe probe's last step loads it, and the browser
// suites and the WebKit tool load it through `window.m2canvas.bench()`. Nothing else in the app
// imports it, and it imports nothing of the app's. It uses nothing WebKit lacks.
//
// Each frame, in the page's own clock:
//   script  the frame's requestAnimationFrame callbacks, each with the microtasks it queued (React
//           renders and commits there a store's change made in a callback)
//   forced  of the script, the time its layout reads took (scrollWidth, getBoundingClientRect…):
//           style and layout forced inside it
//   layout  from the script's end to the bench's own ResizeObserver, which the browser calls once
//           its style and layout are done (resize observers the app made before the bench's are
//           called first, and count here)
//   paint   from there to a message posted in the frame's first callback, which runs once the
//           frame's rendering is done: paint, and handing the layers over (commit)
//   input   the events sent before the frame, each timed with finding what is under the point
// Its wrappers are on only while it replays.

/** A gesture to replay: a pinch's scales (each `gesturechange`'s, from its start), or ⌘-scroll's `deltaY`s in px. */
export type Gesture = { kind: "pinch"; scales: readonly number[] } | { kind: "scroll"; deltas: readonly number[] };

/** A pinch's scales: from 1 to each of `legs` in turn, `n` events each way, eased as fingers are. */
export function pinchScales(legs: readonly number[], n: number): number[] {
  const out: number[] = [];
  let from = 1;
  for (const to of legs) {
    for (let i = 1; i <= n; i++) out.push(from + (to - from) * (0.5 - 0.5 * Math.cos((Math.PI * i) / n)));
    from = to;
  }
  return out;
}

/** The owner's pinch over the tracks (the swipe probe's step A13, 2026-11-14): its scale from 1 to 0.44 and back, 55 changes each way. */
export const A13: readonly number[] = pinchScales([0.44, 1], 55);
/** ⌘-scroll: 60 events of 4 px out, then 60 back in. */
export const CMD_SCROLL: readonly number[] = [...Array<number>(60).fill(4), ...Array<number>(60).fill(-4)];
/** The time between two events, in ms: a 120 Hz trackpad's. */
export const GAP = 8;

/** One frame, in ms. */
export interface Frame {
  /** When it started, from the gesture's first event. */
  at: number;
  /** The time since the frame before; null for the first. */
  dt: number | null;
  /** Its whole work: script, layout and paint. */
  work: number;
  script: number;
  /** Of the script, the layout it forced. */
  forced: number;
  /** Style and layout; null where the browser never called the bench's observer. */
  layout: number | null;
  /** Paint and commit; null as `layout`. */
  paint: number | null;
  /** The events sent before it. */
  input: number;
  /** The view's zoom once the frame was drawn, as `zoom` read it. */
  zoom: number | null;
}

/** One replay: every frame while it lasted, and the view's zoom. */
export interface Run {
  frames: Frame[];
  /** When its last event went, in ms from its first. */
  sent: number;
  events: number;
  zoom: { from: number | null; min: number | null; max: number | null; to: number | null };
}

const now = () => performance.now();
const round = (v: number) => Math.round(v * 100) / 100;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** A frame being measured. */
interface Live {
  t: number;
  t0: number;
  script: number;
  scriptEnd: number | null;
  forced0: number;
  forced: number;
  tRO: number | null;
  tEnd: number | null;
  input: number;
  zoom: number | null;
}

/** An own property of `on` replaced by `with_` until the function this returns is called. */
function patch(on: object, name: string, with_: (was: PropertyDescriptor) => PropertyDescriptor): () => void {
  const was = Object.getOwnPropertyDescriptor(on, name);
  if (!was) return () => {};
  Object.defineProperty(on, name, { ...with_(was), configurable: true, enumerable: was.enumerable });
  return () => Object.defineProperty(on, name, was);
}

/**
 * Replay `g` at (`x`, `y`) in the window, each event a task of its own at its time (`gap` ms apart),
 * dispatched on what is under the point then: one the page was too busy for goes late, and those
 * after it back to back, as queued input does. A pinch is `gesturestart`, a `gesturechange` for each
 * scale, and `gestureend`; ⌘-scroll is wheel events with ⌘ held. Every frame is measured from two
 * frames before its first event to 300 ms after its last; `zoom` reads the view's zoom after each.
 */
export async function replay(x: number, y: number, g: Gesture, opts: { gap?: number; zoom?: () => number | null } = {}): Promise<Run> {
  const gap = opts.gap ?? GAP;
  const zoomOf = opts.zoom ?? (() => null);
  const gesture = (type: string, scale: number) => () => {
    const e = new UIEvent(type, { bubbles: true, cancelable: true });
    for (const [k, v] of Object.entries({ scale, rotation: 0, clientX: x, clientY: y })) Object.defineProperty(e, k, { value: v });
    return e;
  };
  const wheel = (dy: number) => () => new WheelEvent("wheel", { deltaY: dy, deltaMode: 0, clientX: x, clientY: y, metaKey: true, bubbles: true, cancelable: true });
  const list: { t: number; make: () => Event }[] = [];
  if (g.kind === "scroll") g.deltas.forEach((dy, i) => list.push({ t: i * gap, make: wheel(dy) }));
  else {
    list.push({ t: 0, make: gesture("gesturestart", 1) });
    g.scales.forEach((s, i) => list.push({ t: (i + 1) * gap, make: gesture("gesturechange", s) }));
    list.push({ t: (g.scales.length + 1) * gap, make: gesture("gestureend", g.scales.at(-1) ?? 1) });
  }

  // ---------------------------------------------------------------- the wrappers
  // Where the page is now, and the frame being measured (written in callbacks: not narrowed here).
  let phase = "other" as "raf" | "input" | "other";
  const forced = { raf: 0, input: 0, other: 0 };
  let frame = null as Live | null;
  const rAF = window.requestAnimationFrame.bind(window);
  const undo: (() => void)[] = [];
  undo.push(
    patch(window, "requestAnimationFrame", (was) => ({
      ...was,
      value: (cb: FrameRequestCallback) =>
        rAF((t) => {
          const s = now();
          const outer = phase;
          phase = "raf";
          try {
            cb(t);
          } finally {
            // After the microtasks the callback queued: React's render and commit.
            queueMicrotask(() => {
              phase = outer;
              if (frame) {
                const e = now();
                frame.script += e - s;
                frame.scriptEnd = e;
              }
            });
          }
        }),
    })),
  );
  const timed = <T>(f: () => T): T => {
    const s = now();
    try {
      return f();
    } finally {
      forced[phase] += now() - s;
    }
  };
  for (const [proto, names, settable] of [
    [Element.prototype, ["scrollWidth", "scrollHeight", "clientWidth", "clientHeight", "clientLeft", "clientTop"], false],
    [Element.prototype, ["scrollLeft", "scrollTop"], true],
    [HTMLElement.prototype, ["offsetWidth", "offsetHeight", "offsetLeft", "offsetTop"], false],
  ] as const)
    for (const name of names)
      undo.push(
        patch(proto, name, (was) => ({
          get(this: Element) {
            return timed(() => was.get!.call(this));
          },
          set: settable && was.set ? function (this: Element, v: unknown) { timed(() => was.set!.call(this, v)); } : was.set,
        })),
      );
  for (const [proto, name] of [
    [Element.prototype, "getBoundingClientRect"],
    [Element.prototype, "getClientRects"],
    [Document.prototype, "elementFromPoint"],
  ] as const)
    undo.push(
      patch(proto, name, (was) => ({
        ...was,
        value(this: unknown, ...a: unknown[]) {
          return timed(() => (was.value as (...a: unknown[]) => unknown).apply(this, a));
        },
      })),
    );

  // ---------------------------------------------------------------- the frame loop
  // A 1 px box whose width changes each frame: the bench's observer hears it after layout.
  const probe = document.createElement("div");
  probe.style.cssText = "position: fixed; left: 0; top: 0; width: 1px; height: 1px; pointer-events: none; opacity: 0; contain: strict";
  document.body.appendChild(probe);
  const ro = new ResizeObserver(() => {
    if (frame && frame.tRO === null) frame.tRO = now();
  });
  ro.observe(probe);
  const ends = new MessageChannel();
  const open = new Map<number, Live>();
  ends.port1.onmessage = (e: MessageEvent<number>) => {
    const f = open.get(e.data);
    open.delete(e.data);
    if (!f) return;
    f.tEnd = now();
    f.zoom = zoomOf();
  };
  const frames: Live[] = [];
  let looping = true;
  let wide = false;
  // The first callback of each frame: it asks for the next before any other is asked.
  const loop = (t: number) => {
    if (!looping) return;
    if (frame) frame.forced = forced.raf - frame.forced0;
    const f: Live = { t, t0: now(), script: 0, scriptEnd: null, forced0: forced.raf, forced: 0, tRO: null, tEnd: null, input: 0, zoom: null };
    frame = f;
    frames.push(f);
    wide = !wide;
    probe.style.width = wide ? "2px" : "1px";
    open.set(frames.length - 1, f);
    ends.port2.postMessage(frames.length - 1);
    rAF(loop);
  };
  const waitFrames = (n: number) => new Promise<void>((r) => {
    const one = (k: number) => (k ? rAF(() => one(k - 1)) : r());
    one(n);
  });

  // ---------------------------------------------------------------- sending
  // A task now, unclamped (a nested setTimeout(0) waits 4 ms).
  const soonQ: (() => void)[] = [];
  const soonC = new MessageChannel();
  soonC.port1.onmessage = () => soonQ.shift()?.();
  const soon = (f: () => void) => {
    soonQ.push(f);
    soonC.port2.postMessage(0);
  };
  const sent: { at: number; ms: number }[] = [];
  const send = () =>
    new Promise<void>((done) => {
      const base = now();
      let k = 0;
      const next = () => {
        if (k < list.length && list[k].t + base <= now() + 0.5) {
          const ev = list[k++];
          const s = now();
          phase = "input";
          try {
            (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(ev.make());
          } finally {
            phase = "other";
          }
          sent.push({ at: s, ms: now() - s });
        }
        if (k >= list.length) return done();
        const wait = list[k].t + base - now();
        if (wait <= 0.5) soon(next);
        else setTimeout(next, wait);
      };
      soon(next);
    });

  try {
    const from = zoomOf();
    rAF(loop);
    await waitFrames(2);
    const start = now();
    await send();
    const last = sent.at(-1)?.at ?? start;
    await waitFrames(4);
    await sleep(300);
    looping = false;
    if (frame) frame.forced = forced.raf - frame.forced0;
    // Each event to the frame after it.
    for (const e of sent) {
      const f = frames.find((f) => f.t0 >= e.at);
      if (f) f.input += e.ms;
    }
    const drawn = frames.filter((f) => f.tEnd !== null);
    const out: Frame[] = drawn.map((f, i) => {
      const layout = f.tRO === null ? null : Math.max(0, f.tRO - (f.scriptEnd ?? f.t0));
      const paint = f.tRO === null || f.tEnd === null ? null : Math.max(0, f.tEnd - f.tRO);
      return {
        at: round(f.t0 - start),
        dt: i ? round(f.t - drawn[i - 1].t) : null,
        work: round(f.tEnd! - f.t0),
        script: round(f.script),
        forced: round(f.forced),
        layout: layout === null ? null : round(layout),
        paint: paint === null ? null : round(paint),
        input: round(f.input),
        zoom: f.zoom,
      };
    });
    const zooms = out.map((f) => f.zoom).filter((z): z is number => z !== null);
    return {
      frames: out,
      sent: round(last - start),
      events: list.length,
      zoom: { from, min: zooms.length ? Math.min(...zooms) : null, max: zooms.length ? Math.max(...zooms) : null, to: zoomOf() },
    };
  } finally {
    looping = false;
    ro.disconnect();
    probe.remove();
    ends.port1.close();
    soonC.port1.close();
    undo.reverse().forEach((f) => f());
  }
}

// ---------------------------------------------------------------- the split

/** The frames a run's gesture made: from its first event to 34 ms (two frames at 60 Hz) after its last. */
export const during = (r: Run) => r.frames.filter((f) => f.at >= 0 && f.at <= r.sent + 34);

/** A part of a frame over many frames, in ms. */
export interface Part {
  med: number;
  p95: number;
  max: number;
}
/** Runs' frames, split: each part's median, 95th percentile and slowest, and the frames whose work went over 8.3 and 16.7 ms. */
export interface Split {
  runs: number;
  frames: number;
  over8: number;
  over16: number;
  work: Part | null;
  script: Part | null;
  forced: Part | null;
  layout: Part | null;
  paint: Part | null;
  input: Part | null;
  dt: Part | null;
}
export const PARTS = ["work", "script", "forced", "layout", "paint", "input", "dt"] as const;
const WORDS: Record<(typeof PARTS)[number], string> = {
  work: "the frame's work",
  script: "script",
  forced: "of it, layout forced",
  layout: "style and layout",
  paint: "paint and commit",
  input: "input",
  dt: "between frames",
};

const quantile = (s: number[], q: number) => s[Math.min(s.length - 1, Math.floor(q * s.length))];
function part(xs: (number | null)[]): Part | null {
  const s = xs.filter((x): x is number => x !== null).sort((a, b) => a - b);
  // A part not timed in every frame isn't timed.
  if (!s.length || s.length < xs.length) return null;
  return { med: quantile(s, 0.5), p95: quantile(s, 0.95), max: s[s.length - 1] };
}

export function split(runs: readonly Run[]): Split {
  const fs = runs.flatMap(during);
  const of = (k: (typeof PARTS)[number]) => part(k === "dt" ? fs.flatMap((f) => (f.dt === null ? [] : [f.dt])) : fs.map((f) => f[k]));
  return {
    runs: runs.length,
    frames: fs.length,
    over8: fs.filter((f) => f.work > 8.3).length,
    over16: fs.filter((f) => f.work > 16.7).length,
    ...(Object.fromEntries(PARTS.map((k) => [k, of(k)])) as Record<(typeof PARTS)[number], Part | null>),
  };
}

const one = (v: number) => String(Math.round(v * 10) / 10);
/** Rows of runs' splits as a table to print or paste, under its line saying what its cells are; `first` heads the rows' names. */
export function table(first: string, rows: readonly (readonly [string, readonly Run[]])[]): string {
  const out = [
    "Each part of a frame, in ms: median / 95th percentile / slowest",
    `| ${first} | frames | work over 8.3 / 16.7 ms | ${PARTS.map((k) => WORDS[k]).join(" | ")} |`,
    `|---|---|---|${PARTS.map(() => "---").join("|")}|`,
  ];
  for (const [name, runs] of rows) {
    const s = split(runs);
    const cells = PARTS.map((k) => {
      const p = s[k];
      return p ? `${one(p.med)} / ${one(p.p95)} / ${one(p.max)}` : "–";
    });
    out.push(`| ${name} | ${s.frames} | ${s.over8} / ${s.over16} | ${cells.join(" | ")} |`);
  }
  return out.join("\n");
}
