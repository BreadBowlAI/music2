// A picture a pack or a song draws on its card (CARD-60 to CARD-73). Its code runs on the server
// (CARD-64); the window only asks for its shapes while it shows (CARD-67, taste.md DV-17), draws them in
// music2's own style, and does each dot's gestures (CARD-70): a drag is a knob's gesture over the
// values the dot names (leases, heard as it goes, one write and one undo step at release), the
// wheel and the arrows a step, a double-click or Enter a switch's flip. The curve follows each
// `pictures` message the server sends as the values change (CARD-68).

import clsx from "clsx";
import { useEffect, useId, useRef, useState } from "react";
import { attach, release, strike, useAudition } from "../lib/audition";
import { on } from "../lib/events";
import { beatNow, playingNow } from "../lib/clock";
import { useExpanded } from "../lib/expanded";
import { useLook, useLooking, type LookKind, type Seen } from "../lib/taps";
import { barAt, controlLabel, graphApi, inSentence, picks, stepValue, type CardView, type Knob, type NumberKnob } from "../lib/graph";
import { shown as showing, showsOwn } from "../lib/history";
import { useStore } from "../lib/store";
import { useViewDrag, type ViewProps } from "./CardViews";
import { NOTCH, useKnobGesture } from "./Knob";
import { words as said } from "./Settings";

type PictureView = Extract<CardView, { kind: "picture" }>;
type Axis = PictureView["x"];

/** A dot's value as the server names it (CARD-67): its knob, or why the card can't move it. */
interface Moved {
  name: string;
  knob?: string;
  why?: string;
  by?: "knob";
}
type Shape =
  | { kind: "line" | "fill" | "bars"; points: number[]; tone?: string; name?: string; base?: number }
  | { kind: "handle"; x: number | null; y: number | null; moves: Partial<Record<"x" | "y" | "wheel" | "toggles", Moved>>; opens?: string; mark?: string; off: boolean }
  | { kind: "mark"; axis: "x" | "y"; at: number; words?: string }
  | { kind: "label"; x: number; y: number; words: string }
  | { kind: "play"; box: number[]; note: number }
  // The layers the server measures from the sound itself (CARD-80), with their points.
  | { kind: "response"; points?: number[]; tone?: string; name?: string }
  | { kind: "hit"; points?: number[]; note: number; seconds: number; tone?: string }
  | { kind: "part"; name: string; shapes: Shape[] };
type Handle = Extract<Shape, { kind: "handle" }>;

/** A picture as the server draws it (CARD-67): its shapes and words, or why it can't be drawn. */
export interface Drawn {
  module: string;
  node: string;
  view: string;
  words?: string;
  shapes?: Shape[];
  why?: string;
}

interface PicturesIn {
  t: "pictures";
  session?: number;
  pictures: Drawn[];
}

/** How often a picture asks again: the server keeps an ask 10 s (CARD-67). */
const ASK_MS = 4000;
/** How tall a picture is on its card. */
export const PICTURE_H = 132;
/** Room at the foot for the x axis's labels. */
const FOOT = 12;
/** How wide a label's character is at most, its 9 px text drawn (a minus sign's), and the gap either side of it. */
const CHAR = 6.4;
const GAP = 3;
/** How far apart two y labels are at the least, in pixels, so they never touch: a tick between is left bare. */
const APART = 12;

/** Where `v` sits along `a`, from 0 to 1. */
function along(a: Axis, v: number) {
  if (a.scale === "log") return Math.log(Math.max(v, 1e-12) / a.lo) / Math.log(a.hi / a.lo);
  return (v - a.lo) / (a.hi - a.lo);
}
/** Every x a shape reaches: its points, a dot's, a label's, a play place's, a mark across. */
function reaches(shapes: Shape[]): number[] {
  const out: number[] = [];
  for (const s of shapes) {
    if (s.kind === "part") out.push(...reaches(s.shapes));
    else if (s.kind === "line" || s.kind === "fill" || s.kind === "bars" || s.kind === "hit" || s.kind === "response") (s.points ?? []).forEach((v, i) => i % 2 === 0 && out.push(v));
    else if (s.kind === "handle" && s.x !== null) out.push(s.x);
    else if (s.kind === "label") out.push(s.x);
    else if (s.kind === "play") out.push(s.box[0], s.box[2]);
    else if (s.kind === "mark" && s.axis === "x") out.push(s.at);
  }
  return out.filter(Number.isFinite);
}

/**
 * The x axis a picture is drawn on (CARD-102): an axis of time (in seconds or beats) runs from its
 * start to the furthest x its shapes reach, never past its end, so a short envelope fills the
 * picture as a long one does; any other axis as declared.
 */
export function fitted(a: Axis, shapes: Shape[]): Axis {
  if (a.scale === "log" || (a.unit !== "seconds" && a.unit !== "beats")) return a;
  const far = Math.max(...reaches(shapes).filter((v) => v > a.lo));
  return Number.isFinite(far) && far < a.hi ? { ...a, hi: far } : a;
}

/**
 * An axis's grid: a log axis at 1, 2 and 5 of each decade, a plain one at a round step; labelled
 * where it reads, a log axis's decades `first` (taste.md DV-17: 100, 1k, 10k are what it is read by).
 */
