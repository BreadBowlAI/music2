// The knob gestures of taste.md §5.2 (KN-1) and the way values read, on a fresh copy of
// examples/first-song: the Mix view's strips (hats −6 dB and R 30, keys −5 dB and L 25: a pan is a
// knob, a level a fader, and both take the same keys and type-in) and the Bass.
//   words    values read as taste.md's words and numbers: "−6.0 dB", "R 30", "L 25", "2.5 kHz"
//   drag     300px is a knob's whole range; ⇧ makes a drag a tenth as coarse, and ⇧ pressed mid-drag
//            goes on from where the knob is, without a jump
//   reset    ⌥-click puts a knob back at its declared default as one edit; so does ⌫ on a focused knob
//   type     a double-click (or Return on a focused knob) turns the readout into a field of its size:
//            "-3" writes −3 dB, "L20" a pan of −0.2, "2k" 2 kHz on a Hz knob; text that isn't a value
//            marks the field and writes nothing; Escape writes nothing; Tab writes and moves to the next
//            knob or fader
//   step     ↑ on a focused knob is 0.1 dB, ⇧↑ 1 dB, the wheel a step a notch; a run of steps is one edit
//   node run.mjs gestures fix
import { check, done, open, read, sleep, until } from "./fixlib.mjs";

const { browser, page, errors, state, caughtUp, show, view, box } = await open(1440, 900);

const revision = async () => (await state()).view.revision;
const knobIn = (module, label) => `[data-module="${module}"] [data-knob="${label}"]`;
/** A level or a pan on a Mix strip. */
const onStrip = (label) => `[data-testid=mix] [data-knob="${label}"]`;
/** What a knob reads, and the value it holds. */
const reads = (sel) => page.$eval(sel, (e) => e.querySelector(".tabular-nums")?.textContent ?? null);
const valueOf = async (sel) => Number(await page.$eval(sel, (e) => e.dataset.value));
/** A knob's circle (a fader's thumb), scrolled into view: its centre. */
const grip = (sel) => `${sel} svg, ${sel} [data-testid=fader-thumb]`;
const centre = async (sel) => {
  await page.$eval(grip(sel), (e) => e.scrollIntoView({ block: "center", inline: "center" }));
  const b = await box(grip(sel));
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
};
/**
 * Until a control takes a press again: while its last value is being written it is disabled; and
 * until the page shows the revision that wrote it, as a gesture goes from the revision the page
 * shows (the mixer is one call: a change to any of its knobs changes it).
 */
const ready = async (sel) => (await until(async () => (await page.$eval(sel, (e) => e.getAttribute("aria-disabled"))) !== "true")) && caughtUp();
/** Drag a knob up by `dy` pixels (down if negative), holding ⇧ from `fineFrom` pixels on. */
const drag = async (sel, dy, { fineFrom = Infinity } = {}) => {
  await ready(sel);
  const c = await centre(sel);
  await page.mouse.move(c.x, c.y);
  await page.mouse.down();
  const steps = 12;
  let fine = false;
  for (let i = 1; i <= steps; i++) {
    const d = (dy * i) / steps;
    if (!fine && Math.abs(d) > fineFrom) {
      await page.keyboard.down("Shift");
      fine = true;
    }
    await page.mouse.move(c.x, c.y - d);
  }
  await page.mouse.up();
  if (fine) await page.keyboard.up("Shift");
};
const song = () => read("song.ts");
const hatsLine = () => /hats: \{ in: hats\.out, db: db\((-?[\d.]+)\), pan: (-?[\d.]+) \}/.exec(song());
const keysLine = () => /keys: \{ in: keys\.out, db: db\((-?[\d.]+)\), pan: (-?[\d.]+) \}/.exec(song());

await view("mix");
const hatsDb = onStrip("hats.db"), hatsPan = onStrip("hats.pan");
const keysDb = onStrip("keys.db"), keysPan = onStrip("keys.pan");
await page.waitForSelector(hatsPan);

// ---------------------------------------------------------------- words
const words = [await reads(hatsDb), await reads(hatsPan), await reads(keysDb), await reads(keysPan)];
check(JSON.stringify(words) === JSON.stringify(["−6.0 dB", "R 30", "−5.0 dB", "L 25"]), `dB with one decimal and a true minus, pans as L and R: ${words.join(", ")}`);

