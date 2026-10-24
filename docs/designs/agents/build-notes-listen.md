# Agents beside the musician: build notes for track 5, `ag-listen`

**In short**
- Track 5 of the [agents design](design.md), the last, is built on branch `ag-listen`. An agent can now measure what it can't hear: `m2 listen <song>` says the mix's and each track's loudness, true peak, crest, octave bands, stereo, and which tracks mask each other.
- Each track is read two ways: its own sound, before its mixer channel, as a stem is; and as it reaches the mix, after its channel's pan and fader.
- It renders the bars itself, in its own process, never through the app's playback: at background priority (its plugin host too; on a Mac the efficiency cores, on Linux `SCHED_IDLE`), one at a time on the computer. It writes nothing into the song.
- The render gets **copy taps**: after each block, a copy of each buffer it reads. They change no bit of what plays or renders, and only an offline `Copying` has them, which the live engine can't take.
- **The review's fixes** (F1 to F17) are in the last section: warm meters, masking moment by moment among what the master adds up, buses as strips, mute and solo by key, the same bars across a change, and a listen that refuses what it doesn't know and never waits long on the app. Checked at `bfe1a4c`.
- `--with tx.json` measures a change before it is made (checked, never written), with the difference; `--as-heard` hears the musician's listening mute and solo, and says so; `--png` draws the bands over time.
- Gate **G13** plays p50 live while `m2 listen` runs back to back and `m2 set` edits are forwarded to an app beside it.
- The contract changed first: AGENT-7, MON-4, and the text of RENDER-3, RENDER-4, the meters' and the agents gate row ([architecture.md](../../architecture.md#agents)); the gates' README described G13.
- 17 tests were written first and seen failing, then the code made them pass. No UI changed; the picture `--png` draws is in `~/m2-runs/sag/listen-shots/`.
- **All five tracks of the design are built.** Small loose ends are in [gaps.md](../../gaps.md).

## What was built

