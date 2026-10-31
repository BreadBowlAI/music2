// A card's views (CARD-26 to CARD-29): pictures over values that have homes, drawn in music2's look
// (taste.md §2.8). Each holds no value of its own.
//
// - Envelope: the attack, decay, sustain and release as a shape. Its corners move them: the peak the
//   attack, the decay's corner the decay and the sustain, the end the release.
// - Filter: a 12 dB per octave resonant low-pass curve from cutoff and q, a picture of a low-pass,
//   not a measurement of the sound. Its peak moves cutoff (across, on a log scale) and q (up).
// - Keys: which recording plays on which keys and how hard; a click plays the key, as hard as high.
// - Scope: the call's sound as it plays, asked of the server only while it shows (TAP-8).
//
// A drag holds a lease on each control it moves (WRITE-22), heard as it goes, and is written at
// release as one edit, one undo step (CARD-22).

import { useEffect, useRef, useState, type ReactNode } from "react";
import { attach, release, strike } from "../lib/audition";
import { on } from "../lib/events";
import { graphApi, picks, type CardView, type KeyRegion, type NumberKnob, type ViewNode, type ViewPart } from "../lib/graph";
import { useStore } from "../lib/store";
import { words } from "./Settings";

/** How tall each view is on its card, in px. */
export const VIEW_H: Record<CardView["kind"], number> = { envelope: 88, filter: 88, keys: 72, scope: 64 };

/** The inner width a view draws in: a card's, less its sides. */
const W = 184;

/** What every view is given: its card, and what it needs to write. */
export interface ViewProps {
  node: ViewNode;
  /** Its module's key: where a scope reads and a key plays. */
  module: string;
  session: number;
  revision: number;
  editable: boolean;
  /** Why nothing can be changed now, when that is so. */
  locked: string | null;
  /** The card's title, to name an edit ("Wobble amp"). */
  owner: string;
}

/** One view on its card, as its kind draws it. */
export function ViewRow({ view, ...p }: ViewProps & { view: CardView }) {
  return (
    <div className="relative px-4 py-1" style={{ height: VIEW_H[view.kind] }} data-view={view.name} data-view-kind={view.kind}>
      {view.kind === "envelope" ? (
        <EnvelopeView view={view} {...p} />
      ) : view.kind === "filter" ? (
        <FilterView view={view} {...p} />
      ) : view.kind === "keys" ? (
        <KeysView view={view} {...p} />
      ) : (
        <ScopeView {...p} />
      )}
    </div>
  );
}

/** A part's knob on the card, when the card can move it and it is a number. */
function knobOf(node: ViewNode, part: ViewPart): NumberKnob | null {
  const k = part.knob ? node.knobs.find((x) => x.id === part.knob) : undefined;
  return k && !picks(k) ? k : null;
}

/** Why the view can't move `parts` now, in one sentence, or null when it can. */
function blocked(p: ViewProps, parts: [string, ViewPart][]): string | null {
  if (!p.editable) return `${p.locked ?? "It can't be edited right now"}.`;
  const stuck = parts.find(([, part]) => !knobOf(p.node, part));
  if (!stuck) return null;
  const [name, part] = stuck;
  return `${name.charAt(0).toUpperCase()}${name.slice(1)} can't be moved here: ${part.why ?? "it isn't a knob of this card"}.`;
}

/**
 * One drag of a view over the knobs it moves (WRITE-22, CARD-22): each value heard as it moves
 * (`/preview` begins each knob's lease under one gesture, and keeps it alive while held still), and
 * all of them written at release by one `/set` with `with`: one undo step.
 */
