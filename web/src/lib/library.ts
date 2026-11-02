// The library (LIB-1 to LIB-3): everything that can be added to a song, found by the server's one
// search (`GET /api/library`), which ⌘F's panel and the side browser's field both ask. A preview is
// the server's offline render of the item (`GET /api/library/preview`), played here with an
// <audio> element: it never goes through the song's engine.

import { useCallback, useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { on } from "./events";
import { readJson } from "./graph";
import { token } from "./native";
import { http } from "./http";

export type ItemKind = "instrument" | "sound" | "effect" | "sample" | "plugin";

/** An item, as the server's search answers it. */
export interface LibraryItem {
  kind: ItemKind;
  /** `@m2/synths:synth-pluck`, `clap:<id>`, `sound:<from>/<sound>/<name>`. */
  id: string;
  label: string;
  category: string;
  description: string;
  /** More words it is found by; a sound's first is its sound's label ("Synth pluck"). */
  tags: string[];
  /** `std`, `song`, `yours`, `plugin`, or a pack. */
  package: string;
  /** It is music2's own: std's, or a pack version music2 comes with (PACK-19), whatever its name. */
  own?: boolean;
  /** What adding it names; for a sound, the sound it is for. */
  component: string;
  /** Adding it makes a new track. */
  instrument: boolean;
  /** It goes on a track's sound: an effect, an effect plugin, a sound of an effect. */
  effect: boolean;
  /** It has a preview. */
  preview: boolean;
  /** Why it has none, in words, when it has none. */
  no_preview?: string;
  /** A sound's preset: its package's own (`std`) or yours. */
  preset?: { from: "std" | "yours"; name: string };
  /** A sample's type, tempo, key and length, what was measured of its file, and where it is from (LIB-4). */
  sample?: Sample;
  /** From the library (REG-13): not on this computer yet; Get fetches `size` bytes. Its id is `library:<id>`. */
  library?: FromLibrary;
}

/** What a library item's Get gets (REG-13): its pack's version, licence and authors, the bytes it fetches, and the older version you have, if any. */
export interface FromLibrary {
  pack: string;
  version: string;
  license: string;
  authors: string[];
  size: number;
  have: string | null;
}

/** What the search says of the registry (REG-12): whether its catalog is being fetched, when the one here was made, and why the last fetch failed. Null with no registry set. */
export interface RegistrySaid {
  fetching: boolean;
  made: string | null;
  error: string | null;
}

/** What a sample is (LIB-4): said or guessed (type, instrument, tempo, key), and measured (the rest). */
export interface Sample {
  type: "loop" | "one-shot";
  bpm: number | null;
  key: string | null;
  seconds: number | null;
  instrument: string | null;
  channels: number;
  rate: number;
  peak: number;
  hash: string;
  /** Where it came from, as its note says: `{pack, version, file, author, license}`, or `{folder: true}`. */
  from: { pack?: string; version?: string; file?: string; author?: string; license?: string; folder?: boolean };
}

/** The groups the panel shows, in order, by kind. */
export const GROUPS: { kind: ItemKind; name: string }[] = [
  { kind: "instrument", name: "Instruments" },
  { kind: "sound", name: "Sounds" },
  { kind: "effect", name: "Effects" },
  { kind: "sample", name: "Samples" },
  { kind: "plugin", name: "Plugins" },
];

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

/** What the server says of your sample folders with each answer: how many are being indexed, and the files skipped (not WAV). */
export interface FoldersSaid {
  indexing: number;
  not_wav: number;
}

/** What a search answers: the items you have, best first, then From the library's (REG-13). */
interface Answer {
  items: LibraryItem[];
  library: LibraryItem[];
  libraryMore: number;
  registry: RegistrySaid | null;
  folders?: FoldersSaid;
}

/** The items `q` finds, best first; with `fits` (the playhead's beat), only loops that fit the song there (LIB-1). */
export async function searchLibrary(q: string, fits: number | null = null): Promise<Answer> {
  const at = fits === null ? "" : `&fits=${fits}`;
  const res = await http(`/api/library?q=${encodeURIComponent(q)}${at}`, { headers: auth });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return { items: data.items ?? [], library: data.library ?? [], libraryMore: data.library_more ?? 0, registry: data.registry ?? null, folders: data.folders };
}

/** What the search found, with the words it answers: a page never takes an older search's rows for a newer's. */
export interface Found {
  q: string;
  items: LibraryItem[];
  /** From the library: what you can get, after what you have (REG-13). */
  library: LibraryItem[];
  /** The registry, as the answer found it: null with none set. */
  registry?: RegistrySaid | null;
  /** How many more From the library found than it answers (REG-13: at most 20 rows). */
  libraryMore?: number;
  /** Your sample folders, as the answer found them. */
  folders?: FoldersSaid;
  /** Why the search failed, in words. */
  error?: string;
}

/**
 * The library's search for `query` (none when null), asked as typing rests and again when `again`
 * changes (the plugin list came). `found` is the newest answer with the words it answers: an
 * answer to older words is dropped. `now()` answers the words as typed now, at once: the answer on
 * its way for them, else asked now, so Enter never acts on rows of an older search. Its rows are
 * what you have, then From the library's.
 */
export function useSearch(query: string | null, again: unknown, delay = 60, fits: number | null = null) {
  const [found, setFound] = useState<Found | null>(null);
  const asking = useRef<{ q: string; answer: Promise<LibraryItem[]> } | null>(null);
  const fitsAt = useRef(fits);
  fitsAt.current = fits;
  const ask = useCallback((q: string) => {
    if (asking.current?.q === q) return asking.current.answer;
    const answer = searchLibrary(q, fitsAt.current);
    const mine = { q, answer: answer.then((a) => [...a.items, ...a.library]) };
    asking.current = mine;
    answer.then(
      (a) => asking.current === mine && setFound({ q, items: a.items, library: a.library, libraryMore: a.libraryMore, registry: a.registry, folders: a.folders }),
      (e) => asking.current === mine && setFound({ q, items: [], library: [], error: e instanceof Error ? e.message : String(e) }),
    );
    return mine.answer;
  }, []);
  useEffect(() => {
    // Other words, Fits my song turned on or off, or the same asked again: what is on its way no longer lands.
    asking.current = null;
    if (query === null) return;
    const t = window.setTimeout(() => void ask(query), delay);
    return () => window.clearTimeout(t);
  }, [query, again, delay, ask, fits]);
  const now = () => (query === null ? Promise.resolve([]) : ask(query));
  return { found, now };
}

/** A sample's facts in a few words: "Loop · 90 bpm · Am · 2.0 s". */
export function sampleWords(item: LibraryItem): string {
  const s = item.sample;
  if (!s) return item.category;
  const parts: string[] = [s.type === "loop" ? "Loop" : "One-shot"];
  if (s.bpm !== null) parts.push(`${Math.round(s.bpm * 10) / 10} bpm`);
  if (s.key) parts.push(s.key);
  if (s.seconds !== null) parts.push(`${s.seconds < 1 ? s.seconds.toFixed(2) : s.seconds.toFixed(1)} s`);
  return parts.join(" · ");
}

/** What a dragged sample row carries: its id. */
export const SAMPLE_DRAG = "application/x-m2-sample";

/** The sample being dragged from a list, while it is: a drag's data can't be read until its drop. */
let dragging: LibraryItem | null = null;
export const dragSample = (item: LibraryItem | null) => void (dragging = item);
export const draggedSample = () => dragging;

/** Your sample folders (LIB-5), as the server shows them. */
export interface SampleFolder {
  path: string;
  samples: number;
  not_wav: number;
  unreadable: number;
  indexing: boolean;
  missing: boolean;
}

/** Your sample folders; or, with `change`, add one or remove one first. */
export async function sampleFolders(change?: { add: string } | { remove: string }): Promise<SampleFolder[]> {
  const res = change
    ? await http("/api/library/folders", { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(change) })
    : await http("/api/library/folders", { headers: auth });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data.folders ?? [];
}

/** Where an item comes from, in words: "music2", "Yours", "This song", a plugin's maker; a pack's name, "music2" for a version music2 comes with (`own`, PACK-19). */
export function fromWords(item: LibraryItem): string {
  if (item.package === "std" || item.own) return "music2";
  if (item.package === "yours") return "Yours";
  if (item.package === "song") return "This song";
  if (item.kind === "plugin") return item.tags[0] || "Plugin";
  return item.package;
}

/** The preview playing, or being fetched: the item's id. One at a time, for every panel. */
export const usePreview = create<{ playing: string | null; loading: string | null }>(() => ({ playing: null, loading: null }));

let audio: HTMLAudioElement | null = null;
let url: string | null = null;
/** Counts asks, so a preview that arrives after another was asked for (or a stop) never plays. */
let asked = 0;

/** Stop the preview playing, if any. */
export function stopPreview() {
  asked++;
  audio?.pause();
  audio = null;
  if (url) URL.revokeObjectURL(url);
  url = null;
  usePreview.setState({ playing: null, loading: null });
}

/**
 * Play `item`'s preview, or stop it when it is the one playing (Space again). Fetched from the
 * server, which renders it the first time. Answers what to say: why there is none, in words; null
 * when it plays or stopped; undefined when another ask (or a stop) came since, which says its own.
 */
export async function togglePreview(item: LibraryItem): Promise<string | null | undefined> {
  const { playing, loading } = usePreview.getState();
  stopPreview();
  if (playing === item.id || loading === item.id) return null;
  if (!item.preview) return item.no_preview ?? `${item.label} has no preview.`;
  const mine = asked;
  usePreview.setState({ loading: item.id });
  try {
    const res = await http(`/api/library/preview?id=${encodeURIComponent(item.id)}`, { headers: auth });
    if (!res.ok) throw new Error((await readJson(res)).error ?? `HTTP ${res.status}`);
    const blob = await res.blob();
    if (mine !== asked) return undefined;
    url = URL.createObjectURL(blob);
    audio = new Audio(url);
    const done = () => {
      if (mine === asked) stopPreview();
    };
    audio.onended = done;
    usePreview.setState({ loading: null, playing: item.id });
    // A page that may not play sound yet (no gesture) just stays quiet.
    await audio.play().catch(done);
    return mine === asked ? null : undefined;
  } catch (e) {
    if (mine !== asked) return undefined;
    usePreview.setState({ loading: null });
    return e instanceof Error ? e.message : String(e);
  }
}

/** ⌘F's panel: open or not. */
export const useFinding = create<{ open: boolean }>(() => ({ open: false }));

/** A size in words: "860 KB", "4.2 MB". */
export function sizeWords(bytes: number): string {
  if (bytes < 1_000_000) return `${Math.max(1, Math.round(bytes / 1000))} KB`;
  if (bytes < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

/**
 * The Get running (REG-15), if one is: the library item's id and label, how far it is, and whether
 * it is being installed, when it can no longer be cancelled. Every window's, from the server's
 * `get` events.
 */
export const useGetting = create<{ id: string; label: string; done: number; total: number; installing: boolean } | null>(() => null);

on("get", (msg: { id: string; label?: string; done?: number; total?: number; installing?: boolean; end?: boolean }) => {
  if (msg.end) {
    if (useGetting.getState()?.id === msg.id) useGetting.setState(null, true);
    return;
  }
  useGetting.setState({ id: msg.id, label: msg.label ?? msg.id, done: msg.done ?? 0, total: msg.total ?? 0, installing: !!msg.installing }, true);
});

/** What a Get answers: a pack's `got` and the id its item has once installed; a sample's add, as Add sample's. */
export interface Got {
  got: { pack: string; version: string; already?: boolean };
  id?: string | null;
  [k: string]: unknown;
}

/**
 * Get library item `item` (REG-15, REG-16): a pack's whole version installed, or a sample added
 * to the open song at `to` (with `session`). Answers once it is done; `useGetting` says how far.
 */
export async function getFromLibrary(item: LibraryItem, sample?: { session: number; to: unknown }): Promise<Got> {
  useGetting.setState({ id: item.id, label: item.label, done: 0, total: item.library?.size ?? 0, installing: false }, true);
  try {
    const body = sample ? { id: item.id, session: sample.session, to: sample.to } : { id: item.id };
    const res = await http("/api/library/get", { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(body) });
    const data = await readJson(res);
    if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
    return data as Got;
  } finally {
    if (useGetting.getState()?.id === item.id) useGetting.setState(null, true);
  }
}

/** Get version `version` of `pack`, which the open song locks and hasn't got (REG-17). */
export async function getMissing(pack: string, version: string): Promise<Got> {
  const id = `${pack}@${version}`;
  useGetting.setState({ id, label: `${pack} ${version}`, done: 0, total: 0, installing: false }, true);
  try {
    const res = await http("/api/library/get", { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify({ pack, version }) });
    const data = await readJson(res);
    if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
    return data as Got;
  } finally {
    if (useGetting.getState()?.id === id) useGetting.setState(null, true);
  }
}

/** Stop the Get running: nothing of it is kept. Once it is being installed it can't be: why, in words. */
export async function cancelGet(): Promise<string | null> {
  const res = await http("/api/library/get/cancel", { method: "POST", headers: { "content-type": "application/json", ...auth }, body: "{}" });
  const data = await readJson(res);
  return data.cancelled === false && data.why ? data.why : null;
}

/** A version the open song locks that isn't installed (REG-17): whether the library has it (`can`, and the bytes a Get fetches), or why not. */
export interface MissingPack {
  pack: string;
  version: string;
  hash: string;
  can: boolean;
  size?: number;
  why?: string;
}

/** The open song's locked versions that aren't installed. */
export async function missingPacks(): Promise<MissingPack[]> {
  const res = await http("/api/library/missing", { headers: auth });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data.missing ?? [];
}
