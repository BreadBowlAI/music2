// @m2/effects' pictures (CARD-117 to CARD-124, CARD-127 to CARD-129): what each of its cards draws, in TypeScript
// over the picture words (CARD-61), with no kernel. Each but the Clipper's is drawn from the effect's own sums, since none
// can be measured with a click (CARD-82: a delay's or a reverb's click rings past 2 s, drive changes
// with how loud it is, the moving effects change over time), and each is checked against a render
// of the sound within 0.5 dB (CARD-99), at 44.1, 48 and 96 kHz. Each is worked out at the song's
// sample rate (`p.rate`, CARD-87), as its kernel works, its words too. The Flanger's and Grains' carry
// a live line too, where their own kernels say they are now (their meters `phase` and `reads`).
//
// Each shape function takes the card's `p`, so TypeScript checks the names of the values each dot
// moves against the sound's own descriptor, in the axis's unit (CARD-62).

import { axis, knob, live, view, type Moves, type Painter, type PictureDecl, type Shape, type Unit } from "@m2/core";
import { dbWords, figures, hzWords, known, secondsWords, type In, type Num, type Read, type Values } from "std/pictures";

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const db = (x: number) => 20 * Math.log10(Math.max(1e-6, x));
/** The nearest 32-bit float to a number: what a kernel holds. */
const f32 = Math.fround;

// ---------------------------------------------------------------- delay

/** The longest a delay's echoes are drawn, in seconds. */
const ECHO_SECONDS = 8;
/** The quietest echo drawn, in dB. */
const FLOOR = -60;

/** A delay's echoes (CARD-117): seconds across, −60 to 0 dB up. */
export function echoesPicture(): PictureDecl<"seconds", "db"> {
  return view.picture({ x: axis.time(ECHO_SECONDS), y: axis.lin("db", FLOOR, 0) });
}

/** The level of each sound a click makes through the delay at the song's rate `sr`, in dB: the click itself, then each echo, to −60 dB or 8 s. */
export function echoLevels(time: number, feedback: number, mix: number, sr: number): { at: number; db: number }[] {
  const t = clamp(time, 1 / sr, 2), fb = clamp(feedback, 0, 0.95), m = clamp(mix, 0, 1);
  const out = [{ at: 0, db: db(1 - m) }];
  for (let k = 1, level = m; k * t <= ECHO_SECONDS && db(level) > FLOOR; k++, level *= fb) out.push({ at: k * t, db: db(level) });
  return out;
}

/**
 * A delay's echoes (CARD-117), from its own sum: a click (faint, as loud as the mix leaves it), then
 * an echo each `time`, the first as loud as `mix`, each next one `feedback` times the last, as
 * bars down to −60 dB; a dot on the first echo that moves the time across and the mix as its knob
 * does (up, more), and one on the second that moves the feedback as its knob does, each resting at
 * the foot when its echo is under it.
 */
export function echoes<D>(p: Painter<D, "seconds", "db">, v: Values, names: { time: In<D, "seconds">; feedback: Num<D>; mix: Num<D> }): Shape[] {
  const time = known(v, names.time, "time"), fb = known(v, names.feedback, "feedback"), mix = known(v, names.mix, "mix");
  const all = echoLevels(time, fb, mix, p.rate);
  const bar = (e: { at: number; db: number }) => [e.at, Math.max(FLOOR, e.db)];
  // Each dot rests at the foot when its echo is under it (a mix of 0), so it can be dragged back up (review T9).
  const t = clamp(time, 1 / p.rate, 2);
  const first = all[1] ?? { at: t, db: FLOOR }, second = all[2] ?? { at: 2 * t, db: FLOOR };
  return [
    p.bars(bar(all[0]), { base: FLOOR, tone: "faint" }),
    p.bars(all.slice(1).flatMap(bar), { base: FLOOR }),
    p.handle(first.at, Math.max(FLOOR, first.db), { x: names.time, y: knob(names.mix) } as unknown as Moves<D, "seconds", "db">),
    ...(second.at <= ECHO_SECONDS ? [p.handle(second.at, Math.max(FLOOR, second.db), { y: knob(names.feedback), wheel: names.feedback } as unknown as Moves<D, "seconds", "db">)] : []),
  ];
}

/** What `echoes` draws, in words, at the song's rate `sr` (the words' `p.rate`). */
export function echoesWords(v: Values, names: { time: string; feedback: string; mix: string }, sr: number): string {
  const [t, fb, m] = [v[names.time], v[names.feedback], v[names.mix]];
  if (typeof t !== "number" || typeof fb !== "number" || typeof m !== "number") return "Echoes that repeat and fade: one of its values is set by a cable or the code";
  const n = echoLevels(t, fb, m, sr).length - 1;
  const fading = fb > 0 ? `, each ${dbWords(-db(clamp(fb, 0, 0.95)))} quieter` : ", only one";
  return `An echo every ${secondsWords(t)}${fading}: ${n} ${n === 1 ? "echo" : "echoes"} before −60 dB. Drag the first for its time and mix, the second for the feedback`;
}

// ---------------------------------------------------------------- reverb

/** The reverb's comb and all-pass lengths at 44.1 kHz (Freeverb's), its lows cut at 200 Hz: the kernel's own. */
const COMBS = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES = [556, 441, 341, 225];
const CUT = 200;
/** The longest a tail is drawn, in seconds. */
const TAIL_SECONDS = 12;
/**
 * The frequencies the tail is summed over: 32 bands, each the same share of an octave, from 100 Hz
 * to half the rate, so the lows just above the cut, which ring longest in a damped room, are many.
 */
