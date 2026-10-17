import clsx from "clsx";
import { Lock, Minus, MousePointer2, Pencil, Plus, ZoomIn, ZoomOut } from "lucide-react";
import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  barAt,
  bars as barList,
  barsToBeats,
  beatsToBars,
  clipIndex,
  editorHooks,
  findLane,
  findNoteClip,
  graphApi,
  laneTitle,
  lockReason,
  moduleName,
  laneWriting,
  meterSpans,
  writeLane,
  type ClipRef,
  type GraphState,
  type Meter,
  type Note,
  type NoteClip,
} from "../lib/graph";
import { onFrame } from "../lib/clock";
import { RULER_H } from "../lib/layout";
import { chord, keyName, mod, native } from "../lib/native";
import { GRAPHITE, trackColors, type TrackColor } from "../lib/palette";
import { useGrid } from "../lib/grid";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { DockSlot } from "./Dock";
import GridChip from "./GridChip";
import { Hatch, HBands, VLines } from "./draw";
import { Button, EmptyState, keepFocus, Segment } from "./ui";

const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

// The roll's sizes (taste.md §3.5); its ruler is the arrangement's, 28px.
const KEYS_W = 56;
const ROW_H = 14;
const VEL_H = 64;
/** The velocity strip's bars stop this far from its edges. */
const VEL_PAD = 8;
/** The highest pitch, at the top; the lowest is 0. */
const TOP = 127;
const BLACK = [1, 3, 6, 8, 10];
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
  | { kind: "paint"; pointer: number; base: Shape; from: Basis; cells: Set<string>; result: Shape }
  | { kind: "velocity"; pointer: number; index: number; base: Shape; from: Basis; sel: ReadonlySet<number>; result?: Shape };

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
/** Beats and lengths as written: to a millionth of a beat, never -0. */
const clean = (x: number) => Math.round(x * 1e6) / 1e6 + 0;
const pitchName = (p: number) => `${NOTE_NAMES[((p % 12) + 12) % 12]}${Math.floor(p / 12) - 1}`;

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

/** A note's colour: the track's bar, mixed toward its ink the harder it is played (never so far its name can't be read). */
function fill(tone: TrackColor, velocity: number) {
  return `color-mix(in oklab, ${tone.ink} ${Math.round(clamp(velocity, 0, 1) * 45)}%, ${tone.bar})`;
}

/**
 * The piano roll for one notes clip, named by its lane's address and its id (FMT-12): the clip
 * stays open while the lane is edited around it, and closes when it is gone. Every gesture is one
 * lane write, one undo step.
 */
