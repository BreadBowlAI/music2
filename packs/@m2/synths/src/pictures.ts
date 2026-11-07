// @m2/synths' own pictures (CARD-136 to CARD-140): one note as the sound plays it, measured by music2
// (`p.hit`, CARD-80); the polysynth's 24 dB ladder filter, and the FM synth's partials, each drawn
// from the kernel's own sums (kernels-src/src/synth.rs, fm.rs), since a click can't reach inside a
// voice (CARD-81). Each drawn one is checked against a render of the kernel within 0.5 dB
// (CARD-136, CARD-140). The envelopes are std's (CARD-96).

import { axis, knob, view, type Moves, type Painter, type PictureDecl, type Shape } from "@m2/core";
import { dbWords, hzWords, secondsWords } from "std/pictures";

/** A number in plain words: at most two places. */
const figures = (v: number) => String(Math.round(v * 100) / 100);

// ---------------------------------------------------------------- one note

/** The note a picture plays: middle C. */
export const NOTE = 60;

/**
 * The most a note's picture shows up and down: these sounds play middle C at about 0.2 at their
 * defaults (−14 dBFS, their kernels' output gain), so it fills half the picture and has room for a
 * louder patch; past it a waveform is cut at the picture's edge (review U4).
 */
const NOTE_RANGE = 0.4;

/** One note (CARD-136): its sound across, in seconds from its start, as a waveform from −0.4 to 0.4. */
export function notePicture(): PictureDecl<"seconds", "bipolar"> {
  return view.picture({ x: axis.time(1), y: axis.lin("bipolar", -NOTE_RANGE, NOTE_RANGE) });
}

/**
 * One note's shapes (CARD-136): middle C held `seconds` (a second at most), as a copy of the sound
 * plays it (`p.hit`, measured, so it changes as a knob turns), and a place over it that plays it
 * through audition when clicked (CARD-72).
 */
export function note(p: Painter<any, "seconds", "bipolar">, seconds: number): Shape[] {
  const s = Math.min(1, Math.max(0.1, seconds));
  // The place over the hit, so a click anywhere on it plays it.
  return [p.hit({ note: NOTE, seconds: s }), p.play([0, -NOTE_RANGE, s, NOTE_RANGE], { note: NOTE })];
}

/** How long a note's picture lasts: its attack and decay, and a little of what it holds, from 0.3 s to 1 s. */
export function heldFor(v: { readonly [k: string]: unknown }, names: { attack?: string; decay: string }): number {
  const a = names.attack && typeof v[names.attack] === "number" ? (v[names.attack] as number) : 0;
  const d = typeof v[names.decay] === "number" ? (v[names.decay] as number) : 1;
  return Math.min(1, Math.max(0.3, a + d + 0.2));
}

/** What `note` draws, in words. */
export const noteWords = (seconds: number) =>
  `Middle C as this sound plays it, held ${secondsWords(Math.min(1, Math.max(0.1, seconds)))}. Click it to hear it`;

// ---------------------------------------------------------------- the ladder filter

/** The ladder's feedback for a resonance from 0 to 1: the kernel's `feedback`, 4 where it starts to sing. */
export const feedbackOf = (resonance: number) => 4 * resonance + 0.6 * resonance ** 8;

/**
 * The polysynth's filter's gain at `f`, in dB (CARD-136): a 4-pole ladder at `cutoff` with feedback k
 * (`feedbackOf(resonance)`), and the makeup its kernel gives the input, as an analog ladder (the
 * kernel's is made digital at its cutoff, at twice the rate, so it follows this): (1 + k/2) /
 * ((1 + i·r)⁴ + k), r = f / cutoff. It falls 24 dB an octave above the cutoff.
 */
export function ladderDb(f: number, cutoff: number, resonance: number): number {
  const r = f / cutoff;
  const k = feedbackOf(Math.min(1, Math.max(0, resonance)));
  const re = 1 - 6 * r * r + r ** 4 + k;
  const im = 4 * r - 4 * r ** 3;
  return 20 * Math.log10(1 + k / 2) - 10 * Math.log10(Math.max(1e-24, re * re + im * im));
}

/** How many points a drawn curve has: one each sixtieth of the 10 octaves. */
const CURVE_POINTS = 241;

/**
 * The ladder's curve (CARD-136), on std's curve across frequency (`curvePicture({ live: false })`):
 * `ladderDb` at its cutoff and resonance, the line `curve`, kept on the picture (±24 dB, the peak of
 * a ladder that sings rests at its top), and one dot on the curve at the cutoff that moves the cutoff
 * across, and the resonance as its knob (up and down) and by the wheel.
 */
