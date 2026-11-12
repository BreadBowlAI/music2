// How a view uses its camera: `useCamera` on its scroll box, once, at the top of the view.

import { useEffect, useLayoutEffect, useMemo, useState, startTransition, type RefObject } from "react";
import { CULL_MARGIN } from "./numbers";
import { rigs, rigOf, Rig, type CameraControls, type Fixed } from "./rig";
import { makeCamera, mount, unmount, useCameraOf, type CameraRef, type CameraStore } from "./store";

export interface CameraOptions extends Fixed {
  /** The camera's zoom when the view makes it. */
  start?: () => number;
}

/**
 * The camera `name` on the scroll box `box` (VIEW-1): its swipes, zooms and glide (the recognizer),
 * its writer, and the app's moves. Call it at the top of the view, which reads the zoom it draws by
 * with `useZoomOf(camera)`, and lends the camera to what it draws with `<CameraScope camera>`: a
 * zoom renders the view once, and the position is written in that render, before the frame is drawn.
 *
 * The view owns its camera: made when it first asks for `name`, mounted under that name while it is
 * mounted, and kept as long as the view is (a view whose name changes, as the Graph's with its
 * module, finds an earlier name's camera as it left it). Another view mounted under the same name in
 * the same commit has its own.
 */
export function useCamera(name: string, box: RefObject<HTMLElement | null>, options: CameraOptions = {}): CameraControls {
  const [own] = useState(() => new Map<string, CameraStore>());
  let store = own.get(name);
  if (!store) {
    store = makeCamera(options.start?.() ?? 1);
    own.set(name, store);
  }
  const rig = useMemo(() => new Rig(name, store), [name, store]);
  rig.fixed = options;
  useLayoutEffect(() => {
    mount(name, store);
    return () => {
      rig.detach();
      unmount(name, store);
    };
  }, [rig]);
  // After every render: on the box it rendered (one that appears later too), and a zoom it drew written now.
  useLayoutEffect(() => {
    rig.hold(box.current);
    rig.afterRender();
  });
  return rig.controls;
}

/** The app's moves of the camera `name`, while a view uses it; null while none does. */
export const controlsOf = (name: string): CameraControls | null => rigs.get(name)?.controls ?? null;

/** The app's moves of the camera whose box holds `el`; null when none does. */
export const controlsAt = (el: Element): CameraControls | null => [...rigs.values()].find((r) => r.holds(el))?.controls ?? null;

/**
 * Whether the element `ref` is in the view of the camera `cam`, or within a view's height or width
 * of it (VIEW-12): for rows of their own heights, as the arrangement's tracks. Near at first. Coming
 * near or going away renders between frames (a transition): the margin lets it lag. Coming into view
 * before that has landed renders at once.
 */
export function useNear(cam: CameraRef, ref: RefObject<Element | null>): boolean {
  const store = useCameraOf(cam);
  const [near, setNear] = useState(true);
  // After the view's own layout effect has put its camera on its box.
  useEffect(() => {
    const el = ref.current;
    const rig = store && rigOf(store);
    if (!el || !rig) return;
    return rig.watchNear(el, CULL_MARGIN, (to, seen) => {
      if (seen) setNear(true);
      else startTransition(() => setNear(to));
    });
  }, [store]);
  return near;
}
