// A voice's notes (VOX-46, VOX-47): the Notes view of an audio clip. The piano roll's rows, the
// clip's waveform faint behind, each sung note a bar at `pitch + move` as thick as it is loud, the
// pitch line drawn through it as it plays, notes outside the key shaded. Drag a note up or down to
// move it by semitones (by cents with ⌥, onto the key with Lock to key), sideways to move its
// timing, an edge to change its length; right-click it for Straighten. Each edit is worked out
// here and sent as the clip written whole: one undo step.

import { useState } from "react";
import { clipIndex, findLane, type AudioClip, type ClipRef, type GraphState } from "../lib/graph";
import { getLocal, setLocal, useLocal } from "../lib/local";
import { GRAPHITE, trackColors } from "../lib/palette";
import { fit, inKey } from "../lib/songKey";
import { anchors, beatOf, DOWN, findNotes, midiOf, output, retimed, UP, useMeasure, writeClip, type Sung } from "../lib/sung";
import { useZoom } from "../lib/zoom";
import { DockSlot } from "./Dock";
import { Wave } from "./Wave";
import { Button, EmptyState, keepFocus, Popover } from "./ui";

const ROW = 14;
/** Rows kept above the highest note and below the lowest. */
const ROOM = 3;
/** How far a pointer moves before a drag says which way it goes, in px. */
const SLOP = 4;
/** An edge a drag takes the length by, in px. */
const EDGE = 6;

const NAMES = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
const pc = (p: number) => ((Math.round(p) % 12) + 12) % 12;
const nameOf = (p: number) => `${NAMES[pc(p)]}${Math.floor(Math.round(p) / 12) - 1}`;
const cents = (x: number) => Math.round(x * 100) / 100;

/** A drag under way: what it started from, and the clip as it would be written now. */
interface Drag {
  i: number;
  how: "pitch" | "time" | "left" | "right" | null;
  x0: number;
  y0: number;
  notes: Sung[];
  warp?: [number, number][];
  why?: string;
}

