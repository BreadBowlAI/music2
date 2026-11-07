// @m2/mixing's pictures (CARD-110 to CARD-116): what each of its cards draws, in TypeScript over
// the picture words (CARD-61), with no kernel. The EQ's curve is measured from its own sound
// (`p.response()`); the transient shaper's hit and Pump's dip are drawn from the pack's own sums,
// each checked against a render of the sound within 0.5 dB (CARD-125); the gate, the de-esser and
// the multiband draw their values over the sound as it plays.
//
// Each shape function takes the card's `p`, so TypeScript checks the names of the values each dot
// moves against the sound's own descriptor, in the axis's unit (CARD-62).

import { axis, knob, live, view, type Moves, type Painter, type PictureDecl, type Shape } from "@m2/core";
import { dbWords, hzWords, known, type In, type Num, type Read, type Values } from "std/pictures";

/** One of the card's groups: the tab a dot shows (CARD-71). */
type Tab<D> = NonNullable<Moves<D>["opens"]>;

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const db = (x: number) => 20 * Math.log10(Math.max(1e-6, x));

// ---------------------------------------------------------------- the levels over time

/**
 * A level's last 3 seconds (CARD-112): in across, from −80 to 0 dB up, the level going in (faint)
 * and coming out, and the level going in now as a bar. The gate's: where it opens is a line on it.
 */
export function levelsPicture(): PictureDecl<"seconds", "db"> {
  return view.picture({
    x: axis.time(3),
    y: axis.lin("db", -80, 0),
    live: [live.history(live.level("in"), { tone: "faint" }), live.history(live.level("out")), live.level("in")],
  });
}

/**
 * Where a gate opens (CARD-112): its threshold as a line across its last 3 seconds, and a dot near
 * its right end, now, that moves the threshold up and down.
 */
export function threshold<D>(p: Painter<D, "seconds", "db">, v: Values, names: { threshold: In<D, "db"> }): Shape[] {
  const t = clamp(known(v, names.threshold, "threshold"), -80, 0);
  return [p.line([0, t, 3, t], { tone: "soft", name: "threshold" }), p.handle(2.8, t, { y: names.threshold } as Moves<D, "seconds", "db">)];
}

// ---------------------------------------------------------------- the transient shaper

/** The rate the transient shaper's hit is worked out at: the kernel's own sums at 48 kHz. */
const RATE = 48_000;
/** How long the hit is drawn, in seconds. */
export const HIT_SECONDS = 0.4;
/** The hit: up in 1 ms, then down 60 dB in 0.3 s, as std's adsr plays it (attack 1 ms, decay 0.3 s, sustain 0). */
export const HIT = { attack: 0.001, decay: 0.3 } as const;
/** Every how many samples a point is drawn: each millisecond. */
const EACH = RATE / 1000;

/** The one-pole coefficient for a time constant of `time` seconds (the kernel's `pole`). */
const pole = (time: number) => Math.exp(-1 / (time * RATE));

/** What the transient shaper hears of the hit, at each millisecond: see `heard`. */
export type Heard = { level: number[]; start: number[]; tail: number[] };
/** `heard`'s sums, once worked out; and how many times they have been (for the test that a build never does, CARD-113). */
let shares: Heard | null = null;
let worked = 0;
/** How many times this module has worked out its sums: a song's build never does (review T1). */
export const sumsWorked = () => worked;

/**
 * What the transient shaper hears of the hit (CARD-113): at each millisecond, the hit's level, and
 * how far it is a hit's start (0 to 1, the share of `attack`) and a hit's tail (0 to 1, the share of
 * `sustain`), by the kernel's own sums (`@m2/mixing:transient`, its `Window` and `Follow`): the
 * loudest sample of the last 25 ms in 1 ms pieces, glided over 2 ms; a follower that rises over
 * 20 ms and falls with it at once, one that falls over 0.3 s and rises with it at once; each share
 * in full from 6 dB on. The shares don't depend on the card's values, so they are worked out on the
 * picture's first draw and kept: never as a song builds, which loads this module too.
 */
