// Meters (8b.8, taste.md §2.8): the level of every track and of the song's output, from the
// server's `meters` messages (about 30 a second while the engine runs), drawn on canvases by one
// `requestAnimationFrame` loop. Kept here, outside React and the store: 30 messages a second must
// never re-render the page.
//
// - A meter shows the latest peak, never eased. Its peak hold is a 1 px tick, held 1.5 s, then
//   falling 12 dB a second.
// - The server sends one last message as the engine starts to idle, so the meters fall to the
//   silence it ended in; then none. When the socket closes, every meter reads the floor.
// - The scale runs from −60 to +6 dBFS. The fill is `fg-muted` below −6, `warn` from −6 to 0 and
//   `danger` at 0 or above.
// - A header's meter is one bar, the louder side; a Mix strip's is two, left and right. A strip
//   with a channel reads the channel, after its fader (TAP-5); one without, its tracks, the
//   louder of them on each side.
// - A track whose peak reaches 0 dBFS has clipped: its strip's dot stays lit until clicked.
// - The output's loudness (TAP-6), for Master's strip, is kept here too, and told only when the
//   words it is shown in change.

import { useEffect, useSyncExternalStore } from "react";
import { CLOSED, on } from "./events";
import { graphApi } from "./graph";
import type { MetersIn } from "./graph8b";
import { useStore } from "./store";

/** The scale, in dBFS: the floor (silence and below) and the top. */
export const FLOOR = -60;
export const TOP = 6;
/** How long the peak hold stays before it falls, and how fast it falls. */
const HOLD_MS = 1500;
const FALL_DB_PER_S = 12;
/** `aria-valuenow` changes at most this often: a screen reader can't follow 30 a second. */
const ARIA_MS = 100;
/** The meter of the song's output: the Song row's. A track may be called `out`, so the output has a name of its own. */
export const OUT = "song";
/** The sound input's meter, while a take records audio (io): no track's key has an `@`. */
export const INPUT = "@input";
/** A mixer channel's meter, after its fader (TAP-5): no track's key has a `:`. */
export const channelMeter = (channel: string) => `channel:${channel}`;
/** The space between a two-bar meter's bars, in CSS pixels. */
const GAP = 1;

interface Level {
  /** The latest peak, in dB (−Infinity for silence). */
  db: number;
  /** The held peak, and when it was set. */
  peak: number;
  at: number;
}

/** A meter's sides, as `levels` keeps them: both (the louder), left, right. */
const BOTH = 0;
const SIDES = [1, 2];

interface Drawn {
  canvas: HTMLCanvasElement;
  /** The meters it shows the louder of: a track's key, a strip's tracks, or `OUT`. */
  keys: string[];
  /** One bar (the louder side) or two (left and right). */
  bars: 1 | 2;
  /** Its size in CSS pixels, from a ResizeObserver: the loop never reads layout. */
  w: number;
  h: number;
  /** What was last drawn, per bar, in pixels from the bottom, and the size it was drawn at: redrawn only when one changes. */
  level: number[];
  tick: number[];
  size: string;
  /** What `aria-valuenow` (and a two-bar meter's `data-left` and `data-right`) say now. */
  aria: number[];
}

/** Each meter's levels: both sides, left, right. */
const levels = new Map<string, Level[]>();
const meters = new Set<Drawn>();
let frame = 0;
/** When the readouts last changed: every meter's at once, so two meters of one track always agree. */
let ariaAt = -Infinity;
let stopListening: (() => void) | null = null;

const toDb = (x: number) => (x > 0 ? 20 * Math.log10(x) : -Infinity);

/** Where the hold tick of `l` is at `now`: held, then falling. */
function held(l: Level, now: number) {
  const late = now - l.at - HOLD_MS;
  return late <= 0 ? l.peak : l.peak - (FALL_DB_PER_S * late) / 1000;
}

/** One side's new peak, at `now`. */
function hearSide(l: Level, x: number, now: number) {
  const db = toDb(x);
  l.db = db;
  if (db >= held(l, now)) {
    l.peak = db;
    l.at = now;
  }
}

/** One meter's new peaks (`[l, r]`, or one for mono), at `now`. A track's at 0 dBFS or over has clipped. */
function hear(key: string, peaks: number[], now: number) {
  const left = Math.max(0, peaks[0] ?? 0);
  const right = Math.max(0, peaks[1] ?? left);
  let l = levels.get(key);
  if (!l) levels.set(key, (l = [0, 1, 2].map(() => ({ db: -Infinity, peak: -Infinity, at: now }))));
  const sides = l;
  [Math.max(left, right), left, right].forEach((x, i) => hearSide(sides[i], x, now));
  if (key !== OUT && key !== INPUT && Math.max(left, right) >= 1) clip(key);
}

