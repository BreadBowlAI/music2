// 8b.2 The song bar in the app (design-8b §4, taste.md §3.3, §5.1, TB-5): the tempo field (drag,
// Shift-drag, type, no Alt-click reset, undo), the time signature picker that re-bars the ruler,
// the ruler's marks and right-click menu, the song's name edited in place, the transport (Space
// stops back where play started, Shift+Space continues, C, Cmd+L, Cmd+.), a click on empty lane
// space, the loop kept on reopening and turned off by a fast tempo, the bar in the Mac app's
// layout, and a read-only window.
//
//   node e2e/s8-songbar.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { spawn } from "node:child_process";
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until } from "./fixlib.mjs";

const { page, errors, posts, api, state, toasts, key, box } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const status = async () => (await api("/status")).body;
const arrangement = () => JSON.parse(read("data/arrangement.json"));
const tempos = () => arrangement().tempo.map((t) => [t.beat, t.bpm]);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
/** The loop is `start`–`end`, on or off (null: none). */
const loopIs = (l, start, end, on) => !!l && l.start === start && l.end === end && l.on === on;
/** Playback sits on whole frames: a beat it was sent to can read a hair before it. */
const at = (beat, want) => Math.abs(beat - want) < 1e-3;
const sent = (path) => posts.filter((p) => p.url === `/api/graph/${path}`).length;
const field = () => page.$eval("[data-testid=tempo-field]", (e) => ({ bpm: Number(e.dataset.bpm), text: e.textContent, title: e.title, disabled: e.getAttribute("aria-disabled") === "true" }));
const meterText = () => page.$eval("[data-testid=meter-field]", (e) => e.textContent);
const undo = async () => {
  const rev = (await state()).view.revision;
  await key("Meta+z");
  return until(async () => (await state()).view.revision !== rev);
};
/** Drag the tempo field up by `dy` pixels (down when negative), with Shift held when `fine`. */
const dragTempo = async (dy, { fine = false, during } = {}) => {
  const b = await box("[data-testid=tempo-field]");
  const x = b.x + b.width / 2, y = b.y + b.height / 2;
  if (fine) await page.keyboard.down("Shift");
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x, y - dy, { steps: 10 });
  const seen = during ? await during() : null;
  await page.mouse.up();
  if (fine) await page.keyboard.up("Shift");
  return seen;
};
const ruler = () => box("[data-testid=ruler]");
/** A click on the ruler at `beat` (whole beats): moves the playhead, and where play starts. */
const seekRuler = async (beat) => {
  const r = await ruler();
  await page.mouse.click(r.x + beat * 24 + 2, r.y + r.height / 2);
  return until(async () => at((await status()).beat, beat));
};
const orig = { arrangement: read("data/arrangement.json"), manifest: read("music2.toml") };

await page.waitForSelector("[data-testid=tempo-field]");

// ---------------------------------------------------------------- the tempo field (TB-5)

const f0 = await field();
check(f0.bpm === 124 && f0.text === "124BPM" && (await meterText()) === "4/4", `the bar shows the song's tempo and time signature: ${f0.text}, ${await meterText()}`);
check(f0.title.includes("Heard as you drag") && f0.title.includes("Saved in data/arrangement.json") && f0.title.includes("⇧ for fine"), `its tooltip says how, and where it is kept: ${f0.title.replace(/\n/g, " | ")}`);
const during = await dragTempo(40, {
  during: async () => ({ bubble: await page.$eval("[data-testid=value-bubble]", (e) => e.textContent).catch(() => null), file: read("data/arrangement.json") }),
});
check(during.bubble === "134 BPM" && during.file === orig.arrangement, `a drag shows the value as it goes (${during.bubble}) and writes nothing yet`);
check(await until(() => same(tempos(), [[0, 134]])), `let go, 40 px up is 10 BPM more: ${JSON.stringify(tempos())}`);
await sleep(400);
// A drag is the tempo lease's (8b.7): written by its commit, once.
check(sent("tempo/lease/commit") === 1 && sent("tempo") === 0 && (await toasts()).length === 0, `written once, with no toast (${sent("tempo/lease/commit")} writes, toasts: ${await toasts()})`);
check((await field()).bpm === 134, "the field shows it");
check(await page.$eval('button[aria-label="Undo"]', (e) => e.title === "Undo Tempo (⌘Z)"), `Undo names it: ${await page.$eval('button[aria-label="Undo"]', (e) => e.title)}`);
await dragTempo(-40, { fine: true });
check(await until(() => same(tempos(), [[0, 133]])), `Shift-drag is fine: 40 px down is 1 BPM less (${JSON.stringify(tempos())})`);