export function heard(): Heard {
  if (shares) return shares;
  worked += 1;
  const out: Heard = { level: [], start: [], tail: [] };
  // The hit, as std:adsr plays it.
  const rise = 1 / (HIT.attack * RATE);
  const fall = Math.exp(-6.9 / (HIT.decay * RATE));
  let x = 0;
  let attacking = true;
  // The window: 25 pieces of 1 ms, the loudest of each.
  const PIECES = 25;
  const len = Math.max(1, Math.round(RATE / 1000));
  const pieces = new Array<number>(PIECES).fill(0);
  let at = 0, finished = 0, now = 0, left = len;
  // Three followers, each landing on a level that holds still (the kernel's `Follow`): its value
  // and the level it was given last.
  let glided = 0, glidedLast = 0, riser = 0, riserLast = 0, faller = 0, fallerLast = 0;
  const [glide, slow, hang] = [pole(0.002), pole(0.02), pole(0.3)];
  const step = (value: number, last: number, level: number, coef: number) => {
    if (value === level) return value;
    const y = level + (value - level) * coef;
    const landed = level === last && (y === value || Math.abs(y - level) <= 1e-6 * Math.abs(level));
    return landed ? level : Math.abs(y) < 1e-15 ? 0 : y;
  };
  for (let n = 0; n <= HIT_SECONDS * RATE; n++) {
    // Once the hit falls with no start left and all of a tail, it stays so: the level only falls,
    // the riser falls with it, and the faller, 0.3 s slow, only falls further behind. The rest is
    // the hit's own fall.
    const i = out.start.length - 1;
    if (!attacking && n % EACH === 0 && i > 0 && out.start[i] === 0 && out.tail[i] >= 1) {
      for (let m = n; m <= HIT_SECONDS * RATE; m += EACH) {
        out.level.push(x * fall ** (m - n + 1));
        out.start.push(0);
        out.tail.push(1);
      }
      break;
    }
    if (attacking) {
      x += rise;
      if (x >= 1) [x, attacking] = [1, false];
    } else x *= fall;
    let loudest: number;
    if (x === 0 && now === 0 && finished === 0) {
      [at, left, loudest] = [0, len, 0];
    } else {
      if (x > now) now = x;
      loudest = now > finished ? now : finished;
      if (--left === 0) {
        pieces[at] = now;
        at = (at + 1) % PIECES;
        finished = 0;
        for (let i = 0; i < PIECES; i++) if (pieces[i] > finished) finished = pieces[i];
        [now, left] = [0, len];
      }
    }
    glided = step(glided, glidedLast, loudest, glide);
    glidedLast = loudest;
    const level = glided;
    if (level <= riser) riser = riserLast = level;
    else [riser, riserLast] = [step(riser, riserLast, level, slow), level];
    if (level >= faller) faller = fallerLast = level;
    else [faller, fallerLast] = [step(faller, fallerLast, level, hang), level];
    if (n % EACH === 0) {
      out.level.push(x);
      out.start.push(Math.min(1, (db(level) - db(riser)) / 6));
      out.tail.push(Math.min(1, (db(faller) - db(level)) / 6));
    }
  }
  return (shares = out);
}

/** Where the attack's dot sits, in seconds: early in the hit, where all of `attack` applies. */
export const ATTACK_AT = 0.005;
/** Where the sustain's dot sits: on the tail, where all of `sustain` applies. */
export const SUSTAIN_AT = 0.1;

/** One hit, before and after (CARD-113): 1 ms to 0.4 s across on a log scale, so its start is drawn as wide as its tail, −48 to +24 dB up. */
export function hitPicture(): PictureDecl<"seconds", "db"> {
  return view.picture({ x: axis.log("seconds", 0.001, HIT_SECONDS), y: axis.lin("db", -48, 24) });
}

/**
 * One hit through the transient shaper (CARD-113): the hit as it goes in, a faint fill, and as it
 * comes out, the line `after`: each millisecond turned by `attack` × its share of a start plus
 * `sustain` × its share of a tail, at most 24 dB either way, as the kernel does. Two dots, each
 * moving its value up and down by as much as the line moves there: `attack` early in the hit,
 * `sustain` on its tail.
 */
