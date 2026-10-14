# Browser suites on Linux: why 8 runs are not ok

Ground work for the platform design. Measured on jerry-ax102 (Ubuntu 24.04, ext4 on NVMe RAID 0) at f126d03,
with the binaries the lead built in the repo and chrome-headless-shell linux64 133.0.6943.141.

## In short

- 7 runs fail and 1 is PART. The cause is never the music engine. Four things go wrong.
- **Monaco's keys (4 suites).** The code editor picks its key map from the browser's user agent. On Linux it
  expects Ctrl, so the suites' ⌘ keys do nothing inside the editor (⌘↓, ⌘F, ⌘Z).
- **Labels (1 suite).** Off the desktop app, the UI reads `navigator.platform`. On Linux it shows "Ctrl+E", not "⌘E".
- **Case (1 suite).** Linux tells "exports 2" from "Exports 2". The suite expects the Mac's answer, "exports 3".
- **A race (1 suite).** Linux saves so fast that the suite reads the file before the server has published the new
  revision. The Mac's slow `F_FULLFSYNC` hid this.
- **The fix is in the suites.** One small helper says which OS the suites run on. With it, all 28 runs are ok, twice in a
  row, under load. live3b stays PART until Linux builds of its two plugins are on this machine.
- **Two more things pass only by luck.** (1) On Linux a saved file vanishes for about 0.2 ms during each save (OS-03).
  live3a crashed on that once in 11 runs. (2) The layout checks pass only because Inter comes from Google Fonts over
  the network (OS-76).
- **The watcher is not a cause.** No browser suite fails because of it. The walk fallback works. Only cargo tests fail
  on it.

## What I ran

| Run | What | Result |
|---|---|---|
| Lead's baseline | all 29, as they are | 21 ok, 1 PART, 7 FAIL (`~/m2-runs/base-f126d03/e2e.log`) |
| rerun1 | the 7 failing suites, as they are | the same 17 checks fail: not flaky (`~/m2-runs/logs/e2e-rerun1-*.log`) |
| Mac user agent | code, fixes-graph, attention, s6-write, export, with only the user agent set to a Mac's | the 4 editor suites pass in full; export still fails ⌘E (`e2e-ua-*.log`) |
| Fixed suites ×3 | all 29, with the suite fixes below | run 1: 27 ok, newsong failed one label ("Show in Finder"), now in the fix. Runs 2 and 3: 28 ok + live3b PART, 0 failed (`e2e-mod-all*.log`) |
| No web fonts | all 29, fixed, with Google Fonts blocked | 20 suites fail "no console errors"; fx-fixes also fails a toast-fit check (`e2e-nofont-*.log`) |
| Repeats ×6 | notes, s6-fixes, s5-knobs, live3a, g11-play, s6-builders, live3b, fixed | all ok 6 of 6 (live3b PART each time, as expected), except live3a: 5 of 6, a crash on a missing file (`e2e-repeat-*.log`) |

Other agents were building at the same time. The load average was 8 to 54 during these runs.

## The table

