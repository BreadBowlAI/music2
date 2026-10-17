// Files dropped on the window (P0.4, built in lane A; Q52): one layer for the app and a browser.
// In the app, Tauri's drag-drop events give the files' paths and where the pointer is; in a
// browser, HTML5's drag events give the `File`s. Only drags that carry files are this layer's
// (D12): the side browser's instruments and effects keep their own drags.
//
// Kinds of file register what they do (`onFiles`): audio (lane A), MIDI files (lane C). Where a
// drop lands is `target(x, y)`: a lane at a bar, a track, or below the tracks. While dragging, the
// hint shows where (an outline) and says what will happen; a file nobody takes says what music2
// opens.

import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { barAt, lockReason } from "./graph";
import { native } from "./native";
import { useStore } from "./store";
import { zoomNow } from "./zoom";

/** A file being dragged or dropped: its name (none while a browser drags it), its type, and its path (the app) or itself (a browser). */
export interface DropFile {
  name: string;
  type?: string;
  path?: string;
  file?: File;
}

/** Where a drop lands (Q6): a lane at a bar, a track (not on one of its lanes), below the tracks, or nowhere. */
export type DropTarget =
  | { kind: "lane"; module: string; lane: string; beat: number }
  | { kind: "track"; module: string; beat: number }
  | { kind: "below"; beat: number }
  | { kind: "none" };

/** A kind of file the window takes: by extension (".wav") or by type ("audio/wav"). */
interface Taker {
  extensions: string[];
  types: string[];
  /** What dropping `files` at `target` does, in words; null: nothing there (it says where to drop). */
  hint: (files: DropFile[], target: DropTarget) => string | null;
  drop: (files: DropFile[], target: DropTarget) => void;
  /** Where its files would really land, for the outline: a lane of notes may mean a new track after it. */
  where?: (target: DropTarget) => DropTarget;
}

const takers: Taker[] = [];

/** Take files with `extensions` (lowercase, with the dot) or MIME `types`. Returns the unregister. */
export function onFiles(extensions: string[], types: string[], how: Pick<Taker, "hint" | "drop" | "where">) {
  const t: Taker = { extensions, types, ...how };
  takers.push(t);
  return () => void takers.splice(takers.indexOf(t), 1);
}

/**
 * Whether `t` takes `f`: by its name once it is dropped; while a browser drags it, by its type, and
 * a file whose type the browser doesn't say (or says another way) may be one: its name decides at
 * the drop. Only a type that is surely another kind (an MP3's) is refused while dragging.
 */
const takes = (t: Taker, f: DropFile) => {
  if (f.name) return t.extensions.some((x) => f.name.toLowerCase().endsWith(x));
  return !f.type || t.types.includes(f.type) || !KNOWN.some((k) => f.type!.startsWith(k));
};

/** Types that surely aren't a file music2 opens unless a kind lists them. */
const KNOWN = ["audio/", "video/", "image/", "text/", "application/pdf", "application/zip"];

/** The one kind that takes every file, or none. */
const takerOf = (files: DropFile[]) => (files.length ? takers.find((t) => files.every((f) => takes(t, f))) : undefined);

/** What music2 opens, said when a drag carries something else. MIDI files join once a drop takes them (C2b). */
export const OPENS = "music2 opens WAV files";

/** Where a drop at (`x`, `y`) on the page lands: the bar line at or before the pointer (D32). */
export function target(x: number, y: number): DropTarget {
  const ruler = document.querySelector<HTMLElement>("[data-testid=ruler]");
  const timeline = document.querySelector<HTMLElement>("[data-testid=timeline]");
  const graph = useStore.getState().graph;
  if (!ruler || !timeline || !graph) return { kind: "none" };
  const box = timeline.getBoundingClientRect();
  const origin = ruler.getBoundingClientRect().left;
  if (x < origin || x > box.right || y < ruler.getBoundingClientRect().bottom || y > box.bottom) return { kind: "none" };
  const beat = barAt(graph.timeline.meter, Math.max(0, (x - origin) / zoomNow())).start;
  const song = document.querySelector<HTMLElement>("[data-song-row]")?.getBoundingClientRect();
  if (song && y >= song.top) return { kind: "none" };
  const under = document.elementFromPoint(x, y);
  const row = under?.closest<HTMLElement>("[data-lane]");
  const track = under?.closest<HTMLElement>("[data-track]");
  if (row?.dataset.lane && track?.dataset.track !== "song") {
    const id = row.dataset.lane;
    const dot = id.indexOf(".");
    return { kind: "lane", module: id.slice(0, dot), lane: id.slice(dot + 1), beat };
  }
  if (track?.dataset.track && track.dataset.track !== "song") return { kind: "track", module: track.dataset.track, beat };
  const tracks = [...document.querySelectorAll<HTMLElement>("[data-track]")].filter((t) => t.dataset.track !== "song");
  const last = tracks.at(-1)?.getBoundingClientRect();
  return y >= (last?.bottom ?? box.top) ? { kind: "below", beat } : { kind: "none" };
}