function ticks(a: Axis): { v: number; label?: string; first?: boolean }[] {
  const out: { v: number; label?: string; first?: boolean }[] = [];
  // A bare axis has no grid and no numbers (CARD-60): its shapes say their own words.
  if (a.bare) return out;
  if (a.scale === "log") {
    for (let d = Math.floor(Math.log10(a.lo)); d <= Math.ceil(Math.log10(a.hi)); d++) {
      for (const m of [1, 2, 5]) {
        const v = m * 10 ** d;
        if (v >= a.lo && v <= a.hi) out.push({ v, label: m === 1 || m === 5 ? spelled(v, a.unit) : undefined, first: m === 1 });
      }
    }
    return out;
  }
  const raw = (a.hi - a.lo) / 4;
  const p = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 5, 10].map((m) => m * p).find((s) => s >= raw) ?? raw;
  for (let v = Math.ceil(a.lo / step) * step; v <= a.hi + 1e-9; v += step) out.push({ v, label: spelled(v, a.unit) });
  return out;
}

/**
 * The y axis's labelled ticks on a plot `plot` px tall (taste.md DV-17): every tick's label, or one
 * each `APART` px at the least, counted from 0 where 0 is a tick, so they never touch.
 */
function yLabels(a: Axis, plot: number): { v: number; label: string }[] {
  const all = ticks(a).filter((t): t is { v: number; label: string } => t.label !== undefined);
  if (all.length < 2) return all;
  const gap = (plot * Math.abs(along(a, all[1].v) - along(a, all[0].v))) || plot;
  const every = Math.max(1, Math.ceil(APART / gap));
  const from = Math.max(0, all.findIndex((t) => t.v === 0));
  return all.filter((_, i) => (i - from) % every === 0);
}

/**
 * Where a picture's values sit, `w` × `h` pixels: its plot, above the x axis's labels and right of
 * the y axis's, which have room of their own, so no label ever sits on a line (taste.md DV-17).
 */
function layout(view: PictureView, w: number, h: number) {
  // No room for labels under a bare x axis: the plot runs to the foot (taste.md DV-17).
  const plot = Math.max(1, h - (view.x.bare ? 0 : FOOT));
  const labels = yLabels(view.y, plot);
  const left = labels.length ? Math.ceil(2 * GAP + CHAR * Math.max(...labels.map((t) => [...t.label].length))) : 0;
  const span = Math.max(1, w - left);
  return { plot, left, span, labels, X: (v: number) => left + along(view.x, v) * span, Y: (v: number) => (1 - along(view.y, v)) * plot };
}

/** A value as taste.md writes its unit, short: "1k" for 1000 Hz, "−12" for dB. */
function spelled(v: number, unit: string) {
  const n = (x: number) => String(Math.round(x * 100) / 100).replace("-", "−");
  if (unit === "hz") return v >= 1000 ? `${n(v / 1000)}k` : n(v);
  if (unit === "seconds") return v < 1 && v > 0 ? `${n(v * 1000)}ms` : `${n(v)}s`;
  return n(v);
}

/** A tone as one of music2's greys (CARD-61): never a colour. */
const TONE: Record<string, string> = { main: "var(--color-fg)", soft: "var(--color-fg-muted)", faint: "var(--color-fg-faint)" };

/** A value moved by a dot, rounded as a knob rounds it: a whole Hz, a tenth of a dB, else three figures. */
function rounded(v: number, unit: string | null) {
  if (unit === "hz") return Math.round(v);
  if (unit === "db") return Math.round(v * 10) / 10;
  if (v === 0) return 0;
  const p = 10 ** (2 - Math.floor(Math.log10(Math.abs(v))));
  return Math.round(v * p) / p;
}

const inRange = (k: NumberKnob, v: number) => (k.range ? Math.min(k.range[1], Math.max(k.range[0], v)) : v);

/** A picture's size on screen, in pixels: what it is drawn at, so its text and dots are never stretched (taste.md DV-17). */
function useSize(el: React.RefObject<Element | null>): { w: number; h: number } {
  const [size, setSize] = useState({ w: 0, h: 0 });
  useEffect(() => {
    const at = el.current;
    if (!at) return;
    const seen = () => {
      const r = at.getBoundingClientRect();
      setSize((s) => (Math.round(r.width) === s.w && Math.round(r.height) === s.h ? s : { w: Math.round(r.width), h: Math.round(r.height) }));
    };
    seen();
    const ro = new ResizeObserver(seen);
    ro.observe(at);
    return () => ro.disconnect();
  }, [el]);
  return size;
}

