// What Undo and Redo would take back, in words (taste.md §2.14, TB-12): "Undo Reverb size".
//
// The names are the server's (gaps #29): each lives on its undo step, by the revision the change
// made, and follows it through undo and redo, for every window and after a reload. The window that
// makes an edit names it as it records it (`POST /api/graph/step`); a step made elsewhere (an
// agent's Tx, a save in another editor) has a plain name from what it changed ("Bass notes"). The
// state says the two tops as `undo_name` and `redo_name`, each with its id and who made it
// (AGENT-2): a file saved in another program is "Undo outside change: the code in song.ts", an
// agent's change by its name and its words, "Undo Claude: brighter hats" (AGENT-10).
//
// Here too, the window's own changes (AGENT-2): the steps its edits made, kept as its own as their
// answers come (`owned`), so ⌘Z takes one back before the window shows it; and the one gate every
// gesture goes through (`shown`), which waits until the window shows its own last change, so a
// gesture is never dropped, nor refused, for it.

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

/**
 * This window's own changes to the song open (`session`), AGENT-2: the undo steps its edits made, by
 * id (only these may ⌘Z take back unshown), the newest of them, the revisions its changes made (its
 * edits, undos and redos), and the newest of those, which every gesture waits for the window to show.
 */
let mine: { session: number; steps: Set<number>; newest: number; revisions: Set<number>; revision: number } = {
  session: 0,
  steps: new Set(),
  newest: 0,
  revisions: new Set(),
  revision: 0,
};
const minein = (session: number) =>
  mine.session === session ? mine : (mine = { session, steps: new Set(), newest: 0, revisions: new Set(), revision: 0 });

/**
 * An edit of this window on its way: sent (or, a drag let go, waiting to be), and not yet answered,
 * or refused. `end` once it is over.
 */
export interface Editing {
  readonly done: Promise<void>;
  end(): void;
}

/**
 * This window's edits on their way, in the order they started. The server may make an edit's step
 * before its answer reaches the window, so a ⌘Z pressed then would name the step below it, which
 * isn't the newest, and be refused (AGENT-2): Undo and Redo of the top wait for these, and so does
 * every gesture (`shown`).
 */
const onItsWay = new Set<Editing>();

/** An edit of this window is on its way from now: end what this returns once it is over. */
export function editing(): Editing {
  let resolve = () => {};
  const done = new Promise<void>((r) => (resolve = r));
  const e: Editing = {
    done,
    end() {
      onItsWay.delete(e);
      resolve();
      wake();
    },
  };
  onItsWay.add(e);
  return e;
}

/** The edits this window has on its way now. */
export const onTheirWay = (): Editing[] => [...onItsWay];

/** Whether undo step `step` of `session` was made by an edit of this window. */
export function madeHere(session: number, step: number | undefined): boolean {
  return step !== undefined && mine.session === session && mine.steps.has(step);
}

/** The newest undo step of `session` this window's edits made, by id; 0 with none. */
export const newestMade = (session: number) => (mine.session === session ? mine.newest : 0);

/**
 * A change of this window (an edit, an undo, a redo) to `file` was answered at revision `revision`
 * of `session`: a gesture waits until the window shows it. A change of `layout.json` alone makes no
 * revision (PLANE-3): the one its answer names was made by whoever made it.
 */
export function changed(session: number, revision: number, file?: string) {
  if (file === "layout.json") return;
  const m = minein(session);
  m.revisions.add(revision);
  m.revision = Math.max(m.revision, revision);
  wake();
}

/** Whether every revision of `session` after `from`, up to `to`, was made by a change of this window's. */
export function allMine(session: number, from: number, to: number): boolean {
  if (mine.session !== session) return to <= from;
  for (let r = from + 1; r <= to; r++) if (!mine.revisions.has(r)) return false;
  return true;
}

/**
 * An edit of this window was answered: the undo step it made (`answer.step`), kept as this window's
 * own (AGENT-2), and the revision it made, which gestures wait for the window to show. Every
 * answer that names a step comes here, whatever its route (`lib/http.ts`), and a take's news. With
 * the step's name as the server says it (`undo_name`), Undo names it at once, so a ⌘Z pressed
 * before the state is read again takes back this edit, not the one before.
 */
export function owned(session: number, answer: { revision: number; step?: number; undo_name?: StepName | null; file?: string }) {
  // An answer with no step changed nothing.
  if (answer.step === undefined) return;
  changed(session, answer.revision, answer.file);
  const m = minein(session);
  m.steps.add(answer.step);
  m.newest = Math.max(m.newest, answer.step);
  const { graph, set } = useStore.getState();
  // Unless a newer state already says what is on top.
  if (answer.undo_name?.step === answer.step && graph?.session === session && graph.view.revision <= answer.revision) {
    set({ graph: { ...graph, undo_name: answer.undo_name, redo_name: null, can_undo: true } });
  }
}

/**
 * An edit this window made has committed `answer.revision`, as `step`: the server names its undo
 * step so, the window keeps it as its own (`owned`), and Undo names it at once.
 */
