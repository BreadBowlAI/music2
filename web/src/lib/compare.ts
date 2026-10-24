// The Compare key, V (D2; AGENT-3): hear before and after the newest change an agent or another
// program made, with one key.
//
// V undoes that step (`compare: true`), and the pill in the top bar says "Before: Claude: brighter
// hats"; V again redoes it: "After". Each flip is an undo or a redo, heard as one is. While the pill
// is open, agents wait: no agent's step can clear redo and take After away. Esc, or any other
// action but listening (play, stop, the loop, a seek), closes the pill and keeps what plays: on
// Before, the change stays undone, and Redo has it until the next change. A file written directly
// while it is open closes it, saying so.

import { create } from "zustand";
import type { StepName } from "./history";
import { token } from "./native";
import { stepped } from "./notice";
import { useStore } from "./store";

/** What the pill compares, as the server says it in the state. */
export interface Comparing {
  step: number;
  /** Showing the step redone; else undone. */
  after: boolean;
  /** As Undo says it: "Claude: brighter hats". */
  name: string;
}

declare module "./graph" {
  interface GraphState {
    comparing?: Comparing | null;
  }
}

export const useCompare = create<{ pill: (Comparing & { session: number }) | null }>(() => ({ pill: null }));

const headers = () => ({ "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) });

/** What a flip answers: the step it took back, the tops after it, and what the pill compares. */
interface Flipped {
  step?: number;
  undo_name?: StepName | null;
  redo_name?: StepName | null;
  comparing?: Comparing | null;
  /** A refusal's newest step, as the server names it (null: nothing to undo). */
  newest?: StepName | null;
}

/** V's flip: an undo (Before) or a redo (After) of `step`, which the server keeps as compared. */
async function flip(session: number, step: number, redo: boolean): Promise<{ status: number; error?: string; json: Flipped }> {
  const res = await fetch(`/api/graph/${redo ? "redo" : "undo"}`, { method: "POST", headers: headers(), body: JSON.stringify({ session, step, compare: true }) });
  const json = await res.json().catch(() => ({}));
  return { status: res.status, error: json.error, json };
}

/**
 * V: open the pill on the newest change an agent or another program made, or flip it between
 * Before and After. The server decides what the newest change is: a window a moment behind (a
 * change that landed just now) is told, and V goes on with the step it names, once.
 */
export async function compare() {
  const { graph, toast } = useStore.getState();
  if (!graph) return;
  const pill = useCompare.getState().pill;
  if (pill?.session === graph.session) {
    const r = await flip(graph.session, pill.step, !pill.after);
    if (r.status !== 200) {
      useCompare.setState({ pill: null });
      return toast(r.error ?? "Compare stopped: that change isn't the newest any more.");
    }
    tops(graph.session, r.json);
    return useCompare.setState({ pill: { ...pill, after: !pill.after } });
  }
  let step = graph.undo_name?.step ?? 0;
  let r = await flip(graph.session, step, false);
  const newest = r.json.newest;
  if (r.status === 409 && newest?.step !== undefined && newest.step !== step && newest.source !== "page") {
    step = newest.step;
    r = await flip(graph.session, step, false);
  }
  if (r.status !== 200) return toast(r.status === 409 && r.json.newest === null ? "Nothing to compare: there's no change to undo." : (r.error ?? "Nothing to compare now."));
  stepped(step);
  tops(graph.session, r.json);
  useCompare.setState({ pill: { session: graph.session, step, after: false, name: r.json.comparing?.name ?? "" } });
}

/** Undo's and Redo's names after a flip, before the state is read again. */
function tops(session: number, answer: { undo_name?: StepName | null; redo_name?: StepName | null }) {
  const { graph, set } = useStore.getState();
  if (graph?.session === session && answer.undo_name !== undefined) set({ graph: { ...graph, undo_name: answer.undo_name, redo_name: answer.redo_name } });
}

/** Esc, or any other action: the pill closes, keeping what plays. */
export function closeCompare() {
  const pill = useCompare.getState().pill;
  if (!pill) return;
  useCompare.setState({ pill: null });
  void fetch("/api/graph/compare", { method: "POST", headers: headers(), body: JSON.stringify({ session: pill.session, open: false }) }).catch(() => {});
}

/** The server closed it (a `compare` event): a file written directly says so. */
export function heardCompare(e: { session: number; step: number; why: string }) {
  const pill = useCompare.getState().pill;
  if (!pill || pill.session !== e.session || pill.step !== e.step) return;
  useCompare.setState({ pill: null });
  if (e.why === "outside") useStore.getState().toast("A file changed outside the app, so Compare closed. What plays stays.");
}

/** The pill as the server has it, when the window connects or another song opens. */
export function syncCompare() {
  const graph = useStore.getState().graph;
  useCompare.setState({ pill: graph?.comparing ? { session: graph.session, ...graph.comparing } : null });
}

/** A key that is no action of its own: a modifier, held. */
const MODIFIERS = new Set(["Shift", "Meta", "Alt", "Control", "CapsLock", "Fn"]);

/** Whether `t` takes typing. */
const typing = (t: EventTarget | null) => t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || (t instanceof HTMLElement && t.isContentEditable);

/** Keys that only listen (play or stop, back to the start, the loop, the metronome) and V itself. */
const LISTENING = new Set(["KeyV", "Space", "Enter", "Home", "KeyC", "KeyK"]);

/**
 * Any action of the musician's but V and listening closes the pill (D2): a key (Esc only closes
 * it), or a press anywhere but on the pill, the transport's controls and the ruler. Comparing is
 * listening: playing, stopping, looping and moving the playhead keep it. Returns how to stop.
 */
export function followCompare(): () => void {
  const key = (e: KeyboardEvent) => {
    if (!useCompare.getState().pill || MODIFIERS.has(e.key)) return;
    if (LISTENING.has(e.code) && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && !typing(e.target)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
    }
    closeCompare();
  };
  const press = (e: PointerEvent) => {
    if (!useCompare.getState().pill) return;
    if (e.target instanceof Element && e.target.closest("[data-testid=compare-pill], [data-transport], [data-testid=ruler]")) return;
    closeCompare();
  };
  window.addEventListener("keydown", key, true);
  window.addEventListener("pointerdown", press, true);
  return () => {
    window.removeEventListener("keydown", key, true);
    window.removeEventListener("pointerdown", press, true);
  };
}