/** A card's picture (taste.md DV-17): asked for while it shows, drawn from its shapes, its dots moving the card's values. */
export function PictureView({ view, ...p }: ViewProps & { view: PictureView }) {
  const box = useRef<HTMLDivElement>(null);
  // One size, rounded, for the shapes and the live layers alike, so both lay the same plot out at
  // any zoom of the patch (taste.md DV-17, review S10): a zoom is a transform, which no size follows.
  const frame = useRef<HTMLDivElement>(null);
  const size = useSize(frame);
  const [drawn, setDrawn] = useState<Drawn | null>(null);
  const [asked, setAsked] = useState<string | null>(null);
  const count = useRef(0);
  const { session, module, revision } = p;
  const node = p.node.id;
  const name = view.name;
  // Looked at: the card on screen, in a window that shows (taste.md DV-17, as a look, TAP-16).
  const looking = useLooking(box);
  useEffect(() => {
    if (!looking) return;
    let alive = true;
    const take = (d: Drawn) => {
      if (!alive) return;
      count.current += 1;
      box.current?.setAttribute("data-drawn", String(count.current));
      setDrawn(d);
      setAsked(null);
    };
    // By its id in the view of `revision`: a stop names the same, so it never ends an ask made by a
    // renumbered card's id (CARD-67).
    const ask = (on: boolean): Promise<void> => {
      // Never by a revision older than the window's own last change (AGENT-2), which the server
      // refuses (CARD-67): it waits, as a gesture does, until the window shows that change, then
      // asks; the change's answer may come after the song's new state (review U1). Had the
      // revision moved meanwhile, the effect runs again for it.
      const graph = useStore.getState().graph;
      if (on && graph && !showsOwn(graph)) return showing(session).then((g) => (alive && g && g.view.revision === revision ? ask(true) : undefined));
      return graphApi.picture(session, revision, module, node, name, on).then(
        (a) => void (on && a.picture && take(a.picture)),
        (e) => void (alive && on && setAsked(said(e))),
      );
    };
    void ask(true);
    const again = setInterval(() => void ask(true), ASK_MS);
    const off = on<PicturesIn>("pictures", (msg) => {
      const mine = msg.pictures.find((x) => x.module === module && x.node === node && x.view === name);
      if (mine) take(mine);
    });
    return () => {
      alive = false;
      clearInterval(again);
      off();
      void ask(false).catch(() => {});
    };
  }, [session, revision, module, node, name, looking]);
  const why = asked ?? drawn?.why ?? null;
  const words = why ?? drawn?.words ?? view.words ?? `${p.owner}'s picture`;
  // While a dot is dragged the x axis stays as it was at the press (CARD-102), so the dot and the
  // shape it moves stay under the pointer; let go, it fits what is drawn again.
  const [heldX, holdX] = useState<Axis | null>(null);
  const shown: PictureView = drawn?.shapes ? { ...view, x: heldX ?? fitted(view.x, drawn.shapes) } : view;
  return (
    <div ref={box} className="h-full" data-picture={name} data-asking={looking || undefined}>
      <div
        ref={frame}
        className={clsx("relative h-full rounded-sm ring-1 ring-line bg-canvas overflow-hidden", why && "opacity-60")}
        data-greyed={why ? true : undefined}
        role="group"
        aria-label={words}
        title={words}
      >
        {drawn?.shapes && !why && view.live?.length ? <Live view={shown} shapes={drawn.shapes} looking={looking} size={size} {...p} /> : null}
        {drawn?.shapes && !why && <Shapes view={shown} shapes={drawn.shapes} hold={holdX} size={size} {...p} />}
        {why && <p className="absolute inset-0 p-2 text-xs text-fg-muted">{why}</p>}
      </div>
    </div>
  );
}