const BANDS = 32;
const LOWEST = 100;
/** How many times round its combs the eight of them cancel or add, each time, in the all-passes: see `room`. */
const TOGETHER = 10;

/** A reverb's tail (CARD-118): seconds across, −60 to 0 dB up. */
export function tailPicture(): PictureDecl<"seconds", "db"> {
  return view.picture({ x: axis.time(TAIL_SECONDS), y: axis.lin("db", -60, 0) });
}

/** What every tail is drawn with, whatever its size and damping: see `room`. */
export type Room = { before: number[]; grid: number; together: number[] };
/** `room`'s sums at each song's rate, once worked out; and how many times they have been (for the test that a build never does). */
const rooms = new Map<number, Room>();
let worked = 0;
/** How many times this module has worked out its sums: a song's build never does (review T1). */
export const sumsWorked = () => worked;

/** The length at the song's rate `sr` of one of the reverb's combs or all-passes, in samples: the kernel's own sum and rounding, in 32-bit floats. */
const lengthAt = (l: number, sr: number) => Math.floor(f32(f32(l * sr) / 44_100));

/**
 * What the reverb's four all-passes do to its combs' sound, the same for any size and damping, so
 * worked out on a tail's first draw and kept (never as a song builds, which loads this module too):
 * - `before`: where they send each moment of it, and how much: each all-pass gives what goes in at
 *   once, upside down, then again after its length, then half of that after each length more (the
 *   kernel's own sum); their energies, one after the other, gathered in 1 ms, as the share of it all
 *   that comes before each `grid` (a quarter of a millisecond).
 * - `together`: how loud the eight combs' n-th times round are together, against eight apart, for
 *   n from 1 to 40: the combs ring the same click, so where their times round, through the
 *   all-passes, land on the same samples, they cancel or add. Worked out from the all-passes' own
 *   echoes of themselves: each all-pass's is 7/3 at once and −(2/3) × 0.5^j at j lengths either way,
 *   to 4 lengths (within 0.02 dB of every length).
 */
export function room(sr: number): Room {
  const kept = rooms.get(sr);
  if (kept) return kept;
  worked += 1;
  const stages = ALLPASSES.map((l) => lengthAt(l, sr)), combs = COMBS.map((l) => lengthAt(l, sr));
  // The energies, each all-pass after the last, gathered in 1 ms as they go (each bin its energy
  // and its energy × time), as far as 0.5 s.
  const ms = sr / 1000, bins = 500;
  let energy = new Float64Array(bins), timed = new Float64Array(bins);
  energy[0] = 1;
  for (const m of stages) {
    const e2 = new Float64Array(bins), t2 = new Float64Array(bins);
    for (let b = 0; b < bins; b++) {
      const e = energy[b];
      if (e === 0) continue;
      const at = timed[b] / e;
      for (let k = 0, a2 = 1; a2 > 1e-6; k++, a2 = k === 1 ? 1 : a2 * 0.25) {
        const when = at + k * m, bin = Math.floor(when / ms);
        if (bin >= bins) break;
        e2[bin] += e * a2;
        t2[bin] += e * a2 * when;
      }
    }
    energy = e2;
    timed = t2;
  }
  // How much of it all comes before each moment, every quarter of a millisecond, from 0 to 1:
  // `before[i]` is the share that comes before i quarters.
  const grid = sr / 4000;
  let all = 0;
  for (let b = 0; b < bins; b++) all += energy[b];
  const quarters = new Float64Array(4 * bins + 1);
  for (let b = 0; b < bins; b++) if (energy[b] > 0) quarters[Math.floor(timed[b] / energy[b] / grid) + 1] += energy[b] / all;
  for (let i = 1; i < quarters.length; i++) quarters[i] += quarters[i - 1];
  // Past where all but a ten-thousandth has come (40 dB under it all), it has all come.
  let end = 1;
  while (end < quarters.length && quarters[end - 1] <= 1 - 1e-4) end++;
  const before = Array.from(quarters.subarray(0, end));
  before[end - 1] = 1;
  // The all-passes' echo of themselves, at every lag, to 4 lengths of each: the first two's
  // together as a list of lags, the last two's, each pair of them added at each.
  const own = (j: number) => (j === 0 ? 7 / 3 : -(2 / 3) * 0.5 ** Math.abs(j));
  const lags: number[] = [], values: number[] = [];
  for (let i = -4; i <= 4; i++)
    for (let j = -4; j <= 4; j++) {
      const x = own(i) * own(j);
      if (Math.abs(x) > 1e-2) {
        lags.push(i * stages[0] + j * stages[1]);
        values.push(x);
      }
    }
  const reach = 4 * (stages[0] + stages[1] + stages[2] + stages[3]);
  const echo = new Float64Array(2 * reach + 1);
  for (let i = -4; i <= 4; i++)
    for (let j = -4; j <= 4; j++) {
      const x = own(i) * own(j), at = reach + i * stages[2] + j * stages[3];
      for (let k = 0; k < lags.length; k++) echo[at + lags[k]] += x * values[k];
    }
  const whole = echo[reach];
  const at = (lag: number) => (Math.abs(lag) <= reach ? echo[reach + lag] : 0);
  const together: number[] = [];
  for (let n = 1; n <= TOGETHER; n++) {
    let sum = combs.length * whole;
    for (let a = 0; a < combs.length; a++) for (let b = a + 1; b < combs.length; b++) sum += 2 * at(n * (combs[a] - combs[b]));
    together.push(sum / (combs.length * whole));
  }
  const made = { before, grid, together };
  rooms.set(sr, made);
  return made;
}

