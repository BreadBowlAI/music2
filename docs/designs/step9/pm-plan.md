# Track pm: the poly op's options, MIDI controllers, MIDI files in the app

Step 9, the synth turns 4 and 5 (architecture.md, Order of work, item 9), and gaps.md #36. Branch `pm`, from `main`
at `97b4cdf`.

## In short

- Three slices, in this order. Each ends with a commit, and its tests are shown red before they pass.

  | # | Slice | What you get |
  |---|---|---|
  | 1 | The poly op's options | any instrument can play one note at a time, slide between notes, and choose which note stops when too many are held. std's one-note sounds use this one path. The Device card says how an instrument plays |
  | 2 | Pitch wheel and mod wheel | a MIDI keyboard's pitch wheel bends the notes of any instrument it plays; its mod wheel reaches instruments (std's synth: more vibrato) and plugins |
  | 3 | MIDI files in the app | File ▸ Import MIDI…, or drop a `.mid` file: each part becomes a new track with a fitting sound; a question when the file's tempo differs; one undo step |

- **One recorded render changes:** `examples/sounds` (2 of 12 lines). Its lead, sub and bass now hear each note's
  own velocity (0.85), not one rounded to MIDI's 127 steps (108/127). So std goes to 0.3.0 (FMT-3), and every
  shipped song asks for "0.3". With velocities a keyboard sends (k/127), the new path gives the old bits: a test
  holds that.
- **Nothing else changes sound.** The poly op's defaults leave the IR byte for byte as today (RENDER-16).
- **Not in the contract, so logged:** choosing which track each keyboard plays, and a MIDI channel per track.
- Linux timings are information only (PLAT-1). One `ci/check.sh` at the end of the track.

## Words used here

| Word | Meaning |
|---|---|
| poly | the op that runs a voice graph once for each note (RENDER-16) |
| voice graph | what an instrument plays for one note |
| pool | a poly's voices: kept or replaced whole at an edit |
| mono, legato | one note at a time. Legato: a note played while another is held carries on from it, not starting again |
| glide | the time a note's pitch takes to slide from the note before (portamento) |
| steal | which sounding note stops when more notes are held than there are voices |
| retrigger | a voice given a new note starts fresh (reset), or carries on with its envelopes starting again from where they are |
| bend | the pitch wheel (MIDI pitch bend) |

## Slice 1: the poly op's options

**Goal.** RENDER-16's whole op, `poly(notes, voice, { voices, mode, retrigger, steal, glide })`, on any voice graph,
in the arena's pools. std's one-note sounds move onto it; `std:mono` and `std:notecv` go. The Device card says how
each instrument plays.

**What the options do.**

| Option | Value (default first) | What it does |
|---|---|---|
| `mode` | `"poly"` | each note gets a voice (today) |
| | `"mono"` | one voice. Every new note starts again; with a glide, every note slides from the one before |
| | `"legato"` | one voice. A note started while another is held slides to it over `glide` and doesn't start again; any other note starts again at its own pitch (std's `mono()` today) |
| `retrigger` | `true` | a voice given a new note is reset (today, RENDER-10) |
| | `false` | it isn't. If its gate was open, the gate falls for one sample, so envelopes start again from where they are |
| `steal` | `"oldest"` | a free voice, else the oldest (today) |
| | `"quietest"` | a free voice, else the one with the lowest peak over its last block; a tie goes to the oldest |
| | `"none"` | a free voice, else the note isn't played; its Off finds nothing |
| `glide` | `0` (seconds; any Value) | read at the sample a slide starts, 0 to 10 s. A knob, so turning it is a cell write, never a new pool |

