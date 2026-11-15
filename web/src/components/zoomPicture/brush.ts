// What the zoom picture draws with (a brush, not ClipDrawing's pen: that one draws a clip's notes): Canvas 2D in the view's px (from the lanes' left and the view's
// top), at the screen's own pixels. It draws as the page does: a box's edges on the screen's pixels
// (`box`), as the browser places a box; a canvas's or an SVG's shapes where they fall (`fill`).
// A group (a faded track's clips) is drawn apart and laid on at its alpha, its colours through its
// grayscale, as CSS's `opacity` and `filter: grayscale()` do.

/** A colour's red, green and blue (0 to 255) and alpha (0 to 1). */
type Rgba = [number, number, number, number];

const parsed = new Map<string, Rgba>();
let probe: CanvasRenderingContext2D | null = null;

/** A colour the canvas takes (`#rrggbb`, `rgb()`), as numbers: the canvas's own reading of it. */
function rgbaOf(color: string): Rgba {
  const known = parsed.get(color);
  if (known) return known;
  probe ??= document.createElement("canvas").getContext("2d");
  let v: Rgba = [0, 0, 0, 1];
  if (probe) {
    probe.fillStyle = "#000";
    probe.fillStyle = color;
    const s = String(probe.fillStyle);
    if (s.startsWith("#")) v = [parseInt(s.slice(1, 3), 16), parseInt(s.slice(3, 5), 16), parseInt(s.slice(5, 7), 16), 1];
    else {
      const n = (s.match(/[\d.]+/g) ?? []).map(Number);
      v = [n[0] ?? 0, n[1] ?? 0, n[2] ?? 0, n[3] ?? 1];
    }
  }
  parsed.set(color, v);
  return v;
}

/** `color` through CSS's `grayscale(amount)`: its matrix, on the colour's own values (as the browser applies it). */
function grayOf(color: string, amount: number): string {
  const [r, g, b, a] = rgbaOf(color);
  const k = 1 - amount;
  const to = (x: number) => Math.round(Math.min(255, Math.max(0, x)));
  const R = (0.2126 + 0.7874 * k) * r + (0.7152 - 0.7152 * k) * g + (0.0722 - 0.0722 * k) * b;
  const G = (0.2126 - 0.2126 * k) * r + (0.7152 + 0.2848 * k) * g + (0.0722 - 0.0722 * k) * b;
  const B = (0.2126 - 0.2126 * k) * r + (0.7152 - 0.7152 * k) * g + (0.0722 + 0.9278 * k) * b;
  return `rgba(${to(R)}, ${to(G)}, ${to(B)}, ${a})`;
}

/** Mix `a` into `b` by `t` in OKLab, as CSS's `color-mix(in oklab, a t, b)`: a picked clip's body. */
export function mixOklab(a: string, b: string, t: number): string {
  const lin = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  const enc = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);
  const lab = (c: string) => {
    const [r, g, bl] = rgbaOf(c).slice(0, 3).map((v) => lin(v / 255));
    const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * bl);
    const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * bl);
    const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * bl);
    return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s];
  };
  const p = lab(a), q = lab(b);
  const [L, A, B] = p.map((v, i) => v * t + q[i] * (1 - t));
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3, m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3, s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3;
  const rgb = [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s, -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s];
  return `rgb(${rgb.map((c) => Math.round(Math.min(1, Math.max(0, enc(c))) * 255)).join(", ")})`;
}

/** A rounded rectangle's path, each corner `r` px (none where `r` is 0). */
export function rrect(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  if (r > 0) g.roundRect(x, y, w, h, Math.min(r, w / 2, h / 2));
  else g.rect(x, y, w, h);
}

/**
 * What the brush put where, of a kind the suites check (VIEW-13): the very numbers each of its
 * fills, boxes and words was drawn at, in px from the lanes' left and the view's top. What is drawn
 * inside `as` is listed by its kind, and every word's box as a `text`.
 */
export interface Put {
  kind: string;
  x: number;
  y: number;
  w: number;
  h: number;
  of?: Record<string, unknown>;
}