export function hit<D>(p: Painter<D, "seconds", "db">, v: Values, names: { attack: In<D, "db">; sustain: In<D, "db"> }): Shape[] {
  const a = clamp(known(v, names.attack, "attack"), -24, 24);
  const s = clamp(known(v, names.sustain, "sustain"), -24, 24);
  const before: number[] = [], after: number[] = [];
  const { level, start, tail } = heard();
  level.forEach((x, i) => {
    // From its first millisecond: the axis is a log one.
    if (i === 0) return;
    const t = i / 1000;
    const y = Math.max(-120, db(x));
    before.push(t, y);
    after.push(t, x > 0 ? y + clamp(a * start[i] + s * tail[i], -24, 24) : -120);
  });
  const on = (t: number) => after[2 * (Math.round(t * 1000) - 1) + 1];
  return [
    p.fill(before, { base: -48, tone: "faint" }),
    p.line(after, { name: "after" }),
    p.handle(ATTACK_AT, on(ATTACK_AT), { y: names.attack } as Moves<D, "seconds", "db">),
    p.handle(SUSTAIN_AT, on(SUSTAIN_AT), { y: names.sustain } as Moves<D, "seconds", "db">),
  ];
}

/** What `hit` draws, in words. */
export function hitWords(v: Values, names: { attack: string; sustain: string }): string {
  const said = (x: unknown, what: string) =>
    typeof x !== "number" ? `its ${what} set elsewhere` : x === 0 ? `its ${what} as it was` : `its ${what} ${x > 0 ? "up" : "down"} ${dbWords(Math.abs(x))}`;
  return `One hit, before (faint) and after: ${said(v[names.attack], "start")}, ${said(v[names.sustain], "tail")}`;
}

// ---------------------------------------------------------------- the de-esser

/**
 * The spectrum with a band on it (CARD-114): 20 Hz to 20 kHz across, levels from −90 to 0 dB up,
 * the spectrum's own scale (CARD-103); the sound going in (faint) and coming out.
 */
export function bandPicture(): PictureDecl<"hz", "db"> {
  return view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -90, 0), live: [live.spectrum("in", { tone: "faint" }), live.spectrum("out")] });
}

/** The edges of a band-pass of Q 1 at `f`, where it is 3 dB down: about an octave and a half apart. */
export const bandEdges = (f: number): [number, number] => [f * (Math.sqrt(1.25) - 0.5), f * (Math.sqrt(1.25) + 0.5)];

/**
 * The band a de-esser listens to and turns down (CARD-114): a faint box over the whole height of
 * its band (a Q of 1, between the points 3 dB down), and one dot in its middle, moving the
 * frequency across and the threshold as its knob does. The threshold is no line on the picture:
 * the kernel compares the band's peaks with it, and no bar of the spectrum reads those (a bar is a
 * twelfth of an octave, the band about seventeen; an S sound's peaks stand well over its bars).
 */
export function band<D>(p: Painter<D, "hz", "db">, v: Values, names: { freq: In<D, "hz">; threshold: Num<D> }): Shape[] {
  const f = known(v, names.freq, "frequency");
  known(v, names.threshold, "threshold");
  const [lo, hi] = bandEdges(f);
  return [p.fill([lo, -90, hi, -90, hi, 0, lo, 0], { tone: "faint" }), p.handle(f, -45, { x: names.freq, y: knob(names.threshold) } as unknown as Moves<D, "hz", "db">)];
}

/** What `band` draws, in words. */
export function bandWords(v: Values, names: { freq: string; threshold: string; amount: string }): string {
  const [f, t, a] = [v[names.freq], v[names.threshold], v[names.amount]];
  if (typeof f !== "number" || typeof t !== "number") return "Turns a band down while it is loud: its frequency or threshold is set by a cable or the code";
  const [lo, hi] = bandEdges(f);
  const most2 = typeof a === "number" ? `up to ${dbWords(a)}` : "up to an amount set elsewhere";
  return `Listens from ${hzWords(lo)} to ${hzWords(hi)}: when that band's peaks go over ${dbWords(t)}, it turns the band down by as much, ${most2}`;
}

// ---------------------------------------------------------------- the multiband compressor

/** The bands' names, low to high: the card's groups. */
const BANDS = ["Low", "Mid", "High"] as const;

/** The rate the split is worked out at. */
const SPLIT_RATE = 48_000;
/** How many points the split's curve is drawn with: one each twenty-fourth of the 10 octaves. */
const SPLIT_POINTS = 241;

/**
 * How the multiband turns each frequency `f`, in dB, below its thresholds (its compressors
 * resting): its kernel's own sum. Its lower crossover splits the low band (a Linkwitz-Riley
 * low-pass, 24 dB per octave) from the rest (the high-pass), its upper splits the rest into the mid
 * and the high band, and the low band goes through the upper's all-pass so the three sum in step;
 * each band is turned by its gain. With every gain at 0 dB it is flat.
 */