/** The grid and the shapes, at the picture's own size in pixels, and its dots. */
function Shapes({ view, shapes, hold, size, ...p }: ViewProps & { view: PictureView; shapes: Shape[]; hold: (x: Axis | null) => void; size: { w: number; h: number } }) {
  const svg = useRef<SVGSVGElement>(null);
  const { w, h } = size;
  const { plot, left, span, labels, X, Y } = layout(view, w, h);
  // The x axis's labels that fit, none past the right edge and each clear of the others: a log
  // axis's decades first, then the rest where they fit, each in order across (taste.md DV-17).
  const taken: [number, number][] = [];
  const xLabels = new Set(
    ticks(view.x)
      .filter((t) => t.label)
      .map((t) => ({ v: t.v, first: !!t.first, x: X(t.v) + 2, wide: CHAR * [...t.label!].length }))
      .sort((a, b) => Number(b.first) - Number(a.first))
      .filter((t) => {
        if (t.x + t.wide > w || taken.some(([a, b]) => t.x < b + GAP && a < t.x + t.wide + GAP)) return false;
        taken.push([t.x, t.x + t.wide]);
        return true;
      })
      .map((t) => t.v),
  );
  // The plot the shapes are drawn in: none spills over the axes' labels (taste.md DV-17).
  const clip = `plot${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const drag = useViewDrag(p);
  // The group a dot lit on this node (DV-18): its column, lit and in view.
  const nodeKey = p.node.key ?? p.node.id;
  const tab = useExpanded((s) => (s.lit?.node === nodeKey ? s.lit.group : undefined));
  const [held, setHeld] = useState<{ i: number; x: number | null; y: number | null } | null>(null);
  const [picked, setPicked] = useState<number | null>(null);
  const flat: Shape[] = [];
  const flatten = (ss: Shape[]) => ss.forEach((s) => (s.kind === "part" ? flatten(s.shapes) : flat.push(s)));
  flatten(shapes);
  const handles = flat.filter((s): s is Handle => s.kind === "handle");
  // A column lit marks the dot that opens it (taste.md DV-18).
  useEffect(() => {
    const i = handles.findIndex((x) => x.opens !== undefined && x.opens === tab);
    if (i >= 0) setPicked(i);
  }, [tab]);
  useEffect(() => attach(), []);
  const hearing = useAudition((s) => s.on);
  // The notes lane that plays the call (CARD-72), as the keys view's (taste.md DV-9).
  const notes = view.lane;
  const playing = useRef<number | null>(null);
  const knobOf = (m?: Moved): NumberKnob | null => {
    const k: Knob | undefined = m?.knob ? p.node.knobs.find((x) => x.id === m.knob) : undefined;
    return k && !picks(k) ? (k as NumberKnob) : null;
  };
  const pick = (i: number) => {
    setPicked(i);
    const opens = handles[i]?.opens;
    if (!opens) return;
    useExpanded.getState().light(nodeKey, opens);
    // Expanded, its column comes into view once the node has its new size (DV-13).
    setTimeout(() => {
      const col = document.querySelector<HTMLElement>(`[data-node-key="${CSS.escape(nodeKey)}"] [data-column="${CSS.escape(opens)}"]`);
      col?.scrollIntoView({ block: "nearest", inline: "nearest" });
    }, 320);
  };
  /** A dot's drag (CARD-70): from where its values were, by how far the pointer went, one write at release. */
  const down = (e: React.PointerEvent<SVGCircleElement>, i: number) => {
    const hd = handles[i];
    pick(i);
    if (e.button !== 0 || !svg.current || fixedOf(hd, p)) return;
    e.stopPropagation();
    e.preventDefault();
    const el = e.currentTarget, pointer = e.pointerId;
    el.setPointerCapture(pointer);
    const [mx, my] = [hd.moves.x, hd.moves.y];
    const kx = knobOf(mx), ky = knobOf(my);
    const knobs = [kx, ky].filter((k): k is NumberKnob => !!k);
    if (!knobs.length) return;
    drag.begin(knobs);
    hold(view.x);
    const start = knobs.map((k) => k.value);
    const x0 = e.clientX, y0 = e.clientY;
    /** How far `d` pixels move a value from `v`: on its axis in its scale, or as its knob's drag (300 px). */
    const moved = (m: Moved, k: NumberKnob, a: Axis, v: number, d: number, span: number) => {
      if (m.by === "knob") {
        const [lo, hi] = k.range ?? [a.lo, a.hi];
        return k.log && lo > 0 ? v * (hi / lo) ** (d / 300) : v + (d / 300) * (hi - lo);
      }
      return a.scale === "log" ? v * (a.hi / a.lo) ** (d / span) : v + (d / span) * (a.hi - a.lo);
    };
    const values = (ev: PointerEvent) => {
      const dx = ev.clientX - x0, dy = -(ev.clientY - y0);
      const out: number[] = [];
      if (kx && mx) out.push(rounded(inRange(kx, moved(mx, kx, view.x, start[0], dx, span)), kx.unit));
      if (ky && my) out.push(rounded(inRange(ky, moved(my, ky, view.y, start[knobs.length - 1], dy, plot)), ky.unit));
      return out;
    };
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      const vs = values(ev);
      drag.move(vs);
      // The dot follows the pointer; the rest of the picture each `pictures` message (CARD-70). A dot
      // drawn away from its value (an envelope's decay corner) moves by as much as its value did.
      const by = (a: Axis, at: number | null, from: number, to: number) => (at === null ? null : a.scale === "log" ? at * (to / from) : at + (to - from));
      setHeld({
        i,
        x: kx && mx && mx.by !== "knob" ? by(view.x, hd.x, start[0], vs[0]) : hd.x,
        y: ky && my && my.by !== "knob" ? by(view.y, hd.y, start[knobs.length - 1], vs[vs.length - 1]) : hd.y,
      });
    };
    const up = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      setHeld(null);
      hold(null);
      if (ev.type === "pointerup") void drag.end(stepName(p.owner, knobs));
      else void drag.cancel();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };
  const play = (e: React.PointerEvent<SVGRectElement>, s: Extract<Shape, { kind: "play" }>) => {
    if (e.button !== 0 || !notes) return;
    const r = e.currentTarget.getBoundingClientRect();
    const velocity = Math.min(1, Math.max(0.01, 1 - (e.clientY - r.top) / Math.max(1, r.height)));
    e.currentTarget.setPointerCapture(e.pointerId);
    playing.current = strike(notes.module, notes.lane, s.note, velocity);
  };
  const lift = () => {
    release(playing.current);
    playing.current = null;
  };
  const points = (pts: number[]) => {
    const out: string[] = [];
    for (let i = 0; i + 1 < pts.length; i += 2) out.push(`${X(pts[i]).toFixed(1)},${Y(pts[i + 1]).toFixed(1)}`);
    return out.join(" ");
  };
  return (
    <svg ref={svg} viewBox={w && h ? `0 0 ${w} ${h}` : undefined} className="block w-full h-full" data-shapes={flat.length} data-size={`${w}x${h}`} data-x-span={`${view.x.lo},${view.x.hi}`}>
      <clipPath id={clip}>
        <rect x={left} y={0} width={span} height={plot} />
      </clipPath>
      {w > 0 &&
        ticks(view.x).map((t) => (
          <g key={`x${t.v}`}>
            <line x1={X(t.v)} x2={X(t.v)} y1={0} y2={plot} stroke="var(--color-line)" strokeWidth={0.5} />
            {xLabels.has(t.v) && (
              <text x={X(t.v) + 2} y={h - 2} fontSize={9} fill="var(--color-fg-faint)" data-tick>
                {t.label}
              </text>
            )}
          </g>
        ))}
      {w > 0 &&
        ticks(view.y).map((t) => <line key={`y${t.v}`} x1={left} x2={w} y1={Y(t.v)} y2={Y(t.v)} stroke="var(--color-line)" strokeWidth={0.5} />)}
      {/* The y labels in their own room at the left, each by its line, kept whole inside the plot's height. */}
      {w > 0 &&
        labels.map((t) => (
          <text key={`l${t.v}`} x={left - GAP} y={Math.min(plot - 4.5, Math.max(4.5, Y(t.v)))} fontSize={9} textAnchor="end" dominantBaseline="central" fill="var(--color-fg-faint)" data-tick>
            {t.label}
          </text>
        ))}
      <g clipPath={`url(#${clip})`}>
        {w > 0 &&
          flat.map((s, i) => {
            switch (s.kind) {
              case "line":
                return <polyline key={i} data-line={s.name} points={points(s.points)} fill="none" stroke={TONE[s.tone ?? "main"]} strokeWidth={1.25} />;
              case "response":
                return <polyline key={i} data-response data-line={s.name} points={points(s.points ?? [])} fill="none" stroke={TONE[s.tone ?? "main"]} strokeWidth={1.25} />;
              case "hit":
                return (
                  <polygon key={i} data-hit points={points(s.points ?? [])} fill={TONE[s.tone ?? "main"]} fillOpacity={0.12} stroke={TONE[s.tone ?? "main"]} strokeWidth={0.5} />
                );
              case "fill": {
                const pts = s.points;
                const closed = s.base === undefined ? pts : [...pts, pts[pts.length - 2], s.base, pts[0], s.base];
                return <polygon key={i} points={points(closed)} fill={TONE[s.tone ?? "main"]} fillOpacity={0.12} stroke="none" />;
              }
              case "bars":
                return (
                  <g key={i} fill={TONE[s.tone ?? "main"]}>
                    {Array.from({ length: s.points.length / 2 }, (_, j) => {
                      const x = X(s.points[2 * j]), y = Y(s.points[2 * j + 1]), b = Y(s.base ?? 0);
                      return <rect key={j} x={x - 1} width={2} y={Math.min(y, b)} height={Math.abs(b - y)} />;
                    })}
                  </g>
                );
              case "mark": {
                const v = s.axis === "x" ? X(s.at) : Y(s.at);
                return (
                  <line key={i} x1={s.axis === "x" ? v : left} x2={s.axis === "x" ? v : w} y1={s.axis === "x" ? 0 : v} y2={s.axis === "x" ? plot : v} stroke="var(--color-fg-faint)" strokeDasharray="3 3" strokeWidth={1}>
                    {s.words && <title>{s.words}</title>}
                  </line>
                );
              }
              case "label":
                return (
                  <text key={i} x={X(s.x)} y={Y(s.y)} fontSize={10} fill="var(--color-fg-muted)">
                    {s.words}
                  </text>
                );
              case "play": {
                const [x0, y0, x1, y1] = s.box;
                const title = !notes ? "This card plays no notes lane, so it can't play here" : hearing ? "Click to hear it: higher plays it harder" : "Audition is off (in Notes' header): turn it on to hear it";
                return (
                  <rect key={i} data-play={s.note} x={Math.min(X(x0), X(x1))} width={Math.abs(X(x1) - X(x0))} y={Math.min(Y(y0), Y(y1))} height={Math.abs(Y(y1) - Y(y0))} fill="transparent" className={notes ? "cursor-pointer" : undefined} onPointerDown={(e) => play(e, s)} onPointerUp={lift} onPointerCancel={lift}>
                    <title>{title}</title>
                  </rect>
                );
              }
              default:
                return null;
            }
          })}
      </g>
      {w > 0 &&
        handles.map((hd, i) => {
          const x = held?.i === i ? held.x : hd.x, y = held?.i === i ? held.y : hd.y;
          if (x === null || y === null) return null;
          return <Dot key={`h${i}`} i={i} handle={hd} cx={X(x)} cy={Y(y)} marked={picked === i || held?.i === i} knobOf={knobOf} onDown={down} onPick={pick} {...p} />;
        })}
    </svg>
  );
}