export class Brush {
  /** Where shapes go now: the canvas, or a group's own. */
  g: CanvasRenderingContext2D;
  /** What was put where since `reset`, by kind (`as`). */
  put: Put[] = [];
  private tag: { kind: string; of?: Record<string, unknown> } | null = null;
  private gray = 0;
  private scratch: CanvasRenderingContext2D | null = null;
  private group: { alpha: number; y0: number; y1: number } | null = null;

  /** `main`'s top is at `top` px of the view; `dpr` device px a px. */
  constructor(
    readonly main: CanvasRenderingContext2D,
    readonly dpr: number,
    readonly top: number,
  ) {
    this.g = main;
    this.reset();
  }

  /** Ready for a new picture: the view's px on the canvas, nothing left over from the last. */
  reset() {
    const m = this.main;
    m.setTransform(this.dpr, 0, 0, this.dpr, 0, -this.top * this.dpr);
    m.globalAlpha = 1;
    m.globalCompositeOperation = "source-over";
    this.g = m;
    this.gray = 0;
    this.group = null;
    this.put = [];
    this.tag = null;
  }

  /** What `f` draws is listed as `kind` (with `of`), where each fill, box and word of it went. */
  as<T>(kind: string, f: () => T, of?: Record<string, unknown>): T {
    const was = this.tag;
    this.tag = { kind, of };
    try {
      return f();
    } finally {
      this.tag = was;
    }
  }

  private note(x: number, y: number, w: number, h: number) {
    if (this.tag) this.put.push({ kind: this.tag.kind, x, y, w, h, ...(this.tag.of ? { of: this.tag.of } : {}) });
  }

  /** `color` as drawn now: through a group's grayscale. */
  c(color: string): string {
    return this.gray ? grayOf(color, this.gray) : color;
  }

  /** `v` px on the screen's pixels, as the browser puts a box's edge. */
  snap(v: number): number {
    return Math.round(v * this.dpr) / this.dpr;
  }

  /** A rectangle where it falls, as a canvas or an SVG draws one. */
  fill(x: number, y: number, w: number, h: number, color: string, alpha = 1) {
    if (w <= 0 || h <= 0) return;
    const g = this.g;
    g.globalAlpha = alpha;
    g.fillStyle = this.c(color);
    g.fillRect(x, y, w, h);
    g.globalAlpha = 1;
    this.note(x, y, w, h);
  }

  /** A rounded rectangle, each corner `r` px, filled (with `hole`, its ring: what lies between it and `hole`). */
  round(x: number, y: number, w: number, h: number, r: number, color: string, hole?: [number, number, number, number, number]) {
    const g = this.g;
    g.fillStyle = this.c(color);
    g.beginPath();
    rrect(g, x, y, w, h, r);
    if (hole) rrect(g, ...hole);
    g.fill(hole ? "evenodd" : "nonzero");
    this.note(x, y, w, h);
  }

