// The browser every suite drives: E2E_CHROME (run.mjs always sets it), else puppeteer's cached
// chrome-headless-shell 133.0.6943.141.
import os from "node:os";

export const exe =
  process.env.E2E_CHROME ??
  `${os.homedir()}/.cache/puppeteer/chrome-headless-shell/mac_arm-133.0.6943.141/chrome-headless-shell-mac-arm64/chrome-headless-shell`;
