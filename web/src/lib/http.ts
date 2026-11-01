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

/** `fetch` for a request of the page's to the server: an answer that names a step is kept first. */
export async function http(url: string, init?: RequestInit): Promise<Response> {
  const path = new URL(url, location.href).pathname;
  const post = (init?.method ?? "GET") !== "GET";
  // An undo or a redo is a change on its way until its answer is kept: a gesture waits for it.
  const landed = post && TAKES_BACK.includes(path) ? flight() : null;
  try {
    const res = await fetch(url, init);
    if (post && res.ok) await keep(path, res);
    return res;
  } finally {
    landed?.();
  }
}

/** What an answer names of the song's history: the revision a change made, and its step. */
interface Answer {
  revision?: unknown;
  step?: unknown;
  undo_name?: StepName | null;
  file?: string;
}

async function keep(path: string, res: Response) {
  const session = useStore.getState().graph?.session;
  if (session === undefined || !res.headers.get("content-type")?.includes("json")) return;
  const answer = (await res.clone().json().catch(() => null)) as Answer | null;
  if (!answer || typeof answer.revision !== "number") return;
  // An undo, a redo: a change of the window's own, whose step isn't. An answer with no step changed
  // nothing: the revision it names was made by whoever made it.
  if (TAKES_BACK.includes(path)) return changed(session, answer.revision, answer.file);
  if (typeof answer.step === "number") owned(session, { revision: answer.revision, step: answer.step, undo_name: answer.undo_name, file: answer.file });
}
