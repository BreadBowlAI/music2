import clsx from "clsx";
import { Lock, Magnet, Minus, Plus, X, ZoomIn, ZoomOut } from "lucide-react";
import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import { editorHooks, findNoteClip, graphApi, laneWriting, writeLane, type GraphState, type Note, type NoteClip } from "../lib/graph";
import { kbd, native } from "../lib/native";
import { useStore } from "../lib/store";
import { trackColor, trackHue } from "./SongTimeline";
import { Button } from "./ui";

const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

const KEYS_W = 64;
const RULER_H = 26;
const ROW_H = 16;
const VEL_H = 76;
/** The velocity strip's bars stop this far from its edges. */
const VEL_PAD = 8;
/** The highest pitch, at the top; the lowest is 0. */
const TOP = 127;
const BLACK = [1, 3, 6, 8, 10];
const SNAPS = [
  { label: "1/4", step: 1 },
  { label: "1/8", step: 0.5 },
  { label: "1/16", step: 0.25 },
  { label: "1/32", step: 0.125 },
  { label: "Off", step: 0 },
];
const SNAP_KEY = "m2.notes.snap";
/** What a new note plays at, and what the file leaves unwritten. */
const VELOCITY = 0.8;
/** The shortest note, when nothing snaps. */
const MIN_LEN = 1 / 64;

/** The part of a clip the piano roll edits: its length and its notes. */
interface Shape { length: number; notes: Note[] }
interface Box { x0: number; y0: number; x1: number; y1: number }
/**
 * What a shape of the clip was made from: the lane's clips at a revision (the graph's, or the
 * one this view's last write made), or "pending", the write on its way, whose revision is
 * known once it comes back. A write names it, so the server refuses it over a change it missed.
 */
type Basis = { revision: number; clips: NoteClip[] } | "pending";
type Gesture =
  | { kind: "move" | "resize"; pointer: number; x0: number; y0: number; index: number; base: Shape; from: Basis; sel: ReadonlySet<number>; shift: boolean; picked: boolean; result?: Shape }
  | { kind: "empty"; pointer: number; x0: number; y0: number; keep: ReadonlySet<number>; moved: boolean }
  | { kind: "velocity"; pointer: number; index: number; base: Shape; from: Basis; sel: ReadonlySet<number>; result?: Shape };

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
/** Beats and lengths as written: to a millionth of a beat, never -0. */
const clean = (x: number) => Math.round(x * 1e6) / 1e6 + 0;
const pitchName = (p: number) => `${NOTE_NAMES[((p % 12) + 12) % 12]}${Math.floor(p / 12) - 1}`;
const bars = (beats: number, bpb: number) => Number((beats / bpb).toFixed(3));

/** A length in beats as a note value: 1 → 1/4, 0.75 → 3/16. */
function noteValue(beats: number) {
  const whole = beats / 4;
  for (const q of [1, 2, 4, 8, 16, 32, 64]) {
    const p = whole * q;
    if (Math.abs(p - Math.round(p)) < 1e-9) return `${Math.round(p)}/${q}`;
  }
  return `${Number(beats.toFixed(3))} beats`;
}

/** Notes in the order the file keeps them (start, then pitch), with a selection carried along. */
function tidy(notes: Note[], sel: ReadonlySet<number>): [Note[], Set<number>] {
  const order = notes.map((_, i) => i).sort((a, b) => notes[a].start - notes[b].start || notes[a].pitch - notes[b].pitch);
  const out = new Set<number>();
  order.forEach((i, k) => sel.has(i) && out.add(k));
  return [order.map((i) => notes[i]), out];
}

const same = (a: Shape, b: Shape) => a.length === b.length && JSON.stringify(a.notes) === JSON.stringify(b.notes);

function readSnap() {
  try {
    const saved = localStorage.getItem(SNAP_KEY);
    if (saved !== null && SNAPS.some((s) => s.step === Number(saved))) return Number(saved);
  } catch {}
  return 0.25;
}

/** A note's colour: the track's hue, pale when played softly and deeper the harder it is played. */
function fill(hue: string, velocity: number) {
  const v = clamp(velocity, 0, 1);
  return v < 0.5 ? `color-mix(in oklab, ${hue}, #fff ${Math.round((0.5 - v) * 110)}%)` : `color-mix(in oklab, ${hue}, #1f2126 ${Math.round((v - 0.5) * 70)}%)`;
}

