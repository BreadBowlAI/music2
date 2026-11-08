// The root module: the tools of @m2/mixing that send meters, each working, so a pure fixture pins
// their sound (PACK-26, RENDER-2): a saw in short notes through the gate, which closes between
// them, the de-esser, which hears its highs, and the multiband, whose low band is over its threshold.

import { db, song } from "@m2/core";
import { master, mixer } from "std/mix";
import mix from "./tracks/mix";

export default song(() => master(mixer({ mix: { in: mix.out, db: db(-6) } })));
