// The writer (the camera note's §4, C6): the camera keeps fractions of a pixel, a scroll box keeps
// whole ones (WebKit's `scrollLeft` is a `long`). So the writer rounds, remembers what it wrote, and
// compares the box only with that: a slow swipe never loses its fractions, and a move the browser
// made to the box itself (more than 1 px from what was written) is told apart and taken in.

import type { Camera } from "./math";

/** What the writer needs of a scroll box: an element, or the `camera` suite's box that keeps whole pixels. */
export interface ScrollBox {
  scrollLeft: number;
  scrollTop: number;
  readonly scrollWidth: number;
  readonly scrollHeight: number;
  readonly clientWidth: number;
  readonly clientHeight: number;
}

/** How far the box may be from what was written before the move is the browser's. */
const SLACK = 1;

export class Writer {
  /** What the box held after the last write. */
  written: { x: number; y: number };

  constructor(readonly box: ScrollBox) {
    this.written = { x: box.scrollLeft, y: box.scrollTop };
  }

  /** The box's position if the browser moved it since the last write (Tab, a key, a scrollbar, a click), else null. */
  theirs(): { x: number; y: number } | null {
    const x = this.box.scrollLeft, y = this.box.scrollTop;
    if (Math.abs(x - this.written.x) <= SLACK && Math.abs(y - this.written.y) <= SLACK) return null;
    this.written = { x, y };
    return { x, y };
  }

  /** Take the box as it is now as written: its move was no one's (a zoom's drawing clamped it). */
  settle() {
    this.written = { x: this.box.scrollLeft, y: this.box.scrollTop };
  }

  /**
   * Write `c` in whole pixels, if it moved from what was written; what the box held is remembered.
   * Returns the camera as the box can show it: stopped at the box's edges (VIEW-6), its fractions
   * kept wherever it wasn't.
   */
  write(c: Camera): Camera {
    const b = this.box;
    const x = Math.min(Math.max(0, c.x), Math.max(0, b.scrollWidth - b.clientWidth));
    const y = Math.min(Math.max(0, c.y), Math.max(0, b.scrollHeight - b.clientHeight));
    const rx = Math.round(x), ry = Math.round(y);
    if (rx !== Math.round(this.written.x)) b.scrollLeft = rx;
    if (ry !== Math.round(this.written.y)) b.scrollTop = ry;
    this.written = { x: b.scrollLeft, y: b.scrollTop };
    return x === c.x && y === c.y ? c : { ...c, x, y };
  }
}
