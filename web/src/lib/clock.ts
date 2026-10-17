// The transport clock the playheads are drawn from (taste.md §2.8, TL-9): the last status's beat,
// plus the time since it came at the tempo the engine plays there, so a playhead moves every frame
// and never eases between the 200ms polls. One requestAnimationFrame loop runs while anything draws
// from it.

import type { GraphStatus, Loop } from "./graph";

interface Clock {
  beat: number;
  playing: boolean;
  /** When `beat` was read (performance.now()). */
  at: number;
  tempo: { beat: number; bpm: number }[];
  loop: Loop | null;
}

let clock: Clock = { beat: 0, playing: false, at: 0, tempo: [], loop: null };
const listeners = new Set<(beat: number) => void>();
let frame = 0;
let shown = NaN;

/**
 * A poll's answer: where playback is now. `tempo` is the timeline's, for a server that doesn't say
 * the tempo it plays.
 */
export function setClock(status: GraphStatus, tempo: { beat: number; bpm: number }[]) {
  // While an export holds playback (RENDER-6a) the transport plays but the engine processes
  // nothing, so the beat stands still: the playhead waits with it rather than run ahead and jump back.
  // A tempo edit plays from its switch, after the timeline says it (RENDER-27): until then the
  // engine plays the tempo before, which the status says, and the playhead keeps to that.
  const playing = status.bpm ? [{ beat: 0, bpm: status.bpm }] : tempo;
  clock = { beat: status.beat, playing: status.playing && !status.paused, at: performance.now(), tempo: playing, loop: status.loop };
}

const bpmAt = (beat: number) => {
  let bpm = clock.tempo[0]?.bpm ?? 120;
  for (const t of clock.tempo) if (t.beat <= beat) bpm = t.bpm;
  return bpm;
};

/** Where playback is at this moment. */
export function beatNow(): number {
  const c = clock;
  if (!c.playing) return c.beat;
  // A second at most past the last poll: a stalled server doesn't run the playhead away.
  const seconds = Math.min(1, (performance.now() - c.at) / 1000);
  let beat = c.beat + (seconds * bpmAt(c.beat)) / 60;
  const l = c.loop;
  if (l?.on && c.beat < l.end && beat >= l.end && l.end > l.start) beat = l.start + ((beat - l.start) % (l.end - l.start));
  return beat;
}

const tick = () => {
  frame = 0;
  if (!listeners.size) return;
  const beat = beatNow();
  if (beat !== shown) {
    shown = beat;
    for (const f of listeners) f(beat);
  }
  frame = requestAnimationFrame(tick);
};

/** Call `draw` with the beat on every frame it changes (and once now); returns the unsubscribe. */
export function onFrame(draw: (beat: number) => void): () => void {
  listeners.add(draw);
  draw(beatNow());
  if (!frame) frame = requestAnimationFrame(tick);
  return () => {
    listeners.delete(draw);
    if (!listeners.size && frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
  };
}
