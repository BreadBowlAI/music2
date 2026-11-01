// What the musician looks at, for agents beside them (AGENT-6): the focus record.
//
// The page posts it to the server 200 ms after the last change to any of its fields, and once when
// its socket connects; `m2 focus` and `m2 watch` read it. It is attention, not song: the server
// keeps it in memory only, never in a file, a revision or undo. Its fields: the dock's view, the
// selected track, the timeline's picked clips and a marquee's bars, the roll's selected notes (by
// pitch, start and length: notes have no ids), the knob last touched, the beats the timeline shows,
// and the code view's file and cursor line. The server adds where playback is, the loop, unsaved
// code, Compare and the MIDI keyboard's target.

import { create } from "zustand";
import { useClipSelection } from "./clips";
import { token } from "./native";
import { useShell } from "./shell";
import { useStore } from "./store";
import { http } from "./http";

/** A clip as an agent names it: its track's key, its lane's key and its id. */
export interface ClipAt {
  track: string;
  lane: string;
  id: string;
}

/** The roll's selected notes, by what they are, and their clip. */
export interface FocusNotes {
  clip: ClipAt;
  notes: { pitch: number; start: number; length: number }[];
}

/** What the views report of what is looked at, beside the shell's view and selection. */
interface Looked {
  notes: FocusNotes | null;
  knob: string | null;
  seen: [number, number] | null;
  code: { file: string; line: number } | null;
}

export const useLooked = create<Looked>(() => ({ notes: null, knob: null, seen: null, code: null }));

/** A view says what it shows: the roll its selected notes, the timeline its beats, the code its line. */
export function look(p: Partial<Looked>) {
  const now = useLooked.getState() as unknown as Record<string, unknown>;
  if (Object.entries(p).some(([k, v]) => JSON.stringify(now[k]) !== JSON.stringify(v))) useLooked.setState(p);
}

/** Beats as the record says them: to the thousandth, so a scroll that moves nothing says nothing. */
const beat = (b: number) => Math.round(b * 1000) / 1000;

/** The record as it stands, or null with no song open. */
function record() {
  const graph = useStore.getState().graph;
  if (!graph) return null;
  const shell = useShell.getState();
  const sel = useClipSelection.getState();
  const l = useLooked.getState();
  return {
    session: graph.session,
    revision: graph.view.revision,
    view: shell.view,
    track: shell.selected,
    clips: sel.picked.map((c) => ({ track: c.module, lane: c.lane, id: c.id })),
    range: sel.range,
    // What a view shows only while it shows.
    notes: shell.view === "notes" ? l.notes : null,
    knob: l.knob,
    seen: l.seen ? [beat(l.seen[0]), beat(l.seen[1])] : null,
    code: shell.view === "code" ? l.code : null,
  };
}

let timer: ReturnType<typeof setTimeout> | undefined;
/** What was last posted, as sent: the same record isn't sent twice. */
let last = "";

/** Post the record now; `again`: even if it is the one last posted (the socket connected). */
export function postFocus(again = false) {
  clearTimeout(timer);
  const r = record();
  if (!r) return;
  const body = JSON.stringify(r);
  if (!again && body === last) return;
  last = body;
  // Not a write: it works in a read-only window too. A refusal (the song changed meanwhile) is
  // followed by the next change's post.
  void http("/api/focus", { method: "POST", headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) }, body }).catch(() => {});
}

/** Post the record 200 ms after the last change to it. */
const soon = () => {
  clearTimeout(timer);
  timer = setTimeout(postFocus, 200);
};

/** The knob a press or the keyboard reached last: in Device or Mix, by its id. */
function touched(e: Event) {
  const knob = e.target instanceof Element ? e.target.closest<HTMLElement>("[data-knob-id]")?.dataset.knobId : undefined;
  if (knob) look({ knob });
}

/** Follow every field of the record, posting it as it changes. Returns how to stop. */
export function followFocus(): () => void {
  const offs = [
    useShell.subscribe((s, was) => (s.view !== was.view || s.selected !== was.selected) && soon()),
    useClipSelection.subscribe(soon),
    useLooked.subscribe(soon),
    useStore.subscribe((s, was) => (s.graph?.session !== was.graph?.session || s.graph?.view.revision !== was.graph?.view.revision) && soon()),
  ];
  window.addEventListener("pointerdown", touched, true);
  window.addEventListener("focusin", touched, true);
  return () => {
    offs.forEach((off) => off());
    window.removeEventListener("pointerdown", touched, true);
    window.removeEventListener("focusin", touched, true);
    clearTimeout(timer);
  };
}
