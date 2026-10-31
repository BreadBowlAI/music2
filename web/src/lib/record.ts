// Record (lane B, gaps #24): what you play on a MIDI keyboard or the keyboard piano becomes a notes
// clip; on an audio track, what the sound input hears becomes an audio clip (the io plan's slice 3:
// the input and Hear yourself are Settings', which the server reads). R or the top bar's Record button: stopped, one bar of clicks first (unless the menu says
// not), then the song plays and the take starts at the playhead; playing, it starts now. R
// again, or Space, ends it: the server writes it as one clip over whole bars, one Undo step
// ("Undo recording on Bass"), and the window selects it.
//
// The server keeps the take (graph/record.rs); this window shows it: the state's `record` when the
// song loads, then each `{"t": "take", …}` as its phase or notes change, and the outcome:
// `written` (select the clip), `empty`, `waiting` (the code doesn't compile: it is written once it
// does) or `error` (a toast). The count's beats left are this window's own, from the answer's
// `{beats, ms}` and its clock. While counting, Space, Return, Esc and R cancel it, and nothing plays
// (D6). While the code doesn't compile, R says so and starts nothing (the review's R1).
//
// Since track recording (MON-9 to MON-15): Record records into the armed tracks (each header's R;
// one notes track at a time, any number of audio tracks, each from its own input channels), or the
// selected one when none is armed. Punch (Record's menu) records only between the loop's edges and
// replaces what was there; over the loop, each pass is a take; Overdub puts notes into the clip
// under them. An audio clip over a loop has a Takes menu (Wave.tsx).

import { create } from "zustand";
import { on } from "./events";
import { useClipSelection } from "./clips";
import { findLane, clipIndex, lockReason, type ClipRef, type GraphState, type TimelineTrack } from "./graph";
import { useGrid } from "./grid";
import { holdTarget, currentRecordTarget } from "./playTarget";
import { request } from "./projects";
import { settingsApi } from "./settings";
import { useStore } from "./store";

/** A note of the take, in song beats; `held`: its key is still down (it grows to the playhead). */
export interface TakeNote {
  pitch: number;
  start: number;
  length: number;
  velocity: number;
  held?: boolean;
}

/** A lane a take records on: where its clip starts, and an audio lane's input channels ("3-4"). */
export interface TakeLane {
  module: string;
  lane: string;
  kind: "notes" | "audio";
  start: number;
  channels?: string;
}

/** The take as the server says it. */
export interface Take {
  phase: "count" | "take";
  /** Its first lane; every lane it records on (MON-9). */
  module: string;
  lane: string;
  lanes?: TakeLane[];
  /** The pass of the loop it records now, from 0 (MON-11); Punch and Overdub (MON-10, MON-12). */
  pass?: number;
  punch?: boolean;
  overdub?: boolean;
  /** A notes take, or an audio take from the sound input (io); `hear`: the input plays on the track while it records. */
  kind?: "notes" | "audio";
  hear?: boolean;
  /** The bar line the clip starts on, and where recording started. */
  start: number;
  from: number;
  /** The loop's end (each pass ends there), or a punch's. */
  until: number | null;
  count_in: { beats: number; ms: number } | null;
  notes: TakeNote[];
}

/** The armed tracks, each with the lane it records on, and each track's input channels (MON-9, MON-13). */
export interface Arms {
  armed: Record<string, string>;
  channels: Record<string, string>;
}

declare module "./graph" {
  interface GraphState {
    /** The take recording, or null (lane B). */
    record?: Take | null;
    arm?: Arms;
  }
}

interface Recording {
  take: Take | null;
  /** When this window heard the count start (performance.now()), for its beats left. */
  counted: number | null;
  /** Asked for, not answered yet: R again waits for it. */
  asking: boolean;
}

export const useRecord = create<Recording>(() => ({ take: null, counted: null, asking: false }));

/** Record's options, this machine's (`settings.json` `record`): Punch and Overdub since track recording. */
export interface RecordOptions {
  count_in: boolean;
  quantize: boolean;
  punch: boolean;
  overdub: boolean;
}

export const useRecordOptions = create<RecordOptions & { read: boolean }>(() => ({ count_in: true, quantize: false, punch: false, overdub: false, read: false }));

/** The options as kept, read once. */
export async function loadRecordOptions(): Promise<RecordOptions> {
  const o = useRecordOptions.getState();
  if (o.read) return o;
  try {
    const s = await settingsApi.get();
    if (s.record) useRecordOptions.setState({ punch: false, overdub: false, ...s.record, read: true });
  } catch {
    // The defaults: count in, don't snap.
  }
  return useRecordOptions.getState();
}

