// Bringing audio files in (lane A's A2, Q6, Q7): File ▸ Import Audio… (⇧⌘I), a drop, and what an
// import says after. The server copies each file into the song's `assets/` (or uses the same bytes
// there again) and places its clip, as one undo step ("Undo importing vox.wav"). In the app the
// files go as paths; in a browser, as bytes.

import { barAt, graphApi, lockReason, moduleName, readJson, type GraphState, type Instrument } from "./graph";
import { onFiles, onSample, type DropTarget } from "./drop";
import type { LibraryItem } from "./library";
import { useClipSelection } from "./clips";
import { beatNow } from "./clock";
import { native, nativeAction, token } from "./native";
import { useShell } from "./shell";
import { useStore } from "./store";
import { trackMade } from "./events";
import { undoAction } from "../components/GraphCanvas";
import { dropOnSampler, samplerHint } from "./sampler";

/** A file to bring in: its path on this computer (the app), or the file itself (a browser). */
export type Incoming = { name: string; path: string } | { name: string; file: File };

/** Where files go: onto an audio lane, onto a new audio track after a track, or at the end (Q6). */
export type Target = { kind: "lane"; module: string; lane: string; beat: number } | { kind: "track"; module: string; beat: number } | { kind: "below"; beat: number };

/** The files music2 reads (Q1): taken as WAV by their header on the server, offered by name here. */
export const WAV = [".wav", ".wave"];
export const isWav = (name: string) => WAV.some((x) => name.toLowerCase().endsWith(x));

/** What the server answers. */
interface Imported {
  revision: number;
  /** The undo step it made, by its id (AGENT-2): what its toast's Undo takes back. */
  step?: number;
  name: string;
  made: { module: string; lane: string; id: string }[];
  tracks: { module: string; label: string }[];
  notes: string[];
  assets: string[];
}

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

/** An error in words: the server's refusal, without "edit rejected" in front. */
const words = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

/** The most a browser's upload may be, all its files together: the server's `UPLOAD` (routes_audio.rs). */
const UPLOAD = 2 ** 30;

/** Why files that big can't be sent from a browser, in words (the review's R9). */
const tooBig = (files: Incoming[]) =>
  files.length === 1
    ? `${files[0].name} is too big to upload from a browser (over 1 GB).`
    : "these files are too big to upload from a browser together (over 1 GB). Bring in fewer at a time.";

async function post(graph: GraphState, files: Incoming[], to: Target): Promise<Imported> {
  const paths = files.flatMap((f) => ("path" in f ? [f.path] : []));
  let res: Response;
  if (paths.length === files.length) {
    res = await fetch("/api/graph/audio/import", {
      method: "POST",
      headers: { "content-type": "application/json", ...auth },
      body: JSON.stringify({ session: graph.session, paths, to }),
    });
  } else {
    const blobs = files.map((f) => ("file" in f ? f.file : new Blob()));
    if (blobs.reduce((n, b) => n + b.size, 0) > UPLOAD) throw new Error(tooBig(files));
    const list = files.map((f, i) => ({ name: f.name, size: blobs[i].size }));
    const q = `session=${graph.session}&files=${encodeURIComponent(JSON.stringify(list))}&to=${encodeURIComponent(JSON.stringify(to))}`;
    res = await fetch(`/api/graph/audio/import?${q}`, { method: "POST", headers: { "content-type": "application/octet-stream", ...auth }, body: new Blob(blobs) });
  }
  if (res.status === 413) throw new Error(tooBig(files));
  const json = await readJson(res);
  if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
  return json as Imported;
}

/**
 * Bring `files` in at `to`: afterwards the new clips are selected, a new track lit. A toast only
 * when a track was added ("Added Vox, playing vox.wav") or a file changed on the way in (its extra
 * channels); a refusal says why, and nothing is added to the song.
 */
export async function bringIn(files: Incoming[], to: Target) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph || files.length === 0) return;
  const why = lockReason(graph, connected);
  if (why) return void toast(`${why}.`);
  try {
    const done = await post(graph, files, to);
    const fresh = await graphApi.get();
    useStore.getState().set({ graph: fresh });
    const refs = done.made.map((m) => ({ module: m.module, lane: m.lane, id: m.id }));
    if (refs[0]) useShell.getState().select(refs[0].module);
    useClipSelection.getState().only(refs);
    for (const t of done.tracks) trackMade(t.module, done.made.find((m) => m.module === t.module)?.lane ?? "clips");
    const said = [...done.notes];
    if (done.tracks.length === 1) said.unshift(`Added ${done.tracks[0].label}, playing ${done.assets[0]}`);
    else if (done.tracks.length > 1) said.unshift(`Added ${done.tracks.length} audio tracks`);
    // A new track may be off screen: its toast has Undo.
    if (said.length) toast(said.join(" "), done.tracks.length ? undoAction(graph.session, done.step) : undefined);
  } catch (error) {
    toast(`Nothing was added to the song: ${words(error)}`);
  }
}

