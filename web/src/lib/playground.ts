// The Playground (VER-6): which notes clip it is open on, the loop it puts back, and its acts.
//
// It is a view of one clip's versions, at the left of the roll in Notes. Opening it remembers the
// loop and sets it to the clip's bars, on; closing puts the loop back. Neither presses play or
// stop. It closes by itself when the dock stops showing its clip: another clip or track selected,
// another view, the dock hidden, the clip gone, another song. Every act is the one `versions` clip
// gesture (VER-3), one undo step each; the list is read from the song, so what an agent adds shows
// as it lands.

import { create } from "zustand";
import { refIn, send, useClipSelection, type Gesture, type ServerRef } from "./clips";
import { setClock } from "./clock";
import { dockView, findNoteClip, graphApi, sameClip, type ClipRef, type GraphState, type GraphStatus, type Loop, type NoteClip, type Version } from "./graph";
import { useShell } from "./shell";
import { useStatus } from "./status";
import { useStore } from "./store";

/** What a clip's playing notes are named before it has versions (the server's `ORIGINAL`). */
export const ORIGINAL = "original";
/** The longest name a version takes (the server's `VERSION_NAME`). */
const NAME_MAX = 40;

interface Playground {
  /** The clip it is open on, or null: closed. */
  clip: ClipRef | null;
  /** The loop as it was when it opened (null: none was set), to put back. */
  loop: Loop | null;
  /** The version last asked to play, until the song has that step (its revision, once known): a second → goes on from it. */
  aim: { name: string; revision: number | null } | null;
}

export const usePlayground = create<Playground>(() => ({ clip: null, loop: null, aim: null }));

/** Whether the Playground is open on `ref`. */
export const playgroundOn = (ref: ClipRef | null | undefined) => sameClip(usePlayground.getState().clip, ref) && !!ref;

/** A clip's versions as the list shows them: a clip with none has `original` alone, which plays. */
export const versionsOf = (clip: NoteClip): Version[] => (clip.versions?.length ? clip.versions : [{ name: ORIGINAL }]);
/** How many of a clip's versions wait: what its mark on the timeline counts. */
export const waiting = (clip: { versions?: Version[] }) => Math.max((clip.versions?.length ?? 0) - 1, 0);
/** The name of the version that plays. */
export const playingOf = (clip: NoteClip) => versionsOf(clip).find((v) => !v.notes)?.name ?? ORIGINAL;

const toast = (words: string) => useStore.getState().toast(words);
const show = (graph: GraphState) => (s: GraphStatus) => {
  setClock(s, graph.timeline.tempo);
  useStatus.setState(s, true);
};

/**
 * Open the Playground on `ref`, a notes clip: select it, show Notes, remember the loop and set it
 * to the clip's bars, on. It doesn't press play.
 */
export async function openPlayground(ref: ClipRef) {
  const graph = useStore.getState().graph;
  const clip = graph && findNoteClip(graph.timeline, ref);
  if (!graph || !clip) return;
  if (playgroundOn(ref)) return;
  closePlayground();
  const shell = useShell.getState();
  shell.select(ref.module, ref);
  shell.setView("notes");
  useClipSelection.getState().only([ref]);
  // The loop as the server has it now: the status the window holds may be a poll behind.
  const loop = await graphApi.status().then((s) => s.loop, () => useStatus.getState().loop);
  usePlayground.setState({ clip: ref, loop, aim: null });
  await graphApi.loop({ start: clip.start, end: clip.start + clip.length, on: true }).then(show(graph), (e) => toast(e instanceof Error ? e.message : String(e)));
}

/** Close it, and put the loop back as it was. `song`: false when its song is gone, and the loop with it. */
export function closePlayground(song = true) {
  const { clip, loop } = usePlayground.getState();
  if (!clip) return;
  usePlayground.setState({ clip: null, loop: null, aim: null });
  const graph = useStore.getState().graph;
  if (!song || !graph) return;
  // No loop was set before: there is no way to unset one, so it is turned off.
  void graphApi.loop(loop ? { start: loop.start, end: loop.end, on: loop.on } : { on: false }).then(show(graph), () => {});
}

/**
 * ⌥P and the clip menu's row: close it if it is open; else open it on the one selected notes clip
 * (the timeline's, or the one Notes shows). With none, say what it needs.
 */
export function togglePlayground() {
  if (usePlayground.getState().clip) return closePlayground();
  const graph = useStore.getState().graph;
  if (!graph) return;
  const shell = useShell.getState();
  const picked = useClipSelection.getState().picked;
  const ref = picked.length === 1 ? picked[0] : picked.length === 0 && shell.module ? shell.clips[shell.module] : undefined;
  if (!ref || !findNoteClip(graph.timeline, ref)) return void toast("The Playground opens on one notes clip: select one first.");
  void openPlayground(ref);
}