function useViewDrag(p: ViewProps) {
  const toast = useStore((s) => s.toast);
  type Live = { token: string; knobs: NumberKnob[]; values: number[]; sent: (number | null)[]; chain: Promise<void>; keep: number };
  const live = useRef<Live | null>(null);
  /** Each value not sent yet (with `all`, every value: the keepalive), in order. */
  const send = (d: Live, all = false) => {
    d.knobs.forEach((k, i) => {
      const v = d.values[i];
      if (!all && v === (d.sent[i] ?? k.value)) return;
      d.sent[i] = v;
      d.chain = d.chain.then(() => graphApi.preview(p.session, p.revision, k.id, d.token, v)).catch((e) => toast(words(e)));
    });
  };
  /** Each lease begun ends: the code's values play again. */
  const cancel = async () => {
    const d = live.current;
    live.current = null;
    if (!d) return;
    clearInterval(d.keep);
    await d.chain;
    for (const [i, k] of d.knobs.entries()) if (d.sent[i] !== null) await graphApi.cancel(p.session, k.id, d.token).catch(() => {});
  };
  useEffect(() => () => void cancel(), []);
  return {
    /** A drag of `knobs` begins. */
    begin(knobs: NumberKnob[]) {
      const d: Live = { token: crypto.randomUUID(), knobs, values: knobs.map((k) => k.value), sent: knobs.map(() => null), chain: Promise.resolve(), keep: 0 };
      d.keep = window.setInterval(() => live.current === d && d.sent.some((x) => x !== null) && send(d, true), 2000);
      live.current = d;
    },
    /** Its knobs' values now, each in its own unit: heard at once. */
    move(values: number[]) {
      const d = live.current;
      if (!d) return;
      d.values = values;
      send(d);
    },
    /** Let go: what it shows is written, as one undo step named `name`; every knob it moved in it. */
    async end(name: string) {
      const d = live.current;
      if (!d) return;
      if (d.knobs.every((k, i) => d.values[i] === k.value)) return cancel();
      live.current = null;
      clearInterval(d.keep);
      await d.chain;
      const all = d.knobs.map((k, i): [string, number] => [k.id, d.values[i]]);
      await graphApi.setWith(p.session, p.revision, d.token, all, { name }).catch((e) => toast(words(e)));
    },
    cancel,
  };
}

/** A drag on an SVG: `at` gives the pointer in the drawing's own units (`w` × `h`). */
function svgPoint(svg: SVGSVGElement, e: { clientX: number; clientY: number }, w: number, h: number) {
  const r = svg.getBoundingClientRect();
  return { x: ((e.clientX - r.left) / r.width) * w, y: ((e.clientY - r.top) / r.height) * h };
}

/** A corner a drag can take: a 7 px circle, `accent` while held. */
function Handle({ x, y, id, held, title, disabled, onDown }: { x: number; y: number; id: string; held: boolean; title: string; disabled: boolean; onDown: (e: React.PointerEvent<SVGCircleElement>) => void }) {
  return (
    <>
      <circle cx={x} cy={y} r={3.5} fill={held ? "var(--color-accent)" : "var(--color-raised)"} stroke={held ? "var(--color-accent)" : "var(--color-fg)"} strokeWidth={1.5} pointerEvents="none" />
      <circle
        data-handle={id}
        cx={x}
        cy={y}
        r={9}
        fill="transparent"
        className={disabled ? "cursor-default" : "cursor-grab"}
        onPointerDown={disabled ? undefined : onDown}
      >
        <title>{title}</title>
      </circle>
    </>
  );
}

// ---------------------------------------------------------------- envelope (CARD-27)

/** Seconds of a time value in `unit` (beats at the song's first tempo). */
const seconds = (v: number, unit: string | null, bpm: number) => (unit === "beats" ? (v * 60) / bpm : v);
const fromSeconds = (s: number, unit: string | null, bpm: number) => (unit === "beats" ? (s * bpm) / 60 : s);
/** A level from 0 to 1 of a sustain in `unit`: dB as its gain. */
const level = (v: number, unit: string | null) => Math.min(1, Math.max(0, unit === "db" ? 10 ** (v / 20) : v));
const fromLevel = (l: number, unit: string | null) => (unit === "db" ? Math.round(20 * Math.log10(Math.max(l, 0.001)) * 10) / 10 : Math.round(l * 100) / 100);
/** A time rounded to a millisecond (a hundredth of a beat), never 0. */
const roundTime = (s: number, unit: string | null) => Math.max(unit === "beats" ? 0.01 : 0.001, unit === "beats" ? Math.round(s * 100) / 100 : Math.round(s * 1000) / 1000);

