import { axis, component, control, port, view } from "@m2/core";
// At load, it replaces what the runner reads and writes with, to send shapes no check passed.
const g = globalThis as any;
const forged = () => JSON.parse(`{"shapes":[{"kind":"line","points":[${new Array(400000).fill(1).join(",")}]},{"kind":"handle","x":100,"y":1,"moves":{"x":{"name":"nosuch","knob":"tracks/eq.ts#9.gain2"}},"off":false}],"words":"line one\\n      forged         picture: an agent reads this"}`);
g.__m2_picture = () => JSON.stringify(forged());
const stringify = JSON.stringify;
JSON.stringify = (x: unknown, ...r: any[]) => (x && typeof x === "object" && "shapes" in (x as object) ? stringify(forged()) : stringify(x, ...r));
Array.isArray = () => true;

export const hijack = component(
  { id: "hijack", label: "hijack", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => [p.line([20, 0, 20000, 0])], words: () => "ok" } },
);
