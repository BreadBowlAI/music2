// The root module: a small made-up instrument, played by std's Sampler from regions.

import { db, song } from "@m2/core";
import { master, mixer } from "std/mix";
import keys from "./tracks/keys";

export default song(() => master(mixer({ keys: { in: keys.out, db: db(-3) } })));
