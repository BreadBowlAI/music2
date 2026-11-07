// @m2/mixing's pictures (CARD-110 to CARD-116): what each of its cards draws, in TypeScript over
// the picture words (CARD-61), with no kernel. The EQ's curve is measured from its own sound
// (`p.response()`); the transient shaper's hit and Pump's dip are drawn from the pack's own sums,
// each checked against a render of the sound within 0.5 dB (CARD-125); the gate, the de-esser and
// the multiband draw their values over the sound as it plays.
//
// Each shape function takes the card's `p`, so TypeScript checks the names of the values each dot
// moves against the sound's own descriptor, in the axis's unit (CARD-62).

import { axis, knob, live, view, type Moves, type Painter, type PictureDecl, type Shape, type Unit } from "@m2/core";
import { dbWords, hzWords } from "std/pictures";

/** The name of one of the sound's number controls: what a dot moves by the wheel, or as its knob. */
type Num<D> = NonNullable<Moves<D>["wheel"]>;
/** The name of one of the sound's controls in `U` (CARD-62). */
type In<D, U extends Unit> = Extract<NonNullable<Moves<D, U, U>["x"]>, string>;
/** One of the card's groups: the tab a dot shows (CARD-71). */
type Tab<D> = NonNullable<Moves<D>["opens"]>;
/** What plays, by name (CARD-63). */
type Values = { readonly [k: string]: unknown };

/** `v[name]`, which a cable or the code may hold (CARD-63): else the picture can't be drawn, said in words. */
function known(v: Values, name: string, what: string): number {
  const x = v[name];
  if (typeof x !== "number") throw new Error(`its ${what} is set by a cable or worked out by the code, so it can't be drawn`);
  return x;
}

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

/**
 * What the transient shaper hears of the hit, worked out once (CARD-113): at each millisecond, the
 * hit's level, and how far it is a hit's start (0 to 1, the share of `attack`) and a hit's tail (0
 * to 1, the share of `sustain`), by the kernel's own sums (`@m2/mixing:transient`): the loudest
 * sample of the last 25 ms in 1 ms pieces, glided over 2 ms; a follower that rises over 20 ms and
 * falls with it at once, one that falls over 0.3 s and rises with it at once; each share in full
 * from 6 dB on. The shares don't depend on the card's values, so they are worked out at load.
 */
export const HEARD: { level: number[]; start: number[]; tail: number[] } = (() => {
  const out = { level: [] as number[], start: [] as number[], tail: [] as number[] };
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
  // A follower that lands on a level that holds still (the kernel's `Follow`).
  const follower = () => {
    let value = 0, last = 0;
    return {
      get: () => value,
      next(level: number, coef: number) {
        const still = level === last;
        last = level;
        if (value !== level) {
          const y = level + (value - level) * coef;
          const landed = still && (y === value || Math.abs(y - level) <= 1e-6 * Math.abs(level));
          value = landed ? level : Math.abs(y) < 1e-15 ? 0 : y;
        }
        return value;
      },
      set(level: number) {
        value = last = level;
        return level;
      },
    };
  };
  const smooth = follower(), riser = follower(), faller = follower();
  const [glide, slow, hang] = [pole(0.002), pole(0.02), pole(0.3)];
  for (let n = 0; n <= HIT_SECONDS * RATE; n++) {
    if (attacking) {
      x += rise;
      if (x >= 1) [x, attacking] = [1, false];
    } else x *= fall;
    let loudest: number;
    if (x === 0 && now === 0 && finished === 0) {
      [at, left, loudest] = [0, len, 0];
    } else {
      now = Math.max(now, x);
      loudest = Math.max(now, finished);
      if (--left === 0) {
        pieces[at] = now;
        at = (at + 1) % PIECES;
        finished = Math.max(...pieces);
        [now, left] = [0, len];
      }
    }
    const level = smooth.next(loudest, glide);
    const r = level <= riser.get() ? riser.set(level) : riser.next(level, slow);
    const f = level >= faller.get() ? faller.set(level) : faller.next(level, hang);
    if (n % EACH === 0) {
      out.level.push(x);
      out.start.push(Math.min(1, (db(level) - db(r)) / 6));
      out.tail.push(Math.min(1, (db(f) - db(level)) / 6));
    }
  }
  return out;
})();

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
  HEARD.level.forEach((x, i) => {
    // From its first millisecond: the axis is a log one.
    if (i === 0) return;
    const t = i / 1000;
    const y = Math.max(-120, db(x));
    before.push(t, y);
    after.push(t, x > 0 ? y + clamp(a * HEARD.start[i] + s * HEARD.tail[i], -24, 24) : -120);
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
 * on the spectrum's own scale (CARD-103), so a level on it is where the sound is; the sound going
 * in (faint) and coming out.
 */
export function bandPicture(): PictureDecl<"hz", "db"> {
  return view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -90, 0), live: [live.spectrum("in", { tone: "faint" }), live.spectrum("out")] });
}

/** The edges of a band-pass of Q 1 at `f`, where it is 3 dB down: about an octave and a half apart. */
export const bandEdges = (f: number): [number, number] => [f * (Math.sqrt(1.25) - 0.5), f * (Math.sqrt(1.25) + 0.5)];

/**
 * The band a de-esser listens to (CARD-114): a faint box over its band (a Q of 1, between the
 * points 3 dB down), from its threshold up, where what is louder is turned down; and one dot at
 * its frequency and threshold, moving the frequency across and the threshold up and down.
 */
export function band<D>(p: Painter<D, "hz", "db">, v: Values, names: { freq: In<D, "hz">; threshold: In<D, "db"> }): Shape[] {
  const f = known(v, names.freq, "frequency");
  const t = clamp(known(v, names.threshold, "threshold"), -90, 0);
  const [lo, hi] = bandEdges(f);
  return [
    p.fill([lo, t, hi, t, hi, 0, lo, 0], { tone: "faint" }),
    p.line([lo, t, hi, t], { name: "threshold" }),
    p.handle(f, t, { x: names.freq, y: names.threshold } as Moves<D, "hz", "db">),
  ];
}

/** What `band` draws, in words. */
export function bandWords(v: Values, names: { freq: string; threshold: string; amount: string }): string {
  const [f, t, a] = [v[names.freq], v[names.threshold], v[names.amount]];
  if (typeof f !== "number" || typeof t !== "number") return "Turns a band down while it is loud: its frequency or threshold is set by a cable or the code";
  const [lo, hi] = bandEdges(f);
  const most = typeof a === "number" ? `by up to ${dbWords(a)}` : "by an amount set elsewhere";
  return `Listens from ${hzWords(lo)} to ${hzWords(hi)}: louder than ${dbWords(t)} there, that band is turned down, ${most}`;
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
  return `Lows below ${hzWords(Math.min(a, b))}, mids to ${hzWords(Math.max(a, b))}, highs above, each compressed on its own, then turned ${g[0]}, ${g[1]} and ${g[2]}. Drawn below its thresholds`;
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
export function dip<D>(p: Painter<D, "beats", "db">, v: Values, names: { depth: In<D, "db">; shape: Num<D>; rate: string }): Shape[] {
  const depth = clamp(known(v, names.depth, "depth"), -24, 0);
  const shape = clamp(known(v, names.shape, "shape"), 0, 1);
  const rate = v[names.rate];
  const step = RATES[(typeof rate === "string" && rate in RATES ? rate : "1/4") as keyof typeof RATES];
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