export function ladder<D>(
  p: Painter<D, "hz", "db">,
  v: { readonly [k: string]: unknown },
  names: { cutoff: Extract<NonNullable<Moves<D, "hz", "db">["x"]>, string>; resonance: NonNullable<Moves<D>["wheel"]> },
): Shape[] {
  const [cutoff, resonance] = [v[names.cutoff], v[names.resonance]];
  if (typeof cutoff !== "number" || typeof resonance !== "number") throw new Error("its cutoff or resonance is set by a cable or worked out by the code, so it can't be drawn");
  const points: number[] = [];
  for (let i = 0; i < CURVE_POINTS; i++) {
    const f = 20 * 1000 ** (i / (CURVE_POINTS - 1));
    points.push(f, Math.max(-120, Math.min(24, ladderDb(f, cutoff, resonance))));
  }
  return [
    p.fill(points, { base: -120, tone: "faint" }),
    p.line(points, { name: "curve" }),
    p.handle(cutoff, Math.max(-24, Math.min(24, ladderDb(cutoff, cutoff, resonance))), { x: names.cutoff, y: knob(names.resonance), wheel: names.resonance } as unknown as Moves<D, "hz", "db">),
  ];
}

/** What `ladder` draws, in words. */
export function ladderWords(v: { readonly [k: string]: unknown }, names: { cutoff: string; resonance: string }): string {
  const [c, r] = [v[names.cutoff], v[names.resonance]];
  if (typeof c !== "number") return "Cuts the highs, 24 dB an octave: its cutoff is set by a cable or the code";
  const res = typeof r === "number" ? `, resonance ${figures(r)}${feedbackOf(r) >= 4 ? " (it sings)" : ""}` : "";
  return `Cuts the highs above ${hzWords(c)}, 24 dB an octave${res}, at middle C before its envelope opens it`;
}

// ---------------------------------------------------------------- FM partials

/**
 * Bessel functions of the first kind, J₀(x) to J_n(x), by Miller's backward recurrence: what each
 * sideband of a sine bent by a sine of index x is worth.
 */
export function bessel(n: number, x: number): number[] {
  const out = new Array<number>(n + 1).fill(0);
  if (x === 0) {
    out[0] = 1;
    return out;
  }
  // Start well above both n and x, where Jₖ is next to nothing, and come down.
  const top = 2 * Math.ceil((n + 20 + x) / 2);
  const all = new Array<number>(top + 2).fill(0);
  all[top] = 1e-30;
  for (let k = top; k > 0; k--) {
    all[k - 1] = ((2 * k) / x) * all[k] - all[k + 1];
    if (Math.abs(all[k - 1]) > 1e200) for (let i = k - 1; i <= top; i++) all[i] *= 1e-200;
  }
  // J₀ + 2 (J₂ + J₄ + …) = 1.
  let sum = all[0];
  for (let k = 2; k <= top; k += 2) sum += 2 * all[k];
  for (let k = 0; k <= n; k++) out[k] = all[k] / sum;
  return out;
}

/** The lowest a partial is drawn, in dB. */
const FLOOR = -60;
/** The ratios of the note the partials picture shows. */
const MOST = 24;

/** An FM synth's partials (CARD-140): each across at its ratio to the note, 0 to 24, up in dB, −60 to 0. */
export function partialsPicture(): PictureDecl<"scalar", "db"> {
  return view.picture({ x: axis.lin("scalar", 0, MOST), y: axis.lin("db", FLOOR, 0) });
}

/** Middle C, in Hz, and the rate the partials are drawn for: the kernel limits an index by the rate. */
const MIDDLE_C = 440 * 2 ** (-9 / 12);
const RATE = 48000;

/**
 * How far a pair's modulator bends its carrier as a note at velocity 0.8 starts at middle C, for
 * `tone`, with its carrier at ratio `c` and its modulator at `m`: the kernel's index × brightness,
 * bent below the most that keeps the pair's sidebands, out to the index and 4 more, under 1.4 × the
 * rate (`index_limit` and `soft_min`, fm.rs), at 48 kHz.
 */
export function indexAt(amount: number, tone: number, c: number, m: number): number {
  const i = Math.min(32, Math.max(0, amount)) * 2 ** ((Math.min(1, Math.max(0, tone)) - 0.5) * 4);
  const most = (1.4 * RATE - c * MIDDLE_C) / Math.max(1, m * MIDDLE_C) - 4;
  if (most <= 0) return 0;
  return i / Math.sqrt(Math.sqrt(1 + (i / most) ** 4));
}

