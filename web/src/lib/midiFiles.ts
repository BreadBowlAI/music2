// MIDI files (studio lane C, Q32 to Q38): the song, or one notes clip, saved as a MIDI file other
// music apps open. In the app a save panel picks the file first, starting at the song's new file in
// the songs folder's Exports; in a browser it goes straight there, by a free name. The file is
// written by the server (crates/server/src/routes_exchange.rs): never into the song.
//
// And a MIDI file brought in (C2b): File ▸ Import MIDI…, or a drop. Each part becomes a new track
// with a fitting sound, after the tempo question when the song's tempo differs (MidiTempoAsk); a
// one-part file dropped on a notes lane goes into it. One undo step, "importing groove.mid".

import { create } from "zustand";
import { barAt, graphApi, lockReason, moduleName, projectApi, readJson, tilde, type ClipRef, type GraphState } from "./graph";
import { onFiles, type DropTarget } from "./drop";
import { useClipSelection } from "./clips";
import { trackMade } from "./events";
import { guideExported } from "./guide";
import { native, nativeAction, revealLabel, token } from "./native";
import { listed } from "./overlays";
import { useShell } from "./shell";
import { useStore } from "./store";
import { undoAction } from "../components/GraphCanvas";

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

const message = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(path, { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(body) });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as T;
}

/** What a track is called on screen. */
function nameOf(graph: GraphState, module: string) {
  const t = graph.timeline.tracks.find((t) => t.module === module);
  return t ? moduleName({ key: module, label: t.label }) : module;
}

/** The home folder, so the browser's toast can say `~/Music/music2/Exports`. Asked once. */
let home: Promise<string | null> | null = null;
const homeDir = () => (home ??= projectApi.catalog().then((c) => c.home, () => null));

/** In the app: the file its save panel picked (null: cancelled), starting at `start`. */
async function pick(start: string, title: string): Promise<string | null> {
  const slash = start.lastIndexOf("/");
  const arg = { title, name: start.slice(slash + 1), start: start.slice(0, slash), filters: [{ name: "MIDI file", extensions: ["mid", "midi"] }] };
  const picked = await nativeAction("save_file", JSON.stringify(arg));
  return typeof picked === "string" ? picked : null;
}

/**
 * Save the open song as a MIDI file, or `clip` alone (the clip menu's Export as MIDI…). Says where
 * it went in a toast, with Show in Finder in the app; a refusal ("The song has no notes…") in words.
 * Answers the file written, or null.
 */
export async function exportMidi(clip?: ClipRef): Promise<string | null> {
  const { graph, toast } = useStore.getState();
  if (!graph) return null;
  const session = graph.session;
  const at = clip ? { module: clip.module, lane: clip.lane, id: clip.id } : undefined;
  const what = clip ? nameOf(graph, clip.module) : "the song";
  try {
    let path: string | undefined;
    if (native) {
      const start = await post<{ path: string }>("/api/graph/midifile/where", { session, clip: at });
      const picked = await pick(start.path, clip ? "Export Clip as MIDI" : "Export MIDI");
      if (picked === null) return null;
      path = picked;
    }
    const done = await post<{ path: string }>("/api/graph/midifile/export", { session, path, clip: at });
    const slash = done.path.lastIndexOf("/");
    const file = done.path.slice(slash + 1);
    const saved = `Saved ${what} as a MIDI file: ${file}`;
    if (!clip) guideExported(done.path);
    if (native) {
      const reveal = () => void nativeAction("reveal_file", done.path).catch((error) => toast(message(error)));
      toast(`${saved}.`, { label: revealLabel, run: reveal });
    } else {
      toast(`${saved}, in ${tilde(done.path.slice(0, slash), await homeDir())}.`);
    }
    return done.path;
  } catch (error) {
    toast(`Couldn't save ${what} as a MIDI file: ${message(error)}`);
    return null;
  }
}

// ---------------------------------------------------------------- bringing a MIDI file in (C2b)

