// The Compare key, V (D2; AGENT-3): hear before and after the newest change an agent or another
// program made, with one key. Compare only plays: it never writes the song.
//
// V names that step, and the server plays Before, the song with that step taken back, built in
// memory: the pill in the top bar says "Before: Claude: brighter hats". V again plays After, the song
// as it is. The song's files, undo and what the window shows stay at After. While the pill is open,
// agents wait. Esc closes it and keeps what plays: on Before, the step is undone then, and a toast
// says so, with Redo. Any other action but listening (play, stop, the loop, a seek) closes it at
// After. A file written directly while it is open closes it, saying so.
//
// The pill is only what the server says: its `compare` events, its answers and its state, each
// numbered, so the newest wins. One V at a time; a held V's repeats are ignored (App.tsx).

import { create } from "zustand";
import { graphUndo } from "../components/GraphCanvas";
import { clientId } from "./graph";
import type { StepName } from "./history";
import { token } from "./native";
import { stepped } from "./notice";
import { useStore } from "./store";
import { http } from "./http";

/** What the pill compares, as the server says it. */
export interface Comparing {
  step: number;
  /** After plays: the song as it is; else Before, the song with the step taken back. */
  after: boolean;
  /** As Undo says it: "Claude: brighter hats". */
  name: string;
  /** Counts up with every change of the pill the server tells. */
  n: number;
}

declare module "./graph" {
  interface GraphState {
    comparing?: Comparing | null;
  }
}

/** The pill as the server last said it, for song `session`, and the number of what it said. */
export const useCompare = create<{ pill: (Comparing & { session: number }) | null; session: number | null; n: number }>(() => ({
  pill: null,
  session: null,
  n: 0,
}));

/** How many times the server's word changed the pill here: a read begun before one is older. */
let heardTimes = 0;

/**
 * The server says the pill is `pill` (null: closed) in song `session`, as its change `n`: kept when
 * it is newer than what this window heard, or of another song. Whether it was.
 */
function heard(session: number, pill: Comparing | null, n: number): boolean {
  const now = useCompare.getState();
  if (now.session === session && n <= now.n) return false;
  heardTimes += 1;
  useCompare.setState({ session, n, pill: pill ? { ...pill, session } : null });
  return true;
}

