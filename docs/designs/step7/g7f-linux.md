# G7f and the Store layout, on Linux

A study for the owner's choice of RENDER-32's Store layout. On the Mac no layout passed design-7 §11.3's rule, so
the owner decides (§11.3: "If none passes, step 7 stops and the owner decides"). This study asks how close a real
user gets to the limit, measures every layout on Linux, and recommends one.

Measured on 2026-10-15 on the owner's Linux server: AMD Ryzen 9 7950X3D (16 cores, 32 threads), Ubuntu 24.04, kernel
6.8, Rust 1.99.0, wasmtime 49.0.2. Branch `step7` at `a06c543`, in a scratch worktree (now removed). Release builds.
Timings on Linux are for information only (PLAT-1): the Mac must confirm the layout chosen.

**In short**
- Today (P1) each kernel instance reserves 4 GB of the app's address space (room, not memory it uses). One app
  holds about 32,000 kernel instances in all, then no more can be made. p50, the 50-track test song, has 6,058, so
  about 5 copies of it fit.
- music2 holds 1 copy of the open song while you play and edit, 2 while it exports, and 3 if you open another song
  during an export (measured). Today, each time the audio device stops answering, 1 more copy stays behind for good
  (a bug, measured).
- So a song about twice as big as p50 can fail when you open another song during an export. A song more than 5
  times as big can't open at all. At the limit nothing crashes, but the messages are technical. An edit is saved
  but not heard, and nothing says why.
- **Recommendation: P2, with two settings the design didn't list.** Each instance gets 32 MB instead of 4 GB. Its
  memory stays in one place. The "Spectre guard" on its memory checks is off.
  - Sound: the same, bit for bit (8 recorded renders and a 4-minute p16 render checked).
  - CPU: about 2% more engine time at p50 (34 µs on a 2.67 ms block).
  - Limits: the address-space limit goes away (about 4 million instances fit). Memory (RAM) becomes the limit.
  - The trade: the guard protects against a rare kind of attack, and that attack needs a clock. Kernels have no clock.
    With the guard kept on, the same layout costs about 8% instead of 2%.
- Before it lands: measure it on the Mac. Either way, fix the device bug: a player that is let go must free its
  engine.

## 1. How many copies of the song music2 holds at once

A "copy" here is one full set of the song's kernel instances (one renderer). The app opens one song at a time (one
window, one `Actor.open`). Each `music2` command (`render`, `bounce`) is a process of its own.

| When | Copies held | Where in the code |
|---|---|---|
| Playing | 1 | the player's callback holds the live engine (`core/play/src/lib.rs`, `Callback.live`) |
| An edit (an activation) | 1, plus the instances the edit makes | the arena moves every kept instance into the new plan; the old plan keeps only what fades out. Measured: 145 = 144 + 1 for an edit that adds one kernel. G7a on p50: 0 or 1 new per edit. |
| Exporting while playing | 2 | the export builds its own kernel instances (`Render::new` or `Render::lending`; only plugins are borrowed). One export at a time (`exports.running`). Measured: 290 = 2 × 145. |
| Opening another song, a sample-rate change, a new audio device | 2, for a moment | the new song's renderer is built while the old one plays, then the old one fades out (`Actor::play`, `Way::Replace`, `Way::Load`) |
| Opening another song while an export runs | 3 | the export goes on with the old song's instances. Measured: 434 = 3 × 145. |
| Each audio device that stops answering | +1, for good | `Graph::device` lets the player go. Its engine is never freed: the device thread parks forever holding the stream (`Player::start`), and the silent player's thread never ends (`Player::silent`). Measured: 264 instances still alive 1 s after the player was dropped. |

A device "stops answering" when it reports an error, when a message waits 500 ms, or when it hasn't called back for
1 s. Today on Linux a single xrun does it (platform design §8, bug 1). Unplugging an audio interface most likely does
it on every OS. On a real device the old stream is kept running, so the old engine may go on playing beside the new
one. That part is unverified: this server has no sound card.

**How close a real user gets, with today's P1** (about 32,000 instances per app):