export default function NoteView({ graph, clip: ref }: { graph: GraphState; clip: ClipRef }) {
  const toast = useStore((s) => s.toast);
  const { timeline } = graph;
  const { module, lane } = ref;
  const server = findNoteClip(timeline, ref);
  // A bar as long as the time signature where the clip starts says (FMT-13).
  const bpb = barAt(timeline.meter, server?.start ?? 0).length;
  const tone = trackColors(timeline, module) ?? GRAPHITE;

  // What the roll shows while a gesture is under way or its write hasn't come back: the
  // clip as edited here. The selection is indices into the notes shown.
  const [draft, setDraft] = useState<Shape | null>(null);
  const [selected, setSelected] = useState<ReadonlySet<number>>(() => new Set());
  const { step: gridStep, on: snapOn } = useGrid((s) => s.notes);
  // Where a note lands: on the grid while snap is on, anywhere (0) while it is off.
  const snap = snapOn ? gridStep : 0;
  const draw = useShell((s) => s.draw);
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
  // So are they while the lane's file doesn't read: the roll shows the notes it played before, and
  // writing them would replace the file's newer text with that old copy (FMT-9).
  const file = `data/${module}/${lane}.json`;
  const broken = graph.problems.find((p) => p.src === file && p.code.startsWith("lane."));
  const blocked = useStore((s) => lockReason(graph, s.connected));
  const why = graph.view.draft_errors.length > 0 ? "code" : broken ? "lane" : blocked ? "app" : null;
  const locked = why !== null;
  const lockedTitle =
    why === "lane"
      ? `${file} doesn't read, so these are the notes it played before. Fix the file and they can be edited again.\n\n${broken!.message}`
      : why === "app"
        ? blocked!
        : "Notes live in the code's lanes: they can be edited again once the code compiles";
  const seen = useRef(server?.notes.length);
  /** The beat to keep under the pointer while zooming. */
  const anchor = useRef<{ beat: number; x: number } | null>(null);

  const clip: Shape | null = draft ?? (server ? { length: server.length, notes: server.notes } : null);
  const start = server?.start ?? 0;
  // A new note is a grid step long, snapped or not (as in Live).
  const step = gridStep;

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
    // Centred on the clip's pitches, so its whole range shows when it fits (a row to spare at
    // 14px rows for an octave in the 300px dock), and on whole rows: no key cut under the ruler.
    const pitches = server.notes.map((n) => n.pitch);
    const mid = pitches.length ? (Math.min(...pitches) + Math.max(...pitches)) / 2 : 60;
    const centred = RULER_H + (TOP - mid) * ROW_H + ROW_H / 2 - (RULER_H + el.clientHeight) / 2;
    el.scrollTop = Math.round(centred / ROW_H) * ROW_H;
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

  // ---------------------------------------------------------------- writing

  /** What the clip as shown now was made from. */
  const basis = (): Basis => {
    if (sending.current.busy) return "pending";
    if (written.current) return written.current;
    const g = latest.current;
    const l = findLane(g.timeline, module, lane);
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
    const at = b ? clipIndex(b.clips, ref) : -1;
    if (!b || at < 0) return drop();
    s.busy = true;
    // The clip keeps its id; an anonymous one gets the one its reference names.
    const clips = b.clips.map((c, i) => (i === at ? { id: ref.id, start: c.start, length: shape.length, notes: shape.notes } : c));
    writeLane(latest.current, module, lane, clips, b.revision, { name: `${trackName()} notes` })
      .then((r) => void (written.current = { revision: r.revision, clips: r.clips }))
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

  /** What the track is called, for the words of its edits ("Hook notes"). */
  const trackName = () => moduleName(latest.current.view.modules.find((m) => m.key === module) ?? { key: module });

  /**
   * ⌫ with no note selected, on a clip with no notes: the clip goes, as it would from the timeline,
   * where it is the selected clip. A clip made by mistake goes with one key; one with notes never does.
   */
  const removeClip = () => {
    const g = latest.current;
    const l = findLane(g.timeline, module, lane);
    const i = l?.kind === "notes" ? clipIndex(l.clips, ref) : -1;
    if (locked || laneWriting(g) || !l || l.kind !== "notes" || i < 0 || l.clips[i].notes.length) return;
    writeLane(g, module, lane, l.clips.filter((_, k) => k !== i), g.view.revision, { name: `deleting a ${trackName()} clip` }).catch((error) =>
      toast(error instanceof Error ? error.message : String(error)),
    );
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
    else if ((e.key === "Delete" || e.key === "Backspace") && !clip.notes.length && !draft) removeClip();
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
      // Draw mode (B): a click adds a note, and a drag paints one in each cell it crosses.
      if (draw && !locked) {
        const g: Gesture = { kind: "paint", pointer: e.pointerId, base: clip, from: basis(), cells: new Set(), result: clip };
        gesture.current = g;
        paint(g, p);
        return;
      }
      gesture.current = { kind: "empty", pointer: e.pointerId, x0: p.x, y0: p.y, keep: e.shiftKey ? selected : new Set(), moved: false };
      return;
    }
    const i = Number(target.dataset.note);
    const was = selected.has(i);
    const sel = e.shiftKey ? (was ? selected : new Set([...selected, i])) : was ? selected : new Set([i]);
    setSelected(sel);
    // Read only: a click selects, nothing moves.
    if (locked) return;
    const kind = (e.target as HTMLElement).closest("[data-edge]") ? "resize" : "move";
    gesture.current = { kind, pointer: e.pointerId, x0: p.x, y0: p.y, index: i, base: clip, from: basis(), sel, shift: e.shiftKey, picked: was };
  };

  const onGridMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const g = gesture.current;
    if (!g || g.pointer !== e.pointerId || g.kind === "velocity" || !clip) return;
    const p = at(e);
    if (g.kind === "paint") return paint(g, p);
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
    if (g.kind === "paint") {
      if (same(g.result, g.base)) return setDraft(null);
      const added = new Set(g.result.notes.map((_, i) => i).filter((i) => i >= g.base.notes.length));
      return commit(g.result, added, g.from);
    }
    // A click on empty grid selects nothing (D4): notes are added by a double-click, or in Draw mode.
    if (g.kind === "empty") {
      if (!g.moved) setSelected(g.keep);
      return;
    }
    if (!g.result) {
      // A click: shift-clicking a selected note lets it go; a plain click selects just this one.
      if (g.shift && g.picked) setSelected(new Set([...selected].filter((i) => i !== g.index)));
      else if (!g.shift) setSelected(new Set([g.index]));
      return;
    }
    if (same(g.result, g.base)) return setDraft(g.base);
    commit(g.result, g.sel, g.from);
  };

  /** A pointer lost mid-gesture puts the clip back as it was. */
  const onGridCancel = (e: React.PointerEvent<HTMLDivElement>) => {
    const g = gesture.current;
    if (!g || g.pointer !== e.pointerId || g.kind === "velocity") return;
    gesture.current = null;
    setMarquee(null);
    if (g.kind === "paint") setDraft(null);
    else if (g.kind !== "empty" && g.result) setDraft(g.base);
  };

  /** A note in the cell at `p`, a grid step long (as in Live), if the cell is inside the clip. */
  const noteAt = (p: { beat: number; pitch: number }, length: number): Note | null => {
    const from = Math.max(0, snapBeat(p.beat, true));
    if (from >= length) return null;
    return { pitch: p.pitch, start: from, length: clean(Math.min(step, length - from)), velocity: VELOCITY };
  };

  /** A double-click on empty grid: a note in that cell. */
  const addNote = (p: { beat: number; pitch: number }) => {
    const note = clip && noteAt(p, clip.length);
    if (!clip || !note) return;
    commit({ length: clip.length, notes: [...clip.notes, note] }, new Set([clip.notes.length]));
  };

  /** Draw mode: a note in each new cell under the pointer that has none starting there; written as one edit on release. */
  const paint = (g: Extract<Gesture, { kind: "paint" }>, p: { beat: number; pitch: number }) => {
    const note = noteAt(p, g.base.length);
    const cell = note && `${note.start}:${note.pitch}`;
    if (!note || !cell || g.cells.has(cell) || g.result.notes.some((n) => n.start === note.start && n.pitch === note.pitch)) return;
    g.cells.add(cell);
    g.result = { length: g.base.length, notes: [...g.result.notes, note] };
    setDraft(g.result);
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
    return <div data-testid="notes" className="flex-1 min-h-0 grid place-items-center text-sm text-fg-muted">This clip is gone.</div>;
  }

  const end = Math.max(clip.length, ...clip.notes.map((n) => n.start + n.length));
  const width = Math.max(visible, Math.ceil((end + bpb) / bpb) * bpb * ppb);
  const height = (TOP + 1) * ROW_H;
  const shown = new Set(picked);
  const track = graph.view.modules.find((m) => m.key === module);
  const declared = findLane(timeline, module, lane);
  const title = track ? moduleName(track) : module;
  const laneName = declared ? laneTitle(declared) : lane;
  // The view is Notes already: its lane is worth naming only beside the track's other notes lanes.
  const lanes = timeline.tracks.find((t) => t.module === module)?.lanes.filter((l) => l.kind === "notes").length ?? 0;
  const firstBar = barAt(timeline.meter, start).bar + 1;
  const lastBar = barAt(timeline.meter, start + clip.length - 1e-6).bar + 1;
  // Notes being dragged lift off the grid.
  const lifting = gesture.current?.kind === "move" && !!gesture.current.result;

  return (
    <div
      ref={root}
      data-testid="notes"
      data-grid-view="notes"
      tabIndex={0}
      onKeyDown={onKeyDown}
      className="flex-1 min-h-0 flex flex-col bg-canvas outline-none"
      aria-label={`Notes of ${title}'s ${laneName} clip at bar ${firstBar}`}
    >
      {/* The clip's place and the roll's tools sit in the dock's header: the roll itself is edge to edge (taste.md §3.5). */}
      <DockSlot at="title">
        {lanes > 1 && (
          <>
            <span className="font-medium text-fg truncate" title={laneName === lane ? lane : `${laneName} (${lane})`}>
              {laneName}
            </span>
            <span className="text-fg-faint">·</span>
          </>
        )}
        <span className="shrink-0">{lastBar > firstBar ? `bars ${firstBar}–${lastBar}` : `bar ${firstBar}`}</span>
      </DockSlot>
      <DockSlot at="tools">
        {locked && (
          <span
            data-testid="notes-locked"
            data-why={why}
            className="shrink-0 inline-flex items-center gap-1.5 h-ctl-sm px-2 rounded-sm bg-hover text-sm font-medium text-fg"
            title={lockedTitle}
          >
            <Lock size={14} className="text-fg-muted" />
            {why === "lane" ? `Read only: ${file} doesn't read` : why === "app" ? "Read only" : "Read only until the code compiles"}
          </span>
        )}
        {/* The editing tools only while it can be edited: read only, the reason takes their room. */}
        {!locked && (
          <>
            <div role="radiogroup" aria-label="Tool" className="shrink-0 h-ctl-sm flex items-center gap-0.5 p-0.5 rounded-sm bg-hover">
              <Segment role="radio" aria-checked={!draw} data-tool="select" on={!draw} title="Select (B toggles): click selects, double-click adds a note" onClick={() => useShell.getState().set({ draw: false })} className="!px-1.5">
                <MousePointer2 size={14} />
              </Segment>
              <Segment role="radio" aria-checked={draw} data-tool="draw" on={draw} title="Draw (B): click adds a note, drag paints" onClick={() => useShell.getState().set({ draw: true })} className="!px-1.5">
                <Pencil size={14} />
              </Segment>
            </div>
            <Divider />
            {/* Grid [1/16 ▾] and the snap toggle (taste.md §3.5). */}
            <GridChip view="notes" />
            <Divider />
          </>
        )}
        <LengthField
          bars={Number(beatsToBars(timeline.meter, start, clip.length).toFixed(3))}
          toBeats={(n) => clean(barsToBeats(timeline.meter, start, n))}
          disabled={locked}
          onChange={setLength}
        />
        <Divider />
        <div className="shrink-0 flex items-center">
          <Button size="sm" onMouseDown={keepFocus} onClick={() => zoom(0.8)} title={`Zoom out (${mod.cmd} + scroll)`} aria-label="Zoom out" className="w-ctl-sm !px-0">
            <ZoomOut size={16} />
          </Button>
          <Button size="sm" onMouseDown={keepFocus} onClick={() => zoom(1.25)} title={`Zoom in (${mod.cmd} + scroll)`} aria-label="Zoom in" className="w-ctl-sm !px-0">
            <ZoomIn size={16} />
          </Button>
        </div>
      </DockSlot>

      <div className="flex-1 min-h-0 relative">
        <div
          ref={scroller}
          data-testid="roll-scroller"
          className="absolute inset-0 overflow-auto"
          onScroll={(e) => strip.current && (strip.current.style.transform = `translateX(${-e.currentTarget.scrollLeft}px)`)}
        >
          <div className="relative" style={{ width: KEYS_W + width, height: RULER_H + height }}>
            <div className="sticky top-0 z-30 flex" style={{ height: RULER_H }}>
              <div className="sticky left-0 z-40 shrink-0 bg-chrome border-r border-b border-line" style={{ width: KEYS_W }} />
              <Ruler width={width} ppb={ppb} meter={timeline.meter} start={start} length={clip.length} tone={tone} />
            </div>
            <div className="flex">
              <Keys selected={new Set(picked.map((i) => clip.notes[i].pitch))} tone={tone} />
              <div
                ref={grid}
                data-testid="roll-grid"
                className={clsx("relative touch-none", locked ? "cursor-default" : draw ? "cursor-crosshair" : "cursor-default")}
                style={{ width, height }}
                title={
                  locked
                    ? why === "lane"
                      ? `Read only until ${file} reads again`
                      : "Read only until the code compiles"
                    : draw
                      ? "Draw: click to add a note · drag to paint notes · B to select"
                      : `Double-click to add a note · drag a note to move it · drag its right edge to resize · drag across empty space to select · ${keyName("Backspace")} deletes · ${chord("D")} duplicates · arrows move · B to draw`
                }
                onDoubleClick={(e) => {
                  // Select mode adds notes with a double-click; Draw mode added one on the first click.
                  if (locked || draw || (e.target as HTMLElement).closest("[data-note]")) return;
                  addNote(at(e));
                }}
                onPointerDown={onGridDown}
                onPointerMove={onGridMove}
                onPointerUp={onGridUp}
                onPointerCancel={onGridCancel}
                onLostPointerCapture={onGridCancel}
              >
                <Lanes width={width} ppb={ppb} meter={timeline.meter} start={start} step={gridStep} />
                {/* Past the clip's end: hatched, behind its edge in the track's colour. */}
                <div className="absolute top-0 bottom-0 right-0 pointer-events-none border-l-2 bg-chrome/70" style={{ left: clip.length * ppb, borderColor: tone.bar }}>
                  <Hatch />
                </div>
                {clip.notes.map((n, i) => {
                  const w = Math.max(n.length * ppb - 1, 4);
                  const outside = n.start >= clip.length;
                  const on = shown.has(i);
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
                      className={clsx(
                        "absolute rounded-xs cursor-grab active:cursor-grabbing",
                        on
                          ? clsx(
                              "z-10 text-white",
                              lifting
                                ? "shadow-[inset_0_0_0_1px_white,0_0_0_1px_var(--color-fg-strong),var(--shadow-lift)]"
                                : "shadow-[inset_0_0_0_1px_white,0_0_0_1px_var(--color-fg-strong)]",
                            )
                          : locked
                            ? "text-fg-muted shadow-[inset_0_0_0_1px_var(--color-fg-off)]"
                            : "text-fg-strong shadow-[inset_0_0_0_1px_var(--note-ink)] hover:shadow-[inset_0_0_0_2px_var(--note-ink)]",
                        outside && "opacity-40",
                      )}
                      style={
                        {
                          left: n.start * ppb,
                          top: (TOP - n.pitch) * ROW_H + 1,
                          width: w,
                          height: ROW_H - 2,
                          background: on ? tone.ink : locked ? "var(--color-line-strong)" : fill(tone, n.velocity),
                          "--note-ink": tone.ink,
                        } as React.CSSProperties
                      }
                    >
                      {w > 26 && (
                        <span className="absolute left-1.5 top-1/2 -translate-y-1/2 text-xs font-medium leading-none pointer-events-none">
                          {pitchName(n.pitch)}
                        </span>
                      )}
                      {/* The edge to resize by: never more than half a short note, which must stay draggable. */}
                      <div data-edge className="absolute -right-0.5 inset-y-0 w-[min(7px,calc(50%+2px))] cursor-ew-resize rounded-r-xs hover:bg-white/30" />
                    </div>
                  );
                })}
                {marquee && (
                  <div
                    className="absolute z-20 pointer-events-none rounded-xs border border-accent bg-accent/10"
                    style={{ left: marquee.x0, top: marquee.y0, width: marquee.x1 - marquee.x0, height: marquee.y1 - marquee.y0 }}
                  />
                )}
                <PlayLine start={start} ppb={ppb} width={width} />
              </div>
            </div>
          </div>
        </div>
        {clip.notes.length === 0 && (
          <EmptyState className="absolute top-1/2 -translate-x-1/2 -translate-y-1/2 pointer-events-none whitespace-nowrap" style={{ left: KEYS_W + Math.min(clip.length * ppb, visible) / 2 }}>
            {draw ? "Click in the grid to add a note" : "Double-click in the grid to add a note"}
          </EmptyState>
        )}
      </div>

      <div data-testid="velocity" className="shrink-0 flex border-t border-line bg-chrome" style={{ height: VEL_H }}>
        <div className="shrink-0 flex flex-col justify-center pl-2 border-r border-line text-xs font-medium text-fg-muted" style={{ width: KEYS_W }}>
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
            <div className="absolute inset-x-0 border-t border-dashed border-line pointer-events-none" style={{ top: VEL_PAD }} />
            <div className="absolute inset-x-0 border-t border-dashed border-line pointer-events-none" style={{ top: VEL_H / 2 }} />
            <div className="absolute top-0 bottom-0 right-0 pointer-events-none bg-canvas/60" style={{ left: clip.length * ppb }} />
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
                    className={clsx("absolute inset-y-0 w-3 -ml-[5px] group", !locked && "cursor-ns-resize")}
                    style={{ left: n.start * ppb }}
                  >
                    {/* Read only, grey like its notes: nothing here can be dragged. */}
                    <div
                      className="absolute left-[5px] w-[2px] rounded-full"
                      style={{ bottom: VEL_PAD, height: h, background: shown.has(i) ? "var(--color-fg-strong)" : locked ? "var(--color-fg-off)" : tone.ink }}
                    />
                    <div
                      className={clsx("absolute left-[2px] size-2 rounded-full ring-1", shown.has(i) ? "ring-fg-strong bg-white" : clsx("ring-(--note-ink)", !locked && "group-hover:scale-125"))}
                      style={
                        {
                          bottom: VEL_PAD + h - 4,
                          background: shown.has(i) ? undefined : locked ? "var(--color-line-strong)" : fill(tone, n.velocity),
                          "--note-ink": locked ? "var(--color-fg-off)" : tone.ink,
                        } as React.CSSProperties
                      }
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

