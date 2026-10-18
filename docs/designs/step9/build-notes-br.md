# Track br: build notes

Branch `br`, from `main` at `e7518f4`, 2026-10-18. The plan is [br-plan.md](br-plan.md); what changed while
building is written there, under each slice ("As built").

## In short

- **Slice 1, add an installed plugin (gaps #32):** the side browser and + Track list the installed CLAP plugins.
  Drop an effect plugin on a track, or pick an instrument plugin in + Track. One Undo each.
- **Slice 2, presets for std sounds (gaps #26, first half):** every instrument and effect card has **Presets**.
  Save the knobs under a name; load one back as one Undo step. std ships two each for Synth pluck, Synth pad,
  Synth bass, Reverb and Delay.
- **Slice 3, presets for plugins (gaps #26, second half):** a plugin's card has **Presets** too: yours, and the
  plugin's own (CLAP's preset discovery). One of its own loads on a scratch instance in the host process, never on
  the one that plays.
- **No sound changed:** 12 of 12 renders. No format bump, no SDK bump, no new crate.
- **Ready to land:** the review's big finding fixed, `main` (`mf2`) merged in, and `ci/check.sh --gates` passed
  at `1fb6caa`.
  See [Finishing the track](#finishing-the-track-2026-10-18).
- Linux timings are information only. The Mac's run is owed (PLAT-1).

## What you can do now

| You can | Where |
|---|---|
| See the installed plugins | the side browser: **Plugins** under Instruments and under Effects (folded; open one to look) |
| Add an effect plugin after a sound | drag it onto a track, or double-click it |
| Add an instrument plugin as a new track | + Track ▸ Plugins |
| Save a sound's settings as a preset | the card's **Presets** ▸ **Save as preset…** |
| Load a preset | the card's **Presets**: Yours, music2's, or the plugin's own |
| Find your presets | `~/Music/music2/Presets` |

## Decisions

| # | Decision | Why |
|---|---|---|
| D1 | The plugin list is kept per app run in the server, scanned on the first ask | A scan loads every bundle. A song with no plugins starts no host process |
| D2 | A plugin goes through the existing add routes as `clap:<id>` | It is what the code writes. No second route for one gesture |
| D3 | A plugin call always gets a `key` | Without one its settings can't be saved (HOME-6) |
| D4 | One preset format (FMT-22): one JSON file, a plugin's state inside as base64 | One file to copy or share |
| D5 | Yours live in `~/Music/music2/Presets`, fixed | Easy to find and back up; under `$HOME` (PLAT-7) |
| D6 | A load is one Tx, so one Undo | As every other edit |
| D7 | A plugin's own preset loads on a scratch instance in the host process (HOST-8) | A refused Tx or an Undo stays exact; a crash hits only the host process |
| D8 | A plugin's own presets are searched on a thread of their own, never the main thread (HOST-8) | Set by the review. A long search must not freeze plugin windows or hold up autosave |

## Finishing the track (2026-10-18)

**The review** found 3 things.

| # | Finding | Size | Done |
|---|---|---|---|
| 1 | A plugin's preset search ran on the host process's main thread. A long search froze plugin windows and held up autosave; past 30 s the watchdog could end the host process mid-song | big | Fixed (`68fc800`): the search runs on the calling thread, as the scan does. Loading a preset stays on the main thread. HOST-8 says so. Test `a_preset_search_never_runs_on_the_main_thread`, red on the old code ("1 of 1 searches ran on the plugin main thread"), then green |
| 2 | A plugin preset of yours keeps only the numbers its code wrote when saved; another number the code writes later stays, without a word | small | A line in `docs/gaps.md` |
| 3 | A failed read of a plugin's own presets is kept until the app restarts, even a short failure | small | A line in `docs/gaps.md` |

**The merge.** `main` at `c852cb3` (`mf2`: the Mac run's two fixes and the marker race) merged in (`1fb6caa`). Two
conflicts in the plugin host's messages, both sides kept: main's test-only window calls keep their numbers, and
`LoadPreset` and `Loaded` take the next free ones.

**The check.** `ci/check.sh --gates` at `1fb6caa`, word for word:

```
music2 check: PASS
commit   1fb6caab7968 on br (clean)
machine  Ubuntu 24.04.3 LTS, x86_64, 32 threads
tools    rustc 1.99.0, node v24.21.0, 37 min
plugins  /home/jerry/.local/share/m2-test-plugins/clap (the builds ci/plugins.sha256 records)
toolchain        PASS  rustc 1.99.0
web build        PASS  
build            PASS  
app              PASS  the UI connected: first-song in 1 s, with-plugins in 1 s
rust tests       PASS  1737 passed, 0 failed
clippy           PASS  
pool models      PASS  29 passed, 0 failed
std kernels      PASS  tests and clippy
std.wasm         PASS  rebuilt to the same bytes in another folder
mac lint         PASS  the macOS code type-checks and lints
browser suites   PASS  73/73 suites ok
renders          PASS  12 of 12 match e2e/renders.sha256
old engine (S)   PASS  built for G7f from a clean copy of 8aa301e, in /home/jerry/.cache/m2-check/s-8aa301e
gates            PASS  the 7 runs ci/gates.txt names
repo unchanged   PASS  
```

The first run failed only one timing row, while other tracks built (load 4 to 10): G11's "5,000 files in
assets/" case, p95 6.8 ms against 5 ms. Code br changed isn't on that path. The second run passed every row,
that case at p95 1.6 ms, and nothing passed only on a retry. Logs: `~/m2-runs/check-br/` and
`~/m2-runs/check-br.tgz`.

**Pictures** on Xvfb `:95`: `~/m2-runs/app-shots/br-1fb6caa/` (captions in `captions.txt`).

## A check by ear

About 15 minutes, on the owner's Mac, with Surge XT and Airwindows installed. The same steps are in `docs/mac-run.md`.

| # | Do this | You should hear or see |
|---|---|---|
| 117 | Click **Open an Example**, then **First song**. In the side browser, under **Effects**, open **Plugins**. | After a moment it lists the installed effect plugins, Airwindows among them. **Look again** is at its foot. |
| 118 | Drag Airwindows onto the **Bass** track. Press Space. | A message says it was added after the bass, with **Undo**. Its card shows under **Device**. The song plays on, with no click. |
| 119 | Press ⌘Z. | The plugin goes, in one step. |
| 120 | **+ Track ▸ Plugins ▸ Surge XT**. Click the new track, then **Device**. On the Surge XT card, click **Open window**. | A new track named Surge XT. Surge XT's own window opens. |
| 121 | On the Surge XT card in music2, click **Presets**. While its list loads, turn a knob in Surge XT's window. | The knob moves at once: the window never freezes. Within a second or two the list shows "Surge XT's", about 2,000 names. |
| 122 | Click one of them. Play a few notes (a MIDI keyboard, or the keyboard piano). Then press ⌘Z and play again. | A message "Loaded … into Surge XT". You hear that preset. After ⌘Z, the old sound. |
| 123 | **Presets ▸ Save as preset…**, type `My Surge`, press Return. Change a knob in Surge XT's window. Then **Presets ▸ My Surge**. | "Saved…". After the load you hear the sound you saved. `~/Music/music2/Presets` has the file. |
| 124 | **+ Track ▸ Synth pluck**. On its card, **Presets ▸ Glass**. Play notes. Press ⌘Z. | You hear the glassy pluck, then the old one after ⌘Z. |
| 125 | Tell the lead what you saw and heard at 117 to 124. | |