/**
 * One pair's partials as a note starts (CARD-140): a carrier at ratio `c` bent by a sine at ratio `m`
 * with index `i` is Σ Jₙ(i) at c + n·m, each below 0 folded back with its sign turned over, and those
 * that land together added; then times `gain`. `[ratio, amplitude]` of each, by ratio, DC left out.
 */
export function partialsOf(c: number, m: number, i: number, gain: number): [number, number][] {
  const n = Math.ceil(i + 12);
  const j = bessel(n, i);
  const at = new Map<number, number>();
  for (let k = -n; k <= n; k++) {
    const jk = (k < 0 && -k % 2 === 1 ? -1 : 1) * j[Math.abs(k)];
    const f = c + k * m;
    const key = Math.round(Math.abs(f) * 1e6) / 1e6;
    if (key === 0) continue;
    at.set(key, (at.get(key) ?? 0) + (f < 0 ? -jk : jk) * gain);
  }
  return [...at].sort((a, b) => a[0] - b[0]);
}

/** A level in dB, kept on the partials picture. */
const dbOf = (a: number) => Math.max(FLOOR, 20 * Math.log10(Math.max(1e-9, Math.abs(a))));

/** The names a partials picture reads: each pair's carrier, modulator and index, pair B's mix, and the tone. */
type PairNames<D> = {
  carrier: Extract<NonNullable<Moves<D, "scalar", "db">["x"]>, string>;
  modulator: Extract<NonNullable<Moves<D, "scalar", "db">["x"]>, string>;
  amount: NonNullable<Moves<D>["wheel"]>;
};

/**
 * An FM synth's partials (CARD-140): pair A's as bars (`bars` A), pair B's soft, each pair's from
 * `partialsOf` at its index as a note starts (`indexAt`, at 48 kHz), times its share of the mix; and a dot on
 * each pair's first partial above its carrier (at carrier + modulator, marked A or B) that moves the
 * modulator across, under the pointer, and the amount as its knob (up and down) and by the wheel.
 * Feedback, which makes modulator A a saw, isn't drawn: its words say so.
 */
export function partials<D>(p: Painter<D, "scalar", "db">, v: { readonly [k: string]: unknown }, a: PairNames<D>, b: PairNames<D>, names: { mix: string; tone: string }): Shape[] {
  const num = (k: string, or?: number) => {
    const x = v[k];
    if (typeof x === "number") return x;
    if (or !== undefined) return or;
    throw new Error(`its ${k} is set by a cable or worked out by the code, so it can't be drawn`);
  };
  const [mix, tone] = [Math.min(1, Math.max(0, num(names.mix, 0))), num(names.tone, 0.5)];
  const out: Shape[] = [];
  const pairs = [[a, 1 - mix, "A", "main"], [b, mix, "B", "soft"]] as const;
  for (const [pair, gain, mark, tone_] of pairs) {
    const [c, m] = [num(pair.carrier), num(pair.modulator)];
    const parts = partialsOf(c, m, indexAt(num(pair.amount), tone, c, m), gain).filter(([x, y]) => x <= MOST && dbOf(y) > FLOOR);
    out.push(p.bars(parts.flatMap(([x, y]) => [x, dbOf(y)]), { base: FLOOR, tone: tone_ }));
    if (gain > 0) {
      const first = partialsOf(c, m, indexAt(num(pair.amount), tone, c, m), gain).find(([x]) => Math.abs(x - (c + m)) < 1e-6);
      out.push(p.handle(c + m, dbOf(first?.[1] ?? 0), { x: pair.modulator, y: knob(pair.amount), wheel: pair.amount, mark } as unknown as Moves<D, "scalar", "db">));
    }
  }
  return out;
}

/** What `partials` draws, in words. */
export function partialsWords(v: { readonly [k: string]: unknown }, a: { carrier: string; modulator: string }, b: { carrier: string; modulator: string }, names: { mix: string; feedback: string }): string {
  const pair = (n: { carrier: string; modulator: string }, which: string) => {
    const [c, m] = [v[n.carrier], v[n.modulator]];
    return typeof c === "number" && typeof m === "number" ? `pair ${which}, ${figures(c)} bent by ${figures(m)}` : `pair ${which}, set elsewhere`;
  };
  const mix = v[names.mix];
  const fb = typeof v[names.feedback] === "number" && (v[names.feedback] as number) > 0 ? ". Feedback adds more, not drawn" : "";
  const b_ = typeof mix === "number" ? ` (${dbWords(20 * Math.log10(Math.max(1e-6, mix)))})` : "";
  return `The partials of middle C as it starts: ${pair(a, "A")}, and ${pair(b, "B")}${b_}. Drag a dot across for its modulator, up for its amount${fb}`;
}
