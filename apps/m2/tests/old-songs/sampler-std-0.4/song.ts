// The root module: whatever this returns is what you hear.
// Eight bars of std's sampler: the drums play two short WAV files, one for each note, and the keys
// play one plucked note, pitched to every note of the melody. The files are in assets/.

import { db, song } from "@m2/core";
import { master, mixer } from "std/mix";
import drums from "./tracks/drums";
import keys from "./tracks/keys";

export default song(() =>
  master(
    mixer({
      drums: { in: drums.out, db: db(-7) },
      keys: { in: keys.out, db: db(-6), pan: -0.2 },
    }),
  ),
);
