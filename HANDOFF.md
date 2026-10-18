# Handoff

Written 2026-10-16, at the end of step 7 (the engine), updated after its speed work the same day, again on
2026-10-17, when step 7 landed on `main`, and again the same day for the `sounds` merge, after its review and final
check, and when it landed on `main`; again for `taste-next` (taste's look, the song bar and the Mix faders),
after its review and final checks; for 8b's screens (`8b-ui`), after their review and final check; and once more
for `polish` (the rest of the top ten), after its review and final check, and when it landed on `main`; and for
8a's offline half, after its review and final check, and when it landed on `main`; and for `next` (8b.7,
8b.10 and the settings work), after its review and final check, and when it landed on `main`; for `macfix`
(the two rows the Mac's run at `85b069a` failed), after its review and final check; and for 8a's live half (on
`8a`), after its review and final check; and for `studio` (audio, recording, MIDI files and stems, help), after its
review and final check; and for `io` (step 9's audio: disk streaming and recording), after its review and
final check; and for track `ab` (kernel ABI v1 and the sampler), after its review and final check; and for track `sx` (stretching as an offline kernel, Follow tempo), after its review and final check; and for track `ex` (one path for faults), at the end of its build.
Read this first, then `docs/architecture.md` (the contract) and the design for
whatever you pick up.

**In short**
- **Track `ex` is built** (2026-10-18), on branch `ex`, from `main` at `83fa4e4`: one path for faults (plan:
  `docs/designs/step9/ex-plan.md`; build notes: `build-notes-ex.md` beside it). Its review is next. It adds:

  | What happens | Before | Now |
  |---|---|---|
  | A sound, plugin, file or stretch breaks while the song plays | said in six ways, or not at all | one message, once; listed in the status pill's popover with its track until it plays again |
  | The same during an export, `m2 render` or `m2 bounce` | often "Exported" with a part silent | the render fails, naming the part and the code; no file |
  | The input chosen in Settings isn't plugged in | recorded from another, silently | records from another and says which (`input.missing`) |
  | A Mac's Don't Allow during a take | kept a silent take | the take is dropped and the message says why (`input.refused`) |
  | An input unplugged during a take | a note on the take | one problem, naming the input (`input.lost`) |

  Under the hood: the fault ring (`core/runtime/src/fault.rs`): 24-byte records, 256 slots, never allocates, locks
  or waits. Eight codes in the contract's table (RENDER-1, RENDER-25, RENDER-30a). No render changed: 12 of 12.
  `ci/check.sh --gates` passed at `9efece3`: every row, nothing only on a retry (1,682 Rust tests, 71/71 browser
  suites, 12 of 12 renders, the 7 gate runs; 34 minutes).
  Pictures on Xvfb :96: `~/m2-runs/app-shots/ex-9167e95/` (the popover naming a stopped sound on Hats). Owed on the
  Mac: its `ci/check.sh`, then the by-ear steps 103 to 108 (a USB microphone unplugged, and Don't Allow).
- **`sx` is ready to land** (2026-10-18): Linux green at `000dd72`; macOS owed. From `main` at `97b4cdf`, with
  `main` at `09a0687` (`ph`) merged in. Stretching as an offline kernel, and Follow tempo (plan:
  `docs/designs/step9/sx-plan.md`; build notes: `build-notes-sx.md` beside it). It adds:

  | You can now | Where |
  |---|---|
  | Keep an audio clip in time when the tempo changes; its pitch stays | right-click the clip ▸ **Follow tempo** |
  | Change the tempo while followed clips play: nothing waits for their stretch | the tempo field |

  A review found 6 things. The big one is fixed, with tests shown red first:

  | What happens | Before | Now |
  |---|---|---|
  | The cache folder can't be written (a full disk, no permission) | every stretched clip went silent, live and in exports; an export finished without them | the stretch is made in memory instead, with the same bits |
  | A stretch fails in an export | the export finished without the clip | the export fails and names the clip |

  1 small one fixed (a row in the contract's codes table), 4 are lines in `docs/gaps.md`. Under the hood: kernel ABI
  v1 settled; `std:stretch` is the first offline kernel (std 0.2.2). `examples/resample`'s 2 renders changed once (std
  0.2.2's stretch, FMT-3); 10 of 12 keep their bits. `ci/check.sh --gates` passed at `000dd72`: every row, nothing
  only on a retry (1,663 Rust tests, 70/70 browser suites, 12 of 12 renders, the 7 gate runs; 36 minutes).
  Pictures on Xvfb :98: `~/m2-runs/app-shots/sx-000dd72/` (an export with the cache folder read-only).
  Owed on the Mac: its `ci/check.sh`, then the by-ear steps 96 to 102.
- **`ph` is ready to land** (2026-10-18): Linux green at `d897cf0`; macOS unverified since `b0ebc74` (its shared
  memory, wake and plugin windows are lint-checked only). From `main` at `258275b`, with `main` at `3b447b2` (`io`)
  and at `97b4cdf` (`ab`) merged in. Step 9's first synth and plugin turns (plan: `docs/designs/step9/ph-plan.md`; build notes:
  `build-notes-ph.md` beside it). A review found 4 things: the big one fixed, with two tests shown red first; 2 small
  ones fixed (one line each, one with a test shown red first); 1 is a line in `docs/gaps.md`. The first full check
  after the `io` merge failed clippy and the mac lint on a clash the merge made (fixed). After the `ab` merge, one
  run failed only G7f arena's timing under load. Then `ci/check.sh --gates` passed at `d897cf0`: every row, nothing
  only on a retry (1,645 Rust tests, 69/69 browser suites, 12 of 12 renders, the 7 gate runs; 37 minutes). It adds:

  | You can now | What you notice |
  |---|---|
  | Play plugins in a second program | a plugin that crashes or hangs goes quiet, the app says so once, and it comes back; the song plays on |
  | Open a plugin's own window from its box | a knob turned there writes the code, latches a lane, or saves the plugin's settings (one Undo step) |
  | Use a plugin that plays late (a look-ahead limiter) | every other track waits for it, live and in exports; a song with no such plugin is unchanged |

  What the review changed that you may notice:

  | What happens | Before | Now |
  |---|---|---|
  | A plugin gets stuck while it starts, loads its settings or works in its window | the app froze for good: no edit, no save, no message | after 30 seconds music2 ends the plugin program and says so; the song plays on |
  | A plugin gets stuck during an export | "Paused for export" stayed until you quit | after 30 seconds music2 ends the plugin program and playback goes on; the export finishes with those plugins silent from there (a gap) |

  No render changed: 12 of 12 (with-plugins by G7e's 1 dB rule, as two of its base renders already differ: Surge XT).
  Pictures on Xvfb :96: `~/m2-runs/app-shots/ph-659037f/` (a frozen plugin program during an export, and after).
  Owed on the Mac: the by-ear steps 88 to 95 (plugins in their own program, a plugin's own window, a crash, an export).
- **Track `ab` (kernel ABI v1 and the sampler) is ready to land** (2026-10-18), on branch `ab`: Linux green at
  `7399cb8`; macOS owed. From `main` at `258275b`, with `main` at `3b447b2` (track `io`) merged in. It adds:

  | You can now | Where |
  |---|---|
  | Add a Sampler: it plays a WAV file on every note, higher or lower by the note | + Track ▸ Sampler, or the side browser's Sampler group |
  | Give it a file | drop a WAV on its card in Device; Undo takes the code back, the file stays |
  | Play one file for each range of notes, or stop with the note | in its code: `zones`, `mode: "gate"` |
  | Hear it | Open an Example ▸ Sampler |

  Under the hood: one sandbox a box (RENDER-32; on p50, −211 µs a block and −1,080 µs a callback against
  `main`, 75 MB against 204, Linux numbers, information only), and kernel ABI v1 frozen
  ([kernel-abi.md](docs/kernel-abi.md): v0 refused by name, and since track `sx` a later version too, read from the module's bytes first, one import `m2.asset_read`, at most 3,999 sound parts
  a renderer). std 0.2.1, `@m2/core` 1.3.0. No existing render changed: 12 of 12 (2 new, the example's).
  A review found 5 things. The big one came with the merge: a Sampler's file was decoded again at every edit; now
  it is decoded once and kept (`7399cb8`, a test shown red first). The 4 small ones are one line each in
  `docs/gaps.md`. Then `ci/check.sh --gates` passed at `7399cb8`: every row, nothing only on a retry (1,609 Rust
  tests, 68/68 browser suites, 12 of 12 renders, the 7 gate runs; 36 minutes). Pictures:
  `~/m2-runs/app-shots/ab-7399cb8/` (and `ab-sampler/`). Build notes `docs/designs/step9/build-notes-ab.md`; the
  plan `ab-plan.md` beside it. Owed on the Mac: its `ci/check.sh`, G7f's run of the new layout (its footprint and
  address space), and the by-ear steps 83 to 87.
- **`io` is ready to land** (2026-10-18): Linux green at `64380e1`; macOS unverified since `eda5e77`. From `main` at
  `258275b` (`main` hasn't moved since). Step 9's audio turns 1 and 4 (plan: `docs/designs/step9/io-plan.md`; build
  notes: `build-notes-io.md` beside it). A review found 8 things: the 3 big ones fixed, each with a test shown red
  first; 2 small ones fixed; 3 are one line each in `docs/gaps.md`. The first full check passed two older tests only on
  a retry (fixed); the second failed on a test lock this branch added (fixed) and, under load, two older timing tests
  (Known flakes). Then `ci/check.sh --gates` passed: every row, nothing only on a retry (1,592 Rust tests, 67/67
  browser suites, 10 of 10 renders, the 7 gate runs; 44 minutes). It adds:

  | You can now | Where |
  |---|---|
  | Play long audio files (up to 3 hours): they play from the disk, never whole in memory | drop or import them as before |
  | Record your voice or a guitar on an audio track | select the audio track, R (or Record); R again stops |
  | See the take grow as a red clip, with a bar for how loud the input is | the audio track's row |
  | Pick the sound input, and hear yourself while recording (off: use headphones) | Settings (⌘,) ▸ Sound input |

  What the review changed that you may notice:

  | What happens | Before | Now |
  |---|---|---|
  | A USB microphone unplugged during a take | playback stopped | the take is kept up to there, a message says so, the song plays on |
  | The disk fills up during a take | the whole take was lost | the take is kept up to there, and a message says so |
  | A take longer than 3.1 hours (48 kHz stereo) | the clip played only its start | the take ends there, and a message says so |
  | An edit that makes a new long audio clip, on a slow disk | live notes and meters could wait half a second | they never wait for the disk |

  No render changed: 10 of 10. A take is one clip and one undo step ("Undo recording on Vox"); the file stays.
  Pictures on Xvfb :98: `~/m2-runs/app-shots/io-eda5e77/` (`take`, `clip`, `settings`) and `io-8c85e92/` (`unplug`).
  Owed on the Mac: the by-ear steps 73 to 82 (a long file from the disk, a real microphone, the permission question,
  the take against the click, hearing yourself in headphones, a USB microphone unplugged).
- **`studio` is on `main`** (2026-10-18): Linux green at `ad5f111`; macOS owed. `main` fast-forwarded to it
  (`02ba4d3`). From `main` at `85b069a`, with `main` at `44b104a` (8a's live half and `macfix`) merged in. It added:

  | You can now | Where |
  |---|---|
  | Bring in WAV files | File ▸ Import Audio… (⇧⌘I), or drag them from the Finder onto the window |
  | Add an empty audio track | + Track ▸ Audio track |
  | See a clip's waveform; trim it, fade it, change its volume, play it backwards | drag its edges and the small squares at its top; its menu |
  | Record what you play into a clip, after one bar of clicks | R, or the red Record button after Play; R again stops |
  | Choose which MIDI keyboards play | Settings (⌘,) ▸ MIDI keyboards |
  | Export each track as its own file (stems), or the notes as a MIDI file | Export (⇧⌘E): Song, Stems or MIDI; a clip's menu: Export as MIDI… |
  | Learn the app step by step | Help ▸ Make Your First Song |
  | See what every button does, and its key | hold the mouse over it |
  | Read a mistake in the code in plain words, with a fix | the red banner; Details has the rest |

  Not built yet: bringing a MIDI file in from the app (as new tracks), MP3 export, stems of a song that plays plugins,
  and recording audio. Nothing you hear in an export changed: every render is the same, and the count-in never reaches
  a file. A review found 12 things: all fixed, each with a test. The first two full checks passed three older tests
  only on a retry (each read a result a moment too soon): all fixed. One gate timing also failed once, while another
  track's check timed its gates at the same time. Then `ci/check.sh --gates` passed: every row, nothing passed only on
  a retry (1,564 Rust tests, 66/66 browser suites, 10 of 10 renders, the 7 gate runs; 37 minutes). Pictures:
  `~/m2-runs/app-shots/studio-ee2012c/`. What was built and checked: `docs/designs/step8/build-notes-studio.md`; the
  plan: `studio-plan.md` beside it. Owed: the Mac by-ear steps 61 to 72 (the Finder, a real MIDI keyboard, stems in
  another app).
- **8a's live half is on `main`** (2026-10-18): Linux green at `21a7a1b`; macOS owed. `main` fast-forwarded to it
  (`44b104a`; `8a` already had `main` at `1cc7382`, `macfix` included). It adds:

  | What changed | What you notice |
  |---|---|
  | Live playback uses several cores, as exports already did | a big song plays with fewer dropouts; it sounds exactly as on one core |
  | On a Mac those threads are real-time audio threads, in the output's audio workgroup | macOS treats them as audio work |
  | A sound that breaks while the song plays goes silent alone | the rest plays on; a message says which sound, on which track, and that Transport ▸ Stop All Sound starts it again |
  | A sound over its 32 MB says it ran out of memory | the message says "ran out of memory" |
  | Gate G12 plays p50 live and judges every callback | the Mac's check plays it for a minute, muted |

  An export with a broken sound still fails and names it. No render changed: 10 of 10. A review found 6 things:
  all fixed, each with a test; so was one flaky server test. Then `ci/check.sh --gates` passed at `21a7a1b`, with
  `main` at `1cc7382` merged in: every row, nothing passed only on a retry (1,404 Rust tests, 59/59 browser
  suites, 10 of 10 renders, the 7 gate runs; 35 minutes). Its first run failed the gates row under load from
  other jobs (G5 and G7a through the server, which then passed alone 3 times of 3); the second passed, before `main`
  moved. Owed: the owner's Mac sweep, one paste (Mac runs owed, below): it
  picks the live thread count and park or spin. Build notes `docs/designs/step8/build-notes-8a.md` and
  `build-notes-8a-gaps.md`; plans `8a-plan.md` and `8a-live-plan.md` beside them.
- **`macfix` is on `main`** (2026-10-17, a fast-forward to `1cc7382`): the owner's `ci/check.sh` on the Mac at `85b069a`
  passed 11 of 13 rows. It failed G7f store (+135 µs a block over S; bound 80) and one test
  (`new_songs_go_where_settings_say`: a Mac's temporary folder is a link). macfix fixes both. `std:gain` skips its
  `powf` while its dB holds still, with the same bits. The songs folder is named by its real path, as songs are. A
  review found 2 things: both fixed (a test, and the docs). Then `ci/check.sh --gates` passed at `ac5d8c0`: every
  row, nothing passed only on a retry (1,353 Rust tests, 58/58 browser suites, 10 of 10 renders, the 6 gate runs;
  45 minutes). No render changed; std stays 0.2. Owed: one more `ci/check.sh` on the Mac, then
  G7f store once more on the old gain (Mac runs owed). Build notes: `docs/designs/step8/build-notes-macfix.md`.
- **`next` is on `main`** (2026-10-17): Linux green at `1560a19`; macOS owed. `main` fast-forwarded to it.
  It added:

  | You can now | Where |
  |---|---|
  | Hear a clip, a note or an automation point where you drag it, while the song plays | drag it; Esc puts it back |
  | Hear the tempo as you drag it, playing or not | the tempo in the top bar |
  | Turn on a click on each beat | the Metronome button after Loop, or K |
  | Play the selected track with the computer's keys | ⌘K: A is middle C, Z and X change the octave |
  | Play it with a MIDI keyboard, as hard as you press, with the pedal | plug one in; a mark shows in the top bar |
  | Open a song with no sound device | it plays silently; plug one in and it moves there |
  | Choose the sound output, its buffer size and where new songs go | Settings (⌘,) |
  | See what Undo will take back | Undo's tooltip and the Edit menu: "Undo Hats volume" |
  | Turn an effect off, move it, pick what a Duck listens to | the effect card's ⋯ menu |

  Nothing you hear in an export changed: every render is the same, and the click and live notes never reach a file.
  A review found 12 things. All are fixed, each with a test (one more came up while fixing them, also fixed). Then
  `ci/check.sh --gates` passed: every row, nothing passed only on a retry (1,353 Rust tests, 58/58 browser suites, 10 of 10 renders, the 6 gate runs; 44 minutes). Pictures:
  `~/m2-runs/app-shots/next-1560a19/`. What was built and checked: `docs/designs/step8/build-notes-next.md`; the plan:
  `next-plan.md` beside it. Owed: the Mac by-ear steps 51 to 60 (a real MIDI keyboard, the metronome, the outputs).
- **`polish` is on `main`** (2026-10-17): Linux green at `65f59f1`; macOS owed. `main` fast-forwarded to it. It
  added:

  | You can now | Where |
  |---|---|
  | Mute, solo and see the level of each track in the Mix view | M, S and a meter on every strip; a red dot when it gets too loud |
  | Change a track's instrument | Track → Change Instrument…, or the track's menu |
  | See drum names in Notes | Kick, Snare, Closed hat on a drum kit's rows |
  | Zoom the timeline, make tracks taller, follow the playhead | ⌘= ⌘−, a pinch, Z; drag a track's bottom edge; on while playing |
  | Put markers on the ruler | right-click the ruler, Add marker; ⌥← ⌥→ |
  | Repeat a clip | Repeat… in its menu |
  | Open recent songs; the last one opens at launch | the start screen, File → Open Recent |
  | Save a copy, rename a song and its folder | File → Save As… (⇧⌘S), File → Rename Song… |
  | Change a track's ID | the Code view, right-click its file |
  | Edit notes with a menu | right-click a note: transpose, Quantize, Legato, Humanize, Swing |
  | Set the song's key | Key, after the time signature; Notes shades the rows not in it |
  | Draw a filter sweep on a log scale; hide one automation lane | the lane's menu |

  Your view settings (zoom, heights, grids) are kept per song now. Nothing you hear changed: every render is the
  same. A review found 9 things (one twice). All 8 are fixed, each with a test; so are three more that the
  pictures, a sweep and the first full check found. Then `ci/check.sh` passed: every row, nothing passed only on a retry (1,184 Rust tests,
  49/49 browser suites, 10 of 10 renders). Pictures: `~/m2-runs/app-shots/polish-c3dd95a/`. What was built
  and checked: `docs/designs/step8/build-notes-polish.md`; the plan: `polish-plan.md` beside it.
- **`taste-next` is on `main`** (2026-10-17): Linux green at `22bea45`; macOS owed. Taste's new look (one top bar, the
  dock with Device, Notes, Mix and Code, the side browser, Inter off the Mac), 8b.1 (+ Track with the 22 new sounds,
  duplicate, delete, reorder, recolour), 8b.2 (the song bar: tempo, time signature, the ruler's changes, the name, the
  transport) and 8b.3 (the Mix view: faders and pans in the code), merged with `main` at `e571a5f`. A review's 17
  findings (16 problems) are all fixed, each with a test or in the docs. `ci/check.sh` and `ci/check.sh --gates`
  passed at `22bea45`: every row, nothing passed only on a retry (1,130 Rust tests, 35/35 browser suites, 10 of 10
  renders, the gates). No render changed. `main` fast-forwarded to it. Build notes:
  `docs/designs/step8/build-notes-taste.md`; pictures for the owner: `~/m2-runs/app-shots/taste-22bea45/`.
- **The `sounds` merge: Linux green at `e108e46`; macOS owed.** It is on `main` (2026-10-17). It brings 22 new std
  sounds (a polysynth, FM keys and bells, a drum machine), fixes O3 (std's chorus, phaser and tremolo wobble only
  while the song plays), and moves std to 0.2. No existing render changed. A review's 12 confirmed findings (8
  problems) are all fixed: one test, one hint moved first in its message, the rest docs. Then `ci/check.sh --gates`
  passed at `e108e46`: every row, nothing passed only on a retry (922 Rust tests, 29/29 browser suites, 10 of 10
  renders, the 6 gate runs). So did the recorded tests, 5 of 5. Its Mac run is owed (below); one run covers step 7
  too.
- **The back ends of 8b.4, 8b.5, 8b.6 and 8b.8 are on `main`** (2026-10-17; branch `8b-backend`): mute, solo and
  mute in song; clips by id (move, resize, split, join, quantize, paste) and Remove automation; audition (notes
  played from a window); meters and the DSP load. Their screens are on `8b-ui` (below). Reviewed (6
  findings, all fixed with tests); `ci/check.sh --gates` passed at `a8199ff`, every row, nothing passed only on a
  retry (1039 Rust tests, 29/29 browser suites, 10 of 10 renders, the 6 gate runs). macOS owed. Plan:
  `docs/designs/step8/backend-plan.md`; what was built and what the UI uses:
  `docs/designs/step8/build-notes-8b-backend.md`.
- **8b's screens (`8b-ui`) are on `main`** (2026-10-17): Linux green at `92317ad`; macOS owed. What you can do now:
  - mute and solo tracks (M and S on each track, and a pill at the top that counts them);
  - mute a track in the song; Export asks first while a mute or solo changes what you hear;
  - see how loud each track and the song are (meters); the status pill says "Heavy load" and
    "Sound dropped out";
  - edit clips on the timeline (move, copy, resize, split, join, paste, quantize), with a grid;
  - draw automation on the timeline, and hear notes as you click them in Notes.
  A review found 9 things (one twice). All 8 are fixed, each with a test. The full check found
  two old races in the page; both are fixed too. Then `ci/check.sh` passed: every row, nothing
  passed only on a retry (1132 Rust tests, 40/40 browser suites, 10 of 10 renders). Pictures:
  `~/m2-runs/app-shots/8b-ui-92317ad/`. What was built and checked: `docs/designs/step8/build-notes-8b-ui.md`.
- **One new question for the owner:** after Stop and back to bar 1, should the wobble start again as in the
  export? For now it carries on (Open owner decisions).
- **Your own songs** (for the owner): a song made before std 0.2 must change one line to build (below).
- **Step 7 is on `main`** (2026-10-17): Linux green, and macOS green at `cbac57c`; its by-ear steps 7 to 17 are
  still owed. With its speed work it is done on Linux at
  `ffc36aa`, after a review of the speed work's fixes: `ci/check.sh` passes there, every row, with nothing passed
  only on a retry, once without the gates and once with them (912 Rust tests, 29/29 browser suites, 8 of 8
  renders, the old engine S built, the 6 gate runs; `~/m2-runs/check-s7-rv1`, `-rvg`). Before that review it
  passed at `a894448` (`-sp1`, `-sp2`, `-spg`).
- **macOS: green at `cbac57c`** (ci/check.sh on MacBookPro18,1, macOS 26.6.2, 2026-10-17), every row on the first
  run. G7f, which failed at `ed7fc8e` (+328 µs a callback against 320), now reads −19 µs a block (bound 80) and
  −196 µs a callback (bound 320) against S, measured in the same run. Its starting line read −184 µs a callback, so
  about 90 µs of that leans the app's way: still about 400 µs inside the bound. S measured 14% slower than its
  record of 2026-10-14 (10,169 µs a callback against 8,936). Why is not known; the paired turns judge both sides
  under the same conditions, and the verdict is far from its bound, so no second run was asked for.
- **macOS at `85b069a`: 11 of 13 rows** (ci/check.sh on MacBookPro18,1, macOS 26.6.2, 2026-10-17;
  `~/m2-runs/mac-runs/check-85b069a.txt`). 1,338 Rust tests passed and one failed (`new_songs_go_where_settings_say`,
  `songs_folder.rs:38`); 58/58 browser suites; 10 of 10 renders. Gates:

  | Gate | The Mac at `85b069a` | Bound |
  |---|---|---|
  | G11 tx-data, p95: before the preview / to the first block | idle 2.7 / 8.0 ms; with a syncing writer 2.1 / 8.3; 5,000 assets 3.2 / 8.9 | 5 / 20 ms |
  | G11 watcher | 0.21% of a core | 1% |
  | G7f store: a block over S, by the mean | **+135 µs** (5 pairs, −65 to +160): FAIL. P1 +177 over S, the app's layout −46 over P1; footprint 581 MB against S's 554; build 126 ms against 122 | 80 µs |
  | G7f arena: a callback over S, by the mean | −69 µs (5 pairs, −154 to +253); the slowest activation block 3,999 µs | 320 µs; 5,333 µs |

  The same music in both. S measured 10,589 µs a callback, 18% over its record (14% at `cbac57c`). G7f store read
  −19 at `cbac57c`: the +154 since is in what the app and P1 share, not the layout. What it is made of is not
  pinned (`build-notes-macfix.md`). `macfix` fixes both rows.
- `main` has step 7: it fast-forwarded from `0ebc6e6` (the platform step, macOS green at `1750322`).
- **A layout to decide before step 9:** idea 3, one sandbox per box (`docs/designs/kernel-layout/report.md`).
  Research only; it is in Open owner decisions.
- The owner decided O1 to O5 on 2026-10-15 (below), and std's bump to 0.2 with O3's fix. That question is closed.
- **8a's offline half is on `main`** (2026-10-17): Linux green at `bc41693`; macOS owed. `main` fast-forwarded to
  it. Exports, `m2 render` and `m2 bounce` use several cores, with the same bits on any number
  (`--threads N` or `M2_THREADS` to choose). On this server p50 renders about 2.9 times faster on 4 threads.
  Live playback still uses one thread; a song with plugins renders on one thread for now. A review found 4 things:
  all fixed, each with a test. The last check found a fifth (gate G10's timing): fixed too. Then
  `ci/check.sh --gates` passed: every row, nothing passed only on a retry (1,241 Rust tests, 49/49 browser suites,
  10 of 10 renders, the 6 gate runs). No render changed. The owner ran its offline sweep on the Mac the same day:
  go (`build-notes-8a.md`, "The Mac sweep"). Its live half is done on `8a` (above). Build notes
  `docs/designs/step8/build-notes-8a.md`; the plan `8a-plan.md` beside it.

## Your own songs (for the owner)

**In short:** a song you made before std 0.2 needs one line changed. Then it plays as before.

| | |
|---|---|
| Which songs | Songs in `~/Music/music2` that you made before the `sounds` merge lands on `main`. |
| What you see | The song opens but stays silent. The yellow bar at the top says the code doesn't compile. Its error starts: "This app has std 0.2. To use it, change the std line in music2.toml to std = "0.2". The song may sound a little different." To read all of it, hold the mouse over it. |
| What to do | Open the song's `music2.toml` in a text editor. Change the std line to `std = "0.2"`. Save. The app takes the change in, as with any file. |
| The sound | It doesn't change: std 0.2 changed no sound a song already had. The message says "may" because later std versions might. |

## Where everything is

| Branch | Commit | State |
|---|---|---|
| `main` | the tip | Steps 1–7, the fast note edits, the platform layer, the `sounds` merge, taste's look with 8b.1 to 8b.3, 8b.4 to 8b.6 and 8b.8 with their screens, and `polish` (the rest of the top ten). Green on Linux and macOS at `1750322` (the platform step). Step 7: green on Linux at `ffc36aa` and on macOS at `cbac57c`. macOS at `85b069a`: 11 of 13 rows (G7f store and one songs-folder test failed; `macfix` fixes both). |
| `sounds-merge` | `e1003ba` | All on `main` (a fast-forward, 2026-10-17). Linux green at `e108e46` (reviewed; `ci/check.sh --gates` and the recorded tests), macOS owed. Branch and worktree deleted. |
| `8b-backend` | `c9cc6ad` | All on `main` (a fast-forward, 2026-10-17). Linux green at `a8199ff`, macOS owed. Branch and worktree deleted. |
| `taste-next` | `22bea45`, then docs | All on `main` (a fast-forward, 2026-10-17). Linux green (reviewed; `ci/check.sh` and `--gates`), macOS owed. Branch and worktree deleted. |
| `8a` | `21a7a1b`, then docs only | The offline half is on `main` (a fast-forward, 2026-10-17, Linux green at `bc41693`): the pool (`m2-pool`) and the executor, so exports use several cores with the same bits. **The live half is on `main` too** (a fast-forward to `44b104a`, 2026-10-18): R1 (a mixer strip runs with its track), live playback on engine threads (real time, the Mac's workgroup), G12 live (`ci/gates.txt`, `ci/sweep.sh`), and the gaps lane (a sound that breaks goes silent alone and is told; out of memory says so). Reviewed (6 findings, all fixed with tests). Linux green at `21a7a1b` (`ci/check.sh --gates`, nothing passed only on a retry), macOS owed. Branch and worktree deleted. Plans `docs/designs/step8/8a-plan.md` and `8a-live-plan.md`; build notes `build-notes-8a.md` and `build-notes-8a-gaps.md`. |
| `polish` | `65f59f1`, then docs only | All on `main` (a fast-forward, 2026-10-17): the rest of the top ten, built in four lanes. Linux green at `65f59f1` (reviewed; `ci/check.sh`), macOS owed. Branch and worktree deleted. Plan `docs/designs/step8/polish-plan.md`; build notes `build-notes-polish.md`. |
| `8b-ui` | `92317ad`, then docs only | All on `main` (a fast-forward, 2026-10-17). 8b's screens: mute and solo, clips and automation, audition, meters. Linux green at `92317ad` (reviewed; `ci/check.sh`), macOS owed. Branch and worktree deleted. |
| `origin/taste` | `4804247` | All on `main` now (`a0d5f91` and `4804247`, both merged into `taste-next`). Its worktree is deleted. |
| `origin/sounds` | `916127d` | 22 new std sounds (8 on the polysynth, 4 FM, 10 drums), merged; on `main` since 2026-10-17. |
| `next` | `1560a19`, then docs only | All on `main` (a fast-forward, 2026-10-17). From `main` at `70f9b61`, with `main` at `8a6d9e1` (8a's offline half) merged in. 8b.7 (heard while dragging: lanes and the tempo), 8b.10 (the metronome, the keyboard piano, a MIDI keyboard through `midir`), no sound device, Settings, undo names, effect bypass, order and the Duck picker, small fixes. Built in four lanes, joined as merge commits. Reviewed (12 findings, all fixed with tests). Linux green at `1560a19` (`ci/check.sh --gates`, nothing passed only on a retry), macOS owed. Branch and worktree deleted. Plan `docs/designs/step8/next-plan.md`; build notes `build-notes-next.md`. |
| `macfix` | `ac5d8c0`, then docs only | From `main` at `85b069a`. Fixes the two rows the Mac's run at `85b069a` failed: G7f store (`std:gain` skips its `powf` while its dB holds still, the same bits) and `new_songs_go_where_settings_say` (the songs folder by its real path). Reviewed (2 findings, both fixed). Linux green at `ac5d8c0` (`ci/check.sh --gates`, nothing passed only on a retry); macOS owed. All on `main` (a fast-forward to `1cc7382`, 2026-10-17). Branch and worktree deleted. Build notes `docs/designs/step8/build-notes-macfix.md`. |
| `studio` | `ad5f111`, then docs only | All on `main` (a fast-forward to `02ba4d3`, 2026-10-18). From `main` at `85b069a`. Audio in the app (#18), recording notes (#24) with the count-in (#19) and the MIDI keyboards setting (#31), MIDI files and stems (#36), help in plain words (#11), `m2 import` (#37). Built in four lanes, joined as merge commits. Reviewed (12 findings, all fixed with tests). Linux green at `ad5f111` (`ci/check.sh --gates`, nothing passed only on a retry), macOS owed. Branch and worktree deleted. Plan `docs/designs/step8/studio-plan.md`; build notes `build-notes-studio.md`. |
| `io` | `64380e1`, then docs only | **Ready to land.** From `main` at `258275b`. Step 9's audio turns 1 and 4: disk streaming (RENDER-30) and recording audio (RENDER-30a, MON-8). Reviewed (8 findings: 3 big ones fixed with tests, 2 small ones fixed, 3 in gaps.md). Linux green at `64380e1` (`ci/check.sh --gates`, nothing passed only on a retry), macOS unverified since `eda5e77`. Worktree `~/m2-runs/wt/io`. Plan `docs/designs/step9/io-plan.md`; build notes `build-notes-io.md`. |
| `ph` | `d897cf0`, then docs only | **Ready to land.** From `main` at `258275b`, with `main` at `3b447b2` (`io`) and `97b4cdf` (`ab`) merged in. Step 9's first synth and plugin turns: the plugin host process (HOST-3), plugin windows (HOST-7, MON-6) and latency compensation (RENDER-17 to 21). Reviewed (4 findings: the big one fixed with tests, 2 small ones fixed, 1 in gaps.md). Linux green at `d897cf0` (`ci/check.sh --gates`, nothing passed only on a retry), macOS unverified since `b0ebc74`. Worktree `~/m2-runs/wt/ph`. Plan `docs/designs/step9/ph-plan.md`; build notes `build-notes-ph.md`. |
| `ab` | `7399cb8`, then docs only | **Ready to land.** From `main` at `258275b`, with `main` at `3b447b2` (`io`) merged in (`c68d203`). Kernel ABI v1 (ABI-2, ABI-3, ABI-5), one sandbox a box (RENDER-32), and std 0.2.1's Sampler. Reviewed (5 findings: the big one fixed with a test, 4 in gaps.md). Linux green at `7399cb8` (`ci/check.sh --gates`, nothing passed only on a retry), macOS owed. Worktree `~/m2-runs/wt/ab`. Plan `docs/designs/step9/ab-plan.md`; build notes `build-notes-ab.md`. |
| `sx` | `000dd72`, then docs only | **Ready to land.** From `main` at `97b4cdf`, with `main` at `09a0687` (`ph`) merged in (`705f24a`). Step 9's stretching turn: kernel ABI v1 settled (ABI-3), `std:stretch` as the first offline kernel (ABI-2), stretches made in the background (RENDER-30), and Follow tempo. Reviewed (6 findings: the big one fixed with tests, 1 small one fixed, 4 in gaps.md). Linux green at `000dd72` (`ci/check.sh --gates`, nothing passed only on a retry), macOS owed. Worktree `~/m2-runs/wt/sx`. Plan `docs/designs/step9/sx-plan.md`; build notes `build-notes-sx.md`. |
| Mac runs owed | — | `ci/check.sh` ran on `main` at `85b069a`: 11 of 13 rows (`macfix` fixes the two). One more on `main` at `02ba4d3` covers every branch above; then G7f store on the old gain, and 8a's Mac sweep (`ci/sweep.sh`), one paste. Then the by-ear steps in [The Mac run](#the-mac-run-for-the-owner): 7 to 72, 73 to 82 once `io` is on `main`, 83 to 87 once `ab` is, 88 to 95 once `ph` is, and 96 to 102 once `sx` is. |

`taste` and `sounds` start at `8aa301e`, before the platform step. Each has a detached worktree in `~/m2-runs/wt/`
(`taste`, `sounds`); `sounds`' can go once the tracks in flight land. `origin/wip/step7` (the Mac's half-done step 7,
all of it in `step7` as `c8117c3`) and its worktree were deleted when step 7 landed; `step7` and `origin/step7`
after the Mac run passed.

Designs, all in `docs/designs/`:
- `step7/design-7.md` (revision 2, with dated notes from the review; §18 the slices, §19 the owner's questions).
- `step7/build-notes.md`: what each part of step 7 built, every check and gate with its numbers, and what is left.
  Its sections: Rebase, Bug 1, Slice 4, m2-os process, Integration, Review and fixes, The independent check of
  the fixes, Finish, Speed (2026-10-16: the fresh comparison with S, the profile, the cut, the review, the checks,
  the Mac estimate).
- `step7/g7f-linux.md`: the study behind the owner's Store layout (O1).
- `sounds/design.md`: the `sounds` merge, O3's fix and std 0.2, as built, and why. `sounds/build-notes.md`: each
  slice, its checks, the red-first runs, and the records.
- `kernel-layout/report.md`: idea 3, one sandbox per box (2026-10-16). Research only, for the owner to decide
  before step 9. Its notes and sources sit beside it.
- `platform/design.md` (§8: how later steps add to `m2-os`; §13.3: the merges) and `platform/build-notes.md`.
- `step8/design-8a.md`, `design-8b.md`, `taste.md` (the look), `audit-ui.md` (98-shot audit).
- `step8/8a-plan.md` (8a's offline half: its plan, Attack and Decisions), `8a-live-plan.md` (the live half's) and
  `build-notes-8a.md` (each slice, both reviews and final checks, the Mac sweep, the Mac command);
  `build-notes-8a-gaps.md` (the gaps lane).
- `step6/`: the Tx and write path (C1–C21, cited by the contract). `workflows/`: old workflow scripts (Mac paths).

The step 7 review: `~/m2-runs/review-b1/findings.json` (19 confirmed findings, 5 dropped, each lens's summary).
Every confirmed one is fixed; fit-3 (stale *Today* lines) was done in the finish. The speed work's plan, attack,
profile and review: `~/m2-runs/speed/` (`plan.md`; `prof/results.md`; `review-rt`, `review-gate`; 11 confirmed
findings, all fixed). A review of those fixes confirmed 13 more, all fixed (the build notes' "The review of the
fixes"; the runs and scripts in `~/m2-runs/speed/rv/`).

## The owner's decisions

Step 7, 2026-10-15:

| # | Decision | Where |
|---|---|---|
| O1 | The Store layout: each kernel memory reserves 32 MiB, with a 64 KiB guard, no room to grow, and never moves. Cranelift's Spectre guard is off: kernels import nothing, so they have no clock. Revisit before kernels from other people's packages run. The Mac run at `cbac57c` passed G7f with it. At `85b069a` G7f store failed, +135 µs a block against 80, but not in the layout: P1 read +177 over S, the app's layout −46 over P1. | [RENDER-32](docs/architecture.md#engine-shape-step-7); `Layout::P2` in `core/runtime/src/packages.rs` |
| O2 | G7f part 2's bound is 3% of a callback: 320 µs at 512 frames. Part 1's stays 80 µs a block (3% of a 2.67 ms block). | RENDER-32. G7f judges the **mean** of 5 pairs, not the p99: see "Open owner decisions". |
| O3 | std's chorus, phaser and tremolo LFOs run on engine time, so after a stop they differ from the export. Fixed at the `sounds` merge, not in step 7, with std's version bumped. | Fixed on `main` (the `sounds` merge): [RENDER-6](docs/architecture.md#live-engine-and-its-two-clocks), `docs/designs/sounds/design.md` |
| O4 | A note held across an export plays again after the resume: a note-on at the resume, under the 5 ms fade in. | [RENDER-1](docs/architecture.md#offline) |
| O5 | Slice 3's two additions beyond the design stay: a new node's inputs fade in over 5 ms, and `text::slide`. | RENDER-25a, ID-13 |

std 0.2 and the lockfile's trigger, 2026-10-15/16 (moved here from Open owner decisions): O3's fix comes with
std 0.2, as the owner chose. Every fixture and template moves with it (no backwards compatibility), and one std
stays bundled. The lockfile and several bundled std versions wait for "the first std release, after people have
songs, that changes existing renders" (the lead's words, 2026-10-16; [FMT-3](docs/architecture.md#manifest-and-versions)
and the contract's Deferred list). Done in the `sounds` merge: std 0.2 changed no existing render.

Step 7, 2026-10-16, after the Mac run at `ed7fc8e`: speed the engine up first, with no change to any sound; make
G7f measure the whole cost fresh, against S in the same run; then one more Mac run (about 45 minutes at most). O1
and O2 stay as they are.

Decided earlier, and still in force:
- Step 7's Q1–Q9 as design-7 §19 recommends (a SiteId is a call's `(` point; export is a tape pause).
- 8a: gate 8 on this M1 Pro is "provisional" until a base M1 exists; the device buffer keeps its default.
- 8b: tempo drag lets audio drift until release; a MIDI keyboard slice (8b.10) is added; the level slider lives only
  on the Mix strip. The data plane keeps its 20 ms budget, met by playing the edit first (PLANE-5).
- Platform (2026-10-14): a new song's name is taken in any capitals, on every computer (Q1). On Linux the browser
  suites tell Chrome it is a Mac; a Ctrl suite comes with `taste`'s key table (Q2). The server keeps its host name
  (Q3). No hosted Mac runners: the owner runs the check on the Mac at the end of each step. The desktop app may open
  here on an invisible Xvfb screen, once per step, with screenshots sent to the owner.

## Open owner decisions (ask, with a recommendation)

| Question | What was built | Recommendation |
|---|---|---|
| G7f: O2 named a p99 rule. | The mean is judged against the same bounds; the p99's estimate is printed beside it. The p99 moved by hundreds of µs a block (thousands a callback) from one pair to the next; the mean by about 12 µs. It is more lenient than a p99 rule. | Keep the mean. |
| A plugin that keeps failing. | It is started again once. If the new one fails within 10 s, it stays silent until the song is opened again, and the user is told once. | Keep 10 s. |
| The output device changes while playing (headphones in or out). | Playback stops, and a message says why: "Press Play to go on". | Keep it: music shouldn't jump to the speakers on its own. |
| The kernel layout (idea 3): one sandbox per box, with wasmtime's own 4 GiB layout. It undoes the 32 MiB layout of O1 (2026-10-15). | Nothing: research only (`docs/designs/kernel-layout/report.md`). | Decide before kernel ABI v1 freezes in step 9. If yes, build it after 8a, after a short test build that measures it. |
| A songs folder reached through a link (`~/Music` moved to another disk, or a folder in `~/Dropbox`). | Settings, the start screen and New Song show it at its real place (say `/Volumes/Big/Music/music2`), as songs made there are named: one folder, one name everywhere (`macfix`). `settings.json` keeps it as chosen. | Keep it. If you'd rather read `~/Music/music2`, the folder can be resolved only to compare. |
| Space, Return, Space: stop, back to bar 1, play. Should the wobble and the drums' noise start again as in the export? | Not now: they carry on from where they stopped, as at any jump. So that play may differ a little from the export (RENDER-6 allows it). Stop, then Space, carries on smoothly either way. | Keep it (the lead's default, 2026-10-16). Ask once, in these words: "After you stop and go back to bar 1, the wobble carries on from where it was; should it start again as in the export?" Changing it changes no export. |

## The check: `ci/check.sh`

One script checks everything, on the server and on the Mac ([PLAT-6](docs/architecture.md#platforms)). It ends with a
short summary. Its first line says `music2 check: PASS`, `FAIL`, or `PART`. PART means no row failed, but the gates
couldn't judge G7f: the Mac isn't the one S's records name. Only PASS is a pass.

| Row | What it checks |
|---|---|
| toolchain, web build, build | Rust 1.99.0 (pinned), the web UI, every binary |
| app | Linux only: the desktop app on an invisible screen (Xvfb), with `first-song` and `with-plugins`, a screenshot of each |
| rust tests | every Rust test, with a scratch `HOME` (922 at the `sounds` merge's `e108e46`; 912 at step 7's `ffc36aa`); G7c and G7d are tests here |
| clippy | the whole workspace; a warning is an error |
| std kernels, std.wasm | the kernels' tests and lints; `std.wasm` rebuilt to the same bytes in another folder |
| mac lint | Linux only: the Mac-only code, type-checked and linted |
| browser suites | 29 suites (1,049 checks), each on a fresh headless server |
| renders | 10 offline renders against `e2e/renders.sha256`, one record for every OS (8 before the `sounds` merge added `examples/sounds`) |
| old engine (S) | with the gates: builds S's runner, the old engine G7f measures beside the app (`crates/gates/s/build.sh`, outside the repo); about 3 minutes the first time, seconds after |
| gates | on the Mac always; on Linux only with `--gates` (information only, PLAT-1): the 6 runs `ci/gates.txt` names (below) |
| repo unchanged | the check changed nothing in the repo |

- Without the test plugins it says FAIL, even when no row failed.
- Edit no tracked file while it runs: its last row compares `git status` before and after.
- A test or suite that failed, then passed on its retry, is listed under "passed only on a retry". The lead decides
  whether it is a known flake (below) or a real failure, and writes which here.

The gates (`ci/gates.txt`, one `m2-gates` run a line, all in release):

| Run | Gates | Rule, judged on the Mac | Linux at the last checks (information) |
|---|---|---|---|
| `--server tx-data watcher` | G11: a piano-roll write; the watcher | heard within 20 ms at p95; under 1% of a core | p95 8.8 to 9.4 ms; 0.10% |
| `--server --reference 50 --at 180 add-effect` | G5 and G7a through the server: an effect added on p50 at 3:00 | heard within 200 ms of the request, compile included; no voice pool made new | p95 106 to 116 ms |
| `--reference 50 --at 180 add-effect tails` | G5, G7a's engine-rig twin, G7b | no jump back; no click in the edit's own sound; untouched tracks bit-identical | pass |
| `--server export` | G7e: an export of `with-plugins` | pauses, borrows its 2 plugins, resumes within 21.3 ms with no click, the held chord sounds again, the file matches `m2 render` | pass; paused about 255 ms |
| `store --reference 50` | G7f part 1: what the Store layout costs a block, and memory, against S (the commit before step 7) measured in the same run | mean block within S + 80 µs; footprint within S + 64 MB; build within 1.5 × S; 2 renderers within 25% of the address space; the same music | +20 µs a block (+16 held on one die); 0.29% of the address space |
| `arena --reference 50 --at 180` | G7f part 2: a 512-frame callback with the arena, against S measured in the same run | mean callback within S's + 320 µs; slowest activation block under 5,333 µs; the same music | +86 µs a callback (+48 held); 2.3 to 3.1 ms |

Recorded tests, too long for the check (`#[ignore]`): G7c on all of p16 and of p50, p16 looping ten minutes, and the
churn test's 200 edits on p16 (within +20 MB). Run them with `cargo test -p m2-play --release --test bit_identity
--test live --test churn -- --ignored --nocapture` (about 10 minutes; all 5 passed at `18966cd`, after the kept
memory address, and at `e108e46`, the `sounds` merge's tip).

**On the server** (about 10 minutes with a warm `target/` on a quiet server; about 30 with `--gates`, since the
`sounds` merge):

```sh
cd ~/dev/suite2/music2
TW_WAIT=3500 ~/m2-runs/tw check-<label> 'M2_CHECK_OUT=$HOME/m2-runs/check-<label> ci/check.sh'
```

`M2_CHECK_OUT` must be a new folder. It gets `summary.txt`, every log, and the app's two screenshots
(`app-first-song.png`, `app-with-plugins.png`); all logs are also packed in `<folder>.tgz`. Send the owner the
screenshots once per step (step 7's: `~/m2-runs/app-shots/step7-6297c9a/`; the `sounds` merge's: Pop sounds in
`~/m2-runs/app-shots/sounds-e108e46/pop-sounds.png`, and the check's two in `~/m2-runs/check-sm-fg/`).

## The Mac run (for the owner)

About 35 minutes, mostly waiting *(an estimate: 25 at `ed7fc8e`; this time it also builds the old engine
S, about 3 minutes the first time, and G7f plays longer, about 8 minutes a part)*. All it checks is on `main`, pushed.

| # | Do this | Why |
|---|---|---|
| 1 | Plug the Mac in, keep the lid open, and quit other apps. | The speed tests need a quiet Mac. |
| 2 | Open Terminal on the Mac itself, not over ssh. | One test needs your login session. |
| 3 | Type `cd ~/dev/suite2/music2`, then `git fetch`, then `git switch main`, then `git pull`. | This gets everything on `main`. |
| 4 | Type `ci/check.sh` and press Return. | It runs every check and the speed tests. |
| 5 | If one line says `running` for more than 30 minutes, press Ctrl-C once. | It stops that check and still prints the summary. |
| 6 | Copy the last lines, from `music2 check:` to the `logs` line, and paste them back. | That is all the lead needs. |

**What the next run adds** (the run at `cbac57c`, 2026-10-17, passed step 7):
- The `sounds` merge: 22 new std sounds, and std 0.2. Two more renders (10). `std.wasm` is half as big again, so
  each load of std takes a little longer (+70 ms on Linux) and the check a few minutes more.
- 8b's back ends: mute, solo, clips, audition and meters, with about 130 more Rust tests (1,039 on Linux). A mute
  puts a gain node where each track is read, in every plan, so G7f plays a little more.
- `taste-next`: the new look and its screens, with about 90 more Rust tests (1,130 on Linux) and 6 more browser
  suites (35): `taste` (the look's lint), `gestures`, `s8-tracks`, `s8-songbar`, `s8-mix` and `pc-keys` (it runs as
  a PC on every OS, with a PC user agent, so it passes on the Mac too). Every mixer channel and the master now have
  a gain, which changes no bit (no render moved).
- `8b-ui`: 8b's screens, with 2 more Rust tests (1,132 on Linux) and 5 more browser suites (40): `s8-mute`,
  `s8-audition`, `s8-meters`, `s8-clips` and `s8-automation`.
- `polish`: the rest of the top ten, with 52 more Rust tests (1,184 on Linux) and 9 more browser suites
  (49): `pl-mix`, `pl-sounds`, `pl-ids`, `pl-zoom`, `pl-arrange`, `pl-tracks`, `pl-projects`, `pl-notes` and
  `pl-lanes`. No render moved.
- `next`, with 8a's offline half: 1,353 Rust tests on Linux and 9 more browser suites (58):
  `nx-undo`, `nx-effects`, `nx-piano`, `nx-midi`, `nx-settings`, `nx-nodevice`, `s8-leases`, `nx-nits` and
  `nx-click`. The suites post MIDI as a keyboard would (`M2_MIDI=inject`), and the check opens no MIDI device
  (`M2_MIDI=none`), so a keyboard plugged into the Mac never plays into it. No render moved.
- 8a's live half, once merged: 1,404 Rust tests on Linux and one more browser suite (59/59): `sound-stops`, which
  plays a sound that crashes. The gates row plays G12 live for one minute, muted (about 70 s more). Before the run,
  set the output to the built-in speakers at 48 kHz (Audio MIDI Setup): at another rate or buffer, G12 says "not
  the designed set-up" and what to set. The Mac's own real-time code runs its tests for the first time
  (`an_engine_thread_has_the_time_constraint_policy`, `a_worker_joins_and_leaves_the_default_outputs_workgroup`).
  No render moved.
- `studio`: 1,564 Rust tests on Linux and 7 more browser suites (66/66): `st-audio`, `st-record`, `st-midi`,
  `st-stems`, `st-tips`, `st-errors` and `st-guide`. `st-record` has a capture server of its own, which plays the
  count-in into a file. No render moved.

**What must pass:** the first line says `music2 check: PASS`. Every row says PASS (the gates row too, not PART),
with nothing "passed only on a retry". In the gates part:
- G11, as last time.
- G5 and G7a through the server: heard within 200 ms of the request.
- G5 and G7b on the engine rig, with G7a's click check.
- G7e: the export pauses, borrows both plugins, comes back with no click, and the held chord sounds again.
- G7f part 1 (`store`): "block within S + 80 µs, by the mean", "footprint within S + 64 MB", "full build
  within 1.5 × S", the 25% rule, and "the same music".
- G7f part 2 (`arena`): "callback within S's + 320 µs, by the mean", "the same music", and the slowest
  activation block under 5,333 µs.

**What may fail, and why:**
- G7f. At `cbac57c` it read −19 µs a block and −196 a callback; on Linux the back ends' gain nodes moved it by
  less than one run's noise. If either reads within 50 µs of its bound, or fails, the lead asks for one more run.
- The first line and the gates row say PART, or there is an `info machines:` line: the Mac's name differs from
  the two records written by hand. Nothing else is wrong then. The gates part's first line names the Mac
  (`machine: …`): the lead copies what follows `machine: ` over the `machine=` value of the three record lines
  (`store-record` and `store-p1` in `crates/gates/store-S.txt`, `callback-record` in `callback-S.txt`), and asks
  for one more run.
- Anything else that fails is new.

**Then a check by ear** (about 10 minutes; no test can do it, as the tests play no sound):

| # | Do this | You should hear or see |
|---|---|---|
| 7 | Type `cargo run --release -p music2-desktop`. | The app's start screen. |
| 8 | Click **Open an Example**, then **With plugins**. Click the loop button, then press Space. | It plays round and round 4 bars. |
| 9 | Click **Export**, then **Save**. | "Paused for export" in the top bar, and the sound stops for a moment. |
| 10 | Listen as it comes back. | It goes on where it paused, with no click, and the chord sounds again at once. |
| 11 | While it plays, plug in headphones (or connect AirPods). | Playback stops, and a message says the sound output moved. |
| 12 | Press Space. | It plays on, in the headphones. |
| 13 | Quit the app. Type `cargo run --release -p m2-server --example reference -- /tmp/p50 --tracks 50`. | It makes a 50-track test song: more than the Mac plays smoothly. |
| 14 | Type `cargo run --release -p music2-desktop -- /tmp/p50`, press Space, and let it play for a minute. | Gaps or clicks are fine, but it keeps playing: no "the audio device stopped responding". |
| 15 | Look at the Terminal. | It may say `music2: the audio device reported … dropouts`: the count at work. |
| 16 | Quit. Type `cargo run -p music2-desktop` (a slower test build), and play With plugins for a minute. | The Terminal never says "the audio thread allocated". |
| 17 | Tell the lead what you saw and heard at 9 to 16. | |

**A check by ear of the new sounds** (about 5 minutes):

| # | Do this | You should hear or see |
|---|---|---|
| 18 | Quit the app. Type `cargo run --release -p music2-desktop`. Click **Open an Example**, then **Pop sounds**. Press Space. | A short pop song: supersaws, an 808, FM keys, bells and drums. It sounds full, with no clicks. |
| 19 | While it still plays (in its first 20 seconds), press Space to stop. Wait about 10 seconds. Press Space again. | It plays on from where it stopped. The wobble on the keys and the strings (their chorus) carries on smoothly, with no jump. |
| 20 | Press Space, then Return, then Space. | It stops, goes back to bar 1, and plays from there, with no click. The wobble and the drums' noise carry on from where they were, so this play may differ a tiny bit from the export. That is expected for now: it is the open question in Open owner decisions. |
| 21 | Tell the lead what you heard at 18 to 20, and whether 20 sounded right to you. | |

**A look and a listen at the new screens** (`taste-next`, about 10 minutes):

| # | Do this | You should hear or see |
|---|---|---|
| 22 | Quit the app. Type `cargo run --release -p music2-desktop`. Open **first-song**. | The new look: one bar at the top (play, the tempo "124 BPM", "4/4", the song's name in the middle, Export), the tracks, and the dock below. The Mac's own font, and the Mac's menus as before (music2, File, Edit, Track, View, Transport, Window, Help). |
| 23 | Press ⌘T. | + Track's list: Drums, Instruments, Sound design. Pick **Synth pad**: a new track below the selected one, and it plays with the rest. ⌘Z takes it out. |
| 24 | Press ⌥3. Press C to loop, then Space. While it plays, drag Kick's fader up and down. | The Mix view: a strip per track, Master on the right. The level changes as you drag, smoothly, with no clicks; let go and it is written into `song.ts`. |
| 25 | Drag Kick's pan knob a little (it reads "C" in grey). | Nothing changes until it moves; then the kick moves left or right, with no click. Two ⌘Z put the code back. |
| 26 | Still playing, drag the tempo in the top bar up by about 10. | The tempo changes when you let go, with no gap. |
| 27 | Press ⌘. (Command and full stop). | All sound stops at once; play goes on, silent until the next notes. |
| 28 | Press Space to stop, then Space again. | It plays the song again from where you started. Chorus and drum details may not match the export exactly then; the export is always right. |
| 29 | Right-click the ruler at bar 5, **Add tempo change at 5.1**, type 140, Return. | A mark "♩140" on the ruler; the song speeds up from bar 5. Click the mark, then **Remove this tempo change**. |
| 30 | Open the example **With plugins** (it plays Surge XT), press Space, then click **Export**. | While it exports, the grey pill in the top bar says "Paused for export"; then a toast says where the file is, with **Show in Finder**. |
| 31 | Tell the lead what you saw and heard at 22 to 30. | |

**A check by ear of 8b's screens** (about 10 minutes):

| # | Do this | You should hear or see |
|---|---|---|
| 32 | Quit the app. Type `cargo run --release -p music2-desktop`. Click **Open an Example**, then **First song**. Press Space. | Kick, hats, bass and keys play. |
| 33 | While it plays, click **M** on Kick's row (right of its name). Then click it again. | The kick goes quiet at once, and a pill at the top says "1 muted". The second click brings it back. |
| 34 | Click **S** on Bass's row. Then click it again. | Only the bass plays. The second click brings the others back. |
| 35 | Press Space to stop. Click Kick's **⋯** (or right-click its name), then **Mute in song**. Click **Export**, then **Save**, then **Show in Finder** in the message, and play the file. Then press ⌘Z. | Kick's name is crossed out. The file has no kick. ⌘Z brings Kick back. |
| 36 | Double-click Bass's clip. Notes opens. Click a note, then click a key on the piano at the left. | You hear each one while you hold it. |
| 37 | Press Space. Watch the thin bars at the right edge of each track's name, and the Song row's. Press Space again. | They move with the sound, and fall to nothing within about a second after Stop. |
| 38 | Tell the lead what you heard at 33 to 37. | |

**A look and a listen at `polish`** (about 15 minutes). These are what only a Mac can show: its trackpad, its menu
bar, its Ctrl-click, and the sound.

| # | Do this | You should hear or see |
|---|---|---|
| 39 | Quit the app. Type `cargo run --release -p music2-desktop`. Open **First song**. Press ⌥3, then Space. | The Mix view. Each strip has M, S and two thin bars beside its fader that move with the sound. |
| 40 | While it plays, click **M** on the Keys strip, then **S** on the Bass strip. Click each again. | Keys goes quiet; then only the bass plays. The second clicks bring them back. The headers' M and S light up too. |
| 41 | Drag the Hats fader down a lot. | The hats get quieter, but the Hats strip's bars don't move: they show the track before its fader. |
| 42 | Press Space to stop. On the trackpad, pinch out over the timeline, then pinch in. | The timeline zooms in and out around your fingers. The window itself never zooms. |
| 43 | Press ⌘= twice, then Space. Watch the timeline. | It zooms in around the playhead; while it plays, the view pages along with it. Scroll by hand: it stops following until you play again. |
| 44 | Right-click Hats' name, **Change instrument…**, type `kit`, Return. Press Space. | Hats plays the drum kit now, and a message says so. ⌘Z puts the old hat back. |
| 45 | Double-click the Kick's clip, then Ctrl-click a note in Notes. | The note menu opens, and the note doesn't move. Pick **Humanize**: the kick sounds a little less stiff. ⌘Z. |
| 46 | Right-click the ruler at bar 5, **Add marker**, type Chorus, Return. Then Ctrl-click the Chorus marker. | Chorus sits on the ruler. The Ctrl-click opens its menu, and the playhead doesn't jump there. |
| 47 | Press ⇧⌘S. | Save As, with the song's name and a 2 after it. Press Return: the copy opens, and a message says the first one is kept. |
| 48 | Open the **File** menu, then **Open Recent**. | Both songs are listed, the copy first. |
| 49 | Quit the app (⌘Q) and start it again with step 39's command. | The song you had open opens again by itself. |
| 50 | Tell the lead what you saw and heard at 39 to 49. | |
| | **`next`**: what only a Mac with real devices can show. Start the app with step 39's command. | |
| 51 | Press Space. While the song plays, plug in a MIDI keyboard. | A message says the keyboard is connected. A small keyboard mark shows in the top bar. |
| 52 | Click Bass's name. Play some keys, soft, then hard. Hold the sustain pedal while you play. | Bass plays what you press, at once. Soft keys are quiet, hard keys loud. The pedal holds the notes. |
| 53 | Hold a key down and unplug the keyboard. | The note stops. The mark goes, and a message says the keyboard was unplugged. |
| 54 | Press K, then Space. Then press C to loop. | A click on each beat, a higher one on the first beat of each bar. It stays with the drums, also each time the loop jumps back. |
| 55 | Press ⇧⌘E to export. Play the file in the Finder (select it, press Space). | No click in the file. |
| 56 | Press ⌘K. Press A, S, D, F. Then Z, then X. Press ⌘K again. | Bass plays: A is middle C. Z goes an octave down, X back up. A card shows the keys. ⌘K again hides it. |
| 57 | While the song plays, drag a note in Notes, then a point of an automation line. Then drag the tempo number up and down. | You hear each where it is while you drag. The song speeds up and slows down with the tempo. Let go: it stays. ⌘Z goes back. |
| 58 | Press ⌘, for Settings. Plug in headphones. Under Sound output, pick them. Then under Buffer size, pick 128. | The sound moves to the headphones and stops: press Space to go on. The Now row says the headphones and 128 samples (2.7 ms). No crackles. |
| 59 | Unplug the headphones while the song plays. Press Space. Look at Settings. | It plays on the Mac's speakers. Sound output says the headphones "(not connected)". The Now row says "(the output chosen can't be used now)". |
| 60 | Tell the lead what you saw, heard and felt at 51 to 59. | |
| | **`studio`**: what only a Mac can show: the Finder, a real MIDI keyboard, and the sound. Start the app with step 39's command, and open **First song**. | |
| 61 | In the Finder, drag a WAV file (a song or a loop) onto the window, below the tracks, at bar 3. | While you drag, a blue outline and a few words say where it goes ("New audio track · bar 3"). Let go: a new track, named after the file, with its waveform. |
| 62 | Drag another WAV file onto that new track, at bar 9. Press Space. | It lands on the same track at bar 9. Both play with the song, in time. |
| 63 | Press Space to stop. Drag the right edge of a clip to the left. Then drag the small square at its top left to the right. Press Space. | The clip gets shorter. It fades in: the sound starts quiet and grows. No clicks. Two ⌘Z put it back. |
| 64 | Press ⇧⌘I and pick a WAV file. | The Mac's own open window, "Import Audio", shows WAV files. The file lands at the playhead's bar. |
| 65 | Plug in a MIDI keyboard. Click Bass's name. Press R. | The Record button counts 4, 3, 2, 1, with a click on each beat. Then the song plays. |
| 66 | Play a few notes, then press R. | Each note shows in Bass's row as you play it. R stops recording, and the song plays on. The notes are one clip, where you played them. ⌘Z takes it back. |
| 67 | Press Space to stop. Drag across the ruler from bar 1 to bar 3 to loop it. Press R and play until the loop goes round. | The take ends where the loop goes back: only the first time round is kept. The loop plays on. |
| 68 | Press ⌘, for Settings. Under MIDI keyboards, untick your keyboard. Play a key. Then tick it again. | Unticked, the keyboard plays nothing, and Record would record nothing from it. Ticked, it plays again. |
| 69 | Press ⇧⌘E. Click **Stems**, then **Export…**. When it is done, click **Show in Finder**. | A new folder in Exports, with one WAV file for each track. Open one in QuickTime Player or GarageBand: it is that track alone. |
| 70 | Press ⇧⌘E. Click **MIDI**, then **Export…**. Save. Open the file in GarageBand. | The Mac's own save window opens in Exports. In GarageBand the tracks and their notes are there. |
| 71 | Click **Help**, then **Make Your First Song**. Click **Start from an empty song…**, and follow the steps. | Each step ticks itself when you do it. At the end, the card says where your song was saved. |
| 72 | Tell the lead what you saw, heard and felt at 61 to 71. | |
| | **`io`**: long audio from the disk, and recording your voice. Have headphones, and a WAV file 10 minutes or longer. Build the app: `(cd apps/music2-desktop && ../../web/node_modules/.bin/tauri build --bundles app)`, then open `target/release/bundle/macos/music2.app` (the bundle, so macOS asks about the microphone in music2's own words). Open **First song**. | |
| 73 | Drag the long WAV file onto the window, below the tracks. Press Space. Click the ruler near its end, then near its start. | It plays at once from each place you click: no gap, no click. The status pill never says "Disk too slow". |
| 74 | Drag across the ruler over 4 bars of that clip to loop it. Let it go round 10 times. | No gap where the loop jumps back. |
| 75 | Press Space to stop. + Track ▸ Audio track. Press R. | The first time, macOS asks to let music2 use the microphone: "music2 records from your microphone when you press Record on an audio track." Click Allow. The button counts 4, 3, 2, 1, with clicks. |
| 76 | Press K (the click). Count "one, two, three, four" out loud on the clicks for 2 bars. Press R. | A red clip grows as you speak, with a thin bar at its right end that moves with your voice. R ends it: a clip "Audio take.wav" with your voice's waveform. Play it from bar 1: your words land on the clicks, not late. |
| 77 | Press ⌘Z. | The clip goes. "Audio take.wav" stays in the song's `assets` folder. |
| 78 | Put on headphones. Press ⌘, for Settings. Tick **Hear yourself while recording**. Close Settings, press R and speak. | You hear yourself in the headphones while it records, the count included. It sounds a little late at most, never like an echo. Untick it after. |
| 79 | In Settings, under **Sound input**, pick another input (a USB microphone, if you have one). Record again. | It records from that input. |
| 80 | In System Settings ▸ Privacy & Security ▸ Microphone, turn music2 off. Back in music2, press R on the audio track. | A message: "music2 can't use the microphone. Allow it in System Settings ▸ Privacy & Security ▸ Microphone." Nothing records. Turn it back on. |
| 81 | With a USB microphone picked in Settings, press Space, then R, and speak. While it records, unplug the microphone. | The song plays on. A message (since track `ex`): "(the microphone) was unplugged, so the take ended there. It was kept up to then. The song plays on." A clip of your voice up to there. Plug it back in and press R: it records again. |
| 82 | Tell the lead what you saw, heard and felt at 73 to 81. | |
| 83 | Click **Open an Example**, then **Sampler**. Press Space. | Keys play a plucked sound, Drums a kick and a snare, in time. No clicks. |
| 84 | Press Space to stop. + Track ▸ Sampler. In the Finder, drag a short WAV (one sound, like a drum hit) onto the Sampler's card in Device. | While you drag, the card says "Play it in Sampler". Let go: the card names the file. |
| 85 | Press ⌘K. Press A, then S, D, F. | A plays the file as it is (A is middle C). S, D and F play it a little higher each time. |
| 86 | Press ⌘K again, then ⌘Z. | The card says "Drop a WAV here" again. The file stays in the song's `assets` folder. |
| 87 | Tell the lead what you saw, heard and felt at 83 to 86. | |

**A check by ear of plugins in their own program** (`ph`, about 10 minutes):

| # | Do this | You should hear or see |
|---|---|---|
| 88 | Quit the app. Type `cargo run --release -p music2-desktop`. Click **Open an Example**, then **With plugins**. Press Space. | It plays the Surge XT lead as before, with no clicks. |
| 89 | Open Activity Monitor and type `music2` in its search box. | Two music2 programs: the app, and a second one that runs the plugins. |
| 90 | Back in music2, click the **lead** track, then **Device** at the bottom. On the Surge XT box, click **Open window**. | Surge XT's own window opens. The button now says **Close window**. |
| 91 | In Surge XT's window, turn the filter cutoff knob while it plays, and let go. | You hear the sound change as you turn. |
| 92 | Press ⌘Z. | The sound goes back to how it was. |
| 93 | In the Terminal, type `pkill -9 -f -- --m2-plugin-host` while it plays. | The song plays on. The lead goes quiet for a moment, a message says "The plugin host crashed…", and the lead comes back by itself. Surge XT's window closes: open it again from the box. |
| 94 | Click **Export**, then **Save**, while it plays. | "Paused for export" in the top bar, then it plays on. The file sounds like the song. |
| 95 | Tell the lead what you saw, heard and felt at 88 to 94. | |

**A check by ear of stretched clips and Follow tempo** (`sx`, about 5 minutes):

| # | Do this | You should hear or see |
|---|---|---|
| 96 | Click **Open an Example**, then **Resample**. Press Space. Listen from 3 seconds in (bar 2, beat 3) to 5 seconds, then from 6 seconds (bar 4). | At 3 s the keys come back slower, stretched over 4 beats. Their pitch is the same as at the start. No wobble, no clicks, no echo. At 6 s they come back higher and stretched, still on the beat. |
| 97 | Tell the lead what you heard at 96. | |
| 98 | Open **Resample** again. Right-click the first clip on Chops (bar 1). Click **Follow tempo**. Press Space from the start. | It sounds as before. |
| 99 | Double-click the tempo (120), type 90, press Return. Press Space from the start. | The first clip keeps in time with the kick, a little slower, at the same pitch. The second clip (not followed) plays more of its sound in the same time. |
| 100 | While it plays, drag the tempo up and down a little, then let go. | The followed clip goes on in time. At most a short silence, no click, no wrong sound. |
| 101 | Press ⌘Z twice. | The tempo is 120 again, and Follow tempo is off. |
| 102 | Tell the lead what you heard at 98 to 101. | |

**A check by ear of the sound input's problems** (`ex`, about 10 minutes, with a USB microphone):

| # | Do this | You should hear or see |
|---|---|---|
| 103 | Click **Open an Example**, then **First song**. **+ Track ▸ Audio track**. Press ⌘, for Settings. Under **Sound input**, pick the USB microphone. Close Settings. Unplug the USB microphone. Press R. | A message: "(the USB microphone) isn't plugged in, so this take records from (the Mac's microphone)…". It records. The pill says "1 problem"; click it: the same words. Press R to end the take. |
| 104 | Plug the USB microphone back in. Press R, speak, press R. | It records from the USB microphone. No message. The problem is gone. |
| 105 | Press R and speak. While it records, unplug the USB microphone. | The song plays on. One message: "(the USB microphone) was unplugged, so the take ended there. It was kept up to then. The song plays on." A clip of your voice up to there. |
| 106 | Quit music2. In the Terminal type `tccutil reset Microphone com.suite2.music2` (started from the Terminal, macOS asks for the Terminal: then `tccutil reset Microphone com.apple.Terminal`). Open music2 again, open the song, click the audio track, press R. When macOS asks to use the microphone, click **Don't Allow**. | The take stops by itself within a moment. A message: "music2 can't use the microphone. Allow it in System Settings ▸ Privacy & Security ▸ Microphone. The take was only silence, so it wasn't kept." No clip. The song plays on. |
| 107 | Press R again. | The same "music2 can't use the microphone…" words. Nothing records. Then turn music2 (or the Terminal) back on in System Settings ▸ Privacy & Security ▸ Microphone. |
| 108 | Tell the lead what you saw and heard at 103 to 107. | |

After a green run the lead writes here: "macOS: green at <commit> (ci/check.sh on <model>, <date>)", with G7f's
numbers. If a row fails, the lead fixes it on Linux, checks the mac lint, and asks for one more run. The logs are in
`~/m2-check/<commit>.tgz` on the Mac if needed.

**What the lead checks in the pasted lines:**
- The gates part's first line reads `machine: MacBookPro18,1, macOS …, Apple M1 Pro, aarch64, 10 threads`: the Mac S's
  records name. Then there is no `info machines:` line, and no G7f line says `(PLAT-1)`.
- G7f's two "the same music" lines say ok: the app's layout, P1 and S played the same bits.
- "S as recorded": S measured now, beside S's record of 2026-10-14 (a mean of 8,936 µs a callback). Each turn's
  cold first half second is left out of the mean now, so the two should be close (the judged callbacks now spread
  over 3:01 to 3:40, not 3:00 to 3:30). If they differ by more than about 1% (90 µs), doubt the runner before
  trusting a verdict near the bound.
- The "starting a turn" lines, their first two numbers: of the judged steps, each turn's first half over its
  second, against S's. They say whether any of a turn's start is left in the judged steps. On Linux they read −48
  to +63 µs a callback and −10 to +17 a block, held on one die or not, the A/A control included. Under 0 means S's
  start lasted longer, and the verdict then favours the app's layout by about half the value. Below −100 µs a
  callback (or −30 a block), half a second wasn't enough on the Mac: then a pass within half that value of its
  bound is unsure, and the lead asks for one more run.
- G7f's printed comparisons: P1 against S (step 7's own cost) and the app's layout against P1 (O1's cost). They say
  where the time goes if a rule fails.
- "rust tests" includes the first Mac runs of four `core/os` tests (`the_footprint_counts_touched_pages_and_a_reservation_only_maps`,
  `interactive_gives_the_thread_the_user_interactive_class`, `the_mac_is_named`, `the_machine_is_named_in_five_parts`),
  `app_main_thread` (a state load on AppKit's main thread), `no_alloc.rs`, and the server's export, hold and device
  tests. The footprint test now maps and unmaps its own pages, so macOS's malloc cache can't hide them (unverified there).
- "browser suites" 40/40, with `live3b` at 60 checks: it exports with-plugins
  from the UI on real Surge XT.
- "renders" 8 of 8: the owner's layout changes no bit (checked on Linux). With the `sounds` merge, 10 of 10: the
  first Mac run of `examples/sounds`' two records, and of the rebuilt `std.wasm`'s bytes.
- With the `sounds` merge, G7f's S still plays its own std (S's runner sets each project's std line to S's version,
  `a3214f6`), while the app plays the larger std. p50 uses none of the new kernels, so they play the same music.
  If a G7f bound fails, check this first (`docs/designs/sounds/build-notes.md`, slice 1, risk 17).
- G7e compares the export with `m2 render`, which now runs plugins in offline mode (K4): its pass on the Mac
  confirms Surge XT renders the same there.
- With `macfix`, the app's `std:gain` skips its `powf` while its dB holds still; S's doesn't. So G7f store's line
  favours the app by about 250 µs a p50 block, and a pass there says little about the layout. Read the second G7f
  store run, on the old gain (Mac runs owed): about +135 means the Mac's shift since step 7 is still there.

## What to do next, in order

The owner said on 2026-10-16 to go on without waiting for the Mac: each step lands on `main` once Linux is green
and reviewed, and its Mac run is owed. **Mac runs owed:**

| Step | On `main` at | Mac run |
|---|---|---|
| Step 7 and its speed work | `ffc36aa` (Linux checks), then docs only | `ci/check.sh` passed at `cbac57c` (2026-10-17); owed: the by-ear steps 7 to 17 |
| The `sounds` merge (22 sounds, O3, std 0.2) | `e108e46` (Linux checks), then docs only | `ci/check.sh` on `main` at `85b069a` (2026-10-17) covered this and every row below: 11 of 13 rows. It failed G7f store (+135 µs a block, bound 80) and `new_songs_go_where_settings_say`; `macfix` fixes both. Owed: one more `ci/check.sh` once `macfix` is on `main`; then the by-ear steps 18 to 21 |
| The back ends of 8b.4, 8b.5, 8b.6, 8b.8 | `a8199ff` (Linux checks), then docs only | ran at `85b069a` (above); owed: the same one more run; by-ear steps come with its screens (`8b-ui`) |
| `taste-next`: the look, 8b.1, 8b.2, 8b.3 | `22bea45` (Linux checks), then docs only | ran at `85b069a`; owed: the same one more run, then the look-and-listen steps 22 to 31 |
| 8b's screens (`8b-ui`) | `92317ad` (Linux checks), then docs only | ran at `85b069a`; owed: the same one more run, then the by-ear steps 32 to 38 |
| `polish`: the rest of the top ten | `65f59f1` (Linux checks), then docs only | ran at `85b069a`; owed: the same one more run, then the by-ear steps 39 to 50 |
| 8a's offline half (all CPU cores, offline) | `bc41693` (Linux checks), then docs only | its Mac tests ran at `85b069a` (pool models 23 passed); the offline sweep: done (2026-10-17, at `8a6d9e1`: go) |
| `next`: 8b.7, 8b.10, Settings, no sound device, undo names, effect comforts | `1560a19` (Linux checks), then docs only | ran at `85b069a`; owed: the same one more run, then the by-ear steps 51 to 60 (a real MIDI keyboard, the metronome against the beat, switching the output) |
| `macfix`: G7f store (the gain's fast path) and the songs folder by its real path | `ac5d8c0` (Linux checks), then docs only | owed: one more `ci/check.sh`, then G7f store once more on the old gain (below) |
| 8a's live half (live on several cores, a sound that breaks, G12) | `21a7a1b` (Linux checks), then docs only; on `main` at `44b104a` | owed: 8a's Mac sweep below, one paste, about 40 minutes. The same `ci/check.sh` covers its Mac tests and G12's minute |
| `studio`: audio in the app, recording notes, MIDI files and stems, help in plain words | `ad5f111` (Linux checks), then docs only; on `main` at `02ba4d3` | owed: the same one more `ci/check.sh`, then the by-ear steps 61 to 72 (the Finder, a real MIDI keyboard, stems in another app) |
| `io`: audio clips play from the disk; recording audio | `64380e1` (Linux checks), then docs only; ready to land (branch `io`) | macOS: unverified since `eda5e77`. Owed once it is on `main`: one more `ci/check.sh`, then the by-ear steps 73 to 82 (a long file from the disk, a real microphone, the permission question, the take against the click, hearing yourself in headphones, a USB microphone unplugged) |
| `ph`: plugins in their own program, plugin windows, latency compensation | `d897cf0` (Linux checks), then docs only; ready to land (branch `ph`) | macOS: unverified since `b0ebc74` (shared memory, the wake across processes and plugin windows are lint-checked only). Owed once it is on `main`: one more `ci/check.sh`, then the by-ear steps 88 to 95 (two music2 programs, Surge XT's own window, a knob and its undo, the plugin program ended while it plays, an export) |
| `ab`: kernel ABI v1, one sandbox a box, the Sampler | `7399cb8` (Linux checks), then docs only; ready to land (branch `ab`) | owed once it is on `main`: one more `ci/check.sh` (G7f's run of the new layout: its footprint and address space, PLAT-1), then the by-ear steps 83 to 87 (the Sampler example, a WAV dropped on a Sampler, the keyboard piano, Undo) |
| `sx`: `std:stretch` (an offline kernel), stretches in the background, Follow tempo | `000dd72` (Linux checks), then docs only; ready to land (branch `sx`) | owed once it is on `main`: one more `ci/check.sh`, then the by-ear steps 96 to 102 (Resample's stretched clips; Follow tempo and a tempo change) |

**8a's Mac sweep (for the owner): offline and live, one paste.** `8a` is on `main`.

**In short:** about 40 minutes. It plays nothing aloud. Paste one line, wait, then paste back what is in
`~/m2-8a.txt`.

| # | Do this | Why |
|---|---|---|
| 1 | Plug the Mac in, keep the lid open, and quit other apps. | The timings need a quiet Mac. |
| 2 | Set the output to the built-in speakers at 48 kHz (Audio MIDI Setup). | Gate 8 is judged at 48 kHz. |
| 3 | Open Terminal on the Mac itself, paste the line below, and press Return. | It gets `main` and runs every step, one after another. |
| 4 | Paste back what is in `~/m2-8a.txt`. | That is all the lead needs. |

```sh
cd ~/dev/suite2/music2 && git fetch && git switch main && git pull && ci/sweep.sh
```

What each step decides (the full table: `build-notes-8a.md`, "The Mac command"):

| Step | About | What it runs | What it decides |
|---|---|---|---|
| build | 5 min the first time | `cargo build --release -p m2-gates` | — |
| 1. models | 1 min | the pool's models (`m2-pool --features models`) | the pool's orderings and its wake on arm64 (one bug can only show there). A FAIL: stop; it is fixed on Linux first |
| 2. `m2-os` rt | 2 min | `cargo test -p m2-os rt` | the Mac's real-time code, run for the first time: the wake, the time-constraint policy, the output's workgroup joined and left. A FAIL: that code is fixed before anything live is judged |
| 3. sysctl | seconds | `sysctl hw.model hw.perflevel0 hw.perflevel1` | the counts the rules read: 4 fast cores per L2 (the live count's cap), 8 fast cores (an export's) |
| 4. offline sweep | 6 min | `m2-gates realtime --offline --reference 50` | after R1, on the Mac: 8 threads against 4 (an export alone keeps every fast core while 8 beat 4 by 10%); the song's tail (310 µs a block before R1); paced 4-thread blocks (8,045 µs p99 before) |
| 5. live sweep | 10 min | `m2-gates realtime --reference 50 --minutes 1 --sweep 1,2,3,4 --between park,spin` | **the live thread count:** the fewest threads whose p99 is within 5% of the best, with no dropout. **Park or spin:** spin if its p99 at that count is at least 10% lower, with no more dropouts. **The set-up:** every engine thread real time and in the workgroup, or the workgroup code is fixed first |
| 6. gate 8 | 11 min | `m2-gates realtime --reference 50 --minutes 10` | gate 8 on this Mac: no dropout from Play, p99 at most 1,867 µs (70% of the period), no crash, every engine thread in the workgroup. On an M1 Pro the verdict is provisional (a stand-in for a base M1). A FAIL: the study list of design-8a §16, in its order |

**G7f store once more, on the old gain (for the owner, after the next `ci/check.sh`).** macfix made the app's
`std:gain` skip its `powf` while its dB holds still. S plays its own std, with the old gain. So G7f store's line now
favours the app by about 250 µs a p50 block (on Linux), and would hide the Mac's +154 µs since step 7. This run
puts the old gain back on the app's side, so the shift stays in view. Same Terminal, right after the check (about
10 minutes):

```sh
cd ~/dev/suite2/music2
mkdir -p ~/m2-check/std-85b069a && git archive 85b069a std | tar -x -C ~/m2-check/std-85b069a
export M2_GATES_S=$(crates/gates/s/build.sh | tail -1)
M2_STD=~/m2-check/std-85b069a/std cargo run --locked --release -p m2-gates -- store --reference 50 2>&1 | tee ~/m2-g7f-old-gain.txt
```

Paste back `~/m2-g7f-old-gain.txt`. Its "block within S + 80 µs" line: about +135 again means the shift is still
there, unexplained; near step 7's −19 means it is gone. What each says, and the candidates (std's rebuilds; 8a's
blocks through partitions and its floating-point mode): `build-notes-macfix.md`, "The Mac's next run".

1. **The Mac runs** (above), when the owner has time. The check at `85b069a` failed two rows; `macfix` fixes them.
   Once it is on `main`: one more `ci/check.sh`, G7f store on the old gain, then the by-ear steps. Fix what they
   find on Linux, then ask for one more run.
2. **Merge `8a` into `main`** (a merge commit: `8a` already has `main` at `1cc7382`). Then ask the owner for 8a's
   Mac sweep (above, one paste). Its step 5 sets two lines in `core/play/src/engines.rs`: the live count
   (`live_count`) and `BETWEEN` (park or spin). Its step 2 is the first run of the Mac's real-time code: a FAIL
   there is fixed first.
3. **The `sounds` merge is on `main`** (`docs/designs/sounds/`). Ask the owner the Space, Return, Space question
   (Open owner decisions).
4. **`taste-next` and `8b-ui` are on `main`.** What they built, decided and left:
   `docs/designs/step8/build-notes-taste.md` and `build-notes-8b-ui.md` (End). Left on the back end: W1 (the keyed
   compressor, a std change). The rest of their screens' list (U12 to U14, the delete dialog's keys) is in `polish`.
   Lead's default, the owner may change it: in the export question, Return presses the focused button (at first the
   main one), not always the main one as a Mac alert does.
5. **`polish` is on `main`.** What it left is in `docs/gaps.md` and its build notes' End.
6. **8a is done on Linux:** its offline half on `main` (`bc41693`), its live half on `8a` (`21a7a1b`, item 2).
   What it left is in Left for later ("8a's live half") and the build notes' "For the lead" sections.
7. **8b.7 and 8b.10 are on `main`** (`next`, Linux green at `1560a19`). What it left is in `docs/gaps.md` and its build notes' End. Then **step 9**
   (8b.9's Change track ID… came in `polish`, without the engine's pass). Before
   step 9 freezes kernel ABI v1, the owner decides the kernel layout (Open owner decisions). Each adds its OS pieces
   to `m2-os` as platform design §8 says (8a's offline half put cores and priorities in `process` and `thread`,
   Linux's in `linux/cores.rs`; 8b.10's MIDI uses `midir`). Mac run at the end of each.

## Merge notes

Platform design §13.3, updated by what step 7 learned. Never take a side on `Cargo.lock` or `std.wasm`.

| Branch | Where | What to do |
|---|---|---|
| any | `crates/server/src/graph/fsevents.rs` is gone | FSEvents moved to `core/os/src/mac/watch.rs`, the hints and filters to `graph/events.rs`. git sees a delete and an add (`git diff -M40%` pairs it with `mac/watch.rs`). No branch changes that file today; if one does, carry the change by hand. |
| any | `core/kernel/` is now `std/kernels-src/kernel/`; the app test is `app_main_thread.rs` | git follows these renames |
| any | OS code outside `core/os` | `core/os/tests/one_place.rs` fails on it (PLAT-2): move it into `m2-os` |
| any | `Cargo.lock` | take `main`'s, then `cargo build` adds the branch's edges; never `cargo update` |
| `sounds` | done | merged, now on `main`, as these notes said (`docs/designs/sounds/build-notes.md`, slice 1) |
| `taste` | done | merged as `taste-next`, as these notes said (`docs/designs/step8/build-notes-taste.md`, each slice's conflicts and breaks): WRITE-17 is taste's text with the network-volume words; attention waits for the page before a toast's Undo (D7, `cf3115b`); Inter off the Mac (A2); the engine note became neutral states of the status pill (lead decision T2), and fx-fixes checks them at 1100 and 1280 px, not 960 (no app window is narrower than 1100); `instruments.ts` has taste's `category` and `sounds`' `description`; + Track lists the 22 sounds (T3); std 0.2; `delta.rs` keeps the `sounds` merge's `Op::Time` arm; s8-tracks hears `layout.json` within a second on inotify; README supports Linux |
| `8a` (live half) | done: `main` with `next` merged in at `739cfc6` (no conflict), `8a-gaps` at `54d1b93` | To land: merge `8a` into `main`. If `next`'s later work in `core/play` (8b.7's lane leases on `Player::swap_lane`, …) lands first, `8a`'s player hooks are short, with the rest in files of their own (`engines.rs`, `timing.rs`). What `next` had added: small, separate blocks to `core/play/src/lib.rs` and `core/runtime`: the click (B1), the device's buffer and `fell_back` (C1), `Player::freed` and the stream's end (review NX-S5), and the device thread's `open_on` loop, trying the output chosen then the default (NX-S1). Each is listed in `build-notes-next.md` ("For 8a's merge", by lane and in the review). `Choice` folds into 8a's `Options`; 8a's `Player::close()` can replace `set_output`'s wait for `freed`. `next` already merged `main` at `8a6d9e1` (one import line in `engine.rs`: both kept) |
| `8b-ui` | onto `taste-next` | it was planned on `taste`'s screens: rebase or merge onto `taste-next`. Taste's files it will meet: `Mix.tsx` (strips, the lane fader), `SongBar.tsx`, `TopBar.tsx`, `Tracks.tsx`, `SongTimeline.tsx`, `Knob.tsx` (the gesture hook). The browser suites read the Mix strips by `data-knob` ids |

## Known flakes

Seen, not fixed:

| Test | What fails | Note |
|---|---|---|
| G7f part 2's slowest activation block (`arena`) | over its 5,333 µs bound on this server under load (1 of 3 runs beside other agents; 4 of 4 beside 32 busy loops) | Linux timings are information only (PLAT-1); judged on the Mac. In `--gates` here it can fail the gates row. |
| G3 (a note edit within compile + 20 ms) | p95 16–22 ms at n=10 | near its line under load; judged on the Mac |
| G5 and G7a through the server (`--server --reference 50 --at 180 add-effect`) | p95 125 ms past the compile (bound 100), 368 ms from the request (bound 200) | once, in 8a live's first final check, at load up to 26: the session's compile took twice as long too. Alone, 3 runs of 3 passed at the usual numbers (about 111 ms). Judged on the Mac |
| `taste` | the Bass header's click didn't select it within 30 s | once, in a run of three suites at once beside other agents (`taste-next`'s final phase); not seen in its checks |
| `live3b` | "its release writes db(-32) into song.ts" | once, at load 45–74, before step 7; not seen since; watch for it |
| `s5-knobs` | page detached after a double-click on a knob at its default (3 of 11 release runs) | from before the platform step; not seen in its checks |
| `audition_never_waits_for_a_compile`, `midi_never_waits_for_a_compile` | "the compile's span" not found; "attempt to subtract with overflow" (a span ending before it began) | both once, in io's second final check, at load 9 to 10 beside another track's tests; they read spans on the player's clock, which a new player starts again; alone, 3 runs of 3 pass |
| `s8-clips` | "the left edge dragged a bar right: 8+28" (a notes clip's left edge drag landed 4 beats short) | once, in sx's final check beside other tracks' builds; passed on its retry and 2 of 2 alone |
| `st-audio-fix` | "and adds nothing": the song's revision moved after an MP3 was refused | once, passed on its retry, in ph's second final check (load low); probably the ⌘Z just before it still settling. Not seen in io's or studio's checks |
| `nx-effects-fix` | "and says why: Move left (off)": the tooltip read before it changed | once, passed on its retry, in ph's third final check (load 8) |

Fixed in step 7 (of these, only `a_plugin_that_keeps_failing_is_started_again_once` was seen in a `ci/check.sh` run):

| Test | Cause |
|---|---|
| `swapping_two_plugin_keys_moves_the_instances` | counted every plugin instance in the process, so other tests' probes added to it; it counts its own session's now (`cc84d6a`) |
| `a_second_export_waits_for_its_own_hold` | the test raced its own setup (5 runs in 100); it stops filling once the engine holds (`1a1b47c`) |
| `the_footprint_counts_touched_pages_and_a_reservation_only_maps` | other tests mapped memory beside it (1 run in 10); each try reads around its own change (`9408619`), and maps and unmaps its own 64 MB, for macOS's malloc (`70bd927`) |
| `playback_the_device_stopped_is_said`, and two other new server tests | called the catch-up while the actor was busy (1 run in 5 under load); they wait for it (`ed77c41`) |
| G7e's fade-in check | failed 3 of 20 when the hold landed near a kick's attack; the export now starts on a kick's body (`857bd5a`) |
| `a_plugin_that_keeps_failing_is_started_again_once` | passed only on a retry once (the speed work's first check): its rounds' catch-up did nothing while the actor was busy. They run on the actor now (`a894448`): under a stress run, 33 of 200 runs failed before, 0 of 200 after |
| `live3b`'s export checks | heard the chord at about -85 dBFS; the lead goes back to its own level first (`987bf0d`) |

Fixed in 8a's live half:

| Test | Cause |
|---|---|
| `pl-mix` | "its strip meter still reads as the header's" (once, in 8a's offline check): the hats are short hits, so most samples read the floor. It reads until a hat shows (the gaps lane, `e66e6cd`): 10 runs of 10 pass |
| `loading_is_published_while_a_plugin_is_created` | failed once in this track's check, and in 8b's, polish's and next's: it read the activity, then the state, and the plugin's load could end between them. It keeps a pair only while both show it (`c924e0b`): with 5 ms between the reads, 10 of 10 failed before and 0 of 10 after |

Fixed in 8a's offline half:

| Test | Cause |
|---|---|
| G10 in `the_live_engine_passes_every_gate` | passed only on its retry in 8a's first final check. It took one wall-clock timing of each range render, within 10%, and load alone failed it (2 of 8 runs here). Now each is timed on a CPU, the best of three, in turns (`bc41693`): 10 of 10 beside bursts of load on its CPU |

## Left for later

One line each; the details are in `sounds/build-notes.md`, `step7/build-notes.md` and `platform/build-notes.md`.

From `polish` (details in `docs/gaps.md` and `step8/build-notes-polish.md`):
- Meters after the fader need taps in `core/runtime` (8a's area); about a day once 8a lands.
- Curves between automation points need a lane format field and interpolation in the engine.
- A re-keyed track restarts its sounds; Restore (DIAG-6) isn't built.
- `m2` commands for change instrument, markers, the key, a track ID and Save As (gaps #37).
- `s8-mute` still has its own copy of the taste lint.

The `sounds` merge:
- **Stop, back to bar 1, Play is a seek** (its plan's risk 2): std's LFO phases and the drums' hit counts carry on,
  so that play differs from the export (RENDER-6 allows it). If the owner wants it (Open owner decisions): played
  from bar 1, std's song-time state (LFO phases, hit counts) could start again. That changes no export.
- **Stopped, std's effects are still** (risk 3): a tail after Stop goes through a chorus that no longer moves, and a
  tremolo held at its level. Nothing clicks. When live input and audition arrive (8b.10), keys played while stopped
  would hear a still chorus. Decide then, by ear. The likely answer, "run free while stopped, and Play puts the LFO
  back where the song left it", changes no offline render, so it needs no std bump.
- std's free-running sources (oscillators and `noise` at a song's top level) still run on while stopped, so for them
  RENDER-33 still needs a fresh engine. One that waited would hold a constant value (a DC offset).
- A smoother that sets itself from its first input (`Gain`, `Duck`, `Glide`) does that while stopped, from the input
  it has then. It matches the export only when that input at frame 0 is the same. Every fixture meets this today.
- A direct `kernel("std:chorus", …)` call (or phaser, tremolo, bitcrush) that leaves `playing` out always moves:
  the input's default is 1. std/fx connects it.
- G7f's S plays its own std (`a3214f6`): S's runner sets each project's std line to S's version. S can't play the
  app's std, whose std/fx reads `time.playing`.
- Every load of std compiles `std.wasm`, now half as big again: +70 ms a load, and the "rust tests" row +59 s on
  Linux. A compiled-module cache would win it back.
- A `target/` moved between worktrees keeps paths to the old one, and some tests then look for the repo there. If
  one moves, `cargo clean` it first. Tests that find the repo's files from where they were built were not all read;
  only the fixtures test passed quietly, and it now fails instead.
- The app's yellow compile bar shows each message on one line, cut at the window's edge. The std hint now comes
  first in its message (the review's D3); other long messages are still cut. Wrapping them is a choice for `taste`.

8a's live half (details in `step8/build-notes-8a.md`, each "For the lead", and `build-notes-8a-gaps.md`):
- The live thread count and park or spin wait for 8a's Mac sweep: one line each (`live_count`, `BETWEEN` in
  `core/play/src/engines.rs`).
- The Mac's real-time code (the Mach semaphore, the workgroup by the device's id, the time-constraint policy) has
  only been type-checked: it first runs on the owner's Mac. This server refuses real time, so a kept device pool
  and a granted `SCHED_FIFO` never ran here.
- A song with plugins renders on the device's thread alone: plugins on worker threads come later.
- G12 is built without `--server` (live while the song is edited), `--export` (beside an export), `--like-m1`
  and `--capture resample` (gate 8's row; the plan's cut order).
- A toast lasts 4 s, short for the three-line stop message (an owner or polish decision, in `web/`).
- A sound that stops in the few milliseconds before a new plan is taken isn't told.
- `Engine::crashes()` counts only an error that reaches the engine live; G12 adds the plan's stops to it.

Step 7 (its two gaps, a kernel crash that stopped the song and out of memory not said, are fixed: 8a's gaps lane):
- **K13:** a note that starts in the very block after a state change doesn't sound in Surge XT (Surge ends its voices when it applies a state).
- **K14:** a plugin that applies a loaded state later than its next call could still win over the code's values (HOME-5a's revert, with plugin windows, is the real fix).
- A new plugin instance whose saved state adds latency plays uncompensated, and nothing says so (RENDER-17; found in the finish).
- id-1's limit: a call of the same callee written on lines of its own beside another can take that call's instance (the deferred structural matching).
- rt-2's rest: an activation sent while held, then another structural message, then the resume, would still block the engine. The actor sends none while exporting.
- O4's edge: a held note whose Off falls in the first resumed block (within 2.7 ms) doesn't start again.
- A panic while held sends, at the resume, Offs for notes the export already ended: harmless.
- plug-1's flag stays set until a state load lands, so an export makes that plugin new each time until then (slow for a big sampler).
- K4: `CLAP`'s `render.h` couldn't be read here; the thread rule used for render modes (main thread, inactive) is from memory.
- K5: in release builds `no_alloc.rs`' zero-allocation tests check nothing (the callback marks its thread in debug only); CI runs debug.
- G7a's click check (K10a) on p50 at 3:00 catches a missing fade window, but not an edge crossfade made a cut (t02's pluck is quiet there).
- The churn run's +20 MB is read after 70 warm-up edits; the design doesn't say from when.
- The recorded tests aren't in `ci/check.sh`; a `--gates` step could run them on the Mac.
- `ci/check.sh`'s gates row says "usually 22 min": 17 to 18 here with the speed work's longer G7f turns, likely
  about 20 on the Mac.
- The speed work's cut 3 (one Store per voice pool, about −118 µs a callback more on Linux) and cut 4 (the
  non-finite check at activations, −14 to −21 µs) are held: cut 3 changes RENDER-32's words, so it is the owner's
  call, and only if the Mac misses. Cut 2 (the instance inline) was dropped: it didn't save on both CPU dies here.
- G7f's own error: with the app's engine on both sides (the A/A control), a callback came out anywhere from −145 to
  +38 µs from one run to the next on Linux held on one die, and up to +130 not held, as `ci/check.sh` runs it (the
  A/A table in `crates/gates/s/README.md`): about ±150 µs. A block came out within a few µs. A whole process runs a
  little faster or slower, all run long. Starting every side in a fresh process each round would let the rounds
  average that out; it needs a runner for HEAD's engine too. Unmeasured on the Mac.
- S's footprint and its drop beside another thread still come from `store-S.txt`: S's runner can't use m2-os.
- `machine()`'s thread count follows `taskset` and cgroups on Linux, so a pinned run names another machine.
- Linux's footprint leaves out shared memory (`RssShmem`), so it isn't the Mac's `phys_footprint`.
- `store-S.txt` and `callback-S.txt` hold one record each; a Linux record beside the Mac's needs the reader to pick by machine.
- `machine()` stays as it is: 8a's gates print the cores on a `cores:` line of their own, so old records still match.
- Platform design §3's rows OS-82, 83 and 84 still say "Designed (§8)"; they are built.
- The web shows neither device count (`xruns`, `device_errors`) yet (8b.8).
- The latency warning is a toast, not an entry in the problems list.
- Contract *Today* lines from before step 7 that are still stale: RENDER-4 and 5 say the range render isn't built (step 3a built it); RENDER-15, 21, 30, ID-3, ID-12, FMT-20, PKG-4, 8, 10, 11, MON-1, ABI-2 and 3 cite old line numbers.
- Old rustfmt diffs remain on lines no step 7 commit wrote (for example `crates/server/src/graph/tests.rs`).
- Three commits have no blank line after the subject (`e12746b`, `737575a`, `444fc66`): left, as commits aren't
  rewritten.

From the platform step:
- The macOS paths no test takes stay unrun: the barrier's `fsync` fallback, D6's clone error, `sync_dir` on a volume that refuses a folder sync.
- FSEvents: a link-count change being named (evt-2) is unproven on the Mac.
- Step 8's folder push, after a save's renames, still ignores errors, `EIO` too (D2).
- QuickJS's 256 MB limit counts bytes as each OS's `malloc` does (det-3); a contract deferred row.
- `retry_lock` scans with no journal skipped: after a lost lock and an outside edit, Retry could set the session's own completed journal aside. Rare.
- WRITE-15's *Today* names only step 8's push as ignoring errors; a rollback's and the lock's tidy's do too.
- D4's words name inotify's instance limit for `EMFILE`, which is also the process's own file limit.
- An export name over 63 bytes is cut by QuickJS, so its error loses its place.
- The kill test takes about 130 s of `m2-session`'s test time; split per case, its cases would run side by side.
- `surge_plays_notes` can't see a note passed up to 31 samples late (Surge's blocks); the probe plugin's test can.
- inotify running out of watches after start is untested: lowering the limit needs root.
- `apps/music2-desktop/src/main.rs` still says the plugins' main thread is AppKit's (true on macOS only).
- Windows: `push_tree` opens files read-only, but `FlushFileBuffers` needs write access; platform design §8 lists the Unix-only spots.
- The real home here holds a stray `~/Library/Caches/music2/hashes.json` from before S6; left as it is.

## How the owner wants the work done

- **Quality over speed.** Full checks over lighter ones. For each step: design, attack, build in green slices, verify
  (`ci/check.sh`: every test, clippy, every browser suite, the renders; the Mac's timing gates), multi-lens review,
  skeptic confirmation, fix. Commit and push each step once it passes.
- **Simple yet elegant**, a tasteful professional DAW (good enough for A. G. Cook). Don't be afraid to change
  earlier work when it makes things simpler.
- **Writing to the owner:** they are dyslexic and new to music. Short sentences, plain words, tables,
  "In short" first, no walls of text.
- No built-in AI in the app. No backwards compatibility.

## The server

| What | How |
|---|---|
| The machine | `jerry-ax102`: Ubuntu 24.04, Ryzen 9 7950X3D (32 threads), 124 GB, ext4 on RAID, about 2 TB free. No sound card. Its drives have power-loss protection, so its timings are a best case. Other agents' jobs often run beside yours: Linux timings are information only (PLAT-1). |
| Long jobs | `~/m2-runs/tw <name> '<command>'` runs it in a window of tmux session `m2-sh`, which the owner watches; the log goes to `~/m2-runs/logs/`. It waits 9.5 minutes (`TW_WAIT=<seconds>` for longer), then prints `~/m2-runs/tw-wait <channel> <log>` to wait again. No double quotes or parentheses in the command. Never send keys to the tmux sessions `m2`, `1`, `2` or `ssh_tmux`. |
| Test plugins | `ci/plugins.sh` (once): Surge XT 1.3.4 and Airwindows Consolidated into `~/.local/share/m2-test-plugins/clap`, from the archives in `~/.local/share/m2-test-plugins/dl` (Airwindows' zip is gone upstream: keep that copy). Never into `~/.clap` or `/usr/lib/clap`. |
| The desktop app | Only on an invisible Xvfb screen, as the check's app row opens it. Never on a real screen. |
| Scratch `HOME` | Anything that runs music2 gets a scratch `HOME` and `M2_AUDIO=none`, and then `CARGO_HOME=/home/jerry/.cargo RUSTUP_HOME=/home/jerry/.rustup`. Never the real `~/.music2` or `~/Music`. `ci/check.sh` does this itself. |
| Scratch copies | `git worktree add --detach ~/m2-runs/scratch-<label>`, with its own `target/`; delete it when done (they once filled 390 GB). The repo's own `target/` is about 140 GB. |
| Logs | Each check's folder in `~/m2-runs/check-*` (8a's live half's final: `check-8a-live-3`, with the gates, at `21a7a1b`; `-2` passed at `c924e0b` before `main` moved, and `check-8a-live`, its first, failed under load; 8a's offline final: `check-8a-fg`, with the gates, at `bc41693`; the `sounds` merge's: `check-sm1c` and `-sm2b` with the gates, `-sm3`, `-sm4`, and the final `-sm-fg` with the gates, at `e108e46`; step 7's last: `check-s7-rv1`, and `-rvg` with the gates; before them `-sp1`, `-sp2` and `-spg`); the `sounds` merge's plan in `~/m2-runs/sounds/plan.md`; the step 7 gate runs in `~/m2-runs/gates-fix/` and `~/m2-runs/xc/g7f/`; the review in `~/m2-runs/review-b1/`. |
| Tools | Rust 1.99.0 (`rust-toolchain.toml`), Node 24, chrome-headless-shell 133.0.6943.141 (the check installs Rust and Chrome when missing), Xvfb. |
| MIDI | `/dev/snd/seq` gives `EACCES` here: tests inject notes and never open a device. |
