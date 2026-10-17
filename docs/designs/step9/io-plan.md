# io: the plan

For the `io` track: branch `io`, worktree `/home/jerry/m2-runs/wt/io`, made from `main` at `258275b`. Step 9's
audio-and-sample turns 1 and 4 ([Order of work](../../architecture.md#order-of-work), item 9): disk streaming, then
recording audio (gaps #25). Written 2026-10-18 by a planning agent: files read, nothing built or run.

**The Decisions section overrides the rest of this file.**

---

## In short

- **Long audio plays from the disk.** A clip reads its file a little ahead of the playhead, on a thread of its own.
  The sound thread only copies what is ready. It never opens, reads or waits on a file. A 1-hour file never sits
  whole in memory.
- **Every export stays the same.** Exports read the same samples the same way, so `e2e/renders.sha256` doesn't change.
- **You can record a voice or a guitar.** On an audio track, R (or the Record button) counts in one bar, then records
  from the sound input into a new file in `assets/`. R again ends it. The take becomes one clip, one undo step
  ("recording on Vox").
- **You can hear yourself while you record**, through the track's effects. It is off unless you turn it on in
  Settings, so speakers don't howl.

| Slice | What you get | Size |
|---|---|---|
| **1. Disk streaming** | Long files play from the disk. Seeks, loops and tempo changes find their audio ready | L |
| **2. The input and the take** | The engine can read a sound input, write a take safely, and place it as a clip | L |
| **3. Recording in the app** | R records audio on an audio track. Settings picks the input and "Hear yourself" | M |

Build order: 1, 2, 3. Slice 2 touches no file slice 1 does, so it may start beside it. One full `ci/check.sh` at the
end of the track; each slice runs its own tests and the renders.

---

## Decisions

Nobody waits for an answer, and nobody asks the owner.

### Disk streaming (slice 1)

| # | Question | Decision |
|---|---|---|
| D1 | What streams? | **Every audio clip.** A clip with no warp (plain, pitched, reversed, faded) is worked out from its WAV in `assets/` as it plays: no copy. A **warped** clip is rendered once into the cache folder (`<cache>/renders/<key>.f32`, FMT-16's folder, keyed by the clip's `ClipKey`), then streamed from there |
| D2 | Warp's one render | It still holds the source and the render in memory while it is made, as today. The stretching turn (item 9's third) replaces WSOLA anyway, so it is not rewritten now. One line in gaps.md |
| D3 | How files are read | `m2_os::fs::read_at` (Unix `pread`; Windows `seek_read` later), only on the disk thread or an offline render's own thread. `hound` decodes from a reader over it: the same decoder, so the same bits. **No memory map**: a mapped file cut short by another program kills the app (SIGBUS), and a read-ahead thread needs no map. RENDER-30 is amended to say so |
| D4 | What the sound thread reads | Per audio lane, a ring of **slots**. Each slot holds one **chunk**: 0.25 s of the lane's summed samples (`rate / 4` frames), in canonical clip order, as `AudioLane::fill` sums them now. Each slot has an atomic tag (chunk number, or "being written"). The sound thread finds the tag, copies, and checks the tag again. Samples are `AtomicU32` bits (plain loads), so a torn read is never undefined |
| D5 | How many slots | `min(16, chunks the lane has audio in)`. A chunk with no clip in it is silence and takes no slot. A short lane is wholly resident, as today. 16 slots of stereo at 48 kHz is 1.5 MB a lane |
| D6 | What the disk thread keeps ready | One thread per player, `m2-disk`, at background priority. Around the playhead: the chunk before and 2 s ahead, **along the transport's path** (through a loop's wrap). While a loop is on: the loop start's first 0.5 s. Stopped: the same window, so Play starts at once |
| D7 | A miss | The sound thread never waits. A chunk not ready plays silence for that block, and the lane's miss count goes up. The status pill says "The disk was too slow: Vox skipped a moment." |
| D8 | Seeks | `Player::seek` asks the disk thread for the target first. When it isn't ready, it waits up to **30 ms** (gate 3 allows 50 ms), then sends the seek anyway. A target already ready costs nothing |
| D9 | Arrangement switches and edits | `Player::activate` registers the new plan's lanes with the disk thread and waits until each new ring holds the playhead's next 0.5 s, in the new tempo map, for up to **500 ms**. The old plan plays meanwhile (RENDER-27 allows it) |
| D10 | Offline renders | No disk thread. The render's own thread fills the rings before each block, then the **same** fill code runs. Same samples, same sums, same bits (RENDER-1, RENDER-2) |
| D11 | Who owns what | The ring and the chunk maths live in `m2-runtime` (`stream.rs`). The disk thread lives with the player (`m2-play`, `disk.rs`): it already knows the playhead and the loop. A renderer handed to a player is marked live before it is sent, so nothing ever fills a ring on the sound thread |
| D12 | Import and waveforms | Import checks a file by decoding it in pieces, not whole. Its limit goes from 15 minutes to **3 hours**. Waveform peaks (`peaks.rs`) are made in pieces too |

### Recording audio (slices 2 and 3)

| # | Question | Decision |
|---|---|---|
| D13 | Which input? | Settings ▸ **Sound input**: "Same device as the output" (the default) or one input from a list. Kept in `~/.music2/settings.json` as `input: {device, name, monitor}` |
| D14 | When is it opened? | At the first Record on an audio track, and kept open until the song closes or Settings changes it. So macOS asks about the microphone only then. Playing, with no count-in, the take starts at the first sample the input delivers |
| D15 | Channels and format | The input's first two channels (a mono input makes a mono take). 32-bit float WAV at the song's `sample_rate`: the input is converted at the device boundary by the player's `Resampler`, as the output is |
| D16 | Where the samples go | The input callback copies each buffer into two preallocated lock-free rings (our own single-reader ring; no new crate): one to the engine (for hearing yourself), one to the take thread. It never allocates, locks or waits (RENDER-28) |
| D17 | Writing the take | The take thread drains its ring every 50 ms into a hidden part file in `assets/` (`.Vox take.wav.<pid>.part`, locked), as import does. At the end: the WAV header is finished, the bytes pushed (`m2_os::fs::push`), the file named create-only (`name_new`, `Rename::Excl`), the folder synced. **Only then** is the Tx planned (WRITE-1, FMT-14) |
| D18 | The file's name | `<track label> take.wav`, numbered when taken: `Vox take 2.wav` (`AssetName::for_import`, `untaken_folded`) |
| D19 | The Tx | One `Clips` Tx on the lane (`Gesture::Set`): the clip starts at the take's start beat, as long as the take, pinned by its hash. One undo step, named "recording on Vox". Undo takes the clip back; the file stays (music2 never deletes an asset) |
| D20 | Where the take lands | Its first sample is the one heard at the take's start. Each input buffer is stamped with the engine clock: the device's capture time against the output's playback time (cpal's stream timestamps), else the input's buffer length as its latency. The injected input is exact. The take starts and ends as a notes take does (count-in, R, Space, Stop, a jump, the loop's end: first pass only) |
| D21 | Hearing yourself | Settings ▸ **Hear yourself while recording**, off by default. Its words: "Use headphones: speakers feed back into the microphone." On: from Record until the take ends (count-in included), the input plays on the take's audio lane, through the track's effects and fader. It is a live source, like audition (MON-7): never in an export (RENDER-3, MON-4). New rule **MON-8** |
| D22 | Too slow a disk | The take ring holds 4 s. If it fills, the missing part is written as silence, so timing holds, and the take says "The disk was too slow: part of the take is silent." |
| D23 | No input | "No sound input found. Plug in a microphone or an audio interface." On a Mac without permission: "music2 can't use the microphone. Allow it in System Settings ▸ Privacy & Security ▸ Microphone." |
| D24 | Tests' input | `M2_AUDIO=none`: the silent player has a silent input on its own clock, listed as `M2_AUDIO_INPUTS` names (like `M2_AUDIO_OUTPUTS`). `Player::inject_input(samples)` queues samples it delivers. `M2_AUDIO_INPUT=inject` opens `POST /api/graph/input/inject` for browser suites, as `M2_MIDI=inject` does |
| D25 | macOS | The desktop bundle gets `NSMicrophoneUsageDescription`: "music2 records from your microphone when you press Record on an audio track." By-ear check on the Mac owed |
| D26 | Code rules | cpal stays in `m2-play` (it hides each OS, as `outputs.rs` uses it). `read_at` is the one new OS call, in `core/os/src/unix.rs` (PLAT-2) |

