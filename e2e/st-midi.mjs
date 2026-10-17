// MIDI files (studio lane C, Q36, Q37, Q38): the clip menu's Export as MIDI… writes the clip's
// notes to a MIDI file in the songs folder's Exports (a browser page: no save panel), which reads
// back to the clip's notes, with a free name for the next; only one notes clip offers it. A
// one-part MIDI file brought into Bass's notes lane (as a browser uploads it) is one clip and one
// undo step named "importing riff.mid"; a text file named .mid is refused in words.
// node st-midi.mjs <root> <home> <project>
import fs from "node:fs";
import { check, done, HOME, open, read, ROOT, sess, until } from "./fixlib.mjs";
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

/** A Standard MIDI File, format 1 at 96 ticks a beat: each track `{name, notes: [[key, start, length, velocity, channel]]}` (beats). */
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

done(errors);
