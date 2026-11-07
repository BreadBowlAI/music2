// @m2/effects' pictures (CARD-117 to CARD-124): what each of its cards draws, in TypeScript over
// the picture words (CARD-61), with no kernel. Each is drawn from the effect's own sums, since none
// can be measured with a click (CARD-82: a delay's or a reverb's click rings past 2 s, drive changes
// with how loud it is, the moving effects change over time), and each is checked against a render
// of the sound within 0.5 dB (CARD-125). They are worked out at 48 kHz.
//
// Each shape function takes the card's `p`, so TypeScript checks the names of the values each dot
// moves against the sound's own descriptor, in the axis's unit (CARD-62).

import { axis, knob, view, type Moves, type Painter, type PictureDecl, type Shape, type Unit } from "@m2/core";
import { dbWords, secondsWords } from "std/pictures";

/** The name of one of the sound's number controls: what a dot moves by the wheel, or as its knob. */
type Num<D> = NonNullable<Moves<D>["wheel"]>;
/** The name of one of the sound's controls in `U` (CARD-62). */
type In<D, U extends Unit> = Extract<NonNullable<Moves<D, U, U>["x"]>, string>;
/** What plays, by name (CARD-63). */
type Values = { readonly [k: string]: unknown };

/** `v[name]`, which a cable or the code may hold (CARD-63): else the picture can't be drawn, said in words. */
function known(v: Values, name: string, what: string): number {
  const x = v[name];
  if (typeof x !== "number") throw new Error(`its ${what} is set by a cable or worked out by the code, so it can't be drawn`);
  return x;
}

/** A number in plain words: at most three figures, a minus sign as taste.md writes it. */
function figures(v: number): string {
  const r = Math.abs(v) >= 100 ? Math.round(v) : Math.abs(v) >= 10 ? Math.round(v * 10) / 10 : Math.round(v * 100) / 100;
  return String(r).replace("-", "−");
}

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
 * does (up, more), and one on the second that moves the feedback as its knob does.
 */
