import clsx from "clsx";
import { Check, ChevronDown, FoldVertical, Lock, Minus, MousePointer2, Pencil, Plus, UnfoldVertical, ZoomIn, ZoomOut } from "lucide-react";
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
import { attach, KEY_VELOCITY, PAINT_MS, pulse, release, retune, strike, useAudition } from "../lib/audition";
import { onFrame } from "../lib/clock";
import { ROLL_RULER_H } from "../lib/layout";
import { chord, isMac, keyName, mod, native } from "../lib/native";
import { humanize, legato, quantize, swingUnit, targets, transpose } from "../lib/noteTools";
import { chordAt, fit, inKey, loadTheory, stepIn, useTheory, type KeyView } from "../lib/songKey";
import { getLocal, setLocal, useLocal } from "../lib/local";
import { GRAPHITE, trackColors, type TrackColor } from "../lib/palette";
import { pickName, useGrid } from "../lib/grid";
import { keysWidth, namesOf, pitchName, rowName } from "../lib/rowNames";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import AuditionTools from "./AuditionTools";
import { DockSlot } from "./Dock";
import GridChip from "./GridChip";
import NoteMenu, { type NoteTool } from "./NoteMenu";
import { Hatch, HBands, VLines } from "./draw";
import { Button, EmptyState, keepFocus, MenuRow, Popover, Segment } from "./ui";
import { afterLaneWrites, hasOwnId } from "../lib/graph";
import { allMine, editing, showsOwn, through } from "../lib/history";
import { playhead, send as sendGesture, serverRef, useClipboard, type Copied, type Gesture as ClipGesture, type ServerRef } from "../lib/clips";
import { begin as leaseDrag, type LaneLease } from "../lib/laneLease";
import { look } from "../lib/focus";

// The roll's sizes (taste.md §3.5); its ruler is the arrangement's, 28px. The key column is 56 px,
// or 88 where its keys say drum names (`keysWidth`, polish A4). A row is 14 px unless the view
// settings say another height, 10 to 24 (NOTE-9, taste.md NT-5).
const ROW_H = 14;
const ROW_MIN = 10;
const ROW_MAX = 24;
/** How much ⌘↓ and ⌘↑ change a row's height, in px. */
const ROW_STEP = 2;
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
/** Two presses on empty grid this close in time (ms) and place (px) are a double-click's: the second sounds the note it adds. */
const DOUBLE_MS = 500;
const DOUBLE_PX = 5;

/** The part of a clip the piano roll edits: its length and its notes. */
interface Shape { length: number; notes: Note[] }
/** The roll's view settings (NOTE-9), this machine's, in the song's `.music2/view.json`: Lock to key, folded drum rows, the row height. */
interface RollView { lock?: boolean; fold?: boolean; rows?: number }
/** Whether a pointer event holds ⌘ (Ctrl on Linux): the note goes off the grid (NOTE-6). On a Mac a Ctrl-click is a right-click. */
const offGrid = (e: { metaKey: boolean; ctrlKey: boolean }) => (isMac ? e.metaKey : e.ctrlKey);
interface Box { x0: number; y0: number; x1: number; y1: number }
/**
 * What a shape of the clip was made from: the lane's clips at a revision (the graph's, or the
 * one this view's last write made), or "pending", the write on its way, whose revision is
 * known once it comes back. A write names it, so the server refuses it over a change it missed.
 */
