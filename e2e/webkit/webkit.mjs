#!/usr/bin/env node
// The WebKit tool (e2e/README.md, "WebKit numbers"): the zoom bench's runs (zoomlib.mjs's plan, the
// same as `zoom-bench`'s in Chrome) in WebKitGTK's MiniBrowser, the engine of the Linux app and the
// same engine code as the Mac's WebKit. Not a suite, and no tier runs it: a tool for a track that
// wants WebKit's numbers.
//
// It copies the song fresh into <out>, starts the headless server on it with a scratch HOME there,
// serving a copy of the web build whose page gets its key and a small driver (the plan and
// zoomlib's page functions, as text), and opens it in MiniBrowser, full screen on an invisible Xvfb
// screen of its own, sized to a page of 1440×900 px at `--dpr` device px a px. The page runs the
// plan itself and posts each run to a listener here. It prints the table and writes every frame to
// <out>/webkit.json.
//
// WebKit draws in software here (no graphics chip on a server, and Xvfb has none), so paint costs
// more than on a Mac; its style, layout and script are the same engine code.
//
//   node e2e/webkit/webkit.mjs <headless> <song> <out> [--dpr N] [--web DIR]
//
//   headless  the server: `cargo build -p m2-server --example headless` → target/<profile>/examples/headless
//   song      the song to measure (copied, never changed): `node e2e/beat-song.mjs <folder>` makes one
//   out       a new or empty folder for its scratch HOME, the song's copy, the logs and webkit.json
//   --dpr     device px a px (default 2, as the owner's Mac)
//   --web     the web build to serve (default this checkout's web/dist)
// Environment: MINIBROWSER, the browser (default WebKitGTK 4.1's, where Debian and Fedora put it);
// WEBKIT_TIMEOUT, seconds the page may take (default 600).
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { execFileSync, spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { GESTURES, PLAN, rowsOf, zoomRun, zoomTable } from "../zoomlib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const usage = (msg) => {
  console.error(`webkit: ${msg}\nusage: node e2e/webkit/webkit.mjs <headless> <song> <out> [--dpr N] [--web DIR]`);
  process.exit(2);
};
const args = process.argv.slice(2);
const opts = { dpr: 2, web: path.join(HERE, "../../web/dist") };
const pos = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--dpr") opts.dpr = Number(args[++i]);
  else if (args[i] === "--web") opts.web = path.resolve(args[++i] ?? usage("--web needs a folder"));
  else if (args[i].startsWith("--")) usage(`no option ${args[i]}`);
  else pos.push(path.resolve(args[i]));
}
const [headless, song, out] = pos;
if (pos.length !== 3) usage("it takes the server, the song and a folder for what it writes");
if (!(opts.dpr > 0)) usage("--dpr is a number of device px a px");
if (!fs.existsSync(headless)) usage(`no server at ${headless}`);
if (!fs.existsSync(path.join(song, "music2.toml"))) usage(`no song at ${song}`);
if (!fs.existsSync(path.join(opts.web, "index.html"))) usage(`no web build at ${opts.web} (cd web && npm run build)`);
if (fs.existsSync(out) && fs.readdirSync(out).length) usage(`${out} isn't empty; give a new folder, so nothing is written over`);
const minibrowser =
  process.env.MINIBROWSER ??
  [
    ...(fs.existsSync("/usr/lib") ? fs.readdirSync("/usr/lib").map((d) => path.join("/usr/lib", d, "webkit2gtk-4.1/MiniBrowser")) : []),
    "/usr/libexec/webkit2gtk-4.1/MiniBrowser",
  ].find((p) => fs.existsSync(p));
if (!minibrowser) usage("no MiniBrowser (WebKitGTK 4.1's): install it, or set MINIBROWSER");

// ---------------------------------------------------------------- what it serves

fs.mkdirSync(out, { recursive: true });
const proj = path.join(out, "song");
fs.cpSync(song, proj, { recursive: true });
const home = path.join(out, "home");
fs.mkdirSync(path.join(home, ".music2"), { recursive: true });
const token = `webkit${crypto.randomBytes(8).toString("hex")}`;

// The listener: what the page measured.
const results = [];
const logs = [];
let table = null;
let go = false;
let finished;
const done = new Promise((r) => (finished = r));
const listener = http.createServer((req, res) => {
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => {
    res.writeHead(200, { "access-control-allow-origin": "*", "content-type": "text/plain" });
    const what = req.url.slice(1);
    res.end(what === "go" ? (go ? "go" : "wait") : "ok");
    if (what === "go") return;
    const data = body ? JSON.parse(body) : {};
    if (what === "result") {
      results.push(data);
      console.log(`${data.name} ${data.rep}: ${data.run.frames.length} frames, its events sent in ${data.run.sent} ms`);
    } else if (what === "table") table = data.text;
    else if (what === "log") {
      logs.push(data);
      console.log(`page: ${JSON.stringify(data)}`);
    } else if (what === "done") finished();
  });
});
await new Promise((r) => listener.listen(0, "127.0.0.1", r));
const post = `http://127.0.0.1:${listener.address().port}`;