/** The piano roll for one notes clip: clip `index` of `module`'s lane `lane`. Every gesture is one lane write, one undo step. */
export default function NoteView({
  graph,
  module,
  lane,
  index,
  onClose,
}: {
  graph: GraphState;
  module: string;
  lane: string;
  index: number;
  onClose: () => void;
}) {
  const toast = useStore((s) => s.toast);
  const { timeline } = graph;
  const bpb = timeline.beats_per_bar;
  const server = findNoteClip(timeline, { module, lane, index });
  const color = trackColor(timeline, module) ?? "rgba(143,101,116,1)";
  const hue = trackHue(timeline, module) ?? "rgb(143,101,116)";

  // What the roll shows while a gesture is under way or its write hasn't come back: the
  // clip as edited here. The selection is indices into the notes shown.
  const [draft, setDraft] = useState<Shape | null>(null);
  const [selected, setSelected] = useState<ReadonlySet<number>>(() => new Set());
  const [snap, setSnapState] = useState(readSnap);
  const [drawLen, setDrawLen] = useState<number | null>(null);
  const [ppb, setPpb] = useState(48);
  const [visible, setVisible] = useState(0);
  const [marquee, setMarquee] = useState<Box | null>(null);
  const [, setWrites] = useState(0);

  const root = useRef<HTMLDivElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const grid = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture | null>(null);
  const sending = useRef<{ busy: boolean; next: Shape | null }>({ busy: false, next: null });
  /** The lane as this view's last write made it, while the roll shows that write. */
  const written = useRef<{ revision: number; clips: NoteClip[] } | null>(null);
  const latest = useRef(graph);
  latest.current = graph;
  // Notes are edited through the code's lanes: while the code doesn't compile they are read only.
  const locked = graph.view.draft_errors.length > 0;
  const seen = useRef(server?.notes.length);
  /** The beat to keep under the pointer while zooming. */
  const anchor = useRef<{ beat: number; x: number } | null>(null);

  const clip: Shape | null = draft ?? (server ? { length: server.length, notes: server.notes } : null);
  const start = server?.start ?? 0;
  const step = snap || 0.25;

  // Once this view's writes have come back in the graph, show the clip as the graph has it.
  // A change made elsewhere (undo, the code editor) that adds or removes notes ends the selection.
  useEffect(() => {
    const n = server?.notes.length;
    if (draft) {
      if (gesture.current || sending.current.busy || laneWriting(graph)) return;
      setDraft(null);
      written.current = null;
      if (n !== draft.notes.length) setSelected(new Set());
    } else if (n !== seen.current) setSelected(new Set());
    seen.current = n;
  });

  // Open fitted to the clip's length, centred on its notes.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el || !server) return;
    setPpb(clamp(Math.floor((el.clientWidth - KEYS_W - 32) / server.length), 12, 160));
    const pitches = server.notes.map((n) => n.pitch);
    const mid = pitches.length ? (Math.min(...pitches) + Math.max(...pitches)) / 2 : 60;
    el.scrollTop = RULER_H + (TOP - mid) * ROW_H + ROW_H / 2 - (RULER_H + el.clientHeight) / 2;
    root.current?.focus({ preventScroll: true });
    // Only on opening: later edits keep the view where the user put it.
  }, []);

  useEffect(() => {
    const el = scroller.current!;
    const observer = new ResizeObserver(() => setVisible(el.clientWidth - KEYS_W));
    observer.observe(el);
    // ⌘/Ctrl + wheel zooms time around the pointer; plain wheels scroll.
    const wheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      const x = e.clientX - el.getBoundingClientRect().left - KEYS_W;
      setPpb((p) => {
        anchor.current = { beat: (el.scrollLeft + x) / p, x };
        return clamp(p * Math.exp(-e.deltaY * 0.004), 8, 400);
      });
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => {
      observer.disconnect();
      el.removeEventListener("wheel", wheel);
    };
  }, []);
  useLayoutEffect(() => {
    const el = scroller.current, a = anchor.current;
    anchor.current = null;
    if (el && a) el.scrollLeft = Math.max(0, a.beat * ppb - a.x);
  }, [ppb]);
  const zoom = (by: number) => {
    const el = scroller.current;
    if (el) anchor.current = { beat: (el.scrollLeft + el.clientWidth / 2 - KEYS_W) / ppb, x: el.clientWidth / 2 - KEYS_W };
    setPpb((p) => clamp(p * by, 8, 400));
  };

  const setSnap = (s: number) => {
    setSnapState(s);
    try {
      localStorage.setItem(SNAP_KEY, String(s));
    } catch {}
  };

  // ---------------------------------------------------------------- writing

  /** What the clip as shown now was made from. */
  const basis = (): Basis => {
    if (sending.current.busy) return "pending";
    if (written.current) return written.current;
    const g = latest.current;
    const l = g.timeline.tracks.find((t) => t.module === module)?.lanes.find((x) => x.id === lane);
    return { revision: g.view.revision, clips: l?.kind === "notes" ? l.clips : [] };
  };

  /** Show the clip as the graph has it: an edit was refused, or what it was made from was. */
  const drop = () => {
    written.current = null;
    setDraft(null);
    setSelected(new Set());
  };

  /**
   * Send a shape of this clip, made from `from`, as one lane write. While one is on its way, the
   * newest shape waits for it and goes next, made from what it wrote; if it is refused, so are
   * the shapes made on top of it.
   */
  const send = (shape: Shape, from: Basis) => {
    const s = sending.current;
    if (s.busy) {
      s.next = shape;
      return;
    }
    const b = from === "pending" ? written.current : from;
    if (!b?.clips[index]) return drop();
    s.busy = true;
    const clips = b.clips.map((c, i) => (i === index ? { start: c.start, length: shape.length, notes: shape.notes } : c));
    writeLane(latest.current, module, lane, clips, b.revision)
      .then((r) => void (written.current = { revision: r.revision, clips }))
      .catch((error) => {
        toast(error instanceof Error ? error.message : String(error));
        s.next = null;
        drop();
      })
      .finally(() => {
        s.busy = false;
        const next = s.next;
        s.next = null;
        if (next) send(next, "pending");
        else setWrites((n) => n + 1);
      });
  };

  /** Show and write a changed shape of the clip, made from `from`, selecting `sel` (indices into `shape.notes`). */
  const commit = (shape: Shape, sel: ReadonlySet<number>, from: Basis = basis()) => {
    if (locked) return;
    const [notes, s] = tidy(shape.notes, sel);
    const next = { length: clean(shape.length), notes };
    setDraft(next);
    setSelected(s);
    send(next, from);
  };

  /** Beat `b` of the clip on the snap grid, which follows the song's bars. */
  const snapBeat = (b: number, floor = false) => {
    if (!snap) return clean(b);
    const q = (floor ? Math.floor : Math.round)((start + b) / snap + 1e-9) * snap;
    return clean(q - start);
  };

  const at = (e: { clientX: number; clientY: number }) => {
    const r = grid.current!.getBoundingClientRect();
    const x = e.clientX - r.left, y = e.clientY - r.top;
    return { x, y, beat: x / ppb, pitch: clamp(TOP - Math.floor(y / ROW_H), 0, TOP) };
  };

  const picked = clip ? [...selected].filter((i) => i < clip.notes.length) : [];

  const remove = () => {
    if (!clip || !picked.length) return;
    commit({ length: clip.length, notes: clip.notes.filter((_, i) => !selected.has(i)) }, new Set());
  };

  /** Copies of the selected notes right after them; the copies are selected. */
  const duplicate = () => {
    if (!clip || !picked.length) return false;
    const notes = picked.map((i) => clip.notes[i]);
    const from = Math.min(...notes.map((n) => n.start));
    const span = Math.max(...notes.map((n) => n.start + n.length)) - from;
    const by = snap ? Math.max(1, Math.ceil(span / snap - 1e-9)) * snap : span;
    const copies = notes.map((n) => ({ ...n, start: clean(n.start + by) }));
    commit({ length: clip.length, notes: [...clip.notes, ...copies] }, new Set(copies.map((_, k) => clip.notes.length + k)));
    return true;
  };

  /** Move the selected notes by `dp` semitones and `dt` beats, staying on the keyboard and in the clip. */
  const nudge = (dp: number, dt: number) => {
    if (!clip || !picked.length) return;
    const notes = picked.map((i) => clip.notes[i]);
    dp = clamp(dp, -Math.min(...notes.map((n) => n.pitch)), TOP - Math.max(...notes.map((n) => n.pitch)));
    dt = Math.max(dt, -Math.min(...notes.map((n) => n.start)));
    if (!dp && !dt) return;
    commit({ length: clip.length, notes: clip.notes.map((n, i) => (selected.has(i) ? { ...n, pitch: n.pitch + dp, start: clean(n.start + dt) } : n)) }, selected);
  };

  const setLength = (beats: number) => {
    if (!clip || !(beats > 0) || beats === clip.length) return;
    commit({ length: beats, notes: clip.notes }, selected);
  };

  // In the desktop app ⌘D comes from the menu bar, through App's "duplicate" command.
  const duplicateRef = useRef(duplicate);
  duplicateRef.current = duplicate;
  useEffect(() => {
    const hook = () => !!root.current?.contains(document.activeElement) && duplicateRef.current();
    editorHooks.duplicate = hook;
    return () => {
      if (editorHooks.duplicate === hook) delete editorHooks.duplicate;
    };
  }, []);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!clip || e.target instanceof HTMLInputElement) return;
    const cmd = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();
    const arrows: Record<string, [number, number]> = {
      ArrowUp: [e.shiftKey ? 12 : 1, 0],
      ArrowDown: [e.shiftKey ? -12 : -1, 0],
      ArrowLeft: [0, -(e.shiftKey ? bpb : step)],
      ArrowRight: [0, e.shiftKey ? bpb : step],
    };
    let handled = true;
    if ((e.key === "Delete" || e.key === "Backspace") && picked.length) remove();
    else if (cmd && key === "d" && !native) duplicate();
    else if (cmd && key === "a") setSelected(new Set(clip.notes.map((_, i) => i)));
    else if (e.key === "Escape" && picked.length) setSelected(new Set());
    else if (arrows[e.key] && picked.length && !cmd) nudge(...arrows[e.key]);
    else handled = false;
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  // ---------------------------------------------------------------- the grid's gestures

  const onGridDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || !clip || gesture.current) return;
    root.current?.focus({ preventScroll: true });
    e.currentTarget.setPointerCapture(e.pointerId);
    const p = at(e);
    const target = (e.target as HTMLElement).closest<HTMLElement>("[data-note]");
    if (!target) {
      gesture.current = { kind: "empty", pointer: e.pointerId, x0: p.x, y0: p.y, keep: e.shiftKey ? selected : new Set(), moved: false };
      return;
    }
    const i = Number(target.dataset.note);
    const was = selected.has(i);
    const sel = e.shiftKey ? (was ? selected : new Set([...selected, i])) : was ? selected : new Set([i]);
    setSelected(sel);
    setDrawLen(clip.notes[i].length);
    // Read only: a click selects, nothing moves.
    if (locked) return;
    const kind = (e.target as HTMLElement).closest("[data-edge]") ? "resize" : "move";
    gesture.current = { kind, pointer: e.pointerId, x0: p.x, y0: p.y, index: i, base: clip, from: basis(), sel, shift: e.shiftKey, picked: was };
  };

  const onGridMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const g = gesture.current;
    if (!g || g.pointer !== e.pointerId || g.kind === "velocity" || !clip) return;
    const p = at(e);
    if (g.kind === "empty") {
      if (!g.moved && Math.hypot(p.x - g.x0, p.y - g.y0) < 4) return;
      g.moved = true;
      const box = { x0: Math.min(g.x0, p.x), y0: Math.min(g.y0, p.y), x1: Math.max(g.x0, p.x), y1: Math.max(g.y0, p.y) };
      setMarquee(box);
      const hit = new Set(g.keep);
      clip.notes.forEach((n, i) => {
        const x = n.start * ppb, y = (TOP - n.pitch) * ROW_H;
        if (x + Math.max(n.length * ppb, 4) > box.x0 && x < box.x1 && y + ROW_H > box.y0 && y < box.y1) hit.add(i);
      });
      setSelected(hit);
      return;
    }
    if (!g.result && Math.hypot(p.x - g.x0, p.y - g.y0) < 3) return;
    const base = g.base.notes, n0 = base[g.index];
    const moving = base.filter((_, i) => g.sel.has(i));
    const raw = (p.x - g.x0) / ppb;
    let notes: Note[];
    if (g.kind === "move") {
      const dt = Math.max(snapBeat(n0.start + raw) - n0.start, -Math.min(...moving.map((n) => n.start)));
      const dp = clamp(Math.round((g.y0 - p.y) / ROW_H), -Math.min(...moving.map((n) => n.pitch)), TOP - Math.max(...moving.map((n) => n.pitch)));
      notes = base.map((n, i) => (g.sel.has(i) ? { ...n, start: clean(n.start + dt), pitch: n.pitch + dp } : n));
    } else {
      const min = snap || MIN_LEN;
      const dl = Math.max(snapBeat(n0.start + n0.length + raw) - n0.start, min) - n0.length;
      notes = base.map((n, i) => (g.sel.has(i) ? { ...n, length: clean(Math.max(n.length + dl, min)) } : n));
    }
    g.result = { length: g.base.length, notes };
    setDraft(g.result);
  };

  const onGridUp = (e: React.PointerEvent<HTMLDivElement>) => {
    const g = gesture.current;
    if (!g || g.pointer !== e.pointerId || g.kind === "velocity" || !clip) return;
    gesture.current = null;
    setMarquee(null);
    if (g.kind === "empty") {
      if (!g.moved && !locked) addNote(at(e));
      return;
    }
    if (!g.result) {
      // A click: shift-clicking a selected note lets it go; a plain click selects just this one.
      if (g.shift && g.picked) setSelected(new Set([...selected].filter((i) => i !== g.index)));
      else if (!g.shift) setSelected(new Set([g.index]));
      return;
    }
    if (g.kind === "resize") setDrawLen(g.result.notes[g.index].length);
    if (same(g.result, g.base)) return setDraft(g.base);
    commit(g.result, g.sel, g.from);
  };

  /** A pointer lost mid-gesture puts the clip back as it was. */
  const onGridCancel = (e: React.PointerEvent<HTMLDivElement>) => {
    const g = gesture.current;
    if (!g || g.pointer !== e.pointerId || g.kind === "velocity") return;
    gesture.current = null;
    setMarquee(null);
    if (g.kind !== "empty" && g.result) setDraft(g.base);
  };

  /** A click on empty space: a note of the current length in that cell, if it is inside the clip. */
  const addNote = (p: { beat: number; pitch: number }) => {
    if (!clip) return;
    const from = Math.max(0, snapBeat(p.beat, true));
    if (from >= clip.length) return;
    const length = clean(Math.min(drawLen ?? step, clip.length - from));
    const note = { pitch: p.pitch, start: from, length, velocity: VELOCITY };
    commit({ length: clip.length, notes: [...clip.notes, note] }, new Set([clip.notes.length]));
  };

  // ---------------------------------------------------------------- the velocity strip

  const velocityAt = (clientY: number, el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    return Math.round(clamp(1 - (clientY - r.top - VEL_PAD) / (r.height - 2 * VEL_PAD), 0.01, 1) * 100) / 100;
  };

  const velocityTo = (g: Extract<Gesture, { kind: "velocity" }>, v: number) => {
    const dv = v - g.base.notes[g.index].velocity;
    const notes = g.base.notes.map((n, i) =>
      i === g.index ? { ...n, velocity: v } : g.sel.has(i) ? { ...n, velocity: Math.round(clamp(n.velocity + dv, 0.01, 1) * 100) / 100 } : n,
    );
    g.result = { length: g.base.length, notes };
    setDraft(g.result);
  };

  const onStripDown = (e: React.PointerEvent<HTMLDivElement>) => {
    const bar = (e.target as HTMLElement).closest<HTMLElement>("[data-velocity]");
    if (e.button !== 0 || !bar || !clip || gesture.current || locked) return;
    root.current?.focus({ preventScroll: true });
    e.currentTarget.setPointerCapture(e.pointerId);
    const i = Number(bar.dataset.velocity);
    const sel = selected.has(i) ? selected : new Set([i]);
    setSelected(sel);
    const g: Gesture = { kind: "velocity", pointer: e.pointerId, index: i, base: clip, from: basis(), sel };
    gesture.current = g;
    velocityTo(g, velocityAt(e.clientY, e.currentTarget));
  };

  const onStripMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const g = gesture.current;
    if (g?.kind === "velocity" && g.pointer === e.pointerId) velocityTo(g, velocityAt(e.clientY, e.currentTarget));
  };

  const onStripUp = (e: React.PointerEvent<HTMLDivElement>, cancelled = false) => {
    const g = gesture.current;
    if (g?.kind !== "velocity" || g.pointer !== e.pointerId) return;
    gesture.current = null;
    if (cancelled || !g.result || same(g.result, g.base)) return setDraft(g.base);
    commit(g.result, g.sel, g.from);
  };

  if (!clip || !server) {
    return <div data-testid="notes" className="flex-1 min-h-0 grid place-items-center text-sm text-ink-400">This clip is gone.</div>;
  }

  const end = Math.max(clip.length, ...clip.notes.map((n) => n.start + n.length));
  const width = Math.max(visible, Math.ceil((end + bpb) / bpb) * bpb * ppb);
  const height = (TOP + 1) * ROW_H;
  const shown = new Set(picked);
  const title = module === "song" ? "Song" : module;
  const firstBar = Math.floor(start / bpb) + 1;
  const lastBar = Math.ceil((start + clip.length) / bpb);

  return (
    <div
      ref={root}
      data-testid="notes"
      tabIndex={0}
      onKeyDown={onKeyDown}
      className="flex-1 min-h-0 flex flex-col bg-ink-950 outline-none"
      aria-label={`Notes of ${title}'s ${lane} clip ${index + 1}`}
    >
      <header className="h-11 shrink-0 flex items-center gap-3 px-3 bg-ink-850 border-b border-black/[0.06] overflow-x-auto overflow-y-hidden">
        <div className="shrink-0 flex items-center gap-2 min-w-0">
          <span className="size-3 rounded-[4px] shadow-sm ring-1 ring-black/10" style={{ background: color }} />
          <span className="text-[13px] font-semibold text-ink-100">{title}</span>
          <span className="text-ink-500">/</span>
          <span className="font-mono text-[12px] text-ink-200">{lane}</span>
          <span className="text-[12px] text-ink-400">
            · clip {index + 1} · {lastBar > firstBar ? `bars ${firstBar}–${lastBar}` : `bar ${firstBar}`}
          </span>
        </div>
        <Divider />
        <LengthField beats={clip.length} bpb={bpb} disabled={locked} onChange={setLength} />
        <Divider />
        <div className="shrink-0 flex items-center gap-1.5" title="Notes you add or drag land on this grid">
          <Magnet className="size-3.5 text-ink-400" />
          <div role="radiogroup" aria-label="Snap" className="flex items-center gap-0.5 p-0.5 rounded-lg bg-ink-950 ring-1 ring-black/[0.04]">
            {SNAPS.map((s) => (
              <button
                key={s.label}
                role="radio"
                aria-checked={snap === s.step}
                data-snap={s.label}
                onClick={() => setSnap(s.step)}
                className={clsx(
                  "h-6 px-2 rounded-md text-[11px] font-medium tabular-nums cursor-pointer transition-colors",
                  snap === s.step ? "bg-ink-800 text-ink-100 shadow-sm ring-1 ring-black/[0.06]" : "text-ink-400 hover:text-ink-100",
                )}
              >
                {s.label}
              </button>
            ))}
          </div>
        </div>
        <span className="shrink-0 text-[11px] text-ink-400 tabular-nums" title="The length of a note you add by clicking: the last one you clicked or resized" data-testid="draw-length">
          New notes <span className="font-medium text-ink-200">{noteValue(drawLen ?? step)}</span>
        </span>
        <div className="flex-1" />
        {locked && (
          <span
            data-testid="notes-locked"
            className="shrink-0 inline-flex items-center gap-1.5 h-6 px-2 rounded-md bg-amber-500/10 ring-1 ring-amber-600/20 text-[11px] font-medium text-amber-900"
            title="Notes live in the code's lanes: they can be edited again once the code compiles"
          >
            <Lock className="size-3" />
            Read only until the code compiles
          </span>
        )}
        <span className="shrink-0 text-[11px] text-ink-400 tabular-nums">
          {picked.length ? `${picked.length} of ${clip.notes.length} selected` : `${clip.notes.length} ${clip.notes.length === 1 ? "note" : "notes"}`}
        </span>
        <div className="shrink-0 flex items-center">
          <Button size="sm" onClick={() => zoom(0.8)} title="Zoom out (⌘ + scroll)" aria-label="Zoom out" className="!px-1.5">
            <ZoomOut className="size-3.5" />
          </Button>
          <Button size="sm" onClick={() => zoom(1.25)} title="Zoom in (⌘ + scroll)" aria-label="Zoom in" className="!px-1.5">
            <ZoomIn className="size-3.5" />
          </Button>
        </div>
        <Button size="sm" onClick={onClose} title="Close the clip" aria-label="Close the clip" className="!px-1.5">
          <X className="size-4" />
        </Button>
      </header>

      <div className="flex-1 min-h-0 relative">
        <div
          ref={scroller}
          data-testid="roll-scroller"
          className="absolute inset-0 overflow-auto"
          onScroll={(e) => strip.current && (strip.current.style.transform = `translateX(${-e.currentTarget.scrollLeft}px)`)}
        >
          <div className="relative" style={{ width: KEYS_W + width, height: RULER_H + height }}>
            <div className="sticky top-0 z-30 flex" style={{ height: RULER_H }}>
              <div className="sticky left-0 z-40 shrink-0 bg-ink-900 border-r border-b border-black/[0.06]" style={{ width: KEYS_W }} />
              <Ruler width={width} ppb={ppb} bpb={bpb} start={start} length={clip.length} color={color} />
            </div>
            <div className="flex">
              <Keys selected={new Set(picked.map((i) => clip.notes[i].pitch))} />
              <div
                ref={grid}
                data-testid="roll-grid"
                className={clsx("relative touch-none", locked ? "cursor-default" : "cursor-cell")}
                style={{ width, height }}
                title={
                  locked
                    ? "Read only until the code compiles"
                    : `Click to add a note · drag a note to move it · drag its right edge to resize · drag across empty space to select · ⌫ deletes · ${kbd("D")} duplicates · arrows move`
                }
                onPointerDown={onGridDown}
                onPointerMove={onGridMove}
                onPointerUp={onGridUp}
                onPointerCancel={onGridCancel}
                onLostPointerCapture={onGridCancel}
              >
                <Lanes width={width} ppb={ppb} bpb={bpb} start={start} step={snap} />
                <div
                  className="absolute top-0 bottom-0 right-0 pointer-events-none border-l-2"
                  style={{
                    left: clip.length * ppb,
                    borderColor: color,
                    background: "repeating-linear-gradient(135deg, rgba(0,0,0,0.035) 0 6px, transparent 6px 12px), rgba(225,227,230,0.55)",
                  }}
                />
                {clip.notes.map((n, i) => {
                  const w = Math.max(n.length * ppb - 1, 4);
                  const outside = n.start >= clip.length;
                  return (
                    <div
                      key={i}
                      data-note={i}
                      data-pitch={n.pitch}
                      data-start={n.start}
                      data-length={n.length}
                      data-velocity={n.velocity}
                      data-selected={shown.has(i) || undefined}
                      title={`${pitchName(n.pitch)} · beat ${Number((n.start + 1).toFixed(3))} · ${noteValue(n.length)} · velocity ${Math.round(n.velocity * 100)}%`}
                      className={clsx("absolute rounded-[4px] cursor-grab active:cursor-grabbing", shown.has(i) ? "z-10" : "", outside && "opacity-40")}
                      style={{
                        left: n.start * ppb,
                        top: (TOP - n.pitch) * ROW_H + 1,
                        width: w,
                        height: ROW_H - 2,
                        background: fill(hue, n.velocity),
                        color: `color-mix(in oklab, ${hue}, #000 72%)`,
                        boxShadow: shown.has(i)
                          ? "inset 0 0 0 1px rgba(255,255,255,0.9), 0 0 0 1.5px #2b2d32, 0 2px 6px rgba(0,0,0,0.18)"
                          : `inset 0 0 0 1px color-mix(in oklab, ${hue}, #000 40%)`,
                      }}
                    >
                      {w > 26 && (
                        <span className="absolute left-1.5 top-1/2 -translate-y-1/2 text-[9px] font-semibold leading-none pointer-events-none">
                          {pitchName(n.pitch)}
                        </span>
                      )}
                      <div data-edge className="absolute -right-0.5 inset-y-0 w-[7px] cursor-ew-resize rounded-r-[4px] hover:bg-white/30" />
                    </div>
                  );
                })}
                {marquee && (
                  <div
                    className="absolute z-20 pointer-events-none rounded-sm border border-pop-500/70 bg-pop-500/10"
                    style={{ left: marquee.x0, top: marquee.y0, width: marquee.x1 - marquee.x0, height: marquee.y1 - marquee.y0 }}
                  />
                )}
                <PlayLine start={start} ppb={ppb} width={width} />
              </div>
            </div>
          </div>
        </div>
        {clip.notes.length === 0 && (
          <div
            className="absolute top-1/2 -translate-x-1/2 -translate-y-1/2 pointer-events-none rounded-full bg-ink-800/90 px-4 py-2 text-[13px] text-ink-300 shadow-lg ring-1 ring-black/[0.06]"
            style={{ left: KEYS_W + Math.min(clip.length * ppb, visible) / 2 }}
          >
            Click in the grid to add a note
          </div>
        )}
      </div>

      <div data-testid="velocity" className="shrink-0 flex border-t border-black/[0.08] bg-ink-900" style={{ height: VEL_H }}>
        <div className="shrink-0 flex flex-col justify-center pl-2.5 border-r border-black/[0.06] text-[10px] font-medium text-ink-400 leading-tight" style={{ width: KEYS_W }}>
          Velocity
        </div>
        <div className="relative flex-1 min-w-0 overflow-hidden">
          <div
            ref={strip}
            className="absolute inset-y-0 left-0 touch-none"
            style={{ width, transform: `translateX(${-(scroller.current?.scrollLeft ?? 0)}px)` }}
            onPointerDown={onStripDown}
            onPointerMove={onStripMove}
            onPointerUp={(e) => onStripUp(e)}
            onPointerCancel={(e) => onStripUp(e, true)}
            onLostPointerCapture={(e) => onStripUp(e, true)}
          >
            <div className="absolute inset-x-0 border-t border-dashed border-black/[0.08] pointer-events-none" style={{ top: VEL_PAD }} />
            <div className="absolute inset-x-0 border-t border-dashed border-black/[0.06] pointer-events-none" style={{ top: VEL_H / 2 }} />
            <div className="absolute top-0 bottom-0 right-0 pointer-events-none bg-ink-950/60" style={{ left: clip.length * ppb }} />
            {/* Selected notes' bars on top, so a chord's can be picked out. */}
            {clip.notes
              .map((n, i) => ({ n, i }))
              .sort((a, b) => Number(shown.has(a.i)) - Number(shown.has(b.i)))
              .map(({ n, i }) => {
                const h = (VEL_H - 2 * VEL_PAD) * clamp(n.velocity, 0, 1);
                return (
                  <div
                    key={i}
                    data-velocity={i}
                    title={`${pitchName(n.pitch)}: velocity ${Math.round(n.velocity * 100)}% (drag up or down)`}
                    className="absolute inset-y-0 w-3 -ml-[5px] cursor-ns-resize group"
                    style={{ left: n.start * ppb }}
                  >
                    <div
                      className="absolute left-[5px] w-[2px] rounded-full"
                      style={{ bottom: VEL_PAD, height: h, background: shown.has(i) ? "#2b2d32" : `color-mix(in oklab, ${hue}, #000 30%)` }}
                    />
                    <div
                      className={clsx("absolute left-[2px] size-2 rounded-full ring-1", shown.has(i) ? "ring-ink-100 bg-white" : "ring-black/20 group-hover:scale-125")}
                      style={{ bottom: VEL_PAD + h - 4, background: shown.has(i) ? undefined : fill(hue, n.velocity) }}
                    />
                  </div>
                );
              })}
          </div>
        </div>
      </div>
    </div>
  );
}

