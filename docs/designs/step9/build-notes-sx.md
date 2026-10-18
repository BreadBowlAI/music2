# Track sx: build notes

Branch `sx`, from `main` at `97b4cdf`, 2026-10-18. The plan is [sx-plan.md](sx-plan.md); what changed while building is written there, under each slice.

## In short

- **Slice 1, kernel ABI v1 settled:** music2 reads a module's ABI version from its bytes first. A kernel may read its audio file from the disk.
- **Slice 2, `std:stretch`:** warped clips are stretched by a std kernel now, not by the engine. It is the first **offline kernel**: music2 runs it off the audio thread, in pieces. Neither the file nor the stretch is ever whole in memory.
- **Slice 3, Follow tempo:** an audio clip's menu has **Follow tempo**. Turn it on, and the clip keeps its beats when the tempo changes. A live session never waits for a stretch: it is made in the background, and the clip plays it as it is made. An export makes its stretches first, so its bits never depend on timing.
- One example sounds a little different: `examples/resample` (its 2 renders, slice 2). The other 10 renders keep their bits. Slice 3 changes no render.
- Linux timings are information only. The Mac's run is owed (PLAT-1).

## Slice 2, piece by piece

| Piece | Where | Test (each red first) |
|---|---|---|
| The kernel: WSOLA, channels aligned together, the match normalized by loudness, pitch by cubic Hermite | `std/kernels-src/src/stretch.rs` | `a_stretch_keeps_its_pitch`, `a_stretch_is_as_long_as_its_map_says`, `the_identity_map_gives_the_source_back`, `a_stretch_has_no_clicks`, `pitch_with_a_map_transposes_and_keeps_time` |
| The kit: `Cfg::offline()`, `Cfg::time_map()` | `std/kernels-src/kernel/src/lib.rs` | the kernel's tests use them |
| Offline kernels in the manifest; song code gets `kernel.offline` | `core/runtime/src/packages.rs`, `core/ir/src/check.rs` | `song_code_cannot_call_an_offline_kernel` |
| A run: one sandbox, `flags` bit 0 and the time map in its settings, block by block into a sink | `core/runtime/src/offline.rs`, `sandbox.rs` | `a_warped_clip_is_stretched_by_std_stretch` |
| The cache: the time map from the markers and the tempo; the stretch written in pieces; gain, fades and reverse as it is read | `core/runtime/src/audio.rs`, `stream.rs` | `a_warped_render_holds_neither_file_whole`, `an_export_with_an_empty_cache_folder_equals_one_with_a_full_one`, `a_streamed_lane_is_the_rendered_lane` |
| The example on many threads | `core/play/tests/bit_identity.rs` | `the_resample_example_gives_the_same_bits_on_1_to_7_threads` |

The engine's own WSOLA and `WARP_VERSION` are gone. A stretch's file is named by all the kernel is given, std's module hash among them.

## Slice 3, piece by piece

| Piece | Where | Test (each red first) |
|---|---|---|
| A live cache's stretch thread: renders in the order asked; a dropped one stops and its part file goes | `core/runtime/src/renders.rs`, `audio.rs` | `a_tempo_change_never_waits_for_a_stretch`, `a_dropped_render_stops_and_leaves_no_part_file` |
| A render knows its frames done; a chunk fills only once they are made | `core/runtime/src/stream.rs` | `a_lane_plays_its_render_as_it_is_made` |
| An export makes its own renders first | `Cache::default()` is offline | `an_export_never_plays_a_render_half_made` |
| The disk thread skips chunks not made yet, and is woken as renders go | `core/play/src/disk.rs` | `a_player_plays_a_render_as_it_is_made` |
| The session's cache is the live kind | `crates/server/src/graph/actor.rs` | the server's tests |
| Follow tempo: markers at the clip's start, end and each tempo change, from the tempo now; off clears them | `crates/editor/src/clips.rs` | `follow_tempo_keeps_the_clip_where_it_plays_and_undo_takes_it_back`, `follow_tempo_through_the_clips_route` |
| The menu row and its hint; a warped clip's waveform drawn by its markers | `web/src/components/Wave.tsx`, `ClipMenu.tsx` | browser suite `sx-follow` (22 checks); `st-audio` knows the new row |

Pictures (Linux, Xvfb): `~/m2-runs/app-shots/sx-3/`. `menu/app-10s.png` is the clip menu with Follow tempo and its hint. `followed/app-10s.png` is the song at 60 BPM after Follow tempo on its first clip: that clip shows the same part of its file as at 120, and the clip beside it, not followed, shows more. `followed/app-20s.png` is the menu again, Follow tempo checked.

## Renders (FMT-3)

| Render | Changed? | Why |
|---|---|---|
| `examples-resample.wav`, `examples-resample-from4-for6.wav` | yes | its 2 warped clips are stretched by std 0.2.2's kernel: the time map as pairs, the match normalized by loudness, and no fade-in at a stretch's start. std went from 0.2.1 to 0.2.2 |
| the other 10 | no | |

## Speed (Linux, information only)

| What | Time |
|---|---|
| 60 s of stereo 44.1 kHz, stretched to 75 s and up 3 semitones, into a 48 kHz song, written to its file | 0.25 s: about 300 times faster than real time (`stretch_speed_report`) |

## A check by ear

About 5 minutes. No test can do it, as the tests play no sound.

| # | Do | You should hear |
|---|---|---|
| 96 | Click **Open an Example**, then **Resample**. Press Space. Listen from 3 seconds in (bar 2, beat 3) to 5 seconds, then from 6 seconds (bar 4). | At 3 s the keys come back slower, stretched over 4 beats. Their pitch is the same as at the start. No wobble, no clicks, no echo. At 6 s they come back higher and stretched, still on the beat. |
| 97 | Tell the lead what you heard at 96. | |
| 98 | Open **Resample** again. Right-click the first clip on Chops (bar 1). Click **Follow tempo**. Press Space from the start. | It sounds as before. |
| 99 | Double-click the tempo (120), type 90, press Return. Press Space from the start. | The first clip keeps in time with the kick, a little slower, at the same pitch. The second clip (not followed) plays more of its sound in the same time. |
| 100 | While it plays, drag the tempo up and down a little, then let go. | The followed clip goes on in time. At most a short silence, no click, no wrong sound. |
| 101 | Press ⌘Z twice. | The tempo is 120 again, and Follow tempo is off. |
| 102 | Tell the lead what you heard at 98 to 101. | |
