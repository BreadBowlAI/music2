// The take while it records (lane B, Q24): a clip growing in its lane from the bar it starts on to
// the playhead, a 1.5 px `danger` outline round a `danger-soft` body, the notes as the timeline draws
// notes (a key still down grows with the playhead), and "Recording" in its corner. `danger` marks
// recording as well as problems (D18). Drawn from the playhead's own clock on every frame: the server
// says the take only when its notes change. An audio take (the io plan's slice 3) starts where
// recording started, has no notes, and shows the sound input's level as a bar at its growing edge:
// the meters' `input` (no waveform until it is written: gaps). A take on several armed tracks
// (MON-9) draws on each of their lanes; over the loop, each pass from the loop's start (MON-11).

import { useEffect, useState } from "react";
import { onFrame } from "../lib/clock";
import { INPUT } from "../lib/meters";
import type { TrackColor } from "../lib/palette";
import { useRecord } from "../lib/record";
import { useZoom } from "../lib/zoom";
import Meter from "./Meter";
import { Notes } from "./SongTimeline";

/** The take in lane `module.lane`, if one records there. */
export default function Take({ module, lane, color }: { module: string; lane: string; color: TrackColor }) {
  const take = useRecord((s) => (s.take?.phase === "take" && (s.take.lanes ?? [s.take]).some((l) => l.module === module && l.lane === lane) ? s.take : null));
  const mine = take?.lanes?.find((l) => l.module === module && l.lane === lane);
  const ppb = useZoom();
  const [now, setNow] = useState<number | null>(null);
  useEffect(() => (take ? onFrame(setNow) : undefined), [take]);
  if (!take || now === null) return null;
  // To the playhead; never past the loop's end, where the take ends; never before where it started.
  const end = Math.max(take.from, take.until === null ? now : Math.min(now, take.until));
  const audio = (mine?.kind ?? take.kind) === "audio";
  const start = mine?.start ?? take.start;
  const length = Math.max(end - start, 1 / 64);
  const notes = (audio ? [] : take.notes).map((n) => ({
    pitch: n.pitch,
    start: n.start - start,
    length: n.held ? Math.max(end - n.start, 1 / 64) : n.length,
    velocity: n.velocity,
  }));
  return (
    <div
      data-testid="take"
      data-start={start}
      data-pass={take.pass ?? 0}
      data-notes={audio ? 0 : take.notes.length}
      data-kind={audio ? "audio" : "notes"}
      className="absolute rounded-sm overflow-hidden pointer-events-none z-10 bg-danger-soft outline-[1.5px] outline-danger -outline-offset-[1.5px]"
      style={{ top: 4, bottom: 4, left: start * ppb + 1, width: Math.max(length * ppb - 2, 4) }}
    >
      <span className="absolute left-1.5 top-0.5 text-xs font-medium text-danger-strong whitespace-nowrap">Recording</span>
      {audio ? (
        // A canvas sizes to its own drawing unless told: the box gives it the take's height.
        <span className="absolute right-1 top-1 bottom-1 w-[3px]">
          <Meter channels={[INPUT]} label="Input level" title="The sound input's level" testid="take-level" className="w-full h-full" />
        </span>
      ) : (
        /* Below the word, so a high note never runs through it. */
        <div className="absolute inset-x-0 top-3 bottom-0">
          <Notes notes={notes} length={length} color={color} />
        </div>
      )}
    </div>
  );
}
