// What a keyboard plays (the next track's Q4): the selected track's notes lane. The lane of the clip
// Notes shows, when it is that track's; else the track's first notes lane. The Song row, nothing
// selected, or a track with no notes lane: nothing. The keyboard piano plays it (lib/piano.ts), and
// a MIDI keyboard plays it once the window tells the server (lib/midi.ts). While a take records,
// its lane, whatever is selected (lane B, D8).

import { useMemo } from "react";
import { create } from "zustand";
import type { ClipRef, GraphState } from "./graph";
import { useShell } from "./shell";
import { useStore } from "./store";

export interface PlayTarget {
  module: string;
  lane: string;
  /** The track's name, as its header says it: "Plays Bass". */
  label: string;
  /** Its colour, by palette name (lib/palette.ts). */
  colour?: string;
}

/** The take's lane while one records (lib/record.ts): what keys play then, whatever is selected. */
const useHeld = create<{ lane: { module: string; lane: string } | null }>(() => ({ lane: null }));
export const holdTarget = (lane: { module: string; lane: string } | null) => useHeld.setState({ lane });

/** The take's lane as a target, while one records. */
function held(graph: GraphState | null, lane: { module: string; lane: string } | null): PlayTarget | null {
  const track = lane && graph?.timeline.tracks.find((t) => t.module === lane.module);
  return lane && track ? { module: lane.module, lane: lane.lane, label: track.label, colour: track.colour } : null;
}

/** What plays, given the open song and the shell's selection. */
export function playTarget(graph: GraphState | null, selected: string | null, clips: Record<string, ClipRef>): PlayTarget | null {
  const take = held(graph, useHeld.getState().lane);
  if (take) return take;
  if (!graph || !selected || selected === "song") return null;
  const track = graph.timeline.tracks.find((t) => t.module === selected && t.track);
  if (!track) return null;
  const notes = track.lanes.filter((l) => l.kind === "notes");
  const clip = clips[selected];
  const lane = (clip?.module === selected ? notes.find((l) => l.key === clip.lane) : undefined) ?? notes[0];
  return lane ? { module: selected, lane: lane.key, label: track.label, colour: track.colour } : null;
}

/** What plays now, outside React. */
export const currentTarget = () => {
  const { selected, clips } = useShell.getState();
  return playTarget(useStore.getState().graph, selected, clips);
};

/** What plays, following the selection, the clip Notes shows and the song's tracks. */
export function usePlayTarget(): PlayTarget | null {
  const graph = useStore((s) => s.graph);
  const selected = useShell((s) => s.selected);
  const clips = useShell((s) => s.clips);
  const take = useHeld((s) => s.lane);
  return useMemo(() => held(graph, take) ?? playTarget(graph, selected, clips), [graph, selected, clips, take]);
}

/** Two targets are the same lane of the same track. */
export const sameTarget = (a: PlayTarget | null, b: PlayTarget | null) => a?.module === b?.module && a?.lane === b?.lane;
