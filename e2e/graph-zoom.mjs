// The Graph zooms around the pointer (taste.md VIEW-5), on a fresh copy of examples/first-song in a
// small window: the graph's point under the pointer stays under it through a ⌘-scroll and a pinch,
// and the view's middle through the zoom bar's buttons. A node's box tells where the graph is drawn
// and at what scale, so the point is followed in the graph's own units, whatever the zoom's number.
//   wheel   ⌘-scroll in and out, at a point right and below the graph's corner
//   pinch   WebKit's gesture events, at another point
//   bar     Zoom in and Zoom out, around the view's middle
//   fit     Fit still fits
//   dots    the dotted background is the graph's: 20 of its px apart (doubled while under 12 on
//           screen), from its corner, 1 px dots, at each zoom on the way; and on the screen itself,
//           each dot seen in the view's middle keeps its place in the graph's px, from a node's corner
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
/** The dots' pattern at the zoom now: its spacing, its offset and its dots' radius, in screen px. */
const dotsAt = [];
/** The dots on the screen at each zoom: where each is from the node's corner, in the graph's px. */
const seenAt = [];
/**
 * The dots as the screen shows them, in a square at the view's middle, the graph's nodes and wires
 * hidden for the shot: each dot's centre (a blob of pixels off the background's colour), from the
 * node's corner, divided by the zoom.
 */
const seen = async () => {
  const zoom = await page.evaluate(() => {
    const name = window.m2canvas?.camera?.names().find((n) => n.startsWith("graph:"));
    return name ? window.m2canvas.camera.now(name).zoom : NaN;
  });
  const n = await look(id);
  const clip = { x: Math.round(n.left + n.w / 2 - 60), y: Math.round(n.top + n.h / 2 - 60), width: 120, height: 120 };
  await page.$eval(`${graph} [data-canvas]`, (c) => c.querySelectorAll(":scope > div").forEach((d) => (d.style.visibility = "hidden")));
  const png = await page.screenshot({ clip, encoding: "base64" });
  await page.$eval(`${graph} [data-canvas]`, (c) => c.querySelectorAll(":scope > div").forEach((d) => (d.style.visibility = "")));
  const blobs = await page.evaluate(async (png, w, h) => {
    const img = new Image();
    img.src = `data:image/png;base64,${png}`;
    await img.decode();
    const k = img.width / w;
    const cv = Object.assign(document.createElement("canvas"), { width: img.width, height: img.height });
    const g = cv.getContext("2d");
    g.drawImage(img, 0, 0);
    const px = g.getImageData(0, 0, img.width, img.height).data;
    const at = (i) => [px[i * 4], px[i * 4 + 1], px[i * 4 + 2]];
    // The background: the commonest colour.
    const count = new Map();
    for (let i = 0; i < img.width * img.height; i++) count.set(at(i).join(), (count.get(at(i).join()) ?? 0) + 1);
    const bg = [...count].sort((a, b) => b[1] - a[1])[0][0].split(",").map(Number);
    const off = (i) => Math.abs(at(i)[0] - bg[0]) + Math.abs(at(i)[1] - bg[1]) + Math.abs(at(i)[2] - bg[2]);
    const seen = new Uint8Array(img.width * img.height);
    const out = [];
    for (let i = 0; i < seen.length; i++) {
      if (seen[i] || off(i) < 12) continue;
      // A blob: its pixels, weighted by how far off the background each is.
      let sx = 0, sy = 0, sw = 0, edge = false;
      const todo = [i];
      seen[i] = 1;
      while (todo.length) {
        const j = todo.pop();
        const x = j % img.width, y = Math.floor(j / img.width);
        if (x === 0 || y === 0 || x === img.width - 1 || y === img.height - 1) edge = true;
        const wgt = off(j);
        sx += (x + 0.5) * wgt;
        sy += (y + 0.5) * wgt;
        sw += wgt;
        for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
          const nx = x + dx, ny = y + dy;
          if (nx < 0 || ny < 0 || nx >= img.width || ny >= img.height) continue;
          const m = ny * img.width + nx;
          if (!seen[m] && off(m) >= 12) {
            seen[m] = 1;
            todo.push(m);
          }
        }
      }
      if (!edge) out.push({ x: sx / sw / k, y: sy / sw / k });
    }
    return out;
  }, png, clip.width, clip.height);
  seenAt.push({ zoom, dots: blobs.map((b) => ({ x: (clip.x + b.x - n.node.x) / zoom, y: (clip.y + b.y - n.node.y) / zoom })) });
};
const dots = async () => {
  dotsAt.push({
    zoom: Number(await zoomNow()),
    ...(await page.$eval(`${graph} [data-canvas] > svg pattern`, (p) => ({ w: Number(p.getAttribute("width")), x: Number(p.getAttribute("x")), r: Number(p.querySelector("circle").getAttribute("r")) }))),
  });
  await seen();
};
await dots();