function EnvelopeView({ view, ...p }: ViewProps & { view: Extract<CardView, { kind: "envelope" }> }) {
  const bpm = useStore((s) => s.graph?.timeline.tempo[0]?.bpm ?? 120);
  const svg = useRef<SVGSVGElement>(null);
  const drag = useViewDrag(p);
  const [held, setHeld] = useState<{ corner: string; values: number[]; scale: number } | null>(null);
  const parts: [string, ViewPart][] = [["attack", view.attack], ["decay", view.decay], ["sustain", view.sustain], ["release", view.release]];
  const knobs = parts.map(([, part]) => knobOf(p.node, part));
  const why = blocked(p, parts);
  // What each plays: its knob's value, or one to draw by when the card can't show it.
  const now = knobs.map((k, i) => k?.value ?? [0.01, 0.2, 0.6, 0.3][i]);
  const values = held?.values ?? now;
  const units = knobs.map((k) => k?.unit ?? null);
  const H = VIEW_H.envelope - 8;
  const [a, d, r] = [0, 1, 3].map((i) => seconds(values[i], units[i], bpm));
  const s = level(values[2], units[2]);
  const hold = Math.max(0.25 * (a + d + r), 0.05);
  // Pixels a second: kept while a corner is held, so the corner stays under the pointer.
  const k = held?.scale ?? (W - 8) / (a + d + hold + r);
  const x = { peak: 4 + a * k, decay: 4 + (a + d) * k, hold: 4 + (a + d + hold) * k, end: 4 + (a + d + hold + r) * k };
  const y = (l: number) => 4 + (1 - l) * (H - 8);
  const press = (corner: "attack" | "decay" | "release") => (e: React.PointerEvent<SVGCircleElement>) => {
    if (e.button !== 0 || why) return;
    e.stopPropagation();
    e.preventDefault();
    const el = e.currentTarget, pointer = e.pointerId;
    el.setPointerCapture(pointer);
    const used = corner === "attack" ? [0] : corner === "decay" ? [1, 2] : [3];
    drag.begin(used.map((i) => knobs[i]!));
    const scale = k;
    let vals = [...now];
    setHeld({ corner, values: vals, scale });
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer || !svg.current) return;
      const at = svgPoint(svg.current, ev, W, H);
      const sec = (at.x - 4) / scale;
      const [a0, d0] = [seconds(vals[0], units[0], bpm), seconds(vals[1], units[1], bpm)];
      vals = [...vals];
      if (corner === "attack") vals[0] = roundTime(fromSeconds(sec, units[0], bpm), units[0]);
      if (corner === "decay") {
        vals[1] = roundTime(fromSeconds(sec - a0, units[1], bpm), units[1]);
        vals[2] = fromLevel(Math.min(1, Math.max(0, 1 - (at.y - 4) / (H - 8))), units[2]);
      }
      if (corner === "release") vals[3] = roundTime(fromSeconds(sec - a0 - d0 - hold, units[3], bpm), units[3]);
      setHeld({ corner, values: vals, scale });
      drag.move(used.map((i) => vals[i]));
    };
    const end = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", end);
      el.removeEventListener("pointercancel", end);
      if (ev.type === "pointerup") void drag.end(`${p.owner} ${view.name}`).finally(() => setHeld(null));
      else void drag.cancel().finally(() => setHeld(null));
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", end);
    el.addEventListener("pointercancel", end);
  };
  const said = (i: number) => (units[i] === "beats" ? `${values[i]} beats` : units[i] === "db" ? `${values[i]} dB` : i === 2 ? `${values[i]}` : `${Math.round(values[i] * 1000)} ms`);
  const shape = `M4,${y(0)} L${x.peak},${y(1)} L${x.decay},${y(s)} L${x.hold},${y(s)} L${x.end},${y(0)}`;
  return (
    <Picture title={why ?? `Attack ${said(0)} · decay ${said(1)} · sustain ${said(2)} · release ${said(3)}\nDrag a corner to change them`}>
      <svg ref={svg} viewBox={`0 0 ${W} ${H}`} className="w-full h-full overflow-visible" data-envelope>
        <line x1={0} x2={W} y1={y(0)} y2={y(0)} stroke="var(--color-line)" />
        <path d={`${shape} Z`} fill="var(--color-fg)" fillOpacity={0.06} stroke="none" />
        <path d={shape} fill="none" stroke={held ? "var(--color-accent)" : "var(--color-fg)"} strokeWidth={1.5} strokeLinejoin="round" />
        <Handle id="attack" x={x.peak} y={y(1)} held={held?.corner === "attack"} title={`Attack: ${said(0)}`} disabled={!!why} onDown={press("attack")} />
        <Handle id="decay" x={x.decay} y={y(s)} held={held?.corner === "decay"} title={`Decay: ${said(1)} · sustain: ${said(2)}`} disabled={!!why} onDown={press("decay")} />
        <Handle id="release" x={x.end} y={y(0)} held={held?.corner === "release"} title={`Release: ${said(3)}`} disabled={!!why} onDown={press("release")} />
      </svg>
    </Picture>
  );
}