/**
 * How loud what is left of a reverb's tail is `t` seconds in, in dB from its start, at the song's
 * rate `sr`: all the sound still to come after that time, as a click's tail gives it (its energy
 * from then on). By the kernel's own sums: a click with its lows below 200 Hz cut goes into eight combs, each sending
 * back `0.7 + 0.28 × size` of what went round, through a one-pole low-pass of `0.4 × damping` that
 * makes the highs fade sooner; then through the four all-passes (`room`). Each comb's energy at
 * each frequency is the loop's gain, squared, smaller each time round it, and its first times round
 * are as loud together as `room` says.
 */
export function tailLevel(size: number, damping: number, sr: number): (t: number) => number {
  // The last tail asked for is kept, so a draw and its words work its sums out once.
  if (last && last.size === size && last.damping === damping && last.sr === sr) return last.level;
  const { before, grid, together } = room(sr);
  const fb = 0.7 + 0.28 * clamp(size, 0, 1), d = 0.4 * clamp(damping, 0, 1);
  const lengths = COMBS.map((l) => lengthAt(l, sr));
  // Each band's share of a click (its lows cut) and its loop's gain, squared.
  const share: number[] = [], gain: number[] = [];
  for (let i = 0; i < BANDS; i++) {
    const [a, b] = [LOWEST * (sr / 2 / LOWEST) ** (i / BANDS), LOWEST * (sr / 2 / LOWEST) ** ((i + 1) / BANDS)];
    const f = Math.sqrt(a * b), r = (f / CUT) ** 4;
    const g = (fb * (1 - d)) ** 2 / (1 - 2 * d * Math.cos((2 * Math.PI * f) / sr) + d * d);
    share.push(((b - a) * r) / (1 + r));
    gain.push(g);
  }
  // What is still to come from the k-th time round on (k from 0): every later time round's energy,
  // summed over the bands, the first `TOGETHER` as loud together as `room` says; worked out as far as
  // it is asked for, from what is left of each band's whole ringing.
  const ringing = share.map((x, i) => x / (1 - gain[i]));
  const near: number[] = [];
  for (let k = 0, now = share.slice(); k < TOGETHER; k++) {
    let sum = 0;
    for (let i = 0; i < BANDS; i++) {
      sum += now[i];
      now[i] *= gain[i];
    }
    near.push(sum * (together[k] - 1));
  }
  for (let k = TOGETHER - 2; k >= 0; k--) near[k] += near[k + 1];
  const after: number[] = [];
  const upTo = (k: number) => {
    while (after.length <= k) {
      let sum = after.length < TOGETHER ? near[after.length] : 0;
      for (let i = 0; i < BANDS; i++) {
        sum += ringing[i];
        ringing[i] *= gain[i];
      }
      after.push(sum);
    }
  };
  // The share of the all-passes' spread that comes before `when` samples: `before`, every `grid`.
  const top = before.length - 1, span = before.length * grid, shortest = Math.min(...lengths);
  // What is left at `t`: for each comb, what comes after `t` of each time round, by where in the
  // spread it lands: the k-th time round's (from 0) for what comes k to k + 1 lengths before `t`.
  // Each share is read once, as the end of one span and the start of the next.
  const left = (t: number) => {
    const now = t * sr;
    upTo(Math.ceil(now / shortest) + 1);
    let e = 0;
    for (let c = 0; c < lengths.length; c++) {
      const l = lengths[c];
      let w = now - l;
      let s = w <= 0 ? 0 : before[Math.min(top, Math.floor(w / grid))];
      e += after[0] * (1 - s);
      let k = Math.max(1, Math.floor((now - span) / l));
      w = now - k * l;
      s = w <= 0 ? 0 : before[Math.min(top, Math.floor(w / grid))];
      for (; w > 0; k++) {
        w -= l;
        const next = w <= 0 ? 0 : before[Math.min(top, Math.floor(w / grid))];
        e += after[k] * (s - next);
        s = next;
      }
    }
    return e;
  };
  const whole = left(0);
  const level = (t: number) => 10 * Math.log10(Math.max(1e-12, left(t) / whole));
  last = { size, damping, sr, level };
  return level;
}
/** The last tail `tailLevel` worked out. */
let last: { size: number; damping: number; sr: number; level: (t: number) => number } | null = null;

/**
 * When a tail of `level` has fallen `down` dB, in seconds: `TAIL_SECONDS` at most. Doubled from a
 * quarter of a second until past it, then found between by where a straight line through the two
 * ends crosses it (the tail falls nearly straight in dB), each end moved in turn, to a millisecond
 * or 0.02 dB.
 */
export function tailFalls(level: (t: number) => number, down: number): number {
  let lo = 0, hi = 0.25, ylo = 0, yhi = level(hi);
  while (yhi > -down) {
    if (hi >= TAIL_SECONDS) return TAIL_SECONDS;
    [lo, ylo] = [hi, yhi];
    hi = Math.min(TAIL_SECONDS, 2 * hi);
    yhi = level(hi);
  }
  for (let i = 0; i < 12 && hi - lo > 0.001; i++) {
    // Where the line crosses, kept a tenth of the way in from either end, so each step halves at worst.
    const across = (ylo + down) / (ylo - yhi);
    const t = lo + (hi - lo) * Math.min(0.9, Math.max(0.1, across));
    const y = level(t);
    if (Math.abs(y + down) < 0.02) return t;
    if (y > -down) [lo, ylo] = [t, y];
    else [hi, yhi] = [t, y];
  }
  return hi;
}

/** How many points a tail is drawn with. */
const TAIL_POINTS = 48;

