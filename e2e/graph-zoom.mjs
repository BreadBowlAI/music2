// The Graph zooms around the pointer (taste.md VIEW-5), on a fresh copy of examples/first-song in a
// small window: the graph's point under the pointer stays under it through a ⌘-scroll and a pinch,
// and the view's middle through the zoom bar's buttons. A node's box tells where the graph is drawn
// and at what scale, so the point is followed in the graph's own units, whatever the zoom's number.
//   wheel   ⌘-scroll in and out, at a point right and below the graph's corner
//   pinch   WebKit's gesture events, at another point
//   bar     Zoom in and Zoom out, around the view's middle
//   fit     Fit still fits
//   node run.mjs graph-zoom fix
import { check, done, open, until } from "./fixlib.mjs";

const { browser, page, errors, view } = await open(1000, 600);

const graph = "[data-testid=graph]";
await view("graph");
await page.waitForSelector(`${graph} [data-node-id]`);

/** The view's box on screen, its room to scroll, and the box of the node `id` there. */
const look = (id) =>
  page.$eval(
    graph,
    (el, id) => {
      const b = el.getBoundingClientRect();
      const n = el.querySelector(`[data-node-id="${id}"]`).getBoundingClientRect();
      return { left: b.left, top: b.top, w: el.clientWidth, h: el.clientHeight, room: { x: el.scrollWidth - el.clientWidth, y: el.scrollHeight - el.clientHeight }, node: { x: n.left, y: n.top, w: n.width, h: n.height } };
    },
    id,
  );
/** The first node's id: the one each check follows. */
const id = await page.$eval(`${graph} [data-node-id]`, (e) => e.dataset.nodeId);
/** Where the graph's point under (x, y) in `before` is in `after`, on screen. */
const moved = (before, after, x, y) => {
  const s = after.node.w / before.node.w;
  return { x: after.node.x + (x - before.node.x) * s, y: after.node.y + (y - before.node.y) * s, s };
};
/** The zoom bar's number, for the messages. */
const zoomNow = () => page.$eval("[data-zoom]", (e) => e.dataset.zoom);
/** A render, then a frame: the zoom has landed and the view scrolled. */
const settle = () => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

/** ⌘-scroll at (x, y) on screen, `dy` px: up zooms in. */
const wheel = async (x, y, dy) => {
  await page.$eval(graph, (el, x, y, dy) => el.dispatchEvent(new WheelEvent("wheel", { deltaY: dy, deltaMode: 0, clientX: x, clientY: y, ctrlKey: true, bubbles: true, cancelable: true })), x, y, dy);
  await settle();
};
/** A pinch at (x, y), as WebKit sends it: gesture events, each scale from the pinch's start. */
const pinch = async (x, y, scales) => {
  await page.$eval(
    graph,
    (el, x, y, scales) => {
      const send = (type, scale) => {
        const e = new UIEvent(type, { bubbles: true, cancelable: true });
        for (const [k, v] of Object.entries({ scale, clientX: x, clientY: y })) Object.defineProperty(e, k, { value: v });
        el.dispatchEvent(e);
      };
      send("gesturestart", 1);
      for (const s of scales) send("gesturechange", s);
      send("gestureend", scales.at(-1));
    },
    x,
    y,
    scales,
  );
  await settle();
};
/** Checks that the graph's point under (x, y) before is still there after (within a pixel and a half). */
const held = (what, before, after, x, y) => {
  const p = moved(before, after, x, y);
  const off = Math.max(Math.abs(p.x - x), Math.abs(p.y - y));
  check(Math.abs(p.s - 1) > 0.05 && off <= 1.5, `${what}: zoomed ×${p.s.toFixed(2)} and the point stays (${off.toFixed(1)} px off) at ${x}, ${y}`);
};

// ---------------------------------------------------------------- room
// Zooming in at the graph's own corner needs no scroll: it makes the room the checks need. The
// zoom goes to 200%, so each check stays between this zoom and that.
const start = await look(id);
await wheel(start.node.x, start.node.y, -100);
const roomy = await look(id);
check(roomy.room.x > 200 && roomy.room.y > 50, `zoomed in at the corner (${await zoomNow()}): room to scroll ${roomy.room.x} across and ${roomy.room.y} down`);

// ---------------------------------------------------------------- ⌘-scroll
{
  const x = Math.round(roomy.left + roomy.w * 0.6), y = Math.round(roomy.top + roomy.h * 0.55);
  const a = await look(id);
  await wheel(x, y, -60);
  const b = await look(id);
  held("⌘-scroll in", a, b, x, y);
  await wheel(x, y, 60);
  held("⌘-scroll out", b, await look(id), x, y);
}

// ---------------------------------------------------------------- pinch
{
  const x = Math.round(roomy.left + roomy.w * 0.4), y = Math.round(roomy.top + roomy.h * 0.4);
  const a = await look(id);
  await pinch(x, y, [1.1, 1.2, 1.3]);
  held("a pinch in", a, await look(id), x, y);
  const b = await look(id);
  await pinch(x, y, [0.9, 0.77]);
  held("a pinch out", b, await look(id), x, y);
}

// ---------------------------------------------------------------- the zoom bar
{
  const box = await look(id);
  const x = box.left + box.w / 2, y = box.top + box.h / 2;
  await page.click('button[aria-label="Zoom in"]');
  await settle();
  const b = await look(id);
  held("Zoom in, around the view's middle", box, b, x, y);
  await page.click('button[aria-label="Zoom out"]');
  await settle();
  held("Zoom out, around the view's middle", b, await look(id), x, y);
}

// ---------------------------------------------------------------- Fit
await page.click('button[aria-label="Fit to panel"]');
check(await until(async () => (await page.$eval('button[aria-label="Fit to panel"]', (e) => e.getAttribute("aria-pressed"))) === "true", 3000), `Fit fits again (${await zoomNow()})`);

done(errors);
await browser.close();
