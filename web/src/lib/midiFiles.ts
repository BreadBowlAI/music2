// MIDI files (studio lane C, Q37, Q38): the song, or one notes clip, saved as a MIDI file other
// music apps open. In the app a save panel picks the file first, starting at the song's new file in
// the songs folder's Exports; in a browser it goes straight there, by a free name. The file is
// written by the server (crates/server/src/routes_exchange.rs): never into the song.

import { projectApi, readJson, tilde, type ClipRef, type GraphState } from "./graph";
import { guideExported } from "./guide";
import { native, nativeAction, revealLabel, token } from "./native";
import { useStore } from "./store";

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
  return graph.timeline.tracks.find((t) => t.module === module)?.label ?? module;
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