/**
 * Where File ▸ Import Audio… puts files (Q7): at the bar line at or before the playhead; on the
 * selected track when it has an audio lane, else on a new audio track after it (at the end with
 * none selected).
 */
export function menuTarget(graph: GraphState): Target {
  const beat = barAt(graph.timeline.meter, Math.max(0, beatNow())).start;
  const selected = useShell.getState().selected;
  const track = graph.timeline.tracks.find((t) => t.module === selected && t.track);
  const lane = track?.lanes.find((l) => l.kind === "audio");
  if (track && lane) return { kind: "lane", module: track.module, lane: lane.key, beat };
  if (track) return { kind: "track", module: track.module, beat };
  return { kind: "below", beat };
}

// ---------------------------------------------------------------- samples from the library (LIB-7)

/** What the server answers when it adds a sample. */
interface SampleAdded {
  step?: number;
  name: string;
  asset: string;
  made?: { module: string; lane: string; id: string }[];
  tracks?: { module: string; label: string }[];
  /** The loop's own tempo it follows, when it does. */
  follows?: number | null;
  song_bpm?: number;
}

/**
 * Add sample `item` from the library (LIB-7): its file copied into the song with a note of where it
 * came from, then a clip at `to` (a loop at another tempo with Follow tempo on), or the file of the
 * Sampler `to` names. One undo step, with Undo in its toast.
 */
export async function addSample(item: LibraryItem, to: Target | { kind: "sampler"; call: string; title: string }) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return void toast(`${why}.`);
  const body = to.kind === "sampler" ? { session: graph.session, id: item.id, at: graph.view.revision, call: to.call } : { session: graph.session, id: item.id, to };
  try {
    const res = await fetch("/api/library/sample", { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(body) });
    const done = (await readJson(res)) as SampleAdded & { error?: string };
    if (!res.ok) throw new Error(done.error ?? `HTTP ${res.status}`);
    const fresh = await graphApi.get();
    useStore.getState().set({ graph: fresh });
    if (to.kind === "sampler") return void toast(`${to.title} plays ${done.asset}`, undoAction(graph.session, done.step));
    const refs = (done.made ?? []).map((m) => ({ module: m.module, lane: m.lane, id: m.id }));
    if (refs[0]) useShell.getState().select(refs[0].module);
    useClipSelection.getState().only(refs);
    for (const t of done.tracks ?? []) trackMade(t.module, done.made?.find((m) => m.module === t.module)?.lane ?? "clips");
    const onto = done.tracks?.[0] ? ` on a new track, ${done.tracks[0].label}` : "";
    const bpm = (n: number) => Math.round(n * 10) / 10;
    // LIB-7: every loop whose tempo is known follows the song's, at the song's own tempo too.
    const follows = !done.follows
      ? ""
      : bpm(done.follows) === bpm(done.song_bpm ?? 0)
        ? ". Follow tempo is on, so it keeps time if the song's tempo changes"
        : `. Follow tempo is on: its ${bpm(done.follows)} bpm plays at the song's ${bpm(done.song_bpm ?? 0)}`;
    toast(`Added ${item.label}${onto}${follows}`, undoAction(graph.session, done.step));
  } catch (error) {
    toast(`Nothing was added to the song: ${words(error)}`);
  }
}

/** Where Enter puts a sample: as File ▸ Import Audio… puts a file (Q7). */
export const sampleTarget = (graph: GraphState) => menuTarget(graph);

onSample({
  hint(item, t) {
    if (t.kind === "sampler") return `${t.title} plays ${item.label}`;
    const graph = useStore.getState().graph;
    const to = audioTarget(t);
    if (!graph || !to) return null;
    const bar = `bar ${barAt(graph.timeline.meter, to.beat).bar + 1}`;
    if (to.kind === "lane") {
      const tr = graph.timeline.tracks.find((x) => x.module === to.module);
      return `${moduleName({ key: to.module, label: tr?.label })} · ${bar}`;
    }
    return `New audio track · ${bar}`;
  },
  where(t) {
    if (t.kind === "sampler") return t;
    const to = audioTarget(t);
    if (!to) return { kind: "none" };
    return to.kind === "lane" ? { kind: "lane", module: to.module, lane: to.lane, beat: to.beat } : to;
  },
  drop(item, t) {
    if (t.kind === "sampler") return void addSample(item, t);
    const to = audioTarget(t);
    if (to) void addSample(item, to);
  },
});

