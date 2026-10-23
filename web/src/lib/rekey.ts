// Change a track's ID (polish A5, design-8b §11, taste.md:120): the dialog's state and calls, the
// toast with Undo, and the window following a track to its new ID. The UI never calls an ID a
// "key": "key" means only the musical key there.
//
// The ID names a track in code (`track("bass", …)`) and files its data (`data/bass/`). Changing it
// moves both, its mixer channel's name when that is the same, and its layout entry, as one undo
// step on the server. Its name, colour, mute and solo stay; its sounds restart.

import { create } from "zustand";
import { undoAction } from "../components/GraphCanvas";
import { lockReason, moduleName, readJson, type ClipRef, type GraphState } from "./graph";
import { recorded } from "./history";
import { getLocal, setLocal } from "./local";
import { token } from "./native";
import { useShell } from "./shell";
import { useStore } from "./store";

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`/api/graph${path}`, { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(body) });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as T;
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

/** The dialog: the track whose ID is being changed, and its name on screen. */
export const useTrackId = create<{ asking: { module: string; name: string } | null }>(() => ({ asking: null }));

/** What a track is called on screen. */
export function nameOf(graph: GraphState, module: string) {
  const t = graph.timeline.tracks.find((x) => x.module === module);
  return moduleName({ key: module, label: t?.label ?? graph.view.modules.find((m) => m.key === module)?.label });
}

/** Change track ID…: the dialog for `module`, unless the song can't be edited now (the toast says why). */
export function openTrackId(module: string) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return toast(`${why}.`);
  useTrackId.setState({ asking: { module, name: nameOf(graph, module) } });
}

/** The tracks whose code is in `path`: a file's track() calls, by where most of each track's nodes are. */
export function tracksIn(graph: GraphState, path: string) {
  return graph.timeline.tracks
    .filter((t) => t.track && t.module !== "song")
    .filter((t) => {
      const count = new Map<string, number>();
      for (const n of graph.view.modules.find((m) => m.key === t.module)?.nodes ?? []) if (n.file) count.set(n.file, (count.get(n.file) ?? 0) + 1);
      return [...count].sort((a, b) => b[1] - a[1])[0]?.[0] === path;
    })
    .map((t) => ({ module: t.module, name: nameOf(graph, t.module) }));
}

/** Whether `id` can be `module`'s new ID: null, or why not, in ID words, from the server. */
export async function checkId(module: string, id: string): Promise<string | null> {
  const graph = useStore.getState().graph;
  if (!graph) return null;
  const r = await post<{ ok: boolean; error?: string }>("/track/rekey/check", { session: graph.session, module, key: id });
  return r.ok ? null : (r.error ?? "That can't be an ID");
}

/** Change `module`'s ID to `id`: one undo step, said in a toast with Undo ("Bass's ID is low now"). Resolves to the refusal's words, or null. */
export async function changeId(module: string, id: string): Promise<string | null> {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return null;
  const why = lockReason(graph, connected);
  if (why) return `${why}.`;
  const name = nameOf(graph, module);
  try {
    const r = await post<{ revision: number; step?: number; before: string; after: string }>("/track/rekey", { session: graph.session, module, key: id });
    // The window follows the track once the graph has its new ID (below): not before, or the
    // timeline, not knowing it yet, would pick another track.
    recorded(graph.session, r.revision, { name: `changing ${name}'s ID`, structural: true });
    toast(`${name}'s ID is ${id} now`, undoAction(graph.session, r.step));
    return null;
  } catch (error) {
    return message(error);
  }
}

/** `id` (a track's, or a lane's `bass.cutoff`) with the track's ID `from` changed to `to`. */
const moved = (id: string, from: string, to: string) => (id === from ? to : id.startsWith(`${from}.`) ? `${to}${id.slice(from.length)}` : id);

/**
 * The song's view settings follow a track from ID `from` to `to` (X20): its height and its lanes'
 * (`heights`, by ID), and its hidden lanes (`hidden`), kept in `.music2/view.json`.
 */
function followView(from: string, to: string) {
  const heights = getLocal<Record<string, unknown> | null>("heights", null);
  if (heights && typeof heights === "object" && Object.keys(heights).some((k) => moved(k, from, to) !== k)) {
    setLocal("heights", Object.fromEntries(Object.entries(heights).map(([k, v]) => [moved(k, from, to), v])));
  }
  const hidden = getLocal<unknown>("hidden", null);
  if (Array.isArray(hidden) && hidden.some((h) => typeof h === "string" && moved(h, from, to) !== h)) {
    setLocal("hidden", hidden.map((h) => (typeof h === "string" ? moved(h, from, to) : h)));
  }
}

/** The window follows a track from ID `from` to `to`: what is selected, shown, its selected clip and its open lanes, and its view settings. */
function follow(from: string, to: string) {
  followView(from, to);
  const s = useShell.getState();
  const swap = (k: string | null) => (k === from ? to : k);
  const clips: Record<string, ClipRef> = {};
  for (const [k, c] of Object.entries(s.clips)) clips[k === from ? to : k] = k === from ? { ...c, module: to } : c;
  const open = new Set([...s.open].map((k) => (k === from ? to : k)));
  if (s.selected === from || s.module === from || from in s.clips || s.open.has(from)) useShell.setState({ selected: swap(s.selected), module: swap(s.module), clips, open });
}

// An undo, a redo or a rename by hand changes an ID too: when one track's ID goes and one comes in
// the same song, the window follows it (X20).
let last: { session: number; keys: string[] } | null = null;
useStore.subscribe((st) => {
  const g = st.graph;
  if (!g) return void (last = null);
  const keys = g.timeline.tracks.filter((t) => t.track).map((t) => t.module);
  const was = last;
  last = { session: g.session, keys };
  if (!was || was.session !== g.session || was.keys.join() === keys.join()) return;
  const gone = was.keys.filter((k) => !keys.includes(k));
  const came = keys.filter((k) => !was.keys.includes(k));
  if (gone.length === 1 && came.length === 1) follow(gone[0], came[0]);
});