// In a narrow dock (the browser open in a small window) the tools' dividers go before any tool does.
const Divider = () => <div className="w-px h-5 shrink-0 bg-line @max-[900px]/dock:hidden" />;

/**
 * The clip's length in bars, typed or stepped a bar at a time. `bars` is how many it spans and
 * `toBeats` turns a count of bars into beats, both as the song's time signatures count them.
 */
function LengthField({
  bars: value,
  toBeats,
  disabled,
  onChange,
}: {
  bars: number;
  toBeats: (bars: number) => number;
  disabled: boolean;
  onChange: (beats: number) => void;
}) {
  const [text, setText] = useState<string | null>(null);
  const done = () => {
    const n = Number(text);
    setText(null);
    if (text !== null && n > 0 && Number.isFinite(n)) onChange(toBeats(n));
  };
  return (
    <div className="shrink-0 flex items-center gap-1.5" title="How long the clip is, in bars. Notes past its end don't play.">
      <span className="text-xs text-fg-muted @max-[900px]/dock:hidden">Length</span>
      <div className="h-ctl-sm flex items-center rounded-sm bg-raised border border-line-strong hover:not-focus-within:border-fg-off focus-within:border-accent has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-accent has-[:focus-visible]:outline-offset-1 transition-colors duration-80 ease-linear">
        <button aria-label="One bar shorter" disabled={disabled || value <= 1} onClick={() => onChange(toBeats(Math.ceil(value - 1 - 1e-9) || 1))} className="h-full w-6 grid place-items-center text-fg-muted enabled:hover:text-fg disabled:text-fg-off cursor-pointer">
          <Minus size={12} />
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
          className="w-9 h-full bg-transparent text-center text-sm font-medium text-fg disabled:text-fg-off outline-none"
        />
        <button aria-label="One bar longer" disabled={disabled} onClick={() => onChange(toBeats(Math.floor(value + 1 + 1e-9)))} className="h-full w-6 grid place-items-center text-fg-muted enabled:hover:text-fg disabled:text-fg-off cursor-pointer">
          <Plus size={12} />
        </button>
      </div>
      <span className="text-xs text-fg-muted @max-[900px]/dock:hidden">{value === 1 ? "bar" : "bars"}</span>
    </div>
  );
}