const headers = () => ({ "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) });

/** What V and the close answer: what the pill compares now, and, refused, why and what is newest. */
interface Answer {
  comparing?: Comparing | null;
  error?: string;
  /** A refusal's newest step, as the server names it (null: nothing to undo). */
  newest?: StepName | null;
  undo_name?: StepName | null;
  redo_name?: StepName | null;
}

async function post(body: object): Promise<{ status: number; json: Answer }> {
  const res = await http("/api/graph/compare", { method: "POST", headers: headers(), body: JSON.stringify(body) });
  return { status: res.status, json: await res.json().catch(() => ({})) };
}

/** Vs and closes in flight: another V waits for their answers, so each names what the last left. */
let asking = 0;

/**
 * V: open the pill on the newest change an agent or another program made, or flip it between
 * Before and After. The server decides: a window a moment behind (a change that landed just now)
 * is told the newest step, and V goes on with it once, when it is newer and not the musician's own.
 */
export async function compare() {
  const { graph, toast } = useStore.getState();
  if (!graph || asking > 0) return;
  asking += 1;
  try {
    const shown = useCompare.getState().pill;
    const pill = shown?.session === graph.session ? shown : null;
    let step = pill ? pill.step : (graph.undo_name?.step ?? 0);
    const ask = () => post({ session: graph.session, step, before: pill ? pill.after : true, client: clientId });
    let r = await ask();
    const newest = r.json.newest;
    if (!pill && r.status === 409 && newest?.step !== undefined && newest.step > step && newest.source !== "page") {
      step = newest.step;
      r = await ask();
    }
    const c = r.json.comparing;
    if (c) heard(graph.session, c, c.n);
    if (r.status === 200) return;
    if (r.status === 409 && r.json.newest === null) return toast("Nothing to compare: there's no change to undo.");
    toast(r.json.error ?? "Nothing to compare now.");
  } catch (error) {
    toast(error instanceof Error ? error.message : String(error));
  } finally {
    asking -= 1;
  }
}

/** Esc (`keep`): the pill closes, keeping what plays. Any other action: it closes at After. */
export function closeCompare(keep: boolean) {
  const pill = useCompare.getState().pill;
  if (!pill) return;
  asking += 1;
  post({ session: pill.session, open: false, keep })
    .then((r) => {
      const { graph, set, toast } = useStore.getState();
      if (r.status !== 200) return toast(r.json.error ?? "Compare couldn't close.");
      // Esc on Before undid the step: Undo's and Redo's names after it, before the state is read.
      if (r.json.undo_name !== undefined && graph?.session === pill.session) set({ graph: { ...graph, undo_name: r.json.undo_name, redo_name: r.json.redo_name } });
    })
    .catch(() => {})
    .finally(() => (asking -= 1));
}

/** The server told a change of the pill: opened, flipped, or closed, and why. */
export function heardCompare(e: Comparing & { session: number; open: boolean; why: string | null }) {
  if (!heard(e.session, e.open ? { step: e.step, after: e.after, name: e.name, n: e.n } : null, e.n) || e.open) return;
  const toast = useStore.getState().toast;
  // Esc on Before: the step was undone, one undo step, which Redo brings back; its notice's Undo
  // has nothing more to offer.
  if (e.why === "closed" && !e.after) {
    stepped(e.step);
    return toast(`${e.name} is undone. Redo brings it back.`, { label: "Redo", run: () => void graphUndo(e.session, "redo", e.step) });
  }
  if (e.why === "outside") toast("A file changed outside the app, so Compare closed.");
}

/** How many times the pill changed here: what [`syncCompare`] is given when its read begins. */
export const compareHeard = () => heardTimes;

/**
 * The pill as the server has it in the state: when the window connects, another song opens, or the
 * socket fell behind. `since` is [`compareHeard`] when the state's read began: an event heard after
 * it is newer than a closed pill the state says.
 */
export function syncCompare(since?: number) {
  const graph = useStore.getState().graph;
  if (!graph) return void useCompare.setState({ pill: null, session: null, n: 0 });
  const c = graph.comparing;
  if (useCompare.getState().session !== graph.session) {
    return void useCompare.setState({ session: graph.session, n: c?.n ?? 0, pill: c ? { ...c, session: graph.session } : null });
  }
  if (c) return void heard(graph.session, c, c.n);
  if (since === undefined || since === heardTimes) useCompare.setState({ pill: null });
}

/** A key that is no action of its own: a modifier, held. */
const MODIFIERS = new Set(["Shift", "Meta", "Alt", "Control", "CapsLock", "Fn"]);

/** Whether `t` takes typing. */
const typing = (t: EventTarget | null) => t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || (t instanceof HTMLElement && t.isContentEditable);

/** Keys that only listen (play or stop, back to the start, the loop, the metronome) and V itself. */
const LISTENING = new Set(["KeyV", "Space", "Enter", "Home", "KeyC", "KeyK"]);

/**
 * Esc closes the pill and keeps what plays; any other action of the musician's but V and listening
 * closes it at After (D2): a key, or a press anywhere but on the pill, the transport's controls, the
 * ruler and a pointer's buttons. Comparing is listening: playing, stopping, looping and moving the
 * playhead keep it. Returns how to stop.
 */
export function followCompare(): () => void {
  const key = (e: KeyboardEvent) => {
    if (!useCompare.getState().pill || MODIFIERS.has(e.key)) return;
    if (LISTENING.has(e.code) && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && !typing(e.target)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      return closeCompare(true);
    }
    closeCompare(false);
  };
  const press = (e: PointerEvent) => {
    if (!useCompare.getState().pill) return;
    if (e.target instanceof Element && e.target.closest("[data-testid=compare-pill], [data-transport], [data-testid=ruler], [data-testid=pointer-note]")) return;
    closeCompare(false);
  };
  window.addEventListener("keydown", key, true);
  window.addEventListener("pointerdown", press, true);
  return () => {
    window.removeEventListener("keydown", key, true);
    window.removeEventListener("pointerdown", press, true);
  };
}
