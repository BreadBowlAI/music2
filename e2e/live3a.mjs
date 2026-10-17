// Step 3a in the app, on a fresh copy of first-song (124 bpm, 32 beats): the live engine.
//   seek     a click on the ruler, or a far seek, is a jump of the transport: it lands at once and
//            rebuilds nothing (RENDER-8, RENDER-9, RENDER-29)
//   loop     dragged across the ruler, toggled in the bar: playback stays inside it, wrapping
//            (RENDER-9, RENDER-9a); off, it plays straight on
//   notes    a note added in the piano roll while playing is written and played with no jump back
//            (RENDER-26)
//   effect   an effect added while playing takes over where playback is: no jump back (RENDER-25a)
//   stop     the transport holds (RENDER-6a)
//   lock     the piano roll is read only while its lane's file doesn't read (FMT-9), with a chip
//            saying why, and editable again once it is fixed
//   node run.mjs live3a fix
import { check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, api, state, show, view, box, drag } = await open(1600, 1000);
// The roll's rows and its ruler (taste.md §3.5: 14px rows, the arrangement's 28px ruler).
const ROW = 14, RULER = 28;
const status = async () => (await api("/status")).body;
const playing = async () => (await status()).playing;
const playButton = 'button[title="Play / stop (Space)"]';
const play = async () => (await playing()) || (await page.click(playButton), until(playing, 4000));
const stop = async () => (!(await playing())) || (await page.click(playButton), until(async () => !(await playing()), 4000));
const revision = async () => (await state()).view.revision;
const seek = (beat) => api("/seek", { method: "POST", body: JSON.stringify({ beat }) });
/** Playback sits on whole frames: a beat lands within a frame of where it was asked. */
const near = (a, b) => Math.abs(a - b) < 1e-3;
/** The loop as `{start, end, on}`, in that order, or null. */
const loopNow = async () => { const l = (await status()).loop; return l && JSON.stringify({ start: l.start, end: l.end, on: l.on }); };

/** The playhead, sampled every ~25 ms until `finish()`: the largest step back, in beats. */
function watch() {
  const beats = [];
  let stopped = false;
  const run = (async () => {
    while (!stopped) {
      beats.push((await status()).beat);
      await sleep(25);
    }
  })();
  return {
    beats,
    async finish() {
      stopped = true;
      await run;
      let back = 0;
      for (let i = 1; i < beats.length; i++) back = Math.max(back, beats[i - 1] - beats[i]);
      return back;
    },
  };
}

/** Where bar `n`'s line is on screen (its label sits 6 px right of it), and the ruler's middle. */
const barLine = async (n) =>
  page.$$eval("[data-testid=ruler] span", (els, n) => {
    const r = els.find((e) => e.textContent === String(n)).getBoundingClientRect();
    return { x: r.left - 6, y: r.top + 10 };
  }, n);

await page.$eval("[data-testid=timeline]", (e) => { e.scrollLeft = 0; });
const rev0 = await revision();

// ---------------------------------------------------------------- seek

const bar5 = await barLine(5);
await page.mouse.click(bar5.x + 2, bar5.y);
check(await until(async () => near((await status()).beat, 16), 1000), `a click on the ruler at bar 5 lands on beat 16 at once (${(await status()).beat})`);
check(!(await playing()), "stopped, it stays stopped");
await play();
check(await until(async () => (await status()).beat > 16.5, 3000), "it plays on from there");
const bar7 = await barLine(7);
let t = Date.now();
await page.mouse.click(bar7.x + 2, bar7.y);
check(await until(async () => { const b = (await status()).beat; return b > 23.999 && b < 25; }, 1000), `playing, a click at bar 7 is heard from beat 24 within ${Date.now() - t} ms`);
check(await playing(), "and it plays on");
// Far past the song: a seek used to replay the song up to there; now it is a jump.
t = Date.now();
const far = await seek(400);
check(far.status === 200 && (await until(async () => (await status()).beat > 399.999, 1000)), `a seek to beat 400 lands at once (${Date.now() - t} ms, ${JSON.stringify(far.body)})`);
check((await revision()) === rev0, "seeking makes no revision and rebuilds nothing");
check((await seek(-5)).status === 200 && (await until(async () => (await status()).beat < 0.5, 1000)), "a seek before the start lands at 0");
check((await api("/seek", { method: "POST", body: '{"beat": "soon"}' })).status !== 200, "a seek to something that isn't a beat is refused");
await stop();

// ---------------------------------------------------------------- loop