/** The piano keys, one row per pitch, C's named; keys of selected notes lit in the track's colour. */
const Keys = memo(
  function Keys({ selected, tone }: { selected: Set<number>; tone: TrackColor }) {
    return (
      <div className="sticky left-0 z-20 shrink-0 bg-raised border-r border-line" style={{ width: KEYS_W }}>
        {Array.from({ length: TOP + 1 }, (_, k) => {
          const p = TOP - k;
          const black = BLACK.includes(p % 12);
          const lit = selected.has(p);
          return (
            <div
              key={p}
              data-key={p}
              className={clsx("relative", p % 12 === 0 ? "border-b border-line-strong" : !black && !BLACK.includes((p + 11) % 12) && "border-b border-line")}
              style={{ height: ROW_H }}
            >
              {lit && <div className="absolute inset-0" style={{ background: tone.bar }} />}
              {/* The edge between the two white keys either side of a black one, under it: every white key is a key. */}
              {black && <div className="absolute inset-x-0 top-1/2 h-px bg-line" />}
              {black && <div className="absolute left-0 inset-y-[1px] w-[60%] rounded-r-xs" style={{ background: lit ? tone.ink : "var(--color-gray-700)" }} />}
              {p % 12 === 0 && (
                // On a lit key, the strongest grey: the bar tones are light, so it reads.
                <span className={clsx("absolute right-1.5 top-1/2 -translate-y-1/2 text-xs leading-none", lit ? "font-medium text-fg-strong" : p === 60 ? "font-medium text-fg-muted" : "text-fg-faint")}>
                  {pitchName(p)}
                </span>
              )}
            </div>
          );
        })}
      </div>
    );
  },
  (a, b) => a.tone === b.tone && a.selected.size === b.selected.size && [...a.selected].every((p) => b.selected.has(p)),
);

