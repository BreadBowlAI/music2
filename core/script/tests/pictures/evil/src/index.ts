import { axis, component, control, port, view } from "@m2/core";
// Each picture here does what no picture should. Types are bypassed with `any`, as a stranger's pack may.

export const reach = component(
  { id: "reach", label: "reach", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => [p.line([20, 0, 20000, 0])], words: (): string => { const t = (f: () => unknown) => { try { return String(f()); } catch (e) { return "throws"; } }; const g = globalThis as any; return ["Date:" + t(() => typeof g.Date), "perf:" + t(() => typeof g.performance), "rand:" + t(() => Math.random()), "fetch:" + t(() => typeof g.fetch), "setTimeout:" + t(() => typeof g.setTimeout), "WebAssembly:" + t(() => typeof g.WebAssembly), "eval:" + t(() => g.eval("1")), "fnctor:" + t(() => (function () {}).constructor("return 1")())].join(" "); } } },
);

export const bigname = component(
  { id: "bigname", label: "bigname", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => [p.line([20, 0, 20000, 0], { name: "x".repeat(20000000) })], words: () => "ok" } },
);

export const bigint = component(
  { id: "bigint", label: "bigint", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => { const n = (1n << 3000000n).toString(); return [p.label(100, 0, "n" + n.length)]; }, words: () => "ok" } },
);

export const regex = component(
  { id: "regex", label: "regex", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => { /^(a+)+$/.test("a".repeat(31) + "b"); return [p.line([20, 0, 20000, 0])]; }, words: () => "ok" } },
);

export const sorted = component(
  { id: "sorted", label: "sorted", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => { const k = Object.keys(new Array(1500000).fill(0)); k.sort(); return [p.line([20, 0, 20000, 0])]; }, words: () => "ok" } },
);

export const deep = component(
  { id: "deep", label: "deep", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => { const f = (n: number): number => f(n + 1) + 1; return [p.line([20, 0, 20000, f(0)])]; }, words: () => "ok" } },
);

export const memhog = component(
  { id: "memhog", label: "memhog", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => { const s = "x".repeat(100000000); return [p.label(1, 1, "m" + s.length)]; }, words: () => "ok" } },
);

export const spin = component(
  { id: "spin", label: "spin", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => { for (;;) {} }, words: () => "ok" } },
);

export const counter = component(
  { id: "counter", label: "counter", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (() => { let n = 0; return (v: any, p: any) => [p.label(100, 0, "draw " + ++n)]; })(), words: () => "ok" } },
);

export const ansi = component(
  { id: "ansi", label: "ansi", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => [p.line([20, 0, 20000, 0])], words: () => "red \u001b[31m here" } },
);

export const sep = component(
  { id: "sep", label: "sep", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => [p.line([20, 0, 20000, 0])], words: () => "one two" } },
);

export const four00 = component(
  { id: "four00", label: "four00", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => [p.line([20, 0, 20000, 0])], words: () => "é".repeat(400) } },
);

export const four01 = component(
  { id: "four01", label: "four01", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => [p.line([20, 0, 20000, 0])], words: () => "é".repeat(401) } },
);

export const fine = component(
  { id: "fine", label: "fine", inputs: { x: port.audio() }, controls: { freq: control.hz(1000).range(20, 20000).log() }, output: port.audio(),
    editor: { views: { pic: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } } },
  (x: any) => x,
  { pic: { draw: (v: any, p: any) => [p.line([20, 0, 20000, v.freq])], words: (v: any) => `at ${v.freq} Hz` } },
);