/** Change an option: kept at once. */
export async function setRecordOption(change: Partial<RecordOptions>) {
  const now = { ...(await loadRecordOptions()), ...change };
  const kept = { count_in: now.count_in, quantize: now.quantize, punch: now.punch, overdub: now.overdub };
  useRecordOptions.setState({ ...kept, read: true });
  try {
    await settingsApi.set({ record: kept });
  } catch (error) {
    useStore.getState().toast(error instanceof Error ? error.message : String(error));
  }
}

/** The beats left in the count, as the window counts them: 4, 3, 2, 1; null when not counting. */
export function beatsLeft(take: Take | null, counted: number | null, now = performance.now()): number | null {
  if (take?.phase !== "count" || !take.count_in || counted === null) return null;
  const { beats, ms } = take.count_in;
  const done = Math.floor(((now - counted) / ms) * beats);
  return Math.max(1, beats - done);
}

const toastError = (error: unknown) => useStore.getState().toast(error instanceof Error ? error.message : String(error));

/** Notes' grid in beats, for snapping the take: a fixed step (Auto counts as 1/16). */
function notesGrid() {
  const g = useGrid.getState().notes;
  return typeof g.pick === "number" && g.step > 0 ? g.step : 0.25;
}

/** Set the take as heard; a notes take's lane is what keys play while it runs (D8). */
function show(take: Take | null) {
  const was = useRecord.getState();
  const counted = take?.phase === "count" ? (was.take?.phase === "count" ? was.counted : performance.now()) : null;
  useRecord.setState({ take, counted });
  const notes = take?.lanes?.find((l) => l.kind === "notes") ?? (take && take.kind !== "audio" ? take : null);
  holdTarget(notes ? { module: notes.module, lane: notes.lane } : null);
  listen(take?.phase === "count");
}

/** R, the Record button, Transport ▸ Record: start, or stop the take, or cancel the count. */
export function toggleRecord() {
  const { take, asking } = useRecord.getState();
  if (asking) return;
  if (take?.phase === "count") return void cancelRecording();
  if (take) return void stopRecording();
  void startRecording();
}

/** Start a take on the lane keys play (Q21), or on the selected audio track (io). */
export async function startRecording() {
  const { graph, toast, connected } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return toast(`${why}.`);
  // The armed tracks, else the selected one (MON-9): the server records into the armed ones.
  const armed = Object.entries(graph.arm?.armed ?? {})[0];
  const t = armed ? { module: armed[0], lane: armed[1] } : currentRecordTarget();
  if (!t) return toast("Select a notes track or an audio track to record on it.");
  useRecord.setState({ asking: true });
  try {
    const o = await loadRecordOptions();
    const take = await request<Take>("/api/graph/record/start", {
      session: graph.session,
      module: t.module,
      lane: t.lane,
      count_in: o.count_in,
      quantize: o.quantize ? notesGrid() : null,
      punch: o.punch,
      overdub: o.overdub,
    });
    show(take);
  } catch (error) {
    toastError(error);
  } finally {
    useRecord.setState({ asking: false });
  }
}

/** R while recording: the take ends where it is heard, and play goes on. */
export async function stopRecording() {
  const graph = useStore.getState().graph;
  if (!graph) return;
  try {
    await request("/api/graph/record/stop", { session: graph.session });
  } catch (error) {
    toastError(error);
  }
}

/** R, Esc, Space or Return while counting in: nothing plays, nothing is kept. */
export async function cancelRecording() {
  const graph = useStore.getState().graph;
  if (!graph) return;
  show(null);
  try {
    await request("/api/graph/record/cancel", { session: graph.session });
  } catch (error) {
    toastError(error);
  }
}

/** Waits for the take this window ended to be written (or found empty), at most `ms`. */
function ended(ms: number): Promise<void> {
  return new Promise((resolve) => {
    const done = () => {
      off();
      clearTimeout(timer);
      resolve();
    };
    const off = on<{ written?: unknown; empty?: boolean; waiting?: string; error?: string; phase?: unknown }>("take", (m) => {
      if (m.written || m.empty || m.waiting || m.error) done();
    });
    const timer = setTimeout(done, ms);
  });
}

/** Close Song while recording (D35): the take is stopped and written first. */
export async function stopThen(then: () => void) {
  const take = useRecord.getState().take;
  if (!take) return then();
  if (take.phase === "count") {
    await cancelRecording();
    return then();
  }
  const written = ended(3000);
  await stopRecording();
  await written;
  then();
}

