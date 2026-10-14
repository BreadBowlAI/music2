// The browser every suite drives: E2E_CHROME (run.mjs always sets it), else puppeteer's cached
// chrome-headless-shell 133.0.6943.141 for this OS (OS-73). Off the Mac, suites launch it through
// chrome-as-mac.sh, which gives pages a Mac user agent: the code editor (Monaco) takes its keys
// from it, and the suites test the Mac's keys (OS-75, Q2). run.mjs takes `real` from here.
import os from "node:os";
import { fileURLToPath } from "node:url";

const mac = process.platform === "darwin";
const cache = `${os.homedir()}/.cache/puppeteer/chrome-headless-shell`;
export const real =
  process.env.E2E_CHROME ??
  (mac
    ? `${cache}/mac_arm-133.0.6943.141/chrome-headless-shell-mac-arm64/chrome-headless-shell`
    : `${cache}/linux-133.0.6943.141/chrome-headless-shell-linux64/chrome-headless-shell`);
// puppeteer passes the environment on to the wrapper.
if (!mac) process.env.E2E_CHROME_BIN = real;
export const exe = mac ? real : fileURLToPath(new URL("./chrome-as-mac.sh", import.meta.url));
