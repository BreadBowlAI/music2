// @m2/mixing's pictures (CARD-110 to CARD-116): what each of its cards draws, in TypeScript over
// the picture words (CARD-61), with no kernel. The EQ's curve, and the multiband's split under its
// thresholds, are measured from their own sound (`p.response()`); the transient shaper's hit and
// Pump's dip are drawn from the pack's own sums, each checked against a render of the sound within
// 0.5 dB (CARD-99); the gate and the de-esser draw their values over the sound as it plays. How far
// the gate, the de-esser and the multiband turn the sound down, and whether the gate is open, are
// their kernels' own meters, drawn as live layers (CARD-126): no draw works one out.
//
// Each shape function takes the card's `p`, so TypeScript checks the names of the values each dot
// moves against the sound's own descriptor, in the axis's unit (CARD-62).

import { axis, knob, live, view, type Moves, type Painter, type PictureDecl, type Shape } from "@m2/core";
import { dbWords, hzWords, known, reduction, type In, type Num, type Read, type Values } from "std/pictures";

/** One of the card's groups: the tab a dot shows (taste.md DV-18). */
type Tab<D> = NonNullable<Moves<D>["opens"]>;

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const db = (x: number) => 20 * Math.log10(Math.max(1e-6, x));

// ---------------------------------------------------------------- the levels over time

/**
 * A gate's last 3 seconds (CARD-112): in across, from −80 to 0 dB up, the level going in (faint)
 * and coming out, the level going in now as a bar, and whether it was open, a strip along the foot
 * lit where its meter `open` said so (a meter of a switch, CARD-73). Where it opens is a line on it.
 */
