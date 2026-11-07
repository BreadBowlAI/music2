// @m2/wavetable's own pictures (CARD-131 to CARD-134): its tables, its filter and its LFO, drawn from
// the kernel's own sums (kernels-src/src/tables.rs and lib.rs), since none of them can be measured:
// they are inside a voice, where no click reaches (CARD-81). Each is checked against a render of
// the kernel within 0.5 dB (CARD-131, CARD-133). Its envelopes are std's (CARD-96).

import { axis, knob, view, type Moves, type Painter, type PictureDecl, type Shape } from "@m2/core";
import { hzWords, secondsWords } from "std/pictures";

/** The built-in tables, in the order the kernel numbers them (`tables::BUILT_IN`). */
export const TABLES = ["basic", "pulse", "vowels", "harmonics", "digital"] as const;

/** A number in plain words: at most two places. */
const figures = (v: number) => String(Math.round(v * 100) / 100);

// ---------------------------------------------------------------- the tables

/** How many harmonics a drawn frame holds: past these a frame of 128 points can't show more. */
const DRAWN = 48;
/** The frames of built-in table `t`: `tables::frames`. */
const framesOf = (t: number) => (t === 0 ? 4 : 32);

/** A voice's formants (a, e, i, o, u) in Hz, their widths and gains, and the note they are made for: `tables.rs`'s. */
const VOWELS = [
  [730, 1090, 2440],
  [530, 1840, 2480],
  [270, 2290, 3010],
  [570, 840, 2410],
  [300, 870, 2240],
];
const WIDTHS = [130, 150, 200];
const GAINS = [1, 0.6, 0.3];
const VOICE = 110;

/** A frame's harmonics, by number from 1: the frame is Σ re[h]·cos(2πht) − im[h]·sin(2πht). */
type Frame = { re: Float64Array; im: Float64Array };

/**
 * Frame `j` of built-in table `t` as its harmonics, up to `DRAWN`: `tables::built_in`'s own sums.
 * Kept by table and frame, since they never change: a draw sums only the frames it draws (CARD-131).
 */
const kept = new Map<string, Frame>();
export function harmonics(t: number, j: number): Frame {
  const key = `${t}:${j}`;
  const was = kept.get(key);
  if (was) return was;
  const a: Frame = { re: new Float64Array(DRAWN + 1), im: new Float64Array(DRAWN + 1) };
  const sine = (h: number, amp: number) => {
    if (h <= DRAWN) a.im[h] = -amp;
  };
  const along = j / (framesOf(t) - 1);
  if (t === 0) {
    if (j === 0) sine(1, 1);
    else if (j === 1) for (let h = 1; h <= DRAWN; h += 2) sine(h, ((8 / (Math.PI * Math.PI)) * (h % 4 === 3 ? -1 : 1)) / (h * h));
    else if (j === 2) for (let h = 1; h <= DRAWN; h++) sine(h, 2 / (Math.PI * h));
    else for (let h = 1; h <= DRAWN; h += 2) sine(h, 4 / (Math.PI * h));
  } else if (t === 1) {
    // High for `w` of the cycle from its start, low for the rest, its DC taken out.
    const w = 0.5 - 0.48 * along;
    for (let h = 1; h <= DRAWN; h++) {
      const phi = 2 * Math.PI * h * w;
      a.re[h] = (2 / (Math.PI * h)) * Math.sin(phi);
      a.im[h] = (-2 / (Math.PI * h)) * (1 - Math.cos(phi));
    }
  } else if (t === 2) {
    const s = along * (VOWELS.length - 1);
    const from = Math.min(Math.floor(s), VOWELS.length - 2);
    const m = s - from;
    const formants = [0, 1, 2].map((i) => Math.exp(Math.log(VOWELS[from][i]) * (1 - m) + Math.log(VOWELS[from + 1][i]) * m));
    for (let h = 1; h <= DRAWN; h++) {
      const f = h * VOICE;
      let peaks = 0;
      for (let i = 0; i < 3; i++) peaks += GAINS[i] / (1 + ((f - formants[i]) / (0.5 * WIDTHS[i])) ** 2);
      sine(h, (0.05 + peaks) / h);
    }
  } else if (t === 3) {
    const n = Math.min(Math.max(j + 1, Math.floor(1.2 ** j)), DRAWN);
    for (let h = 1; h <= n; h++) sine(h, 2 / (Math.PI * h));
  } else {
    const gray = j ^ (j >> 1);
    sine(1, 1);
    for (let h = 2; h <= 48; h++) if ((gray >> ((h - 2) % 5)) & 1) sine(h, 1 / Math.sqrt(h));
  }
  kept.set(key, a);
  return a;
}