/** Whether the dock still shows the Playground's clip. */
function shown() {
  const { clip } = usePlayground.getState();
  const graph = useStore.getState().graph;
  if (!clip || !graph) return false;
  const shell = useShell.getState();
  return shell.dock && !!shell.module && dockView(graph, shell.view, shell.module) === "notes" && sameClip(shell.clips[shell.module], clip) && !!findNoteClip(graph.timeline, clip);
}

// It closes when the dock stops showing its clip, whatever took it away.
let session: number | undefined;
function follow() {
  const graph = useStore.getState().graph;
  const same = graph?.session === session;
  session = graph?.session;
  const { clip, aim } = usePlayground.getState();
  if (!clip) return;
  if (!same || !graph) return closePlayground(false);
  if (!shown()) return closePlayground();
  // The version asked for plays now: the next flip goes from what the song says.
  const live = findNoteClip(graph.timeline, clip);
  if (aim && ((live && playingOf(live) === aim.name) || (aim.revision !== null && graph.view.revision >= aim.revision))) usePlayground.setState({ aim: null });
}
useStore.subscribe((s, was) => s.graph !== was.graph && follow());
useShell.subscribe((s, was) => (s.dock !== was.dock || s.view !== was.view || s.module !== was.module || s.clips !== was.clips) && follow());

// ---------------------------------------------------------------- the acts (VER-3)

type Act = Extract<Gesture, { op: "versions" }> extends infer G ? (G extends unknown ? Omit<G, "op" | "clip"> : never) : never;

/** The open clip, as the song has it now. */
function open(): { graph: GraphState; ref: ClipRef; clip: NoteClip; at: ServerRef } | null {
  const ref = usePlayground.getState().clip;
  const graph = useStore.getState().graph;
  const clip = ref && graph && findNoteClip(graph.timeline, ref);
  const at = ref && graph && refIn(graph.timeline, ref);
  if (!ref || !graph || !clip || !at) return null;
  // By its id, as an agent names it: a flip right after another isn't refused over the first one's change.
  if (at.id) delete at.sha;
  return { graph, ref, clip, at };
}

/** One act, one undo step named `step`: the server's answer, or (refused, its words in a toast) null. */
async function act(a: Act, step: string) {
  const o = open();
  if (!o) return null;
  try {
    return await send(o.graph, { op: "versions", clip: o.at, ...a } as Gesture, { name: step });
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e));
    return null;
  }
}

/** Play `name`: its notes become the clip's. */
export async function playVersion(name: string) {
  const o = open();
  if (!o || (usePlayground.getState().aim?.name ?? playingOf(o.clip)) === name) return;
  const aim = { name, revision: null as number | null };
  usePlayground.setState({ aim });
  const done = await act({ act: "play", name }, `playing version ${name}`);
  if (usePlayground.getState().aim !== aim) return;
  // Done, the song's next state ends the aim (`follow`), unless it has come already; refused, it ends here.
  const graph = useStore.getState().graph;
  usePlayground.setState({ aim: done && graph && graph.view.revision < done.revision ? { name, revision: done.revision } : null });
}

/** ← and →: play the version `by` places on in the list, stopping at its ends. */
export function flip(by: -1 | 1) {
  const o = open();
  if (!o) return;
  const all = versionsOf(o.clip);
  const now = usePlayground.getState().aim?.name ?? playingOf(o.clip);
  const next = all[all.findIndex((v) => v.name === now) + by];
  if (next) void playVersion(next.name);
}

/** Star `name`, or take its star off; with no name, the one that plays (S). */
export function starVersion(name?: string) {
  const o = open();
  if (!o?.clip.versions?.length) return;
  const v = o.clip.versions.find((x) => x.name === (name ?? playingOf(o.clip)));
  if (v) void act({ act: "star", name: v.name, star: !v.star }, `${v.star ? "taking the star off" : "starring"} version ${v.name}`);
}

export const renameVersion = (name: string, to: string) => act({ act: "rename", name, to }, `renaming version ${name}`);
export const removeVersion = (name: string) => act({ act: "remove", name }, `removing version ${name}`);

/** A name for a copy of `name` that no version of `taken` has: "busy copy", "busy copy 2". */
export function copyName(name: string, taken: readonly string[]) {
  for (let n = 1; ; n++) {
    const end = n === 1 ? " copy" : ` copy ${n}`;
    const made = `${[...name].slice(0, NAME_MAX - end.length).join("").trimEnd()}${end}`;
    if (!taken.includes(made)) return made;
  }
}

/** Duplicate: the playing version's notes under a new name, which then plays. One undo step. */
export function duplicateVersion() {
  const o = open();
  if (!o) return;
  const name = copyName(playingOf(o.clip), versionsOf(o.clip).map((v) => v.name));
  void act({ act: "add", versions: [{ name, notes: o.clip.notes }], play: true }, `duplicating version ${playingOf(o.clip)}`);
}

/** Keep this one: the others are removed, and the Playground closes. */
export async function keepVersion() {
  const o = open();
  if (!o) return;
  if (await act({ act: "keep" }, `keeping version ${playingOf(o.clip)}`)) closePlayground();
}