### Contract changes (with the slice that makes them)

| Rule | Change |
|---|---|
| RENDER-30 | Slice 1: rule text says "read with `read_at` by a disk thread; no map; WAV read in place; warped renders cached"; Why gains the SIGBUS reason; Today rewritten |
| RENDER-30a (new) | Slice 2: the input. Its callback never allocates, locks or waits; it feeds two rings; takes are stamped by the engine clock; a take is durable before a Tx names it |
| MON-8 (new) | Slice 2: hearing yourself is a live source on the take's lane; never in an export |
| Deferred table | Slice 1 marks disk streaming built; slice 3 marks "audio input with monitoring, recording takes as data Tx" built |

---

## Slice 1: disk streaming

**Goal.** Audio clips read their files through a streaming reader off the sound thread. Each lane reads ahead.
Seeks, loops and arrangement switches prefetch. Offline renders read the same samples, so every render keeps its bits.

**Files.**

| Crate | Files | What |
|---|---|---|
| `m2-os` | `core/os/src/fs.rs`, `unix.rs` | `read_at(&File, &mut [u8], u64)` |
| `m2-runtime` | `core/runtime/src/stream.rs` (new), `audio.rs`, `plan.rs`, `lib.rs` | Clip sources (WAV in place; warped render in the cache), the chunk maths, the slot ring, `AudioLane` holding a ring instead of renders, `same()` by descriptor, the offline fill before each block |
| `m2-play` | `core/play/src/disk.rs` (new), `lib.rs` | The `m2-disk` thread; lanes registered in `open`, `replace`, `activate`; the seek wait; loop prefetch; misses in `Status` |
| `m2-server` | `graph.rs`, `graph/actor.rs`, `peaks.rs` | The miss warning in the status; peaks in pieces |
| `m2-editor` | `import.rs` | The check in pieces; `LONGEST` 3 hours |

