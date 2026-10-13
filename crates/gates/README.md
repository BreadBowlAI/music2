# m2-gates

Measures step 3's acceptance gates ([architecture contract](../../docs/architecture.md#acceptance-gates)), as step 3a's design lays them out (its §15): seeks, a 10-minute loop, note edits, stuck notes, added effects, crossfades, tails after stop and range renders. Each gate plays a song, does what the gate names, and judges what it heard. It prints every number, so a recorded run is also the record.

The gates for 3b (previews never wait on a compile; an outside save during a drag doesn't interrupt it), RENDER-33's bit-identity test, the allocation trap and the offline byte-identity check aren't here: they belong to the engine's own tests.

## Run it

```bash
# Step 3a's live engine, at CI sizes: a 3-track reference project, 1 minute of loop, 8 seeks.
cargo run -p m2-gates --release --features engine -- --quick

# The recorded run on p16: 10 minutes of loop, 50 seeks. Run it twice, as the design asks:
cargo run -p m2-gates --release --features engine           # the release build
cargo run -p m2-gates --features engine                     # the dev build `cargo run -p music2-desktop` makes

# The same gates on HEAD's engine, which replays: the negative reference.
cargo run -p m2-gates --release -- --rig replay --quick

# Seek, note edit and add effect through the app's server, in real time on the silent player.
cargo run -p m2-gates --release -- --server

# One or more gates by name; p50 or the audio-heavy p16 instead of p16.
cargo run -p m2-gates --release --features engine -- seek loop --reference 50
cargo run -p m2-gates --release --features engine -- add-effect --stems 4
```

`--help` lists every option. The first line of every run says which build measured: timings from a dev and a release build differ several times over, and the gates must pass on both.

Projects are generated into a new folder under the temp dir and removed afterwards (`--work DIR` and `--keep` to look at them). Nothing reads `~/.music2` or `~/Music`, and nothing plays on a device: the binary sets `M2_AUDIO=none` before anything starts, and `M2_AUDIO_FRAMES=512` unless it is set.

The exit code is 0 when no gate failed, 1 when one did, 2 when a gate couldn't run.

### Features

- `engine`: step 3a's live engine (`m2_play::Player::driven`, `activate`, `swap_lane`, `Renderer::starting_at`).
- `range`: the session's range render (`Render::range`, and `Rendered::exact` and `reproducible`), for what G10 checks that a range render says about itself. It turns on `engine`.

Both are on by default now that step 3a has landed, so `cargo test --workspace` also runs `the_live_engine_passes_every_gate`. `--no-default-features` builds the harness without them.

## The reference projects

p16 and p50 are generated, never checked in, by `m2_session::reference` (`core/session/src/reference.rs`): tracks cycling std `bass` with `delay`, `pluck`, and `pad` with `reverb`, each playing `examples/first-song`'s notes repeated to 512 beats (4:02 at 127 bpm). `--stems N` adds N one-minute stereo WAVs on audio lanes, every second one warped. To write one by hand:

```bash
cargo run -p m2-server --example reference -- /tmp/p16 --tracks 16 [--stems 4]
```

## Rigs

A gate drives a rig: an engine filled a device buffer (512 frames) at a time, in simulated time, so a run never waits for a clock.

- **engine** (`src/engine.rs`) is step 3a's live engine through `Player::driven`, fed what the server feeds it: a renderer built without seeking and activated, or for a data edit that leaves the graph as it was, `SwapLane` for each notes or control lane that changed. Its kernel-reset count is the engine's (`Engine::resets`), which counts every reset of a renderer's graph where the graph resets, so a jump that reset kernels would move it.
- **replay** (`src/replay.rs`) is HEAD's engine as the app drives it at `0ff97b4`: every seek and edit builds a new renderer and replays the song to the playhead, the player swaps it in at its next buffer, and stop outputs zeros without processing. It loops by replaying at each wrap, at the frame the wrap's beat gives, so it never drifts. It must fail the gates step 3 exists for; a gate that passes on it measures nothing. `tests/harness.rs` checks that it does.

Work done off the audio thread (building a renderer, preparing lane data, replaying) is timed on the wall clock. While it runs the device plays on for as many frames, so an engine that builds at the position it read first jumps back by that much (RENDER-8).

**Latency** is counted as the design counts it: the wall time spent preparing after the compile (which every budget leaves out), then the frames until the engine applied the change, then one 512-frame device buffer (10.7 ms at 48 kHz), which is counted, not subtracted.

## The gates

| Gate | Plays | Passes when |
|---|---|---|
| **G1 seek** (RENDER-8, 9, 10, 11) | the reference project from 1:00, then 50 seeks to random beats | every seek is heard within 50 ms (p50, p95 and max printed); the transport lands on the target; no kernel reset and no new renderer. From the engine's trace, after each seek: every note-on the lane files start from the target until the next seek is heard at its frame (a seek that goes silent misses them), and no other (a chased note is one no note starts there); the first of them sounds over -60 dBFS |
| **G2 loop** (RENDER-9, 9a, 10) | beats 128–144 (4 bars at 127 bpm) looped from 1:00 for 10 minutes, about 79 wraps | **tails**: one note mid-loop into a long reverb, then a long delay; over the 50 ms before every wrap the tail is louder than -60 dBFS (else the check is void), and over the 50 ms after it keeps at least half its level; no kernel reset. **Negative control**: the same with a panic before each wrap must be seen as a cut. **Drift**: a click on every beat, found to the sample, lands where the grid puts it in every lap, each lap exactly one loop length; each lap's note-ons equal the first lap's. **The reference project** loops the same way: every lap plays the first lap's note-ons, each at the engine frame its lap and transport frame give |
| **G3 note edit** (RENDER-26, PLANE-1) | the reference project from 1:00; a note written half a beat ahead into `t01`'s lane file | heard within compile + 20 ms; it took `SwapLane`, not a new renderer; no reset; up to the new note, the output is the same bits as a twin engine that wasn't edited (no tail cut); the note is heard where it was written |
| **G4 stuck notes** (RENDER-13, 26a, 9, 6a) | std's `pad` holding a note over beats 4–36, acted on at beat 8 | after each of delete, shorten behind and ahead of the playhead, lengthen, stop, a loop wrap through it, a seek and a moved clip, the pad is under -80 dBFS once its release (measured offline to -80 dBFS) plus 100 ms has passed, it sounds where it still should, and the schedulers hold no note after stop. The same with duplicate notes, layered clips with one id and anonymous clips at one start: delete one, shorten one, and write the same data again, which must leave the output bit-identical |
| **G5 add effect** (RENDER-25, 25a, 8) | the reference project from 1:00; `Session::add_effect` puts a delay after `t02`'s `pluck` | heard within compile + 100 ms; no jump back: the transport moves on by exactly what was played, the frames played while the renderer was built included; nothing changes before the activation; the effect is heard: a twin activates the song as it was at the same frame, and the second after differs from it by more than -60 dBFS (an activation that kept the old renderer, or built the old song, differs by nothing); with `--stems`, no asset is decoded and no clip warped again |
| **G6 crossfade step** (RENDER-25a, 27) | a 440 Hz sine, a sine voice, and a sine voice at 127 then 90 bpm, each swapped with and without a delay (or the other tempo, slower and faster in turn) 50 times at random clocks. The voices play a note on every beat and each activation lands inside one: a new renderer doesn't chase the note the old one ends, so only then does an activation switch sound | sound before every activation (else the case is void); no sample-to-sample step is larger than twice the pure sine's own largest (about 0.0576); across each activation the beat goes on: the transport is where the old renderer's beat puts it in the new one's tempo |
| **G7 tail after stop** (RENDER-6a, 10) | a note into a reverb, a delay with feedback 0.6, and a 500 ms delay inside the voice (silent gaps between echoes); stop mid-tail, then play on 3 s or until the tail ends | the 50 ms after stop keep at least half the 50 ms before; the level only falls (per 100 ms, or per echo period for the delays); it ends under -80 dBFS; no block repeats (frozen); the transport holds and isn't playing; every 100 ms window above -70 dBFS is within 1 dB of the same tail offline, which an engine that idled between echoes, froze or reset would miss |
| **G10 range render** (RENDER-4, 5) | the reference project | from bar 64 with 2 bars of preroll takes no longer than the same rendered length from bar 1 (within 10%; the literal reading, 8 bars from bar 1, is printed too); two renders are the same bits; from bar 1 it is the offline render; with `range`, from bar 64 it says approximate, from bar 1 exact, and a pure song reproducible. Today's `--from` replay at bar 64 is printed for the record |
| **with-plugins** (`plugins`; RENDER-6a, 8, 9, 9a, 10, 11, 25, 26, 26a, HOST-6) | `examples/with-plugins`: Surge XT into Airwindows beside a std kick, built with the plugins on the CLAP search path (`CLAP_PATH`, else `~/Library/Audio/Plug-Ins/CLAP`); skipped, saying why, when they aren't installed | G1: 20 seeks heard within 50 ms, on target, nothing reset or rebuilt; every note from the target on heard at its frame and nothing chased (the trace against the lane files), the first sounding. G2: the 4 bars looped for the loop gate's minutes, every lap the first lap's note-ons on the grid, nothing reset or rebuilt; looping beats 2–10 (each wrap ends a chord and lands where none starts), the 50 ms after each wrap keeps at least half the ratio the chord's release has when it ends as written. G3: a lead note half a beat ahead heard within compile + 20 ms by `SwapLane`, nothing different before it, as loud as the lead after it. G4: deleting or shortening the sounding chord, stop, seek, wrap, a moved clip and a panic leave Surge holding no note, and the lead under -60 dBFS once its release (measured offline) has run. G5: an added effect heard within compile + 100 ms with no jump back, with what building the plugins takes printed, and heard: on a copy whose lead doesn't play (Surge's random phases make one instance differ from the next by tens of dB while it plays), the second after differs from a twin that activates the song as it was at the same frame by more than -60 dBFS, and by 20 dB more than two such twins differ. G7: after stop the plugins ring on, only decay (per 500 ms, within 1 dB: their reverb wobbles from one 100 ms to the next, differently in each instance) and fall at least 10 dB in 3 s (about 7 dB a second; a frozen or held tail doesn't), never repeat a block, the transport holds and Surge holds no note. Plugins aren't bit-exact from one instance to the next, so these judge notes, levels and the engine's counters where the std gates compare bits |

Through the server (`--server`): G1 seeks a third of the time while a code save compiles holding the project (RENDER-29); G3 and G5 make a tenth as many `write_notes` and `add_effect` edits as there are seeks (1 to 10), and sample the playhead every 2 ms for a jump back. They time; they can't listen: the silent player keeps no output and the server no note trace, so what is heard after a seek or an edit is judged on the engine rig, which plays the same engine through the same player callback (each server gate says so in a `skip` line). Each is timed in real time as step 3a's design counts it (§15): to the start of the audio callback that took its message (the engine's `Applied` stamp, through `Graph::applied`), plus the frame its block starts at in that callback, plus one device buffer. A seek counts from the call; an edit from when the server's session finished its work, its compile included (`Graph::last_edit`), which the budgets leave out and the run prints.

Not measured: the property test over random blocks (the runtime's own). A CLAP reset doesn't cut Surge XT's or Airwindows' tails, so on with-plugins a panic at each wrap is printed rather than used as the tail detector's negative control, which runs on the std probes (G2).

## The engine calls the engine rig makes

`src/engine.rs` holds every call into the engine, so following its interface as it changes happens there:

- `m2_play::Player::driven(rate) -> (Player, Driver)`; `Player::{open, activate, swap_lane, transport, seek, set_loop, panic, status, applied, crashed}`; `Driver::{fill, drain_retired, engine}`
- `m2_runtime::Renderer::{build_with, starting_at, lane_slots, caps, generation, trace, trace_entries, held_notes}`; `Engine::{resets, activations, current}`; `audio::Cache::{prune, work}`; `notes_data`, `Caps::fits`, `LaneSwap`, `TraceKind`
