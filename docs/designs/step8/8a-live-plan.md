# 8a, live half: the plan (R1, slices 3 and 4, the gaps lane)

Written 2026-10-17, read-only on the repo: git log, show, diff and grep; nothing built or run. Base: branch `8a`
at `6d4086d`, worktree `$SCRATCH/wt/8a`; `main` is `8a6d9e1` (= `6d4086d` + a HANDOFF commit). Read:
`build-notes-8a.md` (every slice, "For the lead", the review), `8a-plan.md` (Attack and Decisions),
`design-8a.md` (§5.5, §6, §9, §10, §12), `$SCRATCH/s8a/refresh.md` (its Attack), `docs/architecture.md`
(RENDER-2, 2a, 25, 28, 32, HOST-2, HOST-5, PLAT-1, PLAT-2, gate 8), and the code named in §11. Also the `next`
track's diff against `main` (`$SCRATCH/wt/nx`, read only). "Estimate" marks a number nobody measured.

## In short

- **This track makes live playback use several cores.** The device's own thread runs each block on a pool,
  as exports already do. Workers are engine threads: real time at the device's period, and on a Mac they join
  the device's audio workgroup. Linux asks for real time only as far as a user without root can.
- **Four pieces, in this order:**

  | # | Piece | What it gives |
  |---|---|---|
  | 1 | R1: a mixer strip runs with its track | p50's serial tail shrinks from about 228 µs to about 20 µs a block (*estimate*). Same bits |
  | 2 | Slice 3: live on engine threads | `m2-os` gets a `Wake` and engine-thread set-up; the pool's caller sleeps on the `Wake`; the player owns a pool; same bits on 1 and N threads |
  | 3 | Slice 4: gate G12 live | `m2-gates realtime` plays p50 live (the device on a Mac, the silent clock here) and judges dropouts and p99 |
  | 4 | The gaps lane (in parallel) | a kernel that crashes goes silent alone and the app says so; out of memory says "memory"; `pl-mix` made sturdy |

- **Same bits everywhere.** No render record changes. Every live test that runs on N threads checks that tasks
  really ran on workers.
- **A task panic still crashes the player,** as today, through `crash()`. A worker that never comes costs only
  speed. A worker stuck inside a task can't be abandoned safely, so the device thread sleeps on its `Wake` (no
  CPU) and the stall path replaces the player after 1 s, as today (§8, Q1).
- **Plugins stay on one thread.** A song with plugins renders every task on the device's thread.
- **Park by default.** Workers sleep after each callback. One constant (`BETWEEN`) switches to spinning, and
  G12's `--between park,spin` measures both on the Mac.
- **The Mac's check grows by about 2 minutes** (one minute of p50 live, muted). The owner's own Mac run is
  about 35 minutes and decides five things (§7).
- **Time** (agent, *estimate*): about 2 days for this track; the gaps lane about 1 day beside it.

---

## 0. Before anything

| Step | Command or rule |
|---|---|
| Merge `main` into `8a` | `git -C $SCRATCH/wt/8a merge --ff-only main` (8a is an ancestor of main: a fast-forward, nothing rewritten) |
| The gaps lane's worktree | `git -C $SCRATCH/wt/8a worktree add -b 8a-gaps $SCRATCH/wt/8a-g 8a`, after the merge |
| Targets | each worktree its own `target/` (two agents build at once). Never move one between worktrees. Delete `$SCRATCH/wt/8a-g/target` when the lane is merged; `df -h /` at each slice's end |
| Environment | every cargo or music2 command: `CARGO_HOME=/home/<user>/.cargo RUSTUP_HOME=/home/<user>/.rustup`; music2 runs: `HOME=$SCRATCH/8a-scratch/home M2_AUDIO=none` (the gaps lane: `.../8a-scratch/g-home`) |
| Per-commit check | `$SCRATCH/s8a/s2-check.sh <crates>` (renders 10 of 10, the crates' tests, clippy, the Mac lint). The gaps lane copies it to `$SCRATCH/s8a/g-check.sh` with its own worktree and scratch paths |
| Red runs | a small deliberate bug for each new test, as slice 2 did: `$SCRATCH/s8a/s3-red/` and `$SCRATCH/s8a/g-red/` (`<name>.diff`, `<name>.log`, a copy of `s2-red/mutate.py`) |
| Long jobs | `TW_WAIT=<s> $SCRATCH/tw <name> '<cmd>'`; `uptime` first, said in the notes. Xvfb display `:95` only if needed |
| Never | touch `wt/nx*`, send keys to tmux `m2`, `1`, `2`, `ssh_tmux`, `cargo update`, push, rebase |

---

## 1. What the code is today, and what `next` changes

| Piece | Today (`6d4086d`) | What it means here |
|---|---|---|
| `m2-pool` (`core/pool`) | `Pool`, `Runner` (`one`, `wake`, `run`, `rest`, `spread`, `counters`); the caller never parks: with no task finishing for `stuck` it calls `std::thread::sleep(stuck / 4)` (`run.rs` `help`, `together`); workers `park`; a task panic is caught and returned by `run`; a panic in pool code aborts; `Setup.start` only | gains the caller's `Wake`, a `stop` hook, a test `trip`, two counters |
| Executor (`core/runtime/src/lib.rs`) | `render_block_on` runs a plan on any `Runner`; a plan with plugins runs every task on the caller (`Runner::one()` swap); `run_graph` raises a task's panic again after the run (`resume_unwind`) | the panic path to `crash()` already works; nothing in `run_part` changes in this track |
| Live engine (`engine.rs`) | `Engine::process_on(.., runner)` exists; `process` uses `Runner::one()` | the player passes its runner |
| Player (`core/play/src/lib.rs`) | `Callback::fill` → `fill_in` (catch_unwind → `crash()`), `Live::fill` → `engine.process`, `LiveFade`; `Player::start` (cpal, the `m2-play` thread, `serve`), `silent()`, `driven()` | gains a runner, a handover message, `Canon` and the park/spin step in `fill` |
| `m2-os` | `thread::{token, background, interactive, cpu_time}`, `process::{cores, load_average, waited}`; mac code already declares `extern "C"` blocks and links frameworks (`mac/watch.rs`) | gains `rt` (Wake, Group, engine threads) |
| G12 (`crates/gates/src/realtime.rs`) | `--offline` only: sweep, bits, paced, partitions, cost, tails | gains the live runs in a new file `realtime/live.rs` |
| `ci/check.sh` | exports `M2_AUDIO=none`; the gates row runs each `ci/gates.txt` line | G12's live run opens the device explicitly, muted, so the check still plays nothing aloud |

**What `next` changes in the same files** (`$SCRATCH/wt/nx`, not on `main` yet; read only):

| File | `next`'s edit | How this track stays out of its way |
|---|---|---|
| `core/play/src/lib.rs` | `Player::start` → `start_on(&Choice)` → `start_in(Audio, &Choice)` (no device, output choice, buffer size); `silent()` → `silent_as(name, frames)`; `Shared` gains `frames`, `fell_back`, `click`; `fill_in` stores `frames` at its top; `render` destructures `shared` and calls `set_click` | this track's work goes in two new files (`engines.rs`, `timing.rs`). In `lib.rs` only short hooks: one field each in `Callback` and `Shared`, one `Msg` and one `Gone` variant, the `fill` wrapper (which `next` doesn't touch), three lines after `stream.play()` |
| `core/play/src/outputs.rs` (new in `next`) | device choice | not touched |
| `core/runtime/src/engine.rs` | `click` field, `set_click`, `click.mix` after the render | this track doesn't touch `engine.rs`; the gaps lane touches only the render's `Err` arm |
| `core/os` | `midi.rs` behind a feature; one line each in `lib.rs`, `linux/mod.rs`, `mac/mod.rs` | `rt.rs`, `mac/rt.rs`, `linux/rt.rs` are new files; one `mod` line each |

Merge note for whichever lands second: this track's `Player::start_device(frames, live)` is `next`'s
`start_in(Audio::Device, &Choice { frames, .. })` with a `LiveThreads` passed through; its `silent_with(frames,
live)` is `next`'s `silent_as(name, frames)` with the same. Slice 4 starts with a merge of `main`: if `next` is
on `main` by then, build `start_device` and `silent_with` on its shape from the start.

---

## 2. R1: a mixer strip runs with its track (first commit)

### In short

- p50's song partition holds 51 `std:gain` nodes (each channel's fader and the master's): 211 µs of a 226 µs
  serial tail, 37% of a 4-thread block (build notes, run F).
- Two small rules move the strips into their tracks. Partitioning never changes bits (RENDER-2).

### The two rules

| Rule | Where | What |
|---|---|---|
| A. Constants run in no partition | `core/runtime/src/parts.rs`, `Schedule::build` and `Schedule::of` | A node whose exec is `Exec::Const` or `Exec::Notes` does nothing in `step()`. Its buffer is written only by the pre-pass (`Cells::apply`) or never. So it joins no group, gets `part_of = NONE`, and reading it makes no edge: it is frozen for the block, like a node outside the order. `may_read` already allows `NONE`. Partitions (`keys` in `build`) are made only from nodes that run, so a label holding only constants makes no empty task. The 52 no-op calls leave the song's tail too |
| B. A strip joins its track | `core/runtime/src/lib.rs`, `labels()` (gets `order: &[usize]`) | After overlays and fades are labelled: walk the order; a plan node (`i < n`) whose reads, constants aside, all have one label L other than its own, with at least one such read, takes label L. In plan order, so a chain moves whole (`pan` then `gain`). Then fades are labelled again from `w.rewire` (a fade follows the node that reads it) |

What stays in `song`: each `std:add` of `sum` (it reads two tracks), the master's `stereo`, `gain` and
`limit`, and anything reading a fade (during an edit window only: that plan's strip stays in `song`; speed
only, no bits). The depth split still runs after the labels, so any labelling stays acyclic and convex.

### Tests

| Test | Where | Proves | Red by |
|---|---|---|---|
| `a_mixer_strip_runs_with_its_track` | `core/runtime/src/tests/parts.rs` | first-song: every channel's `std:gain` (and a `pan` when written) has its track's label; the master's gain is in `song`; `follow(kick.out)` joins `kick`; the ducker that reads two tracks stays in `song` | `r1-no-strips` (rule B left out) |
| `constants_run_in_no_partition` | same | every `Const` and `Notes` node has `part_of == NONE`; no edge starts at one | `r1-const-parts` (rule A left out) |
| `the_song_tail_is_the_sum_and_the_master` | same | p16: the `song` partition holds exactly its adds and the master chain | `r1-no-strips` |
| existing: `every_edge_goes_forward`, `random_convex_partitions_give_the_same_bits`, `every_pure_fixture_renders_the_same_bits_on_1_and_n_threads`, `an_edit_window_renders_the_same_bits_on_1_and_n_threads`, the debug read check | runtime, play | the same bits, any labels | — |

Numbers (Linux, information): `realtime --offline --reference 50 --sweep 1,4,8`, held on CCD1
(`taskset -c 8-15`), before and after, `uptime` first: the song's tail, the 4- and 8-thread means and p99s.

### Commits

1. "A mixer strip runs with its track, and constants with no task (R1)": `parts.rs`, `labels()`, the tests,
   RENDER-2's words (§3.8).
2. "Build notes for 8a: R1" (with the numbers and logs).

---

## 3. Slice 3: live playback on engine threads

### In short

- `m2-os` gets `rt`: a `Wake` (a futex on Linux, a Mach semaphore on a Mac), the device's `Group` (period and,
  on a Mac, its audio workgroup), and `engine_thread` / `end_engine_thread` / `on_device_thread`.
- The pool's caller sleeps on its `Wake` when a task is stuck; the task that finishes rings it. Workers keep
  std's `park` (std starts them, so their handles never go stale).
