import { axis, component, control, kernel, port, signature, view, type Audio } from "@m2/core";

const curve = view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) });
const code = { curve: { draw: (_v: unknown, p: any) => [p.response()], words: () => "its curve" } };

export const spin = component(
  {
    id: "spin",
    label: "Spin",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    // A value its sound never reads: moving it draws the picture again, at the same copy.
    controls: { drift: control.scalar(0).range(0, 1) },
    output: port.audio(),
    editor: { views: { curve } },
  },
  (x: Audio): Audio => kernel("@test/spin:spin", { in: x }),
  code,
);

export const slow = component(
  {
    id: "slow",
    label: "Slow",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    output: port.audio(),
    editor: { views: { curve } },
  },
  (x: Audio): Audio => kernel("@test/spin:slow", { in: x }),
  code,
);
