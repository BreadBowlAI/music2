# The three waiting branches: where the platform layer meets them

Ground work for the platform design. Measured on jerry-ax102 (Ubuntu 24.04, x86_64, ext4) on 2026-10-14, with
main at `f126d03` and the branches `wip/step7` `5079233`, `taste` `4804247` and `sounds` `916127d`. All three
fork from `8aa301e`. Since then main only added docs.

## In short

- **All three branches merge into main cleanly today.**
- **If main keeps to its own files, the layer meets the branches in five small places.** Measured with trial merges
  (section 1 says how):

  | Branch | Files that will conflict | What each takes |
  |---|---|---|
  | wip/step7 | `apps/music2-desktop/src/main.rs`, `Cargo.lock`, the contract (PLANE-5) | a few lines; keep both sides |
  | taste | the contract (WRITE-18's *Today*) | one line |
  | sounds | `std/kernels/std.wasm` | a rebuild, never "take one side" |

- **taste is the big risk.** taste rewrote the menu bar, the window's title bar code, the fonts, the top bar and
  most browser suites. If main edits those too, taste conflicts in 12 files. So do those pieces on taste, or after
  it merges.
- **Two one-place fixes make 7 of taste's failing browser suites pass on Linux, with no suite edits** (measured):
  1. `isMac` follows `__M2__.platform` when a page is given one (`web/src/lib/native.ts`, one line);
  2. Chrome tells pages it runs on a Mac (Monaco, the code editor, picks its keys from the user agent).

  The per-suite helper that `e2e.md` proposes also works, but it conflicts with taste in 6 suite files. It also
  misses taste's 3 new suites.
- **The libm change merges cleanly into step 7.** All 8 math calls land in step 7's moved audio code. Step 7 adds
  no new host math on the render path.
- **step 7's G7f gate is Mac-only inside.** On Linux its memory reading returns nothing, so two of its rules pass by
  default (0 ≤ 0). It also judges Linux runs against M1 records. The layer should offer the process's memory, and
  each record should name its machine.
- **std.wasm: a path remap is not enough.** sounds' `std.wasm` rebuilt here bit for bit in one folder, and differed
  in 1,003 bytes in another. Cargo hashes `core/kernel`'s absolute path into the build. With the kernel crate
  inside the kernels' own workspace, two folders gave the same bytes, with no machine path in them (measured).
- **No branch adds a failure caused by a missing Linux piece.** The extra failures are each branch's unfinished
  work, plus one race in a taste suite that loses on Linux. Sections 2.3, 3.3 and 4.4 list them.

---

## 1. What was measured, and how

| What | How | Where the result is |
|---|---|---|
| Trial merges | A scratch clone (`git clone --shared`, all branches fetched). On main, a comment was added to each line the surveys name, one group per seam (math, threads, dirs, files and lock, watcher, audio, shell, gates, browser suites, std.wasm, Cargo). Then `git merge --no-commit` of each branch, the conflicted files listed, and `git merge --abort`. | §5 (per seam), and each branch's table |
| Branch tests | `git archive` of each branch into scratch, its own target dir, a scratch `HOME`, `M2_AUDIO=none`. `cargo build`, `cargo test --no-fail-fast`, and `cargo clippy` with warnings listed (not denied). | §2.3, §3.3, §4.4 |
| taste's browser suites | The lead's full run (`~/m2-runs/linux-taste-4804247/e2e.log`, taken while three branch checks ran at once). Then the failing suites again, alone, and with the two one-place fixes. | §3.3 |
| std.wasm | main's and sounds' kernels rebuilt with rustc 1.99.0 (`b940084d7`, the same rustc the Mac used), in different folders, with and without `--remap-path-prefix`. | §4.3 |

**A caveat.** The trial merges used marker edits, not the real layer. A real change can touch fewer lines (fewer
conflicts) or more (more conflicts). The marker edits sit exactly on the lines `survey-code.md` names.

---

## 2. wip/step7 (`5079233`, the engine, unfinished)

### 2.1 OS pieces it adds or changes

| # | Piece | File and lines (on the branch) | On the Mac | On Linux today | Relates to |
|---|---|---|---|---|---|
| 1 | The process's memory from Mach `task_info(TASK_VM_INFO)`: footprint, address space in use, and its limit | `crates/gates/src/store.rs` 151-179 | real numbers | returns `None`. The footprint, the space and the limit stay 0, so "footprint within S + 64 MB" and "2 renderers within 25% of the address space" pass by default (0 ≤ 0) | OS-82; survey-later §5.2 ("the gates' RSS") |
| 2 | QoS `USER_INTERACTIVE` for the drop test's audio-like thread | `store.rs` 288-291 | kept on a P core | nothing: a plain thread. Here `ulimit -r` is 0, so `SCHED_FIFO` would be refused too | OS-83; survey-later #5, #11 |
| 3 | M1 records the rules judge against: `store-S.txt` and `callback-S.txt` (or `$M2_STORE_S`, `$M2_CALLBACK_S`) | `crates/gates/store-S.txt`, `callback-S.txt`; `src/store.rs` 337; `src/arena.rs` 101 | the reference | a Linux run is judged against an M1 Pro's numbers | OS-84, OS-85 |
| 4 | The gates README tells you `CLAP_PATH=~/Library/Audio/Plug-Ins/CLAP` | `crates/gates/README.md` | right | Linux uses `~/.clap`. Neither `~/.clap` nor `/usr/lib/clap` exists here | OS-32 |
| 5 | The probe plugin counts how often the thread calling `process` changes, with `main_thread::token()` | `core/plugins/src/probe.rs` 543 (new; 363, 516, 528, 537 were there) | `pthread_self` | the same, works | OS-25 |
| 6 | A counting allocator in debug builds of the app, and a thread that logs it | `apps/music2-desktop/src/main.rs` 27-35; `core/play/src/alloc.rs` | — | portable. Its thread-locals are `const` and have no destructor, so they allocate nothing. `core/play/tests/no_alloc.rs` passes here | OS-25 (thread-locals); RENDER-28 |
| 7 | Export pauses playback (`Hold`) and waits up to 2 s for the callback to say it held | `crates/server/src/export.rs`; `core/play/src/lib.rs` | — | portable. But one xrun marks the player stalled today (survey-later bug 1). A stalled player never holds, so such an export renders beside playback | survey-later §0 bug 1 |
| 8 | The audio clip path moved: `AudioNode` became `Declick` inside a new `AudioPlayer`, with a linear crossfade when renders swap | `core/runtime/src/audio.rs` | Apple's libm in 8 calls | glibc's libm in the same 8 calls. No new transcendental call anywhere on the render path: 16 calls on main, 16 on step 7, in the same places (grep of non-test code) | OS-45 |
| 9 | Contract text: PLANE-5 still says `F_FULLFSYNC`; RENDER-32 gives the Mac's numbers (131,068 GiB of address space) | `docs/architecture.md` 83, about 315-330 | right | x86_64 here has 48-bit virtual addresses: 128 TiB (131,072 GiB), about the same | OS-02 (wording), OS-82 |

step 7 doesn't touch `main_thread.rs`, `stage.rs`, `lock.rs`, the watcher, `fsevents.rs`, the paths, the menus,
`web/`, `e2e/` or `std.wasm`.

### 2.2 Where the layer on main meets it

| File | The layer's change on main | step 7's change | Trial merge | Merge note: who moves what |
|---|---|---|---|---|
| `core/runtime/src/audio.rs` | `libm` for the 8 calls (OS-45) | moves the code, renames `AudioNode` | **clean**. All 8 edits land in step 7's moved code (checked line by line) | Nothing to do. Add a test that greps the render-path sources for `.sin(`, `.cos(`, `.powf(` and the like, so later step 7 code can't bring host math back. |
| `core/session/tests/renders.json` | `examples/resample` recorded once (the Mac's hash changes too) | untouched | clean | After the merge, run the renders test on Linux and on the Mac runner. step 7 says its offline renders don't change. |
| `apps/music2-desktop/src/main.rs` | the plugin main loop installed per OS (lines 28-32) | inserts the allocator and `watch()` just above | **conflict** | Keep both: `watch()` first, then the layer's install. |
| `Cargo.lock` | new entries (`m2-os`, `libm`) | `m2-session` gains `m2-plugins` (dev), the app gains `m2-play` | **conflict** (neighbouring lines) | Take main's lock; `cargo build` adds step 7's two path edges. Never `cargo update`. |
| `docs/architecture.md` | PLANE-5's sync named for every OS | PLANE-5: "an activation that keeps every instance" | **conflict** (one long line) | Keep both edits. |
| `core/plugins/src/probe.rs` | if `main_thread::token()` and `is_process_main()` move into `m2-os` | 6 calls (4 `token()`, 2 `is_process_main()`), 1 of them new (543) | clean as text, but **won't compile** if the functions move | Keep both as re-exports in `main_thread`, or fix the 6 calls during the merge (the step 7 finisher). |
| `crates/gates/src/store.rs` (new file) | the layer offers "process memory" and "an audio-like thread" | Mach call and QoS inline | clean | The step 7 finisher swaps its two `cfg` blocks for the layer's calls. The layer step should ship both functions (Linux: `/proc/self/status` `VmRSS` and `VmSize`; the limit from the CPU's address bits). |
| `core/play/src/lib.rs` | the audio seam: no device → silent; sort cpal's errors (bug 1) | `Hold`, refused activations, allocation marks | clean | Fix bug 1 on main first: step 7's `Hold` needs a player that one xrun doesn't retire. |
| `core/plugins/src/inprocess.rs` | Linux host extensions (`timer-support`, `posix-fd-support`) | `load_state` returns `bool`; `holds` | clean | — |
| `crates/gates/src/server.rs`, `server/watch.rs` | labels, `cfg(unix)` | small edits | clean | — |

### 2.3 Its own checks on Linux

| Check | Result here | What it needs |
|---|---|---|
| `cargo build --workspace --exclude music2-desktop` | ok (1 min 12 s) | — |
| `m2-runtime`'s unit tests | **don't compile**: `core/runtime/src/tests/live.rs:491` expects `Make` (slice 4 unfinished) | finish slice 4 (not Linux) |
| `m2-server`'s unit tests | **don't compile**: `crates/server/src/graph/tests.rs:1833`, `PluginAt` has no field `1` (slice 4) | the same |
| every other test | 505 pass. The only failures are main's Linux ones: `main_thread`, `hook_tests`, `stage_tests`, `kill_tests`, the renders test | the layer's fixes |
| clippy (desktop app left out) | main's 6 Linux dead-code warnings, plus the two compile errors (`tests.md` §2 lists 15 with the desktop app's test) | — |
| browser suites (the lead's run) | 22 of 29 ok: the same failures as main | the layer's e2e fixes |
| G7f `store` and `arena` | they run, but judge Linux against M1 records, and the memory rules pass by default | the layer's memory function; records that name their machine; the verdict only on the reference Mac |
| with-plugins gates, live3b's plugin part | skipped: no Linux CLAP builds of Surge XT and Airwindows here | put them in a folder and name it in `CLAP_PATH` (the owner's call) |
| G7f on p50, the limits here | `vm.max_map_count` 1,048,576; `overcommit_memory` 0; `ulimit -v` unlimited; 48-bit virtual addresses | enough (2 to 4 renderers need 24,000 to 48,000 mappings, the survey's estimate) |

---

## 3. taste (`4804247`, the new look and 8b.1; 8b.2 unfinished)

### 3.1 OS pieces it adds or changes

| # | Piece | Where | On Linux | Relates to |
|---|---|---|---|---|
| 1 | The menu bar is now one table, `ITEMS` (menu, id, title, accelerator). Accelerators are only ⌘ or ⌃ chords, and tests say so | `apps/music2-desktop/src/menu.rs` | its 3 tests pass here | OS-54; survey-later #16 (the key table's seed) |
| 2 | New chords: ⇧⌘E Export, ⌘W Close Song, ⇧⌘W Close Window, ⌘T, ⌘⌫ Delete Track, ⌘R, ⌘L, ⌘. Stop All Sound, ⌥⌘B Browser | `menu.rs` 20-62 | they become Ctrl chords. Ctrl+Alt+B is AltGr on many European Windows layouts; Ctrl+. is IBus's emoji key on GNOME (unverified) | survey-later §3.1 |
| 3 | The app menu keeps Services, Hide, Hide Others, Show All; View ▸ Full Screen; Window ▸ Minimize, Maximize | `menu.rs` 79-148 | GTK menus (muda 0.20.0) support only Separator, Copy, Cut, Paste, Select All and About. The rest show as **greyed-out items** (`muda-0.20.0/src/platform_impl/gtk/mod.rs` 913 and 1180-1189; read, not seen). Quit sits in a menu called "music2" | OS-54 |
| 4 | "Show in Finder" in the menu; New Song's toast says "Show in Finder" or "Show Folder" | `menu.rs` 33; `NewSongDialog.tsx` 29 | the menu says Finder on Linux | OS-55 |
| 5 | Labels chosen by `isMac`: the top bar's ⌘Z, ⇧⌘Z, ⌥⌘B; Tracks' ⌘⌫ and ⌥4; the shortcuts sheet's own `chord()` and `opt()` beside `kbd()` | `TopBar.tsx` 81, 139, 148; `Tracks.tsx` 23, 423; `Shortcuts.tsx` 7-9 | they show Ctrl forms, which is right. But three helpers now say how a chord is written | OS-53, OS-74; survey-later #16 |
| 6 | Mac symbols written in, shown on every OS: the dock's tab titles ⌥1 to ⌥4, the knob tooltip "⌥-click to reset", "⌥4" in the empty timeline (main already had "⌘ scroll") | `Dock.tsx` 77; `NodeGraph.tsx` 1288; `SongTimeline.tsx` 739 | wrong on Linux | survey-later #16, #17 |
| 7 | Gestures: ⌥-click resets a knob; the song bar ignores ⌥ | `NodeGraph.tsx` 1626; `SongBar.tsx` 204 | several Linux window managers take Alt-click and Alt-drag (unverified per window manager). Headless it passes (gestures 30 of 30) | survey-later #17 |
| 8 | The traffic lights: the padding moved to the new top bar (`pl-20` when native and on a Mac); the window's smallest size is 1100×700; `traffic_light_position(18, 16)` | `TopBar.tsx` 73; `apps/music2-desktop/src/main.rs` 73-85 | native title bar above the bar, no padding: fine | OS-52 |
| 9 | Fonts: Google Fonts removed; `system-ui, -apple-system, "Helvetica Neue"`, and `ui-monospace, "SF Mono", Menlo` | `web/index.html`; `web/src/index.css` | no network needed any more. Here `system-ui` → DejaVu Sans and `monospace` → DejaVu Sans Mono (`fc-match`). taste's lint suite passes (35 of 35), and so do fx-fixes's top-bar fit checks | OS-76; taste.md §2.1 |
| 10 | `.music2/view.json`: written to a temp file, then renamed over | `crates/server/src/graph/view_file.rs` | portable. Windows can refuse a rename over a file another program holds (later) | OS-03 (Windows) |
| 11 | `layout.json` is watched by a stat (size and mtime) on every watcher tick | `crates/server/src/graph.rs` (`layout_moved`) | works with today's walking watcher (s8-tracks: within 172 ms). With inotify, ticks come from events and timers, so re-check the timing after the merge. Size and mtime can miss a same-size edit inside FAT's 2 s | OS-16, OS-20, OS-21 |
| 12 | Colour names from hex through OKLCH: `powf`, `cbrt`, `atan2` on the host | `crates/editor/src/layout.rs` 285-293 | host libm, but it only picks a colour's name for the UI. Not the render path | OS-45 (optional) |
| 13 | Browser suites: 3 new (taste, gestures, s8-tracks) and 28 changed, plus `fixlib.mjs` and `run.mjs`. All still inject `platform: "macos"` and press Meta. New Mac labels checked: "Undo (⌘Z)" (attention, start, undo), "⇧⌘E" (export), "Show in Finder" (newsong, app mode). New Meta chords: `Meta+r/t/d/Backspace/z` (s8-tracks), `Meta+a` (taste) | `e2e/*.mjs` | see §3.3 | OS-74, OS-75 |
| 14 | README and `e2e/README.md`: ⇧⌘E, Show in Finder, `~/Music` | docs | — | — |

taste doesn't touch `native.ts`, `chrome.mjs`, `run.mjs`'s Chrome path, `baseline.sh`, the commit and lock code, the
watcher's event source, the plugins, the audio device, the gates or `std.wasm`.

### 3.2 Where the layer on main meets it

| File | The layer's change on main | taste's change | Trial merge | Merge note |
|---|---|---|---|---|
| `web/src/lib/native.ts` | `isMac` follows `__M2__.platform` when given (OS-74) | untouched | clean | Do it on main. |
| `e2e/run.mjs`, `e2e/chrome.mjs`, `e2e/baseline.sh` | the Chrome path per OS (OS-73); `sha256sum` (OS-77) | `run.mjs`: only rows of `SUITES` | clean | Do it on main. |
| the browser suites, one by one (`e2e.md`'s helper) | `PLATFORM`, `MOD`, `kbd()` in every suite | rewrote most suites | **conflict in 6**: code, export, fixlib, g11-play, notes, start. taste's 3 new suites and its new Meta and ⌘ lines also need the helper after the merge | Prefer the one-place fix on main now (§3.3). Put the helper, if wanted, on taste with the key table. |
| `apps/music2-desktop/src/menu.rs` | a menu per OS (Quit in File, About in Help, no Services or Hide); "Show in Files" | rewritten | **conflict, even for that one label** | Don't touch `menu.rs` on main. The taste or 8b finisher adds the per-OS layout to `ITEMS`, reading the OS from the layer. |
| `apps/music2-desktop/src/main.rs` | the window builder's lines (OS-52) | traffic lights, smallest size | **conflict** | Don't touch the window builder on main. The main-loop install and the paths in this file merge cleanly with taste. |
| `web/index.html`, `web/src/index.css` | serve the fonts from `web/` (`e2e.md`'s "luck" item) | removed Google Fonts; the system font | **conflict** in `index.html` (`index.css`, which taste rewrote, wasn't tried) | Don't serve fonts on main. taste already needs no network. Choose the font on taste (taste.md §2.1). |
| `web/src/components/GraphCanvas.tsx` | the header's `pl-[88px]` | the header moved into `TopBar.tsx` | **conflict** | Leave it alone on main. |
| `README.md` | Linux build notes on its Mac lines | ⇧⌘E, Show in Finder | **conflict** | Write the Linux part as a new section, or after taste. |
| `docs/architecture.md` | WRITE-15, 17, 19 named for every OS | WRITE-18's *Today* (the Read only pill) | **conflict**: one line | Keep both. |
| `crates/server/src/project.rs` | `songs_dir()` from XDG Music; `reserved()` | the new song's tempo | clean | Keep `~/Music` when `XDG_MUSIC_DIR` is unset: the suites read `testhome/Music/music2`. |
| `crates/server/src/graph.rs` | `mod fsevents` (line 40); the device's start | `layout_moved`, the song bar | clean | After the inotify watcher and taste meet, re-run s8-tracks' "layout.json within a second". |
| `web/src/lib/exporting.ts`, `actions.rs` | "reveal" per OS (OS-55) | untouched | clean | — |

### 3.3 Its own checks on Linux

| Check | Result here |
|---|---|
| Rust tests | 733 pass, 12 fail: main's 10, the known long-path flake (`files::tests::diagnostics_…`, my scratch path is long), and `the_published_timeline_is_the_sessions_and_shares_what_didnt_change`. That last one is unfinished 8b.2: the published timeline lacks the session's new `"home"` field. Not Linux. The lead also saw `the_live_engine_passes_every_gate` fail while three checks ran at once; it passed here. |
| clippy (desktop app left out) | main's 6 Linux dead-code warnings only (`tests.md` §2 lists 15 with the desktop app's test) |
| the desktop app | `cargo test -p music2-desktop`: the 3 menu tests pass; `appkit_main_thread` prints "macOS only" |

The browser suites (the lead's run: 18 of 32 ok), each failure looked into:

| Suites | Why they fail here | Linux? | Fix, measured |
|---|---|---|---|
| code, fixes-graph, attention, s6-write | Monaco picks its keys from `navigator.userAgent` (`monaco-editor` 0.52.2, `platform.js` 60-62). On Linux, ⌘ chords do nothing in the editor | yes | Chrome with a Mac user agent: all pass (58, 42, 56, 36 checks) |
| undo, start, export | Tooltips say Ctrl+Z and Ctrl+Shift+E. Off the app, `isMac` reads `navigator.platform` | yes | `native.ts`'s one line: all pass (10, 24, 37) |
| fx-fixes | ext4 tells "exports 2" from "Exports 2" | yes | a product call: fold case on every OS, or `e2e.md`'s OS-aware check |
| graph, layout, live3a | No button is titled "Play / stop (Space)" any more. 8b.2 retitled it ("Play (Space) · continue…") and these suites weren't updated | no: taste's unfinished work (probably fails on the Mac too; unverified) | update the 3 suites on taste |
| views | "for the selected module (Bass)" reads the code view before it fills | a race: lost 6 runs of 6 here (1 under load, 5 alone) | wrap the check in `until()`: 30 of 30 |
| s8-tracks | failed in the loaded run | no | passed alone, 75 of 75 |
| live3b | PART: no CLAP plugins here | as on main | — |

So with the two one-place fixes on main, only fx-fixes stays a Linux failure, and it waits on a product call. The
rest are taste's own.

**What the one-place fix gives up.** The suites keep testing the Mac's keys, also on Linux. Nothing then tests Ctrl,
the key Linux users press. That test belongs with the key table (survey-later §3.1): a small PC-keys suite, on
taste.

---

## 4. sounds (`916127d`, 20 new std sounds)

### 4.1 OS pieces it adds or changes

| # | Piece | Where | On Linux | Relates to |
|---|---|---|---|---|
| 1 | `std.wasm` rebuilt (143,471 → 215,110 bytes), with 6 new kernels. It holds `/Users/jerryxu/dev/suite2/music2/core/kernel/src/lib.rs` and rustc `b940084d7` paths | `std/kernels/std.wasm` | rebuilds bit for bit here only in some folders (§4.3) | OS-64, OS-65 |
| 2 | Kernel math: oscillators, ladder filter, FM | `std/kernels-src/src/{synth,fm,drums,voice,notes}.rs` | inside wasm the math is compiler-builtins' libm, the same on every OS. The native kernel tests use the host's libm but check properties, not bits: 107 pass here (release) | OS-46 |
| 3 | Drum noise seeded from the lane's hit count, not from engine samples | `drums.rs` | portable | — |
| 4 | A new example, `examples/sounds`. It is in `bit_identity`'s "live equals the export" list, but not in `renders.json`'s `PURE` list and not in `e2e/baseline.sh` | `core/play/tests/bit_identity.rs` | live equals export here. Whether the new sounds give the same bytes on every OS is checked nowhere | OS-45, OS-46 (RENDER-2) |

No `cfg`, `libc`, paths, `web/` or `e2e/` changes.

### 4.2 Where the layer on main meets it

| File | The layer's change on main | sounds' change | Trial merge | Merge note |
|---|---|---|---|---|
| `std/kernels/std.wasm` | one deliberate rebuild, with a build that doesn't depend on the folder (OS-64) | rebuilt | **binary conflict** | Never take one side. Rebuild from the merged source with the new build. Check the fixtures render the same bits (`renders.json`, `bit_identity`). Commit the new file once. |
| `std/build.sh`, `std/kernels-src/Cargo.toml`, `.cargo/config.toml`, `core/kernel/` | the fixed build (§4.3) | untouched (sounds edits only `src/`) | clean | If `core/kernel` moves into the kernels' workspace, sounds' new files still say `m2_kernel::…`: nothing to change. |
| `core/session/tests/renders.json`, `PURE` in `gate_tests.rs` | resample re-recorded | untouched | clean | After sounds merges, add `examples/sounds` to `PURE` and `baseline.sh`. Record on Linux, check on the Mac runner. |
| `README.md` | Linux notes | the std section | clean | — |

### 4.3 std.wasm: does it rebuild the same?

| Build | Folder | sha256 (start) | Against the committed file |
|---|---|---|---|
| main, committed | the Mac, at `650f842` | `83cc596169a4c6af` | — |
| main's source, remapped to the Mac's path | two folders here | `807a28a22c7392fd`, both | 10 data bytes differ: panic line numbers. `core/kernel/src/lib.rs` changed in `933f4de` and nobody rebuilt. The code is identical. (`facts.md` §7 found the same.) |
| sounds, committed | the Mac | `15344ec7aca8054f` | — |
| sounds, remapped, folder A (`…/src-sounds-at-another-longer-path`) | here | `15344ec7aca8054f` | **identical** |
| sounds, remapped, folder B (`…/src-sounds`) | here | `1187a860d5dbce16` | same size; 188 code bytes and 815 data bytes differ (the data is laid out in another order) |
| sounds, as `std/build.sh` builds it | here | other bytes, 215,134 bytes | this machine's path is in it |
| sounds, with `m2-kernel` moved inside the kernels' workspace, no remap | two folders here | `3b675dc8de1c4c7a`, both | no machine path at all (it says `kernel/src/lib.rs`). Differs from the Mac's file: a new layout |

**Why.** Cargo gives each crate a `-C metadata` hash. `m2-kernel` is a path dependency outside the kernels'
workspace, so its hash includes its absolute path: `9d788268…` in folder B, `95dfa37e…` in folder A (measured).
That hash changes how symbols are ordered, and so the layout. `--remap-path-prefix` fixes only the strings. This
corrects `facts.md` §7 and `ci.md`, which treat a remap as the whole fix.

**A fix that measured path-independent.** Make `m2-kernel` a member of the kernels' workspace (for example
`std/kernel/`, or `std/kernels-src/kernel/`), so cargo hashes a relative path. Pin rustc in `rust-toolchain.toml`.
Then CI can check: rebuild in a second folder, `cmp` with the committed file. A fixed build folder such as
`/tmp/m2-std` would likely fail on the Mac, where `/tmp` resolves to `/private/tmp` (unverified).

### 4.4 Its own checks on Linux

| Check | Result here |
|---|---|
| Rust tests | the same 10 failures as main (the lead: 688 pass; mine: 687 pass plus the long-path flake) |
| clippy (desktop app left out) | main's 6 Linux dead-code warnings only (`tests.md` §2 lists 15 with the desktop app's test) |
| kernel tests (`cargo test --release` in `std/kernels-src`) | 107 pass |
| browser suites (the lead's run) | the same as main: 21 of 29 ok |
| before merging | re-run its full checks: its commit says the last independent check was cut short |

---

## 5. Per seam: which branch each part of the layer meets

From the trial merges. "clean" means git merged it alone.

| Seam (survey-code §17) | Lines marked on main | step7 | taste | sounds |
|---|---|---|---|---|
| math (OS-45): `audio.rs`, `resample.rs`, `Cargo.toml`, `renders.json` | 8 + 2 calls, deps, a hash | clean | clean | clean |
| threads (OS-23…31): `main_thread.rs`, probe, `inprocess.rs`, `plugin_host.rs`, the app's install, HOST-2 | as surveyed | **app `main.rs`** | clean | clean |
| dirs (OS-32…40): CLAP paths, songs, cache, bundle, session file | as surveyed | clean | clean | clean |
| files and lock (OS-01…19): `stage.rs`, `lock.rs`, `watch.rs`, `tx/mod.rs`, `render.rs`, tests, contract | as surveyed | **contract** (PLANE-5) | **contract** (WRITE-18) | clean |
| watcher (OS-20…22) | `graph.rs` 40, `watcher.rs`, tests | clean | clean | clean |
| audio (OS-59…61) | `Player::start`, the error callback, `stalled()`, `device()` | clean | clean | clean |
| gates (OS-80, 81, 68) | `server.rs`, `server/watch.rs` | clean | clean | clean |
| shell, kept small: `native.ts`, `exporting.ts`, `actions.rs` | 3 places | clean | clean | clean |
| shell, all of it: + `menu.rs`, the window builder, `GraphCanvas.tsx`, `index.html` | as surveyed | clean | **4 files** | clean |
| browser suites in one place: `run.mjs`, `chrome.mjs`, `baseline.sh` | Chrome path, `sha256sum` | clean | clean | clean |
| browser suites one by one (`e2e.md`'s plan) | `platform: "macos"`, Meta, ⌘, Finder, fx-fixes, notes | clean | **6 files** | clean |
| std.wasm (OS-64): build script, config, the file | rebuild | clean | clean | **std.wasm** |
| a new `m2-os` crate in `Cargo.toml` and `Cargo.lock` | 6 crates depend on it | **`Cargo.lock`** | clean | clean |
| README's Mac lines | 9 lines | clean | **README.md** | clean |

Between the branches themselves (measured the same way): step7 then taste conflict in `core/session/src/tx/delta.rs`,
`delta_tests.rs` and `crates/server/src/graph/tests.rs`. step7 then sounds conflict in `core/play/tests/bit_identity.rs`.
sounds then taste conflict in `docs/gaps.md` and `std/src/instruments.ts`.

---

## 6. Suggested order and rules

1. **Build the layer on main without touching taste's files.** Leave alone: `menu.rs`, the window builder in the
   app's `main.rs`, `GraphCanvas.tsx`'s header, the fonts, the README's Mac lines, and the browser suites one by one.
   Then the merges cost the five small conflicts above.
2. **Fix the browser suites in one place on main**: `native.ts` (one line), the Chrome path in `run.mjs` and
   `chrome.mjs`, and a Mac user agent for Chrome (for example a wrapper that `E2E_CHROME` names). A Ctrl-keys suite
   comes later, with the key table, on taste.
3. **libm through one small module**, plus a grep test that bans host transcendental calls in render-path files.
4. **Ship in `m2-os` what step 7 will call**: the process's memory, an audio-like thread, the thread token (or keep
   `main_thread::token()` as a re-export), and a machine description for gate records.
5. **Fix std.wasm's build first** (the kernel crate inside the kernels' workspace, a pinned rustc). Rebuild once
   on main and record that the kernels' hashes change. sounds' merge then rebuilds again.
