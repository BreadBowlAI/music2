// The root module: whatever this returns is what you hear.
// Each track is a file in tracks/; its notes are in data/. Here they're mixed.

import { db, song } from "@m2/core";
import { compress, duck } from "std/fx";
import { master, mixer } from "std/mix";
import bass from "./tracks/bass";
import chords from "./tracks/chords";
import clap from "./tracks/clap";
import hats from "./tracks/hats";
import hook from "./tracks/hook";
import kick from "./tracks/kick";

export default song(() =>
  master(
    // Glue: evens out the mix a little, like on a finished record.
    compress(
      mixer({
        kick: kick.out,
        clap: { in: clap.out, db: db(2) },
        hats: { in: hats.out, db: db(-2), pan: 0.3 },
        // Sidechain: the bass and chords dip whenever the kick hits, so it punches through.
        bass: { in: duck(bass.out, kick.env, { depth: db(-9) }), db: db(-10) },
        chords: { in: duck(chords.out, kick.env, { depth: db(-6) }), db: db(-10) },
        hook: { in: hook.out, db: db(4) },
      }),
      { threshold: db(-12), ratio: 2, attack: 0.02, release: 0.2, makeup: db(3) },
    ),
  ),
);
