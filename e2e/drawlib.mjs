// What a clip's canvas drew (taste.md VIEW-12), for the suites: each canvas keeps the list of what it
// drew last (`drawn`: its notes, their 2 px marks and an automation curve's line, in px of its box,
// with the zoom and the beats it covers). `drawings` runs in the page (`page.$$eval(clips,
// drawings)`), reads nothing outside itself, and places that list on screen as the canvas shows it
// now (stretched with its clip since it drew, if a zoom by the hand hasn't settled), each note with
// the canvas's own pixel at its middle.

/**
 * In the page: the drawing of each clip element of `clips`, or null for one with no canvas or none
 * drawn yet. `start` and `length` are the clip's (its `data-start`, `data-length`); `box` is the
 * canvas on screen; `bitmap` its device px across and down; `stretch` how far its clip has stretched
 * it since it drew (1: not at all).
 */
export function drawings(clips) {
  return clips.map((clip) => {
    const c = clip.querySelector("canvas[data-drawing]");
    const d = c?.drawn;
    if (!c || !d) return null;
    const r = c.getBoundingClientRect();
    const sx = d.w ? r.width / d.w : 1;
    const sy = d.h ? r.height / d.h : 1;
    const ctx = c.getContext("2d");
    // The canvas's pixel under a point of its box (as drawn): [r, g, b, a], 0 to 255.
    const pixel = (x, y) => {
      if (!c.width || !c.height) return null;
      const X = Math.min(c.width - 1, Math.max(0, Math.floor((x / d.w) * c.width)));
      const Y = Math.min(c.height - 1, Math.max(0, Math.floor((y / d.h) * c.height)));
      return [...ctx.getImageData(X, Y, 1, 1).data];
    };
    return {
      kind: c.dataset.drawing,
      start: Number(clip.dataset.start),
      length: Number(clip.dataset.length),
      zoom: d.zoom,
      from: d.from,
      to: d.to,
      ink: d.ink,
      scale: d.scale,
      bitmap: [c.width, c.height],
      drawnBox: [d.w, d.h],
      stretch: Math.round(sx * 1000) / 1000,
      box: { left: r.left, top: r.top, width: r.width, height: r.height },
      opacity: getComputedStyle(c).opacity,
      notes: d.notes.map((n) => ({ x: r.left + n.x * sx, y: r.top + n.y * sy, w: n.w * sx, h: n.h * sy, alpha: n.alpha, pixel: pixel(n.x + n.w / 2, n.y + n.h / 2) })),
      marks: d.marks.map((m) => ({ x: r.left + m.x * sx, y: r.top + m.y * sy, w: m.w * sx, h: m.h * sy, alpha: m.alpha })),
      line: d.line.map(([x, y]) => [r.left + x * sx, r.top + y * sy]),
    };
  });
}

/** `#rrggbb` as [r, g, b]. */
export const rgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