/** A tail's points at the song's rate `sr`, to where it has fallen 60 dB, and where it has fallen 30 dB: the last asked for kept, for a draw and its words. */
function tailShape(size: number, damping: number, sr: number): { points: number[]; half: number } {
  if (shape && shape.size === size && shape.damping === damping && shape.sr === sr) return shape;
  const level = tailLevel(size, damping, sr);
  const end = tailFalls(level, 60);
  const points: number[] = [];
  for (let i = 0; i < TAIL_POINTS; i++) {
    const t = (i / (TAIL_POINTS - 1)) * end;
    points.push(t, Math.max(-60, level(t)));
  }
  // Where it has fallen 30 dB: between the two points either side, in a straight line.
  let half = end;
  for (let i = 2; i < points.length; i += 2)
    if (points[i + 1] <= -30) {
      const [t0, y0, t1, y1] = [points[i - 2], points[i - 1], points[i], points[i + 1]];
      half = t0 + ((t1 - t0) * (-30 - y0)) / (y1 - y0);
      break;
    }
  shape = { size, damping, sr, points, half };
  return shape;
}
let shape: { size: number; damping: number; sr: number; points: number[]; half: number } | null = null;

/**
 * A reverb's tail (CARD-118), from its own sums (`tailLevel`): how loud what is left of it is,
 * from 0 dB at its start down to −60 dB, a faint fill and the line `tail`; one dot where it has
 * fallen 30 dB, that moves the size as its knob does (to the right, longer) and by the wheel.
 */
export function tail<D>(p: Painter<D, "seconds", "db">, v: Values, names: { size: Num<D>; damping: Num<D> }): Shape[] {
  const { points, half } = tailShape(known(v, names.size, "size"), known(v, names.damping, "damping"), p.rate);
  return [
    p.fill(points, { base: -60, tone: "faint" }),
    p.line(points, { name: "tail" }),
    p.handle(half, -30, { x: knob(names.size), wheel: names.size } as unknown as Moves<D, "seconds", "db">),
  ];
}

/** What `tail` draws, in words, at the song's rate `sr` (the words' `p.rate`): the time its dot shows, from the tail the draw kept. */
export function tailWords(v: Values, names: { size: string; damping: string }, sr: number): string {
  const [s, d] = [v[names.size], v[names.damping]];
  if (typeof s !== "number" || typeof d !== "number") return "A room's tail as it fades: its size or damping is set by a cable or the code";
  const highs = d > 0 ? `; damping ${figures(d)} makes its highs fade sooner` : "";
  return `Its tail fades 30 dB in ${secondsWords(tailShape(s, d, sr).half)}${highs}. Drag the dot for the size`;
}

// ---------------------------------------------------------------- moving effects

/** A second of an effect's motion (CARD-119 to CARD-121): seconds across, its own unit up. */
export function motionPicture<U extends Unit>(unit: U, lo: number, hi: number, log = false): PictureDecl<"seconds", U> {
  return view.picture({ x: axis.time(1), y: log ? axis.log(unit, lo, hi) : axis.lin(unit, lo, hi) });
}

/** How many points a second of motion is drawn with. */
const MOTION_POINTS = 241;
/** A second's times. */
const second = () => Array.from({ length: MOTION_POINTS }, (_, i) => i / (MOTION_POINTS - 1));

/** Where an LFO is `t` seconds in, from 0 to 1 of its cycle, from the song's start, at the song's rate `sr` (at most `most` cycles a second): the exact phase, which the phaser's and the tremolo's sums use. */
const phaseAt = (rate: number, t: number, sr: number, most = 20) => ((clamp(rate, 0, most) * Math.round(t * sr)) / sr) % 1;

/**
 * Where the chorus's LFO is, from 0 to 1 of its cycle, as its kernel's `Phase` counts it from the
 * song's start: `rate` (at most `most` cycles a second) over the song's rate `sr` added each sample,
 * in 32-bit floats, so it drifts from the exact phase as the kernel's does (most at a high rate:
 * about 5 × 10⁻⁴ of a cycle by 0.8 s at 96 kHz, which moves the chorus's level near its notch by
 * 0.6 dB). The function given is the phase at sample `n`, asked for `n` that only grow. A 32-bit
 * float in [2^e, 2^(e+1)) is a whole number of 2^(e − 23), so each step inside it adds the step
 * rounded to that: the phase is walked a run of such steps at a time.
 */
function phases(rate: number, sr: number, most: number): (n: number) => number {
  const step = f32(f32(clamp(rate, 0, most)) / f32(sr));
  let at = 0, done = 0;
  const one = () => {
    at = f32(at + step);
    if (at >= 1) at = f32(at - 1);
    done += 1;
  };
  return (n) => {
    while (done < n && step > 0) {
      if (at === 0) {
        one();
        continue;
      }
      let e = Math.floor(Math.log2(at));
      if (2 ** e > at) e -= 1;
      else if (2 ** (e + 1) <= at) e += 1;
      const ulp = 2 ** (e - 23), q = step / ulp;
      // A step half way between two floats rounds to the even one, which hangs on where it lands.
      if (q % 1 === 0.5) {
        one();
        continue;
      }
      const s = Math.round(q) * ulp;
      const inside = s > 0 ? Math.max(0, Math.ceil((2 ** (e + 1) - at) / s) - 1) : n - done;
      const k = Math.min(inside, n - done);
      at += k * s;
      done += k;
      if (done < n) one();
    }
    return at;
  };
}

/** The chorus's two copies (std:chorus): each `base` plus up to `depth` × `sweep` seconds late. */
const CHORUS = { base: 0.008, sweep: 0.006 } as const;

