// The root module: @m2/effects' Clipper played hard, so a pure fixture pins its sound (PACK-34,
// RENDER-2): a saw in short notes pushed 12 dB into it, every peak cut flat at -1 dB.

import { song } from "@m2/core";
import { master, mixer } from "std/mix";
import mix from "./tracks/mix";

export default song(() => master(mixer({ mix: mix.out })));
