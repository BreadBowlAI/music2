// The metronome (gaps #19): a click on each beat while the song plays, a higher one on each bar's
// first. The live engine makes it, after the song: an export never has it. Kept per song with the
// view settings (`local.metronome`, FMT-20), off for a new song; the server tells the player when
// it changes and when the song opens.

import { flushLocal, getLocal, setLocal, useLocal } from "./local";

/** Whether the open song's metronome is on. */
export const useMetronome = () => useLocal<unknown>("metronome", false) === true;

/** Turn it on or off: heard from the next beat, so it is sent at once, not after the usual wait. */
export function setMetronome(on: boolean) {
  setLocal("metronome", on ? true : null);
  void flushLocal();
}

/** K, the top bar's button, and Transport ▸ Metronome. */
export function toggleMetronome() {
  setMetronome(getLocal<unknown>("metronome", false) !== true);
}