/** How late each of the chorus's two copies is with its LFO at `phase`, in seconds (std:chorus's own sum). */
export function chorusDelays(depth: number, phase: number): [number, number] {
  const s = Math.sin(2 * Math.PI * phase), w = clamp(depth, 0, 1) * CHORUS.sweep;
  return [CHORUS.base + w * (0.5 + 0.5 * s), CHORUS.base + w * (0.5 - 0.5 * s)];
}

/**
 * A chorus's motion over a second (CARD-119): how late each of its two copies is, in seconds, the
 * lines `one` and `two`, drifting apart and back `rate` times a second, by up to `depth` × 6 ms.
 */
export function chorusMotion<D>(p: Painter<D, "seconds", "seconds">, v: Values, names: { rate: Num<D>; depth: Num<D> }): Shape[] {
  const rate = known(v, names.rate, "rate"), depth = known(v, names.depth, "depth");
  const phase = phases(rate, p.rate, 20);
  const one: number[] = [], two: number[] = [];
  for (const t of second()) {
    const [a, b] = chorusDelays(depth, phase(Math.round(t * p.rate)));
    one.push(t, a);
    two.push(t, b);
  }
  return [p.line(one, { name: "one" }), p.line(two, { name: "two", tone: "soft" })];
}

/** The phaser's sweep (its kernel's): around 640 Hz, 2.5 octaves either way at full depth. */
const PHASER = { centre: 640, octaves: 2.5 } as const;

/**
 * Where the phaser's notches are `t` seconds in, at the song's rate `sr`, in Hz
 * (`@m2/effects:phaser`'s own sum): six all-pass stages around a frequency that sweeps `rate` times
 * a second, mixed with the sound, cut the sound where their turn is half a cycle, at three places
 * round that frequency.
 */
export function notchesAt(rate: number, depth: number, t: number, sr: number): [number, number, number] {
  const s = Math.sin(2 * Math.PI * phaseAt(rate, t, sr));
  const f = clamp(PHASER.centre * 2 ** (PHASER.octaves * clamp(depth, 0, 1) * s), 10, 0.45 * sr);
  const g = Math.tan((Math.PI * f) / sr), back = sr / Math.PI;
  return [back * Math.atan(g * TURNS[0]), back * Math.atan(g * TURNS[1]), back * Math.atan(g * TURNS[2])];
}
/** Where the six stages' turn is half a cycle, against the sweep's frequency: tan(π/12), tan(3π/12), tan(5π/12). */
const TURNS = [1, 3, 5].map((k) => Math.tan((k * Math.PI) / 12));

/** A phaser's motion over a second (CARD-120): its three notches across frequency, the lines `low`, `mid` and `high`. */
export function phaserMotion<D>(p: Painter<D, "seconds", "hz">, v: Values, names: { rate: Num<D>; depth: Num<D> }): Shape[] {
  const rate = known(v, names.rate, "rate"), depth = known(v, names.depth, "depth");
  const lines: number[][] = [[], [], []];
  for (const t of second()) {
    const [low, mid, high] = notchesAt(rate, depth, t, p.rate);
    lines[0].push(t, low);
    lines[1].push(t, mid);
    lines[2].push(t, high);
  }
  return [p.line(lines[0], { name: "low", tone: "soft" }), p.line(lines[1], { name: "mid" }), p.line(lines[2], { name: "high", tone: "soft" })];
}

/** How loud the tremolo leaves a sound `t` seconds in, at the song's rate `sr`, from 0 to 1 (its kernel's own sum). */
export function tremoloAt(rate: number, depth: number, t: number, sr: number): number {
  return 1 - clamp(depth, 0, 1) * (0.5 - 0.5 * Math.cos(2 * Math.PI * phaseAt(rate, t, sr, 40)));
}

/** A tremolo's motion over a second (CARD-121): how loud it leaves the sound, the line `level`. */
export function tremoloMotion<D>(p: Painter<D, "seconds", "unit01">, v: Values, names: { rate: Num<D>; depth: Num<D> }): Shape[] {
  const rate = known(v, names.rate, "rate"), depth = known(v, names.depth, "depth");
  const points = second().flatMap((t) => [t, tremoloAt(rate, depth, t, p.rate)]);
  return [p.fill(points, { base: 0, tone: "faint" }), p.line(points, { name: "level" })];
}

// ---------------------------------------------------------------- drive and crush

/** How std:drive shapes a sample `x` at `amount` dB: tanh(x g) / tanh(g), g the amount as a gain. */
export function driven(x: number, amount: number): number {
  const g = 10 ** (Math.max(0, amount) / 20);
  return Math.tanh(x * g) / Math.tanh(g);
}

/** A shaping curve (CARD-122): a sample going in across, −1 to 1, and coming out up. */
export function shapePicture(): PictureDecl<"bipolar", "bipolar"> {
  return view.picture({ x: axis.lin("bipolar", -1, 1), y: axis.lin("bipolar", -1, 1) });
}

/** Where drive's dot sits, going in: a quarter of the way up, where more drive lifts the curve most plainly. */
export const DRIVE_AT = 0.25;

/**
 * Drive's curve (CARD-122), from its own sum (`driven`): each sample going in, across, and coming
 * out, the line `curve`, over a faint line where in is out; a dot on it a quarter of the way in
 * that moves the amount as its knob does (up, more) and by the wheel.
 */
