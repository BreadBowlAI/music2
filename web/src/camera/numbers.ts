// The camera's numbers (the camera note's §16): one line here each. G and the rails are set from the
// owner's swipe probe of 2026-11-14 on their Mac (taste.md VIEW-10; its wheel events are the `camera`
// suite's `e2e/fixtures/swipes`); the rest are still guesses (gaps.md).

/**
 * G: wheel events less than this many ms apart are one swipe (VIEW-2). The probe: 50 ms. A flick's
 * first tiny event often comes 50 to 65 ms before its next, so 50 splits it in two, which does no
 * harm (both parts' first events decide the same owner); 80 or 120 ms joins swipes, and their rails
 * get worse.
 */
export const SWIPE_GAP = 50;

/**
 * Rails (VIEW-3), as Chromium's touch scrolling has them: a swipe whose first `RAIL_AFTER` px go
 * (as a sum with signs: a hand's jitter cancels) within `RAIL_ANGLE` degrees of an axis starts on that
 * axis's rail. It breaks free of it, for the rest of the swipe, once its last `RAIL_RECENT` px have
 * gone, all told, more than `RAIL_TURN` degrees off it: a turn, never a long swipe's slant or its
 * jitter adding up. A view may have none (the Graph).
 *
 * The probe: the owner's straight swipes go 0 to 11° off an axis all told, but their first 8 to 20 px
 * often go 15 to 25° off, in small whole deltas such as (3, 0), (5, -3). Within 10° after 8 px (the
 * guess) put 14 of their 32 straight swipes on a rail, and moved them 328 px of the 681 they went
 * off it. Within 25° after 16 px, broken by the last 100 px turning 25° off: all 32 stay on a rail,
 * none moves off; none of their 11 diagonal swipes (25 to 45° off) gets one, and 4721 of their
 * 4814 px across are kept (98%). The break's angle is the start's: a swipe that may start on a rail
 * 22° off also stays on it at 22°. 100 px is a dozen of their events: jitter of a pixel each way
 * can't take that 25° off (47 px would), and a turn to 45° in events of 10 px breaks free at its
 * seventh (its third before). One flick of theirs only 19° off all told, in a step of diagonal
 * flicks, stays on its rail: 8 of its 333 px across move.
 */
export const RAIL_ANGLE = 25;
export const RAIL_AFTER = 16;
export const RAIL_TURN = 25;
export const RAIL_RECENT = 100;

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
