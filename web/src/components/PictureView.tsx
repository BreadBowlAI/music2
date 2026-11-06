// A picture a pack or a song draws on its card (CARD-60 to CARD-73). Its code runs on the server
// (CARD-64); the window only asks for its shapes while it shows (CARD-67, CARD-69), draws them in
// music2's own style, and does each dot's gestures (CARD-70): a drag is a knob's gesture over the
// values the dot names (leases, heard as it goes, one write and one undo step at release), the
// wheel and the arrows a step, a double-click or Enter a switch's flip. The curve follows each
// `pictures` message the server sends as the values change (CARD-68).

import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { attach, release, strike, useAudition } from "../lib/audition";
import { on } from "../lib/events";
import { beatNow } from "../lib/clock";
import { useCards } from "../lib/folded";
import { useLook, useLooking, type LookKind, type Seen } from "../lib/taps";
import { barAt, controlLabel, graphApi, inSentence, picks, type CardView, type Knob, type NumberKnob } from "../lib/graph";
import { useStore } from "../lib/store";
import { useViewDrag, type ViewProps } from "./CardViews";
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
/** The drawing's size, in its own units; the card stretches it across. */
const W = 400;
/** How tall a picture is on its card. */
export const PICTURE_H = 132;
/** Room at the foot for the x axis's labels. */
const FOOT = 12;

