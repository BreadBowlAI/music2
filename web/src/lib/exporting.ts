// Export Audio: the app renders the open song to a WAV file on its own thread, and says
// how it's going with `export` events (see crates/server/src/export.rs). Each names the song
// and its folder: the export runs on when another song is opened, and stays that song's.

import { create } from "zustand";
import { graphApi, projectApi, tilde, type GraphState } from "./graph";
import { native, nativeAction, revealLabel } from "./native";
import { listed, listenedWords, listening, muteInSongCall, setMute, type Listening } from "./overlays";
import { useStore } from "./store";

/** How far along the export is (0..1), or null when none is running, and the song it is of. */
export const useExport = create<{ progress: number | null; song: string | null; dir: string | null }>(() => ({
  progress: null,
  song: null,
  dir: null,
}));

const idle = { progress: null, song: null, dir: null };

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * The export's question (EX-3, design-8b §6.6), while mutes or solos change what you hear: the file
 * has every track, so say so before it is made. Null while nothing asks.
 */
export const useExportAsk = create<{ ask: Listening | null }>(() => ({ ask: null }));

/**
 * What each song's running export's done toast adds, by its folder: what listening changed (null:
 * the server's words; "": nothing, the tracks are muted in the song now), and what stays muted in the song.
 */
const notes = new Map<string, { listened: string | null; kept: string | null }>();

/**
 * Export the open song. While mutes or solos change what you hear, ask first (ExportAsk); else go
 * on: in the app, a save dialog picks the file first (it starts in the songs folder's Exports); in a
 * browser it goes straight there.
 */
export async function exportSong() {
  const graph = useStore.getState().graph;
  if (!graph || busy(graph)) return;
  const ask = listening(graph);
  if (ask) return useExportAsk.setState({ ask });
  await exportNow();
}

/** An export already running: say so, and start none. */
function busy(graph: GraphState) {
  const running = useExport.getState();
  if (running.progress === null) return false;
  useStore.getState().toast(running.dir === graph.dir ? "The song is already being exported." : `“${running.song}” is still being exported. Export this song when it's done.`);
  return true;
}

/** The file to export to: in the app, what its save dialog picked (null: cancelled); in a browser, the songs folder's Exports (undefined). */
async function where(): Promise<string | null | undefined> {
  if (!native) return undefined;
  const picked = await nativeAction("export_path");
  return typeof picked === "string" ? picked : null;
}

/** Export as the song is, whatever is muted while listening ("Export anyway"). */
export async function exportNow() {
  const { toast, graph } = useStore.getState();
  if (!graph || busy(graph)) return;
  try {
    const path = await where();
    if (path === null) return;
    const l = listening(graph);
    await start(graph, path, { listened: l ? listenedWords(l) : null, kept: null });
  } catch (error) {
    useExport.setState(idle);
    toast(`Couldn't export the song: ${message(error)}`);
  }
}

/**
 * "Mute Kick in the song, then export": once the file is picked, the tracks muted while listening
 * are muted in the song (one undo step), their listening mutes go, and the export starts. Its done
 * toast says they stay muted in the song.
 */
export async function muteThenExport(keys: string[], names: string[]) {
  const { toast, graph } = useStore.getState();
  if (!graph || busy(graph)) return;
  try {
    const path = await where();
    if (path === null) return;
    await muteInSongCall(graph.session, keys, true);
    setMute(keys, false);
    await start(graph, path, { listened: "", kept: `${listed(names)} ${names.length > 1 ? "stay" : "stays"} muted in the song.` });
  } catch (error) {
    useExport.setState(idle);
    toast(`Couldn't export the song: ${message(error).replace(/^edit rejected: /, "")}`);
  }
}

/** Start the export of `graph`'s song to `path` (undefined: the songs folder's Exports), keeping what its done toast adds. */
async function start(graph: GraphState, path: string | undefined, note: { listened: string | null; kept: string | null }) {
  useExport.setState({ progress: 0, song: graph.name, dir: graph.dir });
  notes.set(graph.dir, note);
  await graphApi.export(path);
}

/** The home folder, so the browser's toast can say `~/Music/music2/Exports`. Asked once. */
let home: Promise<string | null> | null = null;
const homeDir = () => (home ??= projectApi.catalog().then((c) => c.home, () => null));

/** One of an export's problems, as the server sends it. */
type Problem = { code: string; message: string };

/**
 * What an export's problems mean for its file, in plain words, or null with none: a plugin that
 * isn't installed plays nothing in it (FMT-18a; the problems list names it); mutes and solos while
 * listening are in it after all, said by the tracks' names (`listened`, Q4; "" when the tracks were
 * muted in the song for it: nothing to say); any other says its own message (an instance the
 * export couldn't borrow, RENDER-1).
 */
export function exportNotes(problems: Problem[], listened: string | null = null): string | null {
  const missing = problems.filter((p) => p.code === "plugin.missing").length;
  const said = problems
    .filter((p) => p.code !== "plugin.missing" && !(p.code === "overlays.active" && listened === ""))
    .map((p) => (p.code === "overlays.active" && listened ? listened : p.message));
  if (missing) {
    const which = missing === 1 ? "A plugin isn't installed, so the export plays without it" : `${missing} plugins aren't installed, so the export plays without them`;
    said.unshift(`${which} (the problems list says which).`);
  }
  return said.length ? said.join(" ") : null;
}

/** An `export` event from the server: progress, then the finished file or why it failed. */
export async function onExportEvent(msg: {
  song?: string;
  dir?: string;
  progress?: number;
  done?: boolean;
  path?: string;
  seconds?: number;
  peak_db?: number;
  error?: string;
  problems?: Problem[];
}) {
  const toast = useStore.getState().toast;
  if (typeof msg.progress === "number") return useExport.setState({ progress: msg.progress, song: msg.song ?? null, dir: msg.dir ?? null });
  useExport.setState(idle);
  const note = (msg.dir && notes.get(msg.dir)) || { listened: null, kept: null };
  if (msg.dir) notes.delete(msg.dir);
  if (msg.error) return toast(`Couldn't export “${msg.song ?? "the song"}”: ${msg.error}`);
  if (!msg.done || !msg.path) return;
  const slash = msg.path.lastIndexOf("/");
  const [folder, file] = [tilde(msg.path.slice(0, slash), native ? null : await homeDir()), msg.path.slice(slash + 1)];
  const s = Math.round(msg.seconds ?? 0);
  // Above 0 dB a WAV file can't hold it: the loudest moments are cut flat.
  const loud = (msg.peak_db ?? 0) > 0.05 ? " It's louder than a file can hold, so the loudest moments are clipped." : "";
  const text = `Exported ${file} (${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")})`;
  const reveal = () => void nativeAction("reveal_file", msg.path).catch((error) => toast(message(error)));
  // What the file carries that the user should know goes in the same toast, after it: a second
  // would replace the first, and its Show action with it (one toast at a time, taste.md §2.14).
  const said = exportNotes(msg.problems ?? [], note.listened);
  const after = `${loud}${said ? ` ${said}` : ""}${note.kept ? ` ${note.kept}` : ""}`;
  if (native) toast(`${text}.${after}`, { label: revealLabel, run: reveal });
  else toast(`${text} to ${folder}.${after}`);
}