/** Why dot `h` can't be moved, when it can't (CARD-66). */
function fixedOf(h: Handle, p: ViewProps): string | null {
  const moves = Object.values(h.moves);
  const stuck = moves.find((m) => m && !m.knob);
  if (stuck) return `${stuck.name} can't be moved here: ${stuck.why ?? "it isn't a knob of this card"}.`;
  if (!p.editable) return `${p.locked ?? "It can't be edited right now"}.`;
  return moves.length ? null : "This dot moves nothing.";
}

/** What an undo step of a dot's gesture is called: the card and what moved ("Curve EQ freq2 and gain2"). */
const stepName = (owner: string, ks: (Knob | NumberKnob)[]) => `${owner} ${ks.map((k) => inSentence(controlLabel(k))).join(" and ")}`;

/** A placeholder for a value a dot doesn't move: its steps go nowhere (the gesture isn't editable). */
const NONE = { id: "", label: "", value: 0, unit: null, range: null, log: false } as unknown as NumberKnob;

/**
 * One dot (CARD-70): a 7 px circle with its mark, a button in the picture's group. Its wheel (a step
 * every 40 px), ⌥↑ ⌥↓, and ← → ↑ ↓ step its values as a knob's keys do: a run of steps is one
 * gesture from the value the last step left, written half a second after the last, one undo step.
 * A double-click, Enter or Space flips its switch: one write.
 */
