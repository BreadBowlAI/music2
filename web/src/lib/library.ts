// The library (LIB-1 to LIB-3): everything that can be added to a song, found by the server's one
// search (`GET /api/library`), which ⌘F's panel and the side browser's field both ask. A preview is
// the server's offline render of the item (`GET /api/library/preview`), played here with an
// <audio> element: it never goes through the song's engine.

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
  /** It has a preview. */
  preview: boolean;
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
 * server, which renders it the first time; why there is none comes back in words.
 */
export async function togglePreview(item: LibraryItem): Promise<string | null> {
  const { playing, loading } = usePreview.getState();
  stopPreview();
  if (playing === item.id || loading === item.id) return null;
  if (!item.preview) return item.kind === "plugin" || item.component.startsWith("clap:") ? "Plugins have no preview yet." : `${item.label} has no preview.`;
  const mine = asked;
  usePreview.setState({ loading: item.id });
  try {
    const res = await fetch(`/api/library/preview?id=${encodeURIComponent(item.id)}`, { headers: auth });
    if (!res.ok) throw new Error((await readJson(res)).error ?? `HTTP ${res.status}`);
    const blob = await res.blob();
    if (mine !== asked) return null;
    url = URL.createObjectURL(blob);
    audio = new Audio(url);
    const done = () => {
      if (mine === asked) stopPreview();
    };
    audio.onended = done;
    usePreview.setState({ loading: null, playing: item.id });
    // A page that may not play sound yet (no gesture) just stays quiet.
    await audio.play().catch(done);
    return null;
  } catch (e) {
    if (mine === asked) usePreview.setState({ loading: null });
    return e instanceof Error ? e.message : String(e);
  }
}

/** ⌘F's panel: open or not. */
export const useFinding = create<{ open: boolean }>(() => ({ open: false }));
