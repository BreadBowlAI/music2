// The page's one way to the server (AGENT-2): every request goes through `http`, which keeps the
// undo step an answer names as this window's own as soon as the answer comes, whatever the route
// (an import, a Sampler's file, a clip gesture, a knob's value): so a ⌘Z right after takes it back
// before the window shows it, and no route the page starts to call can forget it. The revision a
// change of the window's own made (an edit's, an undo's, a redo's) is one its next gesture waits to
// show (`shown`, lib/history).
// `the_page_keeps_the_step_of_every_route_it_calls` (crates/server) checks no request goes another way.

import { changed, flight, owned, type StepName } from "./history";
import { useStore } from "./store";

/** The routes whose answers name the step they took back, not one they made: never kept as the window's own. */
const TAKES_BACK = ["/api/graph/undo", "/api/graph/redo", "/api/graph/undo/skip"];

/** This window, as every request of its says (`x-m2-window`, AGENT-2): the server keeps which window made each revision, and judges a gesture's target only by others' changes. */
export const windowId: string = typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `w${Date.now()}${Math.random()}`;

/** `fetch` for a request of the page's to the server: it says its window, and an answer that names a step is kept first. */
export async function http(url: string, init?: RequestInit): Promise<Response> {
  const path = new URL(url, location.href).pathname;
  const post = (init?.method ?? "GET") !== "GET";
  // An undo or a redo is a change on its way until its answer is kept: a gesture waits for it.
  const landed = post && TAKES_BACK.includes(path) ? flight() : null;
  try {
    const headers = new Headers(init?.headers);
    headers.set("x-m2-window", windowId);
    const res = await fetch(url, { ...init, headers });
    if (post && res.ok) await keep(path, res);
    return res;
  } finally {
    landed?.();
  }
}

/** What an answer says at its top of the song's history (AGENT-2): the revision the change made (none when it made none), its step, the files whose ids it moved, and whether the revision also took in a change made elsewhere. */
interface Answer {
  revision?: unknown;
  step?: unknown;
  moved?: unknown;
  mixed?: unknown;
  beats?: unknown;
  undo_name?: StepName | null;
}

async function keep(path: string, res: Response) {
  const session = useStore.getState().graph?.session;
  if (session === undefined || !res.headers.get("content-type")?.includes("json")) return;
  const answer = (await res.clone().json().catch(() => null)) as Answer | null;
  if (!answer || typeof answer !== "object") return;
  // Only a change's answer says `moved`: then its `revision` is the one the change made (another
  // answer's, as naming a step's, is the one shown).
  const moved = Array.isArray(answer.moved) ? answer.moved.filter((m): m is string => typeof m === "string") : undefined;
  const revision = moved !== undefined && typeof answer.revision === "number" ? answer.revision : undefined;
  const mixed = answer.mixed === true;
  const beats = answer.beats === true;
  // An undo, a redo: a change of the window's own, whose step isn't.
  if (TAKES_BACK.includes(path)) return void (revision !== undefined && changed(session, revision, moved!, mixed, beats));
  if (typeof answer.step === "number") owned(session, { revision, step: answer.step, moved, mixed, beats, undo_name: answer.undo_name });
  else if (revision !== undefined) changed(session, revision, moved!, mixed, beats);
}