In mono and legato, when the sounding note ends while older ones are held, the newest of them sounds again, sliding
back (note memory, 8 deep, as std's). Notes that start on one sample count as one, the last. These are std's rules,
now kept by note id instead of by codes summed through two polys.

**Steps and files.**

| Step | What | Files |
|---|---|---|
| 1a | SDK: the options typed and checked. An unknown value, or `voices` with mono or legato, is a compile error in words. Only options that differ from the default are written, so today's IR stays byte for byte. `@m2/core` 1.4.0 | `core/script/sdk/core.ts`, the SDK version |
| 1b | IR: `Op::Poly` gets `alloc` (mode, retrigger, steal; left out when default) and `glide` (a ref to an outer node, resolved as a capture is). `FOp::Poly` carries both; the pool's shape hash includes `alloc` | `core/ir/src/lib.rs`, `check.rs` |
| 1c | Runtime: `PolyNode` allocates by `alloc`. Mono and legato: one voice fed per-sample pitch, gate and velocity from fixed buffers in the pool (no allocation); `NoteField` reads them when there are some. The mono voice never sleeps once started, as an instrument outside a poly. The allocator goes in a new file, so `lib.rs` grows little | `core/runtime/src/poly.rs` (new), `lib.rs` (`PolyNode`, `VoicePool`, `VoiceNote`, `Exec::NoteField`) |
| 1d | std: `play()` in `synth.ts` and `fm.ts` give `voices: 1` as `poly(…, { mode: "legato", retrigger: false, glide })`. `mono()` stays, as that one call. `std:mono`, `std:notecv` and their tests go; `std.wasm` rebuilt by `std/build.sh`. std 0.3.0; every shipped `music2.toml` moves; the two `examples-sounds` lines re-recorded, with why | `std/src/synth.ts`, `fm.ts`, `std/kernels-src/src/notes.rs`, `notes/tests.rs`, `std/m2pkg.toml`, `std/kernels/std.wasm`, `e2e/renders.sha256`, `core/session/tests/renders.json`, the manifests under `examples`, `templates`, `core/session/tests/fixtures`, `e2e/fixtures` |
| 1e | Device card: one line under the instrument's name, from the poly ops its call makes (a std instrument, or a `poly` written in a track). Read-only | `crates/editor/src/voices.rs` (new, as `sampler.rs`), `view.rs` (`voices` on a call node), `web/src/components/NodeGraph.tsx`, `web/src/lib/graph.ts` |
| 1f | Contract: RENDER-16's and RENDER-10's Today, the Deferred row marked built; README's sounds line | `docs/architecture.md`, `README.md` |

**The card's words.** The glide is shown as "70 ms" when it is a number or a knob, else "the glide set in code".

| Options | Words |
|---|---|
| poly, 8 voices, oldest | "Plays up to 8 notes at once. One more stops the oldest." |
| poly, quietest | "Plays up to 8 notes at once. One more stops the quietest." |
| poly, none | "Plays up to 8 notes at once. More are not played until one ends." |
| mono | "Plays one note at a time. Each note starts again, sliding from the last over 70 ms." (no glide: the first sentence and "Each note starts again.") |
| legato | "Plays one note at a time. A note played while another is held slides to it over 70 ms, without starting again." |

**Tests (red first: on a stub where the runtime and the SDK ignore the options).**

| Test | Where | Red on |
|---|---|---|
| `poly_options_reach_the_ir`, `default_options_leave_the_ir_as_today`, `a_bad_poly_option_is_said_in_words` | `core/script/tests/std.rs` (or the SDK's own tests) | no options in the SDK; the last also on an SDK that writes every option |
| `legato_slides_without_starting_again`, `touching_notes_start_again`, `a_held_note_sounds_again_when_the_top_one_ends`, `a_trill_over_a_held_note_glides_each_way`, `mono_starts_every_note_and_glides_from_the_last` | `core/runtime/src/tests/poly.rs` (new). The voice graph is a probe whose output is its pitch and gate fields, so these hold for any voice graph. They port std's `notes/tests.rs` | options ignored |
| `retrigger_false_drops_the_gate_one_sample`, `steal_quietest_takes_the_quietest`, `steal_none_drops_the_note` | same | options ignored |
| `glide_is_a_cell_not_a_new_pool`, `a_mono_pool_moves_whole_across_an_activation` | same | glide as an op field; the shape without `alloc` |
| `std_one_note_sounds_are_one_poly` (synth.lead's IR: a legato poly, no `std:mono`) | `core/script/tests/std.rs` | today's std |
| `mono_sounds_keep_their_bits_on_keyboard_velocities`: a small song (lead, sub, bass, FM keys with `voices: 1`; overlaps, touches, a trill, a return to a held note; velocities k/127) whose render hash is recorded under today's `mono()` first | `core/session/tests/fixtures/mono-k127/`, `core/session/src/gate_tests.rs` | legato stubbed as poly |
| `the_card_says_how_an_instrument_plays` | `crates/editor/src/voices_tests.rs` | no card |
| Device shows "Plays one note at a time" on the Lead | `e2e/pl-sounds.mjs`, one check | `dist-before` |

Kept green, unchanged: 10 of 12 renders; `core/play/tests/bit_identity.rs` (live is the export, every pure fixture);
`no_alloc*.rs` (the sounds fixture now plays legato pools); `fixture_renders_change_only_with_stds_version`.

**Done when** every test above passes, 10 renders keep their bits, the sounds lines are re-recorded under std 0.3.0
with the reason in the commit, the mac lint passes, and RENDER-16's Today says "built".

## Slice 2: pitch wheel and mod wheel

**Goal.** RENDER-14's `Midi` event, made from MIDI input (its comment: "CC, pitch bend, sustain from MIDI input"):
a keyboard's pitch wheel and mod wheel reach the lane it plays, live. Every voice graph hears the bend in its notes'
pitch. A new note field, `modWheel` (0 to 1), carries the wheel. Plugins get both as CLAP MIDI events. The sustain
pedal stays where it is: the server holds Offs back (MON-7).

**Steps and files.**

| Step | What | Files |
|---|---|---|
| 2a | Server: `read()` takes pitch bend (0xE0), CC 1 (the wheel) and CC 121 (reset controllers), and sends each to the target lane as a live event. When a keyboard lets go (unplugged, All Notes Off, ⌘., the song closed, another target), its lane gets the bend centred and the wheel at 0 | `crates/server/src/graph/midi.rs`, `audition.rs` |
| 2b | Runtime: `LiveEv::Midi { source, bytes }`. The scheduler keeps the latest bend and wheel per lane (fixed slots) and merges them at the block's first frame, before the notes (RENDER-14, MON-7). Refused while an export holds, as an On is | `core/runtime/src/sched.rs`, `event.rs`, `lib.rs` (`Renderer::live`), `core/play` (`Player::live`) |
| 2c | Poly: reads `Midi` in its lane. Pitch = the note's pitch (+ glide) + bend × 2 semitones; `NoteField::ModWheel`. The lane's bend and wheel live in its pool and reset with it. With no bend, pitch is filled exactly as today | `core/runtime/src/poly.rs`, `lib.rs`; `core/ir/src/lib.rs` (`NoteField::ModWheel`); `core/script/sdk/core.ts` (`Note.modWheel`) |
| 2d | Plugins: `m2_plugins::Event::Midi { time, bytes }`, sent as `CLAP_EVENT_MIDI` | `core/plugins/src/lib.rs`, `host.rs`; `core/runtime/src/lib.rs` (`plugin_events`) |
| 2e | std: the polysynth's vibrato deepens with the wheel, by up to half a semitone. `std:synth` gets a `wheel` input; at 0 no bit changes | `std/kernels-src/src/synth.rs`, `std/m2pkg.toml`, `std/src/synth.ts` |
| 2f | Contract: RENDER-14's and MON-7's Today; the Deferred row "controller … events beyond note on and off": live controllers built, recording them left | `docs/architecture.md` |

**Tests (red first).**

| Test | Where | Red on |
|---|---|---|
| `a_bend_reaches_every_voice_in_its_lane`, `the_mod_wheel_is_a_note_field`, `bend_and_wheel_reset_with_the_pool` | `core/runtime/src/tests/poly.rs` | `Midi` ignored |
| `a_live_bend_merges_before_the_notes`, `a_bend_is_refused_while_an_export_holds` | `core/runtime/src/tests/audition.rs` | no `LiveEv::Midi` |
| `a_plugin_hears_the_bend_as_midi` | `core/runtime/src/tests.rs` (the fake host) | no `Event::Midi` |
| `a_keyboards_bend_reaches_its_lane`, `a_keyboard_let_go_centres_its_bend` (`M2_MIDI=inject`, waiting on the graph status, never on a sleep) | `crates/server/src/graph/midi_tests.rs` | `read()` drops 0xE0 |
| `the_wheel_deepens_the_synths_vibrato` | `std/kernels-src/src/synth/tests.rs` | no `wheel` input |

Kept green: all 12 renders (no render has a controller), bit identity, the no-allocation tests.

**Done when** these pass, the 12 renders keep their bits, and the contract's Today lines say what is built.

## Slice 3: MIDI files in the app

**Goal.** The app half of studio's C2 (`docs/designs/step8/studio-plan.md`, Q32 to Q36, D10, D11, D34), left as C2b:
File ▸ Import MIDI… and a dropped `.mid` file make a new notes track for each part, with a fitting std sound and one
clip, after the tempo question when needed. One undo step: "Undo importing groove.mid".

| Where | What happens |
|---|---|
| File ▸ Import MIDI… (no key) | a file window for `.mid` and `.midi`; new tracks at the end, from bar 1 |
| A drop below the tracks | new tracks at the end, from that bar |
| A drop on a track's row | new tracks right after it, from that bar (as audio files do) |
| A one-part file dropped on a notes lane | into that lane at that bar; no question (built: `import_into`, D11) |
| The song has no clips | it takes the file's tempo and time signatures; no question |
| The tempos differ | "groove.mid is at 128 BPM. The song is at 120 BPM." "Keep 120: the notes play at your song's speed." Buttons: Keep 120 (Return), Use 128, Cancel |
| Use, at a later bar | the file's tempo map from that bar on; the song's before it; the song's time signatures kept |
| Refused, nothing written | not a MIDI file, no notes, over 32 parts, a note past bar 9,999, the code doesn't compile: each in words |

A toast: "Imported groove.mid: Drums, Bass and Keys", with Undo.

**Steps and files.**

| Step | What | Files |
|---|---|---|
| 3a | `midifile::import_tracks(s, file, name, place, choice)`: `bring()`, then `tempo_plan()` (shifted for a later bar), each part a `NewTrack` (`Play::Instrument`, one clip by `clip_length`), then one `tracks::add_tracks(…, after, arrangement)`. Answers `Asks { file_bpm, song_bpm }` when the question is due and unanswered | `crates/editor/src/midifile.rs`, `midifile_tests.rs` |
| 3b | `MidiTarget` gains `Below { beat }` and `Track { module, beat }`, and the request a `tempo` answer. The route answers `{asks}` (nothing written) or the edit with its `tracks` and the step's name | `crates/server/src/graph/exchange.rs`, `routes_exchange.rs`, `routes_exchange_tests.rs` |
| 3c | Desktop: `import_midi` ("Import MIDI…", no key) after Import Audio… | `apps/music2-desktop/src/menu.rs` |
| 3d | Web: `importMidi()` (the app's `open_file`, a browser's file input); a drop taker for `.mid` and `.midi` with hints ("groove.mid: 3 tracks · bar 1", "Into Bass · bar 3"); `MidiTempoAsk` | `web/src/lib/midiFiles.ts`, `drop.ts`, `web/src/components/MidiTempoAsk.tsx` (new), `App.tsx` |
| 3e | Docs: #36 keeps only MP3 or AAC export and plugin stems; the loose end "Dropping a MIDI file" goes; What works today gains a line | `docs/gaps.md` |

**Tests (red first: `import_tracks` stubbed to refuse; the suite on `dist-before`).**

| Test | Where |
|---|---|
| `each_track_and_channel_becomes_a_track`, `an_empty_song_takes_the_files_tempo`, `a_song_with_clips_asks_then_keeps_or_takes_the_tempo`, `use_at_a_later_bar_keeps_the_songs_tempo_before_it`, `one_undo_step_takes_the_import_back`, `an_exported_song_imports_as_the_same_notes`, `a_refused_file_writes_nothing` | `crates/editor/src/midifile_tests.rs` |
| `an_import_below_asks_then_writes` | `crates/server/src/routes_exchange_tests.rs` |
| `import_midi_follows_import_audio` | `apps/music2-desktop/src/menu.rs` |
| Import MIDI… through the chooser makes Drums (the drum kit) and Bass; the empty song takes the file's 128; one Undo takes all back; a song with clips asks, Return keeps 120, Use writes 128; a 3-part drop below; a one-part drop on Bass at bar 3; a text file named `.mid` says its words. Run with `M2_MIDI=inject` | `e2e/st-midi.mjs` |

A picture on Xvfb :95 (`app-shot.sh`): the question, and the new tracks.

**Done when** these pass, Undo takes a whole import back in one step, no render changes, and gaps.md says what is left.

## Decisions

| # | Decision | Why |
|---|---|---|
| D1 | The options are RENDER-16's shape, word for word: `mode` is `"poly"`, `"mono"` or `"legato"`, with `retrigger`, `steal` and `glide` | the contract fixed the shape; legato is a mode, not a fifth option |
| D2 | `glide` is a Value, read where a slide starts. It does nothing in `"poly"`. `On`'s `from_pitch` stays `None`: the allocator glides itself | a knob must stay a cell write; per-note slides written in data stay deferred |
| D3 | Mono and legato have one voice; `voices` with them is an error | one rule, said in words |
| D4 | A mono or legato voice never sleeps once started | as an instrument outside a poly, which is what std's sounds were: same bits, same cost |
| D5 | Note memory 8 deep; quietest is by the last block's peak | std's depth; the peak is already kept per voice |
| D6 | One std bump (0.3.0) and one SDK bump (1.4.0) for the whole track | slices 1 and 2 both change std and the SDK |
| D7 | `std:mono` and `std:notecv` are deleted; `mono()` stays as one call | one path; songs that call `mono()` keep working |
| D8 | Bend: ±2 semitones (General MIDI's), for the whole lane (every channel), a step at each block line. The wheel 0 to 1. Sustain stays in the server | the smallest thing that is right; the rest is logged |
| D9 | The card is read-only | editing settings on a card is #35 |
| D10 | MIDI files follow studio's Q32 to Q36, D10, D11 and D34 as written; plus Use at a later bar, and a drop on a track's row placing the new tracks after it | decided before; these two were open |

## Risks

| Risk | What we do |
|---|---|
| std's sounds differ by more than velocity rounding (chords into a one-note sound, more than 8 held) | the k/127 song catches it; any other difference is fixed, not re-recorded |
| std 0.3 stops songs that ask for "0.2" at `pkg.unsatisfied` | its hint says the line to write; HANDOFF says so first. FMT-3's lockfile trigger is "after people have songs": the owner decides if that is now |
| Other step 9 tracks (the plugin host process, windows, latency) edit `core/runtime/src/lib.rs` and `m2_plugins::Event` at the same time | the allocator lives in `poly.rs`; slice 2's plugin part is one variant and one mapping; merges only. The host process must carry `Event::Midi` over its ring: said in HANDOFF |
| Another track bumps std or `@m2/core` | at the merge, take the higher version once, and check the renders again |
| A `Midi` event reaches code that expects only On and Off | every `match` on `Event` in runtime, plugins and server is read; a test per kind of consumer |
| MIDI tests that depend on timing | they wait on the graph status, never on a sleep |

## To gaps.md, not built here

One line each:
- Choosing which track each MIDI keyboard plays (today every keyboard plays the selected track).
- A MIDI channel per track (today every channel plays the same lane).
- Recording the pitch wheel and the mod wheel into clips: needs note expression data, a format bump (#24 keeps it).
- Aftertouch, expression (CC 11), breath and other controllers; a bend range other than ±2 semitones.
- The bend and the wheel step once a block (2.7 ms at 48 kHz), with no smoothing.
- The mod wheel on FM, the Sampler and the drums (only the polysynth's vibrato hears it).
- Editing an instrument's voices, mode and glide on its card (with #35).
- Glide in `"poly"` mode (each new note sliding from the last, on many voices).
- MIDI files: controllers, bends and program changes after the first are skipped.