/** A view's frame: a 1 px `line` box, its words in the tooltip. */
function Picture({ title, children, caption }: { title: string; children: ReactNode; caption?: string }) {
  return (
    <div className="relative h-full rounded-sm ring-1 ring-line bg-canvas overflow-hidden" title={title}>
      {children}
      {caption && <span className="absolute right-1 top-0.5 text-xs text-fg-faint pointer-events-none">{caption}</span>}
    </div>
  );
}

// ---------------------------------------------------------------- filter (CARD-27)

/** The lowest and highest frequency drawn, and the dB at the foot and the top. */
const F_LO = 20, F_HI = 20000, DB_LO = -30, DB_HI = 24;

/** A two-pole low-pass's gain at `f`, in dB, for `cutoff` and `q`: 1 / √((1 − r²)² + (r / q)²), r = f / cutoff. */
export function lowpassDb(f: number, cutoff: number, q: number) {
  const r = f / cutoff;
  return -10 * Math.log10((1 - r * r) ** 2 + (r / q) ** 2);
}

function FilterView({ view, ...p }: ViewProps & { view: Extract<CardView, { kind: "filter" }> }) {
  const svg = useRef<SVGSVGElement>(null);
  const drag = useViewDrag(p);
  const [held, setHeld] = useState<number[] | null>(null);
  const parts: [string, ViewPart][] = [["cutoff", view.cutoff], ["q", view.q]];
  const knobs = parts.map(([, part]) => knobOf(p.node, part));
  const why = blocked(p, parts);
  const [cutoff, q] = held ?? [knobs[0]?.value ?? 1000, knobs[1]?.value ?? 0.707];
  const H = VIEW_H.filter - 8;
  const xOf = (f: number) => (Math.log(f / F_LO) / Math.log(F_HI / F_LO)) * W;
  const yOf = (db: number) => (1 - (Math.min(DB_HI, Math.max(DB_LO, db)) - DB_LO) / (DB_HI - DB_LO)) * H;
  const curve = Array.from({ length: 97 }, (_, i) => {
    const f = F_LO * (F_HI / F_LO) ** (i / 96);
    return `${i ? "L" : "M"}${xOf(f).toFixed(1)},${yOf(lowpassDb(f, cutoff, q)).toFixed(1)}`;
  }).join(" ");
  const peak = { x: xOf(cutoff), y: yOf(20 * Math.log10(q)) };
  const press = (e: React.PointerEvent<SVGCircleElement>) => {
    if (e.button !== 0 || why) return;
    e.stopPropagation();
    e.preventDefault();
    const el = e.currentTarget, pointer = e.pointerId;
    el.setPointerCapture(pointer);
    drag.begin([knobs[0]!, knobs[1]!]);
    setHeld([cutoff, q]);
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer || !svg.current) return;
      const at = svgPoint(svg.current, ev, W, H);
      const f = Math.round(Math.min(F_HI, Math.max(F_LO, F_LO * (F_HI / F_LO) ** (at.x / W))));
      const db = DB_HI - (at.y / H) * (DB_HI - DB_LO);
      const nq = Math.round(Math.min(40, Math.max(0.1, 10 ** (db / 20))) * 100) / 100;
      setHeld([f, nq]);
      drag.move([f, nq]);
    };
    const end = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", end);
      el.removeEventListener("pointercancel", end);
      if (ev.type === "pointerup") void drag.end(`${p.owner} ${view.name}`).finally(() => setHeld(null));
      else void drag.cancel().finally(() => setHeld(null));
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", end);
    el.addEventListener("pointercancel", end);
  };
  const hz = cutoff < 1000 ? `${Math.round(cutoff)} Hz` : `${(cutoff / 1000).toFixed(1)} kHz`;
  return (
    <Picture
      caption="Low-pass, drawn"
      title={why ?? `A picture of a 12 dB per octave low-pass at ${hz}, q ${q}: not a measurement of the sound\nDrag its peak: across for the cutoff, up for the q`}
    >
      <svg ref={svg} viewBox={`0 0 ${W} ${H}`} className="w-full h-full overflow-visible" data-filter>
        <line x1={0} x2={W} y1={yOf(0)} y2={yOf(0)} stroke="var(--color-line)" />
        <path d={`${curve} L${W},${H} L0,${H} Z`} fill="var(--color-fg)" fillOpacity={0.06} stroke="none" />
        <path d={curve} fill="none" stroke={held ? "var(--color-accent)" : "var(--color-fg)"} strokeWidth={1.5} />
        <Handle id="peak" x={peak.x} y={peak.y} held={!!held} title={`Cutoff ${hz} · q ${q}`} disabled={!!why} onDown={press} />
      </svg>
    </Picture>
  );
}