/** Row shading (black keys darker, a line under each C) and the bar, beat and snap lines, bars as the song's time signatures count them. */
const Lanes = memo(function Lanes({ width, ppb, meter, start, step }: { width: number; ppb: number; meter: Meter[]; start: number; step: number }) {
  // From the top: pitch 127 (G9) down, twelve rows a period: black keys' rows shaded, a line under each.
  const bands = Array.from({ length: 12 }, (_, k) => {
    const pc = (TOP - k) % 12;
    return [
      ...(BLACK.includes(pc) ? [{ y: k * ROW_H, h: ROW_H, color: "var(--color-hover)" }] : []),
      { y: (k + 1) * ROW_H - 1, h: 1, color: pc === 0 ? "var(--color-line)" : "var(--color-hover)" },
    ];
  }).flat();
  /** Where the first of the lines every `period` beats from `origin` falls, in pixels from `from`. */
  const offset = (origin: number, from: number, period: number) => (((((origin - from) % period) + period) % period) * ppb);
  return (
    <>
      <HBands period={12 * ROW_H} bands={bands} />
      {meterSpans(meter, start, start + width / ppb).map((s) => (
        <VLines
          key={s.start}
          className="inset-y-0"
          style={{ left: (s.start - start) * ppb, width: (s.end - s.start) * ppb }}
          layers={[
            ...(step && step < 1 && step * ppb >= 6 ? [{ every: step * ppb, at: offset(0, s.start, step), color: "var(--color-hover)" }] : []),
            { every: ppb, at: offset(0, s.start, 1), color: "var(--color-line)" },
            { every: s.bar * ppb, at: offset(s.origin, s.start, s.bar), color: "var(--color-line-strong)" },
          ]}
        />
      ))}
    </>
  );
});