// Double-click to type.
const fb = await box("[data-testid=tempo-field]");
await page.mouse.click(fb.x + fb.width / 2, fb.y + fb.height / 2, { count: 2 });
const input = await page.waitForSelector("[data-testid=tempo-field-input]", { timeout: 2000 }).catch(() => null);
check(!!input && (await page.$eval("[data-testid=tempo-field-input]", (e) => e.value === "133" && e.selectionStart === 0 && e.selectionEnd === 3)), "a double-click opens it to type, all selected");
await page.keyboard.type("1000");
await page.keyboard.press("Enter");
const refused = await page.$eval("[data-testid=tempo-field-input] + [role=alert]", (e) => e.textContent).catch(() => null);
check(refused === "A tempo is 20 to 999 BPM" && same(tempos(), [[0, 133]]), `1000 is refused in words, and nothing is written: ${refused}`);
await page.keyboard.press("Escape");
check(await until(async () => !(await page.$("[data-testid=tempo-field-input]"))) && (await field()).bpm === 133, "Escape puts it back");
await page.mouse.click(fb.x + fb.width / 2, fb.y + fb.height / 2, { count: 2 });
await page.waitForSelector("[data-testid=tempo-field-input]");
await page.keyboard.type("128.55 bpm");
await page.keyboard.press("Enter");
check(await until(() => same(tempos(), [[0, 128.55]])), `typing "128.55 bpm" writes 128.55 (${JSON.stringify(tempos())})`);
const fit = await until(async () => (await field()).bpm === 128.55);
const cut = await page.$eval("[data-testid=tempo-field]", (e) => e.scrollWidth > e.clientWidth || e.getBoundingClientRect().width > 76.5);
check(fit && !cut, `a two-decimal tempo fits its 76 px field: ${(await field()).text}`);

// No Alt-click reset: a stray click would reset the song's tempo (taste.md §5.2).
const before = sent("tempo");
await page.keyboard.down("Alt");
await page.mouse.click(fb.x + fb.width / 2, fb.y + fb.height / 2);
await page.keyboard.up("Alt");
await sleep(400);
check(sent("tempo") === before && same(tempos(), [[0, 128.55]]), "Alt-click changes nothing");

// Undo, step by step, back to the bytes the song had.
for (let i = 0; i < 3; i++) await undo();
check(await until(() => read("data/arrangement.json") === orig.arrangement), "three undos put data/arrangement.json back, byte for byte");
check(await until(async () => (await field()).bpm === 124), "and the field follows");

// ---------------------------------------------------------------- the time signature

await page.click("[data-testid=meter-field]");
const picker = await page.waitForSelector("[data-testid=meter-picker]", { timeout: 2000 }).catch(() => null);
check(!!picker, "a click on 4/4 opens the time signature picker");
await page.click("[data-testid=meter-beats]", { clickCount: 3 });
await page.keyboard.type("7");
await page.keyboard.press("Enter");
await until(() => same(arrangement().meter, [{ beat: 0, beats: 7, unit: 4 }]));
await page.click('[data-testid=meter-picker] [data-unit="8"]');
check(await until(async () => same(arrangement().meter, [{ beat: 0, beats: 7, unit: 8 }]) && (await meterText()) === "7/8"), `7 beats, each an eighth: 7/8 (${JSON.stringify(arrangement().meter)}, ${await meterText()})`);
const bars = await page.$$eval("[data-testid=ruler] > span", (els) => els.map((e) => [e.textContent, Math.round(parseFloat(e.style.left))]));
// A 7/8 bar is 3.5 beats, 84 px: bar 2 starts at 84 (its number 6 px in).
check(same(bars.slice(0, 3), [["1", 6], ["2", 90], ["3", 174]]), `the ruler is re-barred: ${JSON.stringify(bars.slice(0, 4))}`);
await page.screenshot({ path: `${ROOT}/s8-songbar-meter.png` });
await page.keyboard.press("Escape");
await undo();
await undo();
check(await until(() => read("data/arrangement.json") === orig.arrangement) && (await meterText()) === "4/4", "two undos put 4/4 back");