// The web build, its page given its key (as the app's shell gives it) and the driver, ahead of the app.
const web = path.join(out, "web");
fs.cpSync(opts.web, web, { recursive: true });
const driver = `// The WebKit tool's driver (e2e/webkit/webkit.mjs): it runs the plan in the page and posts what it measured.
(() => {
  const plan = ${JSON.stringify({ post, steps: PLAN, settle: 3000 })};
  const GESTURES = ${JSON.stringify(GESTURES)};
  const zoomRun = ${zoomRun.toString()};
  const zoomTable = ${zoomTable.toString()};
  const rowsOf = ${rowsOf.toString()};
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const send = (what, body) => fetch(plan.post + "/" + what, { method: "POST", body: JSON.stringify(body) }).catch(() => {});
  window.addEventListener("error", (e) => send("log", { error: String(e.message) }));
  window.addEventListener("resize", () => send("log", { size: [innerWidth, innerHeight] }));
  (async () => {
    send("log", { hello: navigator.userAgent, dpr: devicePixelRatio, size: [innerWidth, innerHeight] });
    for (let i = 0; i < 1200 && !(window.m2canvas && document.querySelector("[data-testid=timeline] [data-clip]")); i++) await sleep(100);
    if (!window.m2canvas) return send("log", { error: "the app never drew a clip" }).then(() => send("done", {}));
    // The driver says when the window is sized.
    for (let i = 0; i < 600 && (await fetch(plan.post + "/go", { method: "POST", body: "{}" }).then((r) => r.text(), () => "wait")) !== "go"; i++) await sleep(100);
    await sleep(plan.settle);
    const results = [];
    for (const step of plan.steps) {
      try {
        const run = await zoomRun(step);
        results.push({ ...step, run });
        await send("result", { ...step, run });
      } catch (e) {
        await send("log", { error: step.name + " " + step.rep + ": " + (e && e.stack) });
      }
    }
    await send("table", { text: await zoomTable("Run", rowsOf(results)) });
    await send("done", {});
  })();
})();
`;
fs.writeFileSync(path.join(web, "zoom-driver.js"), driver);
const index = path.join(web, "index.html");
const head = `<script>window.__M2__ = { native: false, token: ${JSON.stringify(token)}, platform: "macos" };</script><script src="/zoom-driver.js"></script>`;
fs.writeFileSync(index, fs.readFileSync(index, "utf8").replace("<head>", `<head>\n    ${head}`));

// ---------------------------------------------------------------- the server, the screen, the browser

const procs = [];
let stopped = false;
const stopAll = () => {
  if (stopped) return;
  stopped = true;
  for (const p of procs) p.kill();
  listener.close();
};
process.on("exit", stopAll);
process.on("SIGINT", () => process.exit(130));
process.on("uncaughtException", (e) => {
  console.error(e);
  process.exit(1);
});

const session = path.join(home, ".music2/session.json");
const serverLog = fs.openSync(path.join(out, "server.log"), "w");
const server = spawn(headless, [proj, session, token], { env: { ...process.env, HOME: home, M2_AUDIO: "none", M2_MIDI: "none", M2_WEB: web }, stdio: ["ignore", serverLog, serverLog] });
procs.push(server);
for (let i = 0; i < 600 && !fs.existsSync(session); i++) await new Promise((r) => setTimeout(r, 100));
if (!fs.existsSync(session)) throw new Error(`the server didn't start: see ${path.join(out, "server.log")}`);
const { url } = JSON.parse(fs.readFileSync(session, "utf8"));

// An invisible screen of its own, a little taller than the page (MiniBrowser's toolbar is over it).
const xvfb = spawn("Xvfb", ["-displayfd", "3", "-screen", "0", `${Math.round(1440 * opts.dpr)}x${Math.round(1100 * opts.dpr)}x24`, "-nolisten", "tcp"], { stdio: ["ignore", "ignore", "ignore", "pipe"] });
procs.push(xvfb);
const display = await new Promise((r) => xvfb.stdio[3].once("data", (d) => r(String(d).trim())));
const mbLog = fs.openSync(path.join(out, "minibrowser.log"), "w");
const mbHome = path.join(out, "browser-home");
fs.mkdirSync(mbHome);
const mb = spawn(minibrowser, ["--full-screen", url], { env: { ...process.env, DISPLAY: `:${display}`, GDK_SCALE: String(opts.dpr), HOME: mbHome }, stdio: ["ignore", mbLog, mbLog] });
procs.unshift(mb);
const timeout = setTimeout(() => {
  console.error("webkit: the page took too long (WEBKIT_TIMEOUT)");
  finished();
}, Number(process.env.WEBKIT_TIMEOUT ?? 600) * 1000);

// No window manager: the window is sized by hand, until its page is 1440×900 under the toolbar.
const resize = (h) => execFileSync("python3", ["-I", path.join(HERE, "xresize.py"), String(Math.round(1440 * opts.dpr)), String(Math.round(h * opts.dpr))], { env: { ...process.env, DISPLAY: `:${display}` } }).toString().trim();
const sized = () => [...logs].reverse().find((l) => l.size)?.size;
// The page says hello once its window is up (MiniBrowser takes seconds to start its web process).
for (let i = 0; i < 600 && !logs.length; i++) await new Promise((r) => setTimeout(r, 100));
let h = 938;
for (let k = 0; k < 4; k++) {
  resize(h);
  await new Promise((r) => setTimeout(r, 1500));
  const [iw, ih] = sized() ?? [0, 0];
  if (iw === 1440 && ih === 900) break;
  if (iw === 1440) h += 900 - ih;
}
console.log(`the page: ${JSON.stringify(sized())} px`);
go = true;
await done;
clearTimeout(timeout);

const hello = logs.find((l) => l.hello);
const file = path.join(out, "webkit.json");
fs.writeFileSync(file, JSON.stringify({ engine: { userAgent: hello?.hello, dpr: hello?.dpr, window: sized(), minibrowser }, logs, results }, null, 1));
console.log(`\n## ${path.basename(song)}: WebKitGTK (${hello?.hello ?? "?"}), software drawing, ${JSON.stringify(sized())} px at ${hello?.dpr} device px a px\n`);
console.log(table ?? "No table: see the page's errors above.");
console.log(`\nEvery frame: ${file}`);
stopAll();
process.exit(results.length === PLAN.length ? 0 : 1);