| Suite | Checks failing | Cause on Linux | OS-nn | Fix |
|---|---|---|---|---|
| code-proj | "typing osc. suggests sine, saw…", "narrowing to sine", "undoing it all makes it clean again", then a crash: the find box never opens (36 checks don't run) | Monaco's key map. `Meta+ArrowDown` doesn't go to the end of the file, so the import lands mid-file and there is no suggestion. `Meta+z` doesn't undo in the editor. `Meta+f` doesn't open the find box. | OS-75 (the survey says these work; they don't inside Monaco), OS-74 | Ctrl on Linux: `Control+End` for the end of the file, `Control+f`, `Control+z`, `Control+s` |
| fixes-graph-fix | "⌘S saves it", "Redo saves it again", then the same crash (4 checks don't run) | Monaco's key map. `Meta+ArrowUp` doesn't go to the top, so "// my first edit" lands on line 7. The save works, but the file doesn't start with the edit. | OS-75 | `Control+Home`, `Control+f`, `Control+s` |
| attention-attn | none print; it crashes after 2 checks (54 don't run) | Monaco's key map: `Meta+f` (in `fixlib.mjs`'s `key`) never opens the find box | OS-75 | `Control+f`, `Control+s` |
| s6-write-fix | "the save writes the fixed code", "which compiles", "the song is shown", "the banner goes", "the strip says it compiles", "and it plays" | Monaco's key map: `Meta+ArrowDown` doesn't move, so Backspace deletes line 9 (`import kick…`) instead of the broken last line. The save then writes code that still doesn't compile. | OS-75 | `Control+End`, `Control+s` |
| export-export | "with a tooltip naming ⌘E" | Off the app, `isMac` comes from `navigator.platform` ("Linux x86_64"). The tooltip says "Ctrl+E". | OS-74 | expect `kbd("E")`, the same rule the UI uses |
| fx-fixes-none | "any case: exports becomes exports 2", "the export goes to the real Exports folder", "and finishes", "it lands there" | ext4 tells cases apart. "exports 2" is free, so the song gets it. The suite expects "exports 3", the APFS answer. The product is right on both. | OS-71, OS-41 | Ask the songs folder whether it folds case (does `EXPORTS 2` exist?). Expect "exports 3" if it does, else "exports 2". |
| notes-proj | "the whole drag is one write" | A race in the suite. It reads the file, then the revision. The server renames the file into place first and publishes the revision about 2.7 ms later (by design, WRITE-15). On Linux the save lands as the mouse-up returns, so the first read sees the new file and the old revision (2, not 3), every time. On the Mac, `F_FULLFSYNC` (5–14 ms, PLANE-5) runs before the renames, so the first read sees the old file and the next one comes 80 ms later. | OS-02 (timing only) | Wait until the revision moves, wait 300 ms, then check it is exactly one more |
| live3b-proj (PART) | 11 plugin checks skipped | `CLAP_PATH` isn't set. This machine has no Linux builds of Surge XT or Airwindows Consolidated. | OS-32…35, OS-24, OS-27 | See "live3b's plugin part" below |

Measured for notes-proj, 3 runs: the file changed 0.1 ms after the mouse-up returned. The revision followed 2.6 to
2.8 ms later. Here `fsync` takes 0.15 ms (median of 60) and a directory `fsync` 0.08 ms. The Mac's 5–14 ms comes from
the contract (unverified here).

## Why Monaco and the labels differ

| What decides | Today | On Linux |
|---|---|---|
| Monaco's key map (`monaco-editor` 0.52.2, `vs/base/common/platform.js`) | `navigator.userAgent` contains "Macintosh" | Linux key map: `Ctrl+End`, `Ctrl+F`, `Ctrl+Z`. `Meta+…` is unbound in the editor. |
| The app's `isMac` (`web/src/lib/native.ts`), off the app | `navigator.platform` matches `/Mac/` | false: "Ctrl+E", "Show Folder" |
| The app's `isMac`, in the app | `__M2__.platform === "macos"` | false: Tauri sets "linux" |
| The app's own shortcuts (`App.tsx`) | `metaKey \|\| ctrlKey` | both work, so `Meta+e`, `Meta+n`, `Meta+d` pass on Linux by accident |

Proof: with only the user agent set to a Mac's, code, fixes-graph, attention and s6-write pass in full, and export
still fails its ⌘E check. So these are two separate causes.

The survey's OS-74 fix (let `isMac` follow `__M2__.platform`) fixes only the ⌘E check. It does not fix Monaco.
The suites must stop pretending to be a Mac and test the OS they run on.

## The fix, in one place

Add one helper, `e2e/os.mjs` (or put it in `env.mjs`). Every suite takes the OS from it.

```js
// e2e/os.mjs: the one place the suites ask which OS they run on.
export const mac = process.platform === "darwin";
export const PLATFORM = mac ? "macos" : process.platform === "win32" ? "windows" : "linux"; // __M2__.platform
export const MOD = mac ? "Meta" : "Control"; // ⌘ on a Mac, Ctrl elsewhere
export const kbd = (k, shift = false) => (mac ? `${shift ? "⇧" : ""}⌘${k}` : `Ctrl+${shift ? "Shift+" : ""}${k}`); // as native.ts
export const FILE_START = mac ? "Meta+ArrowUp" : "Control+Home"; // Monaco: top of the file
export const FILE_END = mac ? "Meta+ArrowDown" : "Control+End"; // Monaco: end of the file
export const REVEAL = mac ? "Show in Finder" : "Show Folder"; // NewSongDialog's toast button
```

| Where | Change |
|---|---|
| 20 files that set `__M2__` (all suites with a browser, `fixlib.mjs` for the `fix` suites) | `platform: "macos"` → pass `PLATFORM` in as an argument of `evaluateOnNewDocument` |
| code, fixes-graph, attention (`fixlib`), s6-write, newsong | `Meta+ArrowDown` / `Meta+ArrowUp` → `FILE_END` / `FILE_START` (s6-write's `key(k, mods)`: `End` / `Home` with `[MOD]` off the Mac) |
| every `"Meta"` in a chord (code, fixes-graph, attention, s6-*, notes, undo, start, newsong, export, g11-play) | `MOD` |
| export.mjs:65 | `\(⌘E\)` → `kbd("E")` |
| newsong.mjs:220, start.mjs:108 (native mode) | `⌘N`, `⌘O` → `kbd("N")`, `kbd("O")` |
| newsong.mjs:237 | "Show in Finder" → `REVEAL` (this one failed once the platform was Linux) |
| fx-fixes.mjs:56-68 | expect `folds ? "exports 3" : "exports 2"`, with `folds = fs.existsSync(\`${songs}/EXPORTS 2\`)` |
| notes.mjs:97 | `await until(async () => (await revision()) > rev); await sleep(300);` before the check |
| check names | say `kbd("S")` and so on, so Linux logs don't print ⌘ |

On the Mac every value is what the suites use today, so the Mac runs should not change (unverified until the
GitHub macOS runner runs them). All check counts stay the same, so `run.mjs`'s `expect` and the README table stay.

Two small related fixes:

| Piece | Fix | OS-nn |
|---|---|---|
| `chrome.mjs` and `run.mjs` default to the `mac_arm` Chrome path, so Linux needs `E2E_CHROME` | Pick the cached path by platform and arch (`linux64` here) | OS-73 |
| The app: `isMac` off the app follows `navigator.platform`, not `__M2__.platform` | Let `__M2__.platform` decide when it is given. Not needed for green with the helper, but it gives one source of truth. | OS-74 |
| The reveal button says "Show Folder" in `NewSongDialog.tsx` and "Show in Folder" in `exporting.ts` | One label per OS, in `native.ts` | OS-55 |

## Passed only by luck

| What | Why it is luck | Seen here | Fix | OS-nn |
|---|---|---|---|---|
| Any suite that reads a project file in a poll loop while the server saves it: notes, live3a, fixes-graph, fixes-notes, code, dsp, ports, s4-clips, s4-problems, s5-knobs, s6-*, attention, g11-play, live3b | On Linux, `RENAME_SWAP` doesn't exist, so the save renames the old file aside, hashes it, then renames the new one in. Between the two the file is missing. A read then throws `ENOENT` and the suite crashes. Editors, git and agents can see the gap too. | live3a crashed on `data/bass/notes.json` in 1 of 11 runs. A probe made 200 note saves while reading the file: 231 of 116,400 reads found no file, gaps up to 0.19 ms. | `renameat2(RENAME_EXCHANGE)` for Swap and `RENAME_NOREPLACE` for Excl on Linux. Both work on this ext4 (tested with glibc 2.39). This also fixes the cargo test `a_commit_that_fails_after_its_preview_accepts_nothing`. | OS-03 |
| Layout and text-fit checks (fx-fixes name widths and toasts, layout, views) | Inter and JetBrains Mono come from Google Fonts. This machine has only DejaVu and Liberation fonts. Chrome falls back to DejaVu Sans, which is wider. | With the fonts blocked: 20 suites fail "no console errors" (the font stylesheet fails to load), and fx-fixes's "all of it fits the toast" fails: the plugin toast is cut. | Serve the two fonts from `web/` | OS-76 |
| s6-fixes.mjs:112 "the stray file stays, one revision" | The same file-then-revision race as notes:97. It passes because its first poll comes before the save. | 11 of 11 ok | Wait for the revision, as in notes | OS-02 (timing) |
| Shortcuts outside the editor (`Meta+z`, `Meta+e`, `Meta+n`, `Meta+d`) | They pass on Linux only because the app also takes `metaKey`. Nothing tests Ctrl, the key Linux users press. | all ok | `MOD` | OS-75 |
| newsong "⌘N in the code editor" | `Meta+ArrowDown` does nothing in Monaco on Linux, but the check passes wherever the text lands | ok | `FILE_END` | OS-75 |
| start and newsong "in the app" labels | They inject `platform: "macos"`, so they check the Mac's labels on Linux and never Linux's | ok | `PLATFORM`, `kbd()` | OS-74 |
| HANDOFF's flakes: s5-knobs, live3a's ruler drag, g11-play timing | Timing checks with Mac-made limits | s5-knobs 11/11, live3a's ruler checks 11/11, g11-play 11/11 (debug build, under load) | Keep watching. These are judged on the Mac. | OS-85 |

## live3b's plugin part (CLAP_PATH) on Linux

| Need | State here | Note |
|---|---|---|
| A folder with `Surge XT.clap` and `Airwindows Consolidated.clap`, these exact names (live3b.mjs:239) | None: no `~/.clap`, `/usr/lib/clap` or `/usr/local/lib/clap` | Linux x86_64 CLAP builds. Surge XT's `.deb` puts `Surge XT.clap` in `/usr/lib/clap` (unverified). Without root, unpack the release archives into a user folder (e.g. `dpkg-deb -x`) and point `CLAP_PATH` at it. Not done: it needs the owner's OK. |
| The same Surge XT version as the example's state | `data/lead/lead.state.json` says 1.3.4 | The state is Surge's XML patch, saved by the Mac build. That the Linux build loads it the same is unverified. |
| Surge XT's system libraries, even headless | libX11, libxcb, libxkbcommon, freetype, fontconfig, cairo, GL, asound are present. libxcb-cursor, libxcb-util, libxcb-xkb and libxkbcommon-x11 are not. | Run `ldd "Surge XT.clap"` once it is here. Install what says "not found" only with the owner's OK. |
| Finding and loading | `CLAP_PATH` is read first; a `.clap` file is the library itself; `dlopen` works | Works as is (OS-32…35). The Linux default folders `~/.clap` and `/usr/lib/clap` are missing from `search_paths()` (OS-32). |
| The plugin main thread | the headless server drains a channel (`run_main`). The host offers no `timer-support` or `posix-fd-support`. | Whether Surge XT (JUCE) works headless without them is unverified (OS-27). `is_process_main()` is false off macOS (OS-24); the host's thread check uses the executor's thread, not that. |

## Corrections to the surveys and HANDOFF

| Says | Measured |
|---|---|
| OS-75: "`Meta+…` chords work, since the UI takes `metaKey \|\| ctrlKey`" | True outside the code editor. False inside Monaco, which follows the user agent. That is 4 of the 7 failures. |
| OS-74: "a one-line change" fixes the ⌘ checks | It fixes 1 check (export's tooltip). The suites must also use Ctrl on Linux. |
| HANDOFF: browser failures include "the watcher" | None. Outside edits are taken in by walking (s6-write: "after 207 ms, not before the 150 ms of quiet"). Only cargo's `watch_tests` fail on it. |

## Not checked

- The fixed suites on a Mac (needs the GitHub macOS runner).
- What user agent WebKitGTK gives in the Linux desktop app. If it contains "Linux", Monaco and `isMac` agree there.
- Whether GTK menu accelerators (`CmdOrCtrl+Z`) take Ctrl+Z before Monaco in the Linux app. No suite runs in native
  mode with a real menu.
- live3b's plugin part on Linux (no plugins here).
