// A card's views (CARD-26 to CARD-29): pictures over values that have homes, drawn in music2's look
// (taste.md §2.8). Each holds no value of its own.
//
// - Envelope: the attack, decay, sustain and release as a shape. Its corners move them: the peak the
//   attack, the decay's corner the decay and the sustain, the end the release.
// - Filter: a resonant low-pass curve from cutoff and q, a picture of a low-pass, not a measurement
//   of the sound: drawn at 12 dB per octave whatever the filter's own slope (a view doesn't say it),
//   so its words name no slope. Its peak moves cutoff (across, on a log scale) and q (up).
// - Keys: which recording plays on which keys and how hard; a click plays the key, as hard as high.
// - Scope: the call's sound as it plays, asked of the server only while it shows (TAP-8).
//
// A drag holds a lease on each control it moves (WRITE-22), heard as it goes, and is written at
// release as one edit, one undo step (CARD-22).

import clsx from "clsx";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { attach, release, strike, useAudition } from "../lib/audition";
import { on } from "../lib/events";
import { controlLabel, graphApi, inSentence, picks, type CardView, type KeyRegion, type NumberKnob, type ViewNode, type ViewPart } from "../lib/graph";
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

/**
 * A part of a view as it is drawn (CARD-26, CARD-27): its knob, when the card can move it; else
 * what plays (a lane's fallback), which is drawn but fixed; and why the card can't move it.
 */
interface Drawn {
  name: string;
  knob: NumberKnob | null;
  value: number;
  unit: string | null;
  why: string | null;
}

/** Each of `parts`, as the view draws it; or why the view can't be drawn: a part with no value it can show. */
function drawn(p: ViewProps, parts: [string, ViewPart][]): { parts: Drawn[] } | { cant: string } {
  const out: Drawn[] = [];
  for (const [name, part] of parts) {
    const k = part.knob ? p.node.knobs.find((x) => x.id === part.knob) : undefined;
    const knob = k && !picks(k) ? k : null;
    const words = `${name.charAt(0).toUpperCase()}${name.slice(1)}`;
    if (knob) out.push({ name, knob, value: knob.value, unit: knob.unit, why: p.editable ? null : `${p.locked ?? "It can't be edited right now"}.` });
    else if (part.value != null) out.push({ name, knob: null, value: part.value, unit: part.unit ?? null, why: `${words} can't be moved here: ${part.why ?? "it isn't a knob of this card"}.` });
    else return { cant: `${words} can't be drawn here: ${part.why ?? "it isn't a knob of this card"}.` };
  }
  return { parts: out };
}

/** `v` inside knob `k`'s declared range, as a knob's own drag keeps it (PKG-8: a gesture, never the code). */
const inRange = (k: NumberKnob | null, v: number) => (k?.range ? Math.min(k.range[1], Math.max(k.range[0], v)) : v);

/** What an undo step of a view's drag is called: the card and what moved ("Wobble decay and sustain"). */
const stepName = (owner: string, moved: Drawn[]) => `${owner} ${moved.map((d) => inSentence(controlLabel(d.knob ?? { label: d.name }))).join(" and ")}`;

/**
 * One drag of a view over the knobs it moves (WRITE-22, CARD-22): each value heard as it moves
 * (`/preview` begins each knob's lease under one gesture, and keeps it alive while held still; one
 * in flight for each knob, the newest sent when it lands), and all of them written at release by
 * one `/set` with `with`: one undo step. A release refused, or a drag let go of, ends every lease.
 */