function Dot({ i, handle, cx, cy, marked, knobOf, onDown, onPick, ...p }: ViewProps & { i: number; handle: Handle; cx: number; cy: number; marked: boolean; knobOf: (m?: Moved) => NumberKnob | null; onDown: (e: React.PointerEvent<SVGCircleElement>, i: number) => void; onPick: (i: number) => void }) {
  const toast = useStore((s) => s.toast);
  const el = useRef<SVGCircleElement>(null);
  const stuck = fixedOf(handle, p);
  const kx = knobOf(handle.moves.x), ky = knobOf(handle.moves.y), kw = knobOf(handle.moves.wheel);
  // A value the dot doesn't move gets a gesture that does nothing.
  const of = (k: NumberKnob | null) => ({ session: p.session, revision: p.revision, knob: k ?? NONE, editable: p.editable && !!k && !stuck, name: k ? stepName(p.owner, [k]) : "", label: k ? controlLabel(k) : "" });
  const gx = useKnobGesture(of(kx));
  const gy = useKnobGesture(of(ky));
  const gw = useKnobGesture(of(kw));
  const steps = (g: ReturnType<typeof useKnobGesture>, k: NumberKnob | null, n: number) => k && g.step((v, d) => stepValue(d, v, n));
  const stepping = useRef({ gw, kw });
  stepping.current = { gw, kw };
  const wheel = useRef(0);
  // The wheel steps it, never scrolling what is under it: not passive (review G5).
  useEffect(() => {
    const at = el.current;
    if (!at) return;
    const turn = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey) return;
      e.preventDefault();
      const { gw, kw } = stepping.current;
      if (!kw) return;
      wheel.current -= e.deltaMode === 1 ? e.deltaY * NOTCH : e.deltaY;
      const n = Math.trunc(wheel.current / NOTCH);
      if (n === 0) return;
      wheel.current -= n * NOTCH;
      gw.step((v, d) => stepValue(d, v, n));
    };
    at.addEventListener("wheel", turn, { passive: false });
    return () => at.removeEventListener("wheel", turn);
  }, []);
  const flip = () => {
    const k = handle.moves.toggles?.knob ? p.node.knobs.find((x) => x.id === handle.moves.toggles!.knob) : undefined;
    if (!k || stuck) return;
    void graphApi.setValue(p.session, p.revision, k.id, k.value !== true, { name: stepName(p.owner, [k]) }).catch((e) => toast(said(e)));
  };
  const key = (e: React.KeyboardEvent) => {
    if (stuck) return;
    const n = e.shiftKey ? 10 : 1;
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) steps(gw, kw, e.key === "ArrowUp" ? n : -n);
    else if (e.key === "ArrowRight" || e.key === "ArrowLeft") steps(gx, kx, e.key === "ArrowRight" ? n : -n);
    else if (e.key === "ArrowUp" || e.key === "ArrowDown") steps(gy, ky, e.key === "ArrowUp" ? n : -n);
    else if (e.key === "Enter" || e.key === " ") flip();
    else return;
    e.preventDefault();
  };
  const names = Object.values(handle.moves).map((m) => m?.name).filter(Boolean).join(", ");
  return (
    <g>
      <circle cx={cx} cy={cy} r={3.5} fill={marked ? "var(--color-accent)" : handle.off || stuck ? "var(--color-raised)" : "var(--color-fg)"} stroke={marked ? "var(--color-accent)" : stuck ? "var(--color-fg-off)" : "var(--color-fg)"} strokeWidth={1.5} pointerEvents="none" />
      {handle.mark && (
        <text x={cx} y={cy - 7} fontSize={9} textAnchor="middle" fill="var(--color-fg-muted)" pointerEvents="none">
          {handle.mark}
        </text>
      )}
      <circle
        ref={el}
        data-dot={i}
        data-opens={handle.opens}
        data-marked={marked || undefined}
        data-fixed={stuck ? true : undefined}
        tabIndex={0}
        role="button"
        aria-label={`${p.owner}: ${names}`}
        cx={cx}
        cy={cy}
        r={9}
        fill="transparent"
        className={stuck ? "cursor-default" : "cursor-grab"}
        onPointerDown={(e) => onDown(e, i)}
        onFocus={() => onPick(i)}
        onKeyDown={key}
        // A double-click flips the dot's switch: one write, never a drag (CARD-70).
        onClick={(e) => e.detail === 2 && flip()}
      >
        <title>{stuck ?? `${names}: drag to move${handle.moves.wheel ? ", scroll for " + handle.moves.wheel.name : ""}${handle.moves.toggles ? ", double-click to turn it on or off" : ""}`}</title>
      </circle>
    </g>
  );
}

// ---------------------------------------------------------------- live layers (CARD-73)

type LiveLayer = NonNullable<PictureView["live"]>[number];

/** What a live layer looks at (TAP-12), when it looks at the sound: its kind and its point. */
function lookOf(l: LiveLayer): { kind: LookKind; at: string } | null {
  if (l.kind === "spectrum" || l.kind === "level" || l.kind === "scope") return { kind: l.kind, at: l.at ?? "out" };
  if (l.kind === "meter") return { kind: "meter", at: l.name ?? "" };
  if (l.kind === "history") return lookOf(l.of as LiveLayer);
  if (l.kind === "playhead" && l.from && l.from !== "beat") return { kind: "meter", at: l.from };
  return null;
}

/** How many reads a history keeps: about 3 s at 30 a second. */
const HISTORY = 90;
/** A meter's bar's width, and the step from one bar to the next, at the canvas's twice-the-pixels scale (CARD-73). */
const BAR = 8;
const BAR_STEP = 12;
/** How tall a switch's strip is along the foot, at that scale. */
const STRIP = 6;
/** A spectrum's and a level's dB, from the foot of the picture to its top. */
const DB_FOOT = -90;

/**
 * What a meter's read is drawn as (CARD-73): a meter in dB, or of a switch, the most it said since
 * the read before, so no peak or opening between two reads is missed; one in any other unit (a
 * pitch, a step) what it says now, since the most of a place over 33 ms is no place. Its unit comes
 * with the answer to its ask (TAP-15): until then, now.
 */
const drawn = (s: Seen) => ((s.unit === "db" || s.unit === "switch") && s.most !== undefined ? s.most : (s.now ?? 0));

/**
 * A picture's live layers, on a canvas under its shapes (CARD-73): each looks at what plays while
 * the picture shows (`useLook`, one ask for each look in this window), and is drawn as its reads
 * come. A layer that can't come says why, small, in the corner; the shapes stay.
 */