6. **Merge step 7**, fix its two test compile errors, run the renders test, `bit_identity` and the gates, with
   records that name their machine.
7. **Merge sounds**, rebuild `std.wasm`, add `examples/sounds` to `PURE` and `baseline.sh`.
8. **Merge taste.** Then: the key table (`ITEMS` plus one web label helper) with a form per OS; the menu per OS; a
   Ctrl-keys suite; taste's 3 suites that look for the old Play title; the `until()` in views; and s8-tracks'
   `layout.json` timing on the inotify watcher.

---

## 7. Not checked here

- **The Mac side.** Nothing here ran on a Mac: not taste's Play-title suites there, not the relative-path
  `std.wasm` there, not the merged renders there.
- **The real layer.** Marker edits stood in for it (§1).
- **muda's greyed-out items.** Read in muda's source, not seen: this server has no display.
- **Keyboard clashes** (IBus's Ctrl+., AltGr, window managers taking Alt-drag): from `survey-later.md`, not checked.

## How to repeat

```sh
# Trial merges: a scratch clone with every branch, then per branch:
git clone -q --shared ~/dev/suite2/music2 clone && cd clone
git fetch -q origin 'refs/remotes/origin/*:refs/remotes/up/*'
#   ...mark lines on main, commit...
git merge --no-commit --no-ff up/taste; git diff --name-only --diff-filter=U; git merge --abort

# std.wasm in a folder of your choice (copy std/, core/kernel/ and the root Cargo.toml there first):
cd <copy>/std/kernels-src
RUSTFLAGS="-C link-arg=-zstack-size=65536 --remap-path-prefix=<copy>=/Users/jerryxu/dev/suite2/music2" \
  CARGO_TARGET_DIR=<target> cargo build --offline --release --target wasm32-unknown-unknown -v 2>&1 | grep -o 'metadata=[0-9a-f]*'

# taste's suites with the two one-place fixes: native.ts line 16 becomes
#   export const isMac = window.__M2__?.platform ? window.__M2__.platform === "macos" : /Mac/.test(navigator.platform);
# and E2E_CHROME names a script that runs chrome-headless-shell with
#   --user-agent="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/133.0.0.0 Safari/537.36"
```