type Basis = { revision: number; clips: NoteClip[] } | "pending";
type Gesture =
  | { kind: "move" | "resize"; pointer: number; x0: number; y0: number; index: number; base: Shape; from: Basis; sel: ReadonlySet<number>; shift: boolean; picked: boolean; result?: Shape }
  /** Draw mode's press (B): one note, or a chord stamp's notes (NOTE-4), all after `base`'s; a drag sets their length. */
  | { kind: "empty"; pointer: number; x0: number; y0: number; keep: ReadonlySet<number>; moved: boolean }
  | { kind: "paint"; pointer: number; base: Shape; from: Basis; cells: Set<string>; result: Shape }
  | { kind: "draw"; pointer: number; x0: number; base: Shape; from: Basis; moved: boolean; result: Shape }
  | { kind: "velocity"; pointer: number; index: number; base: Shape; from: Basis; sel: ReadonlySet<number>; result?: Shape };

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
/** Beats and lengths as written: to a millionth of a beat, never -0. */
const clean = (x: number) => Math.round(x * 1e6) / 1e6 + 0;

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
  const declared = findLane(timeline, module, lane);
  // The key column: wider where its keys say drum names. Effects read it through a ref.
  const keysW = keysWidth(declared);
  const keysWRef = useRef(keysW);
  keysWRef.current = keysW;

  // What the roll shows while a gesture is under way or its write hasn't come back: the
  // clip as edited here. The selection is indices into the notes shown.
  const [draft, setDraft] = useState<Shape | null>(null);
  const [selected, setSelected] = useState<ReadonlySet<number>>(() => new Set());
  const { step: gridStep, on: snapOn, pick: gridPick, triplet: gridTriplet } = useGrid((s) => s.notes);
  // Where a note lands: on the grid while snap is on, anywhere (0) while it is off.
  const snap = snapOn ? gridStep : 0;
  const draw = useShell((s) => s.draw);
  // The roll's view settings (NOTE-9): Lock to key, folded drum rows, the row height.
  const roll = useLocal<RollView>("roll", {});
  const setRoll = (r: RollView) => setLocal("roll", { ...getLocal<RollView>("roll", {}), ...r });
  const rowH = clamp(Math.round(roll.rows ?? ROW_H), ROW_MIN, ROW_MAX);
  // A lane whose rows are named drums has no key (X26): no shading by it, no Lock to key, no chords.
  const names = namesOf(declared);
  const drums = Object.keys(names).length > 0;
  const keyed: KeyView | null = timeline.key && !drums ? timeline.key : null;
  const locking = !!keyed && !!roll.lock;
  // The chord a stamp places (NOTE-4), while the key has chords to stamp.
  const tool = useShell((s) => s.chord);
  const stamp = keyed && keyed.chords.length > 0 ? tool : null;
  // Swing's amounts, the grooves and the chord kinds: the server's table, read once.
  const theory = useTheory((t) => t.theory);
  useEffect(() => void loadTheory(), []);
  /** Where the pointer is over the grid, for a stamp's outline: null when it isn't, or no chord is picked. */
  const [hover, setHover] = useState<{ beat: number; pitch: number; off: boolean } | null>(null);
  const [ppb, setPpb] = useState(48);
  const [visible, setVisible] = useState(0);
  const [marquee, setMarquee] = useState<Box | null>(null);
  /** The right-click menu (D1), where it opened. */
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [, setWrites] = useState(0);

  const root = useRef<HTMLDivElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const grid = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture | null>(null);
  /** The notes a pointer holds down on the grid, sounding (audition): their handles, null where nothing sounds. A chord stamp holds several. */
  const hold = useRef<{ pointer: number; h: (number | null)[] } | null>(null);
  /** A gesture that added notes (a paste), on its way: the clip's notes before it, to select the new ones once its revision shows. */
  const added = useRef<{ revision: number; before: Note[] } | null>(null);
  /** The last press on empty grid: a second one close by is a double-click's. */
  const lastEmpty = useRef<{ t: number; x: number; y: number } | null>(null);
  const sending = useRef<{ busy: boolean; next: { shape: Shape; name?: string } | null }>({ busy: false, next: null });
  /** The lane as this view's last write made it, while the roll shows that write. */
  const written = useRef<{ revision: number; clips: NoteClip[] } | null>(null);
  /** The pointer gesture heard as it goes while the song plays (8b.7), and its lease: null when it has none. */
  const leased = useRef<{ g: Gesture; lease: LaneLease | null } | null>(null);
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
  // The rows shown, top to bottom (NOTE-8): on a drum lane, folded, the drums the clip used when it
  // opened or Fold was pressed (the kit's named rows if none), and any a note is on since, so no row
  // goes while you work; else every pitch, 127 down to 0.
  const folded = drums && roll.fold !== false;
  const [foldFrom, setFoldFrom] = useState<number[]>(() => usedRows(names, server?.notes ?? []));
  const rows = folded ? [...new Set([...foldFrom, ...(server?.notes ?? []).map((n) => n.pitch), ...(clip?.notes ?? []).map((n) => n.pitch)])].sort((a, b) => b - a) : null;
  // A row shown stays shown while the roll is open: kept, until Fold folds again.
  const fresh = rows?.filter((p) => !foldFrom.includes(p)) ?? [];
  useEffect(() => {
    if (fresh.length) setFoldFrom((was) => [...new Set([...was, ...fresh])]);
  });
  const nRows = rows ? rows.length : TOP + 1;
  /** The row pitch `p` is on, from the top; -1 when it isn't shown. */
  const rowOf = (p: number) => (rows ? rows.indexOf(p) : TOP - p);
  /** The pitch of row `r` from the top, the nearest row there is. */
  const pitchOfRow = (r: number) => (rows ? rows[clamp(r, 0, rows.length - 1)] : clamp(TOP - r, 0, TOP));
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
    // A paste's notes, once the graph shows them: selected (NOTE-5).
    const a = added.current;
    if (a && !draft && server && graph.view.revision >= a.revision) {
      added.current = null;
      setSelected(new Set(addedTo(a.before, server.notes)));
    }
  });

  // Audition listens while the roll is open; closing it (another clip, another view) lets go of every note.
  useEffect(() => attach(), []);

  // Open fitted to the clip's length, centred on its notes.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el || !server) return;
    setPpb(clamp(Math.floor((el.clientWidth - keysWRef.current - 32) / server.length), 12, 160));
    // Centred on the clip's pitches, so its whole range shows when it fits (a row to spare at
    // 14px rows for an octave in the 300px dock), and on whole rows: no key cut under the ruler.
    // Folded, its few rows start at the top.
    const pitches = server.notes.map((n) => n.pitch);
    const mid = pitches.length ? (Math.min(...pitches) + Math.max(...pitches)) / 2 : 60;
    const centred = ROLL_RULER_H + (TOP - mid) * rowH + rowH / 2 - (ROLL_RULER_H + el.clientHeight) / 2;
    el.scrollTop = rows ? 0 : Math.round(centred / rowH) * rowH;
    root.current?.focus({ preventScroll: true });
    // Only on opening: later edits keep the view where the user put it.
  }, []);

  useEffect(() => {
    const el = scroller.current!;
    const observer = new ResizeObserver(() => setVisible(el.clientWidth - keysWRef.current));
    observer.observe(el);
    // ⌘/Ctrl + wheel zooms time around the pointer; plain wheels scroll.
    const wheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      const x = e.clientX - el.getBoundingClientRect().left - keysWRef.current;
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
    if (el) anchor.current = { beat: (el.scrollLeft + el.clientWidth / 2 - keysW) / ppb, x: el.clientWidth / 2 - keysW };
    setPpb((p) => clamp(p * by, 8, 400));
  };
  /** The row in the middle of the roll, kept there while rows get taller or shorter. */
  const rowAnchor = useRef<number | null>(null);
  /** ⌘↓ (`dir` 1) and ⌘↑ (-1): rows 2 px taller or shorter, 10 to 24 px (NOTE-9, taste.md NT-5). */
  const resizeRows = (dir: 1 | -1) => {
    const h = clamp(rowH + dir * ROW_STEP, ROW_MIN, ROW_MAX);
    const el = scroller.current;
    if (h === rowH) return;
    if (el) rowAnchor.current = (el.scrollTop + (el.clientHeight - ROLL_RULER_H) / 2) / rowH;
    setRoll({ rows: h });
  };
  useLayoutEffect(() => {
    const el = scroller.current, r = rowAnchor.current;
    rowAnchor.current = null;
    if (el && r !== null) el.scrollTop = Math.max(0, r * rowH - (el.clientHeight - ROLL_RULER_H) / 2);
  }, [rowH]);

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
   * the shapes made on top of it. `name` is the undo step's ("transposing Bass notes"); without
   * one it is "Bass notes".
   */
  const send = (shape: Shape, from: Basis, name?: string, lease: LaneLease | null = null) => {
    const s = sending.current;
    if (s.busy) {
      lease?.abort();
      s.next = { shape, name };
      return;
    }
    const b = from === "pending" ? written.current : from;
    const at = b ? clipIndex(b.clips, ref) : -1;
    if (!b || at < 0) {
      lease?.abort();
      return drop();
    }
    s.busy = true;
    // The clip keeps its id; an anonymous one gets the one its reference names.
    const clips = b.clips.map((c, i) => (i === at ? { id: ref.id, start: c.start, length: shape.length, notes: shape.notes } : c));
    const step = { name: name ?? `${trackName()} notes` };
    const gesture = lease && notesGesture(shape);
    if (lease && !gesture) lease.abort();
    // A drag heard as it went (8b.7) is written by its lease: one notes gesture, in the lane writes' queue.
    const write =
      lease && gesture
        ? afterLaneWrites(latest.current, (within) => lease.finish(gesture, step, within)).then((r) => ({ revision: r.revision, clips }))
        : writeLane(latest.current, module, lane, clips, b.revision, step, (now) =>
            withClip(now, (c) => {
              const made = merged(b.clips[at], shape, c);
              return made && [{ id: ref.id, start: c.start, length: made.length, notes: made.notes }];
            }),
          );
    write
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
        if (next) send(next.shape, "pending", next.name);
        else setWrites((n) => n + 1);
      });
  };

  /**
   * The lane as `now` shows it, with this clip as `make` makes it from what `now` has of it (none:
   * it goes): a write made anew from the lane as the window shows it. Null when the clip is gone, or
   * `make` can't make it.
   */
  const withClip = (now: GraphState, make: (c: NoteClip) => NoteClip[] | null): NoteClip[] | null => {
    const l = findLane(now.timeline, module, lane);
    const i = l?.kind === "notes" ? clipIndex(l.clips, ref) : -1;
    const made = l?.kind === "notes" && i >= 0 ? make(l.clips[i]) : null;
    return l?.kind === "notes" && made ? l.clips.flatMap((c, k) => (k === i ? made : [c])) : null;
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
    if (locked || !l || l.kind !== "notes" || i < 0 || l.clips[i].notes.length) return;
    writeLane(g, module, lane, l.clips.filter((_, k) => k !== i), g.view.revision, { name: `deleting a ${trackName()} clip` }, (now) => withClip(now, () => [])).catch((error) =>
      toast(error instanceof Error ? error.message : String(error)),
    );
  };

  /** Show and write a changed shape of the clip, made from `from`, selecting `sel` (indices into `shape.notes`); `name` names its undo step. */
  const commit = (shape: Shape, sel: ReadonlySet<number>, from: Basis = basis(), name?: string, lease: LaneLease | null = null) => {
    if (locked) {
      lease?.abort();
      return;
    }
    const [notes, s] = tidy(shape.notes, sel);
    const next = { length: clean(shape.length), notes };
    setDraft(next);
    setSelected(s);
    send(next, from, name, lease);
  };

  /** This clip as `shape`, as the server's notes gesture names it: null when the timeline doesn't draw it now. */
  const notesGesture = (shape: Shape): ClipGesture | null => {
    const l = findLane(latest.current.timeline, module, lane);
    const server = l ? serverRef(l, ref) : null;
    if (!server) return null;
    const notes = shape.notes.map((n) => ({ pitch: n.pitch, start: n.start, length: n.length, velocity: n.velocity }));
    return { op: "notes", clip: server, notes, length: shape.length };
  };

  /**
   * A clip gesture on this clip that the server works out (a groove, NOTE-7; a paste, NOTE-5),
   * sent after this view's lane writes on their way, once the window shows its own edits (the gate,
   * lib/history): it names the clip with the hash the graph draws it with then, so the server
   * refuses it over a change it missed, and a change made elsewhere since the roll drew the notes
   * (by their places) refuses it here, unless the roll drew them before the window showed its own
   * last edit and only its own edits came since (then it goes from the notes as shown). One Tx, one undo step named `name`. Its notes
   * show when the graph does; with `select`, the notes it added are then selected.
   */
  const act = (make: (clip: ServerRef) => ClipGesture, name: string, select = false) => {
    if (locked) return;
    const before = clip?.notes ?? [];
    const asShown = !draft && showsOwn(latest.current);
    const drawnAt = latest.current.view.revision;
    afterLaneWrites(latest.current, async (within) => {
      const g = await through(latest.current.session, within);
      const l = findLane(g.timeline, module, lane);
      const c = l ? serverRef(l, ref) : null;
      const now = findNoteClip(g.timeline, ref);
      if (!c || !now) throw new Error("This clip is gone.");
      if ((asShown || !allMine(g.session, drawnAt, g.view.revision)) && !sameNotes(now.notes, before)) throw new Error(`${trackName()}'s clip was changed in another window or by an agent. It shows as it is now; try again.`);
      return sendGesture(g, make(c), { name }, undefined, within);
    })
      .then((done) => {
        if (!select || done.before === done.after) return;
        added.current = { revision: done.revision, before };
        // Drawn again, in case the graph showed the revision before this answer came.
        setWrites((n) => n + 1);
      })
      .catch((error) => toast(error instanceof Error ? error.message : String(error)));
  };

  /**
   * Gesture `g` shows `shape` now: while the song plays it is heard so (8b.7), by a lease begun at
   * its first change. None while a write of this view is on its way or the roll shows one the graph
   * doesn't yet (the lease names the clip as the graph draws it), nor on a lane with a clip without
   * an id of its own (the write's ids are the roll's): that drag is heard when let go, as before.
   */
  const hear = (g: Gesture, shape: Shape) => {
    const gesture = notesGesture(shape);
    const l = leased.current;
    if (l?.g === g) return gesture && l.lease?.update(gesture);
    const graph = latest.current;
    const lane0 = findLane(graph.timeline, module, lane);
    const named = lane0?.kind === "notes" && lane0.clips.every((_, i) => hasOwnId(lane0.clips, i));
    const free = !locked && named && !!gesture && !sending.current.busy && !written.current && !laneWriting(graph) && showsOwn(graph);
    leased.current = {
      g,
      lease: free
        ? leaseDrag(graph, gesture!, (why) => {
            toast(why);
            cancelGesture();
          })
        : null,
    };
  };
  /** The lease of gesture `g`, taken: it is let go now. */
  const taken = (g: Gesture) => {
    const l = leased.current;
    leased.current = null;
    return l?.g === g ? l.lease : null;
  };
  /** Esc, the server ending a drag, the view gone: the gesture under way is put back, and its lease's preview ends. */
  const cancelGesture = () => {
    const g = gesture.current;
    leased.current?.lease?.abort();
    leased.current = null;
    if (!g) return;
    letGo(g.pointer);
    gesture.current = null;
    setMarquee(null);
    if (g.kind === "paint" || g.kind === "draw") setDraft(null);
    else if (g.kind !== "empty" && g.result) setDraft(g.base);
    if (grid.current?.hasPointerCapture(g.pointer)) grid.current.releasePointerCapture(g.pointer);
  };
  // Esc while a note is dragged, drawn or painted puts it back, and is the drag's alone (not the page's deselect).
  useEffect(() => {
    const esc = (e: KeyboardEvent) => {
      const g = gesture.current;
      if (e.key !== "Escape" || !g || g.kind === "empty" || g.kind === "velocity" || !g.result) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      cancelGesture();
    };
    window.addEventListener("keydown", esc, true);
    return () => {
      window.removeEventListener("keydown", esc, true);
      leased.current?.lease?.abort();
    };
  }, []);

  /** Beat `b` of the clip on the snap grid, which follows the song's bars; `off` (⌘ held): where it is (NOTE-6). */
  const snapBeat = (b: number, floor = false, off = false) => {
    if (!snap || off) return clean(b);
    const q = (floor ? Math.floor : Math.round)((start + b) / snap + 1e-9) * snap;
    return clean(q - start);
  };

  const at = (e: { clientX: number; clientY: number }) => {
    const r = grid.current!.getBoundingClientRect();
    const x = e.clientX - r.left, y = e.clientY - r.top;
    return { x, y, beat: x / ppb, pitch: pitchOfRow(Math.floor(y / rowH)) };
  };

  const picked = clip ? [...selected].filter((i) => i < clip.notes.length) : [];
  // The selected notes, by what they are, for agents beside the musician (AGENT-6).
  const pickedNotes = clip ? picked.map((i) => ({ pitch: clip.notes[i].pitch, start: clip.notes[i].start, length: clip.notes[i].length })) : [];
  const said = JSON.stringify(pickedNotes);
  useEffect(() => look({ notes: pickedNotes.length ? { clip: { track: module, lane, id: ref.id }, notes: pickedNotes } : null }), [said, module, lane, ref.id]);
  useEffect(() => () => look({ notes: null }), []);

  /**
   * An edit of this clip (a key's, a tool's, the menu's), made from `c`, the clip as the roll draws
   * it, and `from`, what that was made from. While the window doesn't show its own last edit yet (an
   * undo a moment before) and the roll has no write of its own on its way, it is made when it goes
   * instead, from the clip as the window shows it then (the gate, AGENT-2): never from notes an own
   * edit has changed since.
   */
  const fromShown = (make: (c: Shape, from: Basis) => void) => {
    if (!clip || locked) return;
    const g = useStore.getState().graph;
    if (draft || sending.current.busy || (g && showsOwn(g) && g.view.revision === latest.current.view.revision)) return make(clip, basis());
    const own = editing();
    through(latest.current.session, own)
      .then((now) => {
        const l = findLane(now.timeline, module, lane);
        const c = findNoteClip(now.timeline, ref);
        if (c && l?.kind === "notes") make({ length: c.length, notes: c.notes }, { revision: now.view.revision, clips: l.clips });
      })
      .catch((error) => toast(error instanceof Error ? error.message : String(error)))
      .finally(() => own.end());
  };

  /** Delete the notes at `which` (the selected ones). */
  const remove = (which: readonly number[] = picked, name?: string) => {
    if (!clip || !which.length) return;
    const gone = new Set(which);
    fromShown((clip, from) => commit({ length: clip.length, notes: clip.notes.filter((_, i) => !gone.has(i)) }, new Set(), from, name));
  };

  /** Copies of the notes at `which` (the selected ones) right after them; the copies are selected. */
  const duplicate = (which: readonly number[] = picked, name?: string) => {
    if (!clip || !which.length) return false;
    fromShown((clip, from) => {
      const notes = which.flatMap((i) => (clip.notes[i] ? [clip.notes[i]] : []));
      if (!notes.length) return;
      const first = Math.min(...notes.map((n) => n.start));
      const span = Math.max(...notes.map((n) => n.start + n.length)) - first;
      const by = snap ? Math.max(1, Math.ceil(span / snap - 1e-9)) * snap : span;
      const copies = notes.map((n) => ({ ...n, start: clean(n.start + by) }));
      commit({ length: clip.length, notes: [...clip.notes, ...copies] }, new Set(copies.map((_, k) => clip.notes.length + k)), from, name);
    });
    return true;
  };

  /**
   * ⌘C, or with `cut` ⌘X (NOTE-5): the selected notes onto the window's one clipboard, as one notes
   * clip that starts where the first of them does, so the timeline pastes it as a clip too.
   */
  const copy = (cut: boolean) => {
    if (!clip || !picked.length) return;
    const notes = picked.map((i) => clip.notes[i]);
    const first = Math.min(...notes.map((n) => n.start));
    const end = Math.max(...notes.map((n) => n.start + n.length));
    const copied: Copied = {
      module,
      lane,
      clip: { start: clean(start + first), length: clean(Math.max(end - first, MIN_LEN)), notes: notes.map((n) => ({ ...n, start: clean(n.start - first) })) },
    };
    useClipboard.setState({ copied: [copied] });
    if (cut && !locked) remove(picked, `cutting ${trackName()} notes`);
  };

  /**
   * ⌘V (NOTE-5): the clipboard's notes into this clip, keeping their distances, the earliest at the
   * playhead when it is inside the clip (else at its start), on the grid while it snaps, and at the
   * key's notes while Lock to key is on. One Tx; the pasted notes are selected.
   */
  const paste = () => {
    const copied = useClipboard.getState().copied.filter((c) => "notes" in c.clip);
    if (!clip || !copied.length || locked) return;
    const now = playhead() - start;
    const into = now >= 0 && now < clip.length ? now : 0;
    const at = clean(start + Math.max(0, snapBeat(into, true)));
    act((c) => ({ op: "paste", clips: copied, at, into: c, ...(locking ? { fit: true } : {}) }), `pasting ${trackName()} notes`, true);
  };

  /**
   * A note tool (D1) on the selected notes, or on every note when none is selected (Q25): one
   * lane write, one undo step named for it. The selection stays as it was. Swing and the grooves
   * (NOTE-7) are the server's: one clip gesture.
   */
  const runTool = (t: NoteTool) => {
    if (!clip || locked) return;
    const who = `${trackName()} notes`;
    const all = targets(clip.notes, picked);
    const keep = new Set(picked);
    if (t.kind === "delete") return remove(all, `deleting ${who}`);
    if (t.kind === "duplicate") return void duplicate(all, `duplicating ${who}`);
    if (t.kind === "swing" || t.kind === "groove") {
      const groove = t.kind === "swing" ? { name: "swing", amount: t.amount, step: swingUnit(gridStep) === 0.5 ? ("eighths" as const) : ("sixteenths" as const) } : { name: t.name };
      const label = t.kind === "swing" ? "swinging" : `${theory?.grooves.find((g) => g.name === t.name)?.label ?? t.name} on`;
      return act((c) => ({ op: "groove", clips: [c], ...(picked.length ? { notes: picked } : {}), groove }), `${label} ${who}`);
    }
    fromShown((clip, from) => {
      let notes: Note[] | null = null;
      let name = who;
      switch (t.kind) {
        case "transpose":
          // On a folded kit, a row up or down is the next row shown (NOTE-8).
          notes = rows && Math.abs(t.by) === 1 ? rowsMoved(clip.notes, picked, t.by as 1 | -1) : transpose(clip.notes, picked, t.by);
          name = `transposing ${who}`;
          break;
        case "quantize":
          notes = quantize(clip.notes, picked, gridStep, start, timeline.meter);
          name = `quantizing ${who} to ${pickName(gridPick, gridTriplet)}`;
          break;
        case "legato":
          notes = legato(clip.notes, picked);
          name = `making ${who} legato`;
          break;
        case "humanize":
          notes = humanize(clip.notes, picked, (b) => 1 / msOf(1, b));
          name = `humanizing ${who}`;
          break;
      }
      if (notes) commit({ length: clip.length, notes }, keep, from, name);
    });
  };

  /**
   * Move the selected notes by `dp` semitones and `dt` beats, staying on the keyboard and in the
   * clip. With Lock to key, a semitone up or down is the next note of the key instead (NOTE-3).
   */
  const nudge = (dp: number, dt: number) => {
    if (!clip || !picked.length) return;
    const sel = selected;
    fromShown((clip, from) => {
      const notes = picked.flatMap((i) => (clip.notes[i] ? [clip.notes[i]] : []));
      if (!notes.length) return;
      // A step up or down: to the next note of the key with Lock to key on (NOTE-3), the next row
      // shown on a folded kit (NOTE-8).
      const oneStep = Math.abs(dp) === 1 ? (dp as 1 | -1) : 0;
      const p = clamp(dp, -Math.min(...notes.map((n) => n.pitch)), TOP - Math.max(...notes.map((n) => n.pitch)));
      const t = Math.max(dt, -Math.min(...notes.map((n) => n.start)));
      if (!p && !t && !(oneStep && rows)) return;
      const pitch = (q: number) => (oneStep && rows ? rowStep(q, oneStep) : oneStep && locking ? stepIn(keyed!, q, oneStep) : q + p);
      commit({ length: clip.length, notes: clip.notes.map((n, i) => (sel.has(i) ? { ...n, pitch: pitch(n.pitch), start: clean(n.start + t) } : n)) }, sel, from);
    });
  };

  /** Pitch `p` a row up (`dir` 1) or down (-1) on the rows shown; `p` itself at their end. */
  const rowStep = (p: number, dir: 1 | -1) => pitchOfRow(rowOf(p) - dir);
  /** The notes at `picked` (every note with none picked) a row up or down on the rows shown; null when none moves. */
  const rowsMoved = (notes: Note[], picked: number[], dir: 1 | -1): Note[] | null => {
    const which = new Set(targets(notes, picked));
    const out = notes.map((n, i) => (which.has(i) ? { ...n, pitch: rowStep(n.pitch, dir) } : n));
    return out.some((n, i) => n.pitch !== notes[i].pitch) ? out : null;
  };

  /** I: the next inversion of the chord a stamp places (NOTE-4). */
  const invert = () => {
    if (!stamp || !keyed) return;
    const n = keyed.chords[0]?.kinds[stamp.kind]?.shapes.length ?? 1;
    useShell.getState().set({ chord: { kind: stamp.kind, inversion: (stamp.inversion + 1) % n } });
  };

  const setLength = (beats: number) => {
    if (!clip || !(beats > 0) || beats === clip.length) return;
    const sel = selected;
    fromShown((clip, from) => commit({ length: beats, notes: clip.notes }, sel, from));
  };

  // In the desktop app ⌘D comes from the menu bar, through App's "duplicate" command; so does
  // Edit ▸ Quantize, through "quantize" (X2): each is the roll's while it has the keyboard.
  const duplicateRef = useRef(duplicate);
  duplicateRef.current = duplicate;
  const toolRef = useRef(runTool);
  toolRef.current = runTool;
  const zoomRef = useRef(zoom);
  zoomRef.current = zoom;
  const rowsRef = useRef(resizeRows);
  rowsRef.current = resizeRows;
  const clipboardRef = useRef((op: "copy" | "cut" | "paste") => (op === "paste" ? paste() : copy(op === "cut")));
  clipboardRef.current = (op) => (op === "paste" ? paste() : copy(op === "cut"));
  useEffect(() => {
    const focused = () => !!root.current?.contains(document.activeElement);
    const hook = () => focused() && duplicateRef.current();
    const quantizeHook = () => focused() && (toolRef.current({ kind: "quantize" }), true);
    // View ▸ Zoom In and Zoom Out zoom the roll while it has the keyboard (X3); Taller and Shorter
    // Tracks (⌘↓ and ⌘↑) make its rows taller and shorter (NOTE-9).
    const zoomHook = (dir: 1 | -1) => focused() && (zoomRef.current(dir > 0 ? 1.5 : 1 / 1.5), true);
    const rowsHook = (dir: 1 | -1) => focused() && (rowsRef.current(dir), true);
    // ⌘C, ⌘X and ⌘V are the roll's notes while it has the keyboard (NOTE-5).
    const takes = () => focused();
    const clipboard = (op: "copy" | "cut" | "paste") => clipboardRef.current(op);
    editorHooks.duplicate = hook;
    editorHooks.quantize = quantizeHook;
    editorHooks.zoom = zoomHook;
    editorHooks.rows = rowsHook;
    editorHooks.takesClipboard = takes;
    editorHooks.clipboard = clipboard;
    // In the app the menu bar's Cut, Copy and Paste run the web view's commands (M7): the roll
    // takes them while it has the keyboard.
    const listeners = (["copy", "cut", "paste"] as const).flatMap((op) => [
      [`before${op}`, (e: Event) => void (focused() && e.preventDefault())],
      [
        op,
        (e: Event) => {
          if (!focused()) return;
          e.preventDefault();
          clipboard(op);
        },
      ],
    ]) as [string, (e: Event) => void][];
    for (const [type, f] of listeners) document.addEventListener(type, f);
    return () => {
      if (editorHooks.duplicate === hook) delete editorHooks.duplicate;
      if (editorHooks.quantize === quantizeHook) delete editorHooks.quantize;
      if (editorHooks.zoom === zoomHook) delete editorHooks.zoom;
      if (editorHooks.rows === rowsHook) delete editorHooks.rows;
      if (editorHooks.takesClipboard === takes) delete editorHooks.takesClipboard;
      if (editorHooks.clipboard === clipboard) delete editorHooks.clipboard;
      for (const [type, f] of listeners) document.removeEventListener(type, f);
    };
  }, []);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!clip || e.target instanceof HTMLInputElement) return;
    // A key in the roll closes its menu: what the menu counted may change.
    if (menu) setMenu(null);
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
    // Q quantizes the selected notes, or every note with none selected (X28).
    else if (key === "q" && !cmd && !e.altKey && !e.shiftKey) runTool({ kind: "quantize" });
    else if ((e.key === "Delete" || e.key === "Backspace") && !clip.notes.length && !draft) removeClip();
    else if (cmd && key === "d" && !native) duplicate();
    else if (cmd && key === "a") setSelected(new Set(clip.notes.map((_, i) => i)));
    // I: the chord a stamp places, in its next inversion (NOTE-4).
    else if (key === "i" && !cmd && !e.altKey && !e.shiftKey && stamp) invert();
    else if (e.key === "Escape" && picked.length) setSelected(new Set());
    // ⌥← and ⌥→ are never the roll's (X1): they go to the page.
    else if (arrows[e.key] && picked.length && !cmd && !e.altKey) nudge(...arrows[e.key]);
    else handled = false;
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  // ---------------------------------------------------------------- the grid's gestures

  /** Sound `pitches` while pointer `pointer` stays down: a note, or a chord stamp's notes. */
  const sound = (pointer: number, pitches: number[], velocity: number) => {
    hold.current?.h.forEach(release);
    hold.current = { pointer, h: pitches.map((p) => strike(module, lane, p, velocity)) };
  };
  /** Pointer `pointer` let go: so do the notes it held. */
  const letGo = (pointer: number) => {
    if (hold.current?.pointer !== pointer) return;
    hold.current.h.forEach(release);
    hold.current = null;
  };
  /** How long `beats` from beat `from` of the clip last, in ms, at the song's tempo there. */
  const msOf = (beats: number, from: number) => {
    const at = start + from;
    const bpm = timeline.tempo.filter((t) => t.beat <= at + 1e-9).at(-1)?.bpm ?? timeline.tempo[0]?.bpm ?? 120;
    return (beats * 60000) / bpm;
  };

  const onGridDown = (e: React.PointerEvent<HTMLDivElement>) => {
    // A Ctrl-click on a Mac is a right-click: the menu opens on the contextmenu that follows (X7).
    if (e.button !== 0 || (isMac && e.ctrlKey) || !clip || gesture.current) return;
    root.current?.focus({ preventScroll: true });
    e.currentTarget.setPointerCapture(e.pointerId);
    const p = at(e);
    const off = offGrid(e);
    const target = (e.target as HTMLElement).closest<HTMLElement>("[data-note]");
    if (!target) {
      // A double-click's second press sounds the notes it is about to add, while it is held.
      const last = lastEmpty.current;
      const now = performance.now();
      lastEmpty.current = { t: now, x: p.x, y: p.y };
      const adding = !locked && last && now - last.t < DOUBLE_MS && Math.hypot(p.x - last.x, p.y - last.y) < DOUBLE_PX ? notesAt(p, clip.length, off) : [];
      if (!draw && adding.length) {
        lastEmpty.current = null;
        sound(e.pointerId, adding.map((n) => n.pitch), VELOCITY);
      }
      // Draw mode (B), as in GarageBand: a press adds a note there (or a chord stamp's notes),
      // sounding while held, and a drag sets their length; an ⌥-drag paints one in each cell it crosses.
      if (draw && !locked) {
        if (e.altKey) {
          const g: Gesture = { kind: "paint", pointer: e.pointerId, base: clip, from: basis(), cells: new Set(), result: clip };
          gesture.current = g;
          paint(g, p, off);
          return;
        }
        const notes = notesAt(p, clip.length, off);
        if (!notes.length) return;
        const g: Gesture = { kind: "draw", pointer: e.pointerId, x0: p.x, base: clip, from: basis(), moved: false, result: { length: clip.length, notes: [...clip.notes, ...notes] } };
        gesture.current = g;
        setHover(null);
        setDraft(g.result);
        hear(g, g.result);
        sound(e.pointerId, notes.map((n) => n.pitch), notes[0].velocity);
        return;
      }
      gesture.current = { kind: "empty", pointer: e.pointerId, x0: p.x, y0: p.y, keep: e.shiftKey ? selected : new Set(), moved: false };
      return;
    }
    const i = Number(target.dataset.note);
    // The note sounds while it is held, at its own velocity: read only too.
    sound(e.pointerId, [clip.notes[i].pitch], clip.notes[i].velocity);
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
    // With a chord picked, the pointer shows where its stamp would land.
    if (!g && stamp && !locked && clip) {
      const p = at(e);
      const h = { beat: p.beat, pitch: p.pitch, off: offGrid(e) };
      if (!(e.target as HTMLElement).closest("[data-note]")) setHover(h);
      else setHover(null);
    }
    if (!g || g.pointer !== e.pointerId || g.kind === "velocity" || !clip) return;
    const p = at(e);
    const off = offGrid(e);
    if (g.kind === "paint") return paint(g, p, off);
    if (g.kind === "draw") return stretch(g, p, off);
    if (g.kind === "empty") {
      if (!g.moved && Math.hypot(p.x - g.x0, p.y - g.y0) < 4) return;
      g.moved = true;
      const box = { x0: Math.min(g.x0, p.x), y0: Math.min(g.y0, p.y), x1: Math.max(g.x0, p.x), y1: Math.max(g.y0, p.y) };
      setMarquee(box);
      const hit = new Set(g.keep);
      clip.notes.forEach((n, i) => {
        const x = n.start * ppb, y = rowOf(n.pitch) * rowH;
        if (y >= 0 && x + Math.max(n.length * ppb, 4) > box.x0 && x < box.x1 && y + rowH > box.y0 && y < box.y1) hit.add(i);
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
      const dt = Math.max(snapBeat(n0.start + raw, false, off) - n0.start, -Math.min(...moving.map((n) => n.start)));
      // Rows moved, so that every moving note stays on a row shown (all 128, or a folded kit's).
      const at0 = moving.map((n) => rowOf(n.pitch));
      const by = clamp(Math.round((g.y0 - p.y) / rowH), Math.max(...at0) - (nRows - 1), Math.min(...at0));
      // With Lock to key the grabbed note lands on the key's note nearest the row under the
      // pointer, and the others move as far, each then to its nearest note of the key (NOTE-3).
      const dp = keyed && locking ? fit(keyed, n0.pitch + by) - n0.pitch : 0;
      const pitch = (q: number) => (keyed && locking ? fit(keyed, q + dp) : pitchOfRow(rowOf(q) - by));
      notes = base.map((n, i) => (g.sel.has(i) ? { ...n, start: clean(n.start + dt), pitch: pitch(n.pitch) } : n));
      // The held note plays each pitch it crosses: an Off for the old, an On for the new.
      const held = hold.current;
      if (held?.pointer === e.pointerId && held.h.length === 1) held.h = [retune(held.h[0], pitch(n0.pitch), n0.velocity)];
    } else {
      const min = snap && !off ? snap : MIN_LEN;
      const dl = Math.max(snapBeat(n0.start + n0.length + raw, false, off) - n0.start, min) - n0.length;
      notes = base.map((n, i) => (g.sel.has(i) ? { ...n, length: clean(Math.max(n.length + dl, min)) } : n));
    }
    g.result = { length: g.base.length, notes };
    setDraft(g.result);
    hear(g, g.result);
  };

  const onGridUp = (e: React.PointerEvent<HTMLDivElement>) => {
    letGo(e.pointerId);
    const g = gesture.current;
    if (!g || g.pointer !== e.pointerId || g.kind === "velocity" || !clip) return;
    gesture.current = null;
    setMarquee(null);
    const lease = taken(g);
    if (g.kind === "paint" || g.kind === "draw") {
      if (same(g.result, g.base)) {
        lease?.abort();
        return setDraft(null);
      }
      const added = new Set(g.result.notes.map((_, i) => i).filter((i) => i >= g.base.notes.length));
      return commit(g.result, added, g.from, undefined, lease);
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
    if (same(g.result, g.base)) {
      lease?.abort();
      return setDraft(g.base);
    }
    commit(g.result, g.sel, g.from, undefined, lease);
  };

  /** A pointer lost mid-gesture puts the clip back as it was. */
  const onGridCancel = (e: React.PointerEvent<HTMLDivElement>) => {
    letGo(e.pointerId);
    const g = gesture.current;
    if (!g || g.pointer !== e.pointerId || g.kind === "velocity") return;
    taken(g)?.abort();
    gesture.current = null;
    setMarquee(null);
    if (g.kind === "paint" || g.kind === "draw") setDraft(null);
    else if (g.kind !== "empty" && g.result) setDraft(g.base);
  };

  /**
   * A note in the cell at `p`, a grid step long (as in Live), if the cell is inside the clip; with
   * `off` (⌘ held) where the pointer is, not on the grid (NOTE-6). With Lock to key its pitch is the
   * key's nearest note (NOTE-3).
   */
  const noteAt = (p: { beat: number; pitch: number }, length: number, off = false): Note | null => {
    const from = Math.max(0, snapBeat(p.beat, true, off));
    if (from >= length) return null;
    const pitch = keyed && locking ? fit(keyed, p.pitch) : p.pitch;
    return { pitch, start: from, length: clean(Math.min(step, length - from)), velocity: VELOCITY };
  };

  /**
   * What a press or a double-click at `p` adds: the chord picked (NOTE-4), built on the key's note
   * nearest the row, or one note. None outside the clip, or where the key has no such chord.
   */
  const notesAt = (p: { beat: number; pitch: number }, length: number, off = false): Note[] => {
    const one = noteAt(p, length, off);
    if (!one || !stamp || !keyed) return one ? [one] : [];
    const c = chordAt(keyed, p.pitch, stamp.kind, stamp.inversion);
    return "pitches" in c ? c.pitches.map((pitch) => ({ ...one, pitch })) : [];
  };

  /** A double-click on empty grid: a note in that cell, or a chord stamp's notes; they are selected. */
  const addNote = (p: { beat: number; pitch: number }, off = false) => {
    const notes = clip ? notesAt(p, clip.length, off) : [];
    if (!clip || !notes.length) return;
    commit({ length: clip.length, notes: [...clip.notes, ...notes] }, new Set(notes.map((_, k) => clip.notes.length + k)));
  };

  /**
   * Draw mode: the notes being drawn (one, or a chord's) end where the pointer is, past a 3 px drag
   * (on the grid when it snaps, unless ⌘ is held), never shorter than a grid step (or the smallest
   * length unsnapped). Their pitches stay.
   */
  const stretch = (g: Extract<Gesture, { kind: "draw" }>, p: { x: number; beat: number }, off: boolean) => {
    if (!g.moved && Math.abs(p.x - g.x0) < 3) return;
    g.moved = true;
    const drawn = g.result.notes.slice(g.base.notes.length);
    const n0 = drawn[0];
    const length = clean(Math.max(snapBeat(p.beat, false, off) - n0.start, snap && !off ? snap : MIN_LEN));
    if (length === n0.length) return;
    g.result = { length: g.base.length, notes: [...g.base.notes, ...drawn.map((n) => ({ ...n, length }))] };
    setDraft(g.result);
    hear(g, g.result);
  };

  /** Draw mode's ⌥-drag: a note in each new cell under the pointer that has none starting there; written as one edit on release. */
  const paint = (g: Extract<Gesture, { kind: "paint" }>, p: { beat: number; pitch: number }, off = false) => {
    const note = noteAt(p, g.base.length, off);
    const cell = note && `${note.start}:${note.pitch}`;
    if (!note || !cell || g.cells.has(cell) || g.result.notes.some((n) => n.start === note.start && n.pitch === note.pitch)) return;
    g.cells.add(cell);
    g.result = { length: g.base.length, notes: [...g.result.notes, note] };
    setDraft(g.result);
    hear(g, g.result);
    // Each painted note sounds for its length, or a moment if that is shorter.
    pulse(module, lane, note.pitch, note.velocity, Math.min(msOf(note.length, note.start), PAINT_MS));
  };

  /**
   * A right-click on the grid (D1): on a note, that note is selected if it wasn't (alone); on
   * empty grid the selection stays. The menu acts on the selection, or on every note.
   */
  const onGridMenu = (e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault();
    if (!clip || gesture.current) return;
    root.current?.focus({ preventScroll: true });
    const target = (e.target as HTMLElement).closest<HTMLElement>("[data-note]");
    if (target) {
      const i = Number(target.dataset.note);
      if (!selected.has(i)) setSelected(new Set([i]));
    }
    setMenu({ x: e.clientX, y: e.clientY });
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
  const height = nRows * rowH;
  const shown = new Set(picked);
  const track = graph.view.modules.find((m) => m.key === module);
  const title = track ? moduleName(track) : module;
  const laneName = declared ? laneTitle(declared) : lane;
  // The view is Notes already: its lane is worth naming only beside the track's other notes lanes.
  const lanes = timeline.tracks.find((t) => t.module === module)?.lanes.filter((l) => l.kind === "notes").length ?? 0;
  const firstBar = barAt(timeline.meter, start).bar + 1;
  const lastBar = barAt(timeline.meter, start + clip.length - 1e-6).bar + 1;
  // Notes being dragged lift off the grid.
  const lifting = gesture.current?.kind === "move" && !!gesture.current.result;
  // The rows shaded: those out of the song's key while one is set (Q24), else the black keys'. A
  // lane whose rows are named drums is never shaded by the key: a kit has no key (X26).
  const keyNotes = keyed ? inKey(keyed) : null;
  // A kit's rows are drums, not keys: none shaded (next D4c).
  const shaded = (keyNotes ? Array.from({ length: 12 }, (_, pc) => pc).filter((pc) => !keyNotes.has(pc)) : drums ? [] : BLACK).join(",");
  // Where a stamp would land under the pointer (NOTE-4): its chord's name and notes, outlined; or
  // why there is none there, in the server's words (not in the key, the blues' blue note), or that
  // it would be past the clip's end.
  const ghost = hover && stamp && keyed && !gesture.current ? notesAt(hover, clip.length, hover.off) : [];
  const ghostChord = hover && stamp && keyed && !gesture.current ? chordAt(keyed, hover.pitch, stamp.kind, stamp.inversion) : null;
  const stampSays = !ghostChord
    ? null
    : !("pitches" in ghostChord)
      ? ghostChord.why
      : ghost.length
        ? `${ghostChord.name} · ${ghostChord.numeral}`
        : "A chord goes inside the clip, before its end";

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
              <Segment role="radio" aria-checked={draw} data-tool="draw" on={draw} title={`Draw (B): click adds a note, drag sets its length, ${mod.alt}-drag paints`} onClick={() => useShell.getState().set({ draw: true })} className="!px-1.5">
                <Pencil size={14} />
              </Segment>
            </div>
            <Divider />
            {/* Grid [1/16 ▾] and the snap toggle (taste.md §3.5). */}
            <GridChip view="notes" />
            <Divider />
            {/* With the song's key: Lock to key and the chord a stamp places (NOTE-3, NOTE-4); on a kit's lane, Fold (NOTE-8). */}
            {keyed && (
              <>
                <Button
                  size="sm"
                  data-testid="lock-key"
                  active={locking}
                  aria-pressed={locking}
                  onMouseDown={keepFocus}
                  onClick={() => setRoll({ lock: !locking })}
                  title={`Lock to key (${keyed.name}): ${locking ? "on" : "off"}. While it is on, notes you draw, drag or paste go to the nearest note of the key, and ↑ ↓ move to the next one`}
                  className="!px-1.5"
                >
                  Lock to key
                </Button>
                <ChordTool keyed={keyed} />
                <Divider />
              </>
            )}
            {drums && (
              <>
                <Button
                  size="sm"
                  data-testid="fold-rows"
                  active={folded}
                  aria-pressed={folded}
                  onMouseDown={keepFocus}
                  onClick={() => {
                    // Folding again folds to what the clip uses now.
                    if (!folded) setFoldFrom(usedRows(names, server.notes));
                    setRoll({ fold: !folded });
                  }}
                  title={folded ? "Fold: on. Only the drums this clip uses show; click to show every row" : "Fold: off. Every row shows; click to show only the drums this clip uses"}
                  className="!px-1.5 !gap-1"
                >
                  {folded ? <UnfoldVertical size={14} /> : <FoldVertical size={14} />}
                  Fold
                </Button>
                <Divider />
              </>
            )}
          </>
        )}
        <LengthField
          bars={Number(beatsToBars(timeline.meter, start, clip.length).toFixed(3))}
          toBeats={(n) => clean(barsToBeats(timeline.meter, start, n))}
          disabled={locked}
          onChange={setLength}
        />
        <Divider />
        <AuditionTools graph={graph} module={module} lane={lane} />
        <Divider />
        <div className="shrink-0 flex items-center">
          <Button size="sm" onMouseDown={keepFocus} onClick={() => zoom(0.8)} title={`Zoom out (${mod.cmd} + scroll). Rows taller or shorter: ${chord("↓")} ${chord("↑")}`} aria-label="Zoom out" className="w-ctl-sm !px-0">
            <ZoomOut size={16} />
          </Button>
          <Button size="sm" onMouseDown={keepFocus} onClick={() => zoom(1.25)} title={`Zoom in (${mod.cmd} + scroll). Rows taller or shorter: ${chord("↓")} ${chord("↑")}`} aria-label="Zoom in" className="w-ctl-sm !px-0">
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
          <div className="relative" style={{ width: keysW + width, height: ROLL_RULER_H + height }}>
            <div className="sticky top-0 z-30 flex" style={{ height: ROLL_RULER_H }}>
              <div className="sticky left-0 z-40 shrink-0 bg-chrome border-r border-b border-line" style={{ width: keysW }} />
              <Ruler width={width} ppb={ppb} meter={timeline.meter} start={start} length={clip.length} tone={tone} />
            </div>
            <div className="flex">
              <Keys selected={new Set(picked.map((i) => clip.notes[i].pitch))} tone={tone} module={module} lane={lane} names={names} width={keysW} rows={rows} rowH={rowH} />
              <div
                ref={grid}
                data-testid="roll-grid"
                data-shaded={shaded}
                data-rows={rows ? rows.join(",") : undefined}
                data-row-height={rowH}
                data-ppb={ppb}
                className={clsx("relative touch-none", locked ? "cursor-default" : draw || stamp ? "cursor-crosshair" : "cursor-default")}
                style={{ width, height }}
                title={
                  locked
                    ? why === "lane"
                      ? `Read only until ${file} reads again`
                      : "Read only until the code compiles"
                    : ghostChord && stampSays
                      ? "pitches" in ghostChord && ghost.length
                        ? `${draw ? "Click" : "Double-click"} to place ${ghostChord.name} (${ghostChord.numeral} in ${keyed?.name}) · I for the next inversion · hold ${mod.cmd} to place it off the grid`
                        : stampSays
                      : draw
                        ? `Draw: click to add a note · drag to set its length · ${mod.alt}-drag to paint notes · hold ${mod.cmd} to place off the grid · B to select`
                        : `Double-click to add a note · drag a note to move it (hold ${mod.cmd} to place it off the grid) · drag its right edge to resize · drag across empty space to select · ${keyName("Backspace")} deletes · ${chord("D")} duplicates · ${chord("C")} copies · ${chord("V")} pastes · arrows move · B to draw · right-click for note tools`
                }
                onContextMenu={onGridMenu}
                onDoubleClick={(e) => {
                  // Select mode adds notes with a double-click; Draw mode added one on the first click.
                  if (locked || draw || (e.target as HTMLElement).closest("[data-note]")) return;
                  addNote(at(e), offGrid(e));
                }}
                onPointerDown={onGridDown}
                onPointerMove={onGridMove}
                onPointerLeave={() => setHover(null)}
                onPointerUp={onGridUp}
                onPointerCancel={onGridCancel}
                onLostPointerCapture={onGridCancel}
              >
                <Lanes width={width} ppb={ppb} meter={timeline.meter} start={start} step={gridStep} shaded={shaded} rowH={rowH} folded={!!rows} />
                {/* Past the clip's end: hatched, behind its edge in the track's colour. */}
                <div className="absolute top-0 bottom-0 right-0 pointer-events-none border-l-2 bg-chrome/70" style={{ left: clip.length * ppb, borderColor: tone.bar }}>
                  <Hatch />
                </div>
                {clip.notes.map((n, i) => {
                  const row = rowOf(n.pitch);
                  if (row < 0) return null;
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
                      title={`${rowName(declared, n.pitch)} · beat ${Number((n.start + 1).toFixed(3))} · ${noteValue(n.length)} · velocity ${Math.round(n.velocity * 100)}%`}
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
                          top: row * rowH + 1,
                          width: w,
                          height: rowH - 2,
                          background: on ? tone.ink : locked ? "var(--color-line-strong)" : fill(tone, n.velocity),
                          "--note-ink": tone.ink,
                        } as React.CSSProperties
                      }
                    >
                      {/* One line inside the note, cut short with "…" when the note is short; none in a row too short for it. */}
                      {w > 26 && rowH >= 12 && (
                        <span className="absolute left-1.5 right-1 top-1/2 -translate-y-1/2 whitespace-nowrap overflow-x-clip text-ellipsis text-xs font-medium leading-none pointer-events-none">
                          {rowName(declared, n.pitch)}
                        </span>
                      )}
                      {/* The edge to resize by: never more than half a short note, which must stay draggable. */}
                      <div data-edge className="absolute -right-0.5 inset-y-0 w-[min(7px,calc(50%+2px))] cursor-ew-resize rounded-r-xs hover:bg-white/30" />
                    </div>
                  );
                })}
                {/* Where a stamp would land: its notes outlined, its chord named by the first (NOTE-4). */}
                {ghost.map((n, k) => (
                  <div
                    key={`ghost${k}`}
                    data-ghost={n.pitch}
                    className="absolute z-10 pointer-events-none rounded-xs border border-dashed"
                    style={{ left: n.start * ppb, top: rowOf(n.pitch) * rowH + 1, width: Math.max(n.length * ppb - 1, 4), height: rowH - 2, borderColor: tone.ink }}
                  />
                ))}
                {hover && stampSays && (
                  <div
                    data-testid="stamp-name"
                    data-placed={ghost.length > 0 || undefined}
                    className={clsx(
                      "absolute z-20 pointer-events-none px-1 rounded-xs text-xs font-medium leading-4 whitespace-nowrap",
                      ghost.length ? "text-white" : "bg-raised text-fg-muted shadow-pop",
                    )}
                    style={{
                      left: (ghost[0]?.start ?? hover.beat) * ppb,
                      top: Math.max(0, (ghost.length ? rowOf(Math.max(...ghost.map((n) => n.pitch))) : rowOf(hover.pitch)) * rowH - 18),
                      background: ghost.length ? tone.ink : undefined,
                    }}
                  >
                    {stampSays}
                  </div>
                )}
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
          <EmptyState className="absolute top-1/2 -translate-x-1/2 -translate-y-1/2 pointer-events-none whitespace-nowrap" style={{ left: keysW + Math.min(clip.length * ppb, visible) / 2 }}>
            {stamp ? (draw ? "Click in the grid to place a chord" : "Double-click in the grid to place a chord") : draw ? "Click in the grid to add a note" : "Double-click in the grid to add a note"}
          </EmptyState>
        )}
      </div>

      {menu && (
        <NoteMenu
          at={menu}
          count={picked.length || clip.notes.length}
          all={!picked.length}
          grid={pickName(gridPick, gridTriplet)}
          off={locked ? lockedTitle : null}
          named={drums}
          onRun={runTool}
          onClose={() => setMenu(null)}
        />
      )}

      <div data-testid="velocity" className="shrink-0 flex border-t border-line bg-chrome" style={{ height: VEL_H }}>
        <div className="shrink-0 flex flex-col justify-center pl-2 border-r border-line text-xs font-medium text-fg-muted" style={{ width: keysW }}>
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
                    title={`${rowName(declared, n.pitch)}: velocity ${Math.round(n.velocity * 100)}% (drag up or down)`}
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
 * `shape`, made from clip `was`, made again on clip `now` (the clip as the window's own edits left it
 * since: an undo a moment before): the notes `shape` took out of `was` come out of `now`, and those
 * it put in go in, its length with them when it changed it. Null when a note it took out isn't in
 * `now` (the window's own edit changed that very note): it can't be made again.
 */
function merged(was: { length: number; notes: Note[] }, shape: Shape, now: { length: number; notes: Note[] }): Shape | null {
  if (sameNotes(was.notes, now.notes) && was.length === now.length) return shape;
  const key = (n: Note) => [n.pitch, n.start, n.length, n.velocity].join(",");
  const count = (notes: readonly Note[]) => notes.reduce((m, n) => m.set(key(n), (m.get(key(n)) ?? 0) + 1), new Map<string, number>());
  const before = count(was.notes), after = count(shape.notes), have = count(now.notes);
  // What the edit took out of `was`, and what it put in: as many of each note as it changed.
  const out = new Map<string, number>(), put = new Map<string, number>();
  for (const k of new Set([...before.keys(), ...after.keys()])) {
    const d = (after.get(k) ?? 0) - (before.get(k) ?? 0);
    if (d < 0) out.set(k, -d);
    else if (d > 0) put.set(k, d);
  }
  for (const [k, n] of out) if ((have.get(k) ?? 0) < n) return null;
  const notes: Note[] = [];
  for (const n of now.notes) {
    const left = out.get(key(n)) ?? 0;
    if (left > 0) out.set(key(n), left - 1);
    else notes.push(n);
  }
  for (const n of shape.notes) {
    const left = put.get(key(n)) ?? 0;
    if (left > 0) {
      put.set(key(n), left - 1);
      notes.push(n);
    }
  }
  return { length: shape.length !== was.length ? shape.length : now.length, notes: notes.sort((a, b) => a.start - b.start || a.pitch - b.pitch) };
}

/** Whether two clips' notes are the same notes, in the same places. */
function sameNotes(a: readonly Note[], b: readonly Note[]) {
  const key = (n: Note) => [n.pitch, n.start, n.length, n.velocity].join(",");
  return a.length === b.length && a.every((n, i) => key(n) === key(b[i]));
}

/** What a folded kit's roll starts from (NOTE-8): the drums `notes` use; the kit's named rows when they use none. */
function usedRows(names: Record<string, string>, notes: readonly Note[]): number[] {
  return notes.length ? [...new Set(notes.map((n) => n.pitch))] : Object.keys(names).map(Number);
}

/** The places in `after` of the notes `before` hasn't (as many of each as it adds): what a paste added. */
function addedTo(before: readonly Note[], after: readonly Note[]): number[] {
  const left = new Map<string, number>();
  const id = (n: Note) => JSON.stringify([n.pitch, n.start, n.length, n.velocity]);
  for (const n of before) left.set(id(n), (left.get(id(n)) ?? 0) + 1);
  return after.flatMap((n, i) => {
    const k = left.get(id(n)) ?? 0;
    if (k > 0) {
      left.set(id(n), k - 1);
      return [];
    }
    return [i];
  });
}

/** An inversion's name: Root, 1st, 2nd, 3rd. */
const INVERSIONS = ["Root", "1st", "2nd", "3rd"];

/**
 * The chord a stamp places (NOTE-4): `Chord ▾` while the roll adds single notes, the chord's kind
 * (and inversion) once one is picked. Its menu lists single notes and the kinds of the server's
 * table, each saying what it is, and the inversions (I steps through them). The window's choice,
 * as Draw mode is.
 */
function ChordTool({ keyed }: { keyed: KeyView }) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const tool = useShell((s) => s.chord);
  const kinds = useTheory((t) => t.theory?.chords);
  const none = keyed.chords.length === 0;
  const set = (chord: { kind: string; inversion: number } | null) => useShell.getState().set({ chord });
  const kind = kinds?.find((k) => k.name === tool?.kind);
  const tick = (on: boolean) => (on ? <Check size={14} className="text-fg" /> : <span className="block size-3.5" />);
  const label = tool && kind ? `${kind.label}${tool.inversion ? `, ${INVERSIONS[tool.inversion]} inversion` : ""}` : "Chord";
  return (
    <>
      <Button
        size="sm"
        data-testid="chord-tool"
        data-chord={tool && !none ? tool.kind : undefined}
        aria-haspopup="menu"
        aria-expanded={!!anchor}
        aria-disabled={none}
        active={!!tool && !none}
        onMouseDown={keepFocus}
        onClick={(e) => {
          if (none) return;
          const b = e.currentTarget;
          setAnchor((a) => (a ? null : b));
        }}
        title={
          none
            ? `${keyed.name} has no chords to stamp: pick a key with a scale of seven notes`
            : `Chord stamps: pick a chord, then a click in Draw (or a double-click) places it, built on ${keyed.name}'s notes so it fits`
        }
        className={clsx("!px-1.5 !gap-1", none ? "text-fg-off cursor-default" : "text-fg", anchor && "bg-hover")}
      >
        {label}
        <ChevronDown size={12} className="text-fg-muted" />
      </Button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} role="menu" aria-label="Chord stamp" data-testid="chord-menu" className="w-[264px]">
          <MenuRow role="menuitemradio" data-chord="none" aria-checked={!tool} icon={tick(!tool)} onClick={() => (setAnchor(null), set(null))} detail="A click adds one note">
            Single notes
          </MenuRow>
          <div role="separator" className="my-1 h-px bg-line" />
          {(kinds ?? []).map((k) => (
            <MenuRow
              key={k.name}
              role="menuitemradio"
              data-chord={k.name}
              aria-checked={tool?.kind === k.name}
              icon={tick(tool?.kind === k.name)}
              detail={k.words}
              onClick={() => {
                setAnchor(null);
                set({ kind: k.name, inversion: tool?.kind === k.name ? tool.inversion : 0 });
              }}
            >
              {k.label}
            </MenuRow>
          ))}
          {tool && kind && (
            <>
              <div role="separator" className="my-1 h-px bg-line" />
              <div role="group" aria-label="Inversion" title="An inversion moves the lowest notes up an octave (I)" className="min-h-ctl pl-2 pr-1 flex items-center gap-2 text-sm">
                <span className="flex-1 text-fg">Inversion</span>
                <div className="h-ctl-sm flex items-center gap-0.5 p-0.5 rounded-sm bg-hover">
                  {INVERSIONS.slice(0, kind.inversions).map((name, i) => (
                    <Segment key={name} role="menuitemradio" aria-checked={tool.inversion === i} data-inversion={i} on={tool.inversion === i} onClick={() => set({ kind: tool.kind, inversion: i })} className="!px-1.5">
                      {name}
                    </Segment>
                  ))}
                </div>
              </div>
            </>
          )}
        </Popover>
      )}
    </>
  );
}

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
        <button aria-label="One bar shorter" title="One bar shorter" disabled={disabled || value <= 1} onClick={() => onChange(toBeats(Math.ceil(value - 1 - 1e-9) || 1))} className="h-full w-6 grid place-items-center text-fg-muted enabled:hover:text-fg disabled:text-fg-off cursor-pointer">
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
        <button aria-label="One bar longer" title="One bar longer" disabled={disabled} onClick={() => onChange(toBeats(Math.floor(value + 1 + 1e-9)))} className="h-full w-6 grid place-items-center text-fg-muted enabled:hover:text-fg disabled:text-fg-off cursor-pointer">
          <Plus size={12} />
        </button>
      </div>
      <span className="text-xs text-fg-muted @max-[900px]/dock:hidden">{value === 1 ? "bar" : "bars"}</span>
    </div>
  );
}