export function recorded(session: number, answer: { revision: number; step?: number; file?: string }, step: Step) {
  const revision = answer.revision;
  const undo: StepName = { name: step.name, structural: !!step.structural, given: true, source: "page", step: answer.step };
  owned(session, answer);
  if (answer.step !== undefined) {
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

// ---------------------------------------------------------------- the gate (AGENT-2)

/** How long a gesture waits for the window to show its own last change before it gives up, saying so. */
const SHOWN_WAIT_MS = 2000;

/** What a gesture that waited too long says (its own last change didn't show within 2 s: the server is stuck). */
export const LATE = "Nothing was changed: your last change isn't showing yet. Try again in a moment.";

/** The gates waiting, each told when something it waits for may have happened. */
const waiters = new Set<() => void>();
function wake() {
  for (const w of [...waiters]) w();
}
useStore.subscribe((s, was) => s.graph !== was.graph && wake());

/** This window's edits on their way now that started before `own` (all of them, without it). */
function ahead(own?: Editing): Editing[] {
  const all = [...onItsWay];
  const at = own ? all.indexOf(own) : -1;
  return at >= 0 ? all.slice(0, at) : all;
}

/**
 * Whether `graph` shows this window's own changes: none of `before` (by default, its edits on their
 * way now) is on its way still, and `graph` shows the newest revision its changes made.
 */
export function showsOwn(graph: GraphState, before: readonly Editing[] = [...onItsWay]): boolean {
  return !before.some((e) => onItsWay.has(e)) && (mine.session !== graph.session || graph.view.revision >= mine.revision);
}

/**
 * The one gate every gesture of this window goes through (AGENT-2): the state to go from. At once
 * when the window shows its own changes; else once it does, most often in milliseconds, its state
 * being on its way. So a gesture is never dropped, nor refused, for the window's own last change.
 * It waits for the edits on their way when it is called, before `own` (the gesture's own edit on
 * its way, if it has one): never for one started after, which may be waiting for it. Null when the
 * song closed or another opened meanwhile, or, said in words (`LATE`), after 2 s.
 */
export function shown(session: number, own?: Editing): Promise<GraphState | null> {
  const before = ahead(own);
  return new Promise((resolve) => {
    const finish = (graph: GraphState | null) => {
      waiters.delete(check);
      clearTimeout(timer);
      resolve(graph);
    };
    const check = () => {
      const graph = useStore.getState().graph;
      if (!graph || graph.session !== session) finish(null);
      else if (showsOwn(graph, before)) finish(graph);
    };
    const timer = setTimeout(() => {
      finish(null);
      if (useStore.getState().graph?.session === session) useStore.getState().toast(LATE);
    }, SHOWN_WAIT_MS);
    waiters.add(check);
    check();
  });
}

/**
 * `shown`, as a gesture's call wants it: the state to go from, or a rejection with what to say (it
 * waited too long, or the song closed meanwhile).
 */
export async function through(session: number, own?: Editing): Promise<GraphState> {
  const now = await shown(session, own);
  if (!now) throw new Error(useStore.getState().graph?.session === session ? LATE : "The song closed, so nothing was changed.");
  return now;
}

/**
 * The revision a gesture of this window drawn at `revision` goes from, once the window shows its own
 * changes (`shown`): `revision` itself, unless the window was still to show its own last change
 * then; then the revision it shows now, when every change since `revision` was the window's own and
 * each of `ids` (the controls, nodes or inputs the gesture names) names there what it named in the
 * state drawn. Otherwise `revision`: the server carries the gesture, or refuses it in its own words
 * (a change made elsewhere meanwhile is never taken over unseen). Rejects with what to say when it
 * gave up.
 */
export async function goes(session: number, revision: number, ids: readonly string[], own?: Editing): Promise<number> {
  const drawn = useStore.getState().graph;
  const now = await through(session, own);
  if (now.view.revision <= revision || !drawn || drawn.session !== session || drawn.view.revision !== revision) return revision;
  if (!allMine(session, revision, now.view.revision)) return revision;
  return ids.every((id) => same(named(drawn, id), named(now, id))) ? now.view.revision : revision;
}

/** What `id` names in `graph`: a node, a knob, a setting or an input, by what it is; null when nothing. */
function named(graph: GraphState, id: string): string | null {
  for (const m of graph.view.modules) {
    for (const n of m.nodes) {
      const at = `${m.key} ${n.component ?? n.kind} ${n.label}`;
      if (n.id === id) return `node ${at}`;
      const k = n.knobs.find((x) => x.id === id);
      if (k) return `knob ${at} ${k.slot ?? ""} ${k.label}`;
      const s = n.settings?.find((x) => x.id === id);
      if (s) return `setting ${at} ${s.slot} ${s.label}`;
      const i = n.inlets.find((x) => x.id === id);
      if (i) return `inlet ${at} ${i.label} ${i.name}`;
    }
  }
  return null;
}
const same = (a: string | null, b: string | null) => a !== null && a === b;

/** The step Undo (or Redo) would take back in `graph`. */
export function next(graph: GraphState, redo: boolean): StepName | null {
  const named = (redo ? graph.redo_name : graph.undo_name) ?? null;
  if (named?.given) return named;
  if (answered && answered.session === graph.session && answered.revision === graph.view.revision) return (redo ? answered.redo : answered.undo) ?? named;
  return named;
}

/**
 * Why Undo (or Redo) can go back no further, said where it is when it has nothing to take back and
 * something cut it off as the song opened (WRITE-24): a change made outside music2, or an undo
 * history that couldn't be read. Null otherwise.
 */
export function ended(graph: GraphState, redo: boolean): string | null {
  if (redo ? graph.redo_name : (graph.undo_name || graph.undo_stuck)) return null;
  const what = redo ? "Redo" : "Undo";
  switch (redo ? graph.redo_end : graph.undo_end) {
    case "outside":
      return `${what} isn't possible past the change made outside music2`;
    case "unread":
      return `${what} isn't possible past this point: the undo history kept with the song couldn't be read`;
    default:
      return null;
  }
}

/** What an Undo (or Redo) took back, as its answer names it: `name` and `structural`. */
export function tookBack(answer: object): Step | null {
  const a = answer as { name?: string | null; structural?: boolean };
  return a.name ? { name: a.name, structural: !!a.structural } : null;
}