The chunk maths must match `render_clip` sample for sample: the cubic read at the same absolute positions (bounds
from the whole file), reverse by index, gain and fades by index, mono-down per clip, then the sum in clip order.

**Tests (each shown red once, then green).**

| Test | Where | Red by |
|---|---|---|
| `read_at_reads_where_asked` | `core/os` | `read_at` stubbed to read nothing |
| `a_streamed_lane_is_the_rendered_lane`: every clip of `examples/resample` (plain, pitched, offset, reversed, faded, warped), filled in odd run splits across chunk edges, equals the old whole render bit for bit | `core/runtime/src/tests/stream.rs` | the chunk fill stubbed to zeros |
| `a_long_file_never_lives_in_memory`: a 3-minute WAV made in a temp folder; the lane holds at most its slots, and no read is over one chunk's bytes | same | the old path (decodes the whole file) |
| `the_sound_thread_never_reads_a_file`: play, seek, a loop's wraps and a tempo activation over a streamed lane, under the allocation trap; no `read_at` on the callback's thread | `core/play/tests/no_alloc.rs` | the ring filled on demand inside `fill` |
| `a_seek_lands_on_audio`: a seek to 1:30 of a long clip; the first block after it equals the offline render there; no miss | `core/play/tests/stream.rs` | the seek sent without the wait |
| `a_loop_over_audio_never_misses`: a 4-bar loop over a long clip, 100 wraps, no miss | same | no loop prefetch |
| `a_tempo_change_finds_its_audio_ready`: no miss across the switch; `an_audio_swap_of_unchanged_clips_changes_no_bit` still passes | same | `activate` without the prefill wait |
| `a_slow_disk_misses_but_never_waits`: reads slowed by 200 ms (a test seam); every callback stays under its period; misses counted and shown | same | the sound thread waiting for its chunk |