function useViewDrag(p: ViewProps) {
  const toast = useStore((s) => s.toast);
  type Live = { token: string; knobs: NumberKnob[]; values: number[]; begun: boolean[]; flying: (Promise<void> | null)[]; queued: boolean[]; keep: number };
  const live = useRef<Live | null>(null);
  /** Send knob `i`'s newest value, unless one is on its way: then once that lands. */
  const send = (d: Live, i: number) => {
    if (d.flying[i]) {
      d.queued[i] = true;
      return;
    }
    d.begun[i] = true;
    d.queued[i] = false;
    const k = d.knobs[i];
    d.flying[i] = graphApi
      .preview(p.session, p.revision, k.id, d.token, d.values[i])
      .catch((e) => toast(words(e)))
      .finally(() => {
        d.flying[i] = null;
        if (d.queued[i]) send(d, i);
      });
  };
  /** Every value on its way has landed. */
  const landed = async (d: Live) => {
    while (d.flying.some(Boolean)) await Promise.all(d.flying.filter(Boolean));
  };
  /** Each lease begun ends: the code's values play again. */
  const cancel = async () => {
    const d = live.current;
    live.current = null;
    if (!d) return;
    clearInterval(d.keep);
    d.queued.fill(false);
    await landed(d);
    for (const [i, k] of d.knobs.entries()) if (d.begun[i]) await graphApi.cancel(p.session, k.id, d.token).catch(() => {});
  };
  useEffect(() => () => void cancel(), []);
  return {
    /** A drag of `knobs` begins. */
    begin(knobs: NumberKnob[]) {
      const n = knobs.length;
      const d: Live = { token: crypto.randomUUID(), knobs, values: knobs.map((k) => k.value), begun: Array(n).fill(false), flying: Array(n).fill(null), queued: Array(n).fill(false), keep: 0 };
      // Held still, each lease is kept alive (the server ends one nobody moves for 10 s).
      d.keep = window.setInterval(() => live.current === d && d.knobs.forEach((_, i) => d.begun[i] && send(d, i)), 2000);
      live.current = d;
    },
    /** Its knobs' values now, each in its own unit: heard at once. */
    move(values: number[]) {
      const d = live.current;
      if (!d) return;
      values.forEach((v, i) => {
        if (v === d.values[i] && (d.begun[i] || v === d.knobs[i].value)) return;
        d.values[i] = v;
        send(d, i);
      });
    },
    /** Let go: what it shows is written, as one undo step named `name`; every knob it moved in it. */
    async end(name: string) {
      const d = live.current;
      if (!d) return;
      if (d.knobs.every((k, i) => d.values[i] === k.value)) return cancel();
      live.current = null;
      clearInterval(d.keep);
      await landed(d);
      const all = d.knobs.map((k, i): [string, number] => [k.id, d.values[i]]);
      await graphApi.setWith(p.session, p.revision, d.token, all, { name }).catch((e) => {
        toast(words(e));
        // The server ended them with the refusal; say so to any it may have missed.
        for (const [i, k] of d.knobs.entries()) if (d.begun[i]) void graphApi.cancel(p.session, k.id, d.token).catch(() => {});
      });
    },
    cancel,
  };
}

/** A drag on an SVG: `at` gives the pointer in the drawing's own units (`w` × `h`). */
function svgPoint(svg: SVGSVGElement, e: { clientX: number; clientY: number }, w: number, h: number) {
  const r = svg.getBoundingClientRect();
  return { x: ((e.clientX - r.left) / r.width) * w, y: ((e.clientY - r.top) / r.height) * h };
}

/** A corner a drag can take: a 7 px circle, `accent` while held; a fixed one hollow and grey, which says why. */
function Handle({ x, y, id, held, title, fixed, onDown }: { x: number; y: number; id: string; held: boolean; title: string; fixed: boolean; onDown: (e: React.PointerEvent<SVGCircleElement>) => void }) {
  return (
    <>
      <circle
        cx={x}
        cy={y}
        r={3.5}
        fill={held ? "var(--color-accent)" : "var(--color-raised)"}
        stroke={held ? "var(--color-accent)" : fixed ? "var(--color-fg-off)" : "var(--color-fg)"}
        strokeWidth={1.5}
        pointerEvents="none"
      />
      <circle data-handle={id} data-fixed={fixed || undefined} cx={x} cy={y} r={9} fill="transparent" className={fixed ? "cursor-default" : "cursor-grab"} onPointerDown={fixed ? undefined : onDown}>
        <title>{title}</title>
      </circle>
    </>
  );
}

/** A view that can't be drawn: its frame greyed, and why in words. */
function Cant({ why }: { why: string }) {
  return (
    <Picture title={why} greyed>
      <p className="p-2 text-xs text-fg-muted">{why}</p>
    </Picture>
  );
}

/**
 * A drag of a corner: from where the corner is, the pointer's moves (in the drawing's units) added
 * to it, so grabbing a corner off its centre moves nothing until the pointer does (review V11).
 * `move` gets how far the pointer went, and whether it went across, and up or down.
 */