// ---------------------------------------------------------------- the ruler's changes (design-8b §4.4)

const r = await ruler();
await page.mouse.click(r.x + 16 * 24 + 2, r.y + r.height / 2, { button: "right" });
const menu = await page.waitForSelector("[data-testid=ruler-menu]", { timeout: 2000 }).catch(() => null);
const rows = menu ? await page.$$eval("[data-testid=ruler-menu] [role=menuitem]", (els) => els.map((e) => e.textContent)) : [];
check(same(rows, ["Add tempo change at 5.1", "Add time signature change at bar 5", "Add marker at bar 5"]), `a right-click on bar 5 offers a change there: ${rows.join(" | ")}`);
await page.$$eval("[data-testid=ruler-menu] [role=menuitem]", (els) => els[0].click());
check(await until(() => same(tempos(), [[0, 124], [16, 124]])), `Add tempo change writes a second entry at beat 16: ${JSON.stringify(tempos())}`);
const typing = await page.waitForSelector("[data-testid=mark-tempo-input]", { timeout: 3000 }).catch(() => null);
check(!!typing, "and opens it at its mark, to type its tempo");
await page.keyboard.type("140");
await page.keyboard.press("Enter");
check(await until(() => same(tempos(), [[0, 124], [16, 140]])), `typed, it is 140 from bar 5: ${JSON.stringify(tempos())}`);
await page.keyboard.press("Escape");
const mark = await until(async () => (await page.$eval('[data-testid=ruler] [data-mark="16"]', (e) => e.textContent).catch(() => null)) === "♩140");
check(mark, "its mark shows on the ruler at bar 5");
check((await field()).bpm === 124 && (await field()).title.startsWith("Tempo from the start: 124 BPM"), `the bar edits the tempo at the playhead: ${(await field()).title.split("\n")[0]}`);
await seekRuler(20);
check(await until(async () => (await field()).bpm === 140) && (await field()).title.startsWith("Tempo from bar 5: 140 BPM"), `past bar 5 it shows that change's: ${(await field()).title.split("\n")[0]}`);
await page.click('[data-testid=ruler] [data-mark="16"]');
await page.waitForSelector("[data-testid=mark-remove]", { timeout: 2000 }).catch(() => null);
const removeText = await page.$eval("[data-testid=mark-remove]", (e) => e.textContent).catch(() => null);
await page.click("[data-testid=mark-remove]").catch(() => {});
check(removeText === "Remove this tempo change" && (await until(() => same(tempos(), [[0, 124]]))), `its popover removes it: ${removeText}`);
check(await until(async () => !(await page.$('[data-testid=ruler] [data-mark="16"]'))), "and its mark goes");
await undo();
await undo();
await undo();
check(await until(() => read("data/arrangement.json") === orig.arrangement), "undo takes the three back");