/** Where `v` sits along `a`, from 0 to 1. */
function along(a: Axis, v: number) {
  if (a.scale === "log") return Math.log(Math.max(v, 1e-12) / a.lo) / Math.log(a.hi / a.lo);
  return (v - a.lo) / (a.hi - a.lo);
}
/** An axis's grid: a log axis at 1, 2 and 5 of each decade, a plain one at a round step; labelled where it reads. */
function ticks(a: Axis): { v: number; label?: string }[] {
  const out: { v: number; label?: string }[] = [];
  if (a.scale === "log") {
    for (let d = Math.floor(Math.log10(a.lo)); d <= Math.ceil(Math.log10(a.hi)); d++) {
      for (const m of [1, 2, 5]) {
        const v = m * 10 ** d;
        if (v >= a.lo && v <= a.hi) out.push({ v, label: m === 1 || m === 5 ? spelled(v, a.unit) : undefined });
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

/** One step of knob `k` from `v` (taste.md §5.2): a 1% ratio in Hz or on a log knob, 0.1 dB, a semitone, else 1% of its range. */
function step(k: NumberKnob, v: number, n: number) {
  if (k.unit === "hz" || k.log) return v * 1.01 ** n;
  if (k.unit === "db") return v + 0.1 * n;
  if (k.unit === "semitones") return v + n;
  return v + (k.range ? (k.range[1] - k.range[0]) / 100 : Math.max(Math.abs(v), 1) / 100) * n;
}

const inRange = (k: NumberKnob, v: number) => (k.range ? Math.min(k.range[1], Math.max(k.range[0], v)) : v);

/** A card's picture (CARD-69): asked for while it shows, drawn from its shapes, its dots moving the card's values. */
export function PictureView({ view, ...p }: ViewProps & { view: PictureView }) {
  const box = useRef<HTMLDivElement>(null);
  const svg = useRef<SVGSVGElement>(null);
  const [drawn, setDrawn] = useState<Drawn | null>(null);
  const [asked, setAsked] = useState<string | null>(null);
  const count = useRef(0);
  const { session, module } = p;
  const node = p.node.id;
  const name = view.name;
  // Looked at: the card on screen, in a window that shows (CARD-69, as a look, TAP-16).
  const looking = useLooking(box);
  // The view's values as it is drawn: a new view asks again at once.
  const values = JSON.stringify(view.values);
  useEffect(() => {
    if (!looking) return;
    let alive = true;
    const take = (d: Drawn) => {
      if (!alive) return;
      count.current += 1;
      box.current?.setAttribute("data-drawn", String(count.current));
      setDrawn(d);
    };
    const ask = (on: boolean) =>
      graphApi.picture(session, module, node, name, on).then(
        (a) => (on && a.picture ? take(a.picture) : undefined),
        (e) => alive && on && setAsked(said(e)),
      );
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
  }, [session, module, node, name, looking, values]);
  const why = asked ?? drawn?.why ?? null;
  return (
    <div ref={box} className="h-full" data-picture={name} data-asking={looking || undefined}>
      <div
        className={clsx("relative h-full rounded-sm ring-1 ring-line bg-canvas overflow-hidden", why && "opacity-60")}
        data-greyed={why ? true : undefined}
        role="img"
        aria-label={why ?? drawn?.words ?? `${p.owner}'s picture`}
        title={why ?? drawn?.words}
      >
        {drawn?.shapes && !why && view.live?.length ? <Live view={view} shapes={drawn.shapes} looking={looking} {...p} /> : null}
        {drawn?.shapes && !why && <Shapes svg={svg} view={view} shapes={drawn.shapes} {...p} />}
        {why && <p className="absolute inset-0 p-2 text-xs text-fg-muted">{why}</p>}
      </div>
    </div>
  );
}

/** The grid and the shapes, and the dots' gestures. */
function Shapes({ svg, view, shapes, ...p }: ViewProps & { svg: React.RefObject<SVGSVGElement | null>; view: PictureView; shapes: Shape[] }) {
  const H = PICTURE_H - 8;
  const plot = H - FOOT;
  const X = (v: number) => along(view.x, v) * W;
  const Y = (v: number) => (1 - along(view.y, v)) * plot;
  const drag = useViewDrag(p);
  const toast = useStore((s) => s.toast);
  const tab = useCards((s) => s.tabs[p.node.id]);
  const [held, setHeld] = useState<{ i: number; x: number | null; y: number | null } | null>(null);
  const [picked, setPicked] = useState<number | null>(null);
  // A tab picked marks the dot that opens it (CARD-71).
  useEffect(() => {
    const i = handles.findIndex((h) => h.opens !== undefined && h.opens === tab);
    if (i >= 0) setPicked(i);
  }, [tab]);
  useEffect(() => attach(), []);
  const hearing = useAudition((s) => s.on);
  const notes = p.node.inputs.find((i) => i.kind === "notes" && i.lane)?.lane;
  const playing = useRef<number | null>(null);
  const knobOf = (m?: Moved): NumberKnob | null => {
    const k: Knob | undefined = m?.knob ? p.node.knobs.find((x) => x.id === m.knob) : undefined;
    return k && !picks(k) ? (k as NumberKnob) : null;
  };
  const anyKnob = (m?: Moved): Knob | undefined => (m?.knob ? p.node.knobs.find((x) => x.id === m.knob) : undefined);
  const flat: Shape[] = [];
  const flatten = (ss: Shape[]) => ss.forEach((s) => (s.kind === "part" ? flatten(s.shapes) : flat.push(s)));
  flatten(shapes);
  const handles = flat.filter((s): s is Handle => s.kind === "handle");
  const stepName = (ks: (Knob | NumberKnob)[]) => `${p.owner} ${ks.map((k) => inSentence(controlLabel(k))).join(" and ")}`;
  /** Why dot `h` can't be moved, when it can't (CARD-66). */
  const fixed = (h: Handle): string | null => {
    const moves = Object.values(h.moves);
    const stuck = moves.find((m) => m && !m.knob);
    if (stuck) return `${stuck.name} can't be moved here: ${stuck.why ?? "it isn't a knob of this card"}.`;
    if (!p.editable) return `${p.locked ?? "It can't be edited right now"}.`;
    return moves.length ? null : "This dot moves nothing.";
  };
  const pick = (i: number) => {
    setPicked(i);
    const opens = handles[i].opens;
    if (opens) useCards.getState().tab(p.node.id, opens);
  };
  /** One write of one value, one undo step (CARD-70): a step, or a flip. */
  const write = (k: Knob, value: number | boolean) => {
    void graphApi.setValue(p.session, p.revision, k.id, value, { name: stepName([k]) }).catch((e) => toast(said(e)));
  };
  const nudge = (m: Moved | undefined, n: number) => {
    const k = knobOf(m);
    if (k) write(k, rounded(inRange(k, step(k, k.value, n)), k.unit));
  };
  const flip = (h: Handle) => {
    const k = anyKnob(h.moves.toggles);
    if (k) write(k, k.value !== true);
  };
  const down = (e: React.PointerEvent<SVGCircleElement>, i: number) => {
    const h = handles[i];
    pick(i);
    if (e.button !== 0 || !svg.current || fixed(h)) return;

    e.stopPropagation();
    e.preventDefault();
    const el = e.currentTarget, pointer = e.pointerId;
    el.setPointerCapture(pointer);
    const r = svg.current.getBoundingClientRect();
    const sx = W / r.width, sy = H / r.height;
    const [mx, my] = [h.moves.x, h.moves.y];
    const kx = knobOf(mx), ky = knobOf(my);
    const knobs = [kx, ky].filter((k): k is NumberKnob => !!k);
    if (!knobs.length) return;
    drag.begin(knobs);
    const start = knobs.map((k) => k.value);
    const x0 = e.clientX, y0 = e.clientY;
    /** How far `d` drawing units move a value from `v` (CARD-70): on its axis in its scale, or as its knob's drag (300 px). */
    const moved = (m: Moved, k: NumberKnob, a: Axis, v: number, d: number, span: number) => {
      if (m.by === "knob") {
        const [lo, hi] = k.range ?? [a.lo, a.hi];
        return k.log && lo > 0 ? v * (hi / lo) ** (d / 300) : v + (d / 300) * (hi - lo);
      }
      return a.scale === "log" ? v * (a.hi / a.lo) ** (d / span) : v + (d / span) * (a.hi - a.lo);
    };
    const values = (ev: PointerEvent) => {
      const dx = (ev.clientX - x0) * sx, dy = -(ev.clientY - y0) * sy;
      const out: number[] = [];
      if (kx && mx) out.push(rounded(inRange(kx, moved(mx, kx, view.x, start[0], mx.by === "knob" ? (ev.clientX - x0) : dx, W)), kx.unit));
      if (ky && my) out.push(rounded(inRange(ky, moved(my, ky, view.y, start[knobs.length - 1], my.by === "knob" ? -(ev.clientY - y0) : dy, plot)), ky.unit));
      return out;
    };
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      const vs = values(ev);
      drag.move(vs);
      // The dot follows the pointer; the rest of the picture each `pictures` message (CARD-70).
      const vx = kx && mx && mx.by !== "knob" ? vs[0] : h.x;
      const vy = ky && my && my.by !== "knob" ? vs[vs.length - 1] : h.y;
      setHeld({ i, x: vx, y: vy });
    };
    const up = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      setHeld(null);
      if (ev.type === "pointerup") void drag.end(stepName(knobs));
      else void drag.cancel();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };
  const key = (e: React.KeyboardEvent, h: Handle) => {
    const n = e.shiftKey ? 10 : 1;
    if (fixed(h)) return;
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) nudge(h.moves.wheel, e.key === "ArrowUp" ? n : -n);
    else if (e.key === "ArrowRight" || e.key === "ArrowLeft") nudge(h.moves.x, e.key === "ArrowRight" ? n : -n);
    else if (e.key === "ArrowUp" || e.key === "ArrowDown") nudge(h.moves.y, e.key === "ArrowUp" ? n : -n);
    else if (e.key === "Enter" || e.key === " ") flip(h);
    else return;
    e.preventDefault();
  };
  const play = (e: React.PointerEvent<SVGRectElement>, s: Extract<Shape, { kind: "play" }>) => {
    if (e.button !== 0 || !notes || !svg.current) return;
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
    <svg ref={svg} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="w-full h-full" data-shapes={flat.length}>
      {ticks(view.x).map((t) => (
        <g key={`x${t.v}`}>
          <line x1={X(t.v)} x2={X(t.v)} y1={0} y2={plot} stroke="var(--color-line)" strokeWidth={0.5} vectorEffect="non-scaling-stroke" />
          {t.label && (
            <text x={X(t.v) + 2} y={H - 2} fontSize={9} fill="var(--color-fg-faint)">
              {t.label}
            </text>
          )}
        </g>
      ))}
      {ticks(view.y).map((t) => (
        <g key={`y${t.v}`}>
          <line x1={0} x2={W} y1={Y(t.v)} y2={Y(t.v)} stroke="var(--color-line)" strokeWidth={0.5} vectorEffect="non-scaling-stroke" />
          {t.label && (
            <text x={2} y={Y(t.v) - 2} fontSize={9} fill="var(--color-fg-faint)">
              {t.label}
            </text>
          )}
        </g>
      ))}
      {flat.map((s, i) => {
        switch (s.kind) {
          case "line":
            return <polyline key={i} data-line={s.name} points={points(s.points)} fill="none" stroke={TONE[s.tone ?? "main"]} strokeWidth={1.25} vectorEffect="non-scaling-stroke" />;
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
              <line key={i} x1={s.axis === "x" ? v : 0} x2={s.axis === "x" ? v : W} y1={s.axis === "x" ? 0 : v} y2={s.axis === "x" ? plot : v} stroke="var(--color-fg-faint)" strokeDasharray="3 3" strokeWidth={1} vectorEffect="non-scaling-stroke">
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
      {handles.map((h, i) => {
        const x = held?.i === i ? held.x : h.x, y = held?.i === i ? held.y : h.y;
        if (x === null || y === null) return null;
        const cx = X(x), cy = Y(y);
        const stuck = fixed(h);
        const marked = picked === i || held?.i === i;
        const names = Object.values(h.moves).map((m) => m?.name).filter(Boolean).join(", ");
        return (
          <g key={`h${i}`}>
            <ellipse
              cx={cx}
              cy={cy}
              rx={3.5 * (W / (svg.current?.getBoundingClientRect().width || W))}
              ry={3.5 * (H / (svg.current?.getBoundingClientRect().height || H))}
              fill={marked ? "var(--color-accent)" : h.off || stuck ? "var(--color-raised)" : "var(--color-fg)"}
              stroke={marked ? "var(--color-accent)" : stuck ? "var(--color-fg-off)" : "var(--color-fg)"}
              strokeWidth={1.5}
              vectorEffect="non-scaling-stroke"
              pointerEvents="none"
            />
            {h.mark && (
              <text x={cx} y={cy - 7} fontSize={9} textAnchor="middle" fill="var(--color-fg-muted)" pointerEvents="none">
                {h.mark}
              </text>
            )}
            <circle
              data-dot={i}
              data-opens={h.opens}
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
              onPointerDown={(e) => down(e, i)}
              onFocus={() => pick(i)}
              onKeyDown={(e) => key(e, h)}
              // A double-click flips the dot's switch: one write, never a drag (CARD-70).
              onClick={(e) => e.detail === 2 && !stuck && flip(h)}
              onWheel={(e) => {
                if (stuck || !h.moves.wheel) return;
                e.preventDefault();
                nudge(h.moves.wheel, e.deltaY < 0 ? 1 : -1);
              }}
            >
              <title>{stuck ?? `${names}: drag to move${h.moves.wheel ? ", scroll for " + h.moves.wheel.name : ""}${h.moves.toggles ? ", double-click to turn it on or off" : ""}`}</title>
            </circle>
          </g>
        );
      })}
    </svg>
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
/** A spectrum's and a level's dB, from the foot of the picture to its top. */
const DB_FOOT = -90;

/**
 * A picture's live layers, on a canvas under its shapes (CARD-73): each looks at what plays while
 * the picture shows (`useLook`, one ask for each look in this window), and is drawn as its reads
 * come. A layer that can't come says why, small, in the corner; the shapes stay.
 */
function Live({ view, shapes, looking, session, module, node }: ViewProps & { view: PictureView; shapes: Shape[]; looking: boolean }) {
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
  // The playhead moves with the song's beat, read from the clock, at the screen's rate.
  const playhead = layers.some((l) => l.kind === "playhead" && (l.from ?? "beat") === "beat");
  useEffect(() => {
    if (!playhead || !looking) return;
    let alive = true;
    const tick = () => {
      if (!alive) return;
      draw();
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    return () => {
      alive = false;
    };
  }, [playhead, looking]);
  const draw = () => {
    const c = canvas.current;
    const g = c?.getContext("2d");
    if (!c || !g) return;
    const r = c.getBoundingClientRect();
    if (c.width !== Math.round(r.width * 2) || c.height !== Math.round(r.height * 2)) [c.width, c.height] = [Math.round(r.width * 2), Math.round(r.height * 2)];
    const css = getComputedStyle(c);
    const grey = (t?: string) => css.getPropertyValue(t === "faint" ? "--color-fg-faint" : t === "soft" ? "--color-fg-muted" : "--color-fg-muted").trim() || "#8a8f96";
    const plot = c.height * ((PICTURE_H - 8 - FOOT) / (PICTURE_H - 8));
    const px = (v: number, a: Axis) => Math.min(1, Math.max(0, along(a, v)));
    const dbY = (db: number) => plot * (1 - Math.min(1, Math.max(0, (db - DB_FOOT) / -DB_FOOT)));
    g.clearRect(0, 0, c.width, c.height);
    layers.forEach((l, i) => {
      const all = seen.current.get(i) ?? [];
      const last = all[all.length - 1];
      g.strokeStyle = g.fillStyle = grey((l as { tone?: string }).tone);
      g.lineWidth = 2;
      if (l.kind === "spectrum" && last?.bins && view.x.unit === "hz") {
        // A twelfth of an octave each, from 20 Hz: a faint fill from the foot.
        g.globalAlpha = 0.35;
        g.beginPath();
        g.moveTo(0, plot);
        last.bins.forEach((db, j) => g.lineTo(px(20 * 2 ** (j / 12), view.x) * c.width, dbY(db)));
        g.lineTo(c.width, plot);
        g.closePath();
        g.fill();
        g.globalAlpha = 1;
      } else if (l.kind === "level" && last?.peak) {
        const db = 20 * Math.log10(Math.max(1e-6, Math.max(...last.peak)));
        const on = l.on ? (flatShapes(shapes).find((s) => s.kind === "line" && s.name === l.on) as { points: number[] } | undefined) : undefined;
        if (on && view.x.unit === "db") {
          // A dot riding its line: x the level, y the line there (CARD-73).
          const x = px(db, view.x) * c.width;
          const y = (1 - px(onLine(on.points, db), view.y)) * plot;
          g.beginPath();
          g.arc(x, y, 6, 0, 2 * Math.PI);
          g.fill();
        } else g.fillRect(0, plot - 4, ((db - DB_FOOT) / -DB_FOOT) * c.width, 4);
      } else if (l.kind === "meter" && last?.now !== undefined) {
        // A bar at the right edge, from the axis's 0 to the meter's number, in the y axis's unit.
        const y0 = (1 - px(Math.min(Math.max(0, view.y.lo), view.y.hi), view.y)) * plot, y1 = (1 - px(last.now, view.y)) * plot;
        g.fillRect(c.width - 10, Math.min(y0, y1), 8, Math.abs(y1 - y0));
      } else if (l.kind === "history" && all.length > 1) {
        g.beginPath();
        all.forEach((s, j) => {
          const v = s.peak ? 20 * Math.log10(Math.max(1e-6, Math.max(...s.peak))) : (s.now ?? 0);
          const y = s.peak ? dbY(v) : (1 - px(v, view.y)) * plot;
          const x = ((j + HISTORY - all.length) / (HISTORY - 1)) * c.width;
          if (j) g.lineTo(x, y);
          else g.moveTo(x, y);
        });
        g.stroke();
      } else if (l.kind === "playhead") {
        const graph = useStore.getState().graph;
        let t: number | null = null;
        if ((l.from ?? "beat") === "beat" && graph) {
          const beat = beatNow();
          const bar = barAt(graph.timeline.meter, Math.max(0, beat));
          t = (beat - bar.start) / bar.length;
        } else if (last?.now !== undefined) t = px(last.now, view.x);
        if (t !== null) g.fillRect(t * c.width - 1, 0, 2, plot);
      } else if (l.kind === "scope" && last?.frames?.length) {
        const f = last.frames;
        g.beginPath();
        for (let j = 0; j < f[0].length; j++) {
          let v = 0;
          for (const ch of f) v += ch[j] ?? 0;
          const x = (j / Math.max(1, f[0].length - 1)) * c.width, y = (1 - (Math.max(-1, Math.min(1, v / f.length)) + 1) / 2) * plot;
          if (j) g.lineTo(x, y);
          else g.moveTo(x, y);
        }
        g.stroke();
      }
    });
    c.dataset.reads = String(reads.current);
  };
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