export function levelsPicture(): PictureDecl<"seconds", "db"> {
  return view.picture({
    x: axis.time(3),
    y: axis.lin("db", -80, 0),
    live: [live.history(live.level("in"), { tone: "faint" }), live.history(live.level("out")), live.level("in"), live.history(live.meter("open"))],
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

/** What a gate's `levels` draws, in words (CARD-112). */
export function gateWords(v: Values, names: { threshold: string; range: string }): string {
  const [t, r] = [v[names.threshold], v[names.range]];
  if (typeof t !== "number") return "The last 3 seconds in and out: its threshold is set by a cable or the code. The strip at the foot lights while it is open";
  const down = typeof r === "number" ? dbWords(r) : "a level set elsewhere";
  return `Opens when the sound reaches ${dbWords(t)}; below that it turns it down to ${down}. The strip at the foot lights while it is open. Drag the line`;
}

/** The most a gate's `down` shows, in dB: the deepest range its card allows. */
export const GATE_MOST = 80;

/**
 * How far a gate turns the sound down (CARD-112), on std's reduction picture: its meter as it plays
 * (the picture's live layers), and its range, the most it turns down, as a guide line.
 */
export function gateDown<D>(p: Painter<D, "seconds", "db">, v: Values, names: { range: Num<D> }): Shape[] {
  const r = v[names.range];
  if (r === undefined) throw new Error(`names ${String(names.range)}, which this sound doesn't have`);
  return reduction(p, { most: typeof r === "number" ? -r : null, top: GATE_MOST });
}

/** What a gate's `down` draws, in words. */
export function gateDownWords(v: Values, names: { range: string }): string {
  const r = v[names.range];
  const most = typeof r === "number" ? `up to ${dbWords(Math.abs(r))}` : "as far as its range says";
  return `How far it turns the sound down: none while it is open, ${most} once it closes`;
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
export type Heard = { level: number[]; start: number[]; tail: number[]; before: number[] };
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
  const out: Heard = { level: [], start: [], tail: [], before: [] };
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
  // Each follower's step, written out in the loop (a call costs more than the step): toward
  // `level` by its coefficient, landing on it once it holds still and the step no longer moves it.
  const last = HIT_SECONDS * RATE;
  for (let n = 0; n <= last; n++) {
    // Once the hit falls with no start left and all of a tail, it stays so: the level only falls,
    // the riser falls with it, and the faller, 0.3 s slow, only falls further behind. The rest is
    // the hit's own fall.
    const i = out.start.length - 1;
    if (!attacking && n % EACH === 0 && i > 0 && out.start[i] === 0 && out.tail[i] >= 1) {
      for (let m = n; m <= last; m += EACH) {
        out.level.push(x * fall ** (m - n + 1));
        out.start.push(0);
        out.tail.push(1);
      }
      break;
    }
    // From 2 ms the hit only falls, so the window's loudest steps only at each millisecond's end:
    // each millisecond is worked out at once, as the kernel's samples sum (`falling`).
    if (n === 2 * EACH + 1) {
      falling(n - 1);
      break;
    }
    if (attacking) {
      x += rise;
      if (x >= 1) {
        x = 1;
        attacking = false;
      }
    } else x *= fall;
    let loudest: number;
    if (x === 0 && now === 0 && finished === 0) {
      at = 0;
      left = len;
      loudest = 0;
    } else {
      if (x > now) now = x;
      loudest = now > finished ? now : finished;
      if (--left === 0) {
        pieces[at] = now;
        at = (at + 1) % PIECES;
        finished = 0;
        for (let i = 0; i < PIECES; i++) if (pieces[i] > finished) finished = pieces[i];
        now = 0;
        left = len;
      }
    }
    if (glided !== loudest) {
      const y = loudest + (glided - loudest) * glide;
      glided = loudest === glidedLast && (y === glided || Math.abs(y - loudest) <= 1e-6 * Math.abs(loudest)) ? loudest : Math.abs(y) < 1e-15 ? 0 : y;
    }
    glidedLast = loudest;
    const level = glided;
    if (level <= riser) riser = riserLast = level;
    else {
      const y = level + (riser - level) * slow;
      riser = level === riserLast && (y === riser || Math.abs(y - level) <= 1e-6 * Math.abs(level)) ? level : Math.abs(y) < 1e-15 ? 0 : y;
      riserLast = level;
    }
    if (level >= faller) faller = fallerLast = level;
    else {
      const y = level + (faller - level) * hang;
      faller = level === fallerLast && (y === faller || Math.abs(y - level) <= 1e-6 * Math.abs(level)) ? level : Math.abs(y) < 1e-15 ? 0 : y;
      fallerLast = level;
    }
    if (n % EACH === 0) {
      out.level.push(x);
      out.start.push(Math.min(1, (db(level) - db(riser)) / 6));
      out.tail.push(Math.min(1, (db(faller) - db(level)) / 6));
    }
  }
  /**
   * The hit's sums after sample `n0` (a millisecond's start, already summed), a millisecond at a time,
   * once the hit only falls: the window's loudest finished piece holds through each millisecond but
   * its last sample; the
   * glide approaches it by its pole each sample; the riser rises toward the level by its pole until
   * the falling level meets it, then falls with it; the faller follows the level by its own pole;
   * each worked out as the sum of its samples' steps.
   */
  function falling(n0: number) {
    const g47 = glide ** (len - 1);
    // A follower of pole `h` after `k` samples of a level gliding from `g0` toward `l`, from `f0`.
    const after = (f0: number, h: number, g0: number, l: number, k: number) =>
      h ** k * f0 + l * (1 - h ** k) + ((1 - h) * (g0 - l) * glide * (h ** k - glide ** k)) / (h - glide);
    for (let n = n0 + EACH; n <= last; n += EACH) {
      // The samples n − 47 to n − 1 chase the window's loudest as it was; the piece of the
      // millisecond that began at n − 48 finishes at n − 1, its loudest its first sample.
      const before = finished;
      // The millisecond's first sample is its loudest: the hit only falls.
      const first = x;
      x *= fall ** EACH;
      // The faller follows a falling level, and is set to a rising one.
      const fell = after(faller, hang, glided, before, len - 1);
      // The riser rises toward the level until the falling level meets it, then falls with it.
      const rose = riser < glided ? after(riser, slow, glided, before, len - 1) : riser;
      glided = before + (glided - before) * g47;
      riser = Math.min(rose, glided);
      faller = Math.max(fell, glided);
      pieces[at] = first;
      at = (at + 1) % PIECES;
      finished = 0;
      for (let i = 0; i < PIECES; i++) if (pieces[i] > finished) finished = pieces[i];
      // Sample n: the new loudest, one step of each.
      glided = finished + (glided - finished) * glide;
      faller = glided >= faller ? glided : glided + (faller - glided) * hang;
      riser = glided <= riser ? glided : glided + (riser - glided) * slow;
      const start = Math.min(1, (db(glided) - db(riser)) / 6), tail = Math.min(1, (db(faller) - db(glided)) / 6);
      out.level.push(x);
      out.start.push(start);
      out.tail.push(tail);
      if (start === 0 && tail >= 1) {
        for (let m = n + EACH; m <= last; m += EACH) {
          out.level.push(x * fall ** (m - n));
          out.start.push(0);
          out.tail.push(1);
        }
        return;
      }
    }
  }
  // The hit as it goes in, drawn the same at any values: from its first millisecond (a log axis).
  for (let i = 1; i < out.level.length; i++) out.before.push(i / 1000, Math.max(-120, db(out.level[i])));
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
  const { level, start, tail, before } = heard();
  const after: number[] = [];
  for (let i = 1; i < level.length; i++) {
    const y = before[2 * i - 1];
    after.push(before[2 * i - 2], level[i] > 0 ? y + Math.min(24, Math.max(-24, a * start[i] + s * tail[i])) : -120);
  }
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

/** The most a de-esser's `down` shows, in dB: the largest amount its card allows. */
export const BAND_MOST = 24;

/**
 * How far a de-esser turns its band down (CARD-114), on std's reduction picture: its meter as it
 * plays, and its amount, the most it turns down, as a guide line.
 */
export function bandDown<D>(p: Painter<D, "seconds", "db">, v: Values, names: { amount: Num<D> }): Shape[] {
  const a = v[names.amount];
  if (a === undefined) throw new Error(`names ${String(names.amount)}, which this sound doesn't have`);
  return reduction(p, { most: typeof a === "number" ? a : null, top: BAND_MOST });
}

/** What a de-esser's `down` draws, in words. */
export function bandDownWords(v: Values, names: { freq: string; threshold: string; amount: string }): string {
  const [f, t, a] = [v[names.freq], v[names.threshold], v[names.amount]];
  const at = typeof f === "number" ? hzWords(f) : "its frequency";
  const most = typeof a === "number" ? `up to ${dbWords(a)}` : "up to its amount, set elsewhere";
  const over = typeof t === "number" ? dbWords(t) : "its threshold";
  return `How far it turns the band down at ${at}: ${most}, while the band's peaks are over ${over}`;
}

// ---------------------------------------------------------------- the multiband compressor

/** The bands' names, low to high: the card's groups. */
const BANDS = ["Low", "Mid", "High"] as const;

/**
 * The click the split is measured with, in dB: under the lowest threshold its card allows (−60 dB),
 * so its compressors rest however they are set (CARD-115).
 */
const UNDER = -80;

/**
 * A multiband compressor's split (CARD-115), on `curvePicture()`'s axes and its spectrum in and out,
 * and how far each band turns down now, its meters as a bar each at the picture's right, from 0 dB
 * up, low, mid and high side by side (CARD-73).
 */
export function splitPicture(): PictureDecl<"hz", "db"> {
  return view.picture({
    x: axis.log("hz", 20, 20000),
    y: axis.lin("db", -24, 24),
    live: [live.spectrum("in", { tone: "faint" }), live.spectrum("out"), ...BANDS.map((b) => live.meter(`${b.toLowerCase()}_reduction`))],
  });
}

/**
 * A multiband compressor's split (CARD-115), on `splitPicture()`: how it turns each frequency below
 * its thresholds, the line `split`, measured from its own sound with a click at −80 dB
 * (`p.response`); the two crossovers as guide lines, a dot on each that moves it across; and a dot
 * in each band, at its gain, that moves the gain up and down and shows its tab. How far each band
 * turns down as it plays is its kernel's meter, a live layer of the picture.
 */
export function split<D>(
  p: Painter<D, "hz", "db">,
  v: Values,
  names: { lowMid: In<D, "hz">; midHigh: In<D, "hz">; gains: readonly [In<D, "db">, In<D, "db">, In<D, "db">]; tabs: readonly [Tab<D>, Tab<D>, Tab<D>] },
): Shape[] {
  const a = known(v, names.lowMid, "low to mid crossover"), b = known(v, names.midHigh, "mid to high crossover");
  // Crossed, the lower one splits low from mid (the kernel's own rule).
  const [lm, mh] = [Math.min(a, b), Math.max(a, b)];
  const g = names.gains.map((n, i) => known(v, n, `${BANDS[i].toLowerCase()} gain`));
  const edges = [20, lm, mh, 20000];
  const centre = (i: number) => Math.sqrt(edges[i] * edges[i + 1]);
  return [
    p.response({ name: "split", level: UNDER }),
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
  if (typeof a !== "number" || typeof b !== "number") return `Lows, mids and highs, each compressed on its own: a crossover is set by a cable or the code. ${BARS}`;
  const g = names.gains.map((n) => (typeof v[n] === "number" ? dbWords(v[n] as number) : "set elsewhere"));
  return `Lows below ${hzWords(Math.min(a, b))}, mids to ${hzWords(Math.max(a, b))}, highs above, each compressed on its own, then turned ${g[0]}, ${g[1]} and ${g[2]}: as it sounds while quieter than its thresholds. ${BARS}`;
}

/** What the multiband's bars show, in words. */
const BARS = "The bars at the right: how far the lows, mids and highs are turned down now";

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
