// The pictures of music2's own instruments and of std's Limit and Master (CARD-131 to CARD-144),
// typed against the descriptor that draws them, as the code editor types them: each dot moves a
// control in its axis's unit. A line that should fail carries `@ts-expect-error`, so a type that
// stops refusing it fails the check too.

import { component, control, type Audio } from "@m2/core";
import { ceiling, ceilingPicture, curvePicture } from "std/pictures";
import { ladder, partials, partialsPicture } from "../../../../packs/@m2/synths/src/pictures";
import { filter, table, tablePicture } from "../../../../packs/@m2/wavetable/src/pictures";

declare const x: Audio;

/** A sound with a value of each unit these pictures move. */
const controls = {
  position: control.frac(0),
  cutoff: control.hz(1000),
  q: control.scalar(0.7),
  resonance: control.frac(0.1),
  carrier: control.scalar(1),
  modulator: control.scalar(2),
  amount: control.scalar(1),
  attack: control.secs(0.01),
  ceiling: control.db(-1),
};
const pictures = { waves: tablePicture(), curve: curvePicture({ live: false }), partials: partialsPicture(), level: ceilingPicture() };

export const right = component({ id: "right", controls, editor: { views: pictures } }, () => x, {
  waves: { draw: (v, p) => table(p, v, { table: "t", file: "f", position: "position", lfo: "l" }), words: () => "" },
  curve: { draw: (v, p) => [...filter(p, v, { shape: "s", cutoff: "cutoff", q: "q" }), ...ladder(p, v, { cutoff: "cutoff", resonance: "resonance" })], words: () => "" },
  partials: {
    draw: (v, p) => partials(p, v, { carrier: "carrier", modulator: "modulator", amount: "amount" }, { carrier: "carrier", modulator: "modulator", amount: "amount" }, { mix: "q", tone: "q" }),
    words: () => "",
  },
  level: { draw: (v, p) => ceiling(p, v, { ceiling: "ceiling" }), words: () => "" },
});

export const wrong = component({ id: "wrong", controls, editor: { views: pictures } }, () => x, {
  waves: {
    // @ts-expect-error a table's position that is a frequency: its axis is 0 to 1
    draw: (v, p) => table(p, v, { table: "t", file: "f", position: "cutoff", lfo: "l" }),
    words: () => "",
  },
  curve: {
    // @ts-expect-error a cutoff that is a time: its axis is in Hz
    draw: (v, p) => ladder(p, v, { cutoff: "attack", resonance: "resonance" }),
    words: () => "",
  },
  partials: {
    // @ts-expect-error a modulator that is a time: its axis is ratios of the note
    draw: (v, p) => partials(p, v, { carrier: "carrier", modulator: "attack", amount: "amount" }, { carrier: "carrier", modulator: "modulator", amount: "amount" }, { mix: "q", tone: "q" }),
    words: () => "",
  },
  level: {
    // @ts-expect-error a ceiling that is a level from 0 to 1: its axis is in dB
    draw: (v, p) => ceiling(p, v, { ceiling: "position" }),
    words: () => "",
  },
});

export const wrongFilter = component({ id: "wrong-filter", controls, editor: { views: { curve: curvePicture({ live: false }) } } }, () => x, {
  curve: {
    // @ts-expect-error a cutoff that is a plain number: its axis is in Hz
    draw: (v, p) => filter(p, v, { shape: "s", cutoff: "q", q: "q" }),
    words: () => "",
  },
});