// A mark with a tempo and a time signature change: Remove these changes is one step (WRITE-21).
const session = (await state()).session;
await post("/tempo", { session, at: 16, bpm: 140, add: true });
await post("/meter", { session, at: 16, beats: 3, unit: 4, add: true });
const both = read("data/arrangement.json");
await until(async () => !!(await page.$('[data-testid=ruler] [data-mark="16"]')));
await page.click('[data-testid=ruler] [data-mark="16"]');
await page.waitForSelector("[data-testid=mark-remove]", { timeout: 2000 }).catch(() => null);
const removeBoth = await page.$eval("[data-testid=mark-remove]", (e) => e.textContent).catch(() => null);
await page.click("[data-testid=mark-remove]").catch(() => {});
check(removeBoth === "Remove these changes" && (await until(() => same(tempos(), [[0, 124]]) && arrangement().meter.every((m) => m.beat === 0))), `"${removeBoth}" takes both out`);
check(await until(async () => (await page.$eval('button[aria-label="Undo"]', (e) => e.title)) === "Undo removing the changes at bar 5 (⌘Z)"), `as one step: ${await page.$eval('button[aria-label="Undo"]', (e) => e.title)}`);
await undo();
check(await until(() => read("data/arrangement.json") === both), "one undo brings both back");
await undo();
await undo();
check(await until(() => read("data/arrangement.json") === orig.arrangement), "and the song is as it was");

// A change from bar 12: the tooltips say bar 12, not "the start2".
await post("/tempo", { session, at: 44, bpm: 140, add: true });
await post("/meter", { session, at: 44, beats: 3, unit: 4, add: true });
// By the route: a click at bar 12 on the ruler would land on its mark.
await post("/seek", { beat: 46 });
await until(async () => at((await status()).beat, 46));
check(await until(async () => (await field()).title.startsWith("Tempo from bar 12: 140 BPM")), `the tempo's tooltip: ${(await field()).title.split("\n")[0]}`);
check(await until(async () => (await page.$eval("[data-testid=meter-field]", (e) => e.title)).startsWith("Time signature from bar 12: 3/4")), `the time signature's: ${(await page.$eval("[data-testid=meter-field]", (e) => e.title)).split("\n")[0]}`);
await post("/seek", { beat: 4 });
check(await until(async () => (await field()).title.startsWith("Tempo from the start: 124 BPM")), "and before it, the start");
await undo();
await undo();
check(await until(() => read("data/arrangement.json") === orig.arrangement), "two undos take them out");

// An edit goes to the tempo it began on, wherever the playhead is when it ends: a drag, and a typed
// tempo, begun before bar 5's 140 and finished after the playhead crossed it.
await post("/tempo", { session, at: 16, bpm: 140, add: true });
await post("/seek", { beat: 14 });
await until(async () => (await field()).bpm === 124);
await dragTempo(24, {
  during: async () => {
    await post("/seek", { beat: 20 });
    await until(async () => at((await status()).beat, 20));
    await sleep(300);
  },
});
check(await until(() => same(tempos(), [[0, 130], [16, 140]])), `a drag sets the tempo it began on: ${JSON.stringify(tempos())}`);
await post("/seek", { beat: 14 });
await until(async () => (await field()).bpm === 130);
const tb = await box("[data-testid=tempo-field]");
await page.mouse.click(tb.x + tb.width / 2, tb.y + tb.height / 2, { count: 2 });
await page.waitForSelector("[data-testid=tempo-field-input]", { timeout: 2000 }).catch(() => null);
await post("/seek", { beat: 20 });
await until(async () => at((await status()).beat, 20));
await sleep(300);
await page.$eval("[data-testid=tempo-field-input]", (e) => e.select()).catch(() => {});
await page.keyboard.type("128");
await page.keyboard.press("Enter");
check(await until(() => same(tempos(), [[0, 128], [16, 140]])), `and so does a typed one: ${JSON.stringify(tempos())}`);
check(await until(async () => (await field()).bpm === 140), "the field shows the tempo at the playhead again");
for (let i = 0; i < 3; i++) await undo();
check(await until(() => read("data/arrangement.json") === orig.arrangement), "three undos put the song back");

// ---------------------------------------------------------------- the song's name (design-8b §4.4)