export function drive<D>(p: Painter<D, "bipolar", "bipolar">, v: Values, names: { amount: Num<D> }): Shape[] {
  const amount = known(v, names.amount, "amount");
  const points: number[] = [];
  for (let i = 0; i <= 200; i++) points.push(-1 + i / 100, driven(-1 + i / 100, amount));
  return [
    p.line([-1, -1, 1, 1], { tone: "faint" }),
    p.line(points, { name: "curve" }),
    p.handle(DRIVE_AT, driven(DRIVE_AT, amount), { y: knob(names.amount), wheel: names.amount } as unknown as Moves<D, "bipolar", "bipolar">),
  ];
}

/** The tone a crush is drawn on: a sine of 1 kHz, a cycle in 1 ms. */
export const CRUSH_HZ = 1000;

/**
 * One cycle of a 1 kHz sine as the bitcrusher plays it at the song's rate `sr`
 * (`@m2/effects:bitcrush`'s own sum), a value for each sample: held each time its clock, going up
 * by `rate` over the song's rate each sample (in 32-bit floats, as the kernel adds it), passes 1,
 * and rounded to `bits` (2^(bits − 1) steps each side of 0), then mixed with the sine by `mix`.
 */
export function crushed(bits: number, rate: number, mix: number, sr: number): number[] {
  const steps = 2 ** (clamp(bits, 1, 24) - 1), m = clamp(mix, 0, 1), step = f32(f32(clamp(rate, 20, sr)) / f32(sr));
  const out: number[] = [];
  let clock = 1, held = 0;
  for (let n = 0; n < sr / CRUSH_HZ; n++) {
    const x = Math.sin((2 * Math.PI * CRUSH_HZ * n) / sr);
    if (clock >= 1) [clock, held] = [f32(clock - 1), Math.round(x * steps) / steps];
    clock = f32(clock + step);
    out.push(x * (1 - m) + held * m);
  }
  return out;
}

/** One cycle of a sine (CARD-123): 1 ms across, −1 to 1 up. */
export function crushPicture(): PictureDecl<"seconds", "bipolar"> {
  return view.picture({ x: axis.time(1 / CRUSH_HZ), y: axis.lin("bipolar", -1, 1) });
}

/** The bitcrusher's cycle (CARD-123): the sine (faint) and, over it, each sample as `crushed` gives it, held for its sample: the line `crushed`. */
export function crush<D>(p: Painter<D, "seconds", "bipolar">, v: Values, names: { bits: Num<D>; rate: Num<D>; mix: Num<D> }): Shape[] {
  const samples = crushed(known(v, names.bits, "bits"), known(v, names.rate, "rate"), known(v, names.mix, "mix"), p.rate);
  const sine: number[] = [], held: number[] = [];
  for (let i = 0; i <= 96; i++) sine.push(i / 96 / CRUSH_HZ, Math.sin((2 * Math.PI * i) / 96));
  // Each sample held for its time, the last up to the cycle's end.
  samples.forEach((y, n) => held.push(n / p.rate, y, Math.min(1 / CRUSH_HZ, (n + 1) / p.rate), y));
  return [p.line(sine, { tone: "faint" }), p.line(held, { name: "crushed" })];
}

// ---------------------------------------------------------------- width

/** How far up a field's fan reaches, below the picture's top, so its dot is drawn whole. */
const FAN = 0.85;
/** What the widener makes up above width 1, from a sound's middle above 300 Hz, 11 ms late: `0.6 × (width − 1)` (its kernel's `WIDEN_SYNTH`). */
const MADE = 0.6;

/** A stereo field (CARD-124): how far apart left and right are, from 2 to the left to 2 to the right (1 is the sound as it came), its depth up. */
export function fieldPicture(): PictureDecl<"scalar", "unit01"> {
  return view.picture({ x: axis.lin("scalar", -2, 2), y: axis.lin("unit01", 0, 1) });
}

/** How wide the widener makes a mono sound, from its sound above 300 Hz: 0 at width 1 or less. */
export const madeUp = (width: number) => MADE * Math.max(0, clamp(width, 0, 2) - 1);

/**
 * A stereo field (CARD-124): a fan from the listener, at the foot, as wide as `width` either side,
 * how far apart it spreads a stereo sound's left and right (the kernel turns their difference by
 * `width`: 0 is mono, 1 the sound as it came, 2 twice as far apart), the faint lines where 1 is;
 * above width 1, a second fan, `made`, the width it makes up for a mono sound from its sound above
 * 300 Hz (`madeUp`); and a dot at the fan's right edge that moves the width across.
 */
export function field<D>(p: Painter<D, "scalar", "unit01">, v: Values, names: { width: In<D, "scalar"> }): Shape[] {
  const w = clamp(known(v, names.width, "width"), 0, 2), m = madeUp(w);
  return [
    p.line([0, 0, -1, FAN], { tone: "faint" }),
    p.line([0, 0, 1, FAN], { tone: "faint" }),
    p.fill([0, 0, -w, FAN, w, FAN], { tone: "faint" }),
    p.line([-w, FAN, 0, 0, w, FAN], { name: "field" }),
    ...(m > 0 ? [p.line([-m, FAN, 0, 0, m, FAN], { name: "made", tone: "soft" })] : []),
    p.handle(w, FAN, { x: names.width } as Moves<D, "scalar", "unit01">),
  ];
}

/** What `field` draws, in words, with no jargon. */
export function fieldWords(v: Values, names: { width: string }): string {
  const w = v[names.width];
  if (typeof w !== "number") return "How wide the sound is: its width is set by a cable or the code";
  if (w <= 0) return "Width 0: mono, left and right the same. Drag the edge to widen it";
  if (w === 1) return "Width 1: the sound as it came. Drag the edge";
  if (w < 1) return `Width ${figures(w)}: a stereo sound's left and right come ${figures(w)} times as far apart, toward mono; a mono sound stays as it is. Drag the edge`;
  return `Width ${figures(w)}: a stereo sound's left and right spread ${figures(w)} times as far apart; a mono sound gets a made-up width of ${figures(madeUp(w))} from its sound above 300 Hz (the faint fan). Drag the edge`;
}