/** Where a MIDI file goes (Q36): into a notes lane (a file of one part; more make new tracks after its track), new tracks after a track, or new tracks at the end. */
export type MidiTarget = { kind: "lane"; module: string; lane: string; beat: number } | { kind: "track"; module: string; beat: number } | { kind: "below"; beat: number };

/** A MIDI file to bring in: its path on this computer (the app), or the file itself (a browser). */
export type MidiIncoming = { name: string; path: string } | { name: string; file: File };

/** The tempo question's answer (Q35). */
export type TempoChoice = "keep" | "use";

/** What the server answers: the edit, or the tempo question (nothing written). */
type Imported =
  | { asks: { file_bpm: number; song_bpm: number } }
  | { revision: number; step?: number; name: string; made: { module: string; lane: string; id: string }[]; tracks: { module: string; label: string }[] };

/** The tempo question while it waits for an answer (MidiTempoAsk): the file, where it goes, both tempos. */
export interface MidiAsk {
  file: MidiIncoming;
  to: MidiTarget;
  fileBpm: number;
  songBpm: number;
}
export const useMidiAsk = create<{ ask: MidiAsk | null }>(() => ({ ask: null }));

/** The most a MIDI file may be: the server's `smf::MAX_BYTES`. */
const MAX_BYTES = 16 * 1024 * 1024;

/** The MIDI file names music2 takes, and the types a browser says while it drags one. */
const MIDI = [".mid", ".midi"];
const MIDI_TYPES = ["audio/midi", "audio/mid", "audio/x-midi", "audio/x-mid", "audio/sp-midi"];

/** A tempo as people read it: 128, or 127.5. */
export const bpm = (n: number) => String(Math.round(n * 100) / 100);

async function send(graph: GraphState, file: MidiIncoming, to: MidiTarget, tempo?: TempoChoice): Promise<Imported> {
  let res: Response;
  if ("path" in file) {
    res = await fetch("/api/graph/midifile/import", {
      method: "POST",
      headers: { "content-type": "application/json", ...auth },
      body: JSON.stringify({ session: graph.session, path: file.path, to, ...(tempo ? { tempo } : {}) }),
    });
  } else {
    if (file.file.size > MAX_BYTES) throw new Error(`${file.name} is over 16 MB: music2 brings in MIDI files up to 16 MB.`);
    const q = `session=${graph.session}&name=${encodeURIComponent(file.name)}&to=${encodeURIComponent(JSON.stringify(to))}${tempo ? `&tempo=${tempo}` : ""}`;
    res = await fetch(`/api/graph/midifile/import?${q}`, { method: "POST", headers: { "content-type": "application/octet-stream", ...auth }, body: file.file });
  }
  const json = await readJson(res);
  if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
  return json as Imported;
}

/**
 * Bring `file` in at `to`, with the tempo question's answer when there is one. When the question
 * is due, it shows (MidiTempoAsk) and nothing is written yet. Afterwards the new clips are selected,
 * the first new track too, and a toast with Undo says what came in ("Imported groove.mid: Drums,
 * Bass and Keys"). A refusal says why, and nothing is added to the song.
 */
export async function bringMidi(file: MidiIncoming, to: MidiTarget, tempo?: TempoChoice) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return void toast(`${why}.`);
  try {
    const done = await send(graph, file, to, tempo);
    if ("asks" in done) return void useMidiAsk.setState({ ask: { file, to, fileBpm: done.asks.file_bpm, songBpm: done.asks.song_bpm } });
    const fresh = await graphApi.get();
    useStore.getState().set({ graph: fresh });
    const refs = done.made.map((m) => ({ module: m.module, lane: m.lane, id: m.id }));
    if (refs[0]) useShell.getState().select(refs[0].module);
    useClipSelection.getState().only(refs);
    for (const t of done.tracks) trackMade(t.module, done.made.find((m) => m.module === t.module)?.lane ?? "notes");
    const into = done.tracks.length ? `: ${listed(done.tracks.map((t) => t.label))}` : refs[0] ? ` into ${nameOf(fresh, refs[0].module)}` : "";
    toast(`Imported ${file.name}${into}`, undoAction(graph.session, done.step));
  } catch (error) {
    toast(`Nothing was added to the song: ${message(error)}`);
  }
}

