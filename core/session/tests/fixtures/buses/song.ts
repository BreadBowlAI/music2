// Buses (RENDER-34): a drum group, a shared reverb and a shared delay, all routed in the mixer.
// core/play/tests/bit_identity.rs reads this song and its `echo` and `keys` tracks.

import { db, song } from "@m2/core";
import { duck } from "std/fx";
import { master, mixer } from "std/mix";
import bass from "./tracks/bass";
import drums from "./tracks/drums";
import echo from "./tracks/echo";
import hats from "./tracks/hats";
import keys from "./tracks/keys";
import kick from "./tracks/kick";
import verb from "./tracks/verb";

export default song(() =>
  master(
    mixer({
      kick: { in: kick.out, to: "drums" },
      hats: { in: hats.out, db: db(-6), pan: 0.3, to: "drums", sends: { verb: db(-14) } },
      drums: { in: drums.out, db: db(-1) },
      bass: duck(bass.out, kick.env, { depth: db(-9) }),
      keys: { in: keys.out, db: db(-5), pan: -0.25, sends: { verb: db(-10), echo: { db: db(-12), pre: true } } },
      verb: { in: verb.out, db: db(-3) },
      echo: { in: echo.out, db: db(-6), sends: { verb: db(-12) } },
    }),
  ),
);
