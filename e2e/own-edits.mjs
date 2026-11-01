// The window's own edits (AGENT-2), on a page that hears of each change late: its reads of the song's
// state held back 400 ms (fixlib's `late`), so each gesture below comes while the page still shows the
// state before the window's own last edit. A gesture then waits for it and goes from it: none is
// dropped, none refused. ⌘Z right after the window's own import takes the import back.
//  - one knob turned twice quickly: both written, in order, each its own undo step;
//  - a menu picked right after a knob of the same call: both written, each its own step;
//  - a clip's left edge dragged right after its right edge was trimmed: both written, the second
//    from the first;
//  - ⌘Z right after a MIDI file came in, and right after an audio file did: each taken back;
// and no refusal is shown, nor answered.
// node own-edits.mjs <root> <home> <project>
import { check, done, open, read, until } from "./fixlib.mjs";

const { page, errors, toasts, key, box, view, show } = await open(1440, 900, { late: 400 });
/** Every answer that refused something, as it came. */
const refusals = [];
page.on("response", (r) => r.status() >= 400 && /\/api\//.test(r.url()) && refusals.push(`${r.request().method()} ${r.url().replace(/^.*\/api/, "/api")} ${r.status()}`));
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
/** No answer refused since `n` refusals, and no toast that says one was. */
const quiet = async (n) => refusals.length === n && !(await toasts()).some((t) => /newest|changed|Nothing was|try again/i.test(t));
const said = async () => `toasts: ${JSON.stringify(await toasts())}, refused: ${JSON.stringify(refusals)}`;

/** Drag the knob (or fader) `sel` up by `dy` pixels and let go: nothing waited for. */
const turn = async (sel, dy) => {
  const grip = `${sel} svg, ${sel} [data-testid=fader-thumb]`;
  await page.$eval(grip, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
  const b = await box(grip);
  const cx = b.x + b.width / 2, cy = b.y + b.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  for (let i = 1; i <= 6; i++) await page.mouse.move(cx, cy - (i * dy) / 6);
  await page.mouse.up();
};

// ---------------------------------------------------------------- one knob, twice quickly
await view("mix");
const hatsPan = '[data-testid=mix] [data-knob="hats.pan"]';
await page.waitForSelector(hatsPan);
const pan = () => /hats: \{ in: hats\.out, db: db\(-6\), pan: (-?[\d.]+) \}/.exec(read("song.ts"))?.[1];
const song0 = read("song.ts");
check(pan() === "0.3", `the hats pan starts at 0.3: ${pan()}`);
let n = refusals.length;
await turn(hatsPan, 30);
await turn(hatsPan, 30);
check(await until(() => pan() === "0.7"), `turned twice, 30px up each, before the page shows the first: 0.3 → 0.5 → 0.7 (${pan()})`);
check(await until(async () => (await undoTitle())?.startsWith("Undo Hats pan")), `Undo names the pan: ${await undoTitle()}`);
check(await quiet(n), `nothing refused: ${await said()}`);
await key("Meta+z");
check(await until(() => pan() === "0.5"), `one ⌘Z takes back the second turn only: ${pan()}`);
await key("Meta+z");
check(await until(() => read("song.ts") === song0), `and another the first: ${pan()}`);
check(await quiet(n), `nothing refused: ${await said()}`);

// ---------------------------------------------------------------- a menu right after a knob of its call
await show("kick");
await view("graph");
const drum = '[data-module="kick"] [data-node="drums.drum"]';
const tune = `${drum} [data-knob="tune"]`, sound = `${drum} [data-knob="sound"]`;
await page.waitForSelector(`${tune} svg`);
await page.waitForSelector(sound);
const kick0 = read("tracks/kick.ts");
const call = () => read("tracks/kick.ts").match(/drums\.drum\([^\n]*\);/)?.[0];
n = refusals.length;
await turn(tune, 30);
await page.click(`${sound} [data-testid=setting-menu]`);
await page.waitForSelector('[data-testid=setting-options] [data-option="snare"]');
await page.click('[data-testid=setting-options] [data-option="snare"]');
check(await until(() => /\{ tune: -?[\d.]+, drum: "snare" \}/.test(call() ?? "")), `the tune turned, then snare picked at once: both written (${call()})`);
check(await quiet(n), `nothing refused: ${await said()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => /\{ tune: -?[\d.]+ \}\)/.test(call() ?? "")), `one ⌘Z takes back the pick only: ${call()}`);
await key("Meta+z");
check(await until(() => read("tracks/kick.ts") === kick0), `and another the tune: ${call()}`);
check(await quiet(n), `nothing refused: ${await said()}`);

// ---------------------------------------------------------------- a clip's edge right after a trim
const PPB = 24;
const hats = () => JSON.parse(read("data/hats/hits.json")).clips[0];
const hatsFile = read("data/hats/hits.json");
check(hats().start === 0 && hats().length === 32, `the hats clip starts at 0, 32 beats long: ${hats().start}, ${hats().length}`);
const clip = '[data-lane="hats.hits"] [data-clip]';
const drag = async (from, dx) => {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + dx, from.y, { steps: 8 });
  await page.mouse.up();
};
let b = await box(clip);
n = refusals.length;
await drag({ x: b.x + b.width - 3, y: b.y + b.height / 2 }, -4 * PPB);
// The left edge, pressed while the page still draws the clip as it was: the trim isn't shown yet.
await drag({ x: b.x + 3, y: b.y + b.height / 2 }, 4 * PPB);
check(await until(() => hats().start === 4 && hats().length === 24), `the right edge in by a bar, then at once the left: 4 to 28 (${hats().start} + ${hats().length})`);
check(await quiet(n), `nothing refused: ${await said()}`);
await key("Meta+z");
check(await until(() => hats().start === 0 && hats().length === 28), `one ⌘Z takes back the left edge only: ${hats().start} + ${hats().length}`);
await key("Meta+z");
check(await until(() => read("data/hats/hits.json") === hatsFile), `and another the trim: ${hats().start} + ${hats().length}`);
check(await quiet(n), `nothing refused: ${await said()}`);

// ---------------------------------------------------------------- ⌘Z right after an import
/** Drop a file of `bytes` named `name`, of `type`, at (`x`, `y`), as a browser does. */
const dropFile = (x, y, name, type, bytes) =>
  page.evaluate(
    (x, y, name, type, bytes) => {
      const dt = new DataTransfer();
      dt.items.add(new File([new Uint8Array(bytes)], name, { type }));
      const at = document.elementFromPoint(x, y) ?? document.body;
      const fire = (t) => at.dispatchEvent(new DragEvent(t, { dataTransfer: dt, clientX: x, clientY: y, bubbles: true, cancelable: true }));
      fire("dragenter");
      fire("dragover");
      fire("drop");
    },
    x,
    y,
    name,
    type,
    [...bytes],
  );
/** A one-track MIDI file at 96 ticks a beat: `notes` as [key, start, length] in beats. */
const midi = (notes) => {
  const vlq = (v) => { const out = [v & 0x7f]; while ((v >>= 7)) out.unshift((v & 0x7f) | 0x80); return out; };
  const ev = notes.flatMap(([k, s, l]) => [[Math.round(s * 96), 1, [0x90, k, 100]], [Math.round((s + l) * 96), 0, [0x80, k, 0]]]).sort((a, c) => a[0] - c[0] || a[1] - c[1]);
  const body = [];
  let now = 0;
  for (const [tick, , bytes] of ev) { body.push(...vlq(tick - now), ...bytes); now = tick; }
  body.push(0, 0xff, 0x2f, 0);
  const len = Buffer.alloc(4); len.writeUInt32BE(body.length);
  return Buffer.concat([Buffer.from([0x4d, 0x54, 0x68, 0x64, 0, 0, 0, 6, 0, 0, 0, 1, 0, 96]), Buffer.from("MTrk"), len, Buffer.from(body)]);
};
/** A mono 16-bit WAV at 48 kHz, a second of a quiet 220 Hz sine. */
const wav = () => {
  const rate = 48000, n = rate;
  const w = Buffer.alloc(44 + n * 2);
  w.write("RIFF", 0); w.writeUInt32LE(36 + n * 2, 4); w.write("WAVEfmt ", 8); w.writeUInt32LE(16, 16);
  w.writeUInt16LE(1, 20); w.writeUInt16LE(1, 22); w.writeUInt32LE(rate, 24); w.writeUInt32LE(rate * 2, 28);
  w.writeUInt16LE(2, 32); w.writeUInt16LE(16, 34); w.write("data", 36); w.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) w.writeInt16LE(Math.round(0.25 * Math.sin((2 * Math.PI * 220 * i) / rate) * 32767), 44 + i * 2);
  return w;
};
const bassFile = read("data/bass/notes.json");
const bassRow = await box('[data-lane="bass.notes"]');
const ruler = await box("[data-testid=ruler]");
n = refusals.length;
let answered = page.waitForResponse((r) => r.url().includes("/api/graph/midifile/import"), { timeout: 8000 });
await dropFile(ruler.x + 8 * PPB + 6, bassRow.y + bassRow.height / 2, "low.mid", "audio/midi", midi([[40, 0, 1], [43, 2, 1]]));
check((await answered).ok() && read("data/bass/notes.json") !== bassFile, "a MIDI file dropped on Bass comes in");
await key("Meta+z");
check(await until(() => read("data/bass/notes.json") === bassFile), "and ⌘Z at once, before the page shows it, takes it back");
check(await quiet(n), `nothing refused: ${await said()}`);

const songAt = read("song.ts");
const rows = await page.$$eval("[data-track]", (els) => els.filter((e) => e.dataset.track !== "song").map((e) => e.getBoundingClientRect().bottom));
n = refusals.length;
answered = page.waitForResponse((r) => r.url().includes("/api/graph/audio/import"), { timeout: 8000 });
await dropFile(ruler.x + 8 * PPB + 6, Math.max(...rows) + 20, "tone.wav", "audio/wav", wav());
check((await answered).ok() && read("song.ts") !== songAt, "a WAV dropped below the tracks comes in on a new track");
await key("Meta+z");
check(await until(() => read("song.ts") === songAt), "and ⌘Z at once takes the track back");
check(await quiet(n), `nothing refused: ${await said()}`);

done(errors);