/** Bar numbers of the song, the clip's extent in its track's colour, and a click to play from there. */
const Ruler = memo(function Ruler({ width, ppb, meter, start, length, tone }: { width: number; ppb: number; meter: Meter[]; start: number; length: number; tone: TrackColor }) {
  const shown = barList(meter, start + width / ppb).filter((b) => b.start >= start - 1e-9);
  return (
    <div
      data-testid="roll-ruler"
      className="relative bg-chrome border-b border-line cursor-pointer overflow-hidden"
      style={{ width }}
      title="Click to play from here"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        const beat = (e.clientX - e.currentTarget.getBoundingClientRect().left) / ppb;
        void graphApi.seek(Math.max(0, Math.round(start + beat))).catch(() => {});
      }}
    >
      <div className="absolute bottom-0 left-0 h-[3px] rounded-r-xs" style={{ width: length * ppb, background: tone.bar }} />
      {shown.map((b) => {
        // Narrow bars are numbered every second or fourth.
        const w = b.length * ppb;
        const every = w >= 36 ? 1 : w >= 18 ? 2 : 4;
        return (
          <div key={b.bar} className="absolute top-0 bottom-0 pointer-events-none" style={{ left: (b.start - start) * ppb }}>
            <div className="absolute bottom-0 h-2 w-px bg-line-strong" />
            {b.bar % every === 0 && <span className="absolute top-1.5 left-1.5 text-xs font-medium text-fg-faint">{b.bar + 1}</span>}
          </div>
        );
      })}
      <div
        className="absolute bottom-[5px] -translate-x-full pointer-events-none text-xs leading-none font-medium px-1 py-px rounded-xs text-white"
        style={{ left: length * ppb - 3, background: tone.ink }}
      >
        End
      </div>
      <PlayCap start={start} ppb={ppb} />
    </div>
  );
});