// ---------------------------------------------------------------- clip

/**
 * The Clipper's last 3 seconds (CARD-127), CARD-143's pattern: the sound going into its cut, pushed
 * up by its gain (faint: its kernel's meter `pushed`, which only the kernel knows, D8), and what
 * comes out, on one axis in dB from −36 to +12, where what it cut is the faint line above the
 * ceiling's. Its shapes are std/pictures' `ceiling`: now, the ceiling's line and its dot.
 */
export function levelPicture(): PictureDecl<"seconds", "db"> {
  return view.picture({
    x: axis.time(3),
    y: axis.lin("db", -36, 12),
    live: [live.history(live.meter("pushed"), { tone: "faint" }), live.history(live.level("out"))],
  });
}

/** What the Clipper does, in words (CARD-127): its gain and its ceiling, then what its two lines are. */
export function levelWords(v: Values, names: { gain: string; ceiling: string }): string {
  const gain = v[names.gain], ceiling = v[names.ceiling];
  const at = typeof ceiling === "number" ? dbWords(ceiling) : "a ceiling set by a cable or the code";
  const pushed = typeof gain !== "number" ? "Pushes your sound up by a gain set by a cable or the code" : gain === 0 ? null : `Pushes your sound up ${dbWords(gain)}`;
  const cuts = pushed === null ? `Cuts every peak over ${at} flat` : `${pushed} and cuts every peak flat at ${at}`;
  // True of a bright sound pushed hard too (PACK-34's last step).
  const works = "It works at 4 times the song's rate, so it adds far fewer harsh tones than a plain clipper; on very bright sounds pushed hard, a few";
  return `${cuts}. ${works}. Faint: your sound ${pushed === null ? "going in" : "pushed up"}; solid: what comes out. Drag the line to move the ceiling`;
}

// ---------------------------------------------------------------- flanger

/** The Flanger's copy at its shortest, in seconds, and how many times later it is at Depth 1: its kernel's `SHORTEST` and `RANGE`. */
const FLANGER = { shortest: 0.0001, range: 100 } as const;
/** How many points a cycle of the sweep is drawn with. */
const SWEEP_POINTS = 240;

/** One cycle of the Flanger's sweep (CARD-128): the cycle across, bare; where its notches are up, 20 Hz to 20 kHz; and a line at its LFO's phase now. */
export function sweepPicture(): PictureDecl<"unit01", "hz"> {
  return view.picture({ x: axis.lin("unit01", 0, 1, { bare: true }), y: axis.log("hz", 20, 20000), live: [live.playhead("phase")] });
}

/**
 * Where the Flanger's lowest notch is at `phase` (0 to 1) of its sweep, in Hz (`@m2/effects:flanger`'s
 * own sum): its copy `0.1 ms × 100^(depth × (0.5 − 0.5 cos 2πφ))` late cuts the sound at `1 / (2 × delay)`
 * and its odd multiples, or, with Feedback below 0 (the copy turned over), at `1 / delay` and its
 * multiples. In seconds, so the same at every rate.
 */
export function notchAt(depth: number, feedback: number, phase: number): number {
  const late = FLANGER.shortest * FLANGER.range ** (clamp(depth, 0, 1) * (0.5 - 0.5 * Math.cos(2 * Math.PI * phase)));
  return (feedback < 0 ? 1 : 0.5) / late;
}

/**
 * The Flanger's sweep (CARD-128), from its own sum (`notchAt`): its lowest notch over one cycle, the
 * line `notch`, and the next two over it, faint (three and five times it, or two and three times it
 * below 0), each kept under 20 kHz.
 */
export function sweep<D>(p: Painter<D, "unit01", "hz">, v: Values, names: { depth: Num<D>; feedback: Num<D> }): Shape[] {
  const depth = known(v, names.depth, "depth"), feedback = known(v, names.feedback, "feedback");
  const teeth = feedback < 0 ? [3, 2, 1] : [5, 3, 1];
  const lines = teeth.map(() => [] as number[]);
  for (let i = 0; i <= SWEEP_POINTS; i++) {
    const phase = i / SWEEP_POINTS, notch = notchAt(depth, feedback, phase);
    teeth.forEach((k, j) => lines[j].push(phase, Math.min(20000, k * notch)));
  }
  return [p.line(lines[0], { name: "third", tone: "faint" }), p.line(lines[1], { name: "second", tone: "faint" }), p.line(lines[2], { name: "notch" })];
}

/**
 * What `sweep` draws, in words: how far its notches sweep and how often, what its feedback does,
 * and, with its loudest peak 6 dB or more over the sound, how loud it comes out (PACK-35: the comb
 * has no make-up gain, its loudest peak `(1 − mix) + mix / (1 − |feedback|)`, Mix set by a cable or
 * the code counted at 1, the most it can be).
 */