function cornerDrag(e: React.PointerEvent<SVGCircleElement>, svg: SVGSVGElement, w: number, h: number, move: (dx: number, dy: number, across: boolean, upDown: boolean) => void, end: (ok: boolean) => void) {
  e.stopPropagation();
  e.preventDefault();
  const el = e.currentTarget, pointer = e.pointerId;
  el.setPointerCapture(pointer);
  const p0 = svgPoint(svg, e, w, h);
  const moved = (ev: PointerEvent) => {
    if (ev.pointerId !== pointer) return;
    const p = svgPoint(svg, ev, w, h);
    move(p.x - p0.x, p.y - p0.y, Math.abs(p.x - p0.x) >= 0.5, Math.abs(p.y - p0.y) >= 0.5);
  };
  const up = (ev: PointerEvent) => {
    if (ev.pointerId !== pointer) return;
    el.removeEventListener("pointermove", moved);
    el.removeEventListener("pointerup", up);
    el.removeEventListener("pointercancel", up);
    end(ev.type === "pointerup");
  };
  el.addEventListener("pointermove", moved);
  el.addEventListener("pointerup", up);
  el.addEventListener("pointercancel", up);
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
  const [held, setHeld] = useState<{ corner: string; values: number[]; scale: number; hold: number } | null>(null);
  const shown = drawn(p, [["attack", view.attack], ["decay", view.decay], ["sustain", view.sustain], ["release", view.release]]);
  if ("cant" in shown) return <Cant why={shown.cant} />;
  const parts = shown.parts;
  const units = parts.map((d) => d.unit);
  const values = held?.values ?? parts.map((d) => d.value);
  const H = VIEW_H.envelope - 8;
  const [a, d, r] = [0, 1, 3].map((i) => seconds(values[i], units[i], bpm));
  const s = level(values[2], units[2]);
  // The sustain is held for a quarter of the rest, kept as it was while a corner is held, so the
  // corners stay under the pointer.
  const hold = held?.hold ?? Math.max(0.25 * (a + d + r), 0.05);
  const k = held?.scale ?? (W - 8) / (a + d + hold + r);
  const x = { peak: 4 + a * k, decay: 4 + (a + d) * k, hold: 4 + (a + d + hold) * k, end: 4 + (a + d + hold + r) * k };
  const y = (l: number) => 4 + (1 - l) * (H - 8);
  /** Which parts a corner moves: those the card can move. */
  const movable = (idx: number[]) => idx.filter((i) => parts[i].knob && !parts[i].why);
  const press = (corner: "attack" | "decay" | "release", idx: number[]) => (e: React.PointerEvent<SVGCircleElement>) => {
    const used = movable(idx);
    if (e.button !== 0 || !used.length || !svg.current) return;
    const start = parts.map((d) => d.value);
    const frozen = { scale: k, hold };
    drag.begin(used.map((i) => parts[i].knob!));
    setHeld({ corner, values: start, ...frozen });
    cornerDrag(
      e,
      svg.current,
      W,
      H,
      (dx, dy, across, upDown) => {
        const vals = [...start];
        const time = (i: number, base: number) => inRange(parts[i].knob, roundTime(fromSeconds(base + dx / frozen.scale, units[i], bpm), units[i]));
        if (corner === "attack" && across) vals[0] = time(0, seconds(start[0], units[0], bpm));
        if (corner === "decay" && across && used.includes(1)) vals[1] = time(1, seconds(start[1], units[1], bpm));
        if (corner === "decay" && upDown && used.includes(2)) vals[2] = inRange(parts[2].knob, fromLevel(Math.min(1, Math.max(0, level(start[2], units[2]) - dy / (H - 8))), units[2]));
        if (corner === "release" && across) vals[3] = time(3, seconds(start[3], units[3], bpm));
        setHeld({ corner, values: vals, ...frozen });
        drag.move(used.map((i) => vals[i]));
      },
      (ok) => {
        const moved = used.map((i) => parts[i]);
        void (ok ? drag.end(stepName(p.owner, moved)) : drag.cancel()).finally(() => setHeld(null));
      },
    );
  };
  const said = (i: number) => (units[i] === "beats" ? `${values[i]} beats` : units[i] === "db" ? `${values[i]} dB` : i === 2 ? `${values[i]}` : `${Math.round(values[i] * 1000)} ms`);
  const tip = (idx: number[], words: string) => [words, ...idx.flatMap((i) => (parts[i].why ? [parts[i].why!] : []))].join("\n");
  const shape = `M4,${y(0)} L${x.peak},${y(1)} L${x.decay},${y(s)} L${x.hold},${y(s)} L${x.end},${y(0)}`;
  const fixed = parts.filter((d) => !d.knob);
  return (
    <Picture title={`How loud a note is over time: attack ${said(0)}, decay ${said(1)}, sustain ${said(2)}, release ${said(3)}\nDrag a corner to change them${fixed.map((d) => `\n${d.why}`).join("")}`}>
      <svg ref={svg} viewBox={`0 0 ${W} ${H}`} className="w-full h-full overflow-visible" data-envelope>
        <line x1={0} x2={W} y1={y(0)} y2={y(0)} stroke="var(--color-line)" />
        <path d={`${shape} Z`} fill="var(--color-fg)" fillOpacity={0.06} stroke="none" />
        <path d={shape} fill="none" stroke={held ? "var(--color-accent)" : "var(--color-fg)"} strokeWidth={1.5} strokeLinejoin="round" />
        <Handle id="attack" x={x.peak} y={y(1)} held={held?.corner === "attack"} title={tip([0], `Attack: ${said(0)}`)} fixed={!movable([0]).length} onDown={press("attack", [0])} />
        <Handle id="decay" x={x.decay} y={y(s)} held={held?.corner === "decay"} title={tip([1, 2], `Decay: ${said(1)} · sustain: ${said(2)}`)} fixed={!movable([1, 2]).length} onDown={press("decay", [1, 2])} />
        <Handle id="release" x={x.end} y={y(0)} held={held?.corner === "release"} title={tip([3], `Release: ${said(3)}`)} fixed={!movable([3]).length} onDown={press("release", [3])} />
      </svg>
    </Picture>
  );
}