// ---------------------------------------------------------------- keys (CARD-28)

/** Whether MIDI note `n` is a black key. */
const black = (n: number) => [1, 3, 6, 8, 10].includes(n % 12);
const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
const noteName = (n: number) => `${NAMES[n % 12]}${Math.floor(n / 12) - 1}`;

function KeysView({ view }: ViewProps & { view: Extract<CardView, { kind: "keys" }> }) {
  useEffect(() => attach(), []);
  const regions: KeyRegion[] = view.regions;
  const held = useRef<number | null>(null);
  const [playing, setPlaying] = useState<number | null>(null);
  if (view.why) return <Picture title={view.why}><p className="p-2 text-xs text-fg-muted">{view.why.charAt(0).toUpperCase() + view.why.slice(1)}.</p></Picture>;
  // The keys its regions cover, two either side, at least two octaves.
  let lo = Math.max(0, Math.min(...regions.map((r) => r.keys[0]), 60) - 2);
  let hi = Math.min(127, Math.max(...regions.map((r) => r.keys[1]), 60) + 2);
  while (hi - lo + 1 < 24) {
    if (lo > 0) lo -= 1;
    if (hi - lo + 1 < 24 && hi < 127) hi += 1;
  }
  const n = hi - lo + 1;
  const H = VIEW_H.keys - 8;
  const KEYS_H = 14;
  const kw = W / n;
  const vy = (v: number) => ((127 - v) / 127) * (H - KEYS_H);
  const lane = view.lane;
  const play = (e: React.PointerEvent<SVGSVGElement>) => {
    if (e.button !== 0 || !lane) return;
    const at = svgPoint(e.currentTarget, e, W, H);
    const pitch = Math.min(hi, lo + Math.floor(at.x / kw));
    // As hard as high: the keys' strip plays at the usual velocity.
    const velocity = at.y >= H - KEYS_H ? 0.8 : Math.min(1, Math.max(0.01, 1 - at.y / (H - KEYS_H)));
    e.currentTarget.setPointerCapture(e.pointerId);
    held.current = strike(lane.module, lane.lane, pitch, velocity, { always: true });
    setPlaying(pitch);
  };
  const lift = () => {
    release(held.current);
    held.current = null;
    setPlaying(null);
  };
  const files = [...new Set(regions.map((r) => r.file))];
  return (
    <Picture title={`${files.length} ${files.length === 1 ? "recording" : "recordings"} from ${noteName(lo)} to ${noteName(hi)}${lane ? "\nClick a key to hear it: higher plays it harder" : ""}`}>
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="w-full h-full cursor-pointer" data-keys onPointerDown={play} onPointerUp={lift} onPointerCancel={lift}>
        {regions.map((r, i) => (
          <rect
            key={i}
            data-region={r.file}
            x={(Math.max(r.keys[0], lo) - lo) * kw}
            width={(Math.min(r.keys[1], hi) - Math.max(r.keys[0], lo) + 1) * kw}
            y={vy(r.velocity[1])}
            height={Math.max(1, vy(r.velocity[0] - 1) - vy(r.velocity[1]))}
            fill="var(--color-accent)"
            fillOpacity={r.release ? 0.08 : 0.16}
            stroke="var(--color-accent)"
            strokeWidth={0.75}
          >
            <title>{`${r.file}: ${noteName(r.keys[0])} to ${noteName(r.keys[1])}, velocity ${r.velocity[0]} to ${r.velocity[1]}${r.round_robin[1] > 1 ? `, take ${r.round_robin[0]} of ${r.round_robin[1]}` : ""}${r.release ? ", when the note ends" : ""}`}</title>
          </rect>
        ))}
        {Array.from({ length: n }, (_, i) => (
          <rect
            key={i}
            data-key={lo + i}
            x={i * kw + 0.25}
            width={kw - 0.5}
            y={H - KEYS_H}
            height={KEYS_H}
            fill={playing === lo + i ? "var(--color-accent)" : black(lo + i) ? "var(--color-fg)" : "var(--color-raised)"}
            stroke="var(--color-line-strong)"
            strokeWidth={0.5}
          />
        ))}
      </svg>
    </Picture>
  );
}

