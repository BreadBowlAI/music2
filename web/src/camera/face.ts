// What the library shows the `camera` suite (taste.md VIEW-10): its functions to check against
// known numbers in the page, its numbers, each camera now, a writer on a box of the suite's own, and
// the platform's glide to set. The app puts it beside `frames()` on `window.m2canvas`.

import { cull, fit, move, reveal, to, toBox, toContent, zoomAround } from "./math";
import * as numbers from "./numbers";
import { rigs } from "./rig";
import { cameraNames, cameraOf } from "./store";
import { setGlide } from "./state";
import { Writer, type ScrollBox } from "./writer";

export const face = {
  numbers,
  move,
  to,
  zoomAround,
  reveal,
  fit,
  toContent,
  toBox,
  cull,
  /** The cameras made now. */
  names: cameraNames,
  /** The camera `name` now, as its store holds it; null while it isn't made. */
  now: (name: string) => (cameraNames().includes(name) ? { ...cameraOf(name).getState() } : null),
  /** A writer on `box`: a box of the suite's own, which keeps whole pixels as WebKit does. */
  writer: (box: ScrollBox) => new Writer(box),
  /** The camera's own glide on or off whatever the platform says (Linux's flag), or (null) as the platform says. */
  glide: setGlide,
  /** The swipes WebKit took over in each view (VIEW-2). */
  takenOver: () => Object.fromEntries([...rigs].map(([name, rig]) => [name, rig.hands.takenOver])),
};
