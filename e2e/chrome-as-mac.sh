#!/bin/sh
# e2e/chrome-as-mac.sh: chrome-headless-shell telling pages it runs on a Mac. The code editor
# (Monaco) takes its keys from the user agent, and the suites test the Mac's keys (OS-75, Q2).
exec "${E2E_CHROME_BIN:?chrome.mjs sets it}" \
  --user-agent="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/133.0.0.0 Safari/537.36" \
  "$@"