// ---------------------------------------------------------------- scope (CARD-29)

/** How often a scope asks again: the server keeps an ask 10 s (TAP-8). */
const ASK_MS = 4000;

/** A scopes message (TAP-8): each scope asked by this window, its last frames per channel or why none. */
interface ScopesIn {
  t: "scopes";
  session?: number;
  scopes: { module: string; node: string; frames?: number[][]; why?: string }[];
}

function ScopeView(p: ViewProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [why, setWhy] = useState<string | null>(null);
  const drawn = useRef(0);
  const box = useRef<HTMLDivElement>(null);
  const { session, module } = p;
  const node = p.node.id;
  useEffect(() => {
    let alive = true;
    const ask = (on: boolean) =>
      graphApi.scope(session, module, node, on).then(
        () => alive && on && setWhy(null),
        (e) => alive && on && setWhy(words(e)),
      );
    void ask(true);
    const again = setInterval(() => void ask(true), ASK_MS);
    const off = on<ScopesIn>("scopes", (msg) => {
      const mine = msg.scopes.find((s) => s.module === module && s.node === node);
      if (!mine) return;
      if (mine.why) return setWhy(mine.why);
      const frames = mine.frames ?? [];
      const c = canvas.current;
      if (!c || !frames.length || !frames[0].length) return;
      setWhy(null);
      draw(c, frames);
      drawn.current += 1;
      box.current?.setAttribute("data-frames", String(drawn.current));
    });
    return () => {
      alive = false;
      clearInterval(again);
      off();
      void ask(false).catch(() => {});
    };
  }, [session, module, node]);
  return (
    <div ref={box} className="h-full" data-scope={node}>
      <Picture title={why ?? "The sound as it plays: the last frames it gave"}>
        <canvas ref={canvas} width={W * 2} height={(VIEW_H.scope - 8) * 2} className="w-full h-full" />
        {why && <p className="absolute inset-0 p-2 text-xs text-fg-muted">{why}</p>}
      </Picture>
    </div>
  );
}

/** The frames as one line, the channels' mean, from −1 at the foot to 1 at the top. */
function draw(c: HTMLCanvasElement, frames: number[][]) {
  const g = c.getContext("2d");
  if (!g) return;
  const css = getComputedStyle(c);
  g.clearRect(0, 0, c.width, c.height);
  g.strokeStyle = css.getPropertyValue("--color-line").trim() || "#dcdfe3";
  g.lineWidth = 1;
  g.beginPath();
  g.moveTo(0, c.height / 2);
  g.lineTo(c.width, c.height / 2);
  g.stroke();
  const len = frames[0].length;
  g.strokeStyle = css.getPropertyValue("--color-fg").trim() || "#33373c";
  g.lineWidth = 2;
  g.beginPath();
  for (let i = 0; i < len; i++) {
    let v = 0;
    for (const ch of frames) v += ch[i] ?? 0;
    v /= frames.length;
    const x = (i / Math.max(1, len - 1)) * c.width;
    const y = (1 - (Math.max(-1, Math.min(1, v)) + 1) / 2) * c.height;
    if (i) g.lineTo(x, y);
    else g.moveTo(x, y);
  }
  g.stroke();
}