/** A view's frame: a 1 px `line` box, its words in the tooltip; greyed when it can't be moved or drawn. */
function Picture({ title, children, caption, greyed }: { title: string; children: ReactNode; caption?: string; greyed?: boolean }) {
  return (
    <div className={clsx("relative h-full rounded-sm ring-1 ring-line bg-canvas overflow-hidden", greyed && "opacity-60")} data-greyed={greyed || undefined} title={title}>
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

/** A resonance from 0 to 1 drawn as a q (CARD-30): 0.5 at 0, no bump at all, to 30 at 1, where a synth's filter sings. */
export const resonanceQ = (r: number) => 0.5 * 60 ** Math.min(1, Math.max(0, r));
/** The resonance a q is drawn for, from 0 to 1. */
const qResonance = (q: number) => Math.min(1, Math.max(0, Math.log(q / 0.5) / Math.log(60)));

function FilterView({ view, ...p }: ViewProps & { view: Extract<CardView, { kind: "filter" }> }) {
  const svg = useRef<SVGSVGElement>(null);
  const drag = useViewDrag(p);
  const [held, setHeld] = useState<number[] | null>(null);
  const shown = drawn(p, [["cutoff", view.cutoff], ["q", view.q]]);
  if ("cant" in shown) return <Cant why={shown.cant} />;
  const parts = shown.parts;
  // Its second value a q, or a resonance from 0 to 1, drawn as the q it stands for.
  const resonant = parts[1].unit === "unit01";
  const qName = resonant ? "resonance" : "q";
  const [cutoff, value] = held ?? parts.map((d) => d.value);
  const q = resonant ? resonanceQ(value) : value;
  const H = VIEW_H.filter - 8;
  const xOf = (f: number) => (Math.log(f / F_LO) / Math.log(F_HI / F_LO)) * W;
  const yOf = (db: number) => (1 - (Math.min(DB_HI, Math.max(DB_LO, db)) - DB_LO) / (DB_HI - DB_LO)) * H;
  const curve = Array.from({ length: 97 }, (_, i) => {
    const f = F_LO * (F_HI / F_LO) ** (i / 96);
    return `${i ? "L" : "M"}${xOf(f).toFixed(1)},${yOf(lowpassDb(f, cutoff, q)).toFixed(1)}`;
  }).join(" ");
  const peak = { x: xOf(cutoff), y: yOf(20 * Math.log10(q)) };
  const used = [0, 1].filter((i) => parts[i].knob && !parts[i].why);
  const press = (e: React.PointerEvent<SVGCircleElement>) => {
    if (e.button !== 0 || !used.length || !svg.current) return;
    const start = [cutoff, value];
    const at0 = { x: xOf(cutoff), db: 20 * Math.log10(q) };
    drag.begin(used.map((i) => parts[i].knob!));
    setHeld(start);
    cornerDrag(
      e,
      svg.current,
      W,
      H,
      (dx, dy, across, upDown) => {
        const vals = [...start];
        if (across && used.includes(0)) vals[0] = inRange(parts[0].knob, Math.round(Math.min(F_HI, Math.max(F_LO, F_LO * (F_HI / F_LO) ** ((at0.x + dx) / W)))));
        if (upDown && used.includes(1)) {
          const db = at0.db - (dy / H) * (DB_HI - DB_LO);
          const to = Math.min(40, Math.max(0.1, 10 ** (db / 20)));
          vals[1] = inRange(parts[1].knob, Math.round((resonant ? qResonance(to) : to) * 100) / 100);
        }
        setHeld(vals);
        drag.move(used.map((i) => vals[i]));
      },
      (ok) => {
        const moved = used.map((i) => parts[i]);
        void (ok ? drag.end(stepName(p.owner, moved)) : drag.cancel()).finally(() => setHeld(null));
      },
    );
  };
  const hz = cutoff < 1000 ? `${Math.round(cutoff)} Hz` : `${(cutoff / 1000).toFixed(1)} kHz`;
  const fixed = parts.filter((d) => !d.knob);
  return (
    <Picture
      caption="Cuts the highs · a drawing"
      title={`A drawing of a filter that cuts the highs above the cutoff (${hz}), with a bump at the cutoff that the ${qName} (${value}) makes taller. Worked out from the two knobs: not measured from the sound.\nDrag the bump: across for the cutoff, up for the ${qName}${fixed.map((d) => `\n${d.why}`).join("")}`}
    >
      <svg ref={svg} viewBox={`0 0 ${W} ${H}`} className="w-full h-full overflow-visible" data-filter>
        <line x1={0} x2={W} y1={yOf(0)} y2={yOf(0)} stroke="var(--color-line)" />
        <path d={`${curve} L${W},${H} L0,${H} Z`} fill="var(--color-fg)" fillOpacity={0.06} stroke="none" />
        <path d={curve} fill="none" stroke={held ? "var(--color-accent)" : "var(--color-fg)"} strokeWidth={1.5} />
        <Handle id="peak" x={peak.x} y={peak.y} held={!!held} title={[`Cutoff ${hz} · ${qName} ${value}`, ...parts.flatMap((d) => (d.why ? [d.why] : []))].join("\n")} fixed={!used.length} onDown={press} />
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
  // As the piano roll's keys do, a click sounds only while audition is on (its switch in Notes).
  const hearing = useAudition((s) => s.on);
  if (view.why) return <Picture title={view.why}><p className="p-2 text-xs text-fg-muted">{view.why.charAt(0).toUpperCase() + view.why.slice(1)}.</p></Picture>;
  // The keys its regions cover, two either side, at least two octaves; around middle C with none.
  const covered = regions.length ? regions : [{ keys: [60, 60] }];
  let lo = Math.max(0, Math.min(...covered.map((r) => r.keys[0])) - 2);
  let hi = Math.min(127, Math.max(...covered.map((r) => r.keys[1])) + 2);
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
    held.current = strike(lane.module, lane.lane, pitch, velocity);
    if (held.current !== null) setPlaying(pitch);
  };
  const lift = () => {
    release(held.current);
    held.current = null;
    setPlaying(null);
  };
  const files = [...new Set(regions.map((r) => r.file))];
  return (
    <Picture title={`${files.length} ${files.length === 1 ? "recording" : "recordings"} from ${noteName(lo)} to ${noteName(hi)}${lane ? (hearing ? "\nClick a key to hear it: higher plays it harder" : "\nAudition is off (in Notes' header): turn it on to hear a key you click") : ""}`}>
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
            data-playing={playing === lo + i || undefined}
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
  const count = useRef(0);
  const box = useRef<HTMLDivElement>(null);
  const { session, module } = p;
  const node = p.node.id;
  // Looked at: the card on screen, in a window that shows (TAP-4: looking costs nothing otherwise).
  const [onScreen, setOnScreen] = useState(false);
  const [shown, setShown] = useState(() => document.visibilityState === "visible");
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const io = new IntersectionObserver((seen) => setOnScreen(seen.some((s) => s.isIntersecting)));
    io.observe(el);
    const vis = () => setShown(document.visibilityState === "visible");
    document.addEventListener("visibilitychange", vis);
    return () => {
      io.disconnect();
      document.removeEventListener("visibilitychange", vis);
    };
  }, []);
  const looking = onScreen && shown;
  useEffect(() => {
    if (!looking) return;
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
      count.current += 1;
      box.current?.setAttribute("data-frames", String(count.current));
    });
    return () => {
      alive = false;
      clearInterval(again);
      off();
      void ask(false).catch(() => {});
    };
  }, [session, module, node, looking]);
  return (
    <div ref={box} className="h-full" data-scope={node} data-asking={looking || undefined}>
      <Picture title={why ?? "The sound's wave, as it plays"}>
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
