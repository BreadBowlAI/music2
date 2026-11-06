import { axis, component, control, port, view } from "@m2/core";
// At load it spins, as a top level that takes far too long would: the runner stops it, once.
for (;;) {}

export const hang = component(
  { id: "hang", label: "hang", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => [p.line([20, 0, 20000, 0])], words: () => "ok" } },
);
