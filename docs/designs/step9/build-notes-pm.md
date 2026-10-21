# Track pm: build notes

Branch `pm`, from `main` at `97b4cdf`, 2026-10-18. The plan is [pm-plan.md](pm-plan.md); what changed while
building is written there, under each slice ("Built, and where it differs").

## In short

- **Slice 1, the poly op's options:** any instrument can play one note at a time (`mode: "mono"` or `"legato"`),
  slide between notes (`glide`), and choose which note stops when too many are held (`steal`). std's lead, sub and
  bass use this one path; `std:mono` and `std:notecv` are gone. The Device card says how an instrument plays.
- **Slice 2, the wheels:** a MIDI keyboard's pitch wheel bends every note of the track it plays (2 semitones each
  way). Its mod wheel deepens the polysynth's vibrato, and reaches plugins.
- **Slice 3, MIDI files in the app:** File ▸ Import MIDI…, or drop a `.mid` file. Each part becomes a new track
  with a fitting sound, after a question when the tempos differ. One Undo takes it all back.
- **One render changed, on purpose:** `examples/sounds` (2 of 12 lines). Its one-note sounds now hear each note's
  own velocity, not one rounded to 127 steps. So std is 0.3.0 and `@m2/core` 1.4.0 (FMT-3). std 0.3.0 also holds
  `sx`'s `std:stretch` (0.2.2 there).
- **Ready to land:** `main` (tracks `ph` and `sx`) merged in, the review's big finding fixed, and
  `ci/check.sh --gates` passed at `e6019bf`. See [Finishing the track](#finishing-the-track-2026-10-18).
- Linux timings are information only. The Mac's run is owed (PLAT-1).

## What you can do now

| You can | Where |
|---|---|
| Play one note at a time, slide between notes | in a track's code: `poly(notes, voice, { mode: "legato", glide: 0.07 })`; std's Lead, 808 sub and Synth bass do |
| See how an instrument plays | Device: one line under its name ("Plays one note at a time."); all of it in the tooltip |
| Bend the notes with a keyboard's pitch wheel | any instrument the keyboard plays |
| More vibrato with the mod wheel | std's polysynth sounds; plugins get the wheel too |
| Bring in a MIDI file as new tracks | File ▸ Import MIDI…, or drop the `.mid` file below the tracks or on a track |

## Decisions

The plan's D1 to D10 hold as written. The main ones:

| # | Decision | Why |
|---|---|---|
| D1 | The options are RENDER-16's shape, word for word | the contract fixed it |
| D2 | `glide` is a value read where a slide starts; it does nothing in `"poly"` | a knob stays a cell write |
| D4 | A mono or legato voice never sleeps once started | as std's one-note sounds did: same bits, same cost |
| D7 | `std:mono` and `std:notecv` are deleted; `mono()` stays as one call | one path |
| D8 | The bend is ±2 semitones for the whole lane, a step at each block line | the smallest thing that is right |
| D10 | MIDI files follow studio's Q32 to Q36; plus Use at a later bar, and a drop on a track's row | decided before |
| R1 | A one-note pool lets go of its notes at a release, and keeps the note it sounds in its voice, as a poly voice does | one path for release and for the stuck-note checks (the review's finding 1) |

## Finishing the track (2026-10-18)

A review found 2 things.

| # | Finding | Size | What was done |
|---|---|---|---|
| 1 | A mono or legato instrument ignored RENDER-26a's release. Moved to another lane, its old note stuck, and came back when nothing else was held. A ghost kept its gate open. The old lane's bend stayed. The stuck-note checks could not see a one-note voice | big | Fixed in `db11f90`: a release drops a one-note pool's held notes and puts its wheels at rest; its voice keeps its sounding note, so `held_notes`, `active_voices` and `gates_are_owned` see it. Tests: the legato halves of `a_replaced_instrument_releases_its_notes` and `a_rewired_poly_is_kept_and_releases_its_notes`, red first |
| 2 | The pitch wheel picks a different drum on a drum kit | small | One line in `docs/gaps.md` |

Then `main` at `09a0687` (track `ph`: the plugin host process, plugin windows, latency compensation) was merged in
(`e718140`). Eight files clashed; each kept both sides. One fix the merge needed: a note delay's room counts the
lane's two wheel events. One small thing it showed is a line in `docs/gaps.md`: a poly's glide isn't delayed with
its notes behind a latent plugin.

Then `main` moved again: `83fa4e4` (track `sx`: `std:stretch` as an offline kernel, Follow tempo) was merged in
(`e5be820`). std's version clashed: 0.3.0 (pm's, the higher) over 0.2.2 (sx's), as the plan says; `std.wasm` was
rebuilt from the merged sources. `e2e/renders.sha256` keeps both tracks' changed lines (sounds from pm, resample
from sx).

Four full checks:

| # | At | What happened |
|---|---|---|
| 1 | `e718140` | Every test row passed, but the `gestures` suite (an older one) passed only on a retry: it read the pan's words a moment too soon after an ⌥-click. Fixed in `73392de`: it waits. "Repo unchanged" failed too, as these docs were being written during the run |
| 2 | `73392de` | Passed: every row, nothing only on a retry (33 minutes). Then `main` moved (`sx`) |
| 3 | `e5be820` | The `st-audio` suite passed only on a retry: it read the revision before the undo's was published. Fixed in `e6019bf`. Gate G5/G7a through the server failed on timing alone (one slow edit of 10, p95 116 ms against 100) while other tracks built; it passed in runs 2 and 4 |
| 4 | `e6019bf` | Passed: every row, nothing only on a retry (1,698 Rust tests, 70/70 browser suites, 12 of 12 renders, the 7 gate runs; 36 minutes) |

**The check's summary, word for word:**

```
music2 check: PASS
commit   e6019bfdecb3 on pm (clean)
machine  Ubuntu 24.04.3 LTS, x86_64, 32 threads
tools    rustc 1.99.0, node v24.21.0, 36 min
plugins  ~/.local/share/m2-test-plugins/clap (the builds ci/plugins.sha256 records)

toolchain        PASS  rustc 1.99.0
web build        PASS  
build            PASS  
app              PASS  the UI connected: first-song in 1 s, with-plugins in 1 s
rust tests       PASS  1698 passed, 0 failed
clippy           PASS  
pool models      PASS  29 passed, 0 failed
std kernels      PASS  tests and clippy
std.wasm         PASS  rebuilt to the same bytes in another folder
mac lint         PASS  the macOS code type-checks and lints
browser suites   PASS  70/70 suites ok
renders          PASS  12 of 12 match e2e/renders.sha256
old engine (S)   PASS  built for G7f from a clean copy of 8aa301e, in ~/.cache/m2-check/s-8aa301e
gates            PASS  the 7 runs ci/gates.txt names
repo unchanged   PASS  
```

## Pictures

From the app on the invisible screen `:95`, in `$SCRATCH/app-shots/pm-73392de/` (captions in
`captions.txt` there). Taken before the `sx` merge, which changed nothing on these screens:

| Picture | Shows |
|---|---|
| `app-4s.png` | First song as it opens |
| `app-10s.png` | groove.mid brought in below the tracks, keeping 124 BPM: Drums, Bass line and Keys 2 |
| `app-20s.png` | Bass line's Synth bass card: "Plays one note at a time." |
