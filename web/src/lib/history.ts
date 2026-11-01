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
// gesture goes through (`shown`, `goesFrom`), which waits until the window shows its own last change,
// then goes from the window's newest own revision in the run of its own changes right after the one
// it was drawn at (its `base`, which it says to the server: refused only when a change after it
// touched the target), and not at all after a change of its own since that moved ids in a file it
// targets (`MOVED`). So the window's own change never refuses its gesture, nor moves it onto
// another call.

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
 * id, in the order their answers came (only these may ⌘Z take back unshown), the revisions its
 * changes made (its edits, undos and redos), each with whether it moved ids (as its answer says,
 * `moved`), and the newest of those, which every gesture waits for the window to show.
 */
/** A revision a change of the window's own made: the files whose ids it moved, and whether it was the window's alone (not `mixed`). */
interface Made {
  moved: readonly string[];
  own: boolean;
}
interface Mine {
  session: number;
  steps: number[];
  revisions: Map<number, Made>;
  revision: number;
}
let mine: Mine = { session: 0, steps: [], revisions: new Map(), revision: 0 };
const minein = (session: number): Mine => (mine.session === session ? mine : (mine = { session, steps: [], revisions: new Map(), revision: 0 }));

/**
 * An edit of this window on its way: sent (or, a drag let go, waiting to be), and not yet answered,
 * or refused. `since`: how many steps the window had kept when it started. `end` once it is over.
 */
