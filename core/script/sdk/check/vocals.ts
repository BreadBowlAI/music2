// The calls of @m2/vocals (VOX-20), checked as the app's code editor checks songs: Tune bare, as + Effect
// adds it, and with its values; and its pitch line (VOX-27), a picture over an axis of semitones. A
// line that should fail carries `@ts-expect-error`, so a type that stops refusing it fails the check
// too.

import { component, ms, port, type Audio } from "@m2/core";
import { tune } from "@m2/vocals";
import { curvePicture } from "std/pictures";
import { pitchLine, pitchPicture } from "../../../../packs/@m2/vocals/src/pictures";

declare const x: Audio;

export const vocals: Audio[] = [
  tune(x),
  tune(x, { glide: 0.1, amount: 1 }),
  tune(x, { glide: ms(50), amount: 0.5 }),
  // @ts-expect-error Tune has no key of its own: it follows the song's
  tune(x, { key: 3 }),
  // @ts-expect-error Tune moves a sound, not a number
  tune(0.5),
];

// Its pitch line is drawn over time and semitones from the note: a picture over other axes can't draw it.
export const line = [
  component(
    { id: "pitch-line", inputs: { in: port.audio() }, output: port.audio(), editor: { views: { pitch: pitchPicture() } } },
    (x) => x,
    { pitch: { draw: (_v, p) => pitchLine(p), words: () => "a pitch line" } },
  ),
  component(
    { id: "pitch-across-frequency", inputs: { in: port.audio() }, output: port.audio(), editor: { views: { pitch: curvePicture() } } },
    (x) => x,
    {
      pitch: {
        // @ts-expect-error a pitch line on a picture across frequency
        draw: (_v, p) => pitchLine(p),
        words: () => "a pitch line",
      },
    },
  ),
];
