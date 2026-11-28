// The time limit's web form (docs/designs/live/web-engine.md): can a page stop a Worker stuck in a
// kernel's endless loop, and start another? `node stuck.mjs`, with M2_CHROME and M2_PUPPETEER as
// for chrome.mjs. The worker's module is (func (export "spin") (loop (br 0))), written as bytes.
import { createServer } from "node:http";
import { pathToFileURL } from "node:url";
const puppeteer = (await import(pathToFileURL(process.env.M2_PUPPETEER + "/lib/puppeteer/puppeteer-core.js").href)).default;
const worker = `
const bytes = new Uint8Array([0,97,115,109,1,0,0,0, 1,4,1,96,0,0, 3,2,1,0, 7,8,1,4,115,112,105,110,0,0, 10,9,1,7,0,3,64,12,0,11,11]);
onmessage = (e) => { if (e.data === "spin") { postMessage("spinning"); new WebAssembly.Instance(new WebAssembly.Module(bytes)).exports.spin(); } else postMessage("pong"); };`;
const server = createServer((req, res) => {
  if (req.url === "/w.js") return res.writeHead(200, { "Content-Type": "text/javascript" }).end(worker);
  res.writeHead(200, { "Content-Type": "text/html" }).end("<!doctype html>");
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const browser = await puppeteer.launch({ executablePath: process.env.M2_CHROME, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.goto(`http://127.0.0.1:${server.address().port}/`);
const r = await page.evaluate(async () => {
  const msg = (w, m) => new Promise((res) => { w.onmessage = (e) => res(e.data); w.postMessage(m); });
  const w = new Worker("/w.js");
  await msg(w, "spin");
  const t0 = performance.now();
  // The page's own thread still runs while the worker spins.
  await new Promise((res) => setTimeout(res, 100));
  const late = performance.now() - t0 - 100;
  const tt = performance.now();
  w.terminate();
  const w2 = new Worker("/w.js");
  const pong = await msg(w2, "ping");
  return { pageLateMs: Math.round(late), terminatedAndNewWorkerAnsweredMs: Math.round(performance.now() - tt), pong };
});
console.log(JSON.stringify(r));
await browser.close();
server.close();
