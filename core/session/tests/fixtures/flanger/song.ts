// The root module: @m2/effects' Flanger at work, so a pure fixture pins its sound (PACK-35, RENDER-2):
// the chords of Up through it ringing (feedback up), and the line of Down through it hollow (feedback
// down), each sweeping at its own rate.

import { song } from "@m2/core";
import { master, mixer } from "std/mix";
import down from "./tracks/down";
import up from "./tracks/up";

export default song(() => master(mixer({ up: up.out, down: down.out })));