// ---------------------------------------------------------------- the playhead

/**
 * The roll's playhead: a line over the grid and a triangle in its ruler, moved every frame from
 * the transport clock (taste.md TL-9). Hidden outside the grid: a line past its end would widen the
 * scrolling area.
 */
function PlayLine({ start, ppb, width }: { start: number; ppb: number; width: number }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(
    () =>
      onFrame((beat) => {
        const el = ref.current;
        if (!el) return;
        const x = (beat - start) * ppb;
        el.style.display = beat < start || x >= width ? "none" : "";
        el.style.transform = `translateX(${x}px)`;
      }),
    [start, ppb, width],
  );
  return <div ref={ref} data-testid="roll-playhead" className="absolute top-0 bottom-0 z-10 w-px bg-fg-strong pointer-events-none" />;
}

function PlayCap({ start, ppb }: { start: number; ppb: number }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(
    () =>
      onFrame((beat) => {
        const el = ref.current;
        if (!el) return;
        el.style.display = beat < start ? "none" : "";
        el.style.transform = `translateX(${(beat - start) * ppb}px)`;
      }),
    [start, ppb],
  );
  return (
    <div ref={ref} className="absolute bottom-0 left-0 pointer-events-none">
      <div className="w-0 h-0 -translate-x-1/2 border-x-[4px] border-x-transparent border-t-[6px] border-t-fg-strong" />
    </div>
  );
}