const name = await box("[data-testid=bar-song-name]");
await page.mouse.click(name.x + name.width / 2, name.y + name.height / 2, { count: 2 });
const titled = await page.waitForSelector("[data-testid=song-title-input]", { timeout: 2000 }).catch(() => null);
check(!!titled && (await page.$eval("[data-testid=song-title-input]", (e) => e.value)) === "first-song", "a double-click on the name opens it to type");
// It opens with the whole name selected.
await page.keyboard.press("Backspace");
await page.keyboard.press("Enter");
const empty = await page.$eval("[data-testid=song-title-input] + [role=alert]", (e) => e.textContent).catch(() => null);
check(empty === "A song needs a name" && read("music2.toml") === orig.manifest, `an empty name is refused in words: ${empty}`);
await page.keyboard.type("Night Drive");
await page.keyboard.press("Enter");
check(await until(() => /^name = "Night Drive"$/m.test(read("music2.toml"))), "Enter writes it into music2.toml");
check(read("music2.toml") === orig.manifest.replace('name = "first-song"', 'name = "Night Drive"'), "and nothing else");
check(await until(async () => (await page.$eval("[data-testid=bar-song-name]", (e) => e.textContent).catch(() => "")) === "Night Drive"), "the bar shows it");
check(await until(() => page.evaluate(() => document.title === "Night Drive")), "and the window's title follows");
await sleep(300);
check((await toasts()).length === 0, `with no toast: ${await toasts()}`);
await undo();
check(await until(() => read("music2.toml") === orig.manifest), "undo puts the name back");

// In the Mac app the bar starts 80 px in (taste.md §3.8): the field keeps clear of the tempo and the
// buttons, as the name does. Laid out as the app's (`data-app`), at its smallest window and around
// where the time shows.
await page.evaluate(() => (document.documentElement.dataset.app = "mac"));
for (const [w, h] of [[1100, 700], [1180, 800], [1280, 800]]) {
  await page.setViewport({ width: w, height: h });
  await sleep(300);
  const n = await box("[data-testid=bar-song-name]");
  await page.mouse.click(n.x + n.width / 2, n.y + n.height / 2, { count: 2 });
  await page.waitForSelector("[data-testid=song-title-input]", { timeout: 2000 }).catch(() => null);
  await page.keyboard.type(" and a much longer name than fits");
  const b = await page.evaluate(() => {
    const r = (s) => {
      const b = document.querySelector(s)?.getBoundingClientRect();
      return b && { left: b.left, right: b.right, width: b.width };
    };
    const time = r("span[title=Time]");
    return { field: r("[data-testid=song-title-input]"), left: Math.max(r("[data-testid=tempo]").right, time?.width ? time.right : 0), right: r('button[aria-label="Undo"]').left };
  });
  check(!!b.field && b.field.left >= b.left + 4 && b.field.right <= b.right - 4, `${w}px in the Mac app: the name's field (${b.field?.left.toFixed(0)}–${b.field?.right.toFixed(0)}) clears the tempo (…${b.left.toFixed(0)}) and Undo (${b.right.toFixed(0)}…)`);
  if (w === 1100) await page.screenshot({ path: `${ROOT}/s8-songbar-name-1100-mac.png`, clip: { x: 0, y: 0, width: w, height: 44 } });
  await page.keyboard.press("Escape");
  await until(async () => !(await page.$("[data-testid=song-title-input]")));
}
await page.evaluate(() => delete document.documentElement.dataset.app);
await page.setViewport({ width: 1440, height: 900 });
check(read("music2.toml") === orig.manifest, "Escape wrote nothing");
await sleep(300);

// ---------------------------------------------------------------- the transport (taste.md §5.1)

