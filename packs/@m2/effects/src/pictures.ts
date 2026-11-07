// @m2/effects' pictures (CARD-117 to CARD-124): what each of its cards draws, in TypeScript over
// the picture words (CARD-61), with no kernel. Each is drawn from the effect's own sums, since none
// can be measured with a click (CARD-82: a delay's or a reverb's click rings past 2 s, drive changes
// with how loud it is, the moving effects change over time), and each is checked against a render
// of the sound within 0.5 dB (CARD-99). They are worked out at 48 kHz.
//
// Each shape function takes the card's `p`, so TypeScript checks the names of the values each dot
// moves against the sound's own descriptor, in the axis's unit (CARD-62).

import { axis, knob, view, type Moves, type Painter, type PictureDecl, type Shape, type Unit } from "@m2/core";
import { dbWords, figures, known, secondsWords, type In, type Num, type Values } from "std/pictures";

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const db = (x: number) => 20 * Math.log10(Math.max(1e-6, x));
/** The rate the pictures are worked out at. */
const RATE = 48_000;

// ---------------------------------------------------------------- delay

/** The longest a delay's echoes are drawn, in seconds. */
const ECHO_SECONDS = 8;
/** The quietest echo drawn, in dB. */
const FLOOR = -60;

/** A delay's echoes (CARD-117): seconds across, −60 to 0 dB up. */
export function echoesPicture(): PictureDecl<"seconds", "db"> {
  return view.picture({ x: axis.time(ECHO_SECONDS), y: axis.lin("db", FLOOR, 0) });
}

/** The level of each sound a click makes through the delay, in dB: the click itself, then each echo, to −60 dB or 8 s. */
export function echoLevels(time: number, feedback: number, mix: number): { at: number; db: number }[] {
  const t = clamp(time, 1 / RATE, 2), fb = clamp(feedback, 0, 0.95), m = clamp(mix, 0, 1);
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
  const all = echoLevels(time, fb, mix);
  const bar = (e: { at: number; db: number }) => [e.at, Math.max(FLOOR, e.db)];
  // Each dot rests at the foot when its echo is under it (a mix of 0), so it can be dragged back up (review T9).
  const t = clamp(time, 1 / RATE, 2);
  const first = all[1] ?? { at: t, db: FLOOR }, second = all[2] ?? { at: 2 * t, db: FLOOR };
  return [
    p.bars(bar(all[0]), { base: FLOOR, tone: "faint" }),
    p.bars(all.slice(1).flatMap(bar), { base: FLOOR }),
    p.handle(first.at, Math.max(FLOOR, first.db), { x: names.time, y: knob(names.mix) } as unknown as Moves<D, "seconds", "db">),
    ...(second.at <= ECHO_SECONDS ? [p.handle(second.at, Math.max(FLOOR, second.db), { y: knob(names.feedback), wheel: names.feedback } as unknown as Moves<D, "seconds", "db">)] : []),
  ];
}