/** cos and sin of 2πk/n for k from 0 to n − 1, kept by n: each point's harmonics are looked up, not worked out. */
const turns = new Map<number, [Float64Array, Float64Array]>();
function turnsOf(n: number): [Float64Array, Float64Array] {
  let t = turns.get(n);
  if (!t) {
    const [c, s] = [new Float64Array(n), new Float64Array(n)];
    for (let k = 0; k < n; k++) {
      c[k] = Math.cos((2 * Math.PI * k) / n);
      s[k] = Math.sin((2 * Math.PI * k) / n);
    }
    turns.set(n, (t = [c, s]));
  }
  return t;
}

/**
 * One cycle of frame `a`, `n + 1` points from phase 0 to 1 (n a power of 2), its harmonics up to
 * `most` (under n / 2, so the points hold them), scaled so its peak is 1: each frame as tall as the
 * others, as a wavetable synth draws them. Summed by an inverse FFT, n log n steps rather than one
 * for each harmonic at each point, so a draw stays far inside its limit (CARD-75).
 */
function cycle(a: Frame, n: number, most: number): number[] {
  const [c, s] = turnsOf(n);
  const top = Math.min(most, DRAWN, n / 2 - 1);
  const re = new Float64Array(n), im = new Float64Array(n);
  for (let h = 1; h <= top; h++) {
    re[h] = a.re[h];
    im[h] = a.im[h];
  }
  // In place: the points in bit-reversed order, then butterflies, each turning by e^(+2πi k/n).
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      const r = re[i], q = im[i];
      re[i] = re[j];
      im[i] = im[j];
      re[j] = r;
      im[j] = q;
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const step = n / len, half = len >> 1;
    for (let start = 0; start < n; start += len) {
      for (let k = 0; k < half; k++) {
        // Plain numbers, not a destructured pair: a pair is an array QuickJS makes each time.
        const wr = c[k * step], wi = s[k * step];
        const u = start + k, v = u + half;
        const tr = re[v] * wr - im[v] * wi, ti = re[v] * wi + im[v] * wr;
        re[v] = re[u] - tr;
        im[v] = im[u] - ti;
        re[u] += tr;
        im[u] += ti;
      }
    }
  }
  let peak = 0;
  for (let i = 0; i < n; i++) if (Math.abs(re[i]) > peak) peak = Math.abs(re[i]);
  const out: number[] = [];
  for (let i = 0; i <= n; i++) out.push(peak > 0 ? re[i % n] / peak : 0);
  return out;
}

/** Table `t` at `position` (0 to 1), as the kernel reads it: between the two frames it falls among, mixed. */
export function frameAt(t: number, position: number): Frame {
  const place = Math.min(1, Math.max(0, position)) * (framesOf(t) - 1);
  const j = Math.min(Math.floor(place), framesOf(t) - 1);
  const m = place - j;
  const a = harmonics(t, j);
  if (m === 0) return a;
  const b = harmonics(t, j + 1);
  return { re: a.re.map((x, h) => x + m * (b.re[h] - x)), im: a.im.map((x, h) => x + m * (b.im[h] - x)) };
}

/**
 * A table (CARD-131): its frames one behind the other, from the first (bottom left) to the last
 * (top right), as a wavetable synth draws one, across its position from 0 to 1 and up from −1 to 1.
 */
export function tablePicture(): PictureDecl<"unit01", "bipolar"> {
  return view.picture({ x: axis.lin("unit01", 0, 1), y: axis.lin("bipolar", -1, 1) });
}

/** How wide each frame is drawn, as a part of the picture; how tall; where the first sits, and how far up the last. */
const WIDE = 0.7;
const TALL = 0.24;
const FOOT = -0.7;
const RISE = 1.3;
/** How many frames are drawn faint behind the one playing. */
const BEHIND = 8;

/** Frame `a` drawn at `position` in `n` points, its harmonics up to `most`: across from `position × (1 − WIDE)`, up from its own line. */
function drawnAt(a: Frame, position: number, n: number, most: number): number[] {
  const out: number[] = [];
  cycle(a, n, most).forEach((v, i) => out.push(position * (1 - WIDE) + (WIDE * i) / n, FOOT + RISE * position + TALL * v));
  return out;
}

/** Where the dot of `position` sits on its drawn frame: `position` of the way along it, so it moves across with the value. */
function dotAt(frame: number[], position: number): number {
  const n = frame.length / 2 - 1;
  const at = position * n;
  const i = Math.min(n - 1, Math.floor(at));
  return frame[2 * i + 1] + (at - i) * (frame[2 * i + 3] - frame[2 * i + 1]);
}

/** The names a table's picture reads: its table, its file, its position, and the LFO's reach on it. */
type TableNames<D> = { table: string; file: string; position: Extract<NonNullable<Moves<D, "unit01", "bipolar">["x"]>, string>; lfo: string };