| Song size | What happens |
|---|---|
| up to about 10,000 instances (1.7 × p50) | fine in every case above |
| about 10,000 to 16,000 | opening another song during an export fails |
| about 16,000 to 32,000 | exporting fails, and so does opening another song |
| over about 32,000 (5.3 × p50) | the song can't open |
| any size | each device that stops answering takes away 1 copy for good: with p50 open, after 4 of them an export fails, and after 5 the song can't be loaded into a new player |

p50 is 50 tracks of std's simple instruments (bass, pluck, pad: about 121 instances a track). Bigger instruments,
more voices or more tracks reach 10,000 soon. A user who wants a full DAW will get there.

## 2. What happens at the limit

Measured with P1 in a server test: a small song (a pad holding a chord, 144 instances) open and playing, then the
process's address space capped at 3 GB above what it held, less than one more instance needs (`RLIMIT_AS`; `mmap`
fails with the same `ENOMEM` as when the address space runs out).

| Action | What the user sees |
|---|---|
| Export | Refused. Toast: "Couldn't export the song: pad/n27: voice: pad/n11: instantiating \`std:adsr\`: mmap failed to reserve 0x104000000 bytes: Cannot allocate memory (os error 12)" |
| A code edit | Saved, revision 2, no problem listed. The old version plays on. Only the log says why ("music2: couldn't play the accepted revision: … mmap failed …"). |
| Opening another song | Refused: "loading the std package: … std.wasm: instantiating: mmap failed to reserve …" |
| The app | Lives on. Playback goes on; a 64 MB allocation and a new thread still work. |