| Layer | What |
|---|---|
| `core/runtime` | Copy taps (`taps.rs`): `Renderer::copy_taps(at, from)` with `TapAt::Out`, `Node(i)` or `Site(k)`. After each block from the one that holds transport frame `from`, `Copies` holds a copy of each tapped buffer, with the block's transport frame and each tap's lateness (a node's offset and latency, RENDER-18 and RENDER-19: a delay's is its input's plus its length). The analysis reads them between chunks and clears them. A plan the live engine plays, or one prepared for it (it has meters), refuses them. The plan keeps each node's and each site's channels and arrival for it (`Plan.sound`, `Plan.sites`). |
| `core/os` | `process::background()`: the whole process at background priority (Linux: nice 10 on each of its threads, which new threads take from their creator; macOS: the process's Darwin background state). `process::in_background(pid)`: what the OS says of a process. |
| `core/plugins` | `Remote::background()` (and `m2_plugins::background_host()` for the program's own host): its host processes start with `--m2-background`, which the host applies first thing. |
| `crates/editor` | `listen.rs`: the measures, from the standards, no FFT and no crate: `Meter` (loudness per BS.1770 gated as EBU R128, integrated and short-term max, from 10 ms steps; true peak 4× oversampled with a windowed sinc, skipping blocks that can't beat the peak found; peak, RMS, crest, clipped, silent; octave bands as 4th-order Butterworth band-passes; stereo correlation, width and mono lows; third-octave columns for the picture) and `masking`. `listen/hear.rs`: which buffers (`tapped`: each track's overlay site into the song's module and its channel's fader), the range render that copies and measures them (`hear`), the JSON and the words, and the difference for `--with`. `listen/png.rs`: the picture, and a PNG writer of its own. `mix::fader`: a channel's fader node. `render::pure` is shared. |
| `crates/server` | `graph::view_file::looped` is public: `m2 listen` reads the loop a closed song keeps. |
| `apps/m2` | `listen.rs`: `m2 listen`: the range (bars, the loop, the song; 2 bars of preroll), the app's loop and mute and solo when it has the song open (read, never written), `--with` through `Session::check`, `--as-heard` through the overlays' values (MON-3), `--png`, the lock, the priority, the JSON and the words. `main.rs`: `read_tx` and `text_bases` shared with `m2 tx`. |
| `crates/gates` | G13: `realtime --beside agents` (`realtime/beside.rs`): the agents start before the live run and stop after it. `ci/gates.txt` runs it a minute; `ci/check.sh` builds `m2` beside `m2-gates`. |
| Docs | The contract; the gates' README; the manual (the command and a Listening section); README's In short; AGENTS.md; gaps; the design's status line; HANDOFF. |

## The tests, and how they failed first

Each was written against the new API with bodies that did nothing (or before the command existed), and run before the code. Logs in `~/m2-runs/sag/listen-logs/` on the build server.

| Where | Tests | Failed first |
|---|---|---|
| `core/play/tests/bit_identity.rs` | `copy_taps_change_no_bit`: every pure fixture with a copy tap on the output, on every node with an audio output and on every overlay site, renders the same bits, and the output's copy is what rendered | yes, no copies (`fail-first-copy-taps.log`) |
| `core/runtime/src/tests/taps.rs` | `copy_taps_copy_each_block_from_the_frame_asked` (a site is the track's own sound, a node that node, from the block asked), `copy_taps_are_offline_only` (a live plan, a constant, no such site or node) | both (`fail-first-copy-taps-runtime.log`) |
| `crates/editor/src/listen_tests.rs` | `a_1_khz_sine_at_minus_23_dbfs_reads_minus_23_lufs`, `the_other_ebu_tech_3341_cases_read_as_it_says` (cases 2 to 5, 9 and 10, and silence under the gate), `true_peak_finds_the_peak_between_samples` (case 19's 12 kHz sine at +3 dBTP, cases 15 to 18's half-scale sines, clipped samples), `pink_noise_has_flat_octave_bands`, `two_tracks_in_one_band_are_named_as_masking`, `a_track_turned_down_in_the_mix_masks_nothing`, `mono_correlates_1_and_inverted_minus_1`, `k_weighting_at_48_khz_is_bs_1770s_table` | all 8 (`fail-first-measures.log`) |
| `apps/m2/tests/listen.rs` | `m2_listen_reports_every_track_and_the_mix`, `m2_listen_with_a_tx_writes_nothing` (with the app open too), `m2_listen_as_heard_applies_the_listening_mutes_and_says_so` (the file's, then the open app's), `m2_listen_png_writes_a_band_picture`, `m2_listen_runs_at_background_priority` (its threads' nice while it waits for the lock, and what it says of itself and its plugin host), `a_second_listen_waits_for_the_first` | all 6, before the command existed (`fail-first-m2-listen.log`) |

Written with the code: `a_host_asked_to_runs_at_background_priority` (`core/plugins`), then seen failing with the flag left out (`fail-first-host-background.log`): on Linux a host process takes its nice from the thread that starts it, so `m2 listen`'s own test passed without the flag, and this test is the one that needs it. `png.rs`'s `the_checks_are_the_standards` (Adler-32's and CRC-32's own check values), and the gates' `the_help_says_what_g13_adds`. G13 isn't shown failing first, as the design says: work at background priority may not displace real-time threads at all.

Two tests changed while they were made, not after: the true-peak test fades its sines in over 20 ms (a step from silence has peaks of its own between samples, which the meter rightly found), and `m2_listen_with_a_tx_writes_nothing` checks the mix's 16 kHz band, not its loudness (the hats are 26 LU under the mix, so 6 dB more of them moves its loudness by under 0.05 LU).

## Choices against the design

Where the design was unclear, or didn't fit today's code, this is what was done and why.

1. **`copy_taps_change_no_bit` is in `core/play/tests`**, not `core/runtime`: the pure fixtures are compiled there, and `core/runtime` has no compiler. It taps more than a listen does: every node with an audio output and every overlay site. Two smaller tests are in `core/runtime`.
2. **Copy taps start at a frame.** Blocks that end before the range aren't copied, so a long preroll, or a render from the song's start, costs no memory; the analysis drops the rest by each tap's song time.
3. **A track's own sound is its overlay site into the song's own module**, where std's mixer is (else its first site), as the design says: "its own sound is read where it enters the mixer". With `--as-heard` a muted track's own sound is silent too, as its meter is.
4. **As it reaches the mix is the channel's fader**, the gain after the pan. The Mix view's walk finds the first node of a channel (the pan when there is one), so `mix::fader` finds the gain that reads `channels.<key>.db`; a fader an automation lane sets reads no constant, and is found by its channel's place among the gains std's mixer made (gaps).
5. **A channel two tracks share is no track's strip**: it carries both. The design names a track in no channel or in two; this adds one in a channel with another track. Each says why, and that its masking isn't measured.
6. **Masking is one group a band, not pairs**: p50 gave 2,809 pairs; each band's group holds every pair. Two rules were added to the design's "within 6 dB of each other and of that band's loudest": louder than −60 dBFS in the band, and the band within 10 dB of the track's own loudest. A 4th-order band-pass lets a 63 Hz tone read −13 dB in the band beside it, and without the second rule every bass would mask every kick at 31.5 and 125 Hz too.
7. **The true peak's filter is a windowed sinc** (Lanczos, 8 lobes a side, 16 taps a phase, each phase's gain 1), not BS.1770 Annex 2's 48-tap example: the Annex gives its filter as an example, this one is flatter near the top, and the tests hold it to EBU Tech 3341's tolerances.
8. **No `png` crate.** The design says it is in `Cargo.lock`; it is, but no crate of the workspace depends on it, so using it adds it to that crate's list in `Cargo.lock`, which this track may not change. A PNG writer of its own (stored deflate, CRC-32, Adler-32), as track 3 wrote its own WebSocket client. The PNG isn't compressed (gaps).
9. **The picture's rows are the mid** (both sides' mean), not each side: one picture a block. With `--tracks`, each track's block is as heard (its own sound when it has no strip).
10. **The whole process at background priority**, not each thread: `m2_os::process::background` covers the threads it has and the ones they start, and the plugin host gets a flag. `m2 listen` always renders on `Threads::Beside`: `--threads` and `M2_THREADS` don't apply. Its JSON says what the OS says of its own threads and its host (`priority`).
11. **With the app open, `m2 listen` reads the files**, as `m2 render` does ("it reads committed files"), and asks the app only for its loop and its mute and solo (`GET /api/graph`). An app it can't reach is passed over: the files' loop and mute and solo are read instead.
12. **The whole song stops at its last clip** (the design said "the song"), and bars past it are refused, as `m2 point` refuses them.
13. **`--with`'s difference holds only what changed**, and the words list each band that moved a dB or more apart from the track's loudness, so a fader's change reads as one number.
14. **`--tracks` measures those tracks alone**, the mix always; masking is among them.
15. **G13's app is the gate's own server**, in its process, holding p50's lock with a `session.json` in a scratch `HOME`, as the tests of `m2` stand for the app: what plays is G12's player beside it (gaps). `m2 listen` runs on bars 33 to 40, not the whole song, so several finish in the minute (4 on Linux, and 95 forwarded edits). Its timings are recorded, on the Mac too, until its first runs there are; that the listens and edits landed, at background priority, is judged everywhere.

## Gaps added

Under "Scripts, agents and the command line" in [gaps.md](../../gaps.md): masking over the whole range, not moment by moment; a strip only in std's mixer; an automated fader found by its channel's place; the analysis on one thread; the PNG uncompressed and unlabelled; no loudness range or momentary loudness; bars stop at the last clip; G13 measures its own player, not the app's. And in HANDOFF: G13's first Mac runs are recorded, then judged.

## Checked

- The crates touched: `m2-runtime`, `m2-play`'s bit-identity tests, `m2-editor`, `m2-os`, `m2-plugins`, `m2-gates`'s own tests and `m2`'s: every test passes, and clippy with every warning an error, for Linux and for `aarch64-apple-darwin`.
- `m2 listen` on first-song: about 0.9 s for its 8 bars (debug build); on 16 bars of p50, about 30 s, of which the render is 25 s, on one thread.
- G13 once by hand at `56389a2`: PASS; 4 listens (14.1 s each), 95 forwarded edits (0.6 s each), every listen in the background; on the silent clock, 4 late callbacks in 22,500, recorded.
- The picture: `~/m2-runs/sag/listen-shots/first-song-bands.png` (first-song, the mix and its four tracks). Looking at it widened the gap between blocks from 2 to 6 pixels, in a lighter grey: at 2 they ran together.
- No UI changed, so there are no screenshots of the app.
- `ci/check.sh --pr` at `ba42c67`: `music2 check --pr: PASS` (quick, clippy, rust, suites, renders, repo unchanged; 6 min), 1,899 Rust tests, 54 of 54 browser suites (1,966 checks), nothing passed only on a retry. The run before it, at `73cb04f`, failed only its fmt row: the new code wasn't formatted (`ba42c67` is `cargo fmt`, no other change). Logs: `~/m2-runs/check-listen.tgz` on the build server.
- The timing gates, as `ci/check.sh`'s step 11 runs `ci/gates.txt`, at `ba42c67` on Linux: all 8 runs PASS (G11 tx-data and watcher, G5 and G7a through the server, G5 and G7b, G7e, G7f store and arena, G12 live, G13); timings recorded, not judged (PLAT-1). G12 alone: callback p99 484 µs, 8 late callbacks in 22,500; G13: p99 588 µs, 5 late, while 4 listens and 96 forwarded edits landed, every listen in the background. Log: `~/m2-runs/sag/listen-logs/gates-at-ba42c67.log`.

## The review's fixes

The review (`~/m2-runs/sag/review-listen.json` on the build server) found 17 things, each confirmed or
plausible. Each fix has a test, seen failing first against `cdce4cb` (the
tip before the fixes): the logs are in `~/m2-runs/sag/listen-logs/review-*.log` on the build server. The
contract changed first (`f5598e8`: AGENT-7, the meters' paragraph and the agents gate row).

| Id | What was wrong | What changed | The test that failed first |
|---|---|---|---|
| F1 | Each meter started cold at the range's first frame: a range starting on a loud note read a step from silence, up to 1.1 dB of true peak the song hasn't got. | The listen copies from up to a second before the range (its preroll), and `Meter::warm` runs every filter and the true peak's history on it without counting it. | `a_meter_warmed_before_the_range_reads_no_step_from_silence`; `a_range_that_starts_on_a_loud_sound_reads_its_true_peak` (0.1 dBTP against the whole song's −1.0) |
| F2 | `--as-heard`'s values were the song's, applied to the change by position: a Tx that took the kick out of the mixer unmuted the keys. | `Ask` carries an `Overlaid`: what gives each revision its values from its own tracks, by key. | `as_heard_mutes_by_key_in_the_change_too` |
| F3 | A loud track's leak into the band beside its own set that band's loudest, and hid a real clash there. | The loudest is the loudest of the tracks that count in the band. | `a_loud_tracks_leak_never_hides_a_clash_beside_it` |
| F4 | Masking was judged over the whole range: tracks that take turns were named, and a bass ducked under the kick too. | 50 ms moments: two tracks mask each other only in a moment where both sound; each pair says the share of the range. | `tracks_that_take_turns_never_mask_each_other`, `a_bass_ducked_under_each_kick_masks_it_only_where_they_meet`; `tracks_that_take_turns_hardly_mask_each_other` |
| F5 | A track going to a group bus was read before the bus's fader, and the bus wasn't measured. | Masking is among what the master adds up: a bus is a strip of its own (its fader; its own sound is what enters its strip), and a track that goes to a bus is read at its own fader and says so (`to`, `why`). | `a_group_bus_is_measured_as_it_reaches_the_mix` |
| F6 | On Linux the listen was only at nice 10, "the cores playback doesn't need" wasn't true there, and G13 ran no plugin host. | Every thread of the listen and its host in `SCHED_IDLE` and the idle I/O class (`core/os`), and the contract says what background priority is on each OS. G13 listens to `examples/with-plugins` every other time and judges its host's priority. Numbers below. | `m2_listen_runs_at_background_priority` (every thread's policy) |
| F7 | Copy taps stayed on a renderer handed to the live engine afterwards, which then copied and allocated on the audio thread. | They live on `Copying`, an offline render that owns its renderer and never gives it up; `Renderer` has none. | The review's scenario (18 allocations and 880 blocks in 200 callbacks); against the new API it can't be written, which is the fix, so it isn't kept. `copy_taps_are_offline_only` keeps the refusal of a live renderer. |
| F8 | `--with` a change of tempo compared different music under the same bar numbers. | Each revision turns the same bars (or the loop's or the song's beats) into its own seconds; `moved` says when they moved. | `a_change_of_tempo_compares_the_same_bars` |
| F9 | The difference dropped a measure one side had and the other hadn't: a track gone silent didn't show. | `{"was", "now"}` for those, `added` and `removed` tracks, masking that changed its share, and the words say "now silent". | `the_difference_says_a_track_went_silent`; the kick in `as_heard_mutes_by_key_in_the_change_too` |
| F10 | The rule "within 10 dB of the track's own loudest band" dropped real harmonics: a dark bass never met a pad at 250 Hz. | The rule is gone. A band is a track's own unless it is 16 dB or more under a band beside it; the octave bands are 8th-order, so a neighbour's tone reads 26 dB down. | `a_dark_bass_and_a_pad_level_in_the_low_mids_mask_each_other` |
| F11 | `--tracks kick,kick` made the kick mask itself. | Each key once. | `a_track_named_twice_is_measured_once` |
| F12 | Unknown flags passed, `--png --json` wrote a file named `--json`, a picture could go into the song, and an unwritable one failed after the render. | The arguments are parsed whole; the picture's path is claimed before anything renders. | `m2_listen_refuses_what_it_doesnt_know`, `the_arguments_are_what_m2_listen_knows` |
| F13 | A song with no clips said to name bars, then refused every bar. | One message, the same both ways. | `a_song_with_no_clips_says_so_once` |
| F14 | One NaN in a tap nulled its loudness, bands and correlation, and nothing said so. | Counted per meter (`non_finite`), read as 0, said in words. | `a_sample_that_isnt_a_number_is_counted_and_read_as_zero` |
| F15 | A second, unchecked GET; a wait on a busy app while holding the listen lock; a silent fall back to the files for `--as-heard`. | The state the reach checked is used; the app is asked only for the loop or the mute and solo, 5 s at most, before the lock; `app` says what was read, or that the files stood in. | `a_busy_app_never_holds_up_a_listen` (an app that never answers: the old listen was still waiting after 60 s) |
| F16 | `--with` read the song twice, so an app's write between could land in the difference. | The Tx is checked on the session that read "before". | None: a race between two reads, fixed by reading once. |
| F17 | A plugin window's unsaved change wasn't mentioned. | The graph state's `plugins[node]` says `unsaved`; `m2 listen` names them when it asked the app (gaps: not when it didn't). | `the_state_says_a_plugin_window_has_changes_not_saved_yet`; `the_unsaved_plugins_are_named` |

What held, and still holds: nothing is written into the song; the lock; EBU Tech 3341's cases and the true
peak's tolerances; the PNG; exit codes; `--tracks ''`; a mono tap read as left = right; and every pure
fixture's bits with copy taps. Every earlier test passes unchanged, but the two masking tests, which now give
moments, not one level a band, and claim the same (`two_tracks_in_one_band_are_named_as_masking` also
asserts the pair meets all the time; its tracks sound through the range, the meter warmed before it, as a
listen's are).

### Where it differs from the leanings

- **Moments of 50 ms**, not 400 ms or a beat: at 400 ms `a_bass_ducked_under_each_kick_masks_it_only_where_they_meet` fails (share 0.8, `review-moment-400ms.log`), because a beat's window always holds both the kick's hit and the bass after it. 50 ms is also the picture's column.
- **A track counts only in a moment it sounds in** (its whole signal louder than −60 dBFS): a band-pass rings on for a moment after a track stops, and without this rule two tracks that take turns met at each turn (4% of the range).
- **The rules kept**, each needed by a test (`review-rules-needed.log`): −60 dBFS (two near-silent tracks); and in place of the 10 dB rule, a band 16 dB or more under a band beside it is that band's leak (`two_tracks_in_one_band_are_named_as_masking` names a false 31.5 Hz pair without it). The bands went from 4th to 8th order so a neighbour's leak (26 dB down) and a dark bass's real harmonics (about 11 dB down) sit well apart.
- **Buses**: masking among what the master adds up, rather than each grouped track scaled by the bus faders on its way: an effect on the bus (a compressor) leaves a track's part of the bus no single number, and the bus as a strip is what reaches the mix. Each grouped track says so.
- **Linux cores**: not pinned. The engine's threads aren't pinned on Linux; sampled during G12 they ran on all 32 CPUs, so no core is playback's. Holding the listen to the other L3 cache (`taskset -c 8-15,24-31`) and core scheduling (`PR_SCHED_CORE`, so no listen thread shares a core with playback at once) were tried: the first changed nothing, the second made the tail worse (p99.9 2,156 µs, 13 late callbacks). `SCHED_IDLE` and the idle I/O class are kept: on a machine with few cores, where playback wants them all, they give way at once.

### F6's numbers

G12 (p50 live on the silent clock, 128 frames, one minute) alone, then with `m2 listen p50 --bars 33-40` back to
back beside it, on the build server (Ryzen 9 7950X3D, 32 threads); scripts and logs in `~/m2-runs/sag/listen-f6/`
and `~/m2-runs/logs/f6-*.log`. Recorded, as Linux timings are (PLAT-1).

| Run | Callback p50 | p99 | Late or gaps |
|---|---|---|---|
| Before (nice 10): G12 alone / with listens | 312 / 356 µs | 488 / 552 µs | 5 / 3 |
| After (`SCHED_IDLE`, idle I/O): alone / with listens | 288 / 352 µs | 480 / 540 µs | 3 / 9 |
| Tried: held to the other L3 cache | 296 / 364 µs | 488 / 544 µs | 3 / 7 |
| Tried: core scheduling | 284 / 344 µs | 476 / 552 µs (p99.9 2,156) | 2 / 13 |
| A bare busy loop at `SCHED_IDLE` beside G12 | 332 µs | 500 µs | 7 |
| G13 after (listens of p50 and of `with-plugins`, and edits) | 372 µs | 604 µs | 8 (the builder's G13: 380 and 588) |
| In the full check at the fixes: G12 alone / G13 | 304 / 368 µs | 508 / 568 µs | 1 / 6 |

A listen costs G12 about what one more busy core costs on this machine (the clocks and caches the cores share),
not CPU time it takes from playback; gaps says so.

### Checked

- The touched crates' tests (`m2-runtime`, `m2-play`'s bit identity, `m2-editor`, `m2-os`, `m2-plugins`, `m2-server`, `m2-gates`, `m2`) and clippy on them with every warning an error.
- G13 once by hand at the fixes: PASS, 6 listens (3 of them of `with-plugins`, each with its host in the background) and 96 forwarded edits.
- The picture after the change to the bands: `~/m2-runs/sag/listen-shots/review-first-song-bands.png` (first-song, bars 1-4: the mix, the kick, the hats and the bass), as before.
- `ci/check.sh --gates=auto` at `bfe1a4c`: PASS, 1,957 Rust tests, 76/76 browser suites, 12 of 12 renders, the 8 gate runs, 29 minutes, nothing passed only on a retry. Logs: `~/m2-runs/check-listen.tgz` on the build server.
