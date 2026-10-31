// Make Your First Song (the studio plan's D3, Q47): seven steps in a card at the top of the side
// browser, each ticking itself. Four tick from what the song shows (a drum track, its notes, a
// bass, its notes); three from what is done while the guide is open (the song played, a sound
// changed, an export finished). Kept per song in `.music2/view.json` as `guide: {open, folded,
// done}`, local to this machine. A step done stays done.

import { create } from "zustand";
import { playingNow } from "./clock";
import { on } from "./events";
import type { GraphState } from "./graph";
import { getLocal, setLocal, useLocal } from "./local";
import { chord, opt } from "./native";
import { useShell } from "./shell";
import { useStore } from "./store";

export type StepId = "drums" | "beat" | "bass" | "bassline" | "listen" | "sound" | "export";

/** The guide as `view.json` keeps it. */
export interface Guide {
  open: boolean;
  /** Folded to one line: "Your first song · 3 of 7". */
  folded?: boolean;
  done: StepId[];
}

/** The steps, in order: what to do (`title`, then `hint`). Keys as this computer spells them. */
export const STEPS: { id: StepId; title: string; hint: () => string }[] = [
  { id: "drums", title: "Add drums.", hint: () => `Press ${chord("T")} and pick Drum kit.` },
  { id: "beat", title: "Make a beat.", hint: () => "Double-click its row for a clip. In Notes, double-click in the Kick and Snare rows (or press B, then click)." },
  { id: "bass", title: "Add a bass.", hint: () => `Press ${chord("T")} and pick Synth bass or 808 sub.` },
  { id: "bassline", title: "Play some bass notes.", hint: () => "Make a clip on it. In Notes, double-click a few notes." },
  { id: "listen", title: "Listen.", hint: () => "Press Space. Press it again to stop." },
  { id: "sound", title: "Change a sound.", hint: () => `Turn a knob in Device (${opt("1")}), or right-click a track, then Change instrument.` },
  { id: "export", title: "Export it.", hint: () => `Press ${chord("E", { shift: true })}. Your song becomes a file you can share.` },
];

const BASSES = new Set(["@m2/synths:synth-bass", "@m2/synths:synth-sub"]);

/** The tracks that play an instrument for which `which` holds. */
function playing(graph: GraphState, which: (component: string) => boolean) {
  return graph.view.modules.filter((m) => m.track && m.nodes.some((n) => !!n.component && which(n.component))).map((m) => m.key);
}

/** How many notes the clips of `module`'s notes lanes hold. */
function notes(graph: GraphState, module: string) {
  const t = graph.timeline.tracks.find((x) => x.module === module);
  return (t?.lanes ?? []).reduce((n, l) => n + (l.kind === "notes" ? l.clips.reduce((c, clip) => c + clip.notes.length, 0) : 0), 0);
}

/** The steps the song shows done now: a track of the Drums group and its notes, a bass and its notes. */
export function seen(graph: GraphState): StepId[] {
  const drums = new Set((graph.instruments ?? []).filter((i) => i.category === "Drums").map((i) => i.component));
  const kits = playing(graph, (c) => drums.has(c));
  const basses = playing(graph, (c) => BASSES.has(c));
  const out: StepId[] = [];
  if (kits.length) out.push("drums");
  if (kits.some((m) => notes(graph, m) >= 4)) out.push("beat");
  if (basses.length) out.push("bass");
  if (basses.some((m) => notes(graph, m) >= 3)) out.push("bassline");
  return out;
}

/** The open song's guide, or null when it has none. */
export const useGuide = () => useLocal<Guide | null>("guide", null);

const now = () => getLocal<Guide | null>("guide", null);

/** Steps done: added to the guide's, in order, if it is open. */
function tick(ids: StepId[]) {
  const g = now();
  if (!g?.open) return;
  const done = STEPS.map((s) => s.id).filter((id) => g.done.includes(id) || ids.includes(id));
  if (done.length !== g.done.length) setLocal("guide", { ...g, done });
}

/** Help ▸ Make Your First Song: the guide shows, in the side browser (shown for it), its ticks as they were. */
export function openGuide() {
  const g = now();
  setLocal("guide", { open: true, folded: false, done: g?.done ?? [] });
  useShell.getState().setBrowser(true);
  const graph = useStore.getState().graph;
  if (graph) tick(seen(graph));
}

/** × on the card: it goes, its ticks kept for when it opens again. */
export function closeGuide() {
  const g = now();
  if (g) setLocal("guide", { ...g, open: false });
}

/** The chevron: the card as one line, or its steps. */
export function foldGuide(folded: boolean) {
  const g = now();
  if (g) setLocal("guide", { ...g, folded });
}

/** Something the guide can't see in the song was done: a sound changed (a knob, an instrument). */
export function guideDid(id: "sound") {
  tick([id]);
}

/** The song was saved as a MIDI file at `path`: Export it ticks, as an audio export does (the review's F6). */
export function guideExported(path: string) {
  useExported.setState({ path });
  tick(["export"]);
}

/** The last file an export made, while this window has been open: the done line names it. */
export const useExported = create<{ path: string | null }>(() => ({ path: null }));

// What ticks the steps, for as long as the page is open: the song as it changes, the transport
// (as the 200 ms status polls set the clock), and an export of this song finishing.
useStore.subscribe((s, before) => {
  if (!s.graph || s.graph === before.graph) return;
  // A song opened with its guide open: the browser shows, for the card.
  if (s.graph.session !== before.graph?.session && now()?.open) useShell.getState().setBrowser(true);
  tick(seen(s.graph));
});
setInterval(() => {
  if (playingNow()) tick(["listen"]);
}, 250);
on("export", (msg: { done?: boolean; dir?: string; path?: string }) => {
  const graph = useStore.getState().graph;
  if (!msg.done || !msg.path || !graph || (msg.dir && msg.dir !== graph.dir)) return;
  useExported.setState({ path: msg.path });
  tick(["export"]);
});