/** The files the person picks: the app's own dialog when it has one, else the page's file input. */
async function pick(): Promise<Incoming[]> {
  if (native) {
    try {
      const arg = JSON.stringify({ title: "Import Audio", filters: [{ name: "WAV files", extensions: ["wav", "wave"] }], multiple: true });
      const answer = await nativeAction("open_file", arg);
      if (answer === null) return [];
      const paths = JSON.parse(answer) as string[] | null;
      return (paths ?? []).map((p) => ({ name: p.split(/[\\/]/).pop() ?? p, path: p }));
    } catch {
      // A shell without the dialog: the page's own.
    }
  }
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.multiple = true;
    input.accept = ".wav,.wave,audio/wav,audio/x-wav";
    input.style.display = "none";
    input.addEventListener("change", () => {
      resolve([...(input.files ?? [])].map((file) => ({ name: file.name, file })));
      input.remove();
    });
    input.addEventListener("cancel", () => {
      resolve([]);
      input.remove();
    });
    document.body.append(input);
    input.click();
  });
}

/** File ▸ Import Audio… (⇧⌘I): pick WAV files, then bring them in where Q7 says. */
export async function importAudio() {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return void toast(`${why}.`);
  const files = await pick();
  const now = useStore.getState().graph;
  if (!now || now.session !== graph.session || files.length === 0) return;
  await bringIn(files, menuTarget(now));
}

/** + Track's last group, "Audio" (Q8): an audio track, not an instrument. Never in Change Instrument. */
export const AUDIO_TRACK: Instrument = {
  component: "audio:track",
  label: "Audio track",
  name: "",
  module: "",
  description: "Plays recordings and loops: drop a WAV file on it",
  category: "Audio",
  lane: "clips",
};

/**
 * + Track ▸ Audio track: an audio track after `after` (else last), selected and lit, with a toast
 * that has Undo ("Undo adding Audio").
 */
export async function addAudioTrack(after: string | null) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return void toast(`${why}.`);
  try {
    const res = await fetch("/api/graph/audio/track", {
      method: "POST",
      headers: { "content-type": "application/json", ...auth },
      body: JSON.stringify({ session: graph.session, ...(after ? { after } : {}) }),
    });
    const json = await readJson(res);
    if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
    const fresh = await graphApi.get();
    useStore.getState().set({ graph: fresh });
    useShell.getState().select(json.module);
    trackMade(json.module, AUDIO_TRACK.lane);
    toast(`Added ${json.label}`, undoAction(graph.session, json.step));
  } catch (error) {
    toast(words(error));
  }
}

// ---------------------------------------------------------------- drops (Q6)

/** The WAV types a browser says while it drags a file (no names until the drop). */
const WAV_TYPES = ["audio/wav", "audio/x-wav", "audio/wave", "audio/x-wave", "audio/vnd.wave"];

/**
 * Where dropped audio goes (Q6): onto the audio lane under the pointer, or a track's audio lane;
 * any other track gets a new audio track right after it; below the tracks, a new one for each file.
 */
function audioTarget(t: DropTarget): Target | null {
  const graph = useStore.getState().graph;
  if (!graph || t.kind === "none" || t.kind === "sampler") return null;
  if (t.kind === "below") return { kind: "below", beat: t.beat };
  const track = graph.timeline.tracks.find((x) => x.module === t.module);
  if (!track) return null;
  const here = t.kind === "lane" ? track.lanes.find((l) => l.key === t.lane) : undefined;
  const audio = here?.kind === "audio" ? here : t.kind === "track" ? track.lanes.find((l) => l.kind === "audio") : undefined;
  if (audio) return { kind: "lane", module: t.module, lane: audio.key, beat: t.beat };
  return track.track ? { kind: "track", module: t.module, beat: t.beat } : null;
}

onFiles(WAV, WAV_TYPES, {
  hint(files, t) {
    if (t.kind === "sampler") return samplerHint(files, t);
    const graph = useStore.getState().graph;
    const to = audioTarget(t);
    if (!graph || !to) return null;
    const bar = `bar ${barAt(graph.timeline.meter, to.beat).bar + 1}`;
    if (to.kind === "lane") {
      const t = graph.timeline.tracks.find((x) => x.module === to.module);
      return `${moduleName({ key: to.module, label: t?.label })} · ${bar}`;
    }
    return files.length > 1 ? `${files.length} new audio tracks · ${bar}` : `New audio track · ${bar}`;
  },
  where(t) {
    if (t.kind === "sampler") return t;
    const to = audioTarget(t);
    if (!to) return { kind: "none" };
    return to.kind === "lane" ? { kind: "lane", module: to.module, lane: to.lane, beat: to.beat } : to;
  },
  drop(files, t) {
    if (t.kind === "sampler") return void dropOnSampler(files, t);
    const to = audioTarget(t);
    if (!to) return;
    const incoming = files.flatMap((f): Incoming[] => (f.path ? [{ name: f.name, path: f.path }] : f.file ? [{ name: f.name, file: f.file }] : []));
    void bringIn(incoming, to);
  },
});