- The player builds its pool on the `m2-play` thread just after the stream starts, and hands the runner to
  the callback as a message. The callback renders each block on it, then sends the workers to sleep.
- The thread count rule: `M2_THREADS` if set; else 1 for the silent and driven players; else the P-cores
  sharing one L2 (capped by the workgroup's advice) on a Mac; else one cache's cores on Linux when real time
  is granted, and 1 when it is refused (this server: 1).

### 3.1 `m2-os`: `rt` (new `core/os/src/rt.rs`, `mac/rt.rs`, `linux/rt.rs`)

```rust
/// One thread sleeps until another rings, or a time passes. A ring before the wait makes the wait
/// return at once. No allocation, no lock. Linux: a futex on one word. macOS: a Mach semaphore.
pub struct Wake { .. }                       // Send + Sync
impl Wake {
    pub fn new() -> io::Result<Wake>;         // off the audio thread (Mac: semaphore_create)
    pub fn ring(&self);                       // never blocks
    pub fn wait(&self, at_most: Duration) -> bool; // true when rung, false when the time ran out
}

/// What engine threads need from the output device: its period, and on a Mac its audio workgroup.
pub struct Group { .. }                       // Send + Sync; Mac: os_release on drop
impl Group {
    pub fn of_output(uid: Option<&str>, rate: u32) -> Group; // None: the default output. Never fails: what's missing is in `missing()`
    pub fn none(period: Option<Duration>) -> Group;          // the silent and driven players: no real time
    pub fn period(&self) -> Option<Duration>;
    pub fn recommends(&self) -> Option<usize>;               // Mac: os_workgroup_max_parallel_threads
    pub fn real_time(&self) -> bool;                         // whether engine threads can be real time here
    pub fn missing(&self) -> Option<String>;                 // plain words: "no workgroup: ..."
}
#[derive(Clone, Copy, Debug)]
pub struct Joined { pub real_time: bool, pub workgroup: bool }
pub fn engine_thread(g: &Group) -> Joined;   // the calling thread becomes an engine thread
pub fn end_engine_thread();                  // before it exits (Mac: leave the workgroup)
pub fn on_device_thread(g: &Group) -> Joined; // once, on the device's own thread
```

| Piece | macOS (`mac/rt.rs`) | Linux (`linux/rt.rs`) |
|---|---|---|
| `Wake` | `semaphore_create` / `semaphore_signal` / `semaphore_timedwait` / `semaphore_destroy` (4 `extern "C"` lines; `mach_task_self` is in libc) | a `AtomicU32`: `ring` swaps 1 and `FUTEX_WAKE`s when it was 0; `wait` takes a 1 at once, else `FUTEX_WAIT` on 0 with the timeout, then takes the 1 (`libc::syscall(SYS_futex, ..)`) |
| `Group::of_output` | the UID as a `CFString`; `AudioObjectGetPropertyData` on the system object, `'uidd'` (translate UID to device), then `'oswg'` (the IO thread's workgroup, retained), then `'fsiz'` (buffer frames) for the period. `#[link(name = "CoreAudio", kind = "framework")]`. Read **after** the stream started (the refresh's Attack, row 19) | the period from the frames if known; `real_time` = `RLIMIT_RTPRIO > 0` |
| `engine_thread` | `thread_policy_set(mach_thread_self(), THREAD_TIME_CONSTRAINT_POLICY, { period, computation = period / 2, constraint = period, preemptible = 1 })` in Mach ticks (`mach_timebase_info`); then `os_workgroup_join(wg, &token)`, the 40-byte token kept in a `thread_local!` of `mac/rt.rs` until `end_engine_thread` (first touched at start, off the block path) | `sched_setscheduler(0, SCHED_FIFO, prio)`, prio 10 or the limit if lower; refused: `real_time: false`, never fatal |
| `end_engine_thread` | `os_workgroup_leave` with the kept token | nothing |
| `on_device_thread` | nothing to do: CoreAudio made its IO thread real time and put it in the group; reports `{ true, true }` | the same `SCHED_FIFO` call, once, on cpal's ALSA thread |
| not built | `os_workgroup_testcancel` each fill (cpal's `DeviceChanged` already makes the server replace the player) | rtkit, `RLIMIT_RTTIME` (the refresh's Attack: Deferred with one trigger) |

Also `thread::foreign(f) -> io::Result<()>` (`#[doc(hidden)]`): runs `f` on a thread made with
`pthread_create`, which std didn't start, and waits for it. Tests use it to drive the callback as CoreAudio
does.

Tests (`core/os`):

| Test | Proves | Red by |
|---|---|---|
| `a_wake_rung_before_its_wait_returns_at_once` | `wait(1 h)` after `ring()` returns true at once (60 s watchdog) | `os-wake-late` (wait skips the take before sleeping: hangs) |
| `a_ring_from_another_thread_ends_the_wait` | a thread waits 1 h; another rings; it returns true | `os-wake-nowake` (no `FUTEX_WAKE`: hangs) |
| `an_unrung_wait_times_out` | `wait(5 ms)` returns false | — |
| `a_wake_works_on_threads_std_did_not_start` | both sides through `thread::foreign` | — |
| `an_engine_thread_gets_fifo_or_says_why` (Linux) | granted: `sched_getscheduler` is FIFO; refused: `real_time: false` and `RLIMIT_RTPRIO` is 0 (this server) | `os-fifo-claims` (reports granted when refused) |
| `a_group_without_a_device_has_no_workgroup_and_says_so` | `Group::none`: no recommendation, `missing()` in plain words | — |
| `an_engine_thread_has_the_time_constraint_policy` (Mac) | `thread_policy_get` reads it back | the owner's check runs it |
| `a_worker_joins_and_leaves_the_default_outputs_workgroup` (Mac) | `Group::of_output` on the default output: a workgroup, a recommendation ≥ 1, join and leave | the owner's check runs it |
| `the_wake_never_blocks` (`core/os/tests/no_blocking.rs`, new) | grep over the `Wake` code in `mac/rt.rs` and `linux/rt.rs`: no allocation, lock or I/O | a `vec!` put in `ring` |

### 3.2 `m2-pool`: the caller sleeps on a `Wake`

| Change | Where | What |
|---|---|---|
| Dependency | `core/pool/Cargo.toml` | `m2-os.workspace = true`. `Cargo.lock` gains one line in `m2-pool`'s entry: one `cargo check --offline -p m2-pool` without `--locked`, then `git diff Cargo.lock` must show only that (8a-plan Decision 28). Every other build `--locked` |
| The caller's sleep | `run.rs` | `Shared` gains `asleep: Padded<AtomicBool>` and `bell: m2_os::rt::Wake`. Where the caller now calls `std::thread::sleep(stuck / 4)` (in `help` and in `together`'s wait for `inside == 0`), it **dozes**: `asleep = true` (SeqCst); look again at what it waits for (`left`, or `inside`); if it changed, take the flag back, and if a ringer took it first, `wait` once to take that ring; else `bell.wait(doze)`, then `asleep = false`. The ringer: after `left −1` in `complete`, and after `inside −1` in a worker, `if asleep.load(SeqCst) && asleep.swap(false, SeqCst) { bell.ring() }`. Dekker's pattern again: either the caller sees the change, or the ringer sees the flag |
| `Setup` | `lib.rs` | `doze: Duration` (the longest one sleep lasts; a ring ends it sooner) and `stop: Option<Arc<dyn Fn(usize) + Send + Sync>>` (run by a worker before it exits, inside its abort guard: the Mac leaves the workgroup there). `plain()`: patience 50 µs, stuck 1 ms, doze 1 ms |
| Counters | `lib.rs` | the caller's `Counters` gain `dozes`, `rung` and `timeouts` |
| A test trip | `lib.rs`, `run.rs` | `#[doc(hidden)] Runner::trip(task: u32, run: u64, what: Trip)` with `enum Trip { Panic, Stall(Duration) }`, and `Runner::runs()`. `run_one` checks it inside the task's `catch_unwind`, before the task: so the panic is a task's panic, raised on the caller after the run. The message names the thread by number (`me`; 0 is the caller), as `thread::current` is banned there. One compare per task |
| Models | `sync.rs`, `tests/models.rs` | the shim wraps the `Wake` with a chaos point before `ring` and `wait`. New model `the_caller_is_woken`: random task spins, `stuck` 0 to 20 µs (the caller dozes often), `doze` 10 s; each case asserts `timeouts == 0` (a lost ring times out) |

The pool's doc changes: "no OS calls of its own: the caller's one sleep goes through `m2-os`'s `Wake`".

Tests and mutants:

| Test | Proves | Red by |
|---|---|---|
| `a_stuck_caller_dozes_until_the_task_ends` (`src/tests.rs`) | 4 threads, spread; a root waits on a latch; `stuck` 1 µs, `doze` 1 h; the test waits until the caller's `dozes ≥ 1`, opens the latch; `run` returns; `rung ≥ 1`, `timeouts == 0` (60 s watchdog) | `pool-no-ring` (no ring in `complete`: hangs) |
| `a_worker_that_never_comes_is_not_waited_for` | 4 threads; worker 3's `start` waits on a latch the test holds; 1,000 runs of random graphs complete, each task once after its inputs; then the latch opens and the pool drops | `pool-wait-woken` (the caller waits for every woken worker to enter the run: hangs) |
| `stop_runs_on_each_worker_before_it_exits` | a counter: +1 in `start`, −1 in `stop`; 0 after the drop | `pool-no-stop` |
| `a_tripped_task_panics_on_a_worker_and_is_raised` | spread; trip a root at run 3; `run` returns its payload, naming a worker (not 0); the pool runs on after | `pool-trip-outside` (the trip outside the `catch_unwind`: the binary aborts) |
| model `the_caller_is_woken` | above | M6: the caller sets `asleep` after its second look (a lost ring: timeouts > 0); M7: the ringer rings before `left −1` (the caller sleeps on: timeouts > 0) |
| `the_block_path_never_blocks` (grep) | unchanged list; `run.rs` still clean | — |

Time: the "pool models" row now builds `m2-os` and libc too: from about 5 s to about 10 s on Linux
(*estimate*), still under its "1 min".

### 3.3 `m2-runtime`

