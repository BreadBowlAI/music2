// Mute and solo while listening (8b.4, MON-1 to MON-4) and mute in the song (MON-5): the calls,
// what this window shows while they are on their way, and the words for them.
//
// Muting while listening is no edit: no undo step, no revision, no project file. The server keeps
// the sets in memory and in `.music2/overlays.json`, and answers each change with the state's
// `overlays`. Mute in the song is an edit: one undo step, the `muted` option in the track's code.

import { create } from "zustand";
import { moduleName, readJson, type GraphState } from "./graph";
import type { Heard, OverlayState } from "./graph8b";
import { editing, recorded } from "./history";
import { token } from "./native";
import { useStore } from "./store";

/** The tracks muted and soloed while listening, by key. */
export interface Sets {
  mute: string[];
  solo: string[];
}

/**
 * What this window asked for and the server hasn't answered yet, or null. The buttons and the pill
 * show it at once, and a state the page fetched meanwhile (older) doesn't flicker them back.
 */
const useAsked = create<{ asked: Sets | null }>(() => ({ asked: null }));

const NONE: string[] = [];

/** The sets as this window shows them: what it asked for, else the state's. */
export function useSets(): Sets {
  const asked = useAsked((s) => s.asked);
  const mute = useStore((s) => s.graph?.overlays?.mute ?? NONE);
  const solo = useStore((s) => s.graph?.overlays?.solo ?? NONE);
  return asked ?? { mute, solo };
}

function sets(): Sets {
  const asked = useAsked.getState().asked;
  if (asked) return asked;
  const o = useStore.getState().graph?.overlays;
  return { mute: o?.mute ?? [], solo: o?.solo ?? [] };
}

/** How `module` is heard, as the state says now. */
const heardOf = (module: string): Heard => useStore.getState().graph?.overlays?.heard[module] ?? "heard";

/** Whether `module` is a track of the song playing: the pill counts only those (an entry of a track that is gone stays, MON-4). */
export const inSong = (o: OverlayState | undefined, module: string) => !o || module in o.heard;

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

/** POST `body` to a graph route; an error answer throws with the server's words. */
async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`/api/graph${path}`, { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(body) });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as T;
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

/** A change as the overlay route takes it: `clear` first, then `exclusive`, then each mute and solo. */
interface Change {
  mute?: Record<string, boolean>;
  solo?: Record<string, boolean>;
  exclusive?: string;
  clear?: boolean;
}

/** Requests sent; only the newest one's answer is shown (M6). */
let sent = 0;
/** One request at a time, in the order asked: two quick clicks on M land in that order. */
let queue: Promise<unknown> = Promise.resolve();

/** Ask for `change`, showing `next` until the server answers. A refusal puts the screen back and says why. */
function ask(change: Change, next: Sets) {
  const { graph, toast } = useStore.getState();
  if (!graph) return;
  const session = graph.session;
  useAsked.setState({ asked: next });
  const n = ++sent;
  queue = queue
    .then(() => post<OverlayState>("/overlay", { session, ...change }))
    .then(
      (overlays) => {
        if (n !== sent) return;
        useAsked.setState({ asked: null });
        const g = useStore.getState().graph;
        if (g?.session === session) useStore.getState().set({ graph: { ...g, overlays } });
      },
      (error) => {
        if (n === sent) useAsked.setState({ asked: null });
        toast(message(error));
      },
    );
}

const put = (list: string[], keys: string[], on: boolean) => (on ? [...new Set([...list, ...keys])].sort() : list.filter((k) => !keys.includes(k)));
const each = (keys: string[], on: boolean) => Object.fromEntries(keys.map((k) => [k, on]));

/** Mute (or unmute) `keys` while listening. */
export function setMute(keys: string[], on: boolean) {
  const s = sets();
  ask({ mute: each(keys, on) }, { ...s, mute: put(s.mute, keys, on) });
}

/** Solo (or unsolo) `keys`: a solo adds to the others. */
export function setSolo(keys: string[], on: boolean) {
  const s = sets();
  ask({ solo: each(keys, on) }, { ...s, solo: put(s.solo, keys, on) });
}

export const isMuted = (module: string) => sets().mute.includes(module);
export const isSoloed = (module: string) => sets().solo.includes(module);

/**
 * The M key and Track ▸ Mute: toggle the track's mute. A track muted in the song is skipped (its
 * mute is an edit, in its menu): the toast says where.
 */
export function toggleMute(module: string, name: string) {
  if (heardOf(module) === "muted-in-song") return useStore.getState().toast(`${name} is muted in the song. Unmute in song is in its menu.`);
  setMute([module], !isMuted(module));
}

export const toggleSolo = (module: string) => setSolo([module], !isSoloed(module));

/**
 * ⌥-click on S: only these tracks soloed (a strip's may be several: "Kick + Snare"); when they are
 * the only solos already, no solo at all.
 */
export function soloOnly(modules: string[]) {
  const s = sets();
  const only = [...new Set(modules)].sort();
  if (!only.length) return;
  if (s.solo.length === only.length && only.every((k) => s.solo.includes(k))) return setSolo(only, false);
  const [first, ...rest] = only;
  ask({ exclusive: first, solo: each(rest, true) }, { ...s, solo: only });
}