export function splitDb(f: number, lowMid: number, midHigh: number, gains: readonly [number, number, number]): number {
  const warp = (hz: number) => Math.tan((Math.PI * clamp(hz, 10, 0.45 * SPLIT_RATE)) / SPLIT_RATE);
  const w = warp(f);
  // At a crossover at `hz`: the Butterworth pair's denominator, (1 − x²) + j√2·x, at x = w / its own.
  const at = (hz: number) => {
    const x = w / warp(hz);
    return { x, re: 1 - x * x, im: Math.SQRT2 * x };
  };
  type C = { re: number; im: number };
  const mul = (a: C, b: C): C => ({ re: a.re * b.re - a.im * b.im, im: a.re * b.im + a.im * b.re });
  const div = (a: C, b: C): C => {
    const n = b.re * b.re + b.im * b.im;
    return { re: (a.re * b.re + a.im * b.im) / n, im: (a.im * b.re - a.re * b.im) / n };
  };
  const [lo, hi] = [at(Math.min(lowMid, midHigh)), at(Math.max(lowMid, midHigh))];
  const one = { re: 1, im: 0 };
  // Linkwitz-Riley: a low-pass 1 / B², a high-pass x⁴ / B², and the all-pass B̄ / B.
  const lp = (c: typeof lo) => div(one, mul(c, c));
  const hp = (c: typeof lo) => div({ re: c.x ** 4, im: 0 }, mul(c, c));
  const ap = (c: typeof lo) => div({ re: c.re, im: -c.im }, c);
  const g = gains.map((x) => 10 ** (x / 20));
  const low = mul(ap(hi), lp(lo));
  const mid = mul(lp(hi), hp(lo));
  const high = mul(hp(hi), hp(lo));
  const re = g[0] * low.re + g[1] * mid.re + g[2] * high.re, im = g[0] * low.im + g[1] * mid.im + g[2] * high.im;
  return 10 * Math.log10(Math.max(1e-12, re * re + im * im));
}

/**
 * A multiband compressor's split (CARD-115), on `curvePicture()`'s axes: how it turns each
 * frequency below its thresholds, the line `split` from its own sum (`splitDb`); the two crossovers
 * as guide lines, a dot on each that moves it across; and a dot in each band, at its gain, that moves
 * the gain up and down and shows its tab. How far its compressors turn down as it plays isn't drawn:
 * its kernel sends no meter (gaps.md).
 */
export function split<D>(
  p: Painter<D, "hz", "db">,
  v: Values,
  names: { lowMid: In<D, "hz">; midHigh: In<D, "hz">; gains: readonly [In<D, "db">, In<D, "db">, In<D, "db">]; tabs: readonly [Tab<D>, Tab<D>, Tab<D>] },
): Shape[] {
  const a = known(v, names.lowMid, "low to mid crossover"), b = known(v, names.midHigh, "mid to high crossover");
  // Crossed, the lower one splits low from mid (the kernel's own rule).
  const [lm, mh] = [Math.min(a, b), Math.max(a, b)];
  const g = names.gains.map((n, i) => known(v, n, `${BANDS[i].toLowerCase()} gain`)) as [number, number, number];
  const points: number[] = [];
  for (let i = 0; i < SPLIT_POINTS; i++) {
    const f = 20 * 1000 ** (i / (SPLIT_POINTS - 1));
    points.push(f, clamp(splitDb(f, lm, mh, g), -24, 24));
  }
  const edges = [20, lm, mh, 20000];
  const centre = (i: number) => Math.sqrt(edges[i] * edges[i + 1]);
  return [
    p.line(points, { name: "split" }),
    p.mark("x", lm, `Low to mid: ${hzWords(lm)}`),
    p.mark("x", mh, `Mid to high: ${hzWords(mh)}`),
    p.handle(a, 18, { x: names.lowMid } as Moves<D, "hz", "db">),
    p.handle(b, 18, { x: names.midHigh } as Moves<D, "hz", "db">),
    ...g.map((y, i) => p.handle(centre(i), clamp(y, -24, 24), { y: names.gains[i], opens: names.tabs[i], mark: BANDS[i][0] } as unknown as Moves<D, "hz", "db">)),
  ];
}