**Done when:** all of the above are green; the 10 renders equal `e2e/renders.sha256`; the bit-identity tests
(RENDER-33, G7c) and `core/os/tests/one_place.rs` pass; clippy for Linux and `aarch64-apple-darwin` is clean;
RENDER-30 is updated; gaps #18 drops "play very long files from disk".

---

## Slice 2: the input and the take

**Goal.** The player reads a sound input beside its output. A take on an audio track is written to `assets/`, made
durable, then placed as a clip by one undoable Tx. Hearing yourself plays the input through the track, never into an
export. Everything is tested with `M2_AUDIO=none` and an injected input.

**Files.**

| Crate | Files | What |
|---|---|---|
| `m2-play` | `core/play/src/inputs.rs` (new), `ring.rs` (new), `lib.rs`, `outputs.rs` | Listing inputs; the cpal input stream; the silent input and `inject_input`; the two rings; capture stamps; conversion to the song's rate |
| `m2-runtime` | `engine.rs`, `audio.rs` | `Msg::Hear(generation, slot, ring)`: the engine adds the input to that lane's output while a take runs |
| `m2-editor` | `import.rs` (or `take.rs`, new) | A growing asset: the part file, append, finish (push, `name_new`, sync), reusing import's pieces |
| `m2-server` | `graph/record.rs`, `graph/take_audio.rs` (new), `settings.rs` | A take on an audio lane (same phases and ends as a notes take); the take thread's writer; the Tx "recording on Vox"; `input` in settings |

**Tests (each shown red once, then green).**

| Test | Where | Red by |
|---|---|---|
| `an_injected_input_records_where_it_was_heard`: a click train injected at known engine frames; the written file's samples, placed at the clip's start, line up to the sample | `crates/server/src/graph/record_tests.rs` | today's refusal of an audio lane |
| `a_take_is_durable_before_the_song_names_it`: the order seen by a test seam is push, name, folder sync, then the Tx | same | the Tx planned before the name |
| `a_count_in_keeps_nothing_before_the_start`: the clip starts on the start bar; samples from the count are not in the file | same | the take started at Record |
| `one_undo_takes_the_recording_back`: Undo says "Undo recording on Vox", the clip goes, the file stays | same | (new behaviour: red on old code) |
| `a_take_ends_at_the_loop_end`: first pass only, as notes | same | (new behaviour: red on old code) |
| `the_input_callback_never_allocates`: the input callback and the engine's hearing path under the allocation trap | `core/play/tests/no_alloc.rs` | a `Vec` made per buffer |
| `hearing_yourself_plays_through_the_track_and_never_in_an_export`: the input is heard after the track's gain; an export during the take has none of it | `core/play/tests/live.rs`, `crates/server/src/graph/tests.rs` | the input added after the mixer, and to exports |
| `a_full_take_ring_writes_silence_and_keeps_time` | `crates/server/src/graph/record_tests.rs` | frames dropped instead |

**Done when:** all of the above are green; renders unchanged; the existing notes-recording tests pass with the
same meaning; RENDER-30a and MON-8 are written.

---

## Slice 3: recording in the app

**Goal.** The Record button and R record audio on an audio track, as they record notes on a notes track. Settings
picks the input and turns "Hear yourself while recording" on or off. Plain words throughout.