// ---------------------------------------------------------------- drag
let rev = await revision();
await drag(hatsPan, 30);
check(await until(() => hatsLine()?.[2] === "0.5"), `30px up of a knob's 300 moves the pan a tenth of its range: 0.3 → 0.5 (${hatsLine()?.[2]})`);
// The file is written before its revision is published: the page's write answered first.
await caughtUp();
check((await revision()) === rev + 1, "as one edit");
check(await until(async () => (await reads(hatsPan)) === "R 50"), "and it reads R 50");
await drag(hatsPan, 30, { fineFrom: 0 });
check(await until(() => hatsLine()?.[2] === "0.52"), `with ⇧, the same 30px moves it a tenth as far: 0.5 → 0.52 (${hatsLine()?.[2]})`);
await drag(hatsPan, -45, { fineFrom: 15 });
// 15px coarse (−0.1), then ⇧ for 30px more (−0.02), from where the knob was when ⇧ went down.
check(await until(() => hatsLine()?.[2] === "0.4"), `⇧ pressed mid-drag goes on finely from there, with no jump: 0.52 → 0.4 (${hatsLine()?.[2]})`);
// The knob's whole place drags it, its value too, not only the circle (taste.md §2.2).
await ready(hatsPan);
const name = await page.$eval(`${hatsPan} .tabular-nums`, (e) => { const r = e.getBoundingClientRect(); return { x: r.left + 4, y: r.top + r.height / 2 }; });
await page.mouse.move(name.x, name.y);
await page.mouse.down();
for (let i = 1; i <= 6; i++) await page.mouse.move(name.x, name.y + i * 2.5);
await page.mouse.up();
check(await until(() => hatsLine()?.[2] === "0.3"), `a drag on its value turns it too: 15px down, 0.4 → 0.3 (${hatsLine()?.[2]})`);

// ---------------------------------------------------------------- reset
await caughtUp();
rev = await revision();
let c = await centre(hatsPan);
await page.keyboard.down("Alt");
await page.mouse.click(c.x, c.y);
await page.keyboard.up("Alt");
check(await until(() => hatsLine()?.[2] === "0"), `⌥-click puts the pan back at its default, centre (${hatsLine()?.[2]})`);
check(await until(async () => (await revision()) === rev + 1 && (await reads(hatsPan)) === "C"), "as one edit, and it reads C");

// ---------------------------------------------------------------- type
await caughtUp();
rev = await revision();
c = await centre(keysDb);
await page.mouse.click(c.x, c.y, { count: 2 });
const field = await page.waitForSelector(`${keysDb} [data-testid=knob-type-in]`, { timeout: 2000 }).catch(() => null);
check(!!field, "a double-click on a fader turns its level into a field");
const selected = field && (await field.evaluate((e) => e.value.slice(e.selectionStart, e.selectionEnd)));
check(selected === "−5.0 dB", `its text, selected (${selected})`);
await page.keyboard.type("-3");
await page.keyboard.press("Enter");
check(await until(() => keysLine()?.[1] === "-3"), `"-3" and Return writes db(-3) (${keysLine()?.[0]})`);
check(!(await page.$(`${keysDb} [data-testid=knob-type-in]`)) && (await until(async () => (await reads(keysDb)) === "−3.0 dB")), "and the field gives way to the readout");
await caughtUp();
check((await revision()) === rev + 1, "one edit");