export function echoes<D>(p: Painter<D, "seconds", "db">, v: Values, names: { time: In<D, "seconds">; feedback: Num<D>; mix: Num<D> }): Shape[] {
  const time = known(v, names.time, "time"), fb = known(v, names.feedback, "feedback"), mix = known(v, names.mix, "mix");
  const all = echoLevels(time, fb, mix);
  const bar = (e: { at: number; db: number }) => [e.at, Math.max(FLOOR, e.db)];
  const first = all[1], second = all[2] ?? { at: 2 * clamp(time, 1 / RATE, 2), db: FLOOR };
  return [
    p.bars(bar(all[0]), { base: FLOOR, tone: "faint" }),
    p.bars(all.slice(1).flatMap(bar), { base: FLOOR }),
    ...(first ? [p.handle(first.at, Math.max(FLOOR, first.db), { x: names.time, y: knob(names.mix) } as unknown as Moves<D, "seconds", "db">)] : []),
    ...(first && second.at <= ECHO_SECONDS ? [p.handle(second.at, Math.max(FLOOR, second.db), { y: knob(names.feedback), wheel: names.feedback } as unknown as Moves<D, "seconds", "db">)] : []),
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
/** How many frequencies the tail is summed over, evenly to half the rate. */
const BINS = 24;

/** A reverb's tail (CARD-118): seconds across, −60 to 0 dB up. */
export function tailPicture(): PictureDecl<"seconds", "db"> {
  return view.picture({ x: axis.time(TAIL_SECONDS), y: axis.lin("db", -60, 0) });
}

/**
 * Where the reverb's four all-passes, one after the other, send each moment of the combs' sound
 * (worked out once): the samples it comes out after and its energy, gathered in 5 ms, leaving out
 * what holds less than a ten-thousandth of it all. Each all-pass
 * gives what goes in at once, upside down, then again after its length, then half of that after
 * each length more (the kernel's own sum).
 */
const SPREAD: readonly { at: number; energy: number }[] = (() => {
  let taps = new Map<number, number>([[0, 1]]);
  for (const len of ALLPASSES) {
    const m = Math.floor((len * RATE) / 44_100);
    const one: [number, number][] = [[0, -1]];
    for (let n = 1, a = 1; a > 1e-6; n++, a *= 0.5) one.push([n * m, a]);
    const next = new Map<number, number>();
    for (const [t1, a1] of taps) for (const [t2, a2] of one) next.set(t1 + t2, (next.get(t1 + t2) ?? 0) + a1 * a2);
    taps = new Map([...next].filter(([, a]) => Math.abs(a) > 1e-5));
  }
  const bins = new Map<number, [number, number]>();
  let all = 0;
  for (const [t, a] of taps) {
    const b = Math.floor(t / (RATE / 200));
    const [e, at] = bins.get(b) ?? [0, 0];
    bins.set(b, [e + a * a, at + a * a * t]);
    all += a * a;
  }
  return [...bins.values()].filter(([energy]) => energy > 1e-4 * all).map(([energy, at]) => ({ at: at / energy, energy }));
})();

/**
 * How loud what is left of a reverb's tail is `t` seconds in, in dB from its start: all the sound
 * still to come after that time, as a click's tail gives it (its energy from then on). By the
 * kernel's own sums: a click with its lows below 200 Hz cut goes into eight combs, each sending
 * back `0.7 + 0.28 × size` of what went round, through a one-pole low-pass of `0.4 × damping` that
 * makes the highs fade sooner; then through the four all-passes (`SPREAD`). Each comb's energy at
 * each frequency is the loop's gain, squared, smaller each time round it.
 */
export function tailLevel(size: number, damping: number): (t: number) => number {
  const fb = 0.7 + 0.28 * clamp(size, 0, 1), d = 0.4 * clamp(damping, 0, 1);
  const lengths = COMBS.map((l) => Math.floor((l * RATE) / 44_100));
  // What is still to come after each time round a comb, summed over the frequencies: a click's
  // share at each (its lows cut, the comb's whole ringing), times its loop's gain, squared, each
  // time round; worked out as far as it is asked for.
  const share: number[] = [], gain: number[] = [];
  for (let i = 0; i < BINS; i++) {
    const f = ((i + 0.5) / BINS) * (RATE / 2);
    const r = (f / CUT) ** 4;
    const g = (fb * (1 - d)) ** 2 / (1 - 2 * d * Math.cos((2 * Math.PI * f) / RATE) + d * d);
    share.push(r / (1 + r) / (1 - g));
    gain.push(g);
  }
  const after: number[] = [];
  const upTo = (k: number) => {
    while (after.length <= k) {
      let sum = 0;
      for (let i = 0; i < BINS; i++) {
        sum += share[i];
        share[i] *= gain[i];
      }
      after.push(sum);
    }
  };
  const shortest = Math.min(...lengths);
  const left = (t: number) => {
    const now = t * RATE;
    upTo(Math.ceil(now / shortest));
    let e = 0;
    for (const l of lengths) {
      for (let j = 0; j < SPREAD_AT.length; j++) {
        const x = (now - SPREAD_AT[j]) / l;
        e += SPREAD_ENERGY[j] * after[x > 0 ? Math.ceil(x) - 1 : 0];
      }
    }
    return e;
  };
  const whole = left(0);
  return (t) => 10 * Math.log10(Math.max(1e-12, left(t) / whole));
}

/** `SPREAD` as two lists, for `tailLevel`'s sums. */
const SPREAD_AT = SPREAD.map((s) => s.at), SPREAD_ENERGY = SPREAD.map((s) => s.energy);

/** When a tail of `level` has fallen `down` dB, in seconds, to 5 ms: `TAIL_SECONDS` at most. */
export function tailFalls(level: (t: number) => number, down: number): number {
  // Doubled from a quarter of a second until past it, then halved.
  let [lo, hi] = [0, 0.25];
  while (level(hi) > -down) {
    if (hi >= TAIL_SECONDS) return TAIL_SECONDS;
    [lo, hi] = [hi, Math.min(TAIL_SECONDS, 2 * hi)];
  }
  while (hi - lo > 0.005) {
    const mid = (lo + hi) / 2;
    if (level(mid) > -down) lo = mid;
    else hi = mid;
  }
  return hi;
}

/** How many points a tail is drawn with. */
const TAIL_POINTS = 48;

/**
 * A reverb's tail (CARD-118), from its own sums (`tailLevel`): how loud what is left of it is,
 * from 0 dB at its start down to −60 dB, a faint fill and the line `tail`; one dot where it has
 * fallen 30 dB, that moves the size as its knob does (to the right, longer) and by the wheel.
 */
export function tail<D>(p: Painter<D, "seconds", "db">, v: Values, names: { size: Num<D>; damping: string }): Shape[] {
  const level = tailLevel(known(v, names.size, "size"), known(v, names.damping, "damping"));
  const end = tailFalls(level, 60);
  const points: number[] = [];
  for (let i = 0; i < TAIL_POINTS; i++) {
    const t = (i / (TAIL_POINTS - 1)) * end;
    points.push(t, Math.max(-60, level(t)));
  }
  return [
    p.fill(points, { base: -60, tone: "faint" }),
    p.line(points, { name: "tail" }),
    p.handle(tailFalls(level, 30), -30, { x: knob(names.size), wheel: names.size } as unknown as Moves<D, "seconds", "db">),
  ];
}

/** What `tail` draws, in words. */
export function tailWords(v: Values, names: { size: string; damping: string }): string {
  const [s, d] = [v[names.size], v[names.damping]];
  if (typeof s !== "number" || typeof d !== "number") return "A room's tail as it fades: its size or damping is set by a cable or the code";
  const highs = d > 0 ? `; damping ${figures(d)} makes its highs fade sooner` : "";
  return `Its tail fades 30 dB in ${secondsWords(tailFalls(tailLevel(s, d), 30))}${highs}. Drag the dot for the size`;
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
export function chorusMotion(p: Painter<any, "seconds", "seconds">, v: Values, names: { rate: string; depth: string }): Shape[] {
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
  const g = Math.tan((Math.PI * f) / RATE);
  return [1, 3, 5].map((k) => (RATE / Math.PI) * Math.atan(g * Math.tan((k * Math.PI) / 12))) as [number, number, number];
}

/** A phaser's motion over a second (CARD-120): its three notches across frequency, the lines `low`, `mid` and `high`. */
export function phaserMotion(p: Painter<any, "seconds", "hz">, v: Values, names: { rate: string; depth: string }): Shape[] {
  const rate = known(v, names.rate, "rate"), depth = known(v, names.depth, "depth");
  const lines: number[][] = [[], [], []];
  for (const t of second()) notchesAt(rate, depth, t).forEach((f, i) => lines[i].push(t, f));
  return [p.line(lines[0], { name: "low", tone: "soft" }), p.line(lines[1], { name: "mid" }), p.line(lines[2], { name: "high", tone: "soft" })];
}

/** How loud the tremolo leaves a sound `t` seconds in, from 0 to 1 (its kernel's own sum). */
export function tremoloAt(rate: number, depth: number, t: number): number {
  return 1 - clamp(depth, 0, 1) * (0.5 - 0.5 * Math.cos(2 * Math.PI * phaseAt(rate, t, 40)));
}

/** A tremolo's motion over a second (CARD-121): how loud it leaves the sound, the line `level`. */
export function tremoloMotion(p: Painter<any, "seconds", "unit01">, v: Values, names: { rate: string; depth: string }): Shape[] {
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
export function crush(p: Painter<any, "seconds", "bipolar">, v: Values, names: { bits: string; rate: string; mix: string }): Shape[] {
  const samples = crushed(known(v, names.bits, "bits"), known(v, names.rate, "rate"), known(v, names.mix, "mix"));
  const sine: number[] = [], held: number[] = [];
  for (let i = 0; i <= 96; i++) sine.push(i / 96 / CRUSH_HZ, Math.sin((2 * Math.PI * i) / 96));
  samples.forEach((y, n) => held.push(n / RATE, y, (n + 1) / RATE, y));
  return [p.line(sine, { tone: "faint" }), p.line(held, { name: "crushed" })];
}

// ---------------------------------------------------------------- width

/** How far up a field's fan reaches, below the picture's top, so its dot is drawn whole. */
const FAN = 0.85;

/** A stereo field (CARD-124): from 2 to the left to 2 to the right across (the width: 1 is the sound as it came), its depth up. */
export function fieldPicture(): PictureDecl<"scalar", "unit01"> {
  return view.picture({ x: axis.lin("scalar", -2, 2), y: axis.lin("unit01", 0, 1) });
}

/**
 * A stereo field (CARD-124): a fan from the listener, at the foot, as wide as `width` either side
 * (how far its sides are turned up against its middle, as the kernel does: 0 one point, the middle
 * alone; 1 the sound as it came; 2 its sides twice as loud), the faint lines where 1 is; and a dot at
 * its right edge that moves the width across.
 */
export function field<D>(p: Painter<D, "scalar", "unit01">, v: Values, names: { width: In<D, "scalar"> }): Shape[] {
  const w = clamp(known(v, names.width, "width"), 0, 2);
  return [
    p.line([0, 0, -1, FAN], { tone: "faint" }),
    p.line([0, 0, 1, FAN], { tone: "faint" }),
    p.fill([0, 0, -w, FAN, w, FAN], { tone: "faint" }),
    p.line([-w, FAN, 0, 0, w, FAN], { name: "field" }),
    p.handle(w, FAN, { x: names.width } as Moves<D, "scalar", "unit01">),
  ];
}

/** What `field` draws, in words. */
export function fieldWords(v: Values, names: { width: string }): string {
  const w = v[names.width];
  if (typeof w !== "number") return "How wide the sound is: its width is set by a cable or the code";
  if (w <= 0) return "Width 0: mono, its middle alone. Drag the edge to widen it";
  const what = w < 1 ? "narrower than it came" : w === 1 ? "as it came" : "wider than it came";
  return `Width ${figures(w)}, ${what}: its sides turned ${w >= 1 ? "up" : "down"} ${dbWords(Math.abs(db(w)))} against its middle. Drag the edge`;
}

export { figures };
