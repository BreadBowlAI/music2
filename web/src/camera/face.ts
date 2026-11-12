// What the library shows the `camera` suite (taste.md VIEW-10): its functions to check against
// known numbers in the page, its numbers, each camera now, a writer on a box of the suite's own, and
// the platform's glide to set. The app puts it beside `frames()` on `window.m2canvas`.

import { cull, cullPlan, fit, move, reveal, to, toBox, toContent, zoomAround } from "./math";
import * as numbers from "./numbers";
import { rigs } from "./rig";
import { cameraNames, cameraNow } from "./store";
import { movingNow, setGlide } from "./state";
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
  cullPlan,
  /** The cameras mounted now. */
  names: cameraNames,
  /** The camera mounted as `name` now, as its store holds it; null while none is. */
  now: (name: string) => {
    const cam = cameraNow(name);
    return cam ? { ...cam.getState() } : null;
  },
  /** Whether a camera moved in the last QUIET ms (VIEW-8). */
  moving: movingNow,
  /** A writer on `box`: a box of the suite's own, which keeps whole pixels as WebKit does. */
  writer: (box: ScrollBox) => new Writer(box),
  /** The camera's own glide on or off whatever the platform says (Linux's flag), or (null) as the platform says. */
  glide: setGlide,
  /** The swipes WebKit took over in each view (VIEW-2). */
  takenOver: () => Object.fromEntries([...rigs].map(([name, rig]) => [name, rig.hands.takenOver])),
};
