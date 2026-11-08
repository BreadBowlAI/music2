// The root module: @m2/effects' Grains at work, so a pure fixture pins its sound (PACK-36, RENDER-2):
// the tune of Up broken into short grains an octave up, and the chords of Down into long ones an
// octave down, from up to 1.5 s back.

import { song } from "@m2/core";
import { master, mixer } from "std/mix";
import down from "./tracks/down";
import up from "./tracks/up";

export default song(() => master(mixer({ up: up.out, down: down.out })));