With the pooling allocator (P3) it is worse: the app crashes. Each `PackageSet` makes its own wasmtime engine (one per
open song; two in the gates' `Lab`), and with pooling each engine reserves its whole pool (48 TB) when it is made. In
5 of 9 G7f runs the second pool didn't fit, and `engine()`'s `expect("wasmtime engine")` panicked: "failed to create
memory pool mapping … mmap failed to reserve 0x2ff32a000000 bytes".

**Two more Linux limits, for every layout:**
- *The map count.* Each kernel instance takes about 4 memory mappings, so p50 takes about 24,000. Ubuntu 24.04 allows
  1,048,576 (about 40 copies). A distribution that keeps the kernel's default, 65,530, stops every layout at 2 copies
  of p50. Then any new mapping fails, the memory allocator's too, which most likely aborts the app (not measured).
- *`ulimit -v`.* Any limit on address space stops P1 almost at once (facts.md §8: 3 instances at 16 GB). P2 needs
  190 GB of address space for p50, so a 16 GB limit stops it too, later.

## 3. The layouts, measured

**Method.** `m2-gates store --reference 50` (G7f part 1) on p50, in a scratch copy of `step7` with two changes: the
layout chosen by an environment variable in `engine()`, and Linux's `/proc/self/status` read for the memory rows
(design §8's `memory()` isn't built yet). Runs pinned to one core (CPUs 4 and 20). Runs alternated, the order rotated
each round. Block time: 11,250 blocks (30 s) from 3:00. "CPU" is the thread's time on a CPU, which preemption doesn't
add to; it agreed with the wall-clock mean in every run.

Three rounds of runs:
1. Every layout, 9 rounds, in one build with the pooling and MPK features on (load 1.6 to 4.6; some runs waited for
   the load to fall). That build alone costs about 1.5%, so its layouts are compared with P1 in the same build.
2. P1 against P2, in a build with nothing but the layout switch, as P2 would ship, 8 rounds (load 1.3 to 1.9).
3. G7f part 2, `m2-gates arena --reference 50 --at 180`, round 2's builds, 6 rounds (load about 1.5).

**The options** (block time against P1 at p50: the median of the differences between runs in the same round):

| Option | Block time mean / p99 vs P1 | Address space per copy of p50 | Most copies of p50 | Build p50 | Footprint per copy (resident) | What a user would notice |
|---|---|---|---|---|---|---|
| **P1** (today): 4 GB per instance, no bounds checks | P1: 1,742 / 2,353 µs | 24,611 GiB (18.8%) | 5 (the 6th failed) | 220 ms | 243 MB | the limits in §1 and §2 |
| **P2** as designed: 32 MB, 64 KB guard, may move, guard on | +9.1% / +218 µs (round 1) | 190 GiB (0.15%) | 12 built (the cap); about 690 by address space | 218 ms | 243 MB | about 9% less room for tracks before glitches |
| **P2, fixed in place** (`memory_may_move(false)`), guard on | +142 µs (+8.2%) / +200 µs (round 2; each round +138 to +150) | 190 GiB | as P2 | 214 ms | 243 MB | about 8% less room for tracks |
| **P2, fixed in place, guard off** (recommended) | **+34 µs (+1.9%) / +45 µs** (round 2; each round +27 to +38) | 190 GiB | as P2 | 214 ms | 243 MB | nothing you can hear; about 2% less room for tracks |
| P2 at 1 GB, fixed in place, guard on | +8.1% / +206 µs (round 1) | 6,059 GiB (4.6%) | about 21 | 221 ms | 243 MB | as P2 guard on: any size under 4 GB pays the same |
| **P3** pooling, 2 × count slots | −0.5% / −1 µs (round 1; 4 runs) | 49,103 GiB per engine, reserved at start (98,205 for the gates' two: 74.9%) | 2 per engine (the pool is full) | 218 ms | 241 MB | crashes when a second engine's pool doesn't fit (5 of 9 runs) |
| **P3 with MPK** (memory protection keys), 2 × count slots | +3.3% / +77 µs (round 1) | 3,279 GiB per engine (6,558 for two: 5.0%) | 2 per engine; 8 with a 48,528-slot pool | 218 ms | 241 MB | not available: Linux on Intel only in wasmtime 49 (needed a patch on this AMD CPU); never on a Mac |

Other numbers:
- **Drop** (a p50 renderer freed): P1 118 ms, P2 85 ms, P3 52 ms. No layout raised the audio-like thread's p99 by more
  than 4 µs.
- **The callback with the arena** (G7f part 2, 512-frame callbacks, 4 blocks each): P1 mean 7,052 µs, p99 9,362 µs.
  P2 fixed, guard on: +566 / +747 µs (+8.0%). P2 fixed, guard off: +125 / +177 µs (+1.8%). The activation block: P1's
  slowest 2,416 µs, P2 guard off 2,535 µs, both far under the 5,333 µs bound.
- **Export speed** (p16 offline, 4 minutes of music, one run each): P1 47.1 s, P2 guard off 47.3 s, P2 guard on 50.3 s.
- **Sound.** Bounds checks don't change the arithmetic. Under P1, P2 guard off and P2 guard on: all 8 renders match
  `e2e/renders.sha256`, and the p16 render is the same file (`ebc87e0b57c0cbb5…`) in all three.
- **The CPU has MPK** (`pku`, `ospke` in `/proc/cpuinfo`), but wasmtime 49 checks for an Intel CPU
  (`wasmtime-internal-core`'s `mpk.rs`, `is_intel_cpu`) and refuses: "mpk is disabled on this system". It ran here
  only with that check patched in the scratch copy.

**Why "guard off" costs so little.** P2 checks every memory access: is the address inside the 32 MB? With the
Spectre guard on, each access must wait until its check is done (Cranelift's `select_spectre_guard`: that wait is the
guard's point), and Cranelift may optimise it less than a plain select. With the guard off, the processor runs the
access and its check side by side. On arm64 the guard is a `csel` with no `csdb` barrier (`use_csdb` is off by
default), the same kind as x86's `cmov`, so the Mac should behave alike. Unverified: measure it there.

## 4. Options the design didn't list

| Option | Verdict |
|---|---|
| P2 fixed in place (`memory_may_move(false)`) | Small gain: checks compare with a constant (about 2 points of the 9%). Safe: `StoreLimits` already stops memory at 32 MB. |
| P2 with the heap Spectre guard off (`enable_heap_access_spectre_mitigation = false`) | **Clearly better**: from about 8% to about 2%. A security trade (§5). |
| One reservation size per kernel, from its declared memory | Not better. Any size under 4 GB needs the same checks (P2 at 1 GB costs what P2 at 32 MB costs), and wasmtime sets the size per engine, not per module. |
| One Store per voice pool | Not better. The address space goes with memories, not Stores (design §11.1), and each voice still has its own memory. |
| P1 for playback, P2 for exports (two engines), or P2 only when P1 runs out | Not clearly better. It keeps P1's speed but compiles std twice, and a song's speed would depend on its history. The live limit stays. |
| Fewer instances (one instance for a whole voice, or voices made as needed) | A big change to the kernel ABI or the audio thread. Later, if ever. |

## 5. Recommendation

**P2, fixed in place, with the heap Spectre guard off.** In `core/runtime/src/packages.rs`'s `engine()`:
`memory_reservation(KERNEL_MEMORY)` (32 MB), `memory_guard_size(64 KiB)`, `memory_reservation_for_growth(0)`,
`memory_may_move(false)`, and `cranelift_flag_set("enable_heap_access_spectre_mitigation", "false")` (an `unsafe`
call, with a comment saying why).

In plain words, for the owner:
- **Sound quality:** no change. The same bits come out.
- **CPU:** about 2% more of the engine's time. The biggest song that plays without glitches gets about 2% smaller.
- **Limits:** the 32,000-instance limit is gone. Opening a big song, exporting and switching songs don't run out.
  The computer's memory is the limit now, as it is for other music apps.
- **Safety:** the guard turned off protects against "Spectre", an attack where code reads memory it shouldn't by
  timing the processor. It needs a clock. Kernels can't read one: they import nothing at all. Today every kernel is
  std's, written by us. If music2 later runs kernels from other people's packages, look at this again.
- **If you'd rather keep the guard:** the same layout with the guard on costs about 8%. Still no limit.
- **If you'd rather change nothing:** P1 costs nothing, and the limits in §1 stay. Fix the device bug and the
  messages in any case.

Not P3: it crashes when a second song's pool doesn't fit, and it caps the instances anyway. Not MPK: no Mac has it,
and wasmtime offers it on Intel only.

## 6. For the lead

**Before it lands** (PLAT-1): on the Mac, alternate G7f part 1 and part 2 between P1 and the recommended layout, as
here. The rule from §11.3 against S: p99 within 80 µs, footprint within 64 MB, 2 copies at under 25% of the address
space, build within 1.5 × S. On Linux the recommended layout meets all four against P1 (p99 +45 µs, footprint
unchanged, 0.3% of the address space for 2 copies, build 214 ms against 220). No S record exists on Linux.

**Problems found** (not fixed here):
1. A player that is let go keeps its engine for good (§1). It belongs with bug 1 in `core/play`: the stream and the
   silent player's thread must end, and the engine be freed, when the `Player` drops.
2. A build that fails after an edit is silent: the edit is saved, the old version plays, and only stderr says so
   (`actor.rs`, "couldn't play the accepted revision"). Export and open show wasmtime's raw text.
3. P3 as designed panics at engine creation when its pool doesn't fit (`packages.rs`, `expect("wasmtime engine")`).
4. G7f part 1 stops at 4 copies (`SIDE_BY_SIDE`), so it never shows P1's limit of 5. On Linux its memory rows need
   `memory()` (platform design §8).
5. wasmtime 49 refuses MPK on AMD CPUs that have it (`is_intel_cpu`).

## Where the numbers are

`~/m2-runs/g7f-linux/`: every run's log (`runs/`: `t-` round 1, `u-` round 2, `a-` round 3, `side-` the copies side
by side), the scripts (`one.sh`, `campaign*.sh`, `summary.py`, `renders.sh`), and the campaigns' logs in
`~/m2-runs/logs/g7f-*`. The scratch copy's changes are in `scratch.diff` there: the layout switch in `engine()`
(`M2_G7F_LAYOUT`), the Linux memory rows and CPU time in `store.rs`, the two server tests
(`g7f_at_the_address_space_limit`, `g7f_how_many_at_once`), the player test (`g7f_leak.rs`) and the MPK patch. The
copy itself was removed.
