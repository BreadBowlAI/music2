import { OUT } from "../lib/meters";
import Meter from "./Meter";

/**
 * A track header's meter (taste.md §3.4; 8b.8): placed by the header, 9 px in from its right edge,
 * 3 px wide and 28 px tall, one bar for the louder side (Q22). `module` is the track's key, or
 * `song` for the Song row's, which shows the song's output. A track nothing reads has no tap: its
 * meter stays empty (Q21).
 */
export default function TrackMeter({ module, name }: { module: string; name: string }) {
  return <Meter channels={[module === "song" ? OUT : module]} label={`${name} level`} className="w-full h-full" />;
}
