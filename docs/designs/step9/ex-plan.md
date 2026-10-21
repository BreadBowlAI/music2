# Track ex: one path for faults

Branch `ex`, from `main` at `83fa4e4`, 2026-10-18. Step 9.

## In short

- **Today, a part that breaks while sound is made is told in six different ways, or not at all.** A kernel that traps is told. A plugin that crashes is told in other words, with no code. A file that can't be read, a Sampler file that won't decode and a stretch that fails say nothing, or only write a log line. An export can say "Exported" with a part silent.
- **After this track, there is one path.** Every part that finds a fault puts a small fixed record in one **fault ring**. It never allocates, locks or waits, so the audio thread can use it too. The session reads the ring and turns each record into a problem (a `Diagnostic`, DIAG-1) that names the sound, the track or the file.
- **Live:** the part goes silent alone. The song plays on. The problem shows once as a message, and stays in the status popover until the part plays again.
- **Offline (an export, `m2 render`, `m2 bounce`):** any fault that changed the sound stops the render, and the error names it. Never "Exported" with a part silent. This is RENDER-1's one rule, made wider. No new rule.
- No sound changes: `e2e/renders.sha256` keeps all 12 lines.

| Slice | You get | Size |
|---|---|---|
| 1. The fault ring | one path; sounds and plugins that stop are told through it, as problems with codes | medium |
| 2. Every fault, and offline fails | unreadable files, files that won't decode and failed stretches are told; an export with any fault fails and names it | medium |
| 3. Inputs | an input chosen but not there, and a Mac microphone refused mid-take, are told; contract and gaps closed | small |

## What is there today

| Fault | Who finds it | Live today | Offline today |
|---|---|---|---|
| a kernel or voice traps, or runs out of memory | audio thread, `run_part` (`core/runtime/src/lib.rs`) | marked in `Stopped` (`core/runtime/src/stopped.rs`); `crates/server/src/graph/stopped.rs` makes a toast string, no code | the render fails, naming the node (8a) |
| a plugin reports an error | audio thread, `PluginNode::process` | `inst.kill()`; the actor's `heard_plugins` makes a toast string, no code | **killed and silent** when not `strict`, and exports are not strict |
| the plugin host crashes or hangs | the host's reader, `HostEvent::Crashed`/`Hung` | `crash_said`: one toast string ending "(plugin.crashed)" | **silent from there, "Exported"** (gaps, ph review) |
| a streamed file can't be read | disk thread / offline filler, `Lane::fill_chunk` (`core/runtime/src/stream.rs`) | `Ring::errors` counts it; nothing reads the count | **silent there, "Exported"** (gaps, io r8) |
| a file in `assets/` won't decode | the build, `Cache::wav`, `Cache::kernel_asset` (`audio.rs`) | silent, nothing said (gaps, ab r4) | **silent, "Exported"** |
| a stretch fails | stretch thread, `renders.rs` | `eprintln!` only (gaps, sx) | the render fails, naming the clip |
| the chosen input isn't there | `open_input`, `open_device` (`core/play/src/inputs.rs`) | records from another input, nothing said (gaps, io r7) | — |
| macOS refuses the microphone mid-take | nobody | the take records silence (gaps) | — |

## Slice 1: the fault ring

**Goal.** One fault path, proven with the two producers that are told today. Their old ways go.