**Files.**

| Part | Files | What |
|---|---|---|
| Server | `routes_settings.rs`, `routes_record.rs`, `routes_8b.rs` | `GET /api/audio/inputs`; `input` in `/api/settings`; `POST /api/graph/input/inject` (only with `M2_AUDIO_INPUT=inject`) |
| Web | `web/src/lib/record.ts`, `playTarget.ts`, `components/RecordButton.tsx`, `Take.tsx`, `SettingsDialog.tsx` | The target may be an audio lane; the take shows as a red clip growing on the lane, with an input level bar; Settings ▸ Sound input and the toggle; D23's words |
| Desktop | `apps/music2-desktop` bundle settings | D25's `NSMicrophoneUsageDescription` |
| Suite | `e2e/io-record.mjs` (new) | Below |
| Docs | `docs/gaps.md`, `docs/architecture.md`, `HANDOFF.md`, `docs/designs/step9/build-notes-io.md` | #25 to What works today; #31's input done; the deferred row; the Mac's by-ear steps |

**Tests (each shown red once, then green).**

| Test | Red by |
|---|---|
| `io-record.mjs`: + Track ▸ Audio track; R; the count's 4, 3, 2, 1; an injected tone; R; a clip "Audio take.wav" with its waveform; ⌘Z takes it back | old code: R on an audio track says "Select a track with notes" |
| `io-record.mjs`: Settings lists the injected input and keeps the toggle; a read-only window can't record (423) | no input in Settings |
| `routes_record_tests.rs`: start and stop on an audio lane through the routes; every window hears the take | today's refusal |

**Done when:** the suite and the route tests are green; `ci/check.sh` says `music2 check: PASS` on Linux, renders
10 of 10; pictures from Xvfb `:98` (`app-shot.sh`) of the take recording, the clip after, and Settings; the HANDOFF
says "macOS: unverified since <commit>" with the by-ear steps (a real microphone, the permission prompt, latency
against the click, hearing yourself in headphones).

---

## Risks

| Risk | What we do |
|---|---|
| A streamed sample differs by one bit from today's render | The reference test keeps the old whole render as its oracle; the 10 renders guard the rest |
| A torn read between the disk thread and the sound thread | Tags checked before and after the copy; atomic sample bits; a stress test rewrites slots while a reader reads |
| Memory per lane (1.5 MB) times many audio lanes | Measured on a 50-lane song with audio in slice 1; tuned later if needed |
| The seek wait (30 ms) or the prefill wait (500 ms) feels slow on a cold disk | Both are caps, not costs: a ready target waits 0 ms. Timings on Linux are information only (PLAT-1) |
| The server has no real microphone | Every test uses the injected input. The Mac's by-ear steps check a real one |
| macOS without the microphone key gives silence or kills the app | D25 adds it; the Mac's check owes it |
| Other tracks change `audio.rs`, `plan.rs`, `lib.rs` too | New code goes in new files (`stream.rs`, `disk.rs`, `inputs.rs`, `ring.rs`, `take_audio.rs`); merges, never rebases |

---

## Into gaps.md instead (one line each)

- A warped clip is rendered whole in memory once, before it streams; the stretching turn makes it stream.
- The render cache folder is never trimmed.
- A lane the disk can't keep up with cuts to silence with a click; it doesn't fade.
- A file another program changes while it plays may play a mix of old and new until the song rebuilds.
- Sound made during the count-in isn't kept in the take.
- No arm button: you hear yourself only while recording.
- One clip per take: no takes stacked per loop pass, no punch in and out.
- An input with its own clock (not the output's device) can drift a little against the song in a long take.
- Only the input's first two channels: no choosing input 3 and 4.
- No recording offset setting for a device that reports its latency wrongly.
- The take shows no waveform until it is written.
- A crash during a take loses it: the part file is swept.
