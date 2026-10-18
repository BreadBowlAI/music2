// MIDI files (studio lane C, Q32 to Q38): the clip menu's Export as MIDI… writes the clip's
// notes to a MIDI file in the songs folder's Exports (a browser page: no save panel), which reads
// back to the clip's notes, with a free name for the next; only one notes clip offers it. A
// one-part MIDI file brought into Bass's notes lane (as a browser uploads it) is one clip and one
// undo step named "importing riff.mid"; a text file named .mid is refused in words.
// C2b: a three-part file dropped below the tracks asks the tempo question (the song has clips);
// Return keeps 124 and makes three tracks, one Undo takes them all back; "Use 128" writes the
// file's tempo from that bar; Esc writes nothing. A one-part file dropped on Bass goes into it, with
// no question; a text file named .mid says its words. Then, in an empty song, File ▸ Import MIDI…
// through the app's file chooser: Drums plays the Drum kit and Low the Synth bass, the song takes
// the file's 128, and one Undo takes it all back.
// node st-midi.mjs <root> <home> <project>
import fs from "node:fs";
import { check, done, HOME, open, read, ROOT, sess, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const exportsDir = `${HOME}/Music/music2/Exports`;

/** A Standard MIDI File's ticks a beat and its tracks: each one's name and notes (key, start, end, velocity, channel), Ons and Offs paired. */
function readSmf(file) {
  const b = fs.readFileSync(file);
  if (b.toString("latin1", 0, 4) !== "MThd") throw new Error(`${file} isn't a MIDI file`);
  const ppq = b.readUInt16BE(12);
  const tracks = [];
  let at = 8 + b.readUInt32BE(4);
  while (at + 8 <= b.length) {
    const id = b.toString("latin1", at, at + 4), len = b.readUInt32BE(at + 4);
    const end = at + 8 + len;
    if (id === "MTrk") {
      let p = at + 8, tick = 0, status = 0, name = null;
      const notes = [], open = new Map();
      const vlq = () => { let v = 0, c; do { c = b[p++]; v = (v << 7) | (c & 0x7f); } while (c & 0x80); return v; };
      while (p < end) {
        tick += vlq();
        if (b[p] & 0x80) status = b[p++];
        if (status === 0xff) {
          const type = b[p++], n = vlq();
          if (type === 0x03 && name === null) name = b.toString("utf8", p, p + n);
          p += n;
          if (type === 0x2f) break;
        } else if (status === 0xf0 || status === 0xf7) {
          p += vlq();
        } else {
          const hi = status & 0xf0, ch = status & 0x0f, a = b[p++], c = hi === 0xc0 || hi === 0xd0 ? 0 : b[p++];
          const k = `${ch}:${a}`;
          if (hi === 0x90 && c > 0) open.set(k, { key: a, start: tick, velocity: c, channel: ch });
          else if ((hi === 0x80 || hi === 0x90) && open.has(k)) {
            notes.push({ ...open.get(k), end: tick });
            open.delete(k);
          }
        }
      }
      notes.sort((x, y) => x.start - y.start || x.key - y.key);
      tracks.push({ name, notes });
    }
    at = end;
  }
  return { ppq, tracks };
}

/** A pitch as a MIDI key: 60, "A1", "C#4". */
const NAMES = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
const keyOf = (p) => {
  if (typeof p === "number") return p;
  const m = /^([A-G])(#|b)?(-?\d+)$/.exec(p);
  return (Number(m[3]) + 1) * 12 + NAMES[m[1]] + (m[2] === "#" ? 1 : m[2] === "b" ? -1 : 0);
};

/** A Standard MIDI File, format 1 at 96 ticks a beat: each track `{name, bpm?, program?: [channel, program], notes: [[key, start, length, velocity, channel]]}` (beats). */
function smfBytes(tracks) {
  const vlq = (v) => { const out = [v & 0x7f]; while ((v >>= 7)) out.unshift((v & 0x7f) | 0x80); return out; };
  const chunks = [Buffer.from([0x4d, 0x54, 0x68, 0x64, 0, 0, 0, 6, 0, 1, 0, tracks.length, 0, 96])];
  for (const t of tracks) {
    const ev = [];
    for (const [k, start, length, v = 100, ch = 0] of t.notes) {
      ev.push([Math.round(start * 96), 1, [0x90 | ch, k, v]], [Math.round((start + length) * 96), 0, [0x80 | ch, k, 0]]);
    }
    ev.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    const body = t.name ? [0, 0xff, 3, ...vlq(t.name.length), ...Buffer.from(t.name)] : [];
    if (t.bpm) {
      const us = Math.round(60_000_000 / t.bpm);
      body.push(0, 0xff, 0x51, 3, (us >> 16) & 0xff, (us >> 8) & 0xff, us & 0xff);
    }
    if (t.program) body.push(0, 0xc0 | t.program[0], t.program[1]);
    let now = 0;
    for (const [tick, , bytes] of ev) { body.push(...vlq(tick - now), ...bytes); now = tick; }
    body.push(0, 0xff, 0x2f, 0);
    const len = Buffer.alloc(4); len.writeUInt32BE(body.length);
    chunks.push(Buffer.from("MTrk"), len, Buffer.from(body));
  }
  return Buffer.concat(chunks);
}

const { page, errors, toasts, box, key } = await open(1440, 900);
const menuIds = () => page.$$eval("[data-testid=clip-menu] [data-menu-item]", (els) => els.map((e) => e.dataset.menuItem));
const rightClick = async (sel, dx = 40) => {
  const b = await box(sel);
  await page.mouse.click(b.x + dx, b.y + b.height / 2, { button: "right" });
  await page.waitForSelector("[data-testid=clip-menu]");
};

// ---------------------------------------------------------------- Export as MIDI… on a notes clip
const bass = JSON.parse(read("data/bass/notes.json")).clips[0];
await rightClick('[data-lane="bass.notes"] [data-clip]');
const ids = await menuIds();
check(ids.indexOf("export-midi") === ids.indexOf("repeat") + 1, `a notes clip's menu has Export as MIDI… after Repeat…: ${ids.join(", ")}`);
const row = await page.$eval('[data-testid=clip-menu] [data-menu-item="export-midi"]', (e) => ({ text: e.textContent, off: e.disabled }));
check(row.text === "Export as MIDI…" && !row.off, `in those words, on: ${JSON.stringify(row)}`);
const bad = await page.evaluate(lint, "[data-testid=clip-menu]");
check(bad.length === 0, `the menu passes the taste lint: ${bad.slice(0, 3).join("; ")}`);
check(!fs.existsSync(`${exportsDir}/first-song Bass.mid`), "no file before");
await page.click('[data-testid=clip-menu] [data-menu-item="export-midi"]');
const file = `${exportsDir}/first-song Bass.mid`;
check(await until(() => fs.existsSync(file)), "a browser page writes it straight to Exports, named by the song and the track");
check(await until(async () => (await toasts()).some((t) => t.includes("Saved Bass as a MIDI file: first-song Bass.mid, in ~/Music/music2/Exports."))), `a toast says where: ${(await toasts()).join(" | ")}`);
const smf = readSmf(file);
check(smf.ppq === 960 && smf.tracks.length === 2 && smf.tracks[1].name === "Bass", `format 1 at 960 a beat, a tempo track and Bass: ${smf.tracks.map((t) => t.name).join(", ")}`);
// The clip's notes, from its start, as they play: inside the clip only.
const want = bass.notes
  .filter((n) => n.start < bass.length && n.start + n.length > 0)
  .map((n) => ({ key: keyOf(n.pitch), start: Math.round(Math.max(0, n.start) * 960), end: Math.round(Math.min(bass.length, n.start + n.length) * 960), velocity: Math.round((n.velocity ?? 0.8) * 127) }))
  .sort((x, y) => x.start - y.start || x.key - y.key);
const got = smf.tracks[1].notes.map(({ key, start, end, velocity }) => ({ key, start, end, velocity }));
check(JSON.stringify(got) === JSON.stringify(want), `the file reads back to the clip's ${want.length} notes`);
check(smf.tracks[1].notes.every((n) => n.channel === 0), "on channel 1");

// Again: a new name, never over the first.
await rightClick('[data-lane="bass.notes"] [data-clip]');
await page.click('[data-testid=clip-menu] [data-menu-item="export-midi"]');
check(await until(() => fs.existsSync(`${exportsDir}/first-song Bass 2.mid`)), "a second export takes the next free name");
check(fs.readFileSync(file).equals(fs.readFileSync(`${exportsDir}/first-song Bass 2.mid`)), "with the same bytes");

// ---------------------------------------------------------------- where it isn't offered
await page.click('[data-fold="bass"]');
await until(async () => !!(await page.$('[data-lane="bass.cutoff"] [data-clip]')));
await rightClick('[data-lane="bass.cutoff"] [data-clip]');
check(!(await menuIds()).includes("export-midi"), "an automation clip's menu has no Export as MIDI…");
await key("Escape");
// Two clips picked: the row goes.
const kick = await box('[data-lane="kick.hits"] [data-clip]');
await page.mouse.click(kick.x + 40, kick.y + kick.height / 2);
const hats = await box('[data-lane="hats.hits"] [data-clip]');
await page.keyboard.down("Shift");
await page.mouse.click(hats.x + 40, hats.y + hats.height / 2);
await page.keyboard.up("Shift");
await page.mouse.click(hats.x + 40, hats.y + hats.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
check(!(await menuIds()).includes("export-midi"), `two clips picked: no Export as MIDI…: ${(await menuIds()).join(", ")}`);
await key("Escape");

// ---------------------------------------------------------------- a MIDI file into a notes lane
/** Upload `bytes` as a browser does: the file's bytes, its name and where it goes in the query. */
const upload = (name, bytes, to) =>
  page.evaluate(
    async (token, name, bytes, to) => {
      const q = `session=${(await (await fetch("/api/graph", { headers: { "x-m2-token": token } })).json()).session}&name=${encodeURIComponent(name)}&to=${encodeURIComponent(JSON.stringify(to))}`;
      const r = await fetch(`/api/graph/midifile/import?${q}`, { method: "POST", headers: { "x-m2-token": token, "content-type": "application/octet-stream" }, body: new Uint8Array(bytes) });
      return { status: r.status, body: await r.json().catch(() => ({})) };
    },
    sess.token,
    name,
    [...bytes],
    to,
  );
const to = { kind: "lane", module: "bass", lane: "notes", beat: 32 };
const riff = smfBytes([{ name: "Riff", notes: [[40, 0, 1], [45, 4, 2]] }]);
const clipsAt = (beat) => JSON.parse(read("data/bass/notes.json")).clips.filter((c) => c.start === beat);
const done1 = await upload("riff.mid", riff, to);
check(done1.status === 200 && done1.body.name === "importing riff.mid", `riff.mid comes into Bass's notes at bar 9: ${JSON.stringify(done1.body).slice(0, 160)}`);
const placed = clipsAt(32);
check(placed.length === 1 && placed[0].length === 8 && placed[0].notes.length === 2, `one clip of two bars with its two notes: ${JSON.stringify(placed[0] ?? null).slice(0, 160)}`);
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
check(await until(async () => (await undoTitle()).startsWith("Undo importing riff.mid")), `Undo names it: ${await undoTitle()}`);
await key("Meta+z");
check(await until(() => clipsAt(32).length === 0), "⌘Z takes it back");
const text = await upload("notes.mid", Buffer.from("these are not notes"), to);
check(text.status === 400 && text.body.error === "notes.mid isn't a MIDI file: it doesn't start with MThd.", `a text file named .mid says so: ${JSON.stringify(text.body)}`);

// ---------------------------------------------------------------- C2b: new tracks from a MIDI file
const ppb = 24;
const lintIn = async (pg, sel, what) => {
  const bad = await pg.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
/** Drag `bytes` named `name` over (`x`, `y`) as a browser does, then drop them there; the hint said while over. */
const dropBytes = (x, y, name, bytes) =>
  page.evaluate(
    async (x, y, name, bytes) => {
      const dt = new DataTransfer();
      dt.items.add(new File([new Uint8Array(bytes)], name, { type: "audio/midi" }));
      const at = document.elementFromPoint(x, y) ?? document.body;
      const fire = (type) => at.dispatchEvent(new DragEvent(type, { dataTransfer: dt, clientX: x, clientY: y, bubbles: true, cancelable: true }));
      fire("dragenter");
      fire("dragover");
      await new Promise((r) => setTimeout(r, 120));
      const hint = document.querySelector("[data-testid=drop-hint]")?.textContent ?? null;
      const outline = document.querySelector("[data-testid=drop-outline]")?.dataset.kind ?? null;
      fire("drop");
      return { hint, outline };
    },
    x,
    y,
    name,
    [...bytes],
  );
const groove = smfBytes([
  { name: "Tempo", bpm: 128, notes: [] },
  { name: "Drums", notes: [[36, 0, 0.25, 100, 9], [38, 1, 0.25, 100, 9]] },
  { name: "Low", program: [0, 33], notes: [[40, 0, 1], [43, 2, 1]] },
  { name: "Keys", program: [1, 0], notes: [[72, 0, 2, 90, 1]] },
]);
await page.evaluate(() => document.querySelector("[data-testid=timeline]").scrollTo({ left: 0 }));
await sleep(200);
const ruler = await box("[data-testid=ruler]");
const xAt = (beat) => ruler.x + beat * ppb + 6;
const yBelow = async () => {
  const rows = await page.$$eval("[data-track]", (els) => els.filter((e) => e.dataset.track !== "song").map((e) => e.getBoundingClientRect().bottom));
  return Math.max(...rows) + 20;
};
const tracksNow = () => page.evaluate(async (token) => (await (await fetch("/api/graph", { headers: { "x-m2-token": token } })).json()).timeline.tracks.filter((t) => t.track).map((t) => t.label), sess.token);
const tempo = () => JSON.parse(read("data/arrangement.json")).tempo;
const askShown = () => page.$eval("[data-testid=midi-ask]", (e) => e.textContent).catch(() => null);
const before = await tracksNow();

// Below the tracks at bar 3, in a song with clips: the question, and nothing written yet.
const below = await dropBytes(xAt(9), await yBelow(), "groove.mid", groove);
check(below.hint === "New tracks · bar 3" && below.outline === "below", `the hint below the tracks: ${JSON.stringify(below)}`);
check(await until(async () => (await askShown()) === "groove.mid is at 128 BPM. The song is at 124 BPM.Keep 124: the notes play at your song's speed.CancelUse 128Keep 124"), `the tempo question: ${await askShown()}`);
await lintIn(page, "[data-testid=midi-ask]", "the tempo question");
if (process.env.ST_SHOT) await page.screenshot({ path: `${process.env.ST_SHOT}/midi-ask.png` });
check(JSON.stringify(await tracksNow()) === JSON.stringify(before), "nothing written while it asks");
await key("Enter");
check(await until(async () => (await tracksNow()).length === before.length + 3), `Return keeps 124 and makes three tracks: ${await tracksNow()}`);
check(JSON.stringify((await tracksNow()).slice(-3)) === JSON.stringify(["Drums", "Low", "Keys 2"]) && JSON.stringify(tempo()) === JSON.stringify([{ beat: 0, bpm: 124 }]), `Drums, Low and Keys 2 at the end, the tempo kept: ${await tracksNow()} ${JSON.stringify(tempo())}`);
check(await until(async () => (await toasts()).some((t) => t.startsWith("Imported groove.mid: Drums, Low and Keys 2"))), `a toast says what came in: ${(await toasts()).join(" | ")}`);
check(await until(async () => (await undoTitle()).startsWith("Undo importing groove.mid")), `Undo names it: ${await undoTitle()}`);
await key("Meta+z");
check(await until(async () => JSON.stringify(await tracksNow()) === JSON.stringify(before)), `one ⌘Z takes all three back: ${await tracksNow()}`);
// Use 128: the file's tempo from bar 3, the song's before it.
await dropBytes(xAt(9), await yBelow(), "groove.mid", groove);
await page.waitForSelector("[data-testid=midi-use]");
await page.click("[data-testid=midi-use]");
check(await until(() => JSON.stringify(tempo()) === JSON.stringify([{ beat: 0, bpm: 124 }, { beat: 8, bpm: 128 }])), `"Use 128" writes it from bar 3: ${JSON.stringify(tempo())}`);
await key("Meta+z");
check(await until(() => JSON.stringify(tempo()) === JSON.stringify([{ beat: 0, bpm: 124 }])), "and ⌘Z takes the tempo back with the tracks");
// Esc: nothing. The undo is on disk before the page hears of it, so first wait until the page's
// revision is the server's: read sooner, the undo's own revision landed after the Esc, like a write.
const pageRev = () => page.$eval("[data-revision]", (e) => e.dataset.revision);
const serverRev = () => page.evaluate(async (token) => String((await (await fetch("/api/graph", { headers: { "x-m2-token": token } })).json()).view.revision), sess.token);
await until(async () => (await serverRev()) === (await pageRev()));
const revAsk = await pageRev();
await dropBytes(xAt(9), await yBelow(), "groove.mid", groove);
await page.waitForSelector("[data-testid=midi-ask]");
await key("Escape");
await until(async () => !(await askShown()));
await sleep(300);
check(!(await askShown()) && (await pageRev()) === revAsk && (await serverRev()) === revAsk, `Esc writes nothing: revision ${revAsk}, now ${await pageRev()} (the server's ${await serverRev()})`);
// One part on Bass's notes at bar 3: into it, no question.
const bassRow = await box('[data-lane="bass.notes"]');
const onBass = await dropBytes(xAt(9), bassRow.y + bassRow.height / 2, "low.mid", smfBytes([{ name: "Low", bpm: 128, notes: [[40, 0, 1]] }]));
check(onBass.hint === "Into Bass · bar 3" && onBass.outline === "lane", `the hint on Bass: ${JSON.stringify(onBass)}`);
check(await until(() => clipsAt(8).length === 1) && !(await askShown()), `a one-part file goes into Bass at bar 3, with no question: ${JSON.stringify(clipsAt(8)).slice(0, 120)}`);
check(await until(async () => (await toasts()).some((t) => t.startsWith("Imported low.mid into Bass"))), `its toast: ${(await toasts()).join(" | ")}`);
// A text file named .mid.
await dropBytes(xAt(9), await yBelow(), "notes.mid", Buffer.from("these are not notes"));
check(await until(async () => (await toasts()).includes("Nothing was added to the song: notes.mid isn't a MIDI file: it doesn't start with MThd.")), `a text file named .mid says so: ${(await toasts()).join(" | ")}`);

// ---------------------------------------------------------------- File ▸ Import MIDI… in an empty song (the app)
const empty = `${ROOT}/empty-song`;
fs.cpSync(`${process.env.E2E_REPO ?? new URL("..", import.meta.url).pathname}/templates/empty`, empty, { recursive: true });
fs.rmSync(`${empty}/template.toml`, { force: true });
fs.writeFileSync(`${ROOT}/groove.mid`, groove);
const app = await page.browser().createBrowserContext().then((c) => c.newPage());
await app.setViewport({ width: 1440, height: 900 });
app.on("pageerror", (e) => errors.push(String(e)));
await app.evaluateOnNewDocument(
  (token, picked) => {
    window.__M2__ = { native: true, token, platform: "macos" };
    const callbacks = new Map();
    const menu = [];
    let next = 1;
    window.__TAURI_INTERNALS__ = {
      invoke: async (cmd, args) => {
        if (cmd === "plugin:event|listen" && args?.event === "menu") menu.push(args.handler);
        return cmd === "action" && args?.id === "open_file" ? JSON.stringify([picked]) : null;
      },
      transformCallback: (cb) => {
        callbacks.set(next, cb);
        return next++;
      },
      unregisterCallback: (id) => callbacks.delete(id),
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__menu = (id) => menu.forEach((h) => callbacks.get(h)?.({ event: "menu", id: 0, payload: id }));
  },
  sess.token,
  `${ROOT}/groove.mid`,
);
const opened = await page.evaluate(
  async (token, path) => (await fetch("/api/graph/open", { method: "POST", headers: { "x-m2-token": token, "content-type": "application/json" }, body: JSON.stringify({ path }) })).status,
  sess.token,
  empty,
);
await app.goto(sess.url, { waitUntil: "domcontentloaded" });
check(opened === 200 && !!(await app.waitForSelector("[data-testid=timeline]", { timeout: 10000 }).catch(() => null)), `an empty song opens in an app page: ${opened}`);
await app.evaluate(() => window.__menu("import_midi"));
const emptyTracks = () => !fs.existsSync(`${empty}/tracks`) ? [] : fs.readdirSync(`${empty}/tracks`, { withFileTypes: true }).filter((e) => e.isFile()).map((e) => e.name).sort();
check(await until(() => emptyTracks().length === 3), `File ▸ Import MIDI… makes three tracks: ${emptyTracks()}`);
const drumsTs = fs.readFileSync(`${empty}/tracks/drums.ts`, "utf8"), lowTs = fs.readFileSync(`${empty}/tracks/low.ts`, "utf8");
check(/drums\.kit|drumKit/.test(drumsTs) && /synth\.bass|synthBass/.test(lowTs), `Drums plays the Drum kit and Low the Synth bass: ${drumsTs.split("\n").at(-2)} | ${lowTs.split("\n").at(-2)}`);
const emptyTempo = () => JSON.parse(fs.readFileSync(`${empty}/data/arrangement.json`, "utf8")).tempo;
check(JSON.stringify(emptyTempo()) === JSON.stringify([{ beat: 0, bpm: 128 }]) && !(await app.$("[data-testid=midi-ask]")), `the empty song takes the file's 128, with no question: ${JSON.stringify(emptyTempo())}`);
const lowClips = JSON.parse(fs.readFileSync(`${empty}/data/low/notes.json`, "utf8")).clips;
check(lowClips.length === 1 && lowClips[0].start === 0 && lowClips[0].notes.length === 2, `Low's notes in one clip from bar 1: ${JSON.stringify(lowClips).slice(0, 120)}`);
if (process.env.ST_SHOT) await app.screenshot({ path: `${process.env.ST_SHOT}/midi-tracks.png` });
await app.evaluate(() => window.__menu("undo"));
check(await until(() => emptyTracks().length === 0 && JSON.stringify(emptyTempo()) === JSON.stringify([{ beat: 0, bpm: 120 }])), `one Undo takes the tracks and the tempo back: ${emptyTracks()} ${JSON.stringify(emptyTempo())}`);

done(errors);
