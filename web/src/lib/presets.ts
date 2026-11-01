// Presets (FMT-22, HOST-8, gaps #26): a sound's settings under a name. The card's Presets popover
// lists yours (in ~/Music/music2/Presets) and music2's own (std's), or a plugin's own for a plugin,
// loads one as one undoable edit, and saves the card's settings as yours. Saving changes no song
// file, so it has no Undo.

import { readJson } from "./graph";
import { editing, fileOf, goes, recorded } from "./history";
import { token } from "./native";
import { http } from "./http";

export interface PresetName {
  name: string;
}

/** A sound card's presets, each group by name. */
export interface PresetList {
  /** What they are for: the ComponentId (`@m2/synths:synth-pluck`), or a plugin's id (`clap:…`). */
  sound: string;
  /** What the app calls the sound: "Synth pluck", or the plugin's name. */
  label: string;
  std: PresetName[];
  yours: PresetName[];
  /** A plugin's own (HOST-8): none when it offers none. */
  plugin: PresetName[];
  /** Why a plugin's own couldn't be read, in words. */
  plugin_error?: string;
}

/** Where a preset comes from: music2's own, yours, or the plugin's own. */
export type PresetFrom = "std" | "yours" | "plugin";

/** A control a preset left alone: what drives it (a lane, other code), or it is written in other units. */
export interface Kept {
  label: string;
  why: "lane" | "code" | "units";
}

interface Loaded {
  file: string;
  before: string;
  after: string;
  revision: number;
  /** The undo step it made, by its id (AGENT-2): what its toast's Undo takes back. */
  step?: number;
  /** False when the sound had these settings already: nothing was written, so there is no Undo. */
  changed: boolean;
  kept: Kept[];
}

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

async function request<T>(url: string, body?: unknown): Promise<T> {
  const res = await http(url, {
    method: body === undefined ? "GET" : "POST",
    headers: { "content-type": "application/json", ...auth },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as T;
}

export const presetsApi = {
  /** The presets for the call `node` names at `revision`. */
  list: (session: number, revision: number, node: string) =>
    request<PresetList>(
      `/api/graph/presets?session=${session}&revision=${revision}&node=${encodeURIComponent(node)}`,
    ),
  /** Save the call's settings as your preset `name`. `taken`: one of that name is there (ask, then `replace`). */
  save: (
    session: number,
    revision: number,
    node: string,
    name: string,
    replace: boolean,
  ) =>
    request<{ name?: string; taken?: string; left?: Kept[] }>(
      "/api/graph/preset/save",
      { session, revision, node, name, replace },
    ),
  /**
   * Load preset `name` into the call `node` names at revision `at`: one undoable edit, named for
   * Undo. It goes through the gate (lib/history): once the window shows its own last edit.
   */
  load: (
    session: number,
    at: number,
    node: string,
    from: PresetFrom,
    name: string,
  ) => {
    const ours = editing();
    return goes(session, at, [fileOf(node)], ours)
      .then((at) => request<Loaded>("/api/graph/preset/load", { session, at, node, from, name }))
      .then((r) => {
        if (r.changed) recorded(session, r, { name: `loading ${name}` });
        return r;
      })
      .finally(() => ours.end());
  },
};

const capital = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** Why each control was left alone, a sentence each: "Cutoff follows its lane, so it kept the lane." */
export function keptWords(kept: Kept[], saved = false): string {
  return kept
    .map((k) => {
      const what = capital(k.label);
      if (saved)
        return k.why === "lane"
          ? `${what} follows its lane, so it isn't in the preset.`
          : `${what} is set by other code, so it isn't in the preset.`;
      if (k.why === "lane")
        return `${what} follows its lane, so it kept the lane.`;
      if (k.why === "units")
        return `${what} is written in other units, so it kept its value.`;
      return `${what} is set by other code, so it kept its value.`;
    })
    .join(" ");
}
