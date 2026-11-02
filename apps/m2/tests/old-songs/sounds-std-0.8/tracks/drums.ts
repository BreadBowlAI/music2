import { hz, ms, track } from "@m2/core";
import { follow } from "std/dsp";
import { drums } from "@m2/drums";
import { sum } from "std/math";

// The beat on one lane: each drum on its General MIDI note (D2 snare, D#2 clap, F#2 closed
// hat, A#2 open hat, ...). An open hat rings until the next hat closes it. The kick has a lane
// of its own, so the sub and the chords can duck under it alone.
export default track("drums", { name: "Drums" }, ({ lane }) => {
  const kick = drums.kick(lane.notes("kick"), { tune: hz(50), punch: 0.6 });
  const kit = drums.kit(lane.notes("beat"), { hatLevel: 2 });
  // How loud the kick is right now, for ducking.
  return { out: sum(kick, kit), kick: follow(kick, { release: ms(160) }) };
});
