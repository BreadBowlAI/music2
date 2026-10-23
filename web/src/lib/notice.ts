// Changes made elsewhere, shown as they land (AGENT-5): a quiet notice with Undo, and a glow on
// what changed.
//
// After each change that is an undo step, the server tells every window with a `step` event: its
// id, who made it (`page`; `agent`, by its name in `by`; or `outside`: a file saved in another
// program), its name, and the tracks, lanes and knobs it changed. For a step that isn't this
// page's own, the page shows one notice ("Claude: brighter hats · Undo", "Changed outside the
// app: the code in tracks/bass.ts · Undo"), a newer one replacing it,
// and outlines what changed until the next click or key press. The notice's Undo sends that step's
// id, so it can never take back another step: once a newer change came, it says so instead. It
// offers Undo only while undo can take the step back.

import { create } from "zustand";
import { useStore } from "./store";

/** A `step` event, as the server sends it. */
export interface StepEvent {
  session: number;
  step: number;
  made: number;
  source: "page" | "agent" | "outside";
  /** An agent's name, as it gave it: plain text (AGENT-10). */
  by?: string | null;
  name: string;
  files: string[];
  /** Whether undo can take it back now. */
  undoable: boolean;
  tracks: string[];
  lanes: [string, string][];
  knobs: string[];
}

/** The notice showing: the newest step not made here, and how many came while it showed. */
export interface Notice {
  /** Counts up with each notice, so a newer one starts its own time. */
  id: number;
  session: number;
  step: number;
  /** "Changed outside the app", or an agent's name: "Claude". */
  who: string;
  name: string;
  /** Steps not made here since the notice first showed, this one included. */
  count: number;
  /** Undo could take it back when it landed. */
  undoable: boolean;
  /** A newer change came after it: its Undo would take back something else. */
  newer: boolean;
  /** The server's words when its Undo was refused. */
  refused?: string;
}

/** What glows: by track key, `module/lane`, and knob id. */
export interface Glow {
  session: number;
  tracks: Set<string>;
  lanes: Set<string>;
  knobs: Set<string>;
}

interface Noticed {
  notice: Notice | null;
  glow: Glow | null;
}

export const useNotice = create<Noticed>(() => ({ notice: null, glow: null }));

/** Who made a step, as a notice says it: an agent by its name, as text. */
export function who(source: string, by?: string | null): string {
  if (source === "agent") return by || "agent";
  return source === "outside" ? "Changed outside the app" : "Changed";
}

/** The notice's words: "Changed outside the app: the code in song.ts", or with a count. */
export function noticeText(n: Notice): string {
  return n.count > 1 ? `${n.who}: ${n.count} changes · newest: ${n.name}` : `${n.who}: ${n.name}`;
}

let ids = 0;

/** The server told of a new step (AGENT-5). */
export function heard(e: StepEvent) {
  if (useStore.getState().graph?.session !== e.session) return;
  const { notice } = useNotice.getState();
  if (e.source === "page") {
    // Made here: what glowed is old news, and the notice's step is no longer the newest.
    const newer = notice && notice.session === e.session && notice.step < e.step ? { ...notice, newer: true } : notice;
    return useNotice.setState({ glow: null, notice: newer });
  }
  const showing = notice && notice.session === e.session && !notice.newer && !notice.refused;
  useNotice.setState({
    notice: {
      id: ++ids,
      session: e.session,
      step: e.step,
      who: who(e.source, e.by),
      name: e.name,
      count: showing ? notice.count + 1 : 1,
      undoable: e.undoable,
      newer: false,
    },
    glow: { session: e.session, tracks: new Set(e.tracks), lanes: new Set(e.lanes.map(([m, l]) => `${m}/${l}`)), knobs: new Set(e.knobs) },
  });
}

/** The musician did something in the song: the glow ends (AGENT-5). */
export function acted() {
  if (useNotice.getState().glow) useNotice.setState({ glow: null });
}

/** Step `step` was undone or redone here: a notice of it has nothing more to offer. */
export function stepped(step: number | undefined) {
  if (step !== undefined && useNotice.getState().notice?.step === step) useNotice.setState({ notice: null, glow: null });
}

/** The notice's Undo was refused: it says why, and offers it no more. */
export function refused(id: number, why: string) {
  const { notice } = useNotice.getState();
  if (notice?.id === id) useNotice.setState({ notice: { ...notice, refused: why } });
}

export function dismissNotice(id: number) {
  if (useNotice.getState().notice?.id === id) useNotice.setState({ notice: null });
}

/** Whether a track header, a lane's clips or a knob glows. */
export const useGlows = (kind: "tracks" | "lanes" | "knobs", id: string | undefined) => useNotice((s) => !!id && !!s.glow?.[kind].has(id));

/** A song opened or closed: nothing of the last one shows. */
export function forget() {
  useNotice.setState({ notice: null, glow: null });
}