/** What `echoes` draws, in words. */
export function echoesWords(v: Values, names: { time: string; feedback: string; mix: string }): string {
  const [t, fb, m] = [v[names.time], v[names.feedback], v[names.mix]];
  if (typeof t !== "number" || typeof fb !== "number" || typeof m !== "number") return "Echoes that repeat and fade: one of its values is set by a cable or the code";
  const n = echoLevels(t, fb, m).length - 1;
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
/** `room`'s sums, once worked out; and how many times they have been (for the test that a build never does). */
let rooms: Room | null = null;
let worked = 0;
/** How many times this module has worked out its sums: a song's build never does (review T1). */
export const sumsWorked = () => worked;

/** The lengths, at 48 kHz, of the reverb's combs and all-passes, in samples (the kernel's own rounding). */
const at48 = (l: number) => Math.floor((l * RATE) / 44_100);

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
export function room(): Room {
  if (rooms) return rooms;
  worked += 1;
  const stages = ALLPASSES.map(at48), combs = COMBS.map(at48);
  // The energies, each all-pass after the last, gathered in 1 ms as they go (each bin its energy
  // and its energy × time), as far as 0.5 s.
  const ms = RATE / 1000, bins = 500;
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
  const grid = RATE / 4000;
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
  return (rooms = { before, grid, together });
}

/**
 * How loud what is left of a reverb's tail is `t` seconds in, in dB from its start: all the sound
 * still to come after that time, as a click's tail gives it (its energy from then on). By the
 * kernel's own sums: a click with its lows below 200 Hz cut goes into eight combs, each sending
 * back `0.7 + 0.28 × size` of what went round, through a one-pole low-pass of `0.4 × damping` that
 * makes the highs fade sooner; then through the four all-passes (`room`). Each comb's energy at
 * each frequency is the loop's gain, squared, smaller each time round it, and its first times round
 * are as loud together as `room` says.
 */
export function tailLevel(size: number, damping: number): (t: number) => number {
  // The last tail asked for is kept, so a draw and its words work its sums out once.
  if (last && last.size === size && last.damping === damping) return last.level;
  const { before, grid, together } = room();
  const fb = 0.7 + 0.28 * clamp(size, 0, 1), d = 0.4 * clamp(damping, 0, 1);
  const lengths = COMBS.map(at48);
  // Each band's share of a click (its lows cut) and its loop's gain, squared.
  const share: number[] = [], gain: number[] = [];
  for (let i = 0; i < BANDS; i++) {
    const [a, b] = [LOWEST * (RATE / 2 / LOWEST) ** (i / BANDS), LOWEST * (RATE / 2 / LOWEST) ** ((i + 1) / BANDS)];
    const f = Math.sqrt(a * b), r = (f / CUT) ** 4;
    const g = (fb * (1 - d)) ** 2 / (1 - 2 * d * Math.cos((2 * Math.PI * f) / RATE) + d * d);
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
    const now = t * RATE;
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
  last = { size, damping, level };
  return level;
}
/** The last tail `tailLevel` worked out. */
let last: { size: number; damping: number; level: (t: number) => number } | null = null;

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

/** A tail's points, to where it has fallen 60 dB, and where it has fallen 30 dB: the last asked for kept, for a draw and its words. */
function tailShape(size: number, damping: number): { points: number[]; half: number } {
  if (shape && shape.size === size && shape.damping === damping) return shape;
  const level = tailLevel(size, damping);
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
  shape = { size, damping, points, half };
  return shape;
}
let shape: { size: number; damping: number; points: number[]; half: number } | null = null;

/**
 * A reverb's tail (CARD-118), from its own sums (`tailLevel`): how loud what is left of it is,
 * from 0 dB at its start down to −60 dB, a faint fill and the line `tail`; one dot where it has
 * fallen 30 dB, that moves the size as its knob does (to the right, longer) and by the wheel.
 */
export function tail<D>(p: Painter<D, "seconds", "db">, v: Values, names: { size: Num<D>; damping: Num<D> }): Shape[] {
  const { points, half } = tailShape(known(v, names.size, "size"), known(v, names.damping, "damping"));
  return [
    p.fill(points, { base: -60, tone: "faint" }),
    p.line(points, { name: "tail" }),
    p.handle(half, -30, { x: knob(names.size), wheel: names.size } as unknown as Moves<D, "seconds", "db">),
  ];
}

/** What `tail` draws, in words. */
export function tailWords(v: Values, names: { size: string; damping: string }): string {
  const [s, d] = [v[names.size], v[names.damping]];
  if (typeof s !== "number" || typeof d !== "number") return "A room's tail as it fades: its size or damping is set by a cable or the code";
  const highs = d > 0 ? `; damping ${figures(d)} makes its highs fade sooner` : "";
  return `Its tail fades 30 dB in ${secondsWords(tailShape(s, d).half)}${highs}. Drag the dot for the size`;
}

// ---------------------------------------------------------------- moving effects

/** A second of an effect's motion (CARD-119 to CARD-121): seconds across, its own unit up. */
export function motionPicture<U extends Unit>(unit: U, lo: number, hi: number, log = false): PictureDecl<"seconds", U> {
  return view.picture({ x: axis.time(1), y: log ? axis.log(unit, lo, hi) : axis.lin(unit, lo, hi) });
}

/** How many points a second of motion is drawn with. */
const MOTION_POINTS = 241;
/** Where an LFO is `t` seconds in, from 0 to 1 of its cycle, as the kernels' `Phase` counts it from the song's start (at most `most` cycles a second). */
const phaseAt = (rate: number, t: number, most = 20) => ((clamp(rate, 0, most) * Math.round(t * RATE)) / RATE) % 1;
/** A second's times. */
const second = () => Array.from({ length: MOTION_POINTS }, (_, i) => i / (MOTION_POINTS - 1));

/** The chorus's two copies (std:chorus): each `base` plus up to `depth` × `sweep` seconds late. */
const CHORUS = { base: 0.008, sweep: 0.006 } as const;

/** How late each of the chorus's two copies is `t` seconds in, in seconds (std:chorus's own sum). */
export function chorusDelays(rate: number, depth: number, t: number): [number, number] {
  const s = Math.sin(2 * Math.PI * phaseAt(rate, t)), w = clamp(depth, 0, 1) * CHORUS.sweep;
  return [CHORUS.base + w * (0.5 + 0.5 * s), CHORUS.base + w * (0.5 - 0.5 * s)];
}

/**
 * A chorus's motion over a second (CARD-119): how late each of its two copies is, in seconds, the
 * lines `one` and `two`, drifting apart and back `rate` times a second, by up to `depth` × 6 ms.
 */
export function chorusMotion<D>(p: Painter<D, "seconds", "seconds">, v: Values, names: { rate: Num<D>; depth: Num<D> }): Shape[] {
  const rate = known(v, names.rate, "rate"), depth = known(v, names.depth, "depth");
  const one: number[] = [], two: number[] = [];
  for (const t of second()) {
    const [a, b] = chorusDelays(rate, depth, t);
    one.push(t, a);
    two.push(t, b);
  }
  return [p.line(one, { name: "one" }), p.line(two, { name: "two", tone: "soft" })];
}

/** The phaser's sweep (its kernel's): around 640 Hz, 2.5 octaves either way at full depth. */
const PHASER = { centre: 640, octaves: 2.5 } as const;

/**
 * Where the phaser's notches are `t` seconds in, in Hz (`@m2/effects:phaser`'s own sum): six
 * all-pass stages around a frequency that sweeps `rate` times a second, mixed with the sound, cut
 * the sound where their turn is half a cycle, at three places round that frequency.
 */
export function notchesAt(rate: number, depth: number, t: number): [number, number, number] {
  const s = Math.sin(2 * Math.PI * phaseAt(rate, t));
  const f = clamp(PHASER.centre * 2 ** (PHASER.octaves * clamp(depth, 0, 1) * s), 10, 0.45 * RATE);
  const g = Math.tan((Math.PI * f) / RATE), back = RATE / Math.PI;
  return [back * Math.atan(g * TURNS[0]), back * Math.atan(g * TURNS[1]), back * Math.atan(g * TURNS[2])];
}
/** Where the six stages' turn is half a cycle, against the sweep's frequency: tan(π/12), tan(3π/12), tan(5π/12). */
const TURNS = [1, 3, 5].map((k) => Math.tan((k * Math.PI) / 12));

/** A phaser's motion over a second (CARD-120): its three notches across frequency, the lines `low`, `mid` and `high`. */
export function phaserMotion<D>(p: Painter<D, "seconds", "hz">, v: Values, names: { rate: Num<D>; depth: Num<D> }): Shape[] {
  const rate = known(v, names.rate, "rate"), depth = known(v, names.depth, "depth");
  const lines: number[][] = [[], [], []];
  for (const t of second()) {
    const [low, mid, high] = notchesAt(rate, depth, t);
    lines[0].push(t, low);
    lines[1].push(t, mid);
    lines[2].push(t, high);
  }
  return [p.line(lines[0], { name: "low", tone: "soft" }), p.line(lines[1], { name: "mid" }), p.line(lines[2], { name: "high", tone: "soft" })];
}

/** How loud the tremolo leaves a sound `t` seconds in, from 0 to 1 (its kernel's own sum). */
export function tremoloAt(rate: number, depth: number, t: number): number {
  return 1 - clamp(depth, 0, 1) * (0.5 - 0.5 * Math.cos(2 * Math.PI * phaseAt(rate, t, 40)));
}

/** A tremolo's motion over a second (CARD-121): how loud it leaves the sound, the line `level`. */
export function tremoloMotion<D>(p: Painter<D, "seconds", "unit01">, v: Values, names: { rate: Num<D>; depth: Num<D> }): Shape[] {
  const rate = known(v, names.rate, "rate"), depth = known(v, names.depth, "depth");
  const points = second().flatMap((t) => [t, tremoloAt(rate, depth, t)]);
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
 * One cycle of a 1 kHz sine as the bitcrusher plays it at 48 kHz (`@m2/effects:bitcrush`'s own sum),
 * a value for each sample: held each time its clock, going up by `rate` over the song's rate each
 * sample, passes 1, and rounded to `bits` (2^(bits − 1) steps each side of 0), then mixed with the
 * sine by `mix`.
 */
export function crushed(bits: number, rate: number, mix: number): number[] {
  const steps = 2 ** (clamp(bits, 1, 24) - 1), m = clamp(mix, 0, 1), step = clamp(rate, 20, RATE) / RATE;
  const out: number[] = [];
  let clock = 1, held = 0;
  for (let n = 0; n < RATE / CRUSH_HZ; n++) {
    const x = Math.sin((2 * Math.PI * CRUSH_HZ * n) / RATE);
    if (clock >= 1) [clock, held] = [clock - 1, Math.round(x * steps) / steps];
    clock += step;
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
  const samples = crushed(known(v, names.bits, "bits"), known(v, names.rate, "rate"), known(v, names.mix, "mix"));
  const sine: number[] = [], held: number[] = [];
  for (let i = 0; i <= 96; i++) sine.push(i / 96 / CRUSH_HZ, Math.sin((2 * Math.PI * i) / 96));
  samples.forEach((y, n) => held.push(n / RATE, y, (n + 1) / RATE, y));
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
