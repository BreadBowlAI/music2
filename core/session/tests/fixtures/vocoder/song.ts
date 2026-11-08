// The root module: @m2/vocals' Vocoder at work, so a pure fixture pins its sound (VOX-29, RENDER-2):
// the chords of Keys made to talk by the made-up voice of Vox, which the song mixes in quietly too.

import { db, song } from "@m2/core";
import { master, mixer } from "std/mix";
import keys from "./tracks/keys";
import vox from "./tracks/vox";

export default song(() => master(mixer({ keys: keys.out, vox: { in: vox.out, db: db(-18) } })));