/** What `split` draws, in words. */
export function splitWords(v: Values, names: { lowMid: string; midHigh: string; gains: readonly [string, string, string] }): string {
  const [a, b] = [v[names.lowMid], v[names.midHigh]];
  if (typeof a !== "number" || typeof b !== "number") return "Lows, mids and highs, each compressed on its own: a crossover is set by a cable or the code";
  const g = names.gains.map((n) => (typeof v[n] === "number" ? dbWords(v[n] as number) : "set elsewhere"));
  return `Lows below ${hzWords(Math.min(a, b))}, mids to ${hzWords(Math.max(a, b))}, highs above, each compressed on its own, then turned ${g[0]}, ${g[1]} and ${g[2]}: as it sounds while quieter than its thresholds`;
}

// ---------------------------------------------------------------- Pump

/** How often Pump dips, by a note's length, and how many beats that is. */
export const RATES = { "1/2": 2, "1/4": 1, "1/8": 0.5, "1/16": 0.25 } as const;

/** How far Pump turns down `into` (0 to 1) of the way through its step, in dB: its own sum. */
export function dipDb(depth: number, shape: number, into: number): number {
  const left = 1 - into;
  return depth * ((1 - shape) * left + shape * left ** 4);
}

/** One bar of Pump's dip (CARD-116): 4 beats across, −24 to 0 dB up, the song's playhead on it and the sound coming out under it. */
export function dipPicture(): PictureDecl<"beats", "db"> {
  return view.picture({ x: axis.lin("beats", 0, 4), y: axis.lin("db", -24, 0), live: [live.history(live.level("out"), { tone: "faint" }), live.playhead("beat")] });
}

/** How many points each step of the dip is drawn with. */
const STEP_POINTS = 48;

/**
 * Pump's dip over a bar of 4 beats (CARD-116), from its own sum (`dipDb`): down `depth` dB at the
 * start of each step and back to 0 dB at its end, the line `dip`; on its second step, a dot at its
 * foot that moves the depth up and down, and a dot on its curve, a third of the way through, that
 * moves the shape as its knob does (up, more: the curve comes back sooner) and by the wheel.
 */
export function dip<D>(p: Painter<D, "beats", "db">, v: Values, names: { depth: In<D, "db">; shape: Num<D>; rate: Read<D> }): Shape[] {
  const depth = clamp(known(v, names.depth, "depth"), -24, 0);
  const shape = clamp(known(v, names.shape, "shape"), 0, 1);
  const rate = v[names.rate];
  if (rate === undefined) throw new Error(`names ${String(names.rate)}, which this sound doesn't have`);
  // A rate the code works out greys the picture: it never draws a rate the song may not play (review T8).
  if (typeof rate !== "string" || !(rate in RATES)) throw new Error("its rate is set by a cable or worked out by the code, so it can't be drawn");
  const step = RATES[rate as keyof typeof RATES];
  const points: number[] = [];
  for (let at = 0; at < 4 - 1e-9; at += step) {
    for (let i = 0; i < STEP_POINTS; i++) points.push(at + (step * i) / STEP_POINTS, dipDb(depth, shape, i / STEP_POINTS));
    points.push(at + step, 0);
  }
  return [
    p.fill(points, { base: 0, tone: "faint" }),
    p.line(points, { name: "dip" }),
    p.handle(step, depth, { y: names.depth } as Moves<D, "beats", "db">),
    p.handle(step + step / 3, dipDb(depth, shape, 1 / 3), { y: knob(names.shape), wheel: names.shape } as unknown as Moves<D, "beats", "db">),
  ];
}

/** What `dip` draws, in words. */
export function dipWords(v: Values, names: { depth: string; shape: string; rate: string }): string {
  const [d, s, r] = [v[names.depth], v[names.shape], v[names.rate]];
  const down = typeof d === "number" ? dbWords(Math.abs(d)) : "an amount set elsewhere";
  const every = typeof r === "string" ? `every ${r} note` : "at a rate set elsewhere";
  const back = typeof s === "number" ? (s >= 0.67 ? "quickly" : s <= 0.33 ? "slowly" : "steadily") : "as its shape says";
  return `Dips ${down} ${every} and comes back ${back}. Drag the foot for the depth, the curve for the shape`;
}