const Divider = () => <div className="w-px h-5 shrink-0 bg-black/[0.08]" />;

/** The clip's length in bars, typed or stepped a bar at a time. */
function LengthField({ beats, bpb, disabled, onChange }: { beats: number; bpb: number; disabled: boolean; onChange: (beats: number) => void }) {
  const [text, setText] = useState<string | null>(null);
  const value = bars(beats, bpb);
  const done = () => {
    const n = Number(text);
    setText(null);
    if (text !== null && n > 0 && Number.isFinite(n)) onChange(n * bpb);
  };
  return (
    <div className="shrink-0 flex items-center gap-1.5" title="How long the clip is. Notes past its end don't play.">
      <span className="text-[11px] text-ink-400">Length</span>
      <div className="flex items-center rounded-lg bg-ink-950 ring-1 ring-black/[0.04]">
        <button aria-label="One bar shorter" disabled={disabled || value <= 1} onClick={() => onChange((Math.ceil(value - 1 - 1e-9) || 1) * bpb)} className="h-6 w-6 grid place-items-center text-ink-400 enabled:hover:text-ink-100 disabled:opacity-40 cursor-pointer">
          <Minus className="size-3" />
        </button>
        <input
          aria-label="Clip length in bars"
          disabled={disabled}
          value={text ?? String(value)}
          inputMode="decimal"
          onChange={(e) => setText(e.target.value)}
          onFocus={(e) => e.currentTarget.select()}
          onBlur={done}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
            else if (e.key === "Escape") {
              setText(null);
              e.currentTarget.blur();
            }
          }}
          className="w-9 h-6 bg-transparent text-center text-[12px] font-medium tabular-nums text-ink-100 outline-none"
        />
        <button aria-label="One bar longer" disabled={disabled} onClick={() => onChange((Math.floor(value + 1 + 1e-9)) * bpb)} className="h-6 w-6 grid place-items-center text-ink-400 enabled:hover:text-ink-100 disabled:opacity-40 cursor-pointer">
          <Plus className="size-3" />
        </button>
      </div>
      <span className="text-[11px] text-ink-400">{value === 1 ? "bar" : "bars"}</span>
    </div>
  );
}