// ---------------------------------------------------------------- room
// Zooming in at the graph's own corner needs no scroll: it makes the room the checks need. The
// zoom goes to 200%, so each check stays between this zoom and that.
const start = await look(id);
await wheel(start.node.x, start.node.y, -100);
const roomy = await look(id);
await dots();
check(roomy.room.x > 200 && roomy.room.y > 50, `zoomed in at the corner (${await zoomNow()}): room to scroll ${roomy.room.x} across and ${roomy.room.y} down`);

// ---------------------------------------------------------------- ⌘-scroll
{
  const x = Math.round(roomy.left + roomy.w * 0.6), y = Math.round(roomy.top + roomy.h * 0.55);
  const a = await look(id);
  await wheel(x, y, -60);
  const b = await look(id);
  held("⌘-scroll in", a, b, x, y);
  await dots();
  await wheel(x, y, 60);
  held("⌘-scroll out", b, await look(id), x, y);
}

// ---------------------------------------------------------------- pinch
{
  const x = Math.round(roomy.left + roomy.w * 0.4), y = Math.round(roomy.top + roomy.h * 0.4);
  const a = await look(id);
  await pinch(x, y, [1.1, 1.2, 1.3]);
  held("a pinch in", a, await look(id), x, y);
  await dots();
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

// ---------------------------------------------------------------- the dots
// Zoomed out as far as it goes, at the graph's corner: 20 px apart would be 6 on screen, so 12.
// Four steps of ⌘-scroll: one event zooms by ×1.5 at most (VIEW-5).
{
  const a = await look(id);
  for (let i = 0; i < 4; i++) await wheel(a.node.x, a.node.y, 100);
  await dots();
  const wrong = dotsAt.filter((d) => {
    let want = 20 * d.zoom;
    while (want < 12) want *= 2;
    return Math.abs(d.w - want) > 0.3 || Math.abs(d.x + d.w / 2) > 0.01 || d.r !== 1;
  });
  check(dotsAt.length === 5 && wrong.length === 0, `the dots are the graph's at each zoom (${dotsAt.map((d) => `${d.zoom}: ${d.w.toFixed(1)} px`).join(", ")})${wrong.length ? `; wrong: ${JSON.stringify(wrong)}` : ""}`);
  // On the screen: every dot is a whole number of 20 graph px from where the first zoom's are, from
  // the node's corner (a dot a screen pixel off, or the pattern from another corner, is not).
  const mod = (v) => ((v % 20) + 20) % 20;
  const dist = (a, b) => Math.min(Math.abs(a - b), 20 - Math.abs(a - b));
  const ref = seenAt[0]?.dots[0];
  const off = seenAt.flatMap((z) => z.dots.map((d) => ({ zoom: z.zoom, by: Math.max(dist(mod(d.x), mod(ref.x)), dist(mod(d.y), mod(ref.y))) * z.zoom }))).filter((d) => d.by > 1);
  check(
    !!ref && seenAt.length === 5 && seenAt.every((z) => z.dots.length >= 4) && off.length === 0,
    `on the screen, the dots keep their place in the graph from a node's corner at each zoom (${seenAt.map((z) => `${z.zoom.toFixed(2)}: ${z.dots.length} dots`).join(", ")})${off.length ? `; off: ${JSON.stringify(off.slice(0, 5))}` : ""}`,
  );
}

done(errors);
await browser.close();
