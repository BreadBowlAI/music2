import { song } from "@m2/core";
import { master, mixer } from "std/mix";
import keys from "./tracks/keys";
import pluck from "./tracks/pluck";

export default song(() => master(mixer({ keys: keys.out, pluck: pluck.out })));
