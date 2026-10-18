// Track sx's Follow tempo (the sx plan, D11), in the app. An audio clip's menu has a check row
// "Follow tempo" after Reverse, with its plain hint under it; a notes clip's has none. Turned on, it
// is one undo step ("Undo Follow tempo") that gives the clip warp markers at its start and end, and
// its waveform stays as it was. The tempo halved, the followed clip's waveform still stays: each beat
// shows the same moment of its file. Turned off at that tempo, it plays at its own speed again, and
// its waveform shows more of the file. ⌘Z three times takes all of it back. Each state linted.
//
//   node e2e/sx-follow.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, check, done, open, read, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

/** A mono 16-bit WAV at 48 kHz of `seconds`, sample `i` from `f(i / rate)` (−1..1), written as `name` in `dir`. */
function wav(dir, name, seconds, f, rate = 48000) {
  const n = Math.round(seconds * rate);
  const b = Buffer.alloc(44 + n * 2);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(rate, 24);
  b.writeUInt32LE(rate * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(Math.max(-1, Math.min(1, f(i / rate))) * 32767), 44 + i * 2);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(`${dir}/${name}`, b);
}

const { page, errors, key, api, state, box } = await open(1440, 900);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const stepIs = async (want) => until(async () => (await undoTitle()) === `Undo ${want} (⌘Z)`);

// ---------------------------------------------------------------- the song: a Tone track with a swell

// At 124 BPM, 8 beats are 3.87 s of an 8 s swell: its level rises from 0 to full.
wav(`${PROJ}/assets`, "swell.wav", 8, (t) => (t / 8) * Math.sin(2 * Math.PI * 330 * t));
write("tracks/tone.ts", 'import { track } from "@m2/core";\n\nexport default track("tone", { name: "Tone" }, ({ lane }) => lane.audio("clips"));\n');
fs.mkdirSync(`${PROJ}/data/tone`, { recursive: true });
write("data/tone/clips.json", JSON.stringify({ clips: [{ id: "swell", start: 0, length: 8, asset: "swell.wav" }] }, null, 2));
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport tone from "./tracks/tone";').replace("      kick: kick.out,", "      kick: kick.out,\n      tone: tone.out,"));

const clip = '[data-lane="tone.clips"] [data-clip="swell"]';
check(!!(await until(() => page.$(`${clip} canvas`), 20000)), "the swell's clip draws its waveform");
const warp = () => JSON.parse(read("data/tone/clips.json")).clips[0].warp ?? [];

/** How much ink each column of the clip's waveform has. */
const columns = () =>
  page.$eval(clip, (el) => {
    const out = [];
    for (const c of el.querySelectorAll("canvas")) {
      const { data, width, height } = c.getContext("2d").getImageData(0, 0, c.width, c.height);
      for (let x = 0; x < width; x++) {
        let ink = 0;
        for (let y = 0; y < height; y++) if (data[(y * width + x) * 4 + 3] > 0) ink++;
        out.push(ink);
      }
    }
    return out;
  }).catch(() => []);
/** The mean difference of two waveforms' columns, in px. */
const apart = (a, b) => (a.length && a.length === b.length ? a.reduce((s, x, i) => s + Math.abs(x - b[i]), 0) / a.length : Infinity);
/** Wait until the waveform is `near` (or, false, not near) `want`: what it is then. */
const drawn = async (want, near) => {
  let now = [];
  await until(async () => ((now = await columns()), (apart(now, want) < 1) === near), 6000);
  return now;
};

/** Open the clip menu on `sel`: its rows, each [id, aria-checked, its words]. */
async function menu(sel) {
  const r = await box(sel);
  await page.mouse.click(r.x + r.width / 2, r.y + r.height - 6, { button: "right" });
  await page.waitForSelector("[data-testid=clip-menu] [data-menu-item]", { timeout: 3000 }).catch(() => null);
  return page.$$eval("[data-testid=clip-menu] [data-menu-item]", (els) => els.map((e) => [e.dataset.menuItem, e.getAttribute("aria-checked"), e.textContent]));
}
const pick = async (id) => page.click(`[data-testid=clip-menu] [data-menu-item=${id}]`);

// ---------------------------------------------------------------- the row

const rows = await menu(clip);
const at = (id) => rows.findIndex((r) => r[0] === id);
check(at("follow") === at("reverse") + 1, `Follow tempo comes after Reverse: ${rows.map((r) => r[0])}`);
const row = rows[at("follow")];
check(row?.[1] === "false", `it is a check row, off: ${row?.[1]}`);
check(row?.[2] === "Follow tempoStretch this clip with the song's tempo. Its pitch stays the same.", `its words and plain hint: ${row?.[2]}`);
await linted("[data-testid=clip-menu]", "the audio clip's menu with Follow tempo");
await page.keyboard.press("Escape");
const kick = await menu('[data-lane="kick.hits"] [data-clip]');
check(kick.length > 0 && !kick.some((r) => r[0] === "follow"), `a notes clip's menu has no Follow tempo: ${kick.map((r) => r[0])}`);
await page.keyboard.press("Escape");

// ---------------------------------------------------------------- on, a slower tempo, off, undo

const before = await columns();
check(before.some((x) => x > 0), "the waveform has ink");
await menu(clip);
await pick("follow");
check(await until(() => warp().length === 2), `on: warp markers at its start and end: ${JSON.stringify(warp())}`);
const [w0, w1] = warp();
check(w0?.[0] === 0 && w0?.[1] === 0 && Math.abs(w1?.[0] - (8 * 60) / 124) < 1e-9 && w1?.[1] === 8, `each the moment of the file it plays now: ${JSON.stringify(warp())}`);
check(await stepIs("Follow tempo"), `one undo step: ${await undoTitle()}`);
check((await menu(clip))[at("follow")]?.[1] === "true", "the row is checked");
await page.keyboard.press("Escape");
check(apart(await drawn(before, true), before) < 1, "its waveform is as it was");

const session = (await state()).session;
check((await post("/tempo", { session, at: 0, bpm: 62 })).status === 200, "the tempo goes to 62 BPM");
check(await until(async () => (await state()).timeline?.tempo?.[0]?.bpm === 62), "the app has it");
check(apart(await drawn(before, true), before) < 1 && warp().length === 2, "followed, its waveform still shows each beat's moment of the file");
await linted(clip, "a followed clip after a tempo change");

await menu(clip);
await pick("follow");
check(await until(() => warp().length === 0), "off: no markers");
check(await stepIs("Follow tempo off"), `named: ${await undoTitle()}`);
const own = await drawn(before, false);
check(apart(own, before) >= 1, `at its own speed, its 8 beats show more of the file (${apart(own, before).toFixed(1)} px apart)`);

for (let i = 0; i < 3; i++) await key("Meta+z");
check(await until(async () => warp().length === 0 && (await state()).timeline?.tempo?.[0]?.bpm === 124), "⌘Z three times: off, the tempo and on, taken back");
check(apart(await drawn(before, true), before) < 1, "the waveform is as it was");

done(errors);