/** The file the person picks: the app's own dialog when it has one, else the page's file input. */
async function pickMidi(): Promise<MidiIncoming | null> {
  if (native) {
    try {
      const arg = JSON.stringify({ title: "Import MIDI", filters: [{ name: "MIDI files", extensions: ["mid", "midi"] }], multiple: false });
      const answer = await nativeAction("open_file", arg);
      if (answer === null) return null;
      const picked = JSON.parse(answer) as string[] | string | null;
      const path = Array.isArray(picked) ? picked[0] : picked;
      return path ? { name: path.split(/[\\/]/).pop() ?? path, path } : null;
    } catch {
      // A shell without the dialog: the page's own.
    }
  }
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = [...MIDI, ...MIDI_TYPES].join(",");
    input.style.display = "none";
    input.addEventListener("change", () => {
      const file = input.files?.[0];
      resolve(file ? { name: file.name, file } : null);
      input.remove();
    });
    input.addEventListener("cancel", () => {
      resolve(null);
      input.remove();
    });
    document.body.append(input);
    input.click();
  });
}

/** File ▸ Import MIDI… (no key): pick a MIDI file; its parts become new tracks at the end, from bar 1 (Q36). */
export async function importMidi() {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return void toast(`${why}.`);
  const file = await pickMidi();
  const now = useStore.getState().graph;
  if (!now || now.session !== graph.session || !file) return;
  await bringMidi(file, { kind: "below", beat: 0 });
}

/**
 * Where a dropped MIDI file goes (Q36): onto the notes lane under the pointer (into it, when the
 * file has one part); any other row of a track, new tracks after it; below the tracks, at the end.
 */
function midiTarget(t: DropTarget): MidiTarget | null {
  const graph = useStore.getState().graph;
  if (!graph || t.kind === "none" || t.kind === "sampler") return null;
  if (t.kind === "below") return { kind: "below", beat: t.beat };
  const track = graph.timeline.tracks.find((x) => x.module === t.module);
  if (!track?.track) return null;
  const lane = t.kind === "lane" ? track.lanes.find((l) => l.key === t.lane) : undefined;
  if (t.kind === "lane" && lane?.kind === "notes") return { kind: "lane", module: t.module, lane: t.lane, beat: t.beat };
  return { kind: "track", module: t.module, beat: t.beat };
}

/** One file at a time: each may ask its own tempo question. */
const ONE = "One MIDI file at a time";

onFiles(MIDI, MIDI_TYPES, {
  hint(files, t) {
    const graph = useStore.getState().graph;
    const to = midiTarget(t);
    if (!graph || !to) return null;
    if (files.length > 1) return ONE;
    const bar = `bar ${barAt(graph.timeline.meter, to.beat).bar + 1}`;
    const named = files[0]?.name ? `${files[0].name}: new tracks` : "New tracks";
    if (to.kind === "lane") return `Into ${nameOf(graph, to.module)} · ${bar}`;
    if (to.kind === "track") return `${named} after ${nameOf(graph, to.module)} · ${bar}`;
    return `${named} · ${bar}`;
  },
  where(t) {
    return midiTarget(t) ?? { kind: "none" };
  },
  drop(files, t) {
    const to = midiTarget(t);
    if (!to) return;
    if (files.length > 1) return void useStore.getState().toast(`${ONE}.`);
    const f = files[0];
    const file: MidiIncoming | null = f?.path ? { name: f.name, path: f.path } : f?.file ? { name: f.name, file: f.file } : null;
    if (file) void bringMidi(file, to);
  },
});