/**
 * A table's shapes (CARD-131): eight of its frames (`BEHIND`), faint; the frame at its position, the line
 * `frame`; when the LFO moves the position, the frames at the two ends of its reach, soft, joined by
 * a soft line; and a dot on the frame that moves the position across. A table that is a file is
 * not drawn (a file's frames are not the picture's to read): only the dot, and its words say so.
 */
export function table<D>(p: Painter<D, "unit01", "bipolar">, v: { readonly [k: string]: unknown }, names: TableNames<D>): Shape[] {
  const position = typeof v[names.position] === "number" ? Math.min(1, Math.max(0, v[names.position] as number)) : null;
  if (position === null) throw new Error("its position is set by a cable or worked out by the code, so it can't be drawn");
  const dot = (y: number) => p.handle(position, y, { x: names.position } as Moves<D, "unit01", "bipolar">);
  if (typeof v[names.file] === "string") return [dot(FOOT + RISE * position)];
  const t = Math.max(0, TABLES.indexOf((v[names.table] as (typeof TABLES)[number]) ?? "basic"));
  const out: Shape[] = [];
  // The frames behind, each one of the table's own (none mixed), kept as a part: drawn again only
  // when the table changes.
  out.push(
    p.part(`behind ${t}`, () =>
      Array.from({ length: BEHIND }, (_, i) => {
        const j = Math.round((i * (framesOf(t) - 1)) / (BEHIND - 1));
        const at = j / (framesOf(t) - 1);
        return p.line(drawnAt(harmonics(t, j), at, 32, 15), { tone: "faint" });
      }),
    ),
  );
  const reach = typeof v[names.lfo] === "number" ? (v[names.lfo] as number) : 0;
  if (reach > 0) {
    const [lo, hi] = [Math.max(0, position - reach), Math.min(1, position + reach)];
    out.push(p.line(drawnAt(frameAt(t, lo), lo, 32, 15), { tone: "soft" }), p.line(drawnAt(frameAt(t, hi), hi, 32, 15), { tone: "soft" }));
    out.push(p.line([lo * (1 - WIDE) + WIDE, FOOT + RISE * lo, hi * (1 - WIDE) + WIDE, FOOT + RISE * hi], { tone: "soft", name: "reach" }));
  }
  const now = drawnAt(frameAt(t, position), position, 64, DRAWN);
  out.push(p.line(now, { name: "frame" }), dot(dotAt(now, position)));
  return out;
}

/** What `table` draws, in words. */
export function tableWords(v: { readonly [k: string]: unknown }, names: { table: string; file: string; position: string; lfo: string }, osc: string): string {
  const pos = v[names.position];
  const at = typeof pos === "number" ? `at position ${figures(pos)}` : "at a position set elsewhere";
  if (typeof v[names.file] === "string") return `Table ${osc} is your file, ${at}: this picture can't draw a file yet. Drag across to move through it`;
  const reach = v[names.lfo];
  const lfo = typeof reach === "number" && reach > 0 && typeof pos === "number" ? `. The LFO moves it from ${figures(Math.max(0, pos - reach))} to ${figures(Math.min(1, pos + reach))}` : "";
  return `Table ${osc}, ${typeof v[names.table] === "string" ? v[names.table] : "basic"}, ${at}: drag across to move through it${lfo}`;
}

// ---------------------------------------------------------------- the filter

/** The filter's shapes, in the order its menu names them. */
const SHAPES = ["lowpass", "bandpass", "highpass", "notch"] as const;

/**
 * The voice's filter's gain at `f`, in dB (CARD-133): its state-variable filter of `shape`, at
 * `cutoff` and `q`, as an analog filter (the kernel's is made digital at the cutoff, so it follows
 * this up to a twelfth of the rate): with r = f / cutoff and D = 1 − r² + i·r / q, the lowpass is
 * 1 / D, the bandpass i·r / q / D (0 dB at its peak), the highpass −r² / D and the notch (1 − r²) / D.
 */
export function svfDb(shape: string, f: number, cutoff: number, q: number): number {
  const r = f / cutoff;
  const below = 10 * Math.log10((1 - r * r) ** 2 + (r / q) ** 2);
  if (shape === "bandpass") return 20 * Math.log10(r / q) - below;
  if (shape === "highpass") return 40 * Math.log10(r) - below;
  if (shape === "notch") return 20 * Math.log10(Math.max(1e-6, Math.abs(1 - r * r))) - below;
  return -below;
}

/** How many points the filter's curve has: one each sixtieth of its 10 octaves. */
const CURVE_POINTS = 241;

