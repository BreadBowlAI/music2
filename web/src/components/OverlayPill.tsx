import type { GraphState } from "../lib/graph";

/**
 * The mutes pill (taste.md §3.3, TB-7; 8b.4): "2 muted · 1 solo", just right of the status pill,
 * only while a track is muted or soloed. Empty until lane A's U1 fills it.
 */
export default function OverlayPill(_: { graph: GraphState }) {
  return null;
}