await seekRuler(4);
await key("Space");
check(await until(async () => (await status()).playing), "Space plays");
check(await until(async () => (await status()).beat > 5.5, 4000), "from where the ruler put the playhead");
const playhead = () => page.$eval("[data-testid=playhead]", (e) => Number(e.dataset.beat));
await key("Space");
// The playhead, sampled every frame for 300 ms from the moment of the stop: at 4 at once, never
// running on (the stop and the seek back are what the status says until the engine takes them).
const trail = await page.evaluate(
  () =>
    new Promise((done) => {
      const seen = [], t0 = performance.now();
      const step = () => {
        seen.push(Number(document.querySelector("[data-testid=playhead]").dataset.beat));
        if (performance.now() - t0 < 300) requestAnimationFrame(step);
        else done(seen);
      };
      requestAnimationFrame(step);
    }),
);
const st = await status();
check(!st.playing && at(st.beat, 4) && (await page.$eval("[data-testid=position]", (e) => e.textContent)) === "2.1.1", `Space stops back where play started: beat ${st.beat}`);
// It played past 5.5, so a beat between 4 and 5 is the playhead running on from 4 after the stop:
// what a poll that still read "playing" did, for up to 200 ms, before the stop was said at once.
const ranOn = trail.filter((b) => b > 4.001 && b < 5);
check(trail.some((b) => at(b, 4)) && at(trail.at(-1), 4) && ranOn.length === 0, `and the playhead goes back and stays: ${[...new Set(trail.map((b) => b.toFixed(2)))].join(" ")}`);
await key("Shift+Space");
check(await until(async () => (await status()).playing && (await status()).beat > 6), `Shift+Space continues from where it stopped (${(await status()).beat.toFixed(2)})`);
await sleep(300);
await key("Space");
check(await until(async () => !(await status()).playing && at((await status()).beat, 4)), "Space then stops back at 4 again: the start point stayed");

// A click on empty lane space moves where play starts, while stopped (taste.md §5.1).
const stopped = () => until(() => page.$eval("[data-testid=play]", (e) => e.getAttribute("aria-label") === "Play"));
await stopped();
const kick = await box('[data-lane="kick.hits"] > div:last-child');
await page.mouse.click(kick.x + 40 * 24 + 2, kick.y + kick.height / 2);
check(await until(async () => at((await status()).beat, 40)), `a click on empty lane space, stopped, moves the playhead there: ${(await status()).beat}`);
await key("Space");
await until(async () => (await status()).playing);
await sleep(200);
await page.mouse.click(kick.x + 44 * 24 + 2, kick.y + kick.height / 2);
await sleep(300);
const playingOn = await status();
check(playingOn.playing && playingOn.beat > 40 && playingOn.beat < 43, `while playing it doesn't jump: ${playingOn.beat.toFixed(2)}`);
await key("Space");
check(await until(async () => !(await status()).playing && at((await status()).beat, 40)), "Space stops back at 40");

// ⌘. stops all sound: it calls panic, and playback goes on.
await seekRuler(0);
await key("Space");
await until(async () => (await status()).playing);
await key("Meta+Period");
check(await until(() => sent("panic") === 1), "⌘. calls panic");
await sleep(200);
check((await status()).playing, "and playback goes on");
await key("Space");
await until(async () => !(await status()).playing);

// C and ⌘L.
check((await status()).loop === null, "no loop yet");
await key("c");
check(await until(async () => loopIs((await status()).loop, 0, 16, true)), `C loops the 4 bars from the playhead: ${JSON.stringify((await status()).loop)}`);
await key("c");
check(await until(async () => loopIs((await status()).loop, 0, 16, false)), "C again turns it off, keeping the range");
await page.click('[data-lane="hats.hits"] [data-clip]');
await key("Meta+l");
check(await until(async () => loopIs((await status()).loop, 0, 32, true)), `⌘L loops the selected clip: ${JSON.stringify((await status()).loop)}`);
check(await until(() => { try { return loopIs(JSON.parse(read(".music2/view.json")).loop, 0, 32, true); } catch { return false; } }), "kept in .music2/view.json");
check((await post("/open", { path: PROJ })).status === 200, "the song is opened again");
check(await until(async () => loopIs((await status()).loop, 0, 32, true)), "with its loop");
await page.waitForSelector("[data-testid=tempo-field]");
// ⌘L loops the clip shown as selected, not every track's remembered one: Hats' clip, then a new
// one-bar clip on Kick at bar 10; then, with nothing selected, the bar at the playhead.
await page.click('[data-lane="hats.hits"] [data-clip]');
const kickLane = read("data/kick/hits.json");
const kl = await box('[data-lane="kick.hits"] > div:last-child');
await page.mouse.click(kl.x + 37 * 24 + 2, kl.y + kl.height / 2, { count: 2 });
await until(() => read("data/kick/hits.json") !== kickLane);
await sleep(300);
await key("Meta+l");
check(await until(async () => loopIs((await status()).loop, 36, 40, true)), `⌘L after picking a clip on Hats, then one on Kick, loops Kick's: ${JSON.stringify((await status()).loop)}`);
await undo();
await until(() => read("data/kick/hits.json") === kickLane);
await page.click('[data-lane="hats.hits"] [data-clip]');
await key("Escape");
await key("Escape");
// The double-click on Kick's lane put the playhead at bar 10: the bar it is in, bar 2, is looped.
await seekRuler(6);
await until(async () => (await page.$eval("[data-testid=position]", (e) => e.textContent)) === "2.3.1");
await key("Meta+l");
check(await until(async () => loopIs((await status()).loop, 4, 8, true)), `with nothing selected, the bar at the playhead: ${JSON.stringify((await status()).loop)}`);

