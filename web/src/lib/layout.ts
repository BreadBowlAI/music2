// The window's layout sizes (taste.md §3.2), mirroring the `--spacing-*` tokens in index.css: the
// timeline and the roll place things from them in code, so the two must agree.

/** The top bar. */
export const BAR_H = 44;
/**
 * The timeline's ruler (next D4a): the markers' 14 px band at its top, the bar numbers and the
 * tempo and time signature changes on the line under it, the beat ticks and the loop at its foot.
 */
export const RULER_H = 36;
/** The roll's ruler, which has no markers. */
export const ROLL_RULER_H = 28;
/** A track's own row (medium height), and a notes or audio sub-row under it. */
export const ROW_H = 44;
/** An automation lane under its track. */
export const LANE_H = 32;
/** The dock's header. */
export const DOCKBAR_H = 32;
/** The side browser. */
export const SIDE_W = 240;
/** The track headers; narrower in a small window (§3.8). */
export const HEADS_W = 240;
export const HEADS_W_SMALL = 200;
/** Below this window width the browser starts hidden and the headers narrow (§3.2, §3.8). */
export const WIDE = 1280;

/** The dock's body: 300px at first; never under 180px, and never so tall the lanes drop under three rows and the Song row. */
export const DOCK_H = 300;
export const DOCK_MIN = 180;
export const dockMax = (windowHeight: number) => Math.max(DOCK_MIN, windowHeight - BAR_H - DOCKBAR_H - RULER_H - 4 * ROW_H);
