// Export Audio: the app renders the open song to a WAV file on its own thread, and says
// how it's going with `export` events (see crates/server/src/export.rs). Each names the song
// and its folder: the export runs on when another song is opened, and stays that song's.

import { create } from "zustand";
import { graphApi, projectApi, tilde } from "./graph";
import { isMac, native, nativeAction } from "./native";
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
 * Export the open song. In the app, a save dialog picks the file first (it starts in the
 * songs folder's Exports); in a browser it goes straight there.
 */
export async function exportSong() {
  const { toast, graph } = useStore.getState();
  if (!graph) return;
  const running = useExport.getState();
  if (running.progress !== null) {
    return toast(running.dir === graph.dir ? "The song is already being exported." : `“${running.song}” is still being exported. Export this song when it's done.`);
  }
  try {
    let path: string | undefined;
    if (native) {
      const picked = await nativeAction("export_path");
      if (typeof picked !== "string") return; // cancelled
      path = picked;
    }
    useExport.setState({ progress: 0, song: graph.name, dir: graph.dir });
    await graphApi.export(path);
  } catch (error) {
    useExport.setState(idle);
    toast(`Couldn't export the song: ${message(error)}`);
  }
}

/** The home folder, so the browser's toast can say `~/Music/music2/Exports`. Asked once. */
let home: Promise<string | null> | null = null;
const homeDir = () => (home ??= projectApi.catalog().then((c) => c.home, () => null));

/** One of an export's problems, as the server sends it. */
type Problem = { code: string; message: string };

/**
 * What an export's problems mean for its file, in plain words, or null with none: a plugin that
 * isn't installed plays nothing in it (FMT-18a; the problems list names it), and any other says
 * its own message (an instance the export couldn't borrow, RENDER-1).
 */
export function exportNotes(problems: Problem[]): string | null {
  const missing = problems.filter((p) => p.code === "plugin.missing").length;
  const said = problems.filter((p) => p.code !== "plugin.missing").map((p) => p.message);
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
  if (msg.error) return toast(`Couldn't export “${msg.song ?? "the song"}”: ${msg.error}`);
  if (!msg.done || !msg.path) return;
  const slash = msg.path.lastIndexOf("/");
  const [folder, file] = [tilde(msg.path.slice(0, slash), native ? null : await homeDir()), msg.path.slice(slash + 1)];
  const s = Math.round(msg.seconds ?? 0);
  // Above 0 dB a WAV file can't hold it: the loudest moments are cut flat.
  const loud = (msg.peak_db ?? 0) > 0.05 ? " It's louder than a file can hold, so the loudest moments are clipped." : "";
  const text = `Exported ${file} (${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")})`;
  const reveal = () => void nativeAction("reveal_file", msg.path).catch((error) => toast(message(error)));
  if (native) toast(`${text}.${loud}`, { label: isMac ? "Show in Finder" : "Show in Folder", run: reveal });
  else toast(`${text} to ${folder}.${loud}`);
  // What the file carries that the user should know, after it.
  const notes = exportNotes(msg.problems ?? []);
  if (notes) toast(notes);
}
