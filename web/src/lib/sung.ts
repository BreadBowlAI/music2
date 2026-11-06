// A voice's notes (VOX-40 to VOX-47): the clip's `sung` notes, the measure the window draws the
// pitch line from, Find notes, and the roll's edits worked out here and sent as the clip written
// whole (`set`): one Tx and one undo step each.

import { useEffect } from "react";
import { create } from "zustand";
import { findLane, type AudioClip, type ClipRef, type GraphState, type TimelineLane } from "./graph";
import { send, serverRef, type Gesture } from "./clips";
import { editing, goesFrom, lanesOf } from "./history";
import { beatAt, secondsAt, type Tempo } from "./peaks";
import { request } from "./projects";
import { useShell } from "./shell";
import { useStore } from "./store";

/** One sung note (VOX-40): where it is in the file (seconds), its pitch as sung, and its edits. */
export interface Sung {
  at: number;
  length: number;
  pitch: number;
  move: number;
  straighten: number;
}

declare module "./graph" {
  interface AudioClip {
    /** The kernel that plays its edited notes (RENDER-30b). */
    render?: string;
    /** Its voice's notes, by `at` (VOX-40). */
    sung?: Sung[];
  }
}

/** The measure Find notes runs, and the kernel that plays the edits: `@m2/vocals`' (VOX-42, VOX-45). */
export const MEASURE = "@m2/vocals:notes";
export const RENDER = "@m2/vocals:voice";
/** How far a note moves, as far as the pack's PSOLA reaches (VOX-23). */
export const DOWN = -7;
export const UP = 12;
/** How near a timing drag may take a note to its neighbour's marker, in seconds (VOX-47). */
const GAP = 0.01;

/** What a measure found in a file (VOX-41). */
export interface Measured {
  hop: number;
  rate: number;
  outputs: string[];
  values: number[][];
}

const measures = create<{ got: Record<string, Measured | "failed"> }>(() => ({ got: {} }));
const asked = new Set<string>();

/** What `@m2/vocals:notes` found in the file clip `c` plays: null until it comes, "failed" when it can't. */
export function useMeasure(c: Pick<AudioClip, "asset" | "hash">): Measured | "failed" | null {
  const session = useStore((s) => s.graph?.session);
  const key = `${session}\n${c.asset}\n${c.hash ?? ""}`;
  const got = measures((s) => s.got[key]);
  useEffect(() => {
    if (session == null || !c.asset || asked.has(key)) return;
    asked.add(key);
    const q = `session=${session}&asset=${encodeURIComponent(c.asset)}&measure=${encodeURIComponent(MEASURE)}`;
    request<Measured>(`/api/graph/clip/measure?${q}`).then(
      (m) => measures.setState((s) => ({ got: { ...s.got, [key]: m } })),
      () => measures.setState((s) => ({ got: { ...s.got, [key]: "failed" } })),
    );
  }, [session, key, c.asset]);
  return got ?? null;
}

/** Output `name` of a measure. */
export const output = (m: Measured, name: string) => m.values[m.outputs.indexOf(name)] ?? [];

/** The MIDI pitch of `hz`. */
export const midiOf = (hz: number) => 69 + 12 * Math.log2(hz / 440);

/** The file's second clip `c` plays at beat `b` of it, as the runtime's time map reads it. */
export function fileAt(c: AudioClip, tempo: Tempo, b: number): number {
  const w = c.warp ?? [];
  if (w.length === 0) return (c.offset ?? 0) + (secondsAt(tempo, c.start + b) - secondsAt(tempo, c.start)) * 2 ** ((c.pitch ?? 0) / 12);
  if (w.length === 1) return w[0][0] + secondsAt(tempo, c.start + b) - secondsAt(tempo, c.start + w[0][1]);
  let i = w.findIndex((m) => m[1] > b);
  i = Math.min(Math.max(i < 0 ? w.length - 1 : i, 1), w.length - 1);
  const [a, z] = [w[i - 1], w[i]];
  return a[0] + ((b - a[1]) * (z[0] - a[0])) / (z[1] - a[1]);
}

/** The beat of clip `c` at which it plays its file's second `s`: [`fileAt`] the other way. */
export function beatOf(c: AudioClip, tempo: Tempo, s: number): number {
  const w = c.warp ?? [];
  if (w.length === 0) return beatAt(tempo, secondsAt(tempo, c.start) + (s - (c.offset ?? 0)) / 2 ** ((c.pitch ?? 0) / 12)) - c.start;
  if (w.length === 1) return beatAt(tempo, secondsAt(tempo, c.start + w[0][1]) + s - w[0][0]) - c.start;
  let i = w.findIndex((m) => m[0] > s);
  i = Math.min(Math.max(i < 0 ? w.length - 1 : i, 1), w.length - 1);
  const [a, z] = [w[i - 1], w[i]];
  return a[1] + ((s - a[0]) * (z[1] - a[1])) / (z[0] - a[0]);
}

/** A number as the lane file keeps it: to a millionth, never -0. */
const clean = (x: number) => Math.round(x * 1e6) / 1e6 + 0;

