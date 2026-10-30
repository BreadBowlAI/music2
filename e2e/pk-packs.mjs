// Packs from a folder (PACK-6, PACK-10, PACK-11): the fixture pack @test/tiny, installed in the
// scratch HOME's store, shows in the browser with std's, its rows saying which pack they are from;
// a double-click on Tiny keys adds a track that plays it, and writes the pack's [dependencies] line
// and music2.lock in the same edit; one Undo takes all of it back. The browser's Packs list shows
// the pack and its version.
//
//   node e2e/pk-packs.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { check, done, HOME, open, PROJ, read, until } from "./fixlib.mjs";

const REPO = process.env.E2E_REPO ?? path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TINY = path.join(REPO, "core/session/tests/fixtures/packs/tiny");

/** Every file under `dir`, by its path there with `/`, sorted bytewise; dot files left out. */
function files(dir, rel = "") {
  const out = [];
  for (const e of fs.readdirSync(path.join(dir, rel), { withFileTypes: true })) {
    if (e.name.startsWith(".")) continue;
    const r = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) out.push(...files(dir, r));
    else out.push(r);
  }
  return out.sort((a, b) => (Buffer.compare(Buffer.from(a), Buffer.from(b))));
}

// Installed as `m2 pack install` leaves it: the files, and m2pack.json listing them (PACK-6).
const store = path.join(HOME, "Music/music2/Packs/@test/tiny/0.1.0");
fs.cpSync(TINY, store, { recursive: true });
const listed = files(store).map((p) => {
  const bytes = fs.readFileSync(path.join(store, p));
  return `  {"path":${JSON.stringify(p)},"size":${bytes.length},"sha256":"sha256:${crypto.createHash("sha256").update(bytes).digest("hex")}"}`;
});
fs.writeFileSync(path.join(store, "m2pack.json"), `{"m2pack": 1, "files": [\n${listed.join(",\n")}\n]}\n`);

const { page, errors, api, state, toasts } = await open(1440, 900, { browser: true });
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
// The lists are made as a song opens (PACK-11): open it again.
check((await post("/close", {})).status === 200 && (await post("/open", { path: PROJ })).status === 200, "the song opens again");
const manifest = read("music2.toml");
check(!fs.existsSync(`${PROJ}/music2.lock`), "it has no music2.lock");

const row = '[data-browser-instrument="@test/tiny:tiny-keys"]';
check(await until(() => page.$(row), 15000), "Tiny keys is in the browser");
const group = await page.$eval(row, (e) => e.closest("[role=group]")?.getAttribute("aria-label"));
check(group === "Keys", `under its category, Keys (${group})`);
const title = await page.$eval(row, (e) => e.getAttribute("title"));
check(title.includes("From @test/tiny 0.1.0"), `its tooltip names its pack (${JSON.stringify(title)})`);
check(!!(await page.$('[data-browser-item="@test/tiny:tiny-crunch"]')), "its effect, Tiny crunch, is with the effects");
const std = await page.$eval('[data-browser-instrument="std:synth-pluck"]', (e) => e.getAttribute("title")).catch(() => "");
check(std !== "" && !std.includes("From "), "std's rows name no pack");

// ---------------------------------------------------------------- add: one edit, with its pack

const tracks = async () => (await state()).view.modules.filter((m) => m.track).map((m) => m.key);
const before = await tracks();
await page.click(row, { count: 2 });
check(await until(async () => (await tracks()).length === before.length + 1, 10000), "a double-click adds a track");
const added = (await tracks()).find((k) => !before.includes(k));
check(await until(() => read("music2.toml").includes('"@test/tiny" = "0.1"')), "music2.toml lists the pack");
const lock = fs.existsSync(`${PROJ}/music2.lock`) ? read("music2.lock") : "";
check(lock.includes('name = "@test/tiny"') && lock.includes('version = "0.1.0"'), "music2.lock keeps its version");
check(read(`tracks/${added}.ts`).includes('import { tinyKeys } from "@test/tiny";'), "the track imports it by its scoped name");
await page.click('button[aria-label="Undo"]');
check(await until(async () => (await tracks()).length === before.length), "one Undo takes the track away");
check(await until(() => read("music2.toml") === manifest && !fs.existsSync(`${PROJ}/music2.lock`)), "and the pack's line and the lock with it");

// ---------------------------------------------------------------- the Packs list

await page.click("[data-browser-packs] button");
check(await until(() => page.$('[data-pack="@test/tiny"]')), "the Packs list shows @test/tiny");
const shown = await page.$eval('[data-pack="@test/tiny"]', (e) => e.textContent);
check(shown.includes("0.1.0"), `with its version (${shown})`);
const said = await page.$eval('[data-pack="@test/tiny"]', (e) => e.getAttribute("title"));
check(said.includes("Tiny keys") && said.includes("CC0-1.0"), `its tooltip says what is in it and its licence (${JSON.stringify(said)})`);

const t = await toasts();
check(!t.some((x) => /error|couldn't/i.test(x)), `no error toast (${t.join(" | ")})`);
done(errors);
