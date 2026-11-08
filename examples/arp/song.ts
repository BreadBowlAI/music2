// The root module: whatever this returns is what you hear.
// Eight bars of C, G, A minor and F, twice. The keys track holds each chord for a bar, and the
// Arpeggiator plays it one note at a time, a sixteenth each, up two octaves, into a pluck; a pad
// holds the same chords under it, and a kick keeps time.

import { db, song } from "@m2/core";
import { gain } from "std/dsp";
import { master, mixer } from "std/mix";
import keys from "./tracks/keys";
import kick from "./tracks/kick";
import pad from "./tracks/pad";

export default song(() => {
  const mix = mixer({
    kick: { in: kick.out, db: db(-5) },
    keys: { in: keys.out, db: db(4), pan: 0.15 },
    pad: { in: pad.out, db: db(-6), pan: -0.15 },
  });
  return master(gain(mix, db(5)));
});