  /** A shape through the points `xy`, filled. */
  poly(xy: [number, number][], color: string) {
    if (!xy.length) return;
    const g = this.g;
    g.fillStyle = this.c(color);
    g.beginPath();
    xy.forEach(([x, y], i) => (i ? g.lineTo(x, y) : g.moveTo(x, y)));
    g.closePath();
    g.fill();
    const xs = xy.map(([x]) => x), ys = xy.map(([, y]) => y);
    this.note(Math.min(...xs), Math.min(...ys), Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys));
  }

  /**
   * Lines 1 px wide, `every` px apart from `from` (as an SVG pattern draws them), across `x0` to `x1`,
   * `y0` to `y1` down. With `beat`, each is listed as a `kind` at the beat it marks (the first's
   * `beat[0]`, then `beat[1]` apart).
   */
  lines(from: number, every: number, x0: number, x1: number, y0: number, y1: number, color: string, beat?: { kind: string; at: [number, number] }) {
    if (every <= 0) return;
    for (let i = Math.max(0, Math.floor((x0 - from) / every) - 1); ; i++) {
      const x = from + i * every;
      if (x >= x1) break;
      if (x + 1 <= x0) continue;
      if (beat) this.as(beat.kind, () => this.fill(x, y0, Math.min(1, x1 - x), y1 - y0, color), { beat: beat.at[0] + i * beat.at[1] });
      else this.fill(x, y0, Math.min(1, x1 - x), y1 - y0, color);
    }
  }

  /** A box from `x0` to `x1` across and `y0` to `y1` down, its edges on the screen's pixels, as the page draws one. */
  box(x0: number, y0: number, x1: number, y1: number, color: string, alpha = 1) {
    const l = this.snap(x0), r = this.snap(x1), t = this.snap(y0), b = this.snap(y1);
    this.fill(l, t, r - l, b - t, color, alpha);
  }

  /** `color` multiplied in at `alpha` over the box (CSS's `mix-blend-mode: multiply`). */
  multiply(x0: number, y0: number, x1: number, y1: number, color: string, alpha: number) {
    this.g.globalCompositeOperation = "multiply";
    this.box(x0, y0, x1, y1, color, alpha);
    this.g.globalCompositeOperation = "source-over";
  }

  /** Words from `x` on the baseline `y` in `font`, cut with "…" to `max` px (CSS's `text-overflow: ellipsis`). Returns their width. */
  text(s: string, x: number, y: number, font: string, color: string, max = Infinity): number {
    const g = this.g;
    g.font = font;
    g.textBaseline = "alphabetic";
    g.textAlign = "left";
    const t = cut(g, s, max);
    if (!t) return 0;
    g.fillStyle = this.c(color);
    // Where the page puts the line: its layout's 64ths of a px.
    x = Math.round(x * 64) / 64;
    let w: number;
    if (t === s) {
      g.fillText(t, x, y);
      w = g.measureText(t).width;
    } else {
      // Cut: the words left, then the "…" in a box of its own after them, as the page lays it out (no
      // kerning between the two, which a font like the Mac's has).
      const head = t.slice(0, -1);
      const hw = Math.round(g.measureText(head).width * 64) / 64;
      g.fillText(head, x, y);
      g.fillText("…", x + hw, y);
      w = hw + g.measureText("…").width;
    }
    const m = g.measureText(t);
    const box = { x, y: y - m.fontBoundingBoxAscent, w, h: m.fontBoundingBoxAscent + m.fontBoundingBoxDescent };
    this.put.push({ kind: "text", ...box });
    this.note(box.x, box.y, box.w, box.h);
    return w;
  }

  /** The shapes from now to `end()` drawn apart, then laid on at `alpha`, through `gray`scale, over the band `y0` to `y1`. */
  begin(alpha: number, gray: number, y0: number, y1: number) {
    if (alpha >= 1 && gray <= 0) return;
    const m = this.main.canvas;
    if (!this.scratch) this.scratch = document.createElement("canvas").getContext("2d");
    const s = this.scratch;
    if (!s) return;
    if (s.canvas.width !== m.width || s.canvas.height !== m.height) {
      s.canvas.width = m.width;
      s.canvas.height = m.height;
    }
    s.setTransform(1, 0, 0, 1, 0, 0);
    s.clearRect(0, 0, m.width, m.height);
    s.setTransform(this.dpr, 0, 0, this.dpr, 0, -this.top * this.dpr);
    this.g = s;
    this.gray = gray;
    this.group = { alpha, y0, y1 };
  }

  end() {
    const grp = this.group;
    if (!grp || !this.scratch) return;
    this.group = null;
    this.gray = 0;
    this.g = this.main;
    const m = this.main;
    const a = Math.max(0, Math.floor((grp.y0 - this.top) * this.dpr)), b = Math.min(m.canvas.height, Math.ceil((grp.y1 - this.top) * this.dpr));
    if (b <= a) return;
    m.save();
    m.setTransform(1, 0, 0, 1, 0, 0);
    m.globalAlpha = grp.alpha;
    m.drawImage(this.scratch.canvas, 0, a, m.canvas.width, b - a, 0, a, m.canvas.width, b - a);
    m.restore();
  }
}

/** Text cut to `max` px with "…", as CSS's `text-overflow: ellipsis` cuts it. */
function cut(g: CanvasRenderingContext2D, s: string, max: number): string {
  if (max === Infinity || g.measureText(s).width <= max + 0.01) return s;
  let lo = 0, hi = s.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (g.measureText(`${s.slice(0, mid)}…`).width <= max + 0.01) lo = mid;
    else hi = mid - 1;
  }
  // The first character is never cut for the "…" (CSS's rule): it stays, the "…" after it, past the edge.
  return `${s.slice(0, Math.max(1, lo))}…`;
}
