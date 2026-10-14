// What the suites get from run.mjs's environment, with fallbacks for running a suite by hand.
//   exe       the browser (chrome.mjs): E2E_CHROME, else puppeteer's cached chrome-headless-shell
//   REPO      the checkout the fixtures come from: E2E_REPO, else the repo this folder is in
//   HEADLESS  the headless server binary, for suites that start a second server: E2E_HEADLESS
import { fileURLToPath } from "node:url";

export { exe } from "./chrome.mjs";
export const REPO = process.env.E2E_REPO ?? fileURLToPath(new URL("..", import.meta.url)).replace(/\/$/, "");
export const HEADLESS = process.env.E2E_HEADLESS ?? `${REPO}/target/debug/examples/headless`;
