import { poly, track } from "@m2/core";
import { adsr, osc, vca } from "std/dsp";
import { vocoder } from "@m2/vocals";
import vox from "./vox";

// Chords on a saw, made to talk by the voice of Vox: the Vocoder gives them its words, lets its S
// through (Clarity), and moves its colour 2 semitones up (Formant).
export default track("keys", { name: "Keys" }, ({ lane }) => {
  const chords = poly(lane.notes("chords"), (n) => vca(osc.saw(n.pitch), adsr(n.gate, { attack: 0.01, release: 0.2 })));
  return vocoder(chords, vox.out, { clarity: 0.6, smooth: 0.04, formant: 2 });
});
