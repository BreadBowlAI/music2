// Feeds (FEED-1, FEED-7): an agent plays a track live. On the run's server with first-song open:
//   - Keys's menu offers Let an agent play this track; it opens a feed, says so in a toast, and the
//     header shows the Live mark; the menu no longer offers it; nothing is written, no undo step;
//   - once an agent writes (the tools key, as Claude), the mark names it: Live · Claude;
//   - while the song plays the feed, the page keeps drawing and the playhead moves: nothing freezes;
//   - Keep what played, on the mark, writes it as one undo step, said in a toast with Undo, which
//     takes it back;
//   - Close, on the mark, ends the feed: the mark goes;
//   - an agent's own open and close show and take away the mark, with its name;
//   - the mark and its menu linted, the mark with a tooltip; no console errors.
//
//   node e2e/live.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { lint } from "./lintlib.mjs";
import { check, done, open, read, sess, sleep, until } from "./fixlib.mjs";

const { page, errors, api, state, toasts } = await open(1440, 900);
const mark = '[data-testid=live-mark][data-module="keys"]';
/** A feed route, with the tools key, as the agent Claude: the status and the JSON answer. */
const agent = async (method, path, body) => {
  const res = await fetch(`${sess.url}/api/feed${path}`, {
    method,
    headers: { "x-m2-token": sess.token, "content-type": "application/json", "x-m2-as": "Claude" },
    body: body ? JSON.stringify({ session: (await state()).session, ...body }) : undefined,
  });
  return { status: res.status, body: await res.json().catch(() => ({})) };
};
const markText = () => page.$eval(mark, (e) => e.textContent ?? "").catch(() => null);
const menuRows = async (m) => {
  await page.click(`[data-track-menu="${m}"]`);
  await page.waitForSelector("[data-testid=track-menu]", { timeout: 3000 });
  const rows = await page.$$eval("[data-testid=track-menu] [role=menuitem]", (els) => els.map((e) => e.textContent ?? ""));
  return rows;
};
const closeMenu = async () => {
  await page.keyboard.press("Escape");
  await until(async () => !(await page.$("[data-testid=track-menu]")));
};
const toasted = (words) => until(async () => (await toasts()).some((t) => t.includes(words)));

// ---------------------------------------------------------------- the musician opens it

const lane = read("data/keys/chords.json");
const steps = (await state()).undo_name?.step ?? null;
let rows = await menuRows("keys");
check(rows.some((r) => r.startsWith("Let an agent play this track")), `Keys's menu offers it: ${rows.join(" | ")}`);
await page.evaluate(() => [...document.querySelectorAll("[data-testid=track-menu] [role=menuitem]")].find((e) => e.textContent.startsWith("Let an agent play"))?.click());
check(await until(async () => !!(await page.$(mark))), "the Live mark shows on Keys's header");
check((await markText()) === "Live", `it says Live, with no writer yet: ${await markText()}`);
check(await toasted("open for an agent to play live"), `a toast says how: ${(await toasts()).join(" | ")}`);
check(read("data/keys/chords.json") === lane && ((await state()).undo_name?.step ?? null) === steps, "nothing written, no undo step");
rows = await menuRows("keys");
check(!rows.some((r) => r.startsWith("Let an agent play")), `the menu no longer offers it: ${rows.join(" | ")}`);
await closeMenu();
const tip = await page.$eval(mark, (e) => e.title);
check(tip.includes("live"), `the mark has a tooltip: ${tip}`);

// ---------------------------------------------------------------- the agent writes, the song plays

const notes = [0, 1, 2, 3, 4, 5, 6, 7].map((k) => ({ pitch: 72 + (k % 4) * 2, start: 0.5 + k * 0.5, length: 0.25 }));
const put = await agent("POST", "/put", { feed: "keys", from: 0, to: 8, notes });
check(put.status === 200 && put.body.taken === 8, `Claude's put is taken: ${JSON.stringify(put.body)}`);
check(await until(async () => (await markText()) === "Live · Claude"), `the mark names the writer: ${await markText()}`);
await api("/transport", { method: "POST", body: JSON.stringify({ action: "play" }) });
const head = async () => (await api("/status")).body.beat;
const b0 = await head();
// The page draws while the feed plays: frames counted for 2 s.
const frames = await page.evaluate(
  () =>
    new Promise((r) => {
      let n = 0;
      let most = 0;
      let last = performance.now();
      const t0 = last;
      const tick = (t) => {
        n++;
        most = Math.max(most, t - last);
        last = t;
        if (t - t0 < 2000) requestAnimationFrame(tick);
        else r({ n, most });
      };
      requestAnimationFrame(tick);
    }),
);
const b1 = await head();
check(frames.n > 20 && frames.most < 1000, `nothing freezes while it plays: ${frames.n} frames in 2 s, the longest gap ${Math.round(frames.most)} ms`);
check(b1 > b0 + 1, `the playhead moves: beat ${b0} → ${b1}`);
const st = await agent("GET", "", null);
check(st.body.feeds?.[0]?.playing === true && st.body.feeds[0].writer === "Claude", `GET /api/feed says it plays: ${JSON.stringify(st.body.feeds?.[0] ?? null)}`);
await api("/transport", { method: "POST", body: JSON.stringify({ action: "stop" }) });

// ---------------------------------------------------------------- Keep what played, Close

await page.click(mark);
await page.waitForSelector("[data-testid=live-menu]", { timeout: 3000 });
const issues = [...(await page.evaluate(lint, "[data-testid=live-menu]")), ...(await page.evaluate(lint, mark))];
check(issues.length === 0, `the mark and its menu are in the design system: ${issues.slice(0, 3)}`);
await page.click("[data-testid=live-keep]");
check(await toasted("Kept what Claude played on Keys as a new clip"), `Keep says so: ${(await toasts()).join(" | ")}`);
check(await until(() => read("data/keys/chords.json") !== lane), "and the lane has it");
const kept = (await state()).undo_name;
check(kept?.step != null && kept.step !== steps, `one undo step: ${JSON.stringify(kept)}`);
await page.evaluate(() => [...document.querySelectorAll("[data-testid=toast] button")].find((b) => b.textContent === "Undo")?.click());
check(await until(() => read("data/keys/chords.json") === lane), "its Undo takes it back, byte for byte");
await page.click(mark);
await page.waitForSelector("[data-testid=live-menu]", { timeout: 3000 });
await page.click("[data-testid=live-close]");
check(await until(async () => !(await page.$(mark))), "Close takes the mark away");
check(((await agent("GET", "", null)).body.feeds ?? []).length === 0, "and the feed is closed");

// ---------------------------------------------------------------- an agent opens and closes

const opened = await agent("POST", "/open", { track: "kick" });
check(opened.status === 200 && opened.body.writer === "Claude", `an agent opens one: ${JSON.stringify(opened.body)}`);
check(await until(async () => (await page.$eval('[data-testid=live-mark][data-module="kick"]', (e) => e.textContent).catch(() => null)) === "Live · Claude"), "Kick shows Live · Claude");
await agent("POST", "/close", { feed: "kick" });
check(await until(async () => !(await page.$('[data-testid=live-mark][data-module="kick"]'))), "its close takes the mark away");
await sleep(100);
done(errors);
