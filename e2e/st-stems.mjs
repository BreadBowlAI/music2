// Stems (studio lane C, Q39, Q40): each track of the song as its own WAV file, in a new folder in
// the songs folder's Exports, named "1 Kick.wav"… in the tracks' order, each the same file `m2
// bounce <song> x --track <key>` writes, byte for byte; a track muted in the song is left out; a
// song that plays plugins is refused in words; one export at a time.
// node st-stems.mjs <root> <home> <project> <m2 cli>
import fs from "node:fs";
import { execFileSync } from "node:child_process";

const [S, H, P, M2] = process.argv.slice(2);
const sess = JSON.parse(fs.readFileSync(`${H}/.music2/session.json`, "utf8"));
const HDR = { "x-m2-token": sess.token };
const api = async (path, body) => {
  const r = await fetch(`${sess.url}/api/${path}`, body === undefined ? { headers: HDR } : { method: "POST", headers: { ...HDR, "content-type": "application/json" }, body: JSON.stringify(body) });
  return { status: r.status, json: await r.json().catch(() => null) };
};
let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const until = async (f, ms = 30000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await sleep(100); } return false; };
const exportsDir = `${H}/Music/music2/Exports`;
const session = async () => (await api("graph")).json.session;
const revision = async () => (await api("graph")).json.view?.revision ?? (await api("graph")).json.revision;

/** A WAV file's `fmt ` and `data` chunks, as bytes. */
function chunks(file) {
  const b = fs.readFileSync(file);
  const out = {};
  for (let off = 12; off + 8 <= b.length; ) {
    const id = b.toString("latin1", off, off + 4), size = b.readUInt32LE(off + 4);
    out[id] = b.subarray(off + 8, off + 8 + size);
    off += 8 + size + (size & 1);
  }
  return out;
}

/** The finished stems in `dir`: once its files stop changing and no part file is left. */
async function finished(dir, count) {
  let last = "";
  return until(() => {
    if (!fs.existsSync(dir)) return false;
    const files = fs.readdirSync(dir);
    const now = files.map((f) => `${f}:${fs.statSync(`${dir}/${f}`).size}`).join();
    const done = files.filter((f) => f.endsWith(".wav") && !f.startsWith(".")).length === count && !files.some((f) => f.endsWith(".part")) && now === last;
    last = now;
    return done;
  });
}

// ---------------------------------------------------------------- the song's stems
const token = await session();
const ok = await api("graph/stems/check", { session: token });
check(ok.status === 200 && ok.json.why === null, `first-song's stems can be made: ${JSON.stringify(ok.json)}`);
const started = await api("graph/stems", { session: token });
const folder = `${exportsDir}/first-song stems`;
check(started.status === 200 && started.json.path === folder, `they go in a new folder in Exports: ${JSON.stringify(started.json)}`);
// While they render, a second export of either kind is refused.
const again = await api("graph/export", {});
check(again.status === 400 && /already being exported/.test(again.json?.error ?? ""), `an export while stems render is refused: ${JSON.stringify(again.json)}`);
check(await finished(folder, 4), "four stems are written");
const files = fs.readdirSync(folder).sort();
check(files.join(" | ") === "1 Kick.wav | 2 Hats.wav | 3 Bass.wav | 4 Keys.wav", `named by number and track, in the tracks' order: ${files.join(" | ")}`);
// Each is `m2 bounce`'s file of its track, on a copy of the song.
const keys = { 1: "kick", 2: "hats", 3: "bass", 4: "keys" };
const named = { 1: "Kick", 2: "Hats", 3: "Bass", 4: "Keys" };
const copyOf = `${S}/bounce-song`;
fs.cpSync(P, copyOf, { recursive: true, filter: (f) => !f.includes("/.music2") });
for (const n of [1, 2, 3, 4]) {
  const stem = `${folder}/${n} ${named[n]}.wav`;
  check(fs.existsSync(stem), `${n} ${named[n]}.wav is there`);
  execFileSync(M2, ["bounce", copyOf, `stem-${keys[n]}`, "--track", keys[n]], { env: { ...process.env, M2_AUDIO: "none", M2_MIDI: "none" }, stdio: "pipe" });
  const bounced = `${copyOf}/assets/stem-${keys[n]}.wav`;
  const [a, b] = [chunks(stem), chunks(bounced)];
  check(a["fmt "].equals(b["fmt "]) && a.data.equals(b.data) && a.data.length > 1000, `${named[n]}'s stem is m2 bounce --track ${keys[n]}'s audio, bit for bit (${a.data.length} bytes of 32-bit float)`);
  check(fs.readFileSync(stem).equals(fs.readFileSync(bounced)), `and the same file, byte for byte`);
}

// ---------------------------------------------------------------- a track muted in the song is left out
const hats = fs.readFileSync(`${P}/tracks/hats.ts`, "utf8");
const before = await revision();
fs.writeFileSync(`${P}/tracks/hats.ts`, hats.replace('{ name: "Hats" }', '{ name: "Hats", muted: true }'));
check(await until(async () => (await revision()) !== before, 10000), "Hats muted in the song (written to its file)");
const second = await api("graph/stems", { session: await session() });
check(second.json?.path === `${exportsDir}/first-song stems 2`, `a second export takes a folder of its own: ${JSON.stringify(second.json)}`);
check(await finished(`${exportsDir}/first-song stems 2`, 3), "three stems");
check(fs.readdirSync(`${exportsDir}/first-song stems 2`).sort().join(" | ") === "1 Kick.wav | 2 Bass.wav | 3 Keys.wav", `Hats is left out, the rest numbered on: ${fs.readdirSync(`${exportsDir}/first-song stems 2`).sort().join(" | ")}`);
fs.writeFileSync(`${P}/tracks/hats.ts`, hats);

// ---------------------------------------------------------------- a song that plays plugins
const keysTs = fs.readFileSync(`${P}/tracks/keys.ts`, "utf8");
const before2 = await revision();
fs.writeFileSync(
  `${P}/tracks/keys.ts`,
  'import { plugin, track } from "@m2/core";\n\nexport default track("keys", { name: "Keys" }, ({ lane }) => plugin("clap:org.surge-synth-team.surge-xt", { notes: lane.notes("chords") }));\n',
);
check(await until(async () => (await revision()) !== before2, 10000), "Keys plays a plugin");
const why = await api("graph/stems/check", { session: await session() });
check(/^Not yet for a song that plays plugins \(.+\)\.$/.test(why.json?.why ?? ""), `the check says why in words: ${JSON.stringify(why.json)}`);
const refused = await api("graph/stems", { session: await session() });
check(refused.status === 400 && refused.json?.error === why.json?.why, `and the export is refused in those words: ${JSON.stringify(refused.json)}`);
check(!fs.existsSync(`${exportsDir}/first-song stems 3`), "with no folder made");
fs.writeFileSync(`${P}/tracks/keys.ts`, keysTs);

console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