export interface Editing {
  readonly done: Promise<void>;
  readonly since: number;
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
    since: mine.steps.length,
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

/**
 * This window's undos and redos on their way. Not edits (Undo's own order is its own: one ⌘Z after
 * another, each naming the top the one before left), but changes all the same: a gesture waits for
 * them too.
 */
const flying = new Set<object>();

/** An undo or a redo of this window is on its way from now: call what this returns once it is answered. */
export function flight(): () => void {
  const f = {};
  flying.add(f);
  return () => {
    flying.delete(f);
    wake();
  };
}

/** Whether undo step `step` of `session` was made by an edit of this window. */
export function madeHere(session: number, step: number | undefined): boolean {
  return step !== undefined && mine.session === session && mine.steps.includes(step);
}

/** The newest undo step of `session` this window kept since it had kept `since` (an edit's `since`); 0 with none. */
export const madeSince = (session: number, since: number) => (mine.session === session ? Math.max(0, ...mine.steps.slice(since)) : 0);

/**
 * A change of this window (an edit, an undo, a redo) made revision `revision` of `session`, as its
 * answer says at its top, moving the ids of the files `moved`: a gesture waits until the window
 * shows it. `mixed`: the revision also took in a change made elsewhere (a file saved outside that
 * rode along), so it isn't the window's own alone. A change of `layout.json` alone makes no revision
 * (PLANE-3), and its answer names none.
 */
export function changed(session: number, revision: number, moved: readonly string[], mixed = false) {
  const m = minein(session);
  m.revisions.set(revision, { moved, own: !mixed });
  m.revision = Math.max(m.revision, revision);
  wake();
}

/** What a gesture targets: files (`song.ts`) and folders (`data/kick/`); `ALL`: everything, so any change of the window's own since it was drawn holds it back. */
export type Targets = readonly string[] | typeof ALL;
export const ALL = null;

/** Whether a change that moved ids in `moved` (a file, or a folder a track's data was in) moves an id `targets` names. */
function touches(moved: string, targets: readonly string[]): boolean {
  return targets.some((t) => t === moved || t.startsWith(`${moved}/`) || (t.endsWith("/") && moved.startsWith(t)));
}

/**
 * Where a gesture of `session` drawn at revision `drawn` goes from, the window showing `to`
 * (AGENT-2): its `base`, the window's newest own revision in the run of its own changes right after
 * `drawn` (none mixed), else `drawn`; and whether a change of the window's own since `drawn` moved
 * an id the gesture `targets` (then it isn't sent).
 */
export function baseOf(session: number, drawn: number, to: number, targets: Targets): { base: number; moved: boolean } {
  let base = drawn;
  let run = true;
  let moved = false;
  if (mine.session !== session) return { base, moved };
  for (let r = drawn + 1; r <= to; r++) {
    const m = mine.revisions.get(r);
    const own = !!m?.own;
    if (m && (targets === ALL || m.moved.some((f) => touches(f, targets)))) moved = true;
    if (run && own) base = r;
    else run = false;
  }
  return { base, moved };
}

/**
 * An edit of this window was answered: the undo step it made (`answer.step`), kept as this window's
 * own (AGENT-2), and the revision it made, which gestures wait for the window to show. Every
 * answer that names a step comes here, whatever its route (`lib/http.ts`), and a take's news. With
 * the step's name as the server says it (`undo_name`), Undo names it at once, so a ⌘Z pressed
 * before the state is read again takes back this edit, not the one before.
 */
export function owned(session: number, answer: { revision?: number; step?: number; moved?: readonly string[]; mixed?: boolean; undo_name?: StepName | null }) {
  // An answer with no step changed nothing.
  if (answer.step === undefined) return;
  // Only an answer that says whether it moved ids names the revision the change made (lib/http).
  if (answer.revision !== undefined && answer.moved !== undefined) changed(session, answer.revision, answer.moved, answer.mixed);
  const m = minein(session);
  if (!m.steps.includes(answer.step)) m.steps.push(answer.step);
  const { graph, set } = useStore.getState();
  // Unless a newer state already says what is on top.
  if (answer.undo_name?.step === answer.step && graph?.session === session && graph.view.revision <= (answer.revision ?? graph.view.revision)) {
    set({ graph: { ...graph, undo_name: answer.undo_name, redo_name: null, can_undo: true } });
  }
}

/**
 * An edit this window made has committed `answer.revision` (none, a change of `layout.json` alone),
 * as `step`: the server names its undo step so, by its id, the window keeps it as its own (`owned`),
 * and Undo names it at once.
 */
export function recorded(session: number, answer: { revision?: number; step?: number; moved?: readonly string[]; mixed?: boolean }, step: Step) {
  const { graph: drawn } = useStore.getState();
  const revision = answer.revision ?? (drawn?.session === session ? drawn.view.revision : 0);
  const undo: StepName = { name: step.name, structural: !!step.structural, given: true, source: "page", step: answer.step };
  owned(session, answer);
  if (answer.step !== undefined) {
    const { graph, set } = useStore.getState();
    // Unless a newer state already says what is on top.
    if (graph?.session === session && graph.view.revision <= revision) set({ graph: { ...graph, undo_name: undo, redo_name: null, can_undo: true } });
  }
  // Said at once, before the server has it: the edit's own refresh names the step plainly.
  answered = { session, revision, undo, redo: null };
  const by = answer.step !== undefined ? { step: answer.step } : { revision };
  request<Named>("/api/graph/step", { session, ...by, name: step.name, structural: !!step.structural })
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

/** How long a gesture waits with nothing moving (none of the changes it waits for answered, nothing new shown) before it gives up, saying so. */
const SHOWN_WAIT_MS = 2000;

/** What a gesture that waited too long says (for 2 s none of its window's changes ahead of it was answered, and it showed nothing new: the server is stuck). */
export const LATE = "Nothing was changed: your last change isn't showing yet. Try again in a moment.";

/** What a gesture says when a change of the window's own moved ids while it waited to show it: it does nothing (AGENT-2). */
export const MOVED = "The song changed while you pressed. Try again.";

/** A gesture not sent: a change of the window's own since it was drawn moved ids (`MOVED`). Its control is drawn again as it is. */
export class Moved extends Error {
  constructor() {
    super(MOVED);
  }
}

/** The gates waiting, each told when something it waits for may have happened. */
const waiters = new Set<() => void>();
function wake() {
  for (const w of [...waiters]) w();
}
useStore.subscribe((s, was) => s.graph !== was.graph && wake());

/** This window's changes on their way now that started before `own` (all of them, without it): its edits, undos and redos. */
function ahead(own?: Editing): object[] {
  const all = [...onItsWay];
  const at = own ? all.indexOf(own) : -1;
  return [...(at >= 0 ? all.slice(0, at) : all), ...flying];
}

/** Whether change `e` (an edit or an undo of this window's) is on its way still. */
const going = (e: object) => onItsWay.has(e as Editing) || flying.has(e);

/**
 * Whether `graph` shows this window's own changes: none of `before` (by default, its changes on
 * their way now) is on its way still, and `graph` shows the newest revision its changes made.
 */
export function showsOwn(graph: GraphState, before: readonly object[] = ahead()): boolean {
  return !before.some(going) && (mine.session !== graph.session || graph.view.revision >= mine.revision);
}

/**
 * The one gate every gesture of this window goes through (AGENT-2): the state to go from. At once
 * when the window shows its own changes; else once it does, most often in milliseconds, its state
 * being on its way. So a gesture is never dropped, nor refused, for the window's own last change.
 * It waits for the edits on their way when it is called, before `own` (the gesture's own edit on
 * its way, if it has one): never for one started after, which may be waiting for it. Null when the
 * song closed or another opened meanwhile, or, said in words (`LATE`), after 2 s in which nothing it
 * waits for moved: a long queue of quick gestures, each answered in turn, waits as long as it takes.
 */
export function shown(session: number, own?: Editing): Promise<GraphState | null> {
  const before = ahead(own);
  return new Promise((resolve) => {
    let timer = 0;
    // What it last saw: how many changes ahead were on their way, and the revision shown.
    let seen = "";
    const finish = (graph: GraphState | null) => {
      waiters.delete(check);
      clearTimeout(timer);
      resolve(graph);
    };
    const late = () => {
      finish(null);
      if (useStore.getState().graph?.session === session) useStore.getState().toast(LATE);
    };
    const check = () => {
      const graph = useStore.getState().graph;
      if (!graph || graph.session !== session) return finish(null);
      if (showsOwn(graph, before)) return finish(graph);
      // Something moved: the 2 s start again.
      const now = `${before.filter(going).length} ${graph.view.revision}`;
      if (now === seen) return;
      seen = now;
      clearTimeout(timer);
      timer = window.setTimeout(late, SHOWN_WAIT_MS);
    };
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
 * How a gesture of this window drawn at revision `drawn`, targeting `targets`, goes (AGENT-2), once
 * the window shows its own changes (`through`): from its `base` (`baseOf`), which it says to the
 * server, refused only when a change after it touched the target. `shown`: the base is newer than
 * the revision drawn, so the gesture is made from what the window shows (`graph`). Rejects with
 * `Moved` when a change of the window's own since `drawn` moved an id it targets: it is not sent.
 * Rejects with what to say when it gave up.
 */
export async function goesFrom(session: number, drawn: number, targets: Targets, own?: Editing): Promise<{ base: number; shown: boolean; graph: GraphState }> {
  const graph = await through(session, own);
  const { base, moved } = baseOf(session, drawn, graph.view.revision, targets);
  if (moved) throw new Moved();
  return { base, shown: base > drawn, graph };
}

/**
 * What control `id` (a knob's or a setting's, as `drawn` names it) is called in `now`, after the
 * window's own changes that moved no ids in its file: the same call (`file#ordinal`), the same
 * declared slot. A value written where the call played its default gives the control a new id
 * (`…/@brightness` becomes `…/1.brightness`); nothing else does. Any other id is the same.
 */
export function carried(id: string, now: GraphState, drawn?: GraphState | null): string {
  const controls = (g: GraphState) => g.view.modules.flatMap((m) => m.nodes.flatMap((n) => [...n.knobs, ...(n.settings ?? [])].map((c) => ({ node: n.id, c }))));
  const there = controls(now);
  if (there.some(({ c }) => c.id === id)) return id;
  const was = drawn && controls(drawn).find(({ c }) => c.id === id);
  const slot = was ? was.c.slot : id.includes("/@") ? id.slice(id.lastIndexOf("/@") + 2) : null;
  const call = id.slice(0, id.lastIndexOf("/"));
  return (slot && there.find(({ node, c }) => node === call && c.slot === slot)?.c.id) || id;
}

/** The file an id of the view names (`tracks/keys.ts#2/1.size` → `tracks/keys.ts`): what a gesture on it targets. */
export const fileOf = (id: string) => id.slice(0, Math.max(0, id.indexOf("#")) || id.length);

/** The folder of a track's lanes (`data/kick/`): what a gesture on its clips targets. */
export const lanesOf = (module: string) => `data/${module}/`;

/**
 * `goesFrom`, as a call that names controls wants it: the revision to go from, and what each id it
 * names is called there (`carried`).
 */
export async function gate(session: number, drawn: number, targets: Targets, own?: Editing): Promise<{ revision: number; id: (id: string) => string }> {
  const was = useStore.getState().graph;
  const f = await goesFrom(session, drawn, targets, own);
  return f.shown ? { revision: f.base, id: (id) => carried(id, f.graph, was) } : { revision: f.base, id: (id) => id };
}

/** The revision a gesture drawn at `drawn` goes from (`goesFrom`): its base. */
export async function goes(session: number, drawn: number, targets: Targets, own?: Editing): Promise<number> {
  return (await goesFrom(session, drawn, targets, own)).base;
}

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