const toggle = "[data-testid=loop-toggle]";
check((await status()).loop === null && !(await page.$("[data-testid=loop-range]")), "no loop to begin with");
check((await page.$eval(toggle, (e) => e.getAttribute("aria-pressed"))) === "false", "the loop button is off");
await seek(2);
await until(async () => near((await status()).beat, 2), 1000);
// The page hears of a seek made here at its next poll (every 200 ms). Until then the timeline can
// still be scrolled to beat 400, where it followed the playhead above, so the bar lines are
// measured only once the page shows beat 2.
await until(async () => near(Number(await page.$eval("[data-testid=playhead]", (e) => e.dataset.beat)), 2), 2000);
// Drag across the ruler from bar 2 to bar 4: beats 4 to 12.
const [b2, b4] = [await barLine(2), await barLine(4)];
await drag({ x: b2.x + 2, y: b2.y }, { x: b4.x + 2, y: b4.y });
check(await until(async () => (await loopNow()) === JSON.stringify({ start: 4, end: 12, on: true }), 2000), `dragging across the ruler loops beats 4–12: ${await loopNow()}`);
check(near((await status()).beat, 2), "the drag doesn't seek");
check(await until(() => page.$eval("[data-testid=loop-range]", (e) => e.dataset.on === "true" && e.dataset.start === "4" && e.dataset.end === "12")), "the ruler shows the loop");
check(await until(() => page.$eval(toggle, (e) => e.getAttribute("aria-pressed") === "true" && /bars 2–3/.test(e.title))), `the loop button is on: ${await page.$eval(toggle, (e) => e.title)}`);
check(!!(await page.$("[data-testid=loop-shade]")), "and the rows are shaded under it");
// Playing from beat 10, it wraps at 12 to 4 and stays inside: 8 beats are 3.9 s at 124 bpm.
await seek(10);
await play();
const inside = watch();
await sleep(5000);
const back = await inside.finish();
// Within a frame: the loop's start frame is beat 4 rounded to a frame.
const out = inside.beats.filter((b) => b < 4 - 1e-3 || b >= 12);
check(out.length === 0 && inside.beats.length > 50, `for 5 s it plays inside beats 4–12 (${inside.beats.length} samples, outside: ${out.slice(0, 3)})`);
check(back > 5, `it went round: the playhead went back ${back.toFixed(2)} beats at the wrap`);
check((await revision()) === rev0, "looping makes no revision");
// Off: the range is kept, and it plays straight on past it.
await page.click(toggle);
check(await until(async () => (await loopNow()) === JSON.stringify({ start: 4, end: 12, on: false }), 1000), `the button turns it off and keeps the range: ${await loopNow()}`);
check(await until(() => page.$eval("[data-testid=loop-range]", (e) => e.dataset.on === "false")) && !(await page.$("[data-testid=loop-shade]")), "the ruler shows the range dimmed");
check(await until(async () => (await status()).beat > 12.5, 6000), "and playback runs on past beat 12");
await page.click(toggle);
check(await until(async () => (await status()).loop?.on === true, 1000), "the button turns it back on over the same range");
await page.click(toggle);
check(await until(async () => (await status()).loop?.on === false, 1000), "and off again");
// The engine refuses what it couldn't play.
const short = await api("/loop", { method: "POST", body: JSON.stringify({ start: 4, end: 4.001 }) });
check(short.status === 400 && /at least/.test(short.body.error), `a loop shorter than 5 ms is refused: ${short.body.error}`);
await stop();

// ---------------------------------------------------------------- a note edit while playing

