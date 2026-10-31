import { component, control, kernel, port, type Audio } from "@m2/core";

/** A choice of three shapes; its kernel plays the place it reads: 0, 1 or 2. */
export const shape = component(
  { id: "shape", controls: { shape: control.choice(["saw", "square", "sine"], "saw") }, output: port.audio() },
  ({ shape }) => kernel("@test/kinds:k", { shape }) as Audio,
);

/** A switch; its kernel plays 1 or 0. */
export const sub = component(
  { id: "sub", controls: { on: control.bool(false) }, output: port.audio() },
  ({ on }) => kernel("@test/kinds:s", { on }) as Audio,
);