| Change | Where |
|---|---|
| `pub mod fpenv` (was private): `Canon`, `canon`, `is_canon`, `mode() -> u64` (the raw register, for G12's set-up line), `#[doc(hidden)] flush_for_tests(on)` (sets FZ and DAZ on x86, FZ on arm64) | `lib.rs` one line; `fpenv.rs` |
| `threads::live(group: Arc<Group>, period: Duration, spin: bool, mark: Option<fn(bool)>) -> (Setup, Arc<Joins>)`: `start` = `tls_eager_initialize`, `canon`, `m2_os::rt::engine_thread(&group)` counted into `Joins` (workers, real time, joined), `mark(true)`; `stop` = `mark(false)`, `end_engine_thread()`. Patience: half a period, or 1.25 periods with `spin`; `stuck` and `doze`: one period. (`Between` lives in `m2-play`; the runtime only takes the bool) | `threads.rs` |
| `threads::live_workers() -> usize` (alive now: +1 in `start`, −1 in `stop`), for the let-go test | `threads.rs` |
| `threads::from_env() -> Result<Option<usize>>`: the one `M2_THREADS` parse; `m2_editor::render::Threads::from_env` calls it | `threads.rs`; `crates/editor/src/render.rs` (a few lines) |

Nothing else in `m2-runtime` changes in slice 3: `run_part`, `run_graph`, `KernelInst`, `plan.rs` and
`engine.rs` belong to the gaps lane (§5.6).

### 3.4 `m2-play`: the player's engine threads

**New `core/play/src/engines.rs`:**

```rust
/// How a player's engine threads run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveThreads { pub count: Option<usize>, pub between: Between }
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Between { Park, Spin }
/// After a callback, workers sleep at once. The owner's Mac run may switch this one line (§7).
pub const BETWEEN: Between = Between::Park;
impl LiveThreads { pub fn from_env() -> Result<LiveThreads> }   // M2_THREADS, BETWEEN

/// The thread count rule, the device thread included.
pub fn live_count(asked: Option<usize>, cores: &Cores, group: &Group, on_device: bool) -> usize;
/// What a player's engine threads did, for G12.
pub struct EngineThreads { pub count: usize, pub real_time: usize, pub joined: usize,
                           pub missing: Option<String>, pub between: Between, pub device_mode: u64 }

pub(crate) struct Handover { runner: Runner, group: Arc<Group>, between: Between }
pub(crate) struct Engines { pub pool: Option<Pool>, pub handover: Box<Handover>, pub joins: Arc<Joins> }
pub(crate) fn for_device(device: &cpal::Device, rate: u32, live: LiveThreads) -> Engines; // Group::of_output by the device's UID
pub(crate) fn for_clock(frames: usize, rate: u32, live: LiveThreads) -> Engines;         // plain threads, no real time
pub(crate) fn after_callback(runner: &mut Runner, between: Between);                     // Park: rest()
```

The count rule (`live_count`), one place:

| Case | Threads |
|---|---|
| `M2_THREADS`, or a count given (tests, G12) | that count, 1 to 64 |
| the silent player, a driven player | 1 |
| on a device, real time refused (`!group.real_time()`: Linux without `RLIMIT_RTPRIO`) | 1: normal-priority workers wake late and can be preempted inside a run (the refresh's Attack, A.9) |
| on a device, a Mac | `min(group.recommends() or per_cache, per_cache)`: the P-cores sharing one L2, `hw.perflevel0.cpusperl2` (4 on the M1 Pro, *estimate*); E-cores never |
| on a device, Linux with real time | `per_cache` (one CCD here) |
| `Pool::new` fails (thread limit) | 1, and `EngineThreads.missing` says why |

**The hooks in `core/play/src/lib.rs`** (each short; `fill_in`, `take_held`'s other arms and `serve` stay as
they are):

| Hook | What |
|---|---|
| `mod engines; mod timing;` and their `pub use` | |
| `Msg::Threads(Box<Handover>)`; `Gone::Runner(Box<Handover>)` | the handover, and what it replaces, leaving for the dropper |
| `Callback` gains `runner: Runner` (`Runner::one()` at first) and `between: Between` | |
| `take_held`: `Msg::Threads(mut h)` → swap `h.runner` with `self.runner`, `on_device_thread(&h.group)` once, retire the box (holding the old runner) | applied at the start of a callback, between runs, like a replace; `leftover` gets its arm |
| `Live::fill(.., runner)` → `engine.process_on(.., runner)`; `render` passes `&mut self.runner` to the live and to the fading engine | one destructure line (`next` edits the same line: a trivial merge) |
| `fill` (the wrapper): `let _mode = fpenv::Canon::enter();` first (RENDER-2a for the resampler and the 5 ms fade; the device thread's own mode back at the end); after `fill_in`: `engines::after_callback(&mut self.runner, self.between)`; the timing record (slice 4) | `next` doesn't touch `fill` |
| `start()`'s `open` closure: after `stream.play()?`: `let e = engines::for_device(&device, config.sample_rate, LiveThreads::from_env()?)`; send `Msg::Threads(e.handover)` when there is a pool; return `e.pool` with the stream; after `serve(..)` returns, `drop(pool)` (it joins the workers, each leaving the workgroup) | the pool lives on the `m2-play` thread, never on the audio thread; a runner whose pool dropped runs alone, so a last CoreAudio callback after the drop is safe |
| `silent()`: `engines::for_clock(frames, SR, LiveThreads::from_env()?)`; the runner goes straight into the callback; the pool moves into the clock's thread and drops after the loop | 1 thread unless `M2_THREADS` |
| `Player::driven_on(rate, runner: Runner)`; `driven(rate)` = `driven_on(rate, Runner::one())`; `Driver` gains `#[doc(hidden)] runner()` (spread, trip, counters) | tests keep their own pool |
| `Player::engine_threads() -> EngineThreads` | G12's set-up line |
| `core/play/tests/no_blocking.rs`: add `engines.rs`, `timing.rs` to its file list | |
| `alloc::all() -> u64` (the process's count, one line in `alloc.rs`) | `no_alloc_workers.rs` reads workers' counts with it |
| `Cargo.toml`: `m2-os` moves from dev-dependencies to dependencies | `Cargo.lock` already lists it under `m2-play`: no lock change |

Why a message for the runner: the workgroup is read after the stream starts, so the pool is made after the
callback exists. The queue keeps the order (the runner arrives before any project), and its handler runs
between runs, where swapping the runner is safe. Until it lands, the first callbacks run on one thread.

### 3.5 A task panic, and a worker that never joins

| What happens | Built as | Test |
|---|---|---|
| A task panics on a worker | caught by the pool, the block finishes, `run` returns the payload, `run_graph` raises it again, `Callback::fill_in`'s `catch_unwind` calls `crash()`: the player says crashed, the server makes a new one (RENDER-28's existing path) | `a_panic_on_a_worker_crashes_the_player` |
| A worker never comes to a block (not scheduled, its `start` still running) | the caller and the other workers take every task; nothing waits for it | `a_worker_that_never_comes_costs_only_speed` |
| A worker stays inside a task (stuck) | the device thread can't abandon the block: the task reads the device thread's own stack (the `Block`, the closure) and the plan. So it dozes on its `Wake`, using no CPU, and the task that ends rings it. Past 1 s with no callback, `Player::stalled()` and the server replaces the player, as with one thread today | `a_stuck_task_puts_the_device_thread_to_sleep_on_its_wake` |
| A worker refused real time or the workgroup | it runs on as a plain thread; `EngineThreads` counts it; G12 says "not the designed set-up" and doesn't pass | G12's verdict tests |

### 3.6 Tests (each shown red once)

| Test | Where | Proves | Red by |
|---|---|---|---|
| `live_on_n_threads_is_the_export_on_1` | `core/play/tests/bit_identity.rs` | every pure fixture, its whole song and a second, through a driven player on 2, 4 and 7 threads with spread, callbacks of mixed sizes: equal to the 1-thread export (RENDER-33). Each fixture with two partitions or more: tasks on workers > 0; reads across threads summed > 0 | `s3-live-one` (`Live::fill` ignores the runner: tasks on workers 0) |
| `edits_on_engine_threads_match_one_thread` | same | G7c's no-op activations and an edit with ghosts and fades (first-song, p16), 4 threads with spread against 1 thread, sample for sample | `rt-fade-reads` (a fade's old side left out of `reads()`: a missing edge; the debug read check fires on a worker, the player crashes, the output differs) |
| `a_resampled_song_on_n_threads_matches_one_thread` | same | resample at a 44.1 kHz device: 4 threads equal 1 | — |
| `the_callback_mixes_in_the_one_mode` | `core/play/tests/engine_threads.rs` (new) | a song whose output is subnormal (a sine × 1e-20 × 1e-20), at a 44.1 kHz device (the resampler) and through a replace (the 5 ms fade); the test thread sets FZ and DAZ (`fpenv::flush_for_tests`); the output equals the same run with the mode clear; after the fills the thread flushes again | `s3-no-canon` (no `Canon` in `fill`) |
| `a_panic_on_a_worker_crashes_the_player` | same | 4 threads, spread; trip `Panic` on a track's partition (a root, so a worker runs it) at block 50; the callback returns; `player.crashed()`; silence after; a panic hook saw it on a worker | `s3-drop-payload` (`run_graph` drops the payload: not crashed) |
| `a_worker_that_never_comes_costs_only_speed` | same | a pool whose worker 3 waits in `start` on a latch; 2 s of first-song on 4 threads (no spread) equal 1 thread; then the latch opens and the player is let go | `pool-wait-woken` (hang, 60 s watchdog) |
| `a_stuck_task_puts_the_device_thread_to_sleep_on_its_wake` | same | the callback driven from `m2_os::thread::foreign` (a thread std didn't start); trip `Stall(30 ms)` on a root at block 40; `stuck` 1 ms; the caller's `dozes ≥ 1`, `rung ≥ 1`; the same bits as 1 thread | `pool-no-ring` (`rung` stays 0) |
| `a_player_let_go_joins_its_workers` | same | `silent_with(480, LiveThreads { count: Some(4), .. })` plays a sine, is let go; `threads::live_workers()` falls back to where it was | `s3-forget-pool` (the pool leaked) |
| `the_live_count_follows_the_rule` | `core/play/src/engines.rs` | §3.4's table on made-up `Cores` and `Group`s | `s3-count-rt` (real time refused ignored: 8, not 1) |
| `engine_threads_never_allocate` | `core/play/tests/no_alloc_workers.rs` (new: its own process, as the refresh's Attack asks, since `ALL` is one count for the process) | a driven player on a 4-thread live set-up (`threads::live` with `Group::none`, `mark = alloc::set_in_callback` so each worker is under the trap from its start to its stop): a fresh worker's first block, play, a seek, an activation with ghosts and fades, a panic, stop, idle, play: `alloc::all()` unchanged (read before the pool drops) | `s3-alloc-task` (a `Vec` made in one task: counted) |
| existing `no_alloc.rs`, `live.rs`, `mute.rs`, `churn.rs`, the runtime's live tests | | unchanged: `driven(rate)` is `driven_on(rate, Runner::one())` | — |

Time (debug, Linux, *estimate*): `bit_identity` +40 to 60 s for the live N-thread runs; if over 60 s, the
whole songs on 2 and 7 threads and only the first 10 s on 4 (as Decision 14 did offline).

### 3.7 Measurements (Linux, information)

With `uptime` first: the existing gates still pass (`ci/check.sh --gates` at the slice's end runs them).
No timing of the live path here beyond G12's silent clock (slice 4).

### 3.8 Contract edits (in the slice's last commit, with R1's in R1's commit)

| Rule | The edit |
|---|---|
| RENDER-2 (R1's commit) | after "an overlay to the track it carries": "A node whose inputs all come from one other module, constants aside, runs with that module: a mixer strip runs with its track. A constant is written only before the graph, so reading it waits for nothing." *Today* adds `a_mixer_strip_runs_with_its_track` |
| RENDER-2 *Today* | "Since 8a's live half, live playback renders on the player's engine threads with the same bits as one thread (`live_on_n_threads_is_the_export_on_1`, `edits_on_engine_threads_match_one_thread`, each run checked to run tasks on workers)." |
| RENDER-2a *Today* | the player's own mixing (the resampler, the 5 ms fade) runs in the one mode, the device thread's own mode put back after each callback (`the_callback_mixes_in_the_one_mode`) |
| RENDER-28 | "Engine threads (the device's callback thread and the player's workers) MUST NOT allocate, lock, do I/O or wait on a compile while they render. They wait only for work a running thread took: they spin while it moves, and sleep when it doesn't (the device thread on its `Wake`, workers parked), so a stuck task costs a dropout and no CPU. Set-up system calls happen once per thread, at its start or on its first fill. A panic in a task makes the player crash, as a panic in the engine always did; a panic outside a task aborts. Debug builds and CI run every engine thread under the allocation trap." The existing bullets stay; *Today* names the new tests |
| RENDER-28a (new) | "Live workers are real-time threads at the device's period, and on a Mac they join the output device's audio workgroup. How many, the device thread included: `M2_THREADS` if set; else the P-cores that share one L2 cache, no more than the workgroup advises (macOS); one cache's cores when real time is granted (Linux); else 1. The silent player uses 1. After each callback the workers sleep. A player let go joins its workers. A song with plugins renders on the device's thread alone. Offline renders never use real-time threads." *Why:* a worker that isn't real time can be preempted inside a block. *Today:* "since 8a's live half (`core/play/src/engines.rs`); the Mac's run is owed." |
| HOST-2 *Today* | add: "Since 8a's live half a song with plugins renders all its partitions on the device's thread: workers never call a plugin." (the rule's words stay) |
| Deferred (new rows) | rtkit and `RLIMIT_RTTIME` for Linux engine threads (trigger: someone plays music2 live on a Linux desktop); plugins on workers, with `is_audio_thread` true on every engine thread and the probe's tests (trigger: a song with plugins measured over its budget on one thread); `os_workgroup_testcancel` each fill (trigger: a dropout traced to a cancelled workgroup) |

### 3.9 Commits

| # | Subject | Holds |
|---|---|---|
| 3 | "m2-os: a Wake, and engine threads that run as audio threads" | `core/os` `rt.rs`, `mac/rt.rs`, `linux/rt.rs`, `thread::foreign`, its tests, `tests/no_blocking.rs` |
| 4 | "The pool's caller sleeps on a Wake while a task is stuck" | `core/pool` (+ `Cargo.lock`'s one line), the model, the tests, the trip |
| 5 | "The player renders on engine threads: driven and silent" | `threads::live`, `fpenv` public, `from_env`, `engines.rs` (`for_clock`, the count rule, `BETWEEN`), the `lib.rs` hooks but `start()`'s, `driven_on`, every test of §3.6 |
| 6 | "On a device, engine threads are real time, in the device's workgroup on a Mac" | `for_device`, `start()`'s three lines, `on_device_thread` in `take_held` |
| 7 | "Contract: engine threads; build notes: slice 3" | §3.8, build notes "Slice 3" with each red run |

---

## 4. Slice 4: gate G12 live

### In short

- The player keeps a record of every callback: a histogram of its time, late callbacks, gaps, dropouts the
  device reports, frames, crashes and non-finite samples (`Player::timing()`).
- `m2-gates realtime` without `--offline` plays p50 live for `--minutes`, muted, and judges it: no dropout from
  the first callback, p99 at most 70% of the period, no crash, every worker set up as designed.
- On a Mac it opens the device itself, whatever `M2_AUDIO` says. Here there is no device: it plays on the
  silent clock, and its timings are information (PLAT-1).
- `ci/gates.txt` gains one line: one minute of p50. The Mac's check grows by about 2 minutes.

### 4.1 The record (new `core/play/src/timing.rs`)

| In `Shared` (one field, `timing: timing::Record`) | Written by |
|---|---|
| `hist: [AtomicU32; 2048]` (4 µs buckets, to 8.2 ms), `over`, `max_ns`, `callbacks` | `fill`, after `fill_in`: `took` against the period, relaxed adds |
| `late` (longer than the period), `gaps` (entry to entry over 1.5 periods; the last entry is a plain `u64` in `Callback`) | same |
| `frames` (device frames filled), `device_mode` (the FP register at the first fill, before `Canon`) | same |
| `non_finite` (the playing renderer's `non_finite()`), stored in `publish()` | one line in `publish` |

`Player::timing() -> Timing` (allocates its copy, off the audio thread): those numbers, `xruns` from
`device_errors()` (cpal's xruns are CoreAudio's overloads too), `crashed`, the engine clock. `Timing::since(&before)`
and `Timing::quantile(q)`. The 8b.8 `Load` (`longest`, `late`) stays as it is: the status pill reads it.

Tests (`timing.rs`'s own module): `a_late_callback_and_a_gap_are_counted` (a driven player at 128 frames:
trip `Stall(10 ms)` once, then fill, sleep 10 ms, fill: `late ≥ 1`, `gaps ≥ 1`; red by `s4-late-ms`, the period
in ms not ns), `the_histogram_counts_each_callback_once`.

### 4.2 The gate (new `crates/gates/src/realtime/live.rs`; `realtime.rs` keeps the offline half)

**Options** (`main.rs` and `USAGE`, `README.md`):

| Option | Meaning | Default |
|---|---|---|
| (no `--offline`) | live | — |
| `--reference N` | the song: p16 or p50 | 50 |
| `--song DIR` | another song (with plugins: renders on one thread, said) | — |
| `--minutes M` | how long each run plays (the existing option) | 10 |
| `--frames F` | the device buffer | 128 |
| `--threads N` | engine threads | the live rule |
| `--sweep LIST` | live: each count plays `--minutes` | — |
| `--between park,spin` | each mode plays | the build's `BETWEEN` |
| `--audible` | play it aloud | muted: rendered, zeros to the device |

**How a run goes:** write p50 and compile it (as the offline half does); a renderer, live (not strict), looping
the whole song (512 beats, 4:02: 10 minutes hold two wraps). Open the player with
`Player::start_device(Some(frames), live)` (new: the device whatever `M2_AUDIO` says, `next`'s `start_in(Device,
..)` shape after the merge), then `mute_output(!audible)` (new: `fill` writes zeros after rendering). No device: on
a Mac a FAIL ("no output device"); on Linux `Player::silent_with(frames, live)` and every timing line says "not
judged on Linux (PLAT-1)". Open, play, take `timing()` at the first callback and at 5 s, play to the end, take it
again, stop, let go.

**What it prints and judges** (a judged run: 128 frames, on a Mac):

| Line | Bound |
|---|---|
| set-up: machine, `cores:`, the workgroup's advice, threads used, workers real time n/N, joined n/N, `between`, the device thread's FP register as found | every worker real time and joined; else "not the designed set-up" and FAIL |
| callback p50, p99, p99.9, max (µs and % of the period), from 5 s on | p99 ≤ 70% of the period (1,867 µs at 128 frames) |
| dropouts from the first callback: xruns + late + gaps | 0 |
| crashes, non-finite samples | 0 |
| frames played against the engine clock's motion | within one block |
| engine threads: tasks on each, parks, signals, dozes (pool counters) | printed |
| `--sweep`, `--between`: a table of p99, max, dropouts, tasks on workers, per count and mode | recorded |

On a Mac that isn't a base M1 the verdict line adds "stand-in (<model>): provisional for gate 8" (design-8a
Q1); the check still judges its bounds there. On Linux the device lines say "not judged on Linux (PLAT-1)"
(this wording doesn't trip `ci/check.sh`'s PART rule, which looks for "on this Mac"); crashes, non-finite and
frames are judged everywhere.

**Tests** (`crates/gates`):

| Test | Proves | Red by |
|---|---|---|
| `each_cause_fails_the_live_verdict` | made-up records: an xrun, a late callback, a gap, a p99 of 71%, a worker not real time, a worker not joined, a crash, a non-finite sample, frames short: each one alone FAILs and is named | `s4-verdict-gaps` (gaps not counted) |
| `a_clean_record_passes` | | — |
| `no_device_fails_on_a_mac_and_plays_the_clock_on_linux` | the choice, as a pure function of (OS, device found) | `s4-no-device` (a skip on a Mac) |
| `a_slow_block_is_counted_late` | a driven player at 128 frames paced in real time for 2 s, `Stall(10 ms)` tripped once: the record has `late ≥ 1` and the verdict FAILs on it | `s4-late-ms` |
| `live_options_are_read_and_nonsense_refused` | `--minutes 0`, `--frames 0`, `--between x`, `--threads 65` refused in plain words | `s4-parse` |
| `the_help_says_what_realtime_measures_live` | | — |

### 4.3 `ci/gates.txt` and `ci/check.sh`

```text
# 8a's live half (design-8a §9.2): G12, p50 on the live engine threads at 128 frames for one minute,
# muted (the recorded 10-minute run is the owner's own, by hand). On Linux, the silent clock: its
# timings are recorded (PLAT-1); crashes and frames are judged everywhere.
realtime --reference 50 --minutes 1
```

| `ci/check.sh` | Change |
|---|---|
| gates row | its estimate "22 min" → "24 min"; the summary's grep already keeps lines with "p99" |
| header comment | "never plays sound": still true (G12 is muted) |
| "pool models" row | its comment names the caller's `Wake` model; time about 10 s |
| nothing else | the new tests are in the "rust tests" row; `no_alloc_workers` is its own binary there |

**The Mac's check grows by about 2 minutes:** p50's compile and build (seconds), 1 minute playing, opening
and closing the device. On Linux `ci/check.sh --gates` grows by the same.

### 4.4 Cut line and what is left

Build in this order; what doesn't fit goes to "Left", in this order, in the build notes:

| Order | Piece | If it doesn't fit |
|---|---|---|
| 1 | the record, the engine-alone run, its verdict and tests, `ci/gates.txt` | must fit |
| 2 | `--sweep`, `--between`, `--frames`, `--threads`, `--song` | must fit: the owner's run needs them |
| 3 | `--capture resample`: 10 s of resample kept inside `fill` on the device's own thread, against its export (RENDER-33, RENDER-2a on CoreAudio's thread). Needs a capture buffer handed in and out by message | left; `the_callback_mixes_in_the_one_mode` and the set-up line's FP register stand in |
| 4 | `--server` (the app's path, edited every 10 s, judged), `--export` (beside an export), `--like-m1` | left, with design-8a §9.2's words as their spec |

### 4.5 Contract edits

| Rule | The edit |
|---|---|
| Gate 8's row | design-8a §12's words (dropouts from the first callback: overloads, late callbacks, gaps over 1.5 periods; every worker joined) plus "on a Mac other than a base M1 the verdict is provisional". *8a's live half:* "live playback renders on engine threads with the same bits on 1 and N threads, on Linux; G12's live run is built: on Linux it plays the silent clock (information); the Mac's run is owed." If `--server`, `--export` and `--like-m1` are left, the row says so |
| `docs/song-as-program.md` line 256 | "Parallel (Not built yet)" → what is built: partitions per track, a pool, the same bits on any count, the live count rule |
| `design-8a.md` | an "As built (live half)" note: the `Wake` only for the caller (workers park); plugins on one thread; the pool made after the stream starts and handed over as a message; `rest()` after each callback, no `again`; no `Player::close` (`serve` already stops the stream and now drops the pool); what of §9.2 was left |

### 4.6 Commits

| # | Subject |
|---|---|
| 8 | "The player keeps a record of every callback's time" (`timing.rs`, the hooks, its tests) |
| 9 | "m2-gates realtime plays live: G12 on the device, or the silent clock here" (`live.rs`, options, verdict, tests, `start_device`, `silent_with`, `mute_output`) |
| 10 | "G12 in the check: one minute of p50 live, muted" (`ci/gates.txt`, `ci/check.sh`'s estimate) |
| 11 | (if built) "G12 --capture: resample on the device's thread against its export" |
| 12 | merge `8a-gaps` |
| 13 | "Contract: gate 8's live words; build notes: slice 4 and the Mac run" |
| 14 | after the final check: "HANDOFF: 8a's live half" |

---

## 5. The gaps lane (worktree `$SCRATCH/wt/8a-g`, branch `8a-gaps`)

### In short

- **A kernel that crashes while the song plays** now stops the whole song with no message
  (`engine.rs`'s render: `crashes += 1`, the transport stopped; the renderer `broken` for good; only tests
  read `crashes`). After this lane, only that sound goes silent, the rest plays on, and the app says which
  sound on which track stopped, and that the next code change or Undo starts it again.
- **A kernel over its 32 MiB** says it ran out of memory, naming the track, not "wasm trap".
- **`pl-mix`** waits for a hat to sound before it judges the meter past the fader.

### 5.1 Setting up

Branch from `8a` once `main` is merged in (§0). Its own target and scratch HOME. Its notes go in a file of its
own, `docs/designs/step8/build-notes-8a-gaps.md` (so the two lanes never edit one notes file); the track adds
one row to `build-notes-8a.md`'s "In short" at the merge. It merges `8a` into itself only if the lead asks.

### 5.2 Gap 1: a crash silences one sound

| Where | Change |
|---|---|
| `core/runtime/src/lib.rs`, `run_part` | live (`!b.strict`): when `step` returns an error for a top-level node, the node **stops**: its outputs are zeroed, it stays poisoned, its stop is recorded, the error is dropped (it allocated; this path only, as today), and the partition goes on. Nothing downstream is skipped. Strict (exports, `m2 render`): unchanged, the render fails naming the node |
| `KernelNode`, `PolyNode` | a `stopped: bool`: a stopped node outputs silence and calls nothing (calling a trapped kernel again each block would allocate an error each block) |
| `step` | a stopped node stays poisoned (`*poisoned = ... \|\| stopped`), so the next activation resets its box as it moves it (RENDER-25) and the new plan's node starts not stopped |
| new `core/runtime/src/stopped.rs` | `pub struct Stopped { nodes: Box<[AtomicU8]>, any: AtomicU32 }`, one per plan, sized when prepared: 0 playing, 1 crashed, 2 out of memory, by plan node. Ghosts, fades and overlays (index ≥ n) are silenced, not recorded: they leave with the window |
| `plan.rs` | `Plan` holds `Arc<Stopped>`, passed to `run_part` through `run_graph`'s arguments; `PlanInfo.stopped: Arc<Stopped>` for the server (like `taps`) |
| `Renderer` | `broken` and `broken()` go (no live error reaches `post` now); `pre` loses its check |
| `engine.rs`, the render's `Err` arm | can't happen live any more: count it and play that block silent; no transport stop |
| `crates/server/src/graph/actor.rs` | one line beside `heard_plugins()` in the actor's loop: `heard_stops()` |
| new `crates/server/src/graph/stopped.rs` | `heard_stops`: for the playing plan, when `any` moved, each newly stopped node once (per plan id and node): a notice. `sound_named(rev, node)`: a kernel by its id's name (`std:delay` → "The delay"), a `poly` as "The instrument", else its node id; the track by its label, as `plugin_named` does |

Words (plain; the lane may tune them, keeping "In short" style):

- "The delay on Bass stopped working, so music2 made it silent. The rest of the song plays on. Your next change to
  the code, or Undo, starts it again."
- "The delay on Bass ran out of memory (a sound may use up to 32 MB), so music2 made it silent. The rest of the
  song plays on. Your next change to the code, or Undo, starts it again."

No automatic restart, unlike a plugin (HOST-5): a kernel's trap is deterministic, so the same input would trap
again. The plugin path stays the model for the rest: an atomic mark on the audio thread, the actor's poll, one
notice.

### 5.3 Gap 2: out of memory says "memory"

| Where | Change |
|---|---|
| `KernelInst` (`lib.rs`) | the store's data becomes `Cap { limits: StoreLimits, refused: bool }`, implementing `wasmtime::ResourceLimiter`: a grow past 32 MiB sets `refused` and fails as a trap at once (wasmtime's documented "trap on grow failure" behaviour), so a kernel never runs on after a refused grow |
| `KernelInst::process`, `reset`, `prewarm` | on an error with `refused` set: a typed `OutOfMemory { node }` error: "bass/delay ran out of memory: a sound may use up to 32 MB" |
| `run_part` (gap 1's path) | kind 2 when the error is `OutOfMemory` (`downcast_ref`), else 1 |
| the build | a kernel that runs out in its init or first reset fails the build with the same words; the server's `unbuilt` message carries them as its detail |

### 5.4 Gap 3: `pl-mix` made sturdy

`e2e/pl-mix.mjs`, the check after the fader goes to −30 dB (now: 10 samples, 150 ms apart, one must read above
−24 dB; first-song's hats are short hits, so all ten can land between them): sample until one reads above
−24 dB, for up to 6 s (40 samples), and every sample read must still match the header's within 1.5 dB. The same
change for the check before the fader. Only this file.

### 5.5 Tests (each shown red first)

| Test | Where | Proves | Red by |
|---|---|---|---|
| `a_trapped_kernel_goes_silent_alone` (replaces `a_trapped_kernel_is_reset_at_the_next_activation`) | `core/runtime/src/tests/live/keep.rs` | `t:trap` in track a, a sine in track b; after the trap: a's node outputs 0, b's output equals a run with no trap bit for bit, the transport plays, `Stopped` marks the node 1, `crashes()` 0 | today's code (the song stops) |
| `a_stopped_kernel_starts_again_at_the_next_activation` | same | an activation resets it as it moves it; it plays again; the new plan's mark is 0 | `g-stop-unpoison` (a stopped node's poison cleared: not reset) |
| `a_trap_on_any_thread_count_gives_the_same_bits` | same | `Engine::process_on` on 1 and 4 threads with spread, through the trap | — |
| `a_trap_still_fails_an_export_and_names_the_node` | `core/runtime/src/tests.rs` or `tests/parts.rs` | strict: the error names the node, as `strict_mode_names_the_kernel_that_broke` | `g-strict-swallow` |
| `a_kernel_over_its_memory_says_it_ran_out` | `keep.rs` (`t:hog`: its process grows its memory by 600 pages, then traps on −1) | strict: the message says "ran out of memory", the node and "32 MB", not "wasm trap"; live: the mark is 2, the rest plays | today's code |
| `a_kernel_that_stops_is_told_once_by_sound_and_track` | `crates/server/src/graph/stopped_tests.rs` (new) | an open song; the test marks a node of the playing plan's `Stopped` (as the audio thread would); the next job's notices hold the words once; a second poll adds none; a new plan can tell again | no `heard_stops` |
| `out_of_memory_is_told_as_memory` | same | kind 2's words | `g-kind` |
| `a_plugin_that_fails_live_goes_silent_alone` (existing) | `keep/plugins.rs` | unchanged but `!cur.broken()` goes | — |
| `pl-mix` | `e2e/pl-mix.mjs` | 10 runs of 10 pass (logged); the new check still fails when the strip meter reads past the fader (a one-line change in the strip's meter source, shown red) | `g-meter-post` |

### 5.6 Files the gaps lane may touch, and must not

| May touch | Must not touch (slice 3 and R1 own them) |
|---|---|
| `core/runtime/src/lib.rs`: `KernelInst` (new, process, reset, prewarm, the store type), `KernelNode::process`, `PolyNode::process`, `step`'s poison line, `run_part`, `run_graph`'s arguments, `Renderer`'s `broken`, `pre`, `post`, `info()` | `labels()`, `mod fpenv` (the top of `lib.rs`), `render_block_on`'s runner lines |
| `core/runtime/src/plan.rs`, new `core/runtime/src/stopped.rs`, `engine.rs` (the render's `Err` arm only) | `parts.rs`, `view.rs`, `threads.rs`, `fpenv.rs`, `gauge.rs` |
| `core/runtime/src/tests/live/keep.rs`, `keep/plugins.rs`, `tests.rs` | `tests/parts.rs` |
| `crates/server/src/graph/actor.rs` (one line), new `graph/stopped.rs`, `graph/stopped_tests.rs` | `crates/server/src/export.rs`, `graph.rs` |
| `e2e/pl-mix.mjs` | `core/play`, `core/pool`, `core/os`, `crates/gates`, `ci/` |
| `docs/architecture.md`: RENDER-25's last sentence and *Today*, a HOST-5-like sentence for kernels there; `docs/gaps.md`; `build-notes-8a-gaps.md` | RENDER-2, 2a, 28, 28a, HOST-2, gate 8, Deferred; `HANDOFF.md` (the track edits it at the end, and takes the two gap lines out of "Left for later") |

### 5.7 Commits, and the merge

| # | Subject |
|---|---|
| G1 | "A kernel that crashes while the song plays goes silent alone" |
| G2 | "A kernel over its 32 MB says it ran out of memory" |
| G3 | "The app says which sound stopped, on which track, and what brings it back" |
| G4 | "pl-mix waits for a hat before it reads the meter past the fader" |
| G5 | "Contract and gaps.md: a sound that stops; build notes for the gaps lane" |

Each green by `g-check.sh` (runtime, play, server tests; renders 10 of 10; clippy; the Mac lint). The track
merges `8a-gaps` into `8a` after commit 11, before the final check. Expected conflicts: none in code; if
`docs/architecture.md` meets, the paragraphs differ.

Contract words (G5): RENDER-25 gains "Live, a node that fails (a kernel's trap, a voice's, a kernel that asks
for more than its memory) goes silent alone and stays silent until the next activation, which resets it; the
rest of the plan plays on, and the app names the sound and its track. Offline the render fails, naming the node."

---

## 6. Checks and the final check

| When | What |
|---|---|
| each commit | `s2-check.sh` with the crates touched (`m2-pool m2-os m2-runtime m2-play m2-editor m2-gates` as fits); the pool's models row's two commands after commit 4 |
| R1, around commit 1 | the offline gate held on CCD1, before R1 (the merged base's release build, made before R1's edit) and after, `uptime` first: `cargo build --locked --release -p m2-gates`, then `TW_WAIT=1800 $SCRATCH/tw 8a-r1 'uptime; cd $SCRATCH/wt/8a && HOME=$SCRATCH/8a-scratch/home M2_AUDIO=none taskset -c 8-15 target/release/m2-gates realtime --offline --reference 50 --sweep 1,4,8'` |
| slice 4, after commit 10 | G12 on the silent clock, information: `realtime --reference 50 --minutes 1 --sweep 1,2,4 --between park,spin` held on CCD1, `uptime` first |
| the end | `TW_WAIT=3600 $SCRATCH/tw check-8a-live 'cd $SCRATCH/wt/8a && M2_CHECK_OUT=$SCRATCH/check-8a-live ci/check.sh --gates'`: every row, nothing passed only on a retry. Then build notes "The final check", HANDOFF, delete the gaps worktree and its target, `df -h /` |

---

## 7. The Mac command for the owner

**In short:** once this track is on `main`, about 35 minutes. Plugged in, lid open, other apps closed, in
Terminal on the Mac itself. It plays nothing aloud. Paste back `~/m2-8a.txt`.

| # | Do this | Why |
|---|---|---|
| 1 | Type the first line and press Return | gets the new code |
| 2 | Each next line, one at a time, waiting for each to finish | each measures one thing (table below) |
| 3 | Paste back what is in `~/m2-8a.txt` | that is all the lead needs |

```sh
cd <repo> && git fetch && git switch main && git pull
cargo test --locked --release -p m2-pool --features models 2>&1 | tail -5 | tee ~/m2-8a.txt
sysctl hw.model hw.perflevel0 hw.perflevel1 | tee -a ~/m2-8a.txt
cargo run --locked --release -p m2-gates -- realtime --offline --reference 50 2>&1 | tee -a ~/m2-8a.txt
cargo run --locked --release -p m2-gates -- realtime --reference 50 --minutes 1 --sweep 1,2,3,4 --between park,spin 2>&1 | tee -a ~/m2-8a.txt
cargo run --locked --release -p m2-gates -- realtime --reference 50 --minutes 10 2>&1 | tee -a ~/m2-8a.txt
```

| Line | About | What it decides |
|---|---|---|
| models | 1 min | on arm64, the pool's orderings and the caller's `Wake` (M5 can only fail there). A FAIL: stop; a bug, fixed on Linux first |
| `sysctl` | seconds | `hw.perflevel0.cpusperl2` is the live count's cap. Not 4: the count rule's table says what the Mac gets; the build notes check it against the gate's set-up line |
| offline sweep | 13 min with the build | S = 1-thread p99 ÷ 4-thread p99. S ≥ 2.5: keep the live default. 2.0 to 2.5: R1 is already in; the default stays and the owner is told the base M1 is at risk. S < 2.0: the live default becomes 1 (one line in `live_count`) and a study follows. 8 threads beat 4 by 10% or more: an export alone uses every P-core on a Mac. A partition's p99 over 667 µs: voices across threads. The executor's own cost over 40 µs a block: trimmed before G7f's next run. Paced 1-thread p99 more than 15% above back to back: it points at spinning (with the next line) |
| live sweep | 10 min | the live default count: the fewest threads whose p99 is within 5% of the best, with no dropout. Park or spin: `BETWEEN` becomes `Spin` if spin's p99 is at least 10% lower at that count, with no more dropouts. Every worker joined the workgroup, or "not the designed set-up": then the workgroup code is fixed first |
| 10-minute run | 11 min | gate 8 on the stand-in: no dropout from the first callback, p99 ≤ 1,867 µs, no crash, every worker joined. On an M1 Pro the verdict is provisional (design-8a Q1); a FAIL: slice 4's study list (`--server`'s edits, the dropper's `munmap`s, QoS) in the order design-8a §16 gives |

The Mac's full `ci/check.sh` is owed as well (it runs the new Mac tests: the Mach `Wake`, the workgroup join,
the time-constraint policy, and G12's one minute). It is a separate run of about 40 minutes.

---

## 8. Open questions, each with the decision to build with

| # | Question | Build with |
|---|---|---|
| Q1 | "A worker that fails to join goes to the `crash()` path": what is a worker that never joins? | Three cases. A task's panic: `crash()` (built, tested). A worker that never comes to a block: not waited for; the block finishes on the threads that came (tested). A worker stuck inside a task: it can't go to `crash()`, since `crash()` retires the plan while the stuck task still reads it and the device thread's stack; so the device thread dozes on its `Wake` and the stall path (1 s with no callback) replaces the player, as today (tested up to the doze) |
| Q2 | The `Wake` inside `m2-pool`: a dependency on `m2-os`, or a trait injected through `Setup`? | A plain dependency: one concrete type, no trait object, no orphan-rule newtype. `m2-os` needs only libc. The models wrap it in the chaos shim |
| Q3 | Workers on the `Wake` too, or std's `park`? | `park`: std starts them, so their handles never go stale, and it needs no OS code of ours. Only the caller, which may be a thread std never started, uses the `Wake`. If the Mac's live sweep shows workers waking late, a `Wake` per worker is the next step (refresh R14) |
| Q4 | When is the workgroup read, and how does the callback get its runner? | After `stream.play()`, on the `m2-play` thread; the pool is made then and its runner sent as `Msg::Threads`, applied at the start of a callback. The first callbacks run on one thread |
| Q5 | The workgroup advises fewer threads than `per_cache`? | `min(advice, per_cache)`, as design-8a §6.3 |
| Q6 | `M2_THREADS` for live too? | Yes: one variable, "how many threads music2 renders on", read by the player as by renders. The silent player uses 1 unless it is set |
| Q7 | Linux without real time: 1 thread or `per_cache`? | 1 (the refresh's Attack, A.9). Real time as far as a user without root can: `SCHED_FIFO` within `RLIMIT_RTPRIO`; no rtkit, no `RLIMIT_RTTIME` (Deferred) |
| Q8 | Park or spin between callbacks? | Park (`BETWEEN`), one constant; G12 `--between` measures both; §7 says when to switch |
| Q9 | `again` across the four layers, or `rest()`? | `rest()` once at the end of each callback (`after_callback`), as the offline half already does |
| Q10 | `Player::close()` (design-8a §6.5)? | Not needed: `serve` already stops the stream once the player is let go; it now drops the pool after it, which joins the workers (tested) |
| Q11 | Plugins on workers? | No (the lead's rule): a plugin song renders on the device's thread; HOST-2 keeps its words |
| Q12 | `os_workgroup_testcancel` at each fill? | No: cpal's `DeviceChanged` already replaces the player. Deferred with a trigger |
| Q13 | R1's scope: only `std:gain`, or a rule? | A rule: any plan node whose reads, constants aside, all come from one other label. Constants and `Notes` nodes run in no partition |
| Q14 | R1 in an edit window, when a strip reads a fade? | That plan keeps the strip in `song` (speed only, no bits); the next plan moves it |
| Q15 | G12's capture run | Third in the cut order (§4.4); left if it doesn't fit, with the callback FZ test and the FP register line in its place |
| Q16 | G12 `--server`, `--export`, `--like-m1` | Left, named in gate 8's row and the build notes, in that order |
| Q17 | G12 judged on which Mac? | Its bounds on any Mac in the check; "provisional (stand-in)" on any Mac but a base M1, for gate 8's acceptance only. The owner's question Q1 (a base M1) stands |
| Q18 | Where the live record lives (`Player::load()` is taken) | `Player::timing()`, in `timing.rs`; 8b.8's `Load` stays for the status pill |
| Q19 | Engine-thread CPU time per block (design-8a §9.1)? | Not built: the pool's counts (tasks, parks, signals, dozes) only. A clock read per task waits for a measured need |
| Q20 | A crashed kernel: restart on its own, as a plugin does? | No: a trap is deterministic. It stays silent until the next activation (a code change, or Undo), and the app says so once per plan |
| Q21 | Out of memory: let `memory.grow` return −1, or trap? | Trap at once, with our own limiter's flag, so the message is certain whatever the kernel does with −1 |
| Q22 | `Renderer::broken` after gap 1 | Deleted: no live error reaches the post-pass now |
| Q23 | The gaps lane's build notes | Their own file; one row in `build-notes-8a.md` at the merge |
| Q24 | `next` and this track both edit `core/play/src/lib.rs` | This track's code lives in `engines.rs` and `timing.rs`; `lib.rs` gets short hooks. Whoever merges second follows §1's merge note |
| Q25 | The `fpenv` test setter in a public module | `#[doc(hidden)]`, named `flush_for_tests`; the FZ test can't reach a `cfg(test)` item from another crate |

---

## 9. Risks

| # | Risk | What is done |
|---|---|---|
| L1 | Mac-only code (workgroup, Mach semaphore, time-constraint) first runs on the owner's Mac | the Mac lint type-checks it on every commit; its Mac tests run in the owner's check; G12's set-up line names what failed; a refused piece is reported, never fatal |
| L2 | Reading `'oswg'` by UID fails on some device | the pool still runs, workers time-constraint without the group; G12 says "not the designed set-up" |
| L3 | `next` lands first and moves `Player::start` | §1's merge note; slice 4 starts with a merge of `main` |
| L4 | The live N-thread tests make `bit_identity` slow | measure; cut to 10 s on 3 and 4 threads if over 60 s |
| L5 | Spinning pools in one test binary | the existing one-pool-at-a-time lock in each test file; watchdogs 60 s, never a time bound |
| L6 | A lost ring on the stuck path | bounded by `doze` (one period live): a dropout that was already late; the model checks none is lost |
| L7 | Disk | two targets at most; the gaps lane's deleted at the merge; `df -h /` each slice |
| L8 | Linux numbers on a loaded server | information only (PLAT-1); `uptime` and "waited for a CPU" in each log |
| L9 | The gaps lane changes `run_part`, which every render runs | strict renders unchanged and tested; the renders row and every 1-vs-N test run on each commit |

---

## 10. Order and time (agent time, *estimate*)

| When | This track (`wt/8a`) | The gaps lane (`wt/8a-g`) |
|---|---|---|
| Day 1, morning | merge; R1 and its numbers (commits 1, 2); `m2-os` `rt` (commit 3) | G1, G2 |
| Day 1, afternoon | the pool's `Wake` (commit 4); the player on driven and silent engine threads (commit 5) | G3, G4 |
| Day 2, morning | the device (commit 6); contract and notes (commit 7); the record (commit 8) | G5; ready to merge |
| Day 2, afternoon | G12 live (commits 9, 10, maybe 11); merge `8a-gaps`; contract and notes (commit 13); the final check; HANDOFF | — |

---

## 11. Files read

In `$SCRATCH/wt/8a` at `6d4086d`: `docs/designs/step8/build-notes-8a.md`, `8a-plan.md`, `design-8a.md`;
`docs/architecture.md` (RENDER-2, 2a, 25, 28, 32, HOST-2, HOST-5, PLAT-1, PLAT-2, gate 8, Deferred);
`docs/song-as-program.md`; `core/pool/src/{lib,run}.rs`, `tests/no_blocking.rs`; `core/runtime/src/{engine,parts,
threads}.rs`, `lib.rs` (`KernelInst`, `step`, `run_graph`, `run_part`, `labels`, `render_block_on`, `pre`,
`post`), `plan.rs` (`PlanInfo`), `packages.rs` (the engine's config), `gauge.rs`,
`tests/live/keep.rs` (`t_kernels`, the trap test); `core/play/src/{lib,alloc,tests}.rs`, `tests/{bit_identity,
no_alloc,no_blocking}.rs`, `Cargo.toml`; `core/os/src/{lib,thread}.rs`, `mac/mod.rs`, `linux/mod.rs`,
`tests/one_place.rs`, `Cargo.toml`; `crates/gates/src/{main,realtime,gates}.rs`; `crates/server/src/graph.rs`
(`let_go`, `plainly`), `graph/actor.rs` (`heard_plugins`, `plugin_named`, `unbuilt`, `export`); `ci/check.sh`,
`ci/gates.txt`; `e2e/pl-mix.mjs`; `std/src/mix.ts`; `Cargo.lock`. `main`'s `HANDOFF.md` (the two gaps, step
7's "Left for later"). `$SCRATCH/s8a/refresh.md` (§4, §6, §7, its Attack), `$SCRATCH/s8a/s2-check.sh`,
`$SCRATCH/tw`. The `next` track's diff against `main` (`core/play`, `core/runtime`, `core/os`). wasmtime 49.0.2
`runtime/limits.rs`; cpal 0.18.2 `DeviceId`, CoreAudio `Device::id`; libc 0.2.190 (Mach and `sched` symbols).

---

## Attack

Written 2026-10-17, read-only. Checked against `main` at `8a6d9e1`. `next` at `1560a19` was read through
`git diff main...next`, run in the main repo, never in `wt/nx`. Nothing was built or run. Load while reading: 16.85
(`uptime`), for the record only.

### In short

- **No bit changes found.** Rule A, rule B, `Canon` in `fill` and the gaps lane change no export and no 1-vs-N
  result (A.4).
- **Seven things must change before building:**
  1. R1's rule A only works inside `Schedule::of`, and it turns an existing test helper red (A1, A2).
  2. A worker refused real time stays in the pool as a plain thread. Linux's real-time guess is wrong for root and
     for cgroups. Then the device thread can wait on a normal thread (A8).
  3. G12 counts start-up blocks no one can hear. It reads two counters that can be a callback apart. It doesn't
     check the device's rate or buffer. So the Mac check would fail or flake (A14, A15, A.6).
  4. On Linux, G12 would open ALSA on this server. It must play the silent clock (A13).
  5. One player test is timed by a 30 ms sleep, and the `Wake` can end early on a signal. Both would flake on this
     server (A.6, A12).
  6. The gaps lane's "ran out of memory" trips the server's own out-of-memory matcher, so the user is told to close
     other apps (A6).
  7. The Mac command contradicts itself about the live count, and it is five long lines. Make it one script (A7,
     A.10).
- **The next track:** three of the plan's edits meet `next`'s. Two of them can be avoided (A.8).
- **Checked and fine:** the device thread never waits on a parked worker. A pool dropped under a running callback is
  safe. Plugins stay on one thread. The handover keeps the queue's order. No `Player::close()` is needed (A.3, A.7).

### A.1 Wrong, or won't build as written

| # | What | Where | The problem |
|---|---|---|---|
| A1 | Rule A "in `Schedule::build` and `Schedule::of`" | `core/runtime/src/parts.rs:144` | Only `of` works. `of` does `groups[p as usize]` for every node of the order, so a `NONE` from `build` panics there. `Schedule::cut` (tests, the gauge's one partition) also goes through `of` and gives constants a partition. If `of` then makes no edge from a constant, the debug `check` fails ("reads across partitions with no edge") |
| A2 | "Existing tests: unchanged" | `core/runtime/src/tests/parts.rs:69-84` | `whole()` asserts that every node of the order is in exactly one partition. Rule A puts every `Const` and `Notes` node in none. `a_track_is_one_partition` (each `sine_track` holds two constants) and every other caller of `whole` go red |
| A3 | `timing: Record` with `hist: [AtomicU32; 2048]` inside `Shared` | `core/play/src/lib.rs:219` | `Shared` derives `Default`. Arrays longer than 32 have no `Default`, so it doesn't compile until `Record` has its own |
| A4 | `Gone::Runner(Box<Handover>)` with `Handover` `pub(crate)` | `core/play/src/lib.rs:116` | `Gone` is `pub`. A private type in a public enum is refused, or warned, and `-D warnings` fails |
| A5 | Rule B, then "fades are labelled again from `w.rewire`" | `labels()` | It does nothing. A node that reads a fade gave that fade its own label, so rule B never moves it |
| A6 | Gaps lane: `OutOfMemory` says "ran out of memory" | `crates/server/src/graph.rs:429-441`, `graph/actor.rs:418-430` | `out_of_memory` matches "out of memory" anywhere in an error's chain. A kernel over 32 MB would then get `unbuilt`'s words: "the computer ran out of memory… Close other apps or songs; music2 keeps trying". An export gets `plainly`'s. That is wrong advice. And `graph.rs` is on the lane's must-not-touch list |
| A7 | §7: "S < 2.0: the live default becomes 1" | §7 | It contradicts §7's live-sweep line, which picks the live count from live runs. An S of 1.8 on 4 threads still beats 1 thread |
| A8 | `Group::real_time()` is `RLIMIT_RTPRIO > 0`; a refused worker "runs on as a plain thread" | §3.1, §3.5 | That is a guess. Root and `CAP_SYS_NICE` get FIFO with a limit of 0. A cgroup with no real-time budget refuses FIFO with a limit above 0. And a worker refused at its start stays in a real-time pool. The device thread then waits on that worker's task, or on `inside`, while normal threads preempt it: priority inversion. Refresh A.9 asked for 1 thread in that case |
| A9 | The period: `'fsiz'` on a Mac, "from the frames if known" on Linux | §3.1 | Linux doesn't know the frames with `BufferSize::Default`. With `next`, `outputs::buffer` falls back to the device's own size without a word. Two ways to get one number, and one of them may be wrong |
| A10 | `thread_policy_set(mach_thread_self(), …)` | §3.1 | `mach_thread_self()` makes a new port right on each call, which is never freed. libc 0.2.190 has `pthread_mach_thread_np(pthread_self())`, which doesn't |
| A11 | What a `Wake` means | §3.1, §3.2 | A Mach semaphore counts rings; the futex word holds one. And after a doze that timed out, the caller stores `asleep = false` without taking a ring that a ringer may have sent. The next doze then wakes at once, and `rung` counts a ring that wasn't its own. No bit changes, but the two OSes differ and the counters lie |
| A12 | `wait(at_most)`: "false when the time ran out" | §3.1 | `FUTEX_WAIT` returns early on a signal (`EINTR`), and `semaphore_timedwait` on `KERN_ABORTED`. As written, that reads as a timeout. The model `the_caller_is_woken` asserts `timeouts == 0` |
| A13 | G12 on Linux opens the device unless none is found | §4.2 | On this server `default_output_device()` finds one and `default_output_config()` fails (refresh A.9). And the rule here is `M2_AUDIO=none` for anything that runs music2 |
| A14 | G12 doesn't set or check the rate | §4.2 | `Player::start` opens at the device's own rate (`core/play/src/lib.rs:784`; `next`'s `open_on` too). 1,867 µs is 70% of 128 frames at 48 kHz. At 44.1 kHz, with a 48 kHz song, the resampler makes some 128-frame callbacks render two blocks |
| A15 | G12 asks for 128 frames | §4.2 | With `next`, a device that can't take 128 frames plays at its own size, without a word. The verdict would judge another buffer |
| A16 | `Runner::trip(task, …)` "on a root, so a worker runs it"; "a panic hook saw it on a worker" | §3.2, §3.6 | Nothing gives a test the task number of a root. A panic hook is one for the whole test binary, whose other tests run at the same time |
| A17 | `a_slow_block_is_counted_late`: "the verdict FAILs" | §4.2 | On Linux the timing lines aren't judged. The verdict can't fail there unless "judged" is an argument |
| A18 | `the_wake_never_blocks` greps `mac/rt.rs` and `linux/rt.rs` | §3.1 | Those files also hold `Group::of_output` (a `CFString`: an allocation) and the token's `thread_local!`. Red from the start |
| A19 | `thread::foreign(f)` | §3.1 | A failed assert inside `f` unwinds across the `extern "C"` start routine. The whole test binary aborts, not one test |
| A20 | The no-alloc sequence lists "a panic" | §3.6 | It means `Player::panic()` (every note off). A Rust panic allocates by design. A builder may read it the other way |

### A.2 Not green alone

| Commit | Problem | Fix |
|---|---|---|
| 1 (R1) | A1 and A2: a panic and red tests | D1, D2 in the same commit |
| 4 (pool) | `Setup` and `Counters` struct literals (`core/pool/src/tests.rs:435`, `lib.rs`) gain fields | fixed in the commit, as expected |
| 6 (device) | Nothing on Linux runs the new lines in `start()`. The drop order after `serve` is untested | D9: the fake `Stream` tests reach it |
| 6 (device) | The period needs the first callback's frames (D8). Today nothing records them before slice 4 | D8: the record (commit 8) moves before commit 6 |
| G2 (gaps) | A6 needs `graph.rs` | D24 |

### A.3 The audio thread and the engine threads

| Item | Verdict |
|---|---|
| Does the device thread ever wait on a parked worker? | **No.** `together` waits only on `inside` and `left`. A worker parks only outside a run (`sleep` comes before `inside + 1`), and holds no task then |
| A pool dropped while a callback still runs (CoreAudio's monitor thread may drop the stream late) | **Safe.** The run in flight finishes, since workers leave only between runs. `live()` then sees `stop`, and later callbacks run alone |
| The handover's order | **Fine.** It is sent before `ready`, so before any project. It would also work later: a held message applies between runs |
| Plugins on one thread | **Fine.** `render_block_on` swaps in `Runner::one()` and never wakes a worker |
| A worker refused real time inside a real-time pool | **Priority inversion** (A8). D4 |
| Linux: the workers and the device thread at one FIFO priority | FIFO threads of equal priority don't preempt each other. A worker spinning for `patience` holds a core the device thread may want. D7 |
| Two `late` counts | 8b.8's `Shared.late` and the record's `late` count the same thing in two places, measured from two clock reads. They will disagree. D13 |
| Spin on a Mac | A time-constraint worker spinning for 1.25 periods runs past its `computation` (half a period). macOS may demote it. The sweep measures it; §7 should say so (D25) |
| `on_device_thread` "once, on its first fill" | It runs on the fill that takes the handover, not on the first fill. RENDER-28's words (D25) |
| The hold's fade and `next`'s click | Both mix outside `render_block_on`'s `Canon` (`engine.rs`'s `render`, `next`'s `click.mix`). `Canon` in `fill` covers them. RENDER-2a should name them (D25) |

### A.4 Bits

No way found to change a bit:
- Rule A: `Const` and `Notes` do nothing in `step` (`core/runtime/src/lib.rs:1216`). Constants are written only by
  `Cells::apply`, in the pre-pass. A `Notes` buffer is empty; a `poly` reads its lane.
- Rule B: labels feed only `Schedule::build`. The names in messages come from the flat graph's `module`
  (`graph/actor.rs:412`), not from labels.
- `Canon` in `fill` changes only the player's own mixing, never an export.
- Gap 2 traps at once only past 32 MiB. No std kernel comes near (HANDOFF).

### A.5 Tests or gates that can't fail

| Test or gate | Why | What to do |
|---|---|---|
| `a_group_without_a_device_has_no_workgroup_and_says_so`, `a_wake_works_on_threads_std_did_not_start` | no red run | keep as smoke tests; say so in the notes |
| `an_engine_thread_gets_fifo_or_says_why` | the granted branch never runs here (`ulimit -Hr` is 0) | the refusal branch is red by `os-fifo-claims`; the notes say the granted branch is a desktop's |
| `engine_threads_never_allocate` with `Group::none` | never runs a workgroup join under the trap | fine: set-up comes before `mark(true)` |
| G12's frames line | tight, it flakes (A.6); loose, it catches nothing | D18 |
| G12's verdict on Linux | not judged | D17 |

### A.6 Flake risks on this server

| Where | Risk | Fix |
|---|---|---|
| `a_stuck_task_puts_the_device_thread_to_sleep_on_its_wake` | A 30 ms `Stall`. Under load 16 the device thread may not run for 30 ms, find the task already done, and never doze: `dozes == 0` | D15: a latch, as the pool's own test has |
| model `the_caller_is_woken` | `EINTR` counted as a timeout (A12) | D5 |
| G12's frames against the engine clock | Two atomics read while the callback runs can be one callback apart. At 128 frames that is the whole "within one block" | D18 |
| the panic hook in `engine_threads.rs` | one hook for the binary; other tests' panics reach it | D15 |
| `threads::live_workers()` | one count for the process | every test in that file holds the one-pool lock, and polls with a 60 s watchdog |
| G12 "from the first callback" on a Mac | The first callbacks run on one thread until the handover. Then the project's first blocks touch thousands of new pages. The song is stopped then, so no one hears it, but it is counted. A FAIL in every check is likely | D16 |

### A.7 Close and device changes

| Case | Verdict |
|---|---|
| No `Player::close()` (Q10) | **Right.** `serve` already stops the stream once every clone is let go (`core/play/src/lib.rs:709-732`) |
| `drop(pool)` after `serve` returns | `next` sets `freed` inside `serve`, after `drop(stream)`. The device reads as free while the old workers still live, still in the old workgroup, and a new player's pool starts beside them. D9 puts the pool's join before `freed` |
| A worker stuck for good in a task | As today: the stream's drop never returns, since CoreAudio waits for the callback. Only that player's threads are lost |
| cpal reroutes the output | The workers stay in a cancelled workgroup until the server replaces the player (DeviceChanged). A pool built just after the reroute gets `EINVAL` from `os_workgroup_join`. D21 |
| Waiting for the first callback (D8) and for the workers' reports (D4) | bounded: 1 s each, then one thread, said |

### A.8 The next track

| Piece | `next` | This plan | Clash | How to avoid it |
|---|---|---|---|---|
| `render`'s destructure line | adds `shared` | adds `runner` | one line | accept |
| `Shared` | 4 fields in the middle (`lib.rs:231-266`) | `timing` | none, if `timing` goes last | D12 |
| `driven()` | adds a line (`lib.rs:838`) | rewrites it to call `driven_on` | yes | D10 |
| `serve` | `freed` after `drop(stream)` | `drop(pool)` after `serve` returns | the order (A.7) | D9: `serve` doesn't change |
| `start()`, `silent()` | rewritten (`start_in`, `open_on`, `silent_as`) | three lines, `silent_with` | yes | accept; §1's merge note |
| `fill`, `take_held`, `leftover`, `Msg`, `Gone`, `Callback` | untouched | the hooks | none | — |
| `Cargo.lock` | `m2-os`'s entry (`midir`) | `m2-pool`'s entry | none | keep the Mac code to `extern "C"` blocks. Adding `objc2-core-audio` or `mach2` to `m2-os` would meet `next`'s entry |
| `core/os/src/lib.rs` | `midi` between `main_loop` and `process` | `rt` between `process` and `thread` | none (a line apart) | — |
| `core/runtime/src/lib.rs` | `pub mod click` (line 40) | `pub mod fpenv` (line 44) | none | — |
| `crates/server/src/graph/actor.rs` | edits `build`, just below `unbuilt` | (gaps lane) | none, if the lane fixes A6 in `graph.rs` | D24 |
| OS-61 (no device: play silently, say so once) | "opening without an audio device" | not mentioned | — | §1 says `next` owns it (D26) |

### A.9 Mac-only code and the Mac lint

The mac lint row (`--target aarch64-apple-darwin --workspace --all-targets`) type-checks every
`cfg(target_os = "macos")` item and test. It can't check:
- that a framework links, or that an `extern "C"` signature matches its header;
- a FourCC typed by hand (`'oswg'`, `'uidd'`);
- the Mach tick arithmetic.

The pool models' build (`--features models`) isn't in the mac lint, but nothing in it is Mac-only: fine.
`CFStringCreateWithBytes` and `CFRelease` are already declared in `mac/watch.rs`. A second declaration with another
type alias trips `clashing_extern_declarations` under `-D warnings`. D22.

### A.10 The Mac command

| Problem | Fix |
|---|---|
| Five long lines to type, for a dyslexic owner | one script, `ci/sweep.sh`; two lines to type (pull, run) |
| The real `HOME` | the script gives music2 a scratch `HOME`, as `ci/check.sh` does |
| The output device | built-in speakers at 48 kHz (Audio MIDI Setup), not Bluetooth: A14, A15 |
| The S rule | A7 |
| HANDOFF's owed offline sweep (`~/m2-8a-offline.txt`) | this run replaces it: one run, not two |

### A.11 The gaps lane

| Item | Verdict |
|---|---|
| In scope? | Yes: HANDOFF's step 6 gives the two gaps to this track. The harness brief doesn't name it, so it is cut first if day 2 runs short |
| A6 | must fix (D24) |
| HANDOFF says "rebuild it (HOST-5's path)"; Q20 decides no restart | fine: a trap is deterministic. HANDOFF's gap words follow Q20 |
| `KernelInst`'s store type and `run_part` | strict renders unchanged; the renders row and every 1-vs-N test run on each commit |

---

## Decisions

| # | For | Build with |
|---|---|---|
| D1 | A1 | Rule A lives in `Schedule::of`. Whatever `part_of` it is given, a `Const` or `Notes` node gets `NONE`, joins no group and makes no edge. So `build` and `cut` both get it |
| D2 | A2 | Commit 1 changes `whole`: every node of the order that runs is in exactly one partition; `Const` and `Notes` are in none. `a_track_is_one_partition` checks "the label is the node's module, or the one other module all its inputs come from". Show the new `whole` red once (a running node left out) |
| D3 | A5 | Rule B without the fade re-labelling. Reads of constants, of `Notes` nodes and of nodes outside the order are set aside. A debug assert: no node that reads a fade moved |
| D4 | A8 | The workers' own reports decide, not a guess. On a device, the `m2-play` thread builds the pool at the rule's count, waits (at most 1 s) until every worker's `start` has reported, and keeps the pool only if every worker is real time. Else it drops the pool, plays on one thread, and `EngineThreads.missing` says why. `Group` has no `real_time()`. The count is one pure function with no OS word in `m2-play` (PLAT-2): `live_count(asked, cores, recommends, on_device)` = `asked`; else 1 off a device; else `min(recommends or per_cache, per_cache)`. Its test needs no made-up `Group` |
| D5 | A12 | `Wake::wait` loops until it is rung, or the deadline has really passed (`EINTR`, `KERN_ABORTED`, a spurious wake) |
| D6 | A11 | One meaning on both OSes: at most one ring is ever waiting. The pool keeps it so. After a doze that timed out, the caller takes its flag back with `swap(false)`. If a ringer took it first, the caller waits once more (at most `doze`) for that ring. Then `rung + timeouts == dozes` exactly, and the model asserts it |
| D7 | A.3 | Linux: the device thread at `min(RLIMIT_RTPRIO, 10)`; workers one lower when the limit is 2 or more, else equal |
| D8 | A9, A.2 | One period rule on both OSes. The `m2-play` thread waits (at most 1 s) for the first callback, and takes that callback's frames at the device's rate as the period. Then it reads the workgroup (so after the IO started) and builds the pool. No `'fsiz'`. No callback within 1 s: one thread, said. The record (commit 8) moves before commit 6 and keeps the last callback's frames; when `next` lands, its `Shared.frames` replaces that field |
| D9 | A.7, A.2 | `serve((stream, engines), shared, alive)`: a tuple drops the stream first, then the pool, and `next`'s `freed` comes after both. `serve` itself doesn't change. Extend `a_device_player_let_go_stops_its_stream_and_frees_its_engine_elsewhere` (the fake `Stream`, `core/play/src/tests.rs:531`) with a pool: after the let go, `live_workers()` is back where it was |
| D10 | A.8 | `driven_on(rate, runner)` calls `driven(rate)` and puts the runner in its callback. `driven` stays as it is |
| D11 | A10 | `pthread_mach_thread_np(pthread_self())` |
| D12 | A3, A.8 | `timing: Record` is the last field of `Shared`. `Record` has its own `Default` (`std::array::from_fn`) |
| D13 | A.3 | One `late`: G12 reads 8b.8's `Shared.late` through `Load`. The record adds only the histogram, gaps, frames, the device's FP register and non-finite samples |
| D14 | A4 | `Handover` is `pub` and `#[doc(hidden)]`, its fields private |
| D15 | A16, A.6 | `Runner::trip(run, Trip)` with `enum Trip { Panic, Hold(Arc<AtomicBool>) }`. In that run, the first task a worker takes trips; with no workers, the caller's first. `Hold` sleeps in 1 ms steps until the flag is set. `Runner::tripped_on()` names the thread that took it. No panic hook. The stuck-task test opens the latch only after it sees `dozes ≥ 1`; the late-callback tests open it from a helper thread after 10 ms |
| D16 | A.6 | G12 opens the player and waits until the callback has taken its engine threads. It opens p50, lets it sit stopped for 2 s, then sends Play. Dropouts count from the first callback after Play is applied, which is what a producer hears. The callbacks before are printed as information. p99 counts from 5 s after Play |
| D17 | A17, A13 | The verdict is a pure function of (record, set-up, judged). Linux passes `judged = false`; the tests pass `true`. A Mac is told from `machine()`'s OS part, as `paired::Place::of` does: no `cfg` in `crates/gates`. On Linux G12 always plays the silent clock and never opens a device. On a Mac, no device is a FAIL |
| D18 | A.6 | Frames: between two records both taken while playing, the device frames × (engine rate ÷ device rate) against the engine clock's motion, within one block plus one callback's frames, plus 8 for the resampler |
| D19 | A14, A15 | G12 doesn't change the device's rate. A judged run whose device isn't at 48 kHz, or whose callbacks aren't 128 frames, is "not the designed set-up" and FAILs. It names what it found and what to set |
| D20 | A18, A19, A20 | The `Wake` gets files of its own (`mac/wake.rs`, `linux/wake.rs`), which the grep reads whole. `thread::foreign` catches a panic in `f` and raises it again on the joining thread. The no-alloc test's word is "`Player::panic()`" |
| D21 | "a worker that fails to join" (the brief) | A task panic: `crash()`. A worker that never comes: not waited for. A worker stuck in a task: the device thread dozes, then the stall path. A worker refused real time: D4. A workgroup join refused with `EINVAL` (the group was cancelled: the output changed under it): the player reports `changed`, and the server replaces it as it does for cpal's DeviceChanged. Any other join refusal: it plays on, `missing` says so, and G12 FAILs. The pool's join at close can't fail, since a worker's panic aborts; so the `start` and `stop` hooks hold no `unwrap` or `expect` |
| D22 | A.9 | In portable `rt.rs`: `ticks(ns, numer, denom)` and the policy's four numbers, tested on Linux (the M1's timebase 125/3: 2,666,667 ns is 64,000 ticks). FourCCs as `u32::from_be_bytes(*b"oswg")`. The CoreFoundation declarations move from `mac/watch.rs` to one `mac/cf.rs` that both files use. `#[link(name = "CoreAudio", kind = "framework")]` only in `mac/rt.rs` |
| D23 | A7, A.10 | §7 becomes `ci/sweep.sh`: a scratch `HOME`, writes `~/m2-8a.txt`, about 35 minutes. The owner types two lines. Before it: built-in speakers at 48 kHz, plugged in, other apps closed. The offline S decides only the note to the owner about the base M1, the export count, the voices trigger and the executor's cost. The live count comes from the live sweep alone. It replaces HANDOFF's owed offline sweep |
| D24 | A6 | `out_of_memory` (`graph.rs`) returns false when the chain holds a kernel's `OutOfMemory` (one line); the lane may touch that function. The kernel's own words say "memory" and "32 MB" |
| D25 | A.3 | RENDER-28: set-up system calls happen once per thread, at its start or on the fill that hands it its engine threads. RENDER-2a names the hold's fade and the metronome's click beside the resampler and the 5 ms fade. §7's spin line says macOS may demote a spinning worker |
| D26 | A.8 | §1 says OS-61 is `next`'s. For live runs `--sweep` is an `Option`: when it isn't given, one run at the rule's count. A count of 1 runs once, whatever `--between` says |
| D27 | A.11 | The gaps lane stays, in parallel. It is cut first if day 2 runs short. HANDOFF's gap words follow Q20 |

## The owner's Mac sweep (added by the lead, 2026-10-17 18:05; overrides the Decisions where they differ)

The owner ran 8a's offline sweep on the reference Mac (MacBookPro18,1) at `main` 8a6d9e1. Raw output, word for word:
`$SCRATCH/mac-runs/8a-offline-8a6d9e1.txt`. Put it in build-notes-8a.md ("The Mac sweep") with these decisions.

| Line | Read | Decision |
|---|---|---|
| models | passed (cargo stopped at no binary; `tail -5` showed only the doc-tests) | none. §7's next command shows each `test result` line instead of `tail -5`: `grep -E "^test result|FAILED|panicked|error"` |
| bits | every count the same bits; 10 of 10 renders on 8 threads | none |
| `cpusperl2` | 4 (8 fast cores, 2 slow) | the live count's cap stays 4 |
| S (p99, 1 ÷ 4 threads) | 4048 ÷ 1385 = 2.92 | ≥ 2.5: keep the live default |
| 8 against 4 | mean 720 against 929 µs: 8 threads are 22% faster (p99 1558 against 1385, worse) | ≥ 10%: **an export alone uses every fast core on a Mac** (`Threads::Alone` counts `fast` on macOS, `per_cache` elsewhere, as the spike chose for Linux). Build it in slice 4 (or the next agent to see this), with a test |
| partitions | the slowest p99 is the song's 392 µs | under 667: no voices across threads |
| executor's cost | −53 µs a block | under 40: nothing to trim |
| paced, 1 thread | p99 4180 against 4048 back to back: +3% | under 15%: by itself no pointer to spinning |
| **paced, 4 threads** | mean 1682 against 929 back to back (+81%); **p99 8045 against 1385 (5.8 times)**; max 52 ms; 501 of 7500 blocks over the period | Not in the rule, and the worst number of the run. The offline workers run at background priority and park between blocks; macOS wakes them late. The live workers are real-time threads in the audio workgroup, so the live sweep (`--between park,spin`) decides. Slice 4: G12's live report prints, for each count and mode, the paced p99 next to the back-to-back p99, and the wake latency of a worker (from the caller's wake to the worker's first task) as information, so the Mac's live run can show whether parking is the cause. If spin wins on the Mac, `BETWEEN` becomes `Spin` there (one constant) |
| song's tail | 310 µs a block, 11% of the work (33% of a 4-thread block) | R1 (slice 3's first commit) is worth it |
| load | 19.75 at the start (the build just before) | information: the next command waits for the load to settle first, or says so |
