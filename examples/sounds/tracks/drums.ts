import { bus } from "@m2/core";
import { gain } from "std/dsp";

// The drum kit's bus: the Kick, Snare, Clap, Hats, Toms, Cymbal and Ride play into it, each on a
// track of its own (Make it a kit made them of the drum machine), and this is the kit's one fader.
export default bus("drums", { name: "Drums" }, ({ input }) => gain(input));
