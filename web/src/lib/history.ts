// What Undo and Redo would take back, in words (taste.md §2.14, TB-12): "Undo Reverb size".
//
// The names are the server's (gaps #29): each lives on its undo step, by the revision the change
// made, and follows it through undo and redo, for every window and after a reload. The window that
// makes an edit names it as it records it (`POST /api/graph/step`); a step made elsewhere (an
// agent's Tx, a save in another editor) has a plain name from what it changed ("Bass notes"). The
// state says the two tops as `undo_name` and `redo_name`, each with its id and who made it
// (AGENT-2): a file saved in another program is "Undo outside change: the code in song.ts", an
// agent's change by its name and its words, "Undo Claude: brighter hats" (AGENT-10).

import { useStore } from "./store";
import { request } from "./projects";
import type { GraphState } from "./graph";

export interface Step {
  /** What it changed, to follow "Undo" or "Undid": "Reverb size", "adding Reverb". */
  name: string;
  /** Adding or removing a part: its undo says so in a toast, since what it takes back may be off screen (ST-5). */
  structural?: boolean;
}

/** A step's name as the server says it; `given`: a window named it, else it is the plain name of what it changed. */
export interface StepName {
  name: string;
  structural: boolean;
  given: boolean;
  /** Its id: what Undo and Redo send, so they take back only this step (AGENT-2). */
  step?: number;
  /** Who made it: this page, an agent (with the tools key), or a program outside the app. */
  source?: "page" | "agent" | "outside";
  /** An agent's name, as it gave it: plain text (AGENT-10). */
  by?: string | null;
}

/** A step as Undo's tooltip says it after "Undo": its name, "outside change: …", or an agent's "Claude: …" (AGENT-2). */
export function said(step: StepName): string {
  if (step.source === "outside") return `outside change: ${step.name}`;
  if (step.source === "agent") return `${step.by ?? "agent"}: ${step.name}`;
  return step.name;
}

declare module "./graph" {
  interface GraphState {
    /** What Undo would take back, and Redo; null with nothing to undo or redo. */
    undo_name?: StepName | null;
    redo_name?: StepName | null;
  }
}

/** What naming a step answers: the names after it, and the revision they are the tops of. */
interface Named {
  named: boolean;
  revision: number;
  undo_name: StepName | null;
  redo_name: StepName | null;
}

/**
 * The names this window's last step got, by the revision they describe. A state read before the
 * name reached the server (the edit's own refresh, still on its way) is that revision too, but
 * names its step plainly: these win over it.
 */
let answered: { session: number; revision: number; undo: StepName | null; redo: StepName | null } | null = null;

/** The undo steps this window's own edits made, by id (AGENT-2): only these may ⌘Z take back unshown. */
let mine: { session: number; steps: Set<number> } = { session: 0, steps: new Set() };

/**
 * This window's edits on their way: sent, and not yet answered and recorded (or refused). The
 * server may have made an edit's step before its answer reaches the window, so a ⌘Z pressed then
 * would name the step below it, which isn't the newest, and be refused (AGENT-2).
 */
const onItsWay = new Set<Promise<void>>();

/**
 * An edit of this window is on its way: call what this returns once its answer is recorded, or it
 * was refused. Undo and Redo of the top wait for it (`edited`).
 */
export function editing(): () => void {
  let done = () => {};
  const p = new Promise<void>((resolve) => (done = resolve));
  onItsWay.add(p);
  return () => {
    onItsWay.delete(p);
    done();
  };
}

/** Once every edit this window has on its way now is answered. */
export function edited(): Promise<void> {
  return Promise.all([...onItsWay]).then(() => {});
}

/** Whether this window has an edit on its way. */
export const sending = () => onItsWay.size > 0;

/** Whether undo step `step` of `session` was made by an edit of this window. */
export function madeHere(session: number, step: number | undefined): boolean {
  return step !== undefined && mine.session === session && mine.steps.has(step);
}

/**
 * An edit this window made has committed `answer.revision`, as `step`: the server names its undo
 * step so. Its id (`answer.step`) is kept as this window's, and Undo names it at once, so a ⌘Z
 * pressed before the state is read again takes back this edit, not the one before.
 */
export function recorded(session: number, answer: { revision: number; step?: number }, step: Step) {
  const revision = answer.revision;
  const undo: StepName = { name: step.name, structural: !!step.structural, given: true, source: "page", step: answer.step };
  if (answer.step !== undefined) {
    if (mine.session !== session) mine = { session, steps: new Set() };
    mine.steps.add(answer.step);
    const { graph, set } = useStore.getState();
    // Unless a newer state already says what is on top.
    if (graph?.session === session && graph.view.revision <= revision) set({ graph: { ...graph, undo_name: undo, redo_name: null, can_undo: true } });
  }
  // Said at once, before the server has it: the edit's own refresh names the step plainly.
  answered = { session, revision, undo, redo: null };
  request<Named>("/api/graph/step", { session, revision, name: step.name, structural: !!step.structural })
    .then((r) => {
      answered = { session, revision: r.revision, undo: r.undo_name, redo: r.redo_name };
      // Shown at once, by a state that is still that revision.
      const { graph, set } = useStore.getState();
      if (graph && graph.session === session && graph.view.revision === r.revision) set({ graph: { ...graph, undo_name: r.undo_name, redo_name: r.redo_name } });
    })
    // A window that can't name it (the song closed meanwhile) leaves it named plainly.
    .catch(() => {});
}

/** The step Undo (or Redo) would take back in `graph`. */
export function next(graph: GraphState, redo: boolean): StepName | null {
  const named = (redo ? graph.redo_name : graph.undo_name) ?? null;
  if (named?.given) return named;
  if (answered && answered.session === graph.session && answered.revision === graph.view.revision) return (redo ? answered.redo : answered.undo) ?? named;
  return named;
}

/** What an Undo (or Redo) took back, as its answer names it: `name` and `structural`. */
export function tookBack(answer: object): Step | null {
  const a = answer as { name?: string | null; structural?: boolean };
  return a.name ? { name: a.name, structural: !!a.structural } : null;
}