// A tempo too fast for the loop turns it off and says so (RENDER-9a).
check((await post("/loop", { start: 0, end: 0.012 })).status === 200, "a loop of 0.012 beats: 5.8 ms at 124 BPM");
await sleep(300);
await dragTempo(80);
check(await until(async () => (await toasts()).some((t) => t.includes("Loop turned off: it's too short to play at 144 BPM"))), `a drag to 144 BPM turns it off, and a toast says so: ${await toasts()}`);
check(loopIs((await status()).loop, 0, 0.012, false) && same(tempos(), [[0, 144]]), "the loop is off, its range kept");
await undo();
await until(() => read("data/arrangement.json") === orig.arrangement);
await page.screenshot({ path: `${ROOT}/s8-songbar.png`, clip: { x: 0, y: 0, width: 1440, height: 44 } });

// ---------------------------------------------------------------- a read-only window (WRITE-17)

check((await post("/close", {})).status === 200, "the song closes");
const holder = spawn("python3", ["-c", `
import fcntl, os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_CREAT)
fcntl.flock(fd, fcntl.LOCK_EX)
os.ftruncate(fd, 0)
os.write(fd, sys.argv[2].replace("PID", str(os.getpid())).encode())
print("ready", flush=True)
sys.stdin.read()
`, `${PROJ}/.music2/lock`, JSON.stringify({ pid: 0, program: "agent", host: "here", host_id: "x", token: "0", since: Math.floor(Date.now() / 1000) }).replace('"pid":0', '"pid":PID')], { stdio: ["pipe", "pipe", "inherit"] });
await new Promise((r) => holder.stdout.once("data", r));
const opened = await post("/open", { path: PROJ });
check(opened.status === 200 && opened.body.read_only?.why === "held", "opened while another program holds it: read-only");
await page.waitForSelector("[data-testid=read-only-banner]", { timeout: 6000 }).catch(() => {});
const ro = await field();
check(ro.disabled && ro.bpm === 124 && ro.title.includes("Read only"), `the tempo shows its value and says why it can't change: ${ro.title.replace(/\n/g, " | ")}`);
const writes = posts.length;
await dragTempo(40);
const rb = await box("[data-testid=tempo-field]");
await page.mouse.click(rb.x + rb.width / 2, rb.y + rb.height / 2, { count: 2 });
await page.click("[data-testid=meter-field]");
const nb = await box("[data-testid=bar-song-name]");
await page.mouse.click(nb.x + nb.width / 2, nb.y + nb.height / 2, { count: 2 });
const rr = await ruler();
await page.mouse.click(rr.x + 8 * 24 + 2, rr.y + rr.height / 2, { button: "right" });
await sleep(400);
const opens = await page.evaluate(() => ["tempo-field-input", "meter-picker", "song-title-input", "ruler-menu", "value-bubble"].filter((id) => document.querySelector(`[data-testid=${id}]`)));
check(opens.length === 0 && posts.slice(writes).every((p) => !/tempo|meter|title/.test(p.url)) && read("data/arrangement.json") === orig.arrangement, `a drag, a double-click, the picker, the name and the ruler's menu don't edit it${opens.length ? `: ${opens}` : ""}`);
holder.stdin.end();
await new Promise((r) => holder.once("exit", r));

done(errors);
