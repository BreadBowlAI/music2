// An effect card's ⋯ menu (lane D of next): turn an effect off and on again (D1), move it before
// or after the effect written next to it (D2); and what a Duck listens to (D3). Each is one undo
// step on the server, a `TextEdit` of the effects' own calls; a refusal says why in words. What
// the view says of an effect card is added to graph.ts's types from here, as graph8b.ts adds its
// own.

import { lockReason, readJson, type ViewNode } from "./graph";
import { editing, goes, recorded, type Step } from "./history";
import { token } from "./native";
import { useStore } from "./store";
import { http } from "./http";

/** An effect card's ⋯ menu, as the view says it: each row's words when it can't run here. */
export interface EffectMenu {
  /** Why Turn off (or Turn on, when it is off) can't run here. */
  bypass?: string;
  /** Move left: before the effect it is written around. */
  left: MoveRow;
  /** Move right: after the effect written around it. */
  right: MoveRow;
}

/** A Move row: the effect it swaps with ("Reverb"), or why it can't, in words. */
export interface MoveRow {
  with?: string;
  refused?: string;
}

/** A Duck's "Listens to" row: the track it hears now, the ones it could, or why it can't change here. */
export interface Listens {
  /** The card's input row it is (the Duck's side input). */
  port: string;
  /** The track whose sound it ducks, by name. */
  ducks?: string;
  /** The track it listens to now, by key. */
  track?: string;
  /** The tracks it can listen to: key and name. */
  choices: [string, string][];
  refused?: string;
}

declare module "./graph" {
  interface ViewNode {
    /** A Duck's "Listens to" row. */
    listens?: Listens;
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
  /** The undo step it made, by its id (AGENT-2): what its toast's Undo takes back. */
  step?: number;
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

/** POST an effect edit; an error answer throws with the server's words. */
async function post(path: string, body: unknown): Promise<Edited> {
  const res = await http(path, { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(body) });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as Edited;
}

/**
 * Run an edit of effect card `body.node`, as `step`: Undo's once it commits; a refusal's words in a
 * toast. It goes through the gate (lib/history): once the window shows its own last edit.
 */
async function edit(path: string, body: { node: string } & Record<string, unknown>, step: Step) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return toast(`${why}.`);
  const ours = editing();
  try {
    const revision = await goes(graph.session, graph.view.revision, [body.node], ours);
    const r = await post(path, { session: graph.session, revision, ...body });
    if (r.before !== r.after) recorded(graph.session, r, step);
  } catch (error) {
    toast(message(error));
  } finally {
    ours.end();
  }
}

/**
 * Turn effect card `node` (called `title`, "Reverb") off, or on again (`off` false). Its card says
 * so at once (dimmed, "Reverb · Off"), so there is no toast: one undo step, "turning Reverb off".
 */
export function turnEffect(node: ViewNode, title: string, off: boolean) {
  return edit("/api/graph/effect/bypass", { node: node.id, on: off }, { name: `turning ${title} ${off ? "off" : "on"}` });
}

/**
 * Move effect card `node` (called `title`) left, before `other` (`earlier`), or right, after it.
 * The cards swap on screen, so there is no toast: one undo step, "moving Delay before Reverb".
 */
export function moveEffect(node: ViewNode, title: string, other: string, earlier: boolean) {
  return edit("/api/graph/effect/move", { node: node.id, earlier }, { name: `moving ${title} ${earlier ? "before" : "after"} ${other}` });
}

/**
 * Make Duck card `node` listen to track `key` (called `name`). The row says so at once, so there is
 * no toast: one undo step, "Bass's duck listens to Hats".
 */
export function listenTo(node: ViewNode, key: string, name: string) {
  const whose = node.listens?.ducks ? `${node.listens.ducks}'s duck` : "the duck";
  return edit("/api/graph/effect/sidechain", { node: node.id, track: key }, { name: `${whose} listens to ${name}` });
}