| Crate | Files | Change |
|---|---|---|
| `m2-runtime` | `core/runtime/src/fault.rs` (new) | `Fault`: `Copy`, 24 bytes: a code (a small enum), the plan's id, and a subject (node, lane and clip, asset slot, render, plugin handle, input) as numbers. `Faults`: a bounded ring of 256, many writers, one reader ([D1](#decisions)). `report` never allocates, locks or waits. A full ring counts what it lost |
| `m2-runtime` | `lib.rs`, `plan.rs`, `stopped.rs` (goes) | the plan holds the ring (`Arc<Faults>`), given when it is prepared. `run_part` live reports `sound.stopped`; a plugin's error live reports `plugin.failed`. `Stopped` and its `news`, `seen`, `stops` go: the node's own `exec.stopped` keeps it silent |
| `m2-editor` | `crates/editor/src/faults.rs` (new) | `Namer`: made from a compiled revision when its plan is prepared. It names a subject in plain words, with the track's key: "The delay on Bass", "loop.wav on Drums". `sound_named` moves here from the server |
| `m2-server` | `graph.rs`, `graph/faults.rs` (new, replaces `stopped.rs`), `actor.rs`, `lib.rs` | one live ring per app, in `Inner`, given to each plan, the live `Cache` and the `Player`. The actor turns `HostEvent::Crashed`/`Hung` into `plugin.crashed` faults ([D9](#decisions)). The 300 ms notices thread drains the ring, names each fault with the namer of its plan ([D2](#decisions)), and adds it to the session's live problems. `news()` and `state()` list them after the revision's own. A new one is toasted once and sends `graph_changed`. `crash_said` and `heard_plugins`' notice strings go; its revive logic stays |
| docs | `architecture.md` (DIAG table, DIAG-4, RENDER-25) | the codes `sound.stopped` and `plugin.failed`; `plugin.crashed` said through the path |

**Tests, each shown red first.**

| Test | Where | Red on |
|---|---|---|
| `a_fault_report_never_allocates_locks_or_waits`: 8 threads report 10,000 faults under the counting allocator; none allocates | `core/runtime/src/tests.rs` (its `GlobalAlloc`) | `report` stubbed to push to a `Vec` |
| `a_full_ring_counts_what_it_lost` | `core/runtime/src/fault.rs` tests | stubs |
| `a_stopped_sound_is_a_problem_naming_it_and_its_track`: a trapping kernel live; `problems` holds `sound.stopped`, `module` the track's key; one toast | `crates/server/src/graph/faults_tests.rs` (from `stopped_tests.rs`) | today's code (a toast, no problem) |
| `a_plugin_that_fails_live_is_one_problem` (probe tag `fails-process`) | `faults_tests.rs` | today's code |
| `a_host_crash_is_one_plugin_crashed_problem` (probe tag `crash-at-<n>`, in the host process) | `faults_tests.rs`, from the crash test it replaces | today's code (a string) |
| `a_fault_from_a_plan_gone_is_never_named_by_the_new_one` | `faults_tests.rs` | the namer taken from the plan that plays now |
| `a_problem_goes_when_its_part_plays_again`: Stop All Sound clears it | `faults_tests.rs` | before the clear |

**Done when:** all green. The kept stop and crash tests pass through the new path. Renders 12 of 12. No `notices` push is left for these faults.

**As built (slice 1).** Where the build went another way than the table above:

| What | Why |
|---|---|
| The live `audio::Cache` holds the ring; each plan takes it from the cache it is built with. `prepare` keeps its arguments | the cache is already given to every live plan; an offline render's cache makes its own ring |
| The `Player` doesn't get the ring yet | nothing in it reports until slice 3 (inputs) |
| `PluginHost::gone(h)` is new in `m2-plugins` (an atomic load) | a crashed host makes `process` fail too; the audio thread asks, so a crash is told once, as `plugin.crashed`, never also as `plugin.failed` |
| The export's give-back reports `plugin.failed` for a plugin that won't come back | the old notice told it; nothing on the audio thread sees it fail |
| A toast is once per code and per part's words ("The delay on Bass") in the open song | a node's name is its number in its file (`bass/n5`), which an edit changes |
| A fault of the plan before the newest is told, not listed | its part plays again in the new plan (a stopped sound is reset as it moves; a plugin is made again) |
| A plugin left silent is reported again in each new plan | it plays on silent there, so it stays listed |
| `Code` has `OutOfMemory` and `PluginHung` beside the plan's codes | their words differ; they are `sound.stopped` and `plugin.crashed` in the table |
| The plugin tests are in `crates/server/src/graph/tests.rs` | their rig (`Ex`, `two_probes`) lives there |
| `e2e/sound-stops.mjs` now checks the problem is listed, and listed again after ⌘. with no second toast (13 checks) | D3 |
| No `Once` flag in `m2-runtime` (it went in the ex review, r4) | a node reports once through its own `stopped` flag, a lane through its `Tell`; nothing used it |

## Slice 2: every fault, and offline renders fail

**Goal.** The producers that say nothing today report into the ring. Each offline render has a ring of its own. It reads it before its first block and after each block. The first fault stops the render, with the namer's words and the code.

| Crate | Files | Change |
|---|---|---|
| `m2-runtime` | `stream.rs` | `fill_chunk` reports `asset.unreadable` once per lane per plan. `Ring::errors` and its reader `errors()` go |
| `m2-runtime` | `audio.rs` | `Cache` holds the ring. `wav` and `kernel_asset` report `asset.unreadable` for a file that is there but won't decode ([D6](#decisions)) |
| `m2-runtime` | `renders.rs` | a stretch that fails reports `stretch.failed`, with the clip's lane and number. The `eprintln!` goes |
| `m2-runtime` | `lib.rs` | a plugin's error, or a dead instance, fails the node when not live (`!b.live`), not when not strict ([D5](#decisions)) |
| `m2-os` | `core/os/src/fs.rs` | the test seam `watch_reads` may fail a read (`fn(usize) -> io::Result<()>`), as an unplugged drive does |
| `m2-editor` | `render.rs` | `Render` makes its ring, gives it to its `Renderer` and `Cache`, reads it each block. A fault, or a node's error, becomes one error type, `Faulted { code, words }` |
| `m2-server` | `export.rs`, `graph/actor.rs` | the export's `error` event carries `code`. The problems an export carries widen from `plugin.missing` to every revision error that silences a part, `asset.missing` too ([D7](#decisions)) |
| `m2` | `apps/m2/src/main.rs` | `m2 render` and `m2 bounce` exit non-zero, printing the words and the code |
| docs | `architecture.md` (RENDER-1, RENDER-25, RENDER-30, DIAG table) | the one rule below ([D4](#decisions)); codes `asset.unreadable`, `stretch.failed` |

**Tests, each shown red first.** One per listed case, offline and live.

| Case | Test | Where | Red on |
|---|---|---|---|
| ph | `an_export_during_a_host_crash_fails_naming_the_plugin`: probe `crash-at-<n>:<file>` in the host process | `crates/server/src/graph/tests.rs` | today's code (it says done) |
| ph | `a_render_whose_plugin_fails_fails` (probe `fails-process`) | `crates/editor` render tests | today's code (silent) |
| io r8 | `an_export_of_a_streamed_file_cut_short_fails_naming_it`: the file is cut short after the build | `core/runtime/src/tests/stream.rs` with `Render` | today's code (silent) |
| io r8 | `a_streamed_file_that_cant_be_read_is_told_once`: the seam fails reads | `faults_tests.rs` | today's code (a count) |
| ab r4 | `a_sampler_file_that_wont_decode_fails_the_export_naming_it` (a damaged WAV in `assets/`) | `crates/editor` render tests | today's code |
| ab r4 | `a_sampler_file_that_wont_decode_is_told_naming_it_and_its_track` | `faults_tests.rs` | today's code |
| sx | `a_stretch_that_fails_live_is_told_naming_the_clip` | `faults_tests.rs` | today's code (a log line) |
| sx | `a_stretch_that_fails_fails_the_export` (kept), now with `stretch.failed` | `core/runtime/src/tests.rs` | the code missing |
| all | `a_missing_clip_file_is_named_in_the_exports_problems` | `crates/server/src/graph/tests.rs` | the old filter |

**Done when:** all green. Renders 12 of 12 (no fault in them). `m2 render` of the crash case exits non-zero.

**As built (slice 2).** Where the build went another way than the tables above:

| What | Why |
|---|---|
| The runtime's offline `Renderer` reads its ring, before each block and after it (`Renderer::faulted`), not the editor's `Render` | every offline render gets the rule, `m2 bounce`'s own loop too. Only a renderer built with an offline cache reads its ring: a live one's is the app's |
| A node's error offline is a `fault::Failed` (the fault, and the error as its words), not a second error type | its words stay what they were; `Render` turns any `Failed` into `Faulted` with the namer |
| An offline stretch that fails still fails the build, as a `Failed` with `stretch.failed` | it fails at once, and keeps the kernel's words as the detail |
| Live, the stretch thread only marks the render failed; each lane that plays it reports `stretch.failed` | the stretch thread knows no plan. A lane knows its plan, its place and the clip's, and reports once per plan |
| A lane reports through a `Tell` (its plan's reporter and its place), set by each plan that builds or keeps it. It tells a new plan what it met at once | a lane is shared by plans; its failed chunks stay silent in its slots, so a new plan would hear nothing of them |
| A plan's id is taken before its build | its build reports a file it can't read as that plan |
| The session keeps a fault of a plan not sent yet until it is sent | a build reports before its plan is sent, and the notices thread may read between |
| A file that is there but empty is silent, not a fault | it holds no audio; nothing changed |
| A plugin's error offline is told even when the instance dies first: a dead instance fails the render too | a crash during an export kills the lent instances |
| `Namer` names from any song (`faults::Song`: a revision, a compiled song, or a stem's parts); `Render::new`, `lending` and `range` take it in place of a graph and data | an offline render names its own faults; a stem's graph is rooted at a track |
| The seam is `fn(&File, usize) -> io::Result<()>` | a test fails one file's reads on one thread (the disk or stretch thread), while other tests read theirs |
| `m2` installs the probe plugin when `M2_PROBE` is set | the CLI test of the crash case needs a plugin it can crash; tests install it the same way |
| The export also carries `lane.unit-mismatch` | it plays its last accepted data, as `lane.invalid` does |
| The plugin and Sampler offline tests are in `crates/editor/src/render.rs`; the cut-short file's in `core/runtime/src/tests/stream.rs`, with `Renderer`; the missing clip's in `crates/server/src/export.rs` | each beside its rig |
| A Sampler is named as "the instrument on Drums" | it is a `poly`, named as a stopped sound is |

## Slice 3: inputs, and closing

**Goal.** The sound input reports through the same path. The contract, gaps and handoff are closed.

| Crate | Files | Change |
|---|---|---|
| `m2-play` | `core/play/src/inputs.rs`, `lib.rs` | the player holds the ring. `open_input` reports `input.missing` when the chosen input isn't there, both for devices and under `M2_AUDIO=none` ([D8](#decisions)). An input gone mid-take reports `input.lost`; the once-a-second `eprintln!` of `gone` goes (overruns stay in the log, [gaps](#what-goes-to-gapsmd)). `input_refused()`: the OS's answer, or the inject seam's |
| `m2-server` | `graph/record.rs`, `routes_record.rs` | each record tick asks `input_refused()`. Refused: the take ends, its silent file isn't kept, and `input.refused` is reported with the `NO_MICROPHONE` words. The inject route gains `refuse`. The take's own end words for an unplug come from the namer |
| docs | `architecture.md` (RENDER-30a, DIAG table, Order of work), `gaps.md`, `HANDOFF.md`, `build-notes-ex.md` (new) | |

**Tests, each shown red first.**

| Case | Test | Where | Red on |
|---|---|---|---|
| io r7 | `a_chosen_input_that_isnt_there_is_told_naming_the_one_used`: `M2_AUDIO_INPUTS="Mic A,Mic B"`, Settings chose "Mic C" | `crates/server/src/graph/record_tests.rs` | today's code |
| Mac mic | `a_take_the_os_refuses_is_dropped_and_said` (inject `refuse`) | `record_tests.rs` | today's code (a silent take kept) |
| io r2 kept | `an_input_unplugged_mid_take_is_one_problem` (the unplug test, adapted) | `record_tests.rs` | before `input.lost` |
| the app | browser suite `ex-faults`: a trapping kernel; the popover lists it, naming the track; one toast | `e2e/ex-faults.mjs` | before slice 1 (run once on it) |

**Done when:** all green. `ci/check.sh --gates` passes, run with `$SCRATCH/tw`. The Mac lint passes. A picture of the popover from the app on `:96`. By-ear steps written, with one for the owner's Mac: click Don't Allow during the first take. Six gaps lines gone.

**As built (slice 3).** Where the build went another way than the tables above:

| What | Why |
|---|---|
| An input's fault is about `Subject::Input(n)`, its opening, with plan 0. Openings are counted for the whole app (`Listen::open`) | an input is of no plan; a new player starts no count again, so an old number never names a new input |
| The session names it with an `InputNamer` per opening (the one used, and the one chosen when missing), kept for the take opened last and the one before, as plans' namers are. A fault of an opening not named yet waits | the player reports as it opens, before its take names it |
| The player reports `input.missing`. The take thread reports `input.lost` and `input.refused` | "mid-take" is the take's to know; an input unplugged between takes is not a problem, the next R opens another |
| An input's problems stay listed through a new plan, and go at the next take | an edit doesn't play the input again; the next take does (D3) |
| While the input chosen is missing, each take opens the input again | it finds the one chosen once it is plugged in, and the next take is told again |
| `InputChoice` carries the name Settings listed | a device that isn't there can only be named by what Settings kept |
| The take's own note for an unplug ("The sound input was unplugged…") went; the problem says it, naming the input | two messages for one unplug otherwise |
| A take refused is dropped as Cancel drops it (its part file goes), and the input closes | the next R asks the OS again, and is refused in words |
| The tests name the silent inputs with `Player::name_inputs`, not `M2_AUDIO_INPUTS` | the environment is the whole test process's |
| `ex-faults` was shown red with the session's list stubbed, not on the code before slice 1 | the same red (no problem listed, a toast only), without a second build of the old tree |

## Decisions

| # | Question | Pick |
|---|---|---|
| D1 | What is the ring? | Hand-written in `m2-runtime`, about 60 lines: a bounded queue of 256 slots, each with an atomic turn number (Vyukov's design). Writers claim a slot with one compare-and-swap; a full ring adds to `lost` and returns. One reader. No new crate: crossbeam-channel's wake-up may lock |
| D2 | Who names a fault? | Never its producer. It reports numbers only. The session names it, with the `Namer` of the plan it came from, kept for the plan that plays and the one before. A fault from an older plan is dropped: that part plays again in the new plan, and is told again if it faults again |
| D3 | What is "once"? | A toast once per code and part per open song. The problem stays in the popover until the part plays again: a new plan, Stop All Sound, a plugin made again, the next take. If it faults again, it is listed again, with no second toast. Lost faults add "and N more problems" to the last one |
| D4 | The offline rule (RENDER-1) | "An offline render MUST fail when any fault changes its sound, naming the part and the code: a node's error, a file that can't be read or decoded, a stretch, a plugin. Never a file with a part silent." RENDER-25's "Offline the render fails, naming the node" and RENDER-30's stretch sentence point to it |
| D5 | Strict or live? | Two things. `strict` stays what it is: an export silences NaN and infinity and counts them, as today ([gaps](#what-goes-to-gapsmd)). A plugin's error now goes by live or not: live, it goes silent alone; offline, the render fails |
| D6 | New codes (DIAG-2) | `sound.stopped` (Error), `plugin.failed` (Error), `asset.unreadable` (Error: there, but can't be read or decoded while it plays), `stretch.failed` (Error), `input.missing` (Warning), `input.lost` (Warning), `input.refused` (Error). `plugin.crashed` stays. All "on the session, not a revision; offline, the render fails". `asset.missing` stays what validation finds |
| D7 | Problems known before a render | Not faults. The export runs and names them, as `plugin.missing` does today, widened to `asset.missing`, `state.*` and `lane.invalid` |
| D8 | The chosen input isn't there | Record from the one used, and say so, as outputs do: "Mic C isn't plugged in, so this take records from Mic A." |
| D9 | The plugin host | `m2-plugins` can't see `m2-runtime`'s ring. The actor turns its `HostEvent::Crashed`/`Hung` into faults as it hears them. The audio thread reports `plugin.failed` itself |
| D10 | What isn't a fault | Misses (disk too slow), dropouts, input overruns, and a cache folder that can't take a stretch: none silences a part for good. They stay in the status and the log |
| D11 | Mac microphone | A poll each record tick, on the control side. No OS callback. `m2_os::host::microphone_refused` stays the one OS call (PLAT-2); Windows adds its own there later |

## Risks

| Risk | What we do |
|---|---|
| A storm of faults fills the ring | Each part reports once per plan, which keeps it small. A full ring counts what it lost and says so (D3) |
| An export now fails where it used to finish | That is the point. A test per case. Known problems still export (D7) |
| Plugin errors offline flip from silence to failure, and older export tests expected silence | Read each failing test: change it only where silence was the bug. Say which in the commit |
| The drain every 300 ms makes tests slow or flaky | Tests poll the news with a deadline, as the record tests do |
| A namer per plan costs time in each prepare | It is built from the compiled revision the plan already holds, once; measured in slice 1 (Linux, information only) |
| Windows | Nothing here is OS code but the microphone check. Design note only |

## What goes to gaps.md

One line each, not fixed here:

- An export whose sound had NaN or infinite samples silences them; the app's export doesn't say so (`m2 render` warns).
- When the OS can't start the stretch thread or the disk thread, warped or long clips are silent with a line in the log only.
- A stretch the cache folder can't take is made in memory with a line in the log only (kept from the sx line; no sound changes).
- `m2 check` doesn't find a file in `assets/` that won't decode; only a build does.
- A problem from a live fault can't be dismissed; it goes when its part plays again.
- Input overruns during a take are counted in the log, not told.

Removed as fixed: "An export during which the host process crashes", "A streamed file that can't be read", "A Sampler file that is in assets/ but won't decode", "A stretch that fails plays silence" (its first half), "an input chosen in Settings that is unplugged", and the Mac "Don't Allow" take.
