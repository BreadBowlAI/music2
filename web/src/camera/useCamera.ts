// How a view uses its camera: `useCamera` on its scroll box, once, at the top of the view.

import { useLayoutEffect, useMemo, useState, type RefObject } from "react";
import { CULL_MARGIN } from "./numbers";
import { rigs, Rig, type CameraControls, type Fixed } from "./rig";
import { adopt, cameraOf, forgetCameras } from "./store";

export interface CameraOptions extends Fixed {
  /** The camera's zoom when it is made. */
  start?: () => number;
  /** Keep the camera when the view unmounts (the app lets go of it with `forgetCameras`); else it goes with the view. */
  keep?: boolean;
}

/**
 * The camera `name` on the scroll box `box` (VIEW-1): its swipes, zooms and glide (the recognizer),
 * its writer, and the app's moves. Call it at the top of the view, which reads the zoom it draws by
 * with `useZoomOf(name)`: a zoom renders the view once, and the position is written in that
 * render, before the frame is drawn.
 */
export function useCamera(name: string, box: RefObject<HTMLElement | null>, options: CameraOptions = {}): CameraControls {
  const rig = useMemo(() => new Rig(name, cameraOf(name, options.start)), [name]);
  rig.fixed = options;
  const keep = !!options.keep;
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    adopt(name, rig.cam);
    rig.attach(el);
    return () => {
      rig.detach();
      if (!keep) forgetCameras(name);
    };
  }, [rig]);
  // After every render: a zoom it drew is written now.
  useLayoutEffect(() => rig.afterRender());
  return rig.controls;
}

/** The app's moves of the camera `name`, while a view uses it; null while none does. */
export const controlsOf = (name: string): CameraControls | null => rigs.get(name)?.controls ?? null;

/**
 * Whether the element `ref` is in the view of the camera `name`, or within a view's height or width
 * of it (VIEW-12): for rows of their own heights, as the arrangement's tracks. Near at first; heard
 * from one IntersectionObserver on the box, so it renders only when the element crosses that edge.
 */
export function useNear(name: string, ref: RefObject<Element | null>): boolean {
  const [near, setNear] = useState(true);
  useLayoutEffect(() => {
    const el = ref.current;
    const rig = rigs.get(name);
    if (!el || !rig) return;
    return rig.watchNear(el, CULL_MARGIN, setNear);
  }, [name, rigs.get(name)]);
  return near;
}