export default function SungView({ graph, clip: ref, track }: { graph: GraphState; clip: ClipRef; track: string }) {
  const lane = findLane(graph.timeline, ref.module, ref.lane);
  const ppb = useZoom();
  const roll = useLocal<{ lock?: boolean }>("roll", {});
  const [drag, setDrag] = useState<Drag | null>(null);
  const [menu, setMenu] = useState<{ i: number; anchor: HTMLElement; amount: number } | null>(null);
  const c = lane?.kind === "audio" ? (lane.clips[clipIndex(lane.clips, ref)] as AudioClip | undefined) : undefined;
  const measure = useMeasure(c ?? { asset: "" });
  if (!lane || lane.kind !== "audio" || !c) return null;
  const tempo = graph.timeline.tempo;
  const key = graph.timeline.key ?? null;
  const locking = !!key && !!roll.lock;
  const color = trackColors(graph.timeline, ref.module) ?? GRAPHITE;
  const shown: AudioClip = drag?.warp ? { ...c, warp: drag.warp } : c;
  const notes = drag?.notes ?? c.sung ?? [];
  const find = () => void findNotes(graph, lane, ref, track);
  const tools = (
    <DockSlot at="tools">
      {key && (
        <label className="flex items-center gap-1 text-xs text-fg-muted">
          <input type="checkbox" checked={locking} onChange={(e) => setLocal("roll", { ...getLocal("roll", {}), lock: e.target.checked })} />
          Lock to key
        </label>
      )}
      <Button size="sm" data-testid="find-notes" onMouseDown={keepFocus} onClick={find} title="Find the notes this voice sings. Your edits stay.">
        Find notes
      </Button>
    </DockSlot>
  );
  if (notes.length === 0) {
    return (
      <div data-testid="sung-view" data-empty className="flex-1 min-h-0 grid place-items-center px-6">
        {tools}
        <EmptyState>
          <p>Find the notes this voice sings, to fix their pitch and timing.</p>
          <Button variant="primary" data-testid="find-notes-here" onClick={find}>
            Find notes
          </Button>
        </EmptyState>
      </div>
    );
  }
  const played = notes.map((n) => n.pitch + n.move);
  const hi = Math.ceil(Math.max(...played)) + ROOM;
  const lo = Math.floor(Math.min(...played)) - ROOM;
  const rows = hi - lo + 1;
  const y = (p: number) => (hi - p) * ROW + ROW / 2;
  const x = (s: number) => beatOf(shown, tempo, s) * ppb;
  const keyed = key ? inKey(key) : null;
  const m = measure && measure !== "failed" ? measure : null;
  const hz = m ? output(m, "hz") : [];
  const clarity = m ? output(m, "clarity") : [];
  const level = m ? output(m, "level") : [];
  const loudest = Math.max(1e-6, ...level);
  const loudness = (n: Sung) => {
    if (!m) return 0.5;
    const [a, b] = [Math.floor((n.at * m.rate) / m.hop), Math.ceil(((n.at + n.length) * m.rate) / m.hop)];
    const part = level.slice(a, b);
    return part.length ? Math.min(1, part.reduce((s, v) => s + v, 0) / part.length / loudest) : 0.5;
  };
  // The pitch line through a note as it plays: the line moved by `move`, pulled to its pitch by `straighten`.
  const line = (n: Sung) => {
    if (!m) return "";
    const pts: string[] = [];
    for (let k = Math.floor((n.at * m.rate) / m.hop); k * m.hop < (n.at + n.length) * m.rate; k++) {
      if (!(clarity[k] >= 0.5 && hz[k] > 0)) continue;
      const p = midiOf(hz[k]);
      pts.push(`${x((k * m.hop) / m.rate).toFixed(1)},${y(p + n.move + n.straighten * (n.pitch - p)).toFixed(1)}`);
    }
    return pts.join(" ");
  };

  const step = (what: string) => `${what} a note of ${track}`;
  const begin = (e: React.PointerEvent, i: number) => {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const at = e.clientX - r.left;
    const how = at < EDGE ? "left" : at > r.width - EDGE ? "right" : null;
    setDrag({ i, how, x0: e.clientX, y0: e.clientY, notes: c.sung ?? [] });
  };
  const moving = (e: React.PointerEvent) => {
    if (!drag) return;
    const dx = e.clientX - drag.x0, dy = e.clientY - drag.y0;
    const how = drag.how ?? (Math.abs(dx) < SLOP && Math.abs(dy) < SLOP ? null : Math.abs(dy) > Math.abs(dx) ? "pitch" : "time");
    if (!how) return;
    const was = c.sung ?? [];
    const n = was[drag.i];
    if (how === "pitch") {
      const rows = -dy / ROW;
      let to = e.altKey ? cents(n.pitch + n.move + rows) : n.pitch + n.move + Math.round(rows);
      if (locking && key && !e.altKey) to = fit(key, to);
      const move = cents(to - n.pitch);
      const kept = Math.min(UP, Math.max(DOWN, move));
      const why = kept !== move ? "A note moves at most 7 semitones down and 12 up: further, the voice would sound broken." : undefined;
      setDrag({ ...drag, how, why, warp: undefined, notes: was.map((o, k) => (k === drag.i ? { ...o, move: kept } : o)) });
    } else {
      const [a, z] = [beatOf(c, tempo, n.at), beatOf(c, tempo, n.at + n.length)];
      // Moved whole, a note keeps its length: it stops short of its neighbours (VOX-47).
      const room = anchors(c, tempo, was, drag.i);
      const d = how === "time" ? Math.min(Math.max(dx / ppb, room.lo - a), room.hi - z) : dx / ppb;
      const [from, to] = how === "left" ? [a + d, z] : how === "right" ? [a, z + d] : [a + d, z + d];
      const warp = retimed(c, tempo, was, drag.i, from, to);
      if (warp) setDrag({ ...drag, how, warp, notes: was });
    }
  };
  const end = () => {
    if (!drag) return;
    const d = drag;
    setDrag(null);
    if (d.how === "pitch" && d.notes[d.i].move !== (c.sung ?? [])[d.i]?.move) void writeClip(ref, c, { sung: d.notes }, step("Moving"));
    if (d.how !== "pitch" && d.how && d.warp) void writeClip(ref, c, { warp: d.warp }, step("Retiming"));
  };
  const straighten = (i: number, amount: number) => {
    const sung = (c.sung ?? []).map((o, k) => (k === i ? { ...o, straighten: Math.round(amount) / 100 } : o));
    void writeClip(ref, c, { sung }, step("Straightening"));
  };

  return (
    <div data-testid="sung-view" className="relative flex-1 min-h-0 overflow-auto">
      {tools}
      <div className="relative" style={{ width: c.length * ppb, height: rows * ROW }}>
        {Array.from({ length: rows }, (_, r) => {
          const p = hi - r;
          const out = keyed && !keyed.has(pc(p));
          return <div key={p} className={out ? "absolute inset-x-0 bg-line/40" : "absolute inset-x-0"} style={{ top: r * ROW, height: ROW }} />;
        })}
        <div className="absolute inset-0 opacity-25 pointer-events-none">
          <Wave clip={shown} color={color} bare />
        </div>
        <svg className="absolute inset-0 pointer-events-none" width={c.length * ppb} height={rows * ROW}>
          {notes.map((n, i) => (
            <polyline key={i} points={line(n)} fill="none" stroke={color.ink} strokeWidth={1.5} />
          ))}
        </svg>
        {notes.map((n, i) => {
          const h = 4 + Math.round(8 * loudness(n));
          const left = x(n.at), width = Math.max(4, x(n.at + n.length) - left);
          const p = n.pitch + n.move;
          return (
            <div
              key={i}
              data-testid="sung-note"
              data-note={i}
              data-pitch={p}
              role="button"
              aria-label={`${nameOf(p)}${n.move ? `, moved ${n.move > 0 ? "up" : "down"} ${Math.abs(n.move)}` : ""}${n.straighten ? `, straightened ${Math.round(n.straighten * 100)}%` : ""}`}
              title={`Sung ${nameOf(n.pitch)} (${n.pitch.toFixed(2)}). Drag up or down to move it, sideways for its timing; right-click to straighten it.`}
              className="absolute rounded-sm cursor-grab touch-none"
              style={{ left, width, top: y(p) - h / 2, height: h, background: color.bar, opacity: keyed && !keyed.has(pc(p)) ? 0.6 : 0.9 }}
              onPointerDown={(e) => begin(e, i)}
              onPointerMove={moving}
              onPointerUp={end}
              onPointerCancel={() => setDrag(null)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ i, anchor: e.currentTarget, amount: Math.round(n.straighten * 100) });
              }}
            />
          );
        })}
      </div>
      {drag?.why && (
        <div role="status" className="absolute left-2 bottom-2 text-xs bg-canvas px-2 py-1 rounded-sm text-fg-muted">
          {drag.why}
        </div>
      )}
      {menu && (
        <Popover anchor={menu.anchor} onClose={() => setMenu(null)} className="w-[220px] p-3 space-y-2">
          <label className="block text-sm">
            Straighten: {menu.amount}%
            <input
              type="range"
              min={0}
              max={100}
              value={menu.amount}
              data-testid="straighten"
              className="w-full"
              onChange={(e) => setMenu({ ...menu, amount: Number(e.target.value) })}
              onPointerUp={() => straighten(menu.i, menu.amount)}
              onKeyUp={() => straighten(menu.i, menu.amount)}
            />
          </label>
          <p className="text-xs text-fg-muted">Flattens its wobble and vibrato: 100% sings one straight pitch.</p>
        </Popover>
      )}
    </div>
  );
}