/**
 * The piano keys, one row per pitch, C's named; keys of selected notes lit in the track's colour. On
 * a lane whose notes have names (a drum kit, polish A4), each named row says its name and no C
 * does, and every row is drawn alike, a line under each: no black keys (next D4c).
 * The keyboard is an instrument (NT-3): a key sounds on lane `module.lane` while held, at
 * `KEY_VELOCITY`, and a drag along it plays each key it reaches. A key that sounds, from a key or
 * a note held anywhere in the roll, is drawn in `press`, never like a selected note's. `rows`: the
 * pitches shown, top to bottom, on a folded kit's lane (NOTE-8); null for all 128. Each row is
 * `rowH` px (NOTE-9).
 */
const Keys = memo(
  function Keys({
    selected,
    tone,
    module,
    lane,
    names,
    width,
    rows,
    rowH,
  }: {
    selected: Set<number>;
    tone: TrackColor;
    module: string;
    lane: string;
    /** The lane's note names, by MIDI note. */
    names: Record<string, string>;
    width: number;
    rows: number[] | null;
    rowH: number;
  }) {
    const drums = Object.keys(names).length > 0;
    const held = useAudition((s) => s.held);
    const sounding = new Set([...held.values()].filter((n) => n.module === module && n.lane === lane).map((n) => n.pitch));
    const press = useRef<{ pointer: number; h: number | null; pitch: number } | null>(null);
    const shown = rows ?? Array.from({ length: TOP + 1 }, (_, k) => TOP - k);
    const pitchAt = (e: React.PointerEvent<HTMLDivElement>) => shown[clamp(Math.floor((e.clientY - e.currentTarget.getBoundingClientRect().top) / rowH), 0, shown.length - 1)];
    const up = (e: React.PointerEvent<HTMLDivElement>) => {
      if (press.current?.pointer !== e.pointerId) return;
      release(press.current.h);
      press.current = null;
    };
    return (
      <div
        data-testid="roll-keys"
        className="sticky left-0 z-20 shrink-0 bg-raised border-r border-line touch-none select-none cursor-pointer"
        style={{ width }}
        title="Click or drag along the keys to hear them"
        onPointerDown={(e) => {
          if (e.button !== 0 || press.current) return;
          e.currentTarget.setPointerCapture(e.pointerId);
          const pitch = pitchAt(e);
          press.current = { pointer: e.pointerId, h: strike(module, lane, pitch, KEY_VELOCITY), pitch };
        }}
        onPointerMove={(e) => {
          const p = press.current;
          if (p?.pointer !== e.pointerId || p.h === null) return;
          const pitch = pitchAt(e);
          if (pitch === p.pitch) return;
          p.pitch = pitch;
          // Let go elsewhere (⌘., a blur): the drag plays no more until the next press.
          p.h = retune(p.h, pitch, KEY_VELOCITY);
        }}
        onPointerUp={up}
        onPointerCancel={up}
        onLostPointerCapture={up}
      >
        {shown.map((p) => {
          const black = !drums && BLACK.includes(p % 12);
          const on = sounding.has(p);
          const lit = !on && selected.has(p);
          return (
            <div
              key={p}
              data-key={p}
              data-sounding={on || undefined}
              className={clsx(
                "relative",
                drums ? "border-b border-line" : p % 12 === 0 ? "border-b border-line-strong" : !black && !BLACK.includes((p + 11) % 12) && "border-b border-line",
              )}
              style={{ height: rowH }}
            >
              {on ? <div className="absolute inset-0 bg-press" /> : lit && <div className="absolute inset-0" style={{ background: tone.bar }} />}
              {/* The edge between the two white keys either side of a black one, under it: every white key is a key. */}
              {black && <div className="absolute inset-x-0 top-1/2 h-px bg-line" />}
              {black && (
                <div
                  data-black
                  className="absolute left-0 inset-y-[1px] rounded-r-xs w-[60%]"
                  style={{ background: on ? "var(--color-press)" : lit ? tone.ink : "var(--color-gray-700)" }}
                />
              )}
              {drums
                ? names[p] && (
                    <span data-name className={clsx("absolute right-1 top-1/2 -translate-y-1/2 text-xs leading-none whitespace-nowrap", lit || on ? "font-medium text-fg-strong" : "text-fg-muted")}>
                      {names[p]}
                    </span>
                  )
                : p % 12 === 0 && (
                    // On a lit key, the strongest grey: the bar tones are light, so it reads.
                    <span className={clsx("absolute right-1.5 top-1/2 -translate-y-1/2 text-xs leading-none", lit || on ? "font-medium text-fg-strong" : p === 60 ? "font-medium text-fg-muted" : "text-fg-faint")}>
                      {pitchName(p)}
                    </span>
                  )}
            </div>
          );
        })}
      </div>
    );
  },
  (a, b) =>
    a.tone === b.tone &&
    a.module === b.module &&
    a.lane === b.lane &&
    a.width === b.width &&
    a.names === b.names &&
    a.rowH === b.rowH &&
    (a.rows ?? []).join() === (b.rows ?? []).join() &&
    a.selected.size === b.selected.size &&
    [...a.selected].every((p) => b.selected.has(p)),
);