// Text that isn't a value: the field says so and stays; nothing is written. Escape leaves it.
rev = await revision();
await page.$eval(`${keysPan} .tabular-nums`, (e) => e.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
await page.waitForSelector(`${keysPan} [data-testid=knob-type-in]`, { timeout: 2000 });
await page.keyboard.type("loud");
await page.keyboard.press("Enter");
await sleep(400);
const bad = await page.$eval(`${keysPan} [data-testid=knob-type-in]`, (e) => e.getAttribute("aria-invalid")).catch(() => null);
check(bad === "true" && !!(await page.$(`${keysPan} [role=alert]`)), "a double-click on the readout too; text that isn't a value marks the field, with a line saying what to type");
await page.keyboard.press("Escape");
await sleep(400);
check(!(await page.$(`${keysPan} [data-testid=knob-type-in]`)) && (await revision()) === rev && keysLine()?.[2] === "-0.25", "Escape closes it, and nothing is written");

// A pan typed as L20.
c = await centre(keysPan);
await page.mouse.click(c.x, c.y, { count: 2 });
await page.waitForSelector(`${keysPan} [data-testid=knob-type-in]`, { timeout: 2000 });
await page.keyboard.type("L20");
await page.keyboard.press("Enter");
check(await until(() => keysLine()?.[2] === "-0.2"), `"L20" writes a pan of -0.2 (${keysLine()?.[2]})`);

// Tab writes and moves on to the next knob.
await caughtUp();
c = await centre(hatsDb);
await page.mouse.click(c.x, c.y, { count: 2 });
await page.waitForSelector(`${hatsDb} [data-testid=knob-type-in]`, { timeout: 2000 });
await page.keyboard.type("-9db");
await page.keyboard.press("Tab");
check(await until(() => hatsLine()?.[1] === "-9"), `"-9db" and Tab writes db(-9) (${hatsLine()?.[1]})`);
// Hats' pan comes before its fader; the next control is the next strip's pan.
check(
  await until(async () => (await page.evaluate(() => document.activeElement?.closest("[data-knob]")?.dataset.knob)) === "bass.pan"),
  `and the next control, bass.pan, has the focus (${await page.evaluate(() => document.activeElement?.closest("[data-knob]")?.dataset.knob)})`,
);

// Return on a focused knob types too: "2k" on a Hz knob.
await view("graph");
await show("bass");
const hi = knobIn("bass", "hi");
check((await reads(hi)) === "2.5 kHz", `the bass's xrange hi reads 2.5 kHz (${await reads(hi)})`);
await page.$eval(`${hi} svg`, (e) => e.focus());
await page.keyboard.press("Enter");
await page.waitForSelector(`${hi} [data-testid=knob-type-in]`, { timeout: 2000 });
await page.keyboard.type("2k");
await page.keyboard.press("Enter");
check(await until(async () => (await valueOf(hi)) === 2000), `Return, "2k" and Return set it to 2000 Hz (${await valueOf(hi)})`);
check(await until(async () => (await reads(hi)) === "2.0 kHz"), `and it reads 2.0 kHz (${await reads(hi)})`);
// (The knob shows a value before its edit is written: wait for the song to have it.)
await until(async () => (await state()).view.modules.find((m) => m.key === "bass").nodes.some((n) => n.knobs.some((k) => k.label === "hi" && k.value === 2000)));

// ---------------------------------------------------------------- step
await view("mix");
await page.waitForSelector(keysDb);
await caughtUp();
rev = await revision();
await page.$eval(grip(keysDb), (e) => e.focus());
for (let i = 0; i < 3; i++) await page.keyboard.press("ArrowUp");
check(await until(() => keysLine()?.[1] === "-2.7"), `↑ three times on a focused fader: −3 → −2.7 dB (${keysLine()?.[1]})`);
await sleep(800);
check((await revision()) === rev + 1, "written once, when the steps stop");
await caughtUp();
await page.keyboard.down("Shift");
await page.keyboard.press("ArrowUp");
await page.keyboard.up("Shift");
check(await until(() => keysLine()?.[1] === "-1.7"), `⇧↑ is ten steps: 1 dB (${keysLine()?.[1]})`);
await sleep(800);
await caughtUp();
rev = await revision();
await page.keyboard.press("Backspace");
check(await until(() => keysLine()?.[1] === "0"), `⌫ on a focused fader resets it to its default, 0 dB (${keysLine()?.[1]})`);
await caughtUp();
check((await revision()) === rev + 1, "as one edit");

rev = await revision();
const before = Number(hatsLine()?.[2]);
c = await centre(hatsPan);
await page.mouse.move(c.x, c.y);
await page.mouse.wheel({ deltaY: -80 });
check(await until(() => Math.abs(Number(hatsLine()?.[2]) - (before + 0.04)) < 1e-9), `the wheel steps it, a step (1% of its range) a notch: ${before} → ${hatsLine()?.[2]}`);
await sleep(800);
await caughtUp();
check((await revision()) === rev + 1, "two notches, one edit");

await browser.close();
done(errors);
