// A recording on a card (CARD-5; track ab's Sampler first): a WAV or FLAC dropped on it plays there.
// The server copies the file into the song's `assets/` (create-only), as Import Audio does, and
// writes its name into the call (`sample: "kick.wav"`), as one undo step ("Undo setting Sampler to
// kick.wav"). In the app the file goes as a path; in a browser, as bytes. The drop itself is the
// audio files' (audioImport.ts): over a recording, or a card with one, `drop.ts` names it as the
// target.

import { graphApi, lockReason, readJson } from "./graph";
import type { DropFile } from "./drop";
import { token } from "./native";
import { useStore } from "./store";
import { undoAction } from "../components/GraphCanvas";

/** A drop on a recording: the call, the setting it is (`sample`), and the card's title ("Sampler"). */
export type RecordingTarget = { kind: "recording"; call: string; setting: string; title: string };

/** The recording under `el`, or the card it is the one recording of (`data-recording-call`). */
export function recordingAt(el: Element | null): RecordingTarget | null {
  const at = el?.closest<HTMLElement>("[data-recording-call]");
  if (!at?.dataset.recordingCall || !at.dataset.recordingSetting) return null;
  return { kind: "recording", call: at.dataset.recordingCall, setting: at.dataset.recordingSetting, title: at.dataset.recordingTitle || "it" };
}

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

/** An error in words: the server's refusal, without "edit rejected: " in front. */
const words = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

/** What dropping `files` on a recording says while they are dragged. */
export function recordingHint(files: DropFile[], t: RecordingTarget): string {
  return files.length > 1 ? `${t.title} plays one file: drop one` : `Play it in ${t.title}`;
}

/** `files` dropped on a recording: the one WAV comes in, and the call plays it. A toast with Undo says so; a refusal says why. */
export async function dropOnRecording(files: DropFile[], t: RecordingTarget) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  if (files.length !== 1) return void toast(`${t.title} plays one WAV or FLAC file: drop one at a time.`);
  const why = lockReason(graph, connected);
  if (why) return void toast(`${why}.`);
  const [f] = files;
  try {
    const at = graph.view.revision;
    let res: Response;
    if (f.path) {
      const body = JSON.stringify({ session: graph.session, at, call: t.call, setting: t.setting, file: f.path });
      res = await fetch("/api/graph/sample", { method: "POST", headers: { "content-type": "application/json", ...auth }, body });
    } else if (f.file) {
      const q = `session=${graph.session}&at=${at}&call=${encodeURIComponent(t.call)}&setting=${encodeURIComponent(t.setting)}&name=${encodeURIComponent(f.name)}`;
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
