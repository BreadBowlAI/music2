// Bars by the time signatures (FMT-13): the timeline and the roll with a 7/8 (and 100 bpm) from
// bar 5, on a fresh copy of the converted first-song.   node run.mjs s4-meter fix
import { ROOT, check, done, open, sleep, until, write } from "./fixlib.mjs";
const { browser, page, errors, box } = await open();
write("data/arrangement.json", JSON.stringify({ tempo: [{ beat: 0, bpm: 124 }, { beat: 16, bpm: 100 }], meter: [{ beat: 0, beats: 4, unit: 4 }, { beat: 16, beats: 7, unit: 8 }] }));
check(await until(async () => (await page.$$eval("[data-testid=ruler] span", (els) => els.map((e) => e.textContent))).includes("7/8 ♩100")), "an arrangement written from outside is taken in");
const ruler = await page.$$eval("[data-testid=ruler] span", (els) => els.map((e) => [e.textContent, Math.round(parseFloat(e.style.left))]));
check(JSON.stringify(ruler.slice(0, 7)) === JSON.stringify([["1", 6], ["2", 102], ["3", 198], ["4", 294], ["5", 390], ["6", 474], ["7", 558]]), `bar numbers follow the meter: ${JSON.stringify(ruler.slice(0, 8))}`);
check(ruler.some(([t]) => t === "7/8 ♩100"), "the 7/8 and the tempo change share a mark");
await page.screenshot({ path: `${ROOT}/s4-meter.png` });
const c = await box('[data-lane="hats.hits"] [data-clip]');
await page.mouse.click(c.x + 40, c.y + c.height / 2, { count: 2 });
await until(() => page.$("[data-testid=notes]"));
await sleep(300);
const header = await page.$eval("[data-testid=notes] header", (e) => e.textContent);
check(/bars 1–9/.test(header), `the clip's bars, counted by the meter: ${header.slice(0, 40)}`);
const len = () => page.$eval('[data-testid=notes] input[aria-label="Clip length in bars"]', (e) => e.value);
check((await len()) === "8.571", `its length in bars: 4 of 4/4 and 16 beats of 7/8 (${await len()})`);
await page.click('[data-testid=notes] button[aria-label="One bar longer"]');
const fs = await import("node:fs");
const hats = () => JSON.parse(fs.readFileSync(`${process.argv[4]}/data/hats/hits.json`, "utf8")).clips[0].length;
check(await until(() => hats() === 33.5), `one bar longer ends it on the next bar line, beat 33.5 (${hats()})`);
const roll = await page.$$eval("[data-testid=roll-ruler] span", (els) => els.map((e) => e.textContent));
check(JSON.stringify(roll.slice(0, 7)) === JSON.stringify(["1", "2", "3", "4", "5", "6", "7"]), `the roll's ruler: ${roll.slice(0, 8)}`);
await page.mouse.click(400, 700);
await page.screenshot({ path: `${ROOT}/s4-meter-roll.png` });
await browser.close();
done(errors);
