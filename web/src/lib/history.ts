// What Undo and Redo would take back, in words (taste.md §2.14, TB-12): "Undo Reverb size".
//
// The server keeps the undo steps but no names for them yet (gaps #29), so this window names the
// steps it makes, as it makes them, and follows them as its own Undo and Redo move them. The names
// are trusted only while nothing else has changed the song: revisions are numbered one by one, so
// when every revision since the names began is one this window made, its stacks are the server's
// tops. Any other (a save in another editor, an agent's Tx) and Undo goes unnamed until this window
// edits again: an unnamed Undo is honest, a wrongly named one isn't (principle 7).

export interface Step {
  /** What it changed, to follow "Undo" or "Undid": "Reverb size", "adding Reverb". */
  name: string;
  /** Adding or removing a part: its undo says so in a toast, since what it takes back may be off screen (ST-5). */
  structural?: boolean;
}

let h = { session: -1, base: 0, undo: [] as Step[], redo: [] as Step[], own: new Set<number>() };

/** Whether every revision after `base` up to `revision` is one this window made. */
const ours = (revision: number) => {
  for (let r = h.base + 1; r <= revision; r++) if (!h.own.has(r)) return false;
  return true;
};

/** Start the names over at `revision`: what came before it is unknown. */
const restart = (session: number, revision: number) => (h = { session, base: revision, undo: [], redo: [], own: new Set() });

/** An edit this window made has committed `revision`, as `step`: it is Undo's now, and redo is gone. */
export function recorded(session: number, revision: number, step: Step) {
  if (h.session !== session || !ours(revision - 1)) restart(session, revision - 1);
  h.undo.push(step);
  h.redo = [];
  h.own.add(revision);
}

/** This window's Undo (or Redo) committed `revision`: the step moves to the other stack. Returns it, if it was known. */
export function stepped(session: number, redo: boolean, revision: number): Step | null {
  if (h.session !== session || !ours(revision - 1)) {
    restart(session, revision);
    return null;
  }
  const step = (redo ? h.redo : h.undo).pop() ?? null;
  if (step) (redo ? h.undo : h.redo).push(step);
  h.own.add(revision);
  return step;
}

/** The step Undo (or Redo) would take back in the song showing `revision`, if this window knows it. */
export function next(session: number, revision: number, redo: boolean): Step | null {
  if (h.session !== session || !ours(revision)) return null;
  return (redo ? h.redo : h.undo).at(-1) ?? null;
}
