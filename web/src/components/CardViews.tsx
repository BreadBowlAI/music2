// A card's views (CARD-26 to CARD-29): pictures over values that have homes, drawn in music2's look
// (taste.md §2.8). Each holds no value of its own. An envelope and a filter are pictures std draws
// (CARD-96, CARD-97), drawn by `PictureView`.
//
// - Keys: which recording plays on which keys and how hard; a click plays the key, as hard as high.
// - Scope: the call's sound as it plays, asked of the server only while it shows (TAP-8, TAP-16).
//
// A drag holds a lease on each control it moves (WRITE-22), heard as it goes, and is written at
// release as one edit, one undo step (CARD-22).

import clsx from "clsx";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { attach, release, strike, useAudition } from "../lib/audition";
import { graphApi, type CardView, type KeyRegion, type NumberKnob, type ViewNode } from "../lib/graph";
import { useStore } from "../lib/store";
import { useLook, useLooking } from "../lib/taps";
import { editing, fileOf, gate } from "../lib/history";
import { words } from "./Settings";
import { PICTURE_H, PictureView } from "./PictureView";

/** How tall each view is on its card, in px. */
export const VIEW_H: Record<CardView["kind"], number> = { keys: 72, scope: 64, picture: PICTURE_H };

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

/** One view on its card, as its kind draws it; `height` in place of its kind's, for a picture on a chain card's face. */
export function ViewRow({ view, height, ...p }: ViewProps & { view: CardView; height?: number }) {
  return (
    <div className="relative px-4 py-1" style={{ height: height ?? VIEW_H[view.kind] }} data-view={view.name} data-view-kind={view.kind}>
      {view.kind === "keys" ? (
        <KeysView view={view} {...p} />
      ) : view.kind === "picture" ? (
        <PictureView view={view} {...p} />
      ) : (
        <ScopeView {...p} />
      )}
    </div>
  );
}

/**
 * One drag of a view over the knobs it moves (WRITE-22, CARD-22): each value heard as it moves
 * (`/preview` begins each knob's lease under one gesture, and keeps it alive while held still; one
 * in flight for each knob, the newest sent when it lands), and all of them written at release by
 * one `/set` with `with`: one undo step. A release refused, or a drag let go of, ends every lease.
 * It goes through the gate (`gate`, lib/history): begun while the window doesn't show its own last
 * edit yet, its first value waits until it does, and it goes from there (`revision`) when only the
 * window's own edits that moved no ids came since; else from the revision it was drawn at, which the
 * server carries or refuses; after an edit of its own that moved ids, it isn't sent.
 */
export function useViewDrag(p: ViewProps) {
  const toast = useStore((s) => s.toast);
  type Live = {
    token: string;
    knobs: NumberKnob[];
    values: number[];
    begun: boolean[];
    flying: (Promise<void> | null)[];
    queued: boolean[];
    keep: number;
    revision: number;
    /** Once it is through the gate; false when it gave up there (said in words). */
    ready: Promise<boolean>;
  };
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
    d.flying[i] = d.ready
      .then((ok) => (ok ? graphApi.preview(p.session, d.revision, k.id, d.token, d.values[i]) : undefined))
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
      const d: Live = {
        token: crypto.randomUUID(),
        knobs,
        values: knobs.map((k) => k.value),
        begun: Array(n).fill(false),
        flying: Array(n).fill(null),
        queued: Array(n).fill(false),
        keep: 0,
        revision: p.revision,
        ready: Promise.resolve(true),
      };
      // Through the gate (AGENT-2): from what the window shows once it shows its own last change,
      // when only its own changes that moved no ids came since, each knob as it is called there.
      d.ready = gate(p.session, p.revision, [...new Set(knobs.map((k) => fileOf(k.id)))]).then(
        (g) => ((d.revision = g.revision), (d.knobs = d.knobs.map((k) => ({ ...k, id: g.id(k.id) }))), true),
        (e) => (toast(words(e)), false),
      );
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
      // An edit on its way from the release: a gesture after it, or an Undo, waits for it (AGENT-2).
      const ours = editing();
      try {
        if (!(await d.ready)) return;
        await landed(d);
        const all = d.knobs.map((k, i): [string, number] => [k.id, d.values[i]]);
        await graphApi.setWith(p.session, d.revision, d.token, all, { name }, ours).catch((e) => {
          toast(words(e));
          // The server ended them with the refusal; say so to any it may have missed.
          for (const [i, k] of d.knobs.entries()) if (d.begun[i]) void graphApi.cancel(p.session, k.id, d.token).catch(() => {});
        });
      } finally {
        ours.end();
      }
    },
    cancel,
  };
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

/** Where the pointer is on an SVG, in the drawing's own units (`w` × `h`). */
function svgPoint(svg: SVGSVGElement, e: { clientX: number; clientY: number }, w: number, h: number) {
  const r = svg.getBoundingClientRect();
  return { x: ((e.clientX - r.left) / r.width) * w, y: ((e.clientY - r.top) / r.height) * h };
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

function ScopeView(p: ViewProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [why, setWhy] = useState<string | null>(null);
  const count = useRef(0);
  const box = useRef<HTMLDivElement>(null);
  const { session, module } = p;
  const node = p.node.id;
  // Looked at: the card on screen, in a window that shows (TAP-16; TAP-4: looking costs nothing otherwise).
  const looking = useLooking(box);
  useLook({ session, module, node, kind: "scope", at: "out" }, looking, (seen) => {
    const frames = seen.frames ?? [];
    const c = canvas.current;
    if (!c || !frames.length || !frames[0].length) return;
    setWhy(null);
    draw(c, frames);
    count.current += 1;
    box.current?.setAttribute("data-frames", String(count.current));
  }, setWhy);
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
