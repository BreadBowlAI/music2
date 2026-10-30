// The library (LIB-1 to LIB-3): everything that can be added to a song, found by the server's one
// search (`GET /api/library`), which ⌘F's panel and the side browser's field both ask. A preview is
// the server's offline render of the item (`GET /api/library/preview`), played here with an
// <audio> element: it never goes through the song's engine.

import { useCallback, useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { readJson } from "./graph";
import { token } from "./native";

export type ItemKind = "instrument" | "sound" | "effect" | "sample" | "plugin";

/** An item, as the server's search answers it. */
export interface LibraryItem {
  kind: ItemKind;
  /** `std:synth-pluck`, `clap:<id>`, `sound:<from>/<sound>/<name>`. */
  id: string;
  label: string;
  category: string;
  description: string;
  /** More words it is found by; a sound's first is its sound's label ("Synth pluck"). */
  tags: string[];
  /** `std`, `song`, `yours`, `plugin`, or a pack. */
  package: string;
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
  /** A sound's preset: music2's own (`std`) or yours. */
  preset?: { from: "std" | "yours"; name: string };
  /** A sample's type, tempo, key and length (packs bring them). */
  sample?: { type: "loop" | "one-shot"; bpm: number | null; key: string | null; seconds: number | null };
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

/** The items `q` finds, best first. */
export async function searchLibrary(q: string): Promise<LibraryItem[]> {
  const res = await fetch(`/api/library?q=${encodeURIComponent(q)}`, { headers: auth });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data.items ?? [];
}

/** What the search found, with the words it answers: a page never takes an older search's rows for a newer's. */
export interface Found {
  q: string;
  items: LibraryItem[];
  /** Why the search failed, in words. */
  error?: string;
}

/**
 * The library's search for `query` (none when null), asked as typing rests and again when `again`
 * changes (the plugin list came). `found` is the newest answer with the words it answers: an
 * answer to older words is dropped. `now()` answers the words as typed now, at once: the answer on
 * its way for them, else asked now, so Enter never acts on rows of an older search.
 */
export function useSearch(query: string | null, again: unknown, delay = 60) {
  const [found, setFound] = useState<Found | null>(null);
  const asking = useRef<{ q: string; answer: Promise<LibraryItem[]> } | null>(null);
  const ask = useCallback((q: string) => {
    if (asking.current?.q === q) return asking.current.answer;
    const mine = { q, answer: searchLibrary(q) };
    asking.current = mine;
    mine.answer.then(
      (items) => asking.current === mine && setFound({ q, items }),
      (e) => asking.current === mine && setFound({ q, items: [], error: e instanceof Error ? e.message : String(e) }),
    );
    return mine.answer;
  }, []);
  useEffect(() => {
    // Other words, or the same asked again: what is on its way no longer lands.
    asking.current = null;
    if (query === null) return;
    const t = window.setTimeout(() => void ask(query), delay);
    return () => window.clearTimeout(t);
  }, [query, again, delay, ask]);
  const now = () => (query === null ? Promise.resolve([]) : ask(query));
  return { found, now };
}

/** Where an item comes from, in words: "music2", "Yours", "This song", a plugin's maker. */
export function fromWords(item: LibraryItem): string {
  if (item.package === "std" || item.package.startsWith("@m2/")) return "music2";
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
    const res = await fetch(`/api/library/preview?id=${encodeURIComponent(item.id)}`, { headers: auth });
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
