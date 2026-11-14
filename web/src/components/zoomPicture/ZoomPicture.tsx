// The arrangement's drawn zoom (taste.md VIEW-5, VIEW-13; docs/designs/camera/drawn-zoom.md): while a
// pinch or a run of ⌘-scroll lasts, the camera shows this picture over the arrangement's clear part
// and has it paint each frame, sharp at the zoom shown, from what the arrangement renders from; the
// page under it stays as it was laid out until the zoom ends, and the picture hides in the frame
// that lays it out. Two opaque canvases, sticky in the scroll box: the ruler's (it stops where the
// ruler's tools start) and the lanes'. The camera library owns when it shows (`useDrawnZoom`); this
// says what is drawn.
//
// The scene (scene.ts) is read once a zoom, and again when a render says something changed (`Said`)
// or a store it reads does. The playhead's clock paints it again while it shows (it plays, a take
// grows). Each paint says what it drew (`face.picture("arrangement")`): each clip's box, the
// playhead, the bars and the marks, at the zoom shown.

import { useEffect, useRef, type RefObject } from "react";
import { useDrawnZoom, type CameraState } from "../../camera";
import { useClipSelection } from "../../lib/clips";
import { beatNow, onFrame } from "../../lib/clock";
import { RULER_H } from "../../lib/layout";
import { useNotice } from "../../lib/notice";
import { usePoints } from "../../lib/points";
import { useRecord } from "../../lib/record";
import { useSteps } from "../../lib/steps";
import { ARRANGEMENT } from "../../lib/zoom";
import { paintLanes, type DrawnLanes } from "./lanes";
import { Pen } from "./pen";
import { paintRuler, type DrawnRuler } from "./ruler";
import type { Said } from "./said";
import { readScene, type Scene } from "./scene";

/** What one paint drew, in px from the lanes' left at the zoom shown (`x`: the box's scroll across then). */
export interface PictureDrawn extends DrawnLanes, DrawnRuler {
  n: number;
  zoom: number;
  x: number;
}

/** A scene with the pens it paints with: one a canvas, made for it at the view's size. */
interface Painting {
  sc: Scene;
  lanes: Pen;
  ruler: Pen;
}

/** Size a canvas to `w` × `h` px at `dpr` device px a px, its bitmap on the screen's pixels. */
function size(c: HTMLCanvasElement, w: number, h: number, dpr: number) {
  const bw = Math.max(1, Math.round(w * dpr)), bh = Math.max(1, Math.round(h * dpr));
  if (c.width !== bw || c.height !== bh) {
    c.width = bw;
    c.height = bh;
  }
  const sw = `${bw / dpr}px`, sh = `${bh / dpr}px`;
  if (c.style.width !== sw) c.style.width = sw;
  if (c.style.height !== sh) c.style.height = sh;
}

export function ZoomPicture({ said, box, headerW }: { said: Said; box: RefObject<HTMLDivElement | null>; headerW: number }) {
  const holder = useRef<HTMLDivElement>(null);
  const rulerEl = useRef<HTMLCanvasElement>(null);
  const lanesEl = useRef<HTMLCanvasElement>(null);
  const paints = useRef(0);
  const picture = useDrawnZoom<Painting>(
    ARRANGEMENT,
    holder,
    () => {
      const b = box.current, rc = rulerEl.current, lc = lanesEl.current;
      const sc = b && readScene(b, said);
      if (!sc || !rc || !lc) throw new Error("the arrangement has said nothing to draw yet");
      size(rc, Math.max(0, sc.tools), RULER_H, sc.dpr);
      size(lc, sc.w, Math.max(0, sc.h - RULER_H), sc.dpr);
      // A word in the empty view's middle is the page's: the picture has a hole there.
      const hole = sc.hole;
      lc.style.clipPath = hole
        ? `path(evenodd, "M0 0H${sc.w}V${sc.h}H0Z M${hole.x} ${hole.y - RULER_H}h${hole.w}v${hole.h}h${-hole.w}Z")`
        : "";
      // Every px of them is painted, but not as opaque canvases: Chrome draws an opaque canvas's text
      // with colour fringes (LCD smoothing), the page's in grey.
      const lanes = lc.getContext("2d"), ruler = rc.getContext("2d");
      if (!lanes || !ruler) throw new Error("no 2D canvas");
      return { sc, lanes: new Pen(lanes, sc.dpr, RULER_H), ruler: new Pen(ruler, sc.dpr, 0) };
    },
    ({ sc, lanes, ruler }, s: CameraState): PictureDrawn => {
      const z = s.zoom;
      // Where the box is scrolled when the zoom is laid out: the writer's whole px.
      const X = Math.max(0, Math.round(s.x));
      const beat = beatNow();
      lanes.reset();
      ruler.reset();
      const l = paintLanes(lanes, sc, z, X, beat);
      const r = paintRuler(ruler, sc, z, X, s.x, beat);
      return { n: ++paints.current, zoom: z, x: X, ...l, ...r };
    },
  );
  // The playhead moves while it plays, and a take grows: painted again on its clock's frames.
  useEffect(() => onFrame(() => picture.redraw()), [picture]);
  // What the view renders from changed (the song, a track's state, the selection): read again.
  useEffect(() => {
    const again = () => picture.redraw(true);
    const offs = [
      said.hear(again),
      useClipSelection.subscribe(again),
      usePoints.subscribe(again),
      useSteps.subscribe(again),
      useNotice.subscribe(again),
      useRecord.subscribe(again),
    ];
    return () => offs.forEach((off) => off());
  }, [said, picture]);
  return (
    <div ref={holder} aria-hidden data-zoom-picture className="sticky top-0 left-0 z-[45] h-0 w-0 shrink-0 pointer-events-none">
      <canvas ref={rulerEl} data-picture="ruler" className="absolute top-0" style={{ left: headerW }} />
      <canvas ref={lanesEl} data-picture="lanes" className="absolute" style={{ left: headerW, top: RULER_H }} />
    </div>
  );
}
