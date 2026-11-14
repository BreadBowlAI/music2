// The ruler in the zoom picture (taste.md VIEW-13): its bar lines and beat ticks, bar numbers, tempo
// and time signature changes, the selected bars' band, the loop's brace, the markers and the
// playhead's cap, at the zoom shown, in the page's px and order (`Ruler`, `RulerMarkers`).

import { bars as barsOf, meterSpans } from "../../lib/graph";
import { BEAT_LINES, changes, changeWords, TOOLS_W } from "../../lib/arrangement";
import { RULER_H } from "../../lib/layout";
import type { Pen } from "./pen";
import type { Scene } from "./scene";

/** What the ruler drew, in px from the lanes' left: each bar number and marker, the selected bars' band, the loop's brace, the playhead's cap. */
export interface DrawnRuler {
  bars: { bar: number; x: number }[];
  marks: { name: string; beat: number; x: number }[];
  band: { x: number; w: number } | null;
  brace: { x: number; w: number } | null;
  cap: number;
}

/** Lines 1 px wide, `every` px apart from `from`, across `x0` to `x1`, `y0` to `y1` down. */
function lines(p: Pen, from: number, every: number, x0: number, x1: number, y0: number, y1: number, color: string) {
  if (every <= 0) return;
  for (let i = Math.max(0, Math.floor((x0 - from) / every) - 1); ; i++) {
    const x = from + i * every;
    if (x >= x1) break;
    if (x + 1 > x0) p.fill(x, y0, Math.min(1, x1 - x), y1 - y0, color);
  }
}

/**
 * The ruler at zoom `z` seen from `X`, `x` the camera shown across (the room its end lays out), the
 * playhead at `beat`. Its picture stops where the ruler's tools start.
 */
export function paintRuler(p: Pen, sc: Scene, z: number, X: number, x: number, beat: number): DrawnRuler {
  const { a, colors, type } = sc;
  const drawn: DrawnRuler = { bars: [], marks: [], band: null, brace: null, cap: beat * z - X };
  const w = sc.tools;
  const foot = RULER_H - 1;
  // As wide as the song, the view, and where the zoom looked: the room its end lays out.
  const width = Math.max(a.room * z + TOOLS_W, a.visible, x + sc.w);
  const right = Math.min(w, width - X);
  p.fill(0, 0, w, foot, colors.chrome);
  p.fill(0, foot, w, 1, colors.line);
  const g = p.g;
  g.save();
  g.beginPath();
  g.rect(0, 0, right, foot);
  g.clip();
  const meter = a.timeline.meter;
  // The bar lines down it, and a tick at each beat along its foot.
  for (const s of meterSpans(meter, 0, width / z)) {
    const left = s.start * z - X, end = s.end * z - X;
    if (left > w || end < 0) continue;
    lines(p, left, s.bar * z, Math.max(0, left), Math.min(end, w), 0, foot, colors.lineStrong);
    if (z >= BEAT_LINES) lines(p, left, z, Math.max(0, left), Math.min(end, w), foot - 5, foot, colors.lineStrong);
  }
  // The bar numbers under the markers' band, each change of tempo or time signature after its bar's
  // number when it is at a bar line, else 3 px after its beat.
  const marks = changes(a.timeline);
  const ruled = barsOf(meter, width / z);
  const numberFont = type.font(500, 11), chipFont = type.font(400, 11);
  const numberBase = 17 + type.baseline(500, 11, 11), chipBase = 17 + type.baseline(400, 11, 11);
  for (const b of ruled) {
    const bw = b.length * z;
    const every = bw >= 40 ? 1 : bw >= 20 ? 2 : bw >= 10 ? 4 : 8;
    const numbered = b.bar % every === 0 && b.start * z + 6 < width - TOOLS_W - 16;
    const change = marks.find((c) => Math.abs(c.beat - b.start) < 1e-6);
    const at = b.start * z + 6 - X;
    if ((!numbered && !change) || at > w || at + 200 < 0) continue;
    let after = at;
    if (numbered) {
      after += p.text(String(b.bar + 1), at, numberBase, numberFont, colors.fgFaint) + 6;
      drawn.bars.push({ bar: b.bar + 1, x: at });
    }
    if (change) p.text(changeWords(change), after, chipBase, chipFont, colors.fgMuted);
  }
  for (const c of marks) {
    if (ruled.some((b) => Math.abs(c.beat - b.start) < 1e-6)) continue;
    const at = c.beat * z + 3 - X;
    if (at <= w && at + 200 >= 0) p.text(changeWords(c), at, chipBase, chipFont, colors.fgMuted);
  }
  // Over them: the selected bars' band, and the loop's brace.
  if (a.bars) {
    const l = a.bars[0] * z - X, r = l + (a.bars[1] - a.bars[0]) * z;
    drawn.band = { x: l, w: r - l };
    p.box(l, 0, r, 14, colors.accentSoft);
    p.box(l, 0, l + 1, 14, colors.accent);
    p.box(r - 1, 0, r, 14, colors.accent);
  }
  if (a.loop) {
    const l = a.loop.start * z - X, r = l + (a.loop.end - a.loop.start) * z;
    const lit = a.punch || a.loop.on;
    drawn.brace = { x: l, w: r - l };
    p.box(l, foot - 6, r, foot, a.punch ? colors.dangerSoft : a.loop.on ? colors.accentRange : colors.lineStrong);
    if (lit) {
      const edge = a.punch ? colors.danger : colors.accent;
      p.box(l, foot - 6, l + 1, foot, edge);
      p.box(r - 1, foot - 6, r, foot, edge);
    }
  }
  // The markers: a tick, and the name beside it on the ruler's colour, cut where the next one starts.
  const list = [...a.markers].sort((m, n) => m.beat - n.beat);
  const markFont = type.font(400, 11), markBase = type.baseline(400, 11, 14);
  list.forEach((m, i) => {
    const at = m.beat * z - X;
    const span = Math.max(((list[i + 1]?.beat ?? width / z) - m.beat) * z, 1);
    if (at > w || at + span < 0) return;
    g.font = markFont;
    const full = 1 + 8 + g.measureText(m.name).width;
    const boxW = Math.min(full, span);
    p.box(at, 0, at + 1, 14, colors.fgFaint);
    if (boxW > 1) {
      p.box(at + 1, 0, at + boxW, 14, colors.chrome);
      g.save();
      g.beginPath();
      g.rect(p.snap(at + 1), 0, p.snap(at + boxW) - p.snap(at + 1), 14);
      g.clip();
      p.text(m.name, at + 1 + 4, markBase, markFont, colors.fg, boxW - 1 - 8);
      g.restore();
    }
    drawn.marks.push({ name: m.name, beat: m.beat, x: at });
  });
  g.restore();
  // The playhead's cap, an 8 × 6 triangle at the ruler's foot.
  const cap = beat * z - X;
  g.fillStyle = p.c(colors.fgStrong);
  g.beginPath();
  g.moveTo(cap - 4, RULER_H - 6);
  g.lineTo(cap + 4, RULER_H - 6);
  g.lineTo(cap, RULER_H);
  g.closePath();
  g.fill();
  return drawn;
}
