// The web engine's probe in headless Chrome: `node chrome.mjs <out dir>`, after wasmtime.rs wrote
// <out dir>/cases.json. Serves this checkout and <out dir> on localhost, cross-origin isolated
// (COOP and COEP), and writes <out dir>/chrome-main.json, chrome-worker.json,
// chrome-worklet.json and chrome-realtime.json. M2_CHROME names the browser; M2_PUPPETEER the folder of puppeteer-core
// (e2e/node_modules/puppeteer-core by default).
import { createReadStream, existsSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../../../..");
const dir = process.argv[2];
const exe = process.env.M2_CHROME;
const puppeteerDir = process.env.M2_PUPPETEER || join(root, "e2e/node_modules/puppeteer-core");
const puppeteer = (await import(pathToFileURL(join(puppeteerDir, "lib/puppeteer/puppeteer-core.js")).href)).default;

const types = { ".mjs": "text/javascript", ".js": "text/javascript", ".json": "application/json", ".wasm": "application/wasm", ".html": "text/html" };
const server = createServer((req, res) => {
  const url = new URL(req.url, "http://x").pathname;
  const file = url.startsWith("/out/") ? join(dir, url.slice(5)) : url === "/" ? join(here, "index.html") : join(root, normalize(url));
  const headers = { "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp" };
  if (url === "/") return res.writeHead(200, { ...headers, "Content-Type": "text/html" }).end('<!doctype html><script type="module"></script>');
  if (!existsSync(file) || !statSync(file).isFile()) return res.writeHead(404, headers).end();
  res.writeHead(200, { ...headers, "Content-Type": types[extname(file)] || "application/octet-stream" });
  createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox", "--autoplay-policy=no-user-gesture-required"] });
try {
  const page = await browser.newPage();
  page.on("console", (m) => console.log("page:", m.text()));
  await page.goto(`${base}/`);
  const r = await page.evaluate(async (u) => (await import(u)).run(), `${base}/docs/designs/live/probe/page.mjs`);
  const agent = `${r.page.agent} (headless shell), page isolated: ${r.page.isolated}`;
  writeFileSync(join(dir, "chrome-main.json"), JSON.stringify({ host: `${agent}, the page's thread`, flushes: r.page.flushes, results: r.main }, null, 2));
  const where = { worker: "a dedicated Worker", worklet: "an AudioWorklet of an OfflineAudioContext", realtime: "an AudioWorklet of an AudioContext" };
  for (const [name, w] of [["worker", r.worker], ["worklet", r.offline], ["realtime", r.realtime]]) {
    const host = `${agent}, ${where[name]}`;
    writeFileSync(join(dir, `chrome-${name}.json`), JSON.stringify({ host, ...w }, null, 2));
  }
  if (r.page.math) writeFileSync(join(dir, "chrome-math.json"), JSON.stringify({ host: agent, math: r.page.math }, null, 2));
  console.log(`main ${r.main.length}, worker ${r.worker.results.length}, offline worklet ${r.offline.results?.length}, real-time worklet ${r.realtime.results?.length ?? r.realtime.error}`);
} finally {
  await browser.close();
  server.close();
}
