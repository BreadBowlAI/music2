// What a clip's canvas drew (taste.md VIEW-12), for the suites. A lane's canvas draws the clips that
// sit still; a clip that moves, and a take, draws on its own. Each canvas keeps the list of what it
// drew last (`drawn`): for each clip, its notes, their 2 px marks and an automation curve's line, in px
// of the canvas's box, with the zoom and the beats it holds. `drawings` runs in the page
// (`page.$$eval(clips, drawings)`), reads nothing outside itself, and places a clip's part of that list
// on screen as the canvas shows it now (stretched since it drew, if a zoom by the hand hasn't
// settled), each note with the canvas's own pixel at its middle.

/**
 * In the page: the drawing of each clip element of `clips`, or null for one no canvas drew. `kind` is
 * "lane" for a lane's canvas, else the clip's own ("notes", "curve"); `start` and `length` are the
 * clip's (its `data-start`, `data-length`); `from` and `to` the beats the canvas holds; `k` the
 * clip's px a beat on screen; `box` the canvas on screen and `clip` the clip's box; `bitmap` its
 * device px across and down; `stretch` how far a zoom has stretched it since it drew (1: not at all).
 */
export function drawings(clips) {
  return clips.map((clip) => {
    const own = clip.querySelector("canvas[data-drawing]");
    const c = own ?? clip.parentElement?.querySelector(":scope > canvas[data-drawing=lane]");
    const d = c?.drawn;
    const start = Number(clip.dataset.start), length = Number(clip.dataset.length);
    const e = d?.clips.find((x) => x.start === start && x.length === length);
    if (!c || !d || !e) return null;
    const r = c.getBoundingClientRect();
    const b = clip.getBoundingClientRect();
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
      kind: own ? c.dataset.drawing : "lane",
      start,
      length,
      zoom: d.zoom,
      from: d.from,
      to: d.to,
      k: e.k * sx,
      ink: d.ink,
      scale: d.scale,
      bitmap: [c.width, c.height],
      drawnBox: [d.w, d.h],
      stretch: Math.round(sx * 1000) / 1000,
      box: { left: r.left, top: r.top, width: r.width, height: r.height },
      clip: { left: b.left, top: b.top, right: b.right, bottom: b.bottom },
      opacity: getComputedStyle(c).opacity,
      notes: e.notes.map((n) => ({ x: r.left + n.x * sx, y: r.top + n.y * sy, w: n.w * sx, h: n.h * sy, alpha: n.alpha, pixel: pixel(n.x + n.w / 2, n.y + n.h / 2) })),
      marks: e.marks.map((m) => ({ x: r.left + m.x * sx, y: r.top + m.y * sy, w: m.w * sx, h: m.h * sy, alpha: m.alpha })),
      line: e.line.map(([x, y]) => [r.left + x * sx, r.top + y * sy]),
    };
  });
}

/** `#rrggbb` as [r, g, b]. */
export const rgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
