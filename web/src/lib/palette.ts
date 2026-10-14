// The track colours (taste.md §2.7): twelve muted hues of equal lightness and chroma, so no track
// shouts, each in three tones from one formula:
//   bar  = oklch(0.72 0.085 h)   the header's colour bar, clip borders, notes in the roll
//   tint = oklch(0.905 0.032 h)  clip bodies
//   ink  = oklch(0.46 0.075 h)   marks drawn inside clips, note borders (at least 5.2:1 on tint)
// The muting is in the palette: nothing mutes them again at draw time. They live here, not in CSS,
// because they are applied through `style` on clips and notes.

import type { TimelineView } from "./graph";

export interface TrackColor { bar: string; tint: string; ink: string }

export const TRACK_COLORS = {
  rose:   { bar: "#d48e98", tint: "#f4d7db", ink: "#7c464e" },
  coral:  { bar: "#d4927e", tint: "#f4d9d1", ink: "#7c4839" },
  amber:  { bar: "#c69c68", tint: "#edddc9", ink: "#725124" },
  olive:  { bar: "#b1a666", tint: "#e4e1c9", ink: "#625922" },
  lime:   { bar: "#95af75", tint: "#d9e4ce", ink: "#4c6031" },
  green:  { bar: "#7cb48a", tint: "#d1e6d5", ink: "#356443" },
  teal:   { bar: "#61b6ab", tint: "#c9e7e2", ink: "#14655d" },
  cyan:   { bar: "#60b2c6", tint: "#c9e6ed", ink: "#146272" },
  blue:   { bar: "#79aad8", tint: "#d0e2f4", ink: "#345b80" },
  indigo: { bar: "#98a0da", tint: "#dadef5", ink: "#4e5482" },
  violet: { bar: "#b197d0", tint: "#e4dbf1", ink: "#624d7a" },
  plum:   { bar: "#c990b5", tint: "#efd8e6", ink: "#744765" },
} as const satisfies Record<string, TrackColor>;
/** The Song row and the Master strip only; never a track's. */
export const GRAPHITE = { bar: "#9a9fa6", tint: "#dbdee2", ink: "#51565c" } as const satisfies TrackColor;
/** Each colour is far in hue from the two before it. */
export const DEFAULT_ORDER = ["rose", "teal", "amber", "indigo", "green", "plum", "olive", "blue", "coral", "cyan", "violet", "lime"] as const;

export type TrackColorName = keyof typeof TRACK_COLORS;

/**
 * A module's colours, or null if it has no row in the timeline. Tracks take the default order in
 * the order the timeline lists them; the song's own lanes are graphite, so a lane made on the song
 * never recolours a track. (Colours by name in `layout.json`, frozen on its first write, come with
 * the track builders.)
 */
export function trackColors(timeline: TimelineView, module: string): TrackColor | null {
  const row = timeline.tracks.find((t) => t.module === module);
  if (!row) return null;
  if (!row.track) return GRAPHITE;
  const i = timeline.tracks.filter((t) => t.track).findIndex((t) => t.module === module);
  return TRACK_COLORS[DEFAULT_ORDER[i % DEFAULT_ORDER.length]];
}
