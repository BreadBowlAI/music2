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

import { CLOSED, on } from "./events";
import type { MetersIn } from "./graph8b";

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

interface Level {
  /** The latest peak, in dB (−Infinity for silence). */
  db: number;
  /** The held peak, and when it was set. */
  peak: number;
  at: number;
}

interface Drawn {
  canvas: HTMLCanvasElement;
  key: string;
  /** Its size in CSS pixels, from a ResizeObserver: the loop never reads layout. */
  w: number;
  h: number;
  /** What was last drawn, in pixels from the bottom, and the size it was drawn at: redrawn only when one changes. */
  level: number;
  tick: number;
  size: string;
  /** What `aria-valuenow` says now, and when it last changed. */
  aria: number;
  ariaAt: number;
}

const levels = new Map<string, Level>();
const meters = new Set<Drawn>();
let frame = 0;
let stopListening: (() => void) | null = null;

const toDb = (x: number) => (x > 0 ? 20 * Math.log10(x) : -Infinity);

/** One meter's new peak, at `now`. */
function hear(key: string, x: number, now: number) {
  const db = toDb(x);
  const l = levels.get(key);
  if (!l) return void levels.set(key, { db, peak: db, at: now });
  l.db = db;
  if (db >= held(l, now)) {
    l.peak = db;
    l.at = now;
  }
}

/** Where the hold tick of `l` is at `now`: held, then falling. */
function held(l: Level, now: number) {
  const late = now - l.at - HOLD_MS;
  return late <= 0 ? l.peak : l.peak - (FALL_DB_PER_S * late) / 1000;
}

function onMeters(msg: MetersIn) {
  const now = performance.now();
  for (const [key, peaks] of Object.entries(msg.tracks ?? {})) hear(key, Math.max(0, ...peaks), now);
  if (msg.out) hear(OUT, Math.max(0, ...msg.out), now);
}

/** Every meter to the floor: what it was told is old news. */
function floor() {
  for (const l of levels.values()) {
    l.db = -Infinity;
    l.peak = -Infinity;
  }
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

function draw(m: Drawn, now: number) {
  const l = levels.get(m.key);
  const c = m.canvas;
  const { w, h } = m;
  const dpr = window.devicePixelRatio || 1;
  const size = `${w}x${h}@${dpr}`;
  const level = l ? heightOf(l.db, h) : 0;
  const tick = l ? heightOf(Math.max(held(l, now), l.db), h) : 0;
  const db = l && l.db > FLOOR ? Math.min(Math.round(l.db), TOP) : FLOOR;
  if (db !== m.aria && now - m.ariaAt >= ARIA_MS) {
    m.aria = db;
    m.ariaAt = now;
    c.setAttribute("aria-valuenow", String(db));
    c.setAttribute("aria-valuetext", db <= FLOOR ? "Silent" : `${db} dB`);
  }
  if (level === m.level && tick === m.tick && size === m.size) return;
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
  g.fillStyle = p.trough;
  g.fillRect(0, 0, w, h);
  // The fill in its zones, from the bottom: up to −6 dB, up to 0, above.
  const zones: [number, string][] = [
    [heightOf(-6, h), p.fill],
    [heightOf(0, h), p.warn],
    [h, p.danger],
  ];
  let from = 0;
  for (const [to, colour] of zones) {
    const top = Math.min(level, to);
    if (top > from) {
      g.fillStyle = colour;
      g.fillRect(0, h - top, w, top - from);
    }
    from = to;
  }
  // The peak hold: a 1 px tick, above the fill.
  if (tick > 0) {
    g.fillStyle = p.tick;
    g.fillRect(0, h - tick, w, 1);
  }
}

function loop() {
  const now = performance.now();
  for (const m of meters) draw(m, now);
  frame = meters.size ? requestAnimationFrame(loop) : 0;
}

/**
 * Draw meter `key` (a track's key, or `OUT`) on `canvas` until the returned function is called.
 * The first meter shown starts the listening and the loop; the last one gone stops both.
 */
export function mount(key: string, canvas: HTMLCanvasElement): () => void {
  const m: Drawn = { canvas, key, w: canvas.clientWidth, h: canvas.clientHeight, level: -1, tick: -1, size: "", aria: NaN, ariaAt: -Infinity };
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
    cancelAnimationFrame(frame);
    frame = 0;
  };
}
