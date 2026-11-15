// What the arrangement and each track's rows render from, as their last render said it: the zoom
// picture reads its scene from this (taste.md VIEW-13), so it draws what the page draws. A render
// that says something new tells the picture once, after its commit: while it shows, it reads its
// scene again (the song changed in the middle of a zoom).

import { createContext, useContext, useLayoutEffect } from "react";
import type { Heard } from "../../lib/graph8b";
import type { Grid } from "../../lib/grid";
import type { TimelineTrack, TimelineView } from "../../lib/graph";
import type { Marker } from "../../lib/markers";
import type { TrackColor } from "../../lib/palette";

/** What a track's rows (`TrackRows`) render from. */
export interface TrackSaid {
  track: TimelineTrack;
  color: TrackColor;
  /** A muted track's clips, or one a solo leaves off, draw faint and grey; muted in the song, hatched too. */
  heard: Heard;
  /** The code has never compiled: its clips greyed. */
  waiting: boolean;
  selected: boolean;
  /** The Song's row, at the foot. */
  last: boolean;
  /** Clips can be edited now: an audio clip's fade handles show while it is picked. */
  editable: boolean;
}

/** What the arrangement (`SongTimeline`) renders from, around its rows. */
export interface ArrangementSaid {
  timeline: TimelineView;
  headerW: number;
  /** The song's room, in beats: the lanes' width less the ruler's tools, at any zoom. */
  room: number;
  /** The lanes' width on screen. */
  visible: number;
  /** The loop as the status says it, and whether it loops; Punch records between its edges. */
  loop: { start: number; end: number; on: boolean } | null;
  punch: boolean;
  /** The selected bars (ARR-9). */
  bars: [number, number] | null;
  /** The markers as this window draws them. */
  markers: readonly Marker[];
  grid: Grid;
}

/** The arrangement's and its rows' last word, and who hears it change. */
export class Said {
  arrangement: ArrangementSaid | null = null;
  readonly tracks = new Map<string, TrackSaid>();
  private hearer: (() => void) | null = null;
  private soon = false;

  /** Something a render draws by changed: heard once, after the commit's layout effects. */
  changed() {
    if (this.soon || !this.hearer) return;
    this.soon = true;
    queueMicrotask(() => {
      this.soon = false;
      this.hearer?.();
    });
  }

  /** Hear each change, until the function this returns is called. */
  hear(f: () => void): () => void {
    this.hearer = f;
    return () => void (this.hearer === f && (this.hearer = null));
  }
}

/** The arrangement's `Said`, lent to its rows. */
export const SaidScope = createContext<Said | null>(null);

/** A track's rows say what they render from, at each render. */
export function useSaidTrack(module: string, info: TrackSaid) {
  const said = useContext(SaidScope);
  useLayoutEffect(() => {
    if (!said) return;
    said.tracks.set(module, info);
    said.changed();
  });
  useLayoutEffect(() => () => void said?.tracks.delete(module), [said, module]);
}
