// The root module: whatever this returns is what you hear.

import { song, db } from "@m2/core";
import { duck } from "std/fx";
import { master, mixer } from "std/mix";
import bass from "./tracks/bass";
import hats from "./tracks/hats";
import keys from "./tracks/keys";
import kick from "./tracks/kick";

export default song(() =>
  master(
    mixer({
      kick: kick.out,
      hats: { in: hats.out, db: db(-34.7), pan: -0.0577 },
      // Sidechain: the bass ducks whenever the kick hits.
      bass: duck(bass.out, kick.env, { depth: db(-42.9) }),
      keys: { in: keys.out, db: db(-11.5), pan: -0.542 },
    }),
  ),
);