function Live({ view, shapes, looking, size, session, module, node }: ViewProps & { view: PictureView; shapes: Shape[]; looking: boolean; size: { w: number; h: number } }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const seen = useRef(new Map<number, Seen[]>());
  const [why, setWhy] = useState<string | null>(null);
  const reads = useRef(0);
  const layers = view.live ?? [];
  const frame = useRef(0);
  const paint = () => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => draw());
  };
  // The playhead moves with the song's beat, at the screen's rate, only while the song plays (review G14).
  const playhead = layers.some((l) => l.kind === "playhead" && (l.from ?? "beat") === "beat");
  const greys = useRef<Record<string, string> | null>(null);
  useEffect(() => {
    if (!playhead || !looking) return;
    let alive = true;
    let was = false;
    const tick = () => {
      if (!alive) return;
      const now = playingNow();
      // Once more as it stops, so the line rests where the song did. The newest draw: the axis may
      // have changed since this began.
      if (now || was) latest.current();
      was = now;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    return () => {
      alive = false;
    };
  }, [playhead, looking]);
  const latest = useRef(() => {});
  // The size the shapes are laid out at, the newest: a draw may come from a frame asked for earlier;
  // drawn again when it changes, so the layers never wait for a read to follow the shapes.
  const sized = useRef(size);
  sized.current = size;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => paint(), [size.w, size.h]);
  const draw = () => {
    const c = canvas.current;
    const g = c?.getContext("2d");
    const { w, h } = sized.current;
    if (!c || !g || !w || !h) return;
    if (c.width !== 2 * w || c.height !== 2 * h) [c.width, c.height] = [2 * w, 2 * h];
    // The greys read once, not at every frame.
    if (!greys.current) {
      const css = getComputedStyle(c);
      greys.current = { faint: css.getPropertyValue("--color-fg-faint").trim() || "#8a8f96", soft: css.getPropertyValue("--color-fg-muted").trim() || "#8a8f96" };
    }
    const grey = (t?: string) => (t === "faint" ? greys.current!.faint : greys.current!.soft);
    // The plot the shapes are drawn on (`layout`, from their size), above the x axis's labels and
    // right of the y axis's, at the canvas's twice-the-pixels scale: `across(f)` is `f`, 0 to 1, across it.
    const lay = layout(view, w, h);
    const plot = 2 * lay.plot;
    const across = (f: number) => 2 * (lay.left + f * lay.span);
    const px = (v: number, a: Axis) => Math.min(1, Math.max(0, along(a, v)));
    // A level's bar or history in dB on the picture's own y axis when that is in dB (CARD-103); else,
    // and for a spectrum always, from −90 dB at the foot to 0 dB at the top.
    const ownY = (db: number) => plot * (1 - Math.min(1, Math.max(0, (db - DB_FOOT) / -DB_FOOT)));
    const dbY = (db: number) => (view.y.unit === "db" ? (1 - px(db, view.y)) * plot : ownY(db));
    /** What each layer drew last, for an agent's eye and the suites: its value, and its place across and down, from 0 to 1; a history's values too, oldest first. */
    const said: ({ v: number; x?: number; y: number; h?: number[] } | null)[] = layers.map(() => null);
    // Several meters' bars stand side by side, in the order the picture lists them, the last at the
    // right edge, so none hides another (CARD-73): where each one's bar starts.
    const meters = layers.filter((l) => l.kind === "meter").length;
    const meterX = (i: number) => c.width - 2 - BAR - BAR_STEP * (meters - 1 - layers.slice(0, i).filter((l) => l.kind === "meter").length);
    g.clearRect(0, 0, c.width, c.height);
    layers.forEach((l, i) => {
      const all = seen.current.get(i) ?? [];
      const last = all[all.length - 1];
      g.strokeStyle = g.fillStyle = grey((l as { tone?: string }).tone);
      g.lineWidth = 2;
      if (l.kind === "spectrum" && last?.bins && view.x.unit === "hz") {
        // A twelfth of an octave each, from 20 Hz: a faint fill from the foot, on a scale of its own
        // (−90 dB at the foot, 0 dB at the top) whatever the y axis: a level behind a curve of gain,
        // so a quiet sound still shows (CARD-103).
        g.globalAlpha = 0.35;
        g.beginPath();
        g.moveTo(across(0), plot);
        last.bins.forEach((db, j) => g.lineTo(across(px(20 * 2 ** (j / 12), view.x)), ownY(db)));
        const loudest = Math.max(...last.bins);
        said[i] = { v: loudest, y: ownY(loudest) / plot };
        g.lineTo(across(1), plot);
        g.closePath();
        g.fill();
        g.globalAlpha = 1;
      } else if (l.kind === "level" && last?.peak) {
        const db = 20 * Math.log10(Math.max(1e-6, Math.max(...last.peak)));
        const on = l.on ? (flatShapes(shapes).find((s) => s.kind === "line" && s.name === l.on) as { points: number[] } | undefined) : undefined;
        if (on && view.x.unit === "db") {
          // A dot riding its line: x the level, y the line there (CARD-73).
          const x = across(px(db, view.x));
          const y = (1 - px(onLine(on.points, db), view.y)) * plot;
          g.beginPath();
          g.arc(x, y, 6, 0, 2 * Math.PI);
          g.fill();
          said[i] = { v: db, x: px(db, view.x), y: y / plot };
        } else if (view.y.unit === "db") {
          // A bar at the left edge, from the foot up to the level on the y axis.
          const y = dbY(db);
          g.fillRect(across(0), y, 8, plot - y);
          said[i] = { v: db, y: y / plot };
        } else {
          g.fillRect(across(0), plot - 4, across((db - DB_FOOT) / -DB_FOOT) - across(0), 4);
          said[i] = { v: db, y: 1 };
        }
      } else if (l.kind === "meter" && last?.now !== undefined) {
        // A bar at the right edge, from the axis's 0 to the meter's number, in the y axis's unit.
        const v = drawn(last);
        const y0 = (1 - px(Math.min(Math.max(0, view.y.lo), view.y.hi), view.y)) * plot, y1 = (1 - px(v, view.y)) * plot;
        const x = meterX(i);
        g.fillRect(x, Math.min(y0, y1), BAR, Math.abs(y1 - y0));
        said[i] = { v, x: (x + BAR / 2) / c.width, y: y1 / plot };
      } else if (l.kind === "history" && all.length > 1 && last?.unit === "switch") {
        // A switch over time (CARD-73): a strip along the plot's foot, lit where it was on at all.
        const at = (j: number) => across(Math.max(0, (j + HISTORY - all.length) / (HISTORY - 1)));
        all.forEach((s, j) => {
          if (drawn(s) >= 0.5) g.fillRect(at(j - 1), plot - STRIP, Math.max(1, at(j) - at(j - 1)), STRIP);
        });
        said[i] = { v: drawn(last), y: (plot - STRIP / 2) / plot, h: all.map(drawn) };
      } else if (l.kind === "history" && all.length > 1) {
        g.beginPath();
        const h: number[] = [];
        all.forEach((s, j) => {
          const v = s.peak ? 20 * Math.log10(Math.max(1e-6, Math.max(...s.peak))) : drawn(s);
          const y = s.peak ? dbY(v) : (1 - px(v, view.y)) * plot;
          const x = across((j + HISTORY - all.length) / (HISTORY - 1));
          if (j) g.lineTo(x, y);
          else g.moveTo(x, y);
          h.push(v);
          said[i] = { v, y: y / plot, h };
        });
        g.stroke();
      } else if (l.kind === "playhead") {
        const graph = useStore.getState().graph;
        let t: number | null = null;
        if ((l.from ?? "beat") === "beat" && graph) {
          const beat = Math.max(0, beatNow());
          if (view.x.unit === "beats") {
            // On an axis of beats: the song's beat, from the axis's start, round again each time it
            // has crossed it (CARD-104), so a picture of whole cycles shows where its cycle is.
            const span = view.x.hi - view.x.lo;
            t = span > 0 ? (((beat - view.x.lo) % span) + span) % span / span : null;
          } else {
            const bar = barAt(graph.timeline.meter, beat);
            t = (beat - bar.start) / bar.length;
          }
          if (t !== null) said[i] = { v: beat, x: t, y: 0 };
        } else if (last?.now !== undefined) t = px(last.now, view.x); // a place: where it is now, never its most
        if (t !== null) g.fillRect(across(t) - 1, 0, 2, plot);
      } else if (l.kind === "scope" && last?.frames?.length) {
        const f = last.frames;
        g.beginPath();
        for (let j = 0; j < f[0].length; j++) {
          let v = 0;
          for (const ch of f) v += ch[j] ?? 0;
          const x = across(j / Math.max(1, f[0].length - 1)), y = (1 - (Math.max(-1, Math.min(1, v / f.length)) + 1) / 2) * plot;
          if (j) g.lineTo(x, y);
          else g.moveTo(x, y);
        }
        g.stroke();
      }
    });
    c.dataset.reads = String(reads.current);
    // Where it laid the plot out, in pixels, for the suites to hold against the shapes' (taste.md DV-17).
    c.dataset.plot = `${lay.left},${lay.span},${lay.plot}`;
    c.dataset.said = JSON.stringify(said.map((x) => x && { v: +x.v.toFixed(3), ...(x.x !== undefined ? { x: +x.x.toFixed(4) } : {}), y: +x.y.toFixed(4), ...(x.h ? { h: x.h.map((v) => +v.toFixed(2)) } : {}) }));
    c.dataset.span = `${view.x.lo},${view.x.hi}`;
  };
  latest.current = draw;
  return (
    <>
      <canvas ref={canvas} data-live={layers.length} className="absolute inset-0 w-full h-full pointer-events-none" />
      {layers.map((l, i) => {
        const look = lookOf(l);
        return look ? (
          <LayerLook
            key={i}
            look={{ session, module, node: node.id, ...look }}
            looking={looking}
            seen={(s) => {
              const kept = seen.current.get(i) ?? [];
              kept.push(s);
              if (kept.length > HISTORY) kept.shift();
              seen.current.set(i, kept);
              reads.current += 1;
              paint();
            }}
            why={setWhy}
          />
        ) : null;
      })}
      {why && <span className="absolute right-1 bottom-0.5 max-w-[70%] truncate text-xs text-fg-faint" title={why} data-live-why>{why}</span>}
    </>
  );
}

/** One live layer's look (TAP-16): a component, so each layer's ask is its own hook. */
function LayerLook({ look, looking, seen, why }: { look: { session: number; module: string; node: string; kind: LookKind; at: string }; looking: boolean; seen: (s: Seen) => void; why: (w: string | null) => void }) {
  useLook(look, looking, seen, why);
  return null;
}

/** Every shape of `shapes`, those in its parts too. */
function flatShapes(shapes: Shape[]): Shape[] {
  return shapes.flatMap((s) => (s.kind === "part" ? flatShapes(s.shapes) : [s]));
}

/** A line's y at `x`, between its points. */
function onLine(points: number[], x: number) {
  for (let i = 2; i + 1 < points.length; i += 2) {
    const [x0, y0, x1, y1] = [points[i - 2], points[i - 1], points[i], points[i + 1]];
    if (x <= x1) return x1 === x0 ? y1 : y0 + ((x - x0) / (x1 - x0)) * (y1 - y0);
  }
  return points[points.length - 1] ?? 0;
}
