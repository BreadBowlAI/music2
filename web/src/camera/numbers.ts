// The camera's numbers (the camera note's §16). Each is a guess the owner's swipe probe report
// (taste.md VIEW-10) may change: one line here each.

/** G: wheel events less than this many ms apart are one swipe (VIEW-2). */
export const SWIPE_GAP = 50;

/** Rails (VIEW-3): a swipe whose first `RAIL_AFTER` px lie within `RAIL_ANGLE` degrees of an axis keeps to it. */
export const RAIL_ANGLE = 20;
export const RAIL_AFTER = 8;

/**
 * The glide after a flick (VIEW-4): the system's on a Mac, which reaches the page as more wheel
 * events; the camera's own where the page gets none (WebKitGTK), kept here as which platforms glide
 * on their own.
 */
export const SYSTEM_GLIDE = { mac: true, other: false } as const;
/** The camera's own glide: the share of its speed kept each ms (UIScrollView's normal rate). */
export const GLIDE_RATE = 0.998;
/** It starts at the speed of the swipe's last this many ms. */
export const GLIDE_FROM = 50;
/** And stops under this speed, in px a ms. */
export const GLIDE_STOP = 0.02;

/** The scrollbars: the browser's own (D5, VIEW-7). The camera draws none. */
export const SCROLLBARS = "browser" as const;
/** At an edge the view stops (D3, VIEW-6): the camera never goes past its box's room. */
export const EDGES = "stop" as const;

/** A wheel's line, as Chrome may count one, in px; a page is the box's height. */
export const LINE = 33;
/** ⌘-scroll and a pinch's wheel (VIEW-5): this many px of wheel zoom by `ZOOM_STEP`; one event counts this many at most. */
export const ZOOM_WHEEL = 100;
export const ZOOM_STEP = 1.5;

/** After the camera last moved, its views' hover waits this many ms (VIEW-8). */
export const QUIET = 250;

/**
 * Culling (VIEW-12): a view draws what is in view and this many views more each side; the window
 * it draws moves in steps of this share of a view, never every frame.
 */
export const CULL_MARGIN = 1;
export const CULL_STEP = 0.5;