function onMeters(msg: MetersIn) {
  const now = performance.now();
  for (const [key, peaks] of Object.entries(msg.tracks ?? {})) hear(key, peaks, now);
  for (const [key, peaks] of Object.entries(msg.channels ?? {})) hear(channelMeter(key), peaks, now);
  if (msg.out) hear(OUT, msg.out, now);
  if (msg.input) hear(INPUT, msg.input, now);
  hearLoudness(msg.loudness);
}

/** Every meter to the floor: what it was told is old news. */
function floor() {
  for (const l of levels.values()) {
    for (const s of l) {
      s.db = -Infinity;
      s.peak = -Infinity;
    }
  }
  hearLoudness(undefined);
}

// ---------------------------------------------------------------- loudness (TAP-6)

/** The output's loudness as Master's strip says it: whole LUFS, the true peak to a tenth of a dB; null for none. */
export interface Loudness {
  short: number | null;
  peak: number | null;
}

let loudness: Loudness = { short: null, peak: null };
const loudnessTold = new Set<() => void>();

function hearLoudness(l: MetersIn["loudness"]) {
  const short = l?.short == null ? null : Math.round(l.short);
  const peak = l?.true_peak == null ? null : Math.round(l.true_peak * 10) / 10;
  if (short === loudness.short && peak === loudness.peak) return;
  loudness = { short, peak };
  loudnessTold.forEach((f) => f());
}

/** How often a view that shows the loudness asks for it again: the server keeps an ask 10 s. */
const ASK_MS = 4000;

/**
 * The output's loudness, asked for while the calling view shows (TAP-6: the server measures it only
 * then), and stopped when it goes; re-rendered only when its words change.
 */
export function useLoudness(): Loudness {
  const session = useStore((s) => s.graph?.session);
  useEffect(() => {
    if (session === undefined) return;
    const ask = (on: boolean) => void graphApi.loudness(session, on).catch(() => {});
    ask(true);
    const again = setInterval(() => ask(true), ASK_MS);
    return () => {
      clearInterval(again);
      ask(false);
    };
  }, [session]);
  return useSyncExternalStore(
    (f) => {
      loudnessTold.add(f);
      return () => void loudnessTold.delete(f);
    },
    () => loudness,
  );
}

/** The colours, read once from the design's tokens. */
let colours: { trough: string; fill: string; warn: string; danger: string; tick: string } | null = null;
function palette() {
  if (colours) return colours;
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(`--color-${name}`).trim();
  colours = { trough: v("line"), fill: v("fg-muted"), warn: v("warn"), danger: v("danger"), tick: v("fg-strong") };
  return colours;
}

/** `db` as a height in pixels from the bottom of a meter `h` tall: 0 at the floor and below. */
const heightOf = (db: number, h: number) => (db <= FLOOR ? 0 : Math.round(((Math.min(db, TOP) - FLOOR) / (TOP - FLOOR)) * h));

/** The louder of `keys` on `side`, at `now`: its latest peak and where its hold tick is. */
function sideOf(keys: string[], side: number, now: number) {
  let db = -Infinity;
  let hold = -Infinity;
  for (const k of keys) {
    const l = levels.get(k)?.[side];
    if (!l) continue;
    db = Math.max(db, l.db);
    hold = Math.max(hold, held(l, now), l.db);
  }
  return { db, hold };
}

/** A level as a readout says it: whole dB, the floor for silence. */
const readout = (db: number) => (db > FLOOR ? Math.min(Math.round(db), TOP) : FLOOR);

