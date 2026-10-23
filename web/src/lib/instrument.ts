// Change a track's instrument (polish A3): the call, its toast with Undo, and a refusal's way to
// the code. The list it is picked from is + Track's (`Tracks.tsx`, `InstrumentPicker` in its
// change mode).
//
// One undo step on the server: the track's one instrument call becomes a call of the new one,
// reading the same notes, at the new one's own settings. The track keeps its name, colour, lane,
// notes, mixer channel, effects and mute.

import { undoAction } from "../components/GraphCanvas";
import { lockReason, moduleName, readJson, type GraphState, type Instrument } from "./graph";
import { guideDid } from "./guide";
import { recorded } from "./history";
import { token } from "./native";
import { useShell } from "./shell";
import { useStore } from "./store";

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

/** What a track is called on screen. */
function nameOf(graph: GraphState, module: string) {
  const t = graph.timeline.tracks.find((x) => x.module === module);
  return moduleName({ key: module, label: t?.label ?? graph.view.modules.find((m) => m.key === module)?.label });
}

/** The instrument `module` plays now, if it is one the list offers: what the list ticks. */
export function currentInstrument(graph: GraphState, module: string): Instrument | undefined {
  const listed = graph.instruments ?? [];
  const nodes = graph.view.modules.find((m) => m.key === module)?.nodes ?? [];
  const playing = new Set(nodes.map((n) => n.component).filter(Boolean));
  const found = listed.filter((i) => playing.has(i.component));
  return found.length === 1 ? found[0] : undefined;
}

/** The notes lane `module`'s instrument reads: what the list's foot writes it with. */
export function notesLane(graph: GraphState, module: string) {
  return graph.timeline.tracks.find((t) => t.module === module)?.lanes.find((l) => l.kind === "notes")?.key ?? "notes";
}

/** Open the Code view on `module`'s own file: where a change the app can't make is made. */
function showCode(module: string) {
  const s = useShell.getState();
  s.select(module);
  s.setView("code");
}

/**
 * Change `module`'s instrument to `instrument`: one undo step, said in a toast with Undo ("Bass
 * plays Pad now"). A refusal says why in words; one that sends you to the code has Show code.
 */
export async function changeInstrument(module: string, instrument: Instrument) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return toast(`${why}.`);
  const name = nameOf(graph, module);
  const session = graph.session;
  try {
    const res = await fetch("/api/graph/track/instrument", {
      method: "POST",
      headers: { "content-type": "application/json", ...auth },
      body: JSON.stringify({ session, module, component: instrument.component }),
    });
    const data = await readJson(res);
    if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
    const r = data as Edited;
    recorded(session, r, { name: `changing ${name}'s instrument`, structural: true });
    toast(`${name} plays ${instrument.label} now`, undoAction(session, r.step));
    guideDid("sound");
  } catch (error) {
    const words = message(error);
    toast(words, /in code\.$|in the Code view\.$/.test(words) ? { label: "Show code", run: () => showCode(module) } : undefined);
  }
}
