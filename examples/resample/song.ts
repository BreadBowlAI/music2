// The resampling loop: bounce a synth to audio (`m2 bounce`), then chop, reverse,
// pitch and stretch the bounce on an audio lane.

import { db, song } from "@m2/core";
import { duck } from "std/fx";
import { master, mixer } from "std/mix";
import chops from "./tracks/chops";
import kick from "./tracks/kick";
import "./tracks/keys"; // the source of the bounce; not in the mix

export default song(() =>
  master(
    mixer({
      kick: kick.out,
      chops: { in: duck(chops.out, kick.env, { depth: db(-6) }), db: db(-2) },
    }),
  ),
);
