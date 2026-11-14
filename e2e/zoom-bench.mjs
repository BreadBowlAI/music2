// The zoom bench (taste.md VIEW-10; the zoom note, docs/designs/camera/zoom.md, §1 and §6 step 1): a
// measure in the nightly tier, so a change to how zooming draws can be compared with what was
// before. In Chrome at 1440×900 and 2 device px a px, as the owner's Mac, on two songs: a beat song
// like the owner's (beat-song.mjs: 10 tracks, 312 clips, nearly all in view) and the big song
// (big-song.mjs: 24 tracks of 64 clips). On each, three times: the owner's A13 pinch and a run of
// ⌘-scroll over the timeline, at 24 px a beat from the song's start, and the A13 pinch over the
// Song's graph at Fit. The app's zoom bench (`m2canvas.bench()`, the swipe probe's own) replays each
// and splits every frame into script (and the layout it forced), style and layout, paint and
// commit, and input; the plan and the page's side are zoomlib.mjs's, which the WebKit tool shares.
//
// It prints a table for each song and writes every frame as JSON beside its log
// (<out>/zoom-bench-fix.json, the run's folder's name and .json). It judges no speed: its checks
// prove only that it measured: each run saw frames while its gesture lasted, every part was timed,
// and the zoom went out and came back.
//
//   node e2e/zoom-bench.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { ROOT, check, done, open, sleep, until } from "./fixlib.mjs";
import { GESTURES, PLAN, measured, rowsOf, zoomRun, zoomTable } from "./zoomlib.mjs";

const { browser, page, errors, api } = await open(1440, 900);
// The owner's Mac: a window of 1440×900 px at 2 device px a px.
await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 2 });
const here = path.dirname(fileURLToPath(import.meta.url));

const SONGS = [
  { song: "beat", what: "the beat song (beat-song.mjs: 10 tracks, 312 clips)", make: "beat-song.mjs", last: "keys" },
  { song: "big", what: "the big song (big-song.mjs: 24 tracks of 64 clips)", make: "big-song.mjs", last: "keys-6" },
];
const engine = { browser: await browser.version(), userAgent: await page.evaluate(() => navigator.userAgent), dpr: await page.evaluate(() => devicePixelRatio), window: [1440, 900] };
const songs = [];
for (const s of SONGS) {
  const dir = path.join(ROOT, s.song);
  execFileSync(process.execPath, [path.join(here, s.make), dir]);
  const opened = (await api("/open", { method: "POST", body: JSON.stringify({ path: dir }) })).status === 200;
  const drawn =
    opened &&
    (await page.waitForSelector(`[data-testid=timeline] [data-track="${s.last}"]`, { timeout: 60000 }).then(
      () => true,
      () => false,
    )) &&
    (await until(async () => (await page.$$("[data-testid=timeline] [data-clip]")).length > 50, 60000));
  check(drawn, `${s.what} opens, its clips drawn`);
  if (!drawn) continue;
  // Its first draws and the compile done.
  await sleep(2000);
  const results = [];
  for (const step of PLAN) results.push({ ...step, run: await page.evaluate(zoomRun, step) });
  console.log(`\n## ${s.what}: Chrome ${engine.browser}, ${engine.window.join("×")} at ${engine.dpr} device px a px\n`);
  console.log(await page.evaluate(zoomTable, "Run", rowsOf(results)));
  console.log("");
  for (const g of GESTURES) {
    const runs = results.filter((r) => r.name === g).map((r) => r.run);
    const [framesOk, zoomOk, seen] = measured(runs);
    check(
      framesOk,
      `${s.song} song, ${g}: each of its ${runs.length} runs saw frames while the gesture lasted, every part of each frame timed, and its input: ${JSON.stringify(seen.map(({ frames, untimed, input }) => ({ frames, untimed, input })))}`,
    );
    check(zoomOk, `${s.song} song, ${g}: each run's zoom went out and came back to where it started: ${JSON.stringify(seen.map((x) => x.zoom))}`);
  }
  songs.push({ song: s.song, what: s.what, results });
}
const json = `${ROOT}.json`;
fs.writeFileSync(json, JSON.stringify({ engine, songs }, null, 1));
console.log(`Every frame: ${json}`);

await browser.close();
done(errors);
