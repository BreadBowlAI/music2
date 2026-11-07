// The pictures of @m2/effects (CARD-117 to CARD-124), checked as the app's code editor checks songs:
// each dot names the card's own value in the unit its axis draws, or a number control where it
// moves as its knob does (CARD-62). A line that should fail carries `@ts-expect-error`, so a type
// that stops refusing it fails the check too.

import { component, control, port } from "@m2/core";
import { drive, echoes, echoesPicture, field, fieldPicture, motionPicture, shapePicture, tail, tailPicture, tremoloMotion } from "../../../../packs/@m2/effects/src/pictures";

export const dots = [
  component(
    {
      id: "time-in-hz",
      inputs: { in: port.audio() },
      controls: { time: control.hz(4), feedback: control.frac(0.3), mix: control.frac(0.3) },
      output: port.audio(),
      editor: { views: { echoes: echoesPicture() } },
    },
    (x) => x,
    {
      echoes: {
        // @ts-expect-error a delay's time that is a frequency, moved across seconds
        draw: (v, p) => echoes(p, v, { time: "time", feedback: "feedback", mix: "mix" }),
        words: () => "echoes",
      },
    },
  ),
  component(
    {
      id: "feedback-a-switch",
      inputs: { in: port.audio() },
      controls: { time: control.secs(0.25), feedback: control.bool(false), mix: control.frac(0.3) },
      output: port.audio(),
      editor: { views: { echoes: echoesPicture() } },
    },
    (x) => x,
    {
      echoes: {
        // @ts-expect-error a feedback that is a switch, moved as a knob
        draw: (v, p) => echoes(p, v, { time: "time", feedback: "feedback", mix: "mix" }),
        words: () => "echoes",
      },
    },
  ),
  component(
    {
      id: "size-a-choice",
      inputs: { in: port.audio() },
      controls: { size: control.choice(["room", "hall"], "room"), damping: control.frac(0.5) },
      output: port.audio(),
      editor: { views: { tail: tailPicture() } },
    },
    (x) => x,
    {
      tail: {
        // @ts-expect-error a reverb's size that is a choice, moved as a knob
        draw: (v, p) => tail(p, v, { size: "size", damping: "damping" }),
        words: () => "a tail",
      },
    },
  ),
  // Review T7: a name a picture only reads is the sound's own too.
  component(
    {
      id: "damping-misspelt",
      inputs: { in: port.audio() },
      controls: { size: control.frac(0.5), damping: control.frac(0.5) },
      output: port.audio(),
      editor: { views: { tail: tailPicture() } },
    },
    (x) => x,
    {
      tail: {
        // @ts-expect-error a damping the sound doesn't have
        draw: (v, p) => tail(p, v, { size: "size", damping: "dampin" }),
        words: () => "a tail",
      },
    },
  ),
  component(
    { id: "rate-misspelt", inputs: { in: port.audio() }, controls: { rate: control.hz(1), depth: control.frac(0.5) }, output: port.audio(), editor: { views: { motion: motionPicture("unit01", 0, 1) } } },
    (x) => x,
    {
      motion: {
        // @ts-expect-error a rate the sound doesn't have
        draw: (v, p) => tremoloMotion(p, v, { rate: "rates", depth: "depth" }),
        words: () => "a motion",
      },
    },
  ),
  component(
    { id: "amount-a-choice", inputs: { in: port.audio() }, controls: { amount: control.choice(["soft", "hard"], "soft") }, output: port.audio(), editor: { views: { curve: shapePicture() } } },
    (x) => x,
    {
      curve: {
        // @ts-expect-error drive's amount that is a choice, moved as a knob
        draw: (v, p) => drive(p, v, { amount: "amount" }),
        words: () => "a curve",
      },
    },
  ),
  component(
    { id: "width-in-db", inputs: { in: port.audio() }, controls: { width: control.db(0) }, output: port.audio(), editor: { views: { field: fieldPicture() } } },
    (x) => x,
    {
      field: {
        // @ts-expect-error a width in dB, moved across a width
        draw: (v, p) => field(p, v, { width: "width" }),
        words: () => "a field",
      },
    },
  ),
];
