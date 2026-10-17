// An effect card's ⋯ menu (lane D of next): turn an effect off and on again (D1). Each is one undo
// step on the server, a `TextEdit` of the effect's own call; a refusal says why in words. What the
// view says of an effect card is added to graph.ts's types from here, as graph8b.ts adds its own.

import { lockReason, readJson, type ViewNode } from "./graph";
import { recorded, type Step } from "./history";
import { token } from "./native";
import { useStore } from "./store";

/** An effect card's ⋯ menu, as the view says it: each row's words when it can't run here. */
export interface EffectMenu {
  /** Why Turn off (or Turn on, when it is off) can't run here. */
  bypass?: string;
}

declare module "./graph" {
  interface ViewNode {
    /** An effect turned off: `bypass: true` in its call. Its sound goes on without it. */
    bypassed?: boolean;
    /** What its ⋯ menu offers, on an effect's card. */
    menu?: EffectMenu;
  }
  interface PortEdge {
    /** From an effect turned off: what reads the call hears the effect's input, drawn from its card. */
    bypassed?: boolean;
  }
}

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

/** An edit's answer: the revision it made, and what it changed, in words. */
interface Edited {
  file: string;
  before: string;
  after: string;
  revision: number;
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

/** POST an effect edit; an error answer throws with the server's words. */
async function post(path: string, body: unknown): Promise<Edited> {
  const res = await fetch(path, { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(body) });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as Edited;
}

/** Run an edit of the open song, as `step`: Undo's once it commits; a refusal's words in a toast. */
async function edit(path: string, body: Record<string, unknown>, step: Step) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return toast(`${why}.`);
  try {
    const r = await post(path, { session: graph.session, revision: graph.view.revision, ...body });
    if (r.before !== r.after) recorded(graph.session, r.revision, step);
  } catch (error) {
    toast(message(error));
  }
}

/**
 * Turn effect card `node` (called `title`, "Reverb") off, or on again (`off` false). Its card says
 * so at once (dimmed, "Reverb · Off"), so there is no toast: one undo step, "turning Reverb off".
 */
export function turnEffect(node: ViewNode, title: string, off: boolean) {
  return edit("/api/graph/effect/bypass", { node: node.id, on: off }, { name: `turning ${title} ${off ? "off" : "on"}` });
}