// While counting, Space, Return, Esc and R cancel the count: before the page's own keys see them,
// so Space doesn't play and Return doesn't seek (D6).
function onCountKey(e: KeyboardEvent) {
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  if (!(e.code === "Space" || e.key === "Enter" || e.key === "Escape" || e.key.toLowerCase() === "r")) return;
  const t = e.target;
  if (t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || (t instanceof HTMLElement && t.isContentEditable)) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  if (!e.repeat) void cancelRecording();
}

let listening = false;
function listen(counting: boolean) {
  if (counting === listening) return;
  listening = counting;
  if (counting) window.addEventListener("keydown", onCountKey, true);
  else window.removeEventListener("keydown", onCountKey, true);
}

/** The clip a take wrote, to select once the timeline has it. */
let toSelect: { module: string; lane: string; id: string } | null = null;

function selectWritten(graph: GraphState | null) {
  if (!toSelect || !graph) return;
  const lane = findLane(graph.timeline, toSelect.module, toSelect.lane);
  if (!lane || clipIndex(lane.clips, toSelect) < 0) return;
  useClipSelection.getState().only([toSelect]);
  toSelect = null;
}

interface TakeMsg extends Partial<Take> {
  /** `note`: what to say about an audio take (the disk was slow, the input went away). */
  written?: { revision: number; made: { module: string; lane: string; id: string }[]; module: string; lane: string; note?: string };
  empty?: boolean;
  /** The code doesn't compile: the take is written once it does. */
  waiting?: string;
  error?: string;
}

on<TakeMsg>("take", (m) => {
  const { toast, graph } = useStore.getState();
  if (m.written) {
    toSelect = m.written.made[0] ?? null;
    selectWritten(graph);
    if (m.written.note) toast(m.written.note);
    return;
  }
  if (m.empty) return toast("Nothing was played, so nothing was recorded.");
  if (m.waiting) return toast(m.waiting);
  if (m.error) return toast(m.error);
  if (m.phase === undefined) return;
  show(m.phase === null ? null : (m as Take));
});

// The state's `record` each time the song is fetched: a window that just loaded, or another song.
let seen: Take | null | undefined;
useStore.subscribe((s) => {
  const r = s.graph ? (s.graph.record ?? null) : null;
  if (r !== seen) {
    seen = r;
    // A take another window started, or one this window missed; none once the song closes.
    if (!s.graph || r || useRecord.getState().take) show(r);
  }
  selectWritten(s.graph);
});

/**
 * The lane a track's R arms (MON-9): its notes lane, else its audio lane; of several, the one of the
 * clip the Notes view shows for that track (`shown`), as what plays chooses (playTarget.ts), else the
 * first.
 */
export function armLane(track: TimelineTrack, shown?: ClipRef): { lane: string; audio: boolean } | null {
  const pick = (kind: "notes" | "audio") => {
    const lanes = track.lanes.filter((l) => l.kind === kind);
    return (shown?.module === track.module ? lanes.find((l) => l.key === shown.lane) : undefined) ?? lanes[0];
  };
  const notes = pick("notes");
  const audio = pick("audio");
  return notes ? { lane: notes.key, audio: false } : audio ? { lane: audio.key, audio: true } : null;
}

/** Arm a track on `lane`, or disarm it; or choose the input channels it records ("3-4", "2"). No undo step. */
export async function arm(module: string, change: { lane?: string; armed?: boolean; channels?: string }) {
  const { graph } = useStore.getState();
  if (!graph) return;
  try {
    await request("/api/graph/arm", { session: graph.session, module, ...change });
  } catch (error) {
    toastError(error);
  }
}

/** How many channels the sound input Settings chose has: what a track's R menu lists. */
export async function inputChannels(): Promise<{ name: string; channels: number }> {
  try {
    const [s, inputs] = await Promise.all([settingsApi.get(), settingsApi.inputs()]);
    const id = s.input?.device ?? null;
    const input = inputs.find((i) => i.id === id) ?? inputs.find((i) => i.default) ?? inputs[0];
    return input ? { name: input.name, channels: Math.max(1, input.channels) } : { name: "The input", channels: 2 };
  } catch {
    return { name: "The input", channels: 2 };
  }
}

/** The channels an input of `n` offers: its pairs ("1-2", "3-4", …), then each one alone. */
export function channelChoices(n: number): string[] {
  const pairs = Array.from({ length: Math.floor(n / 2) }, (_, i) => `${2 * i + 1}-${2 * i + 2}`);
  return [...pairs, ...Array.from({ length: n }, (_, i) => `${i + 1}`)];
}

/** "3–4" for "3-4": a range as the window says it. */
export const channelsSaid = (c: string) => c.replace("-", "–");
