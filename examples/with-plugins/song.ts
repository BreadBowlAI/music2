// Hosted CLAP plugins next to std kernels. Needs Surge XT and Airwindows Consolidated
// installed (see `m2 plugins`).

import { db, song } from "@m2/core";
import { duck } from "std/fx";
import { master, mixer } from "std/mix";
import kick from "./tracks/kick";
import lead from "./tracks/lead";

export default song(() =>
  master(
    mixer({
      kick: kick.out,
      lead: { in: duck(lead.out, kick.env, { depth: db(-6) }), db: db(-4) },
    }),
  ),
);