function draw(m: Drawn, now: number, read: boolean) {
  const c = m.canvas;
  const { w, h } = m;
  const dpr = window.devicePixelRatio || 1;
  const size = `${w}x${h}@${dpr}`;
  const sides = m.bars === 1 ? [BOTH] : SIDES;
  const got = sides.map((s) => sideOf(m.keys, s, now));
  if (read) {
    // Its value is the louder side; a two-bar meter says each side too.
    const both = readout(Math.max(...got.map((g) => g.db)));
    const each = got.map((g) => readout(g.db));
    if (both !== m.aria[0]) {
      c.setAttribute("aria-valuenow", String(both));
      c.setAttribute("aria-valuetext", both <= FLOOR ? "Silent" : `${both} dB`);
    }
    if (m.bars === 2 && (each[0] !== m.aria[1] || each[1] !== m.aria[2])) {
      c.dataset.left = String(each[0]);
      c.dataset.right = String(each[1]);
    }
    m.aria = [both, ...each];
  }
  const level = got.map((g) => heightOf(g.db, h));
  const tick = got.map((g) => heightOf(g.hold, h));
  if (size === m.size && level.every((v, i) => v === m.level[i]) && tick.every((v, i) => v === m.tick[i])) return;
  m.level = level;
  m.tick = tick;
  if (size !== m.size) {
    m.size = size;
    c.width = Math.max(1, Math.round(w * dpr));
    c.height = Math.max(1, Math.round(h * dpr));
  }
  const g = c.getContext("2d");
  if (!g) return;
  const p = palette();
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, w, h);
  const bw = m.bars === 1 ? w : (w - GAP) / 2;
  sides.forEach((_, i) => {
    const x = i * (bw + GAP);
    g.fillStyle = p.trough;
    g.fillRect(x, 0, bw, h);
    // The fill in its zones, from the bottom: up to −6 dB, up to 0, above.
    const zones: [number, string][] = [
      [heightOf(-6, h), p.fill],
      [heightOf(0, h), p.warn],
      [h, p.danger],
    ];
    let from = 0;
    for (const [to, colour] of zones) {
      const top = Math.min(level[i], to);
      if (top > from) {
        g.fillStyle = colour;
        g.fillRect(x, h - top, bw, top - from);
      }
      from = to;
    }
    // The peak hold: a 1 px tick, above the fill.
    if (tick[i] > 0) {
      g.fillStyle = p.tick;
      g.fillRect(x, h - tick[i], bw, 1);
    }
  });
}

function loop() {
  const now = performance.now();
  const read = now - ariaAt >= ARIA_MS;
  if (read) ariaAt = now;
  for (const m of meters) draw(m, now, read);
  frame = meters.size ? requestAnimationFrame(loop) : 0;
}

/**
 * Draw the meter of `keys` (a track's key, a strip's tracks, or `OUT`) on `canvas` until the
 * returned function is called: one bar for the louder side, or two for left and right. The first
 * meter shown starts the listening and the loop; the last one gone stops both.
 */
export function mount(keys: string[], canvas: HTMLCanvasElement, bars: 1 | 2 = 1): () => void {
  const m: Drawn = { canvas, keys, bars, w: canvas.clientWidth, h: canvas.clientHeight, level: [], tick: [], size: "", aria: [] };
  const sized = new ResizeObserver(() => {
    m.w = canvas.clientWidth;
    m.h = canvas.clientHeight;
  });
  sized.observe(canvas);
  meters.add(m);
  if (!stopListening) {
    const offMeters = on<MetersIn>("meters", onMeters);
    const offClosed = on(CLOSED, floor);
    stopListening = () => {
      offMeters();
      offClosed();
    };
  }
  if (!frame) frame = requestAnimationFrame(loop);
  return () => {
    sized.disconnect();
    meters.delete(m);
    if (meters.size) return;
    stopListening?.();
    stopListening = null;
    levels.clear();
    hearLoudness(undefined);
    cancelAnimationFrame(frame);
    frame = 0;
  };
}

// ---------------------------------------------------------------- clips (taste.md §2.8)

/** The tracks that clipped since their dot was last cleared, and who is told when that changes. */
let clipped = new Set<string>();
const told = new Set<() => void>();
const tell = () => told.forEach((f) => f());

function clip(key: string) {
  if (clipped.has(key)) return;
  clipped = new Set(clipped).add(key);
  tell();
}

/** Clear the clip dot of `keys`. */
export function clearClips(keys: string[]) {
  if (!keys.some((k) => clipped.has(k))) return;
  clipped = new Set([...clipped].filter((k) => !keys.includes(k)));
  tell();
}

// Another song: none of the last one's clips stays.
let session: number | undefined;
useStore.subscribe((s) => {
  if (s.graph?.session === session) return;
  session = s.graph?.session;
  if (!clipped.size) return;
  clipped = new Set();
  tell();
});

const subscribe = (f: () => void) => {
  told.add(f);
  return () => void told.delete(f);
};

/** Whether any of `keys` clipped since its dot was last cleared. */
export function useClipped(keys: string[]) {
  const key = keys.join(" ");
  return useSyncExternalStore(subscribe, () => key.split(" ").some((k) => clipped.has(k)));
}