/** The piano keys, one row per pitch, C's named; keys of selected notes lit. */
const Keys = memo(
  function Keys({ selected }: { selected: Set<number> }) {
    return (
      <div className="sticky left-0 z-20 shrink-0 bg-white border-r border-black/[0.08]" style={{ width: KEYS_W }}>
        {Array.from({ length: TOP + 1 }, (_, k) => {
          const p = TOP - k;
          const black = BLACK.includes(p % 12);
          const lit = selected.has(p);
          return (
            <div
              key={p}
              data-key={p}
              className={clsx("relative", p % 12 === 0 ? "border-b border-black/[0.14]" : !black && !BLACK.includes((p + 11) % 12) && "border-b border-black/[0.07]")}
              style={{ height: ROW_H }}
            >
              {lit && <div className="absolute inset-0 bg-pop-500/25" />}
              {black && <div className={clsx("absolute left-0 inset-y-[1px] w-[58%] rounded-r-[3px] shadow-sm", lit ? "bg-pop-600" : "bg-ink-200")} />}
              {p % 12 === 0 && (
                <span className={clsx("absolute right-1.5 top-1/2 -translate-y-1/2 text-[10px] leading-none tabular-nums", p === 60 ? "font-semibold text-ink-200" : "text-ink-400")}>
                  {pitchName(p)}
                </span>
              )}
            </div>
          );
        })}
      </div>
    );
  },
  (a, b) => a.selected.size === b.selected.size && [...a.selected].every((p) => b.selected.has(p)),
);

