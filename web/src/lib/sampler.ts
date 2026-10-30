// A sampler's file (track ab): a WAV dropped on a Sampler's card in Device plays in it. The server
// copies the file into the song's `assets/` (create-only) and writes `sample: "kick.wav"` into the
// call, as one undo step ("Undo setting Sampler to kick.wav"). In the app the file goes as a path;
// in a browser, as bytes. The drop itself is the audio files' (audioImport.ts): over a Sampler's
// card, `drop.ts` names it as the target.

import { graphApi, lockReason, readJson } from "./graph";
import type { DropFile } from "./drop";
import { token } from "./native";
import { useStore } from "./store";
import { undoAction } from "../components/GraphCanvas";

declare module "./graph" {
  interface ViewNode {
    /** A Sampler's card: the files it plays (none yet, one, one per zone, or its regions' files), and for regions what it plays in words ("5 recordings, 2 layers, round robin of 2"). */
    sampler?: { files: string[]; regions?: string };
  }
}

/** A drop on a Sampler's card: the call, and the card's title ("Sampler"). */
export type SamplerTarget = { kind: "sampler"; call: string; title: string };

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

/** An error in words: the server's refusal, without "edit rejected" in front. */
const words = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

/** What dropping `files` on a Sampler's card says while they are dragged. */
export function samplerHint(files: DropFile[], t: SamplerTarget): string {
  return files.length > 1 ? `${t.title} plays one WAV: drop one` : `Play it in ${t.title}`;
}

/** `files` dropped on a Sampler's card: the one WAV comes in, and the Sampler plays it. A toast with Undo says so; a refusal says why. */
export async function dropOnSampler(files: DropFile[], t: SamplerTarget) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  if (files.length !== 1) return void toast(`${t.title} plays one WAV file: drop one at a time.`);
  const why = lockReason(graph, connected);
  if (why) return void toast(`${why}.`);
  const [f] = files;
  try {
    const at = graph.view.revision;
    let res: Response;
    if (f.path) {
      const body = JSON.stringify({ session: graph.session, at, call: t.call, file: f.path });
      res = await fetch("/api/graph/sample", { method: "POST", headers: { "content-type": "application/json", ...auth }, body });
    } else if (f.file) {
      const q = `session=${graph.session}&at=${at}&call=${encodeURIComponent(t.call)}&name=${encodeURIComponent(f.name)}`;
      res = await fetch(`/api/graph/sample?${q}`, { method: "POST", headers: { "content-type": "application/octet-stream", ...auth }, body: f.file });
    } else return;
    const json = await readJson(res);
    if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
    const fresh = await graphApi.get();
    useStore.getState().set({ graph: fresh });
    toast(`${t.title} plays ${json.asset}`, undoAction(graph.session, json.step));
  } catch (error) {
    toast(`Nothing was changed: ${words(error)}`);
  }
}