/**
 * The warp markers of a timing edit (VOX-47): note `i` of `notes` to play from beat `from` to beat
 * `to` of the clip. Markers at its start and end there; markers kept where they play now at the end
 * of the note before and the start of the note after (the clip's start and end when there is none);
 * those between dropped; a clip with none gets its start and end too, so it follows the tempo from
 * then on (D7). The note stops 10 ms short of a neighbour's marker. Null for a drag that can't be.
 */
export function retimed(c: AudioClip, tempo: Tempo, notes: readonly Sung[], i: number, from: number, to: number): [number, number][] | null {
  const n = notes[i];
  const { start, end, before, after, lo, hi, gap } = anchors(c, tempo, notes, i);
  const a = Math.min(Math.max(from, lo), hi), z = Math.min(Math.max(to, lo), hi);
  if (!(z - a > gap(a)) || !(n.at > before[0]) || !(n.at + n.length < after[0])) return null;
  const was = c.warp?.length ? c.warp : [start, end];
  const kept = was.filter((m) => m[0] < before[0] || m[0] > after[0]);
  const all: [number, number][] = [...kept, before, [n.at, a], [n.at + n.length, z], after].map((m) => [clean(m[0]), clean(m[1])]);
  all.sort((p, q) => p[1] - q[1]);
  // Forward in both, as the lane file asks: a marker that isn't goes.
  const out: [number, number][] = [];
  for (const m of all) if (!out.length || (m[0] > out[out.length - 1][0] && m[1] > out[out.length - 1][1])) out.push(m);
  return out;
}

/**
 * The markers a timing edit of note `i` keeps (VOX-47): the clip's start and end, the end of the
 * note before and the start of the note after (as file second and beat of the clip), and how far
 * the note may go between them (`lo` to `hi`, beats), 10 ms short of each.
 */
export function anchors(c: AudioClip, tempo: Tempo, notes: readonly Sung[], i: number) {
  const now = (s: number) => beatOf(c, tempo, s);
  const start: [number, number] = [fileAt(c, tempo, 0), 0];
  const end: [number, number] = [fileAt(c, tempo, c.length), c.length];
  const before: [number, number] = i > 0 ? [notes[i - 1].at + notes[i - 1].length, now(notes[i - 1].at + notes[i - 1].length)] : start;
  const after: [number, number] = i + 1 < notes.length ? [notes[i + 1].at, now(notes[i + 1].at)] : end;
  // 10 ms short of each anchor, in beats at the tempo there.
  const gap = (b: number) => beatAt(tempo, secondsAt(tempo, c.start + b) + GAP) - (c.start + b);
  return { start, end, before, after, lo: before[1] + gap(before[1]), hi: after[1] - gap(after[1]), gap };
}

/** Clip `c` as the lane file writes it, with `changes`: what `set` sends (its hash as drawn left out). */
function written(c: AudioClip, changes: Partial<AudioClip>): Record<string, unknown> {
  const { sha: _sha, ...rest } = { ...c, ...changes } as AudioClip & { sha?: string };
  return rest as unknown as Record<string, unknown>;
}

/**
 * Write clip `ref`, drawn as `c`, with `changes` (its notes, its markers): one Tx and one undo step,
 * named `step`. Refused, the toast says the server's words.
 */
export async function writeClip(ref: ClipRef, c: AudioClip, changes: Partial<AudioClip>, step: string) {
  const { graph, toast } = useStore.getState();
  if (!graph) return null;
  const ours = editing();
  try {
    const f = await goesFrom(graph.session, graph.view.revision, [lanesOf(ref.module)], ours);
    const gesture = { op: "set", module: ref.module, lane: ref.lane, put: [written(c, changes)] } as unknown as Gesture;
    return await send(f.graph, gesture, { name: step }, undefined, ours, f.base);
  } catch (error) {
    toast(error instanceof Error ? error.message.replace(/^edit rejected: /, "") : String(error));
    return null;
  } finally {
    ours.end();
  }
}

/**
 * Find notes on clip `ref` (VOX-43): one undo step, "Find notes on Vox"; the song gets
 * `@m2/vocals` when it lacks it. Then the Notes view shows the clip's notes.
 */
export async function findNotes(graph: GraphState, lane: TimelineLane, ref: ClipRef, track: string) {
  const { toast } = useStore.getState();
  const ours = editing();
  try {
    const f = await goesFrom(graph.session, graph.view.revision, [lanesOf(ref.module)], ours);
    const drawn = f.shown ? findLane(f.graph.timeline, ref.module, ref.lane) : lane;
    const clip = drawn && serverRef(drawn, ref);
    if (!clip) return;
    const gesture = { op: "find_notes", clip, measure: MEASURE, render: RENDER } as unknown as Gesture;
    await send(f.graph, gesture, { name: `Find notes on ${track}` }, undefined, ours, f.base);
    const shell = useShell.getState();
    shell.select(ref.module, ref);
    shell.setView("notes");
  } catch (error) {
    toast(error instanceof Error ? error.message.replace(/^edit rejected: /, "") : String(error));
  } finally {
    ours.end();
  }
}