/** Row shading (black keys darker, a line under each C) and the bar, beat and snap lines. */
const Lanes = memo(function Lanes({ width, ppb, bpb, start, step }: { width: number; ppb: number; bpb: number; start: number; step: number }) {
  // From the top: pitch 127 (G9) down, twelve rows a period.
  const stops = Array.from({ length: 12 }, (_, k) => {
    const pc = (TOP - k) % 12;
    const c = BLACK.includes(pc) ? "rgba(0,0,0,0.045)" : "rgba(255,255,255,0.55)";
    return `${c} ${k * ROW_H}px ${(k + 1) * ROW_H}px`;
  }).join(", ");
  const lines = Array.from({ length: 12 }, (_, k) => {
    const pc = (TOP - k) % 12;
    const c = pc === 0 ? "rgba(0,0,0,0.11)" : "rgba(0,0,0,0.035)";
    return `transparent ${k * ROW_H}px ${(k + 1) * ROW_H - 1}px, ${c} ${(k + 1) * ROW_H - 1}px ${(k + 1) * ROW_H}px`;
  }).join(", ");
  const offset = (period: number) => ((((-start % period) + period) % period) * ppb).toFixed(2);
  const v = (c: string) => `linear-gradient(to right, ${c} 1px, transparent 1px)`;
  const layers = [
    { image: v("rgba(0,0,0,0.17)"), size: bpb * ppb, x: offset(bpb) },
    { image: v("rgba(0,0,0,0.08)"), size: ppb, x: offset(1) },
    ...(step && step < 1 && step * ppb >= 6 ? [{ image: v("rgba(0,0,0,0.035)"), size: step * ppb, x: offset(step) }] : []),
  ];
  return (
    <>
      <div className="absolute inset-0 pointer-events-none" style={{ backgroundImage: `linear-gradient(to bottom, ${lines}), linear-gradient(to bottom, ${stops})`, backgroundSize: `100% ${12 * ROW_H}px`, backgroundPosition: "0 0" }} />
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          width,
          backgroundImage: layers.map((l) => l.image).join(", "),
          backgroundSize: layers.map((l) => `${l.size}px 100%`).join(", "),
          backgroundPosition: layers.map((l) => `${l.x}px 0`).join(", "),
        }}
      />
    </>
  );
});

