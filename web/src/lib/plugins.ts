// The installed plugins (gaps #32): what the side browser's Plugins groups, the Device view's + and
// + Track list. The server asks the plugin host process to scan (HOST-3) on the first ask, and keeps
// the list for the app run; Look again scans again. A list is asked for only when one opens, so a
// song with no plugins starts no host process until then.
//
// A plugin is added through the routes std's sounds use, by its `clap:<id>` as `component`: as an
// effect (`Effect`) or as a new track's instrument (`Instrument`), each in the group "Plugins".

import { create } from "zustand";
import { readJson, type Effect, type Instrument } from "./graph";
import { token } from "./native";

/** An installed plugin, as `GET /api/plugins` lists it: an instrument, an effect, or both. */
export interface Plugin {
  id: string;
  name: string;
  vendor: string;
  version: string;
  instrument: boolean;
  effect: boolean;
}

/** The group plugins are listed in. */
export const PLUGINS = "Plugins";

interface Listed {
  /** `idle` before the first ask; `looking` while the host process scans. */
  state: "idle" | "looking" | "listed" | "failed";
  plugins: Plugin[];
  /** Where to install one (the user's CLAP folder), as the server's OS writes it. */
  folder: string | null;
  /** Why the list couldn't be had. */
  error: string | null;
}

export const usePluginList = create<Listed>(() => ({ state: "idle", plugins: [], folder: null, error: null }));

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

async function fetchList(again: boolean) {
  usePluginList.setState({ state: "looking", error: null });
  try {
    const res = await fetch(again ? "/api/plugins/scan" : "/api/plugins", { method: again ? "POST" : "GET", headers: auth });
    const data = await readJson(res);
    if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
    usePluginList.setState({ state: "listed", plugins: data.plugins ?? [], folder: data.folder ?? null });
  } catch (e) {
    usePluginList.setState({ state: "failed", error: e instanceof Error ? e.message : String(e) });
  }
}

/** Ask for the list, once: a list that opens calls it. */
export function wantPlugins() {
  if (usePluginList.getState().state === "idle") void fetchList(false);
}

/** Look again: the host process scans now (a plugin installed since shows). */
export function lookAgain() {
  if (usePluginList.getState().state !== "looking") void fetchList(true);
}

const by = (p: Plugin) => (p.vendor ? `By ${p.vendor}` : "A plugin");

/** The effect plugins, as the effect lists list effects. */
export const pluginEffects = (list: Plugin[]): Effect[] =>
  list.filter((p) => p.effect).map((p) => ({ component: p.id, label: p.name, name: "plugin", module: "@m2/core", description: by(p), category: PLUGINS, insert: "" }));

/** The instrument plugins, as + Track lists instruments: played from a notes lane. */
export const pluginInstruments = (list: Plugin[]): Instrument[] =>
  list.filter((p) => p.instrument).map((p) => ({ component: p.id, label: p.name, name: "plugin", module: "@m2/core", description: by(p), category: PLUGINS, lane: "notes" }));

/** Whether `component` names a plugin, not a std sound. */
export const isPlugin = (component: string) => component.startsWith("clap:");

/** The effect or instrument `component` names among the plugins listed, if it is one. */
export function listedEffect(component: string) {
  return pluginEffects(usePluginList.getState().plugins).find((e) => e.component === component);
}
export function listedInstrument(component: string) {
  return pluginInstruments(usePluginList.getState().plugins).find((i) => i.component === component);
}

/** What a list says in place of plugins: while it looks, when none are installed, or when it failed. Null when it lists some. */
export function pluginsNote(l: Listed, kind: "effect" | "instrument"): string | null {
  if (l.state === "idle" || l.state === "looking") return "Looking for plugins…";
  if (l.state === "failed") return `Couldn't look for plugins: ${l.error}`;
  if (l.plugins.length === 0) return `No plugins are installed. music2 looks in ${l.folder ?? "your CLAP folder"}.`;
  if (!l.plugins.some((p) => p[kind])) return kind === "effect" ? "None of your plugins is an effect." : "None of your plugins is an instrument.";
  return null;
}