/** The hint while files are dragged over the window: where the pointer is, what it says, and where they'd land. */
export interface Hint {
  at: { x: number; y: number };
  words: string;
  target: DropTarget;
  /** Where the outline goes: where the files would land. */
  shown?: DropTarget;
}

export const useDropHint = create<{ hint: Hint | null }>(() => ({ hint: null }));

/** What a drag of `files` at (`x`, `y`) says, and where it lands. */
function hintAt(files: DropFile[], x: number, y: number): Hint {
  const graph = useStore.getState().graph;
  const at = { x, y };
  if (!graph) return { at, words: "Open a song first", target: { kind: "none" } };
  const t = takerOf(files);
  if (!t) return { at, words: OPENS, target: { kind: "none" } };
  const why = lockReason(graph, useStore.getState().connected);
  if (why) return { at, words: why, target: { kind: "none" } };
  const where = target(x, y);
  const words = t.hint(files, where);
  return words ? { at, words, target: where, shown: t.where?.(where) ?? where } : { at, words: "Drop it on a track, or below the tracks", target: { kind: "none" } };
}

/** Files dropped at (`x`, `y`): to the kind that takes them, where they land. Nothing when the hint said so. */
function dropAt(files: DropFile[], x: number, y: number) {
  useDropHint.setState({ hint: null });
  const h = hintAt(files, x, y);
  const t = takerOf(files);
  if (!t || h.target.kind === "none") {
    // Said in a toast, so a drop that does nothing says why.
    if (files.length) useStore.getState().toast(h.words === OPENS ? `${OPENS}.` : `${h.words}.`);
    return;
  }
  t.drop(files, h.target);
}

/** Start listening: Tauri's drag events in the app, HTML5's in a browser. Returns the stop. */
export function listenForDrops(): () => void {
  if (native) {
    // Tauri gives physical pixels.
    const px = (p: { x: number; y: number }) => ({ x: p.x / (window.devicePixelRatio || 1), y: p.y / (window.devicePixelRatio || 1) });
    let files: DropFile[] = [];
    const offs = [
      listen<{ paths?: string[]; position: { x: number; y: number } }>("tauri://drag-enter", (e) => {
        files = (e.payload.paths ?? []).map((p) => ({ name: p.split(/[\\/]/).pop() ?? p, path: p }));
        // A drag that carries no files isn't this layer's (D12).
        if (!files.length) return;
        const { x, y } = px(e.payload.position);
        useDropHint.setState({ hint: hintAt(files, x, y) });
      }),
      listen<{ position: { x: number; y: number } }>("tauri://drag-over", (e) => {
        if (!files.length) return;
        const { x, y } = px(e.payload.position);
        useDropHint.setState({ hint: hintAt(files, x, y) });
      }),
      listen<{ paths?: string[]; position: { x: number; y: number } }>("tauri://drag-drop", (e) => {
        const dropped = (e.payload.paths ?? []).map((p) => ({ name: p.split(/[\\/]/).pop() ?? p, path: p }));
        files = [];
        if (!dropped.length) return;
        const { x, y } = px(e.payload.position);
        dropAt(dropped, x, y);
      }),
      listen("tauri://drag-leave", () => {
        files = [];
        useDropHint.setState({ hint: null });
      }),
    ];
    return () => offs.forEach((p) => void p.then((un) => un()));
  }
  const carries = (e: DragEvent) => !!e.dataTransfer && [...e.dataTransfer.types].includes("Files");
  /** What a browser lets the page see while dragging: each item's type, no names. */
  const dragged = (e: DragEvent): DropFile[] => [...(e.dataTransfer?.items ?? [])].filter((i) => i.kind === "file").map((i) => ({ name: "", type: i.type }));
  let depth = 0;
  const over = (e: DragEvent) => {
    if (!carries(e)) return;
    e.preventDefault();
    const h = hintAt(dragged(e), e.clientX, e.clientY);
    if (e.dataTransfer) e.dataTransfer.dropEffect = h.target.kind === "none" ? "none" : "copy";
    useDropHint.setState({ hint: h });
  };
  const enter = (e: DragEvent) => {
    if (!carries(e)) return;
    depth++;
    over(e);
  };
  const leave = (e: DragEvent) => {
    if (!carries(e)) return;
    depth = Math.max(0, depth - 1);
    if (depth === 0) useDropHint.setState({ hint: null });
  };
  const drop = (e: DragEvent) => {
    if (!carries(e)) return;
    e.preventDefault();
    depth = 0;
    const files = [...(e.dataTransfer?.files ?? [])].map((file) => ({ name: file.name, type: file.type, file }));
    dropAt(files, e.clientX, e.clientY);
  };
  const pairs = [["dragenter", enter], ["dragover", over], ["dragleave", leave], ["drop", drop]] as const;
  for (const [t, f] of pairs) window.addEventListener(t, f);
  return () => {
    for (const [t, f] of pairs) window.removeEventListener(t, f);
  };
}