/** The pill's ×: the track's mute and solo both off. */
export function release(module: string) {
  const s = sets();
  ask({ mute: { [module]: false }, solo: { [module]: false } }, { mute: put(s.mute, [module], false), solo: put(s.solo, [module], false) });
}

/** The pill's Clear all: every mute and solo off. */
export const clearAll = () => ask({ clear: true }, { mute: [], solo: [] });

/** "Duck", "Duck and Filter": what a track drives besides its sound (MON-2), which a mute leaves on. Null when nothing. */
export function drives(o: OverlayState | undefined, module: string): string | null {
  const names = [...new Set((o?.feeds[module] ?? []).map((f) => f.label))];
  return names.length ? listed(names) : null;
}

/** "Kick", "Kick and Hats", "Kick, Hats and Bass". */
export const listed = (names: string[]) => (names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names.at(-1)}` : (names[0] ?? ""));

/** What the mutes pill says: "1 muted", "2 muted · 1 solo", "1 solo"; empty when nothing is on. Tracks no longer in the song aren't counted. */
export function pillWords(o: OverlayState | undefined, s: Sets) {
  const m = s.mute.filter((k) => inSong(o, k)).length;
  const n = s.solo.filter((k) => inSong(o, k)).length;
  return [m ? `${m} muted` : "", n ? `${n} ${n === 1 ? "solo" : "solos"}` : ""].filter(Boolean).join(" · ");
}

/** Whether the mutes pill shows now. */
export function usePill() {
  const s = useSets();
  const o = useStore((st) => st.graph?.overlays);
  return pillWords(o, s) !== "";
}

/** A track's name on screen, by its key. */
export function trackName(graph: GraphState, module: string) {
  const t = graph.timeline.tracks.find((x) => x.module === module);
  return moduleName({ key: module, label: t?.label ?? graph.view.modules.find((m) => m.key === module)?.label });
}

/** The tracks in timeline order, by key. */
const order = (graph: GraphState) => graph.timeline.tracks.filter((t) => t.track && t.module !== "song").map((t) => t.module);

/** What listening changes in what you hear: the tracks muted only while you listen, and the soloed ones while a solo leaves a track out. */
export interface Listening {
  muted: string[];
  soloed: string[];
  mutedNames: string[];
  soloedNames: string[];
}

/**
 * What the mutes and solos while listening change in what you hear now (the export's question,
 * EX-3), or null when nothing: a track muted only while you listen, or one a solo leaves out. A
 * track muted in the song is the song's, and the export leaves it out too.
 */
export function listening(graph: GraphState): Listening | null {
  const o = graph.overlays;
  if (!o) return null;
  const heard = Object.values(o.heard);
  if (!heard.some((h) => h === "muted" || h === "silenced")) return null;
  const keys = order(graph);
  const muted = keys.filter((k) => o.heard[k] === "muted");
  const soloed = heard.includes("silenced") ? keys.filter((k) => o.solo.includes(k)) : [];
  const name = (k: string) => trackName(graph, k);
  return { muted, soloed, mutedNames: muted.map(name), soloedNames: soloed.map(name) };
}

/** "is" or "are", for `names`. */
export const is = (names: string[]) => (names.length > 1 ? "are" : "is");

/**
 * What an export made while listening differed says in its done toast (Q4): the file has every
 * track. "Kick was muted only while you listened, so it's in the file."
 */
export function listenedWords(l: Listening) {
  const was = (names: string[]) => (names.length > 1 ? "were" : "was");
  if (!l.soloed.length) return `${listed(l.mutedNames)} ${was(l.mutedNames)} muted only while you listened, so ${l.muted.length > 1 ? "they're" : "it's"} in the file.`;
  const solo = `only ${listed(l.soloedNames)} ${was(l.soloedNames)} soloed`;
  const head = l.muted.length ? `${listed(l.mutedNames)} ${was(l.mutedNames)} muted and ${solo}` : solo[0].toUpperCase() + solo.slice(1);
  return `${head} while you listened, so the file has every track.`;
}

/** An edit's answer: the revision it made, and what it changed, in words. */
export interface Edited {
  file: string;
  before: string;
  after: string;
  revision: number;
  /** The undo step it made, by its id (AGENT-2): what its toast's Undo takes back. */
  step?: number;
}

/**
 * Mute `modules` in the song, or unmute them (MON-5): the `muted` option in their code, one Tx and
 * one undo step, which Undo names "mute in song" ("Undid mute in song"). A write: refused in a
 * read-only window (423) and while the code doesn't compile, in words.
 */
export async function muteInSongCall(session: number, modules: string[], muted: boolean) {
  const answered = editing();
  try {
    const r = await post<Edited>("/track/mute", { session, modules, muted });
    if (r.before !== r.after) recorded(session, r, { name: muted ? "mute in song" : "unmute in song", structural: true });
    return r;
  } finally {
    answered();
  }
}