/** Bar numbers of the song, the clip's extent in its track's colour, and a click to play from there. */
const Ruler = memo(function Ruler({ width, ppb, bpb, start, length, color }: { width: number; ppb: number; bpb: number; start: number; length: number; color: string }) {
  const first = Math.ceil(start / bpb - 1e-9);
  const count = Math.ceil((start + width / ppb) / bpb) - first;
  const every = bpb * ppb >= 36 ? 1 : bpb * ppb >= 18 ? 2 : 4;
  return (
    <div
      data-testid="roll-ruler"
      className="relative bg-ink-900/95 backdrop-blur border-b border-black/[0.06] cursor-pointer overflow-hidden"
      style={{ width }}
      title="Click to play from here"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        const beat = (e.clientX - e.currentTarget.getBoundingClientRect().left) / ppb;
        void graphApi.seek(Math.max(0, Math.round(start + beat))).catch(() => {});
      }}
    >
      <div className="absolute bottom-0 left-0 h-[3px] rounded-r-sm" style={{ width: length * ppb, background: color }} />
      {Array.from({ length: Math.max(0, count) }, (_, k) => {
        const bar = first + k;
        const x = (bar * bpb - start) * ppb;
        return (
          <div key={bar} className="absolute top-0 bottom-0 pointer-events-none" style={{ left: x }}>
            <div className="absolute bottom-0 h-2 w-px bg-black/20" />
            {bar % every === 0 && <span className="absolute top-1.5 left-1.5 text-[11px] font-medium text-ink-400 tabular-nums">{bar + 1}</span>}
          </div>
        );
      })}
      <div
        className="absolute bottom-[5px] -translate-x-full pointer-events-none text-[9px] font-semibold tracking-wider px-1 rounded-sm text-white"
        style={{ left: length * ppb - 3, background: color }}
      >
        END
      </div>
      <PlayCap start={start} ppb={ppb} />
    </div>
  );
});