/**
 * Row shading (`shaded`: the pitch classes darker, "1,3,6,8,10" for the black keys, or those out of
 * the song's key), a line under each C, and the bar, beat and snap lines, bars as the song's time
 * signatures count them. Rows are `rowH` px; `folded`: a kit's rows, a line under each, no C's.
 */
const Lanes = memo(function Lanes({
  width,
  ppb,
  meter,
  start,
  step,
  shaded,
  rowH,
  folded,
}: {
  width: number;
  ppb: number;
  meter: Meter[];
  start: number;
  step: number;
  shaded: string;
  rowH: number;
  folded: boolean;
}) {
  const dark = new Set(shaded ? shaded.split(",").map(Number) : []);
  // From the top: pitch 127 (G9) down, twelve rows a period: the shaded rows darker, a line under
  // each. A folded kit's rows aren't pitches in order: one row a period, a line under it.
  const bands = folded
    ? [{ y: rowH - 1, h: 1, color: "var(--color-hover)" }]
    : Array.from({ length: 12 }, (_, k) => {
        const pc = (TOP - k) % 12;
        return [
          ...(dark.has(pc) ? [{ y: k * rowH, h: rowH, color: "var(--color-hover)" }] : []),
          { y: (k + 1) * rowH - 1, h: 1, color: pc === 0 ? "var(--color-line)" : "var(--color-hover)" },
        ];
      }).flat();
  /** Where the first of the lines every `period` beats from `origin` falls, in pixels from `from`. */
  const offset = (origin: number, from: number, period: number) => (((((origin - from) % period) + period) % period) * ppb);
  return (
    <>
      <HBands period={(folded ? 1 : 12) * rowH} bands={bands} />
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

/**
 * Bar numbers of the song from beat `start`, and a click to play from there; with a clip's `length`
 * and its track's `tone`, the clip's extent in that colour and its End. The beat grid's ruler too.
 */
export const Ruler = memo(function Ruler({
  width,
  ppb,
  meter,
  start,
  length,
  tone,
}: {
  width: number;
  ppb: number;
  meter: Meter[];
  start: number;
  length?: number;
  tone?: TrackColor;
}) {
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
      {length !== undefined && tone && <div className="absolute bottom-0 left-0 h-[3px] rounded-r-xs" style={{ width: length * ppb, background: tone.bar }} />}
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
      {length !== undefined && tone && (
        <div
          className="absolute bottom-[5px] -translate-x-full pointer-events-none text-xs leading-none font-medium px-1 py-px rounded-xs text-white"
          style={{ left: length * ppb - 3, background: tone.ink }}
        >
          End
        </div>
      )}
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
export function PlayLine({ start, ppb, width }: { start: number; ppb: number; width: number }) {
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

export function PlayCap({ start, ppb }: { start: number; ppb: number }) {
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