const notesFile = "data/bass/notes.json";
const original = read(notesFile);
const clipBox = await box('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await page.mouse.click(clipBox.x + 60, clipBox.y + clipBox.height / 2, { count: 2 });
check(await until(async () => (await page.$$("[data-testid=notes] [data-note]")).length === 48), "the bass clip opens in the piano roll");
await page.$eval("[data-testid=roll-scroller]", (el, row, ruler) => { el.scrollTop = ruler + (127 - 60) * row - el.clientHeight / 2; }, ROW, RULER);
await sleep(150);
const grid = await box("[data-testid=roll-grid]");
const ppb = await page.evaluate(() => {
  const n = [...document.querySelectorAll("[data-testid=notes] [data-note]")].find((e) => Number(e.dataset.start) > 0);
  return (n.getBoundingClientRect().left - document.querySelector("[data-testid=roll-grid]").getBoundingClientRect().left) / Number(n.dataset.start);
});
const cell = (pitch, beat) => ({ x: grid.x + beat * ppb, y: grid.y + (127 - pitch) * ROW + ROW / 2 });
await seek(4);
await play();
await until(async () => (await status()).beat > 4.5, 3000);
let rev = await revision();
let w = watch();
const c4 = cell(60, 20.1);
await page.mouse.click(c4.x, c4.y, { count: 2 });
check(await until(() => JSON.parse(read(notesFile)).clips[0].notes.some((n) => n.pitch === "C4" && n.start === 20)), "a double-click adds C4 at beat 20 while playing");
check(await until(async () => (await revision()) === rev + 1, 3000), "one write");
await sleep(400);
let stepBack = await w.finish();
check(stepBack < 1e-6, `the playhead never went back (largest step back ${stepBack.toFixed(4)} beats over ${w.beats.length} samples)`);
check(await playing(), "and it plays on");
check(!(await page.$("[data-testid=compile-banner]")), "no compile banner");
await stop();

// ---------------------------------------------------------------- an effect added while playing

const keysTs = "tracks/keys.ts";
const keysOrig = read(keysTs);
await show("keys");
// Selecting Keys keeps the dock on Notes (taste.md §3.7): its graph is the Device view.
await view("graph");
await seek(4);
await play();
await until(async () => (await status()).beat > 4.5, 3000);
rev = await revision();
w = watch();
await page.click('[data-module="keys"] button[aria-label="Add an effect after pluck"]');
await page.waitForSelector('[data-module="keys"] [data-effect="delay"]');
await page.click('[data-module="keys"] [data-effect="delay"]');
check(await until(() => read(keysTs).includes("delay(pluck("), 5000), "a delay goes after the keys' pluck while playing");
check(await until(() => page.$('[data-module="keys"] [data-node="delay"]'), 5000), "and shows in the graph");
await sleep(400);
stepBack = await w.finish();
check(stepBack < 1e-6, `the playhead never went back (largest step back ${stepBack.toFixed(4)} beats over ${w.beats.length} samples)`);
check(await playing(), "and it plays on");
await page.click('button[aria-label="Undo"]');
check(await until(() => read(keysTs) === keysOrig, 5000), "undo takes the delay out");

// ---------------------------------------------------------------- stop

await stop();
const held = (await status()).beat;
await sleep(500);
check((await status()).beat === held && !(await playing()), `stopped, the transport holds at beat ${held.toFixed(3)}`);

// ---------------------------------------------------------------- the piano roll's lock

const locked = () => page.$("[data-testid=notes-locked]");
// Back from the Device view: open the clip again.
const again = await box('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await page.mouse.click(again.x + 60, again.y + again.height / 2, { count: 2 });
check(await until(async () => (await page.$$("[data-testid=notes] [data-note]")).length === 49), "the bass clip opens again, C4 and all");
await page.$eval("[data-testid=roll-scroller]", (el, row, ruler) => { el.scrollTop = ruler + (127 - 60) * row - el.clientHeight / 2; }, ROW, RULER);
await sleep(150);
check(!(await locked()), "the roll is editable");
const broken = original.replace('"start": 0, "length": 32,', '"start": 0, "length": 32, "swing": 0.2,');
check(broken !== original, "(the fixture's clip is as expected)");
// The clip with the C4 just added, the file then broken by hand (an agent, a text editor).
const withC4 = read(notesFile);
write(notesFile, withC4.replace(/"length": 32,/, '"length": 32, "swing": 0.2,'));
check(await until(async () => (await state()).problems.some((p) => p.code === "lane.invalid" && p.src === notesFile), 5000), "the file is lane.invalid");
check(await until(async () => !!(await locked()), 3000), "the open piano roll locks");
const chip = await page.$eval("[data-testid=notes-locked]", (e) => ({ why: e.dataset.why, text: e.textContent, title: e.title }));
check(chip.why === "lane" && chip.text.includes("data/bass/notes.json") && /swing/.test(chip.title), `with a chip saying why: "${chip.text}" (${chip.title.slice(0, 120)}…)`);
check(await page.$eval('[data-testid=dock] input[aria-label="Clip length in bars"]', (e) => e.disabled), "the length can't be changed");
const before = read(notesFile);
const d4 = cell(62, 6.1);
await page.mouse.click(d4.x, d4.y, { count: 2 });
await sleep(800);
check(read(notesFile) === before, "a double-click adds nothing: the broken file is left as it is");
check((await page.$$("[data-testid=notes] [data-note]")).length === 49, "the roll still shows the notes the lane plays");
write(notesFile, withC4);
check(await until(async () => !(await locked()), 5000), "fixed by hand, it unlocks");
await page.mouse.click(d4.x, d4.y, { count: 2 });
check(await until(() => JSON.parse(read(notesFile)).clips[0].notes.some((n) => n.pitch === "D4" && n.start === 6), 5000), "and a double-click adds a note again");
write(notesFile, original);

await page.screenshot({ path: `${process.argv[2]}/live3a.png` });
await browser.close();
done(errors);