// ---------------------------------------------------------------- the playhead

/** Where playback is, for the roll's playhead: one poll shared by its parts. */
const play = { beat: null as number | null, listeners: new Set<(beat: number) => void>(), timer: 0 };

function usePlayBeat() {
  const [beat, setBeat] = useState(play.beat);
  useEffect(() => {
    play.listeners.add(setBeat);
    if (play.listeners.size === 1) {
      const poll = () =>
        graphApi.status().then((s) => {
          play.beat = s.beat;
          play.listeners.forEach((l) => l(s.beat));
        }, () => {});
      void poll();
      play.timer = window.setInterval(poll, 200);
    }
    return () => {
      play.listeners.delete(setBeat);
      if (play.listeners.size === 0) window.clearInterval(play.timer);
    };
  }, []);
  return beat;
}

/** Hidden outside the grid: a transformed line past its end would widen the scrolling area. */
function PlayLine({ start, ppb, width }: { start: number; ppb: number; width: number }) {
  const beat = usePlayBeat();
  if (beat == null || beat < start || (beat - start) * ppb >= width) return null;
  return (
    <div
      data-testid="roll-playhead"
      className="absolute top-0 bottom-0 z-10 w-px bg-sun shadow-[0_0_6px_rgba(201,165,78,0.4)] pointer-events-none transition-transform duration-200 ease-linear"
      style={{ transform: `translateX(${(beat - start) * ppb}px)` }}
    />
  );
}

function PlayCap({ start, ppb }: { start: number; ppb: number }) {
  const beat = usePlayBeat();
  if (beat == null || beat < start) return null;
  return (
    <div className="absolute bottom-0 left-0 pointer-events-none transition-transform duration-200 ease-linear" style={{ transform: `translateX(${(beat - start) * ppb}px)` }}>
      <div className="w-0 h-0 -translate-x-1/2 border-x-[5px] border-x-transparent border-t-[7px] border-t-sun" />
    </div>
  );
}