export function sweepWords(v: Values, names: { rate: string; depth: string; feedback: string; mix: string }): string {
  const [rate, depth, feedback, mix] = [v[names.rate], v[names.depth], v[names.feedback], v[names.mix]];
  const line = "The line: where they are now";
  const fb =
    typeof feedback !== "number"
      ? "Its feedback is set by a cable or the code"
      : feedback === 0
        ? "No feedback: the plain sweep"
        : feedback > 0
          ? `Feedback ${figures(feedback)} rings at the peaks between them`
          : `Feedback ${figures(feedback)} turns the copy over: its notches move half way, and its peaks ring where they were, a hollow sound`;
  let loud = "";
  if (typeof feedback === "number") {
    const m = typeof mix === "number" ? Math.min(Math.max(mix, 0), 1) : 1;
    const peak = 20 * Math.log10(1 - m + m / (1 - Math.min(Math.abs(feedback), 0.9)));
    if (peak >= 6) loud = `. It comes out up to ${dbWords(peak)} louder at its peaks: turn Mix or the track down`;
  }
  if (typeof depth !== "number") return `Its notches sweep as far as a cable or the code says. ${fb}. ${line}${loud}`;
  const turned = typeof feedback === "number" ? feedback : 0;
  const top = notchAt(depth, turned, 0), bottom = notchAt(depth, turned, 0.5);
  if (depth <= 0) return `Its notches stand still at ${hzWords(top)} and over. ${fb}. ${line}${loud}`;
  const how =
    typeof rate !== "number" ? "as fast as a cable or the code says" : rate > 1 ? `${figures(rate)} times a second` : rate === 1 ? "once a second" : `once each ${secondsWords(1 / rate)}`;
  return `Its notches sweep from ${hzWords(top)} down to ${hzWords(bottom)} and back, ${how}. ${fb}. ${line}${loud}`;
}

// ---------------------------------------------------------------- grains

/** Grains' buffer, in seconds, how far its grains stay behind the newest sound, in frames, and its values' ranges: its kernel's. */
const GRAINS = { buffer: 2.5, margin: 2, sizes: [0.01, 0.5], spread: 2, pitch: 12 } as const;

/** Grains' buffer (CARD-129): from now, 0 s, to 2.5 s back, across; its window's level up, bare; and a line where its newest grain reads now. */
export function cloudPicture(): PictureDecl<"seconds", "unit01"> {
  return view.picture({ x: axis.lin("seconds", 0, GRAINS.buffer), y: axis.lin("unit01", 0, 1, { bare: true }), live: [live.playhead("reads")] });
}

/**
 * Where Grains' grains start, in seconds back from the newest sound, at the song's rate `sr`
 * (`@m2/effects:grains`' own sum): from `back` to `back + spread`, where `back` keeps a grain behind
 * the newest sound until it ends: `(ratio − 1) × size` when that is over 0, and 2 frames, a grain
 * `round(size × sr)` frames long at `2^(pitch / 12)` its speed.
 */
export function zone(size: number, spread: number, pitch: number, sr: number): [number, number] {
  const len = Math.max(2, Math.round(f32(clamp(size, GRAINS.sizes[0], GRAINS.sizes[1]) * sr)));
  const ratio = 2 ** (clamp(pitch, -GRAINS.pitch, GRAINS.pitch) / 12);
  const back = (Math.max(0, (ratio - 1) * len) + GRAINS.margin) / sr;
  return [back, back + clamp(spread, 0, GRAINS.spread)];
}

/**
 * Grains' buffer (CARD-129), from its own sum (`zone`): a guide line at 2.5 s, the oldest sound it
 * holds (an axis of seconds is drawn to its furthest shape, CARD-102, so the line keeps the whole
 * buffer in view), a faint band where its grains start, and a dot at its far edge that moves the
 * spread across. Its live line, where the newest grain reads now, is its kernel's.
 */
export function cloud<D>(p: Painter<D, "seconds", "unit01">, v: Values, names: { size: Num<D>; spread: In<D, "seconds">; pitch: Num<D> }): Shape[] {
  const size = known(v, names.size, "size"), spread = known(v, names.spread, "spread"), pitch = known(v, names.pitch, "pitch");
  const [near, far] = zone(size, spread, pitch, p.rate);
  return [
    p.mark("x", GRAINS.buffer, `The oldest sound it holds: ${GRAINS.buffer} s back`),
    p.fill([near, 0, far, 0, far, 1, near, 1], { tone: "faint" }),
    p.handle(far, 0.5, { x: names.spread } as Moves<D, "seconds", "unit01">),
  ];
}

/** What `cloud` draws, in words, at the song's rate `sr`: its grains, how far back its band reaches and at what pitch, then its line and its dot. */
export function cloudWords(v: Values, names: { size: string; density: string; spread: string; pitch: string }, sr: number): string {
  const [size, density, spread, pitch] = [v[names.size], v[names.density], v[names.spread], v[names.pitch]];
  const pieces = typeof size === "number" ? `Pieces of ${secondsWords(size)}` : "Pieces as long as a cable or the code says";
  const many = typeof density === "number" ? `${figures(density)} a second` : "as many a second as a cable or the code says";
  let from = "each from as far back as a cable or the code says";
  if (typeof size === "number" && typeof spread === "number" && typeof pitch === "number") {
    // The band's far edge (review X10): an octave up starts them a grain's length further back.
    const [near, far] = zone(size, spread, pitch, sr);
    from = spread > 0 ? `each from up to ${secondsWords(far)} back` : near * sr > GRAINS.margin + 0.5 ? `each from ${secondsWords(near)} back` : "each from just behind your sound";
  }
  const semitones = typeof pitch === "number" ? Math.abs(pitch) : 0;
  const pitched =
    typeof pitch !== "number"
      ? " at a pitch a cable or the code sets"
      : pitch === 0
        ? ""
        : ` and ${semitones === 12 ? "an octave" : `${figures(semitones)} semitone${semitones === 1 ? "" : "s"}`} ${pitch > 0 ? "higher" : "lower"}`;
  return `${pieces}, ${many}, ${from}${pitched} (the band). The line: where the newest grain reads now. Drag the band's edge for the spread`;
}