/**
 * The voice's filter (CARD-133), on std's curve across frequency (`curvePicture({ live: false })`):
 * its shape's curve from `svfDb`, the line `curve`, and one dot at the cutoff that moves it across
 * and the q as its knob (up and down) and by the wheel. The dot sits at the curve's peak, 20 · log10(q)
 * dB, or 0 dB for a bandpass, and a notch's at the picture's foot.
 */
export function filter<D>(
  p: Painter<D, "hz", "db">,
  v: { readonly [k: string]: unknown },
  names: { shape: string; cutoff: Extract<NonNullable<Moves<D, "hz", "db">["x"]>, string>; q: NonNullable<Moves<D>["wheel"]> },
): Shape[] {
  const [cutoff, q] = [v[names.cutoff], v[names.q]];
  if (typeof cutoff !== "number" || typeof q !== "number") throw new Error("its cutoff or q is set by a cable or worked out by the code, so it can't be drawn");
  const shape = typeof v[names.shape] === "string" ? (v[names.shape] as string) : "lowpass";
  const points: number[] = [];
  for (let i = 0; i < CURVE_POINTS; i++) {
    const f = 20 * 1000 ** (i / (CURVE_POINTS - 1));
    points.push(f, Math.max(-120, Math.min(120, svfDb(shape, f, cutoff, q))));
  }
  const y = shape === "notch" ? -24 : shape === "bandpass" ? 0 : Math.min(24, 20 * Math.log10(q));
  return [
    p.fill(points, { base: -120, tone: "faint" }),
    p.line(points, { name: "curve" }),
    p.handle(cutoff, y, { x: names.cutoff, y: knob(names.q), wheel: names.q } as unknown as Moves<D, "hz", "db">),
  ];
}

/** What `filter` draws, in words. */
export function filterWords(v: { readonly [k: string]: unknown }, names: { shape: string; cutoff: string; q: string }): string {
  const [c, q, s] = [v[names.cutoff], v[names.q], v[names.shape]];
  if (typeof c !== "number") return "The filter: its cutoff is set by a cable or the code";
  const what = { lowpass: `Cuts the highs above ${hzWords(c)}`, bandpass: `Keeps a band around ${hzWords(c)}`, highpass: `Cuts the lows below ${hzWords(c)}`, notch: `Cuts a narrow band out at ${hzWords(c)}` };
  const shape = (SHAPES as readonly string[]).includes(s as string) ? (s as keyof typeof what) : "lowpass";
  return `${what[shape]}${typeof q === "number" ? `, q ${figures(q)}` : ""}, at middle C before its envelope and LFO move it`;
}

// ---------------------------------------------------------------- the LFO

/** The least the LFO's picture shows: whole cycles of it, reaching a second at least. */
const LFO_SECONDS = 1;

/** The LFO (CARD-134): whole cycles of its sine in seconds across, from a note's start, −1 to 1 up. */
export function lfoPicture(): PictureDecl<"seconds", "bipolar"> {
  return view.picture({ x: axis.time(40), y: axis.lin("bipolar", -1, 1) });
}

/**
 * The LFO's shapes (CARD-134): the kernel's sine from each note's start, rising first, over the fewest
 * whole cycles that reach a second (at most 40 s), the line `shape`. No dot: its rate is in Hz, which a
 * time axis can't move under the pointer; its knobs are beside it.
 */
export function lfo(p: Painter<any, "seconds", "bipolar">, v: { readonly [k: string]: unknown }, names: { rate: string }): Shape[] {
  const rate = v[names.rate];
  if (typeof rate !== "number" || !(rate > 0)) throw new Error("its rate is set by a cable or worked out by the code, so it can't be drawn");
  const cycles = Math.max(1, Math.ceil(LFO_SECONDS * rate - 1e-9));
  const span = Math.min(40, cycles / rate);
  const n = Math.min(512, 32 * cycles);
  const points: number[] = [];
  for (let i = 0; i <= n; i++) points.push((span * i) / n, Math.sin((2 * Math.PI * rate * span * i) / n));
  return [p.mark("y", 0), p.line(points, { name: "shape" })];
}

/** What `lfo` draws, in words. */
export function lfoWords(v: { readonly [k: string]: unknown }, names: { rate: string; position: string; cutoff: string }): string {
  const [r, pos, cut] = [v[names.rate], v[names.position], v[names.cutoff]];
  if (typeof r !== "number") return "A sine from each note's start, at a rate set elsewhere";
  const to = [typeof pos === "number" && pos > 0 ? `both positions ${figures(pos)} each way` : "", typeof cut === "number" && cut > 0 ? `the cutoff ${figures(cut)} octaves each way` : ""].filter(Boolean);
  return `A sine from each note's start, once every ${secondsWords(1 / r)}${to.length ? `: it moves ${to.join(" and ")}` : ": it moves nothing yet"}`;
}
