// A view that scrolls both ways (the arrangement, the Graph, the roll, the beat grid) moves both
// ways at once under a diagonal two-finger swipe (taste.md §3.8). WebKit on the Mac, the desktop
// app's web view, holds a swipe to the way it mostly goes (its "predominant axis" filter); the
// wheel event still carries both, so such a view scrolls itself by them. A swipe straight across
// or straight down is left to the web view, with its own feel at the edges.

/** The pixels a line-mode wheel step moves. */
const LINE = 33;

/** A wheel event's move in pixels, across and down. */
export function wheelPixels(e: Pick<WheelEvent, "deltaX" | "deltaY" | "deltaMode">, page: { w: number; h: number }): [number, number] {
  if (e.deltaMode === 2) return [e.deltaX * page.w, e.deltaY * page.h];
  const k = e.deltaMode === 1 ? LINE : 1;
  return [e.deltaX * k, e.deltaY * k];
}

/** Whether something between `target` and `view` scrolls on its own: a list or a field inside. */
function scrollsInside(target: EventTarget | null, view: HTMLElement) {
  for (let n = target instanceof Element ? target : null; n && n !== view; n = n.parentElement) {
    const s = getComputedStyle(n);
    if ((/auto|scroll/.test(s.overflowY) && n.scrollHeight > n.clientHeight) || (/auto|scroll/.test(s.overflowX) && n.scrollWidth > n.clientWidth)) return true;
  }
  return false;
}

/** Scroll `view` by a diagonal wheel both ways at once. Returns how to stop. */
export function scrollFreely(view: HTMLElement): () => void {
  const wheel = (e: WheelEvent) => {
    // A zoom (⌘ or a pinch), or a picture's dot that took the wheel (DV-17).
    if (e.defaultPrevented || e.ctrlKey || e.metaKey) return;
    const [dx, dy] = wheelPixels(e, { w: view.clientWidth, h: view.clientHeight });
    if (dx === 0 || dy === 0 || scrollsInside(e.target, view)) return;
    const left = view.scrollLeft, top = view.scrollTop;
    view.scrollLeft = left + dx;
    view.scrollTop = top + dy;
    // At an edge both ways, the swipe goes on to the web view.
    if (view.scrollLeft !== left || view.scrollTop !== top) e.preventDefault();
  };
  view.addEventListener("wheel", wheel, { passive: false });
  return () => view.removeEventListener("wheel", wheel);
}
