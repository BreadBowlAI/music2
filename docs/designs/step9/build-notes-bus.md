# Track bus: build notes

Branch `bus`, from `main` at `e7518f4`, 2026-10-18. The plan is [bus-plan.md](bus-plan.md); what changed while
building is written there, under each slice ("As built").

## In short

- **Ready to land** (2026-10-18). Linux green at `7bb44f3` (`ci/check.sh --gates`); macOS owed. `main` at
  `34dcfc4` (the Mac run, `mf2`, and tracks `br` and `gb`) is merged in.
- **Buses, sends and groups** (gap #27). A bus is a track that plays no notes. It plays what other tracks send it:
  a shared reverb, a delay, or a group (the drums through one fader).
- **All routing is in one place: the mixer in `song.ts`.** A channel gets `sends` (a knob each, after the fader or
  before it) and `to` (a group bus instead of the master).
- **In the Mix view:** Add bus (Reverb, Delay, Group), a send knob per bus on every strip, and an output picker.
- **No sound changed:** `e2e/renders.sha256` keeps all 12 lines. A song with no buses makes the same graph and bits.
- **New versions:** `@m2/core` 1.5.0 and std 0.4.0. A song asking for std "0.3" needs `std = "0.4"` (HANDOFF,
  "Your own songs"); it sounds the same.
- Linux timings are information only. The Mac's run is owed (PLAT-1).

| Slice | You get |
|---|---|
| 1. Buses in song code | `bus()` and `feed()` in `@m2/core`; sends and groups in std's mixer; each bus in its own partition; a loop refused in plain words |
| 2. The edits | Add a bus, add or remove a send, before or after the fader, a track's output; each one undo step, with routes |
| 3. The Mix view | Bus strips, send knobs, the output picker, Add bus; the timeline marks a bus |

## The decisions

| # | Decision | Why |
|---|---|---|
| 1 | A bus is a track (`Module.bus` in the IR) | Keys, files, rename, delete, mute and solo come free |
| 2 | Routing lives in the song's one mixer | A send after the fader needs the fader; the Mix view shows one place |
| 3 | A new send: after the fader, −12 dB | You hear it at once, under the dry sound |
| 4 | A channel goes to one place: the master or one bus | Groups of groups work; loops are refused at three doors |
| 5 | Soloing a bus keeps its sources' dry sound too | One cell per track; a wet-only solo is logged |
| 6 | One engine change: a bus gets a partition of its own | Every bus had folded into the song's serial tail |
| 7 | One core change: `Confined` lets a node outside the ranges rewire to a node made inside them (WRITE-8's own words) | A new send renames the mixer's later nodes, which the bus feed reads |
| 8 | No new external crate | None needed |

## The contract

| Rule | Change |
|---|---|
| RENDER-34 (new) | Buses: a track the song feeds; std's mixer routes sends and groups; a loop is a build error naming it |
| RENDER-2 | A bus's partition: a bus fed by one other bus alone runs with it |
| MON-3 | Routes join "who feeds whom": the mute and solo table |
| PKG-6 | Nested dictionaries and settings in an entry; the `key` setting; the bare one-control entry |
| WRITE-6, WRITE-10 | `Confined` now does what WRITE-8 says (Today line); the bus edits' rows |

## The review's findings

Four, all minor, so none was fixed now (the owner's rule): each is one line in [gaps.md](../../gaps.md), and the docs finding 2 named were corrected.

| # | Finding | Where it went |
|---|---|---|
| 1 | Change ID on a bus breaks its routes; the build refuses it with std's words | gaps.md, Mixing; the plan's Decision 4 corrected |
| 2 | Old songs need `std = "0.4"`; HANDOFF still said 0.3 | HANDOFF's "Your own songs" and gaps.md's std line fixed |
| 3 | An automated send shows "—" in the Mix view, with no menu | gaps.md, Mixing |
| 4 | `Confined`'s new rewire rule has no core test | gaps.md, For developers |

## The check

The first run, at `3a0f504`, passed but for one suite that passed only on its retry: `st-midi`'s "Esc writes
nothing" read the page's revision before the page heard of the undo just before it. A test race, not the app's:
fixed in `9c985c3` (it waits until the page's revision is the server's), 3 of 3 alone. The second run, at
`9c985c3`, passed clean (1,751 Rust tests, 72/72 suites). Then `main` moved (track `br` landed) and was merged in
again (`792d281`; bus's by-ear steps moved to 126 to 132), which passed clean (1,770 Rust tests, 74/74 suites).
Then track `gb` landed and was merged in too (`7bb44f3`, no code conflicts). The last run:

`ci/check.sh --gates` at `7bb44f3` (`~/m2-runs/check-bus/summary.txt`), word for word:

```
music2 check: PASS
commit   7bb44f3c14c9 on bus (clean)
machine  Ubuntu 24.04.3 LTS, x86_64, 32 threads
tools    rustc 1.99.0, node v24.21.0, 34 min
plugins  /home/jerry/.local/share/m2-test-plugins/clap (the builds ci/plugins.sha256 records)

toolchain        PASS  rustc 1.99.0
web build        PASS  
build            PASS  
app              PASS  the UI connected: first-song in 1 s, with-plugins in 1 s
rust tests       PASS  1770 passed, 0 failed
clippy           PASS  
pool models      PASS  29 passed, 0 failed
std kernels      PASS  tests and clippy
std.wasm         PASS  rebuilt to the same bytes in another folder
mac lint         PASS  the macOS code type-checks and lints
browser suites   PASS  74/74 suites ok
renders          PASS  12 of 12 match e2e/renders.sha256
old engine (S)   PASS  built for G7f from a clean copy of 8aa301e, in /home/jerry/.cache/m2-check/s-8aa301e
gates            PASS  the 7 runs ci/gates.txt names
repo unchanged   PASS  

gates:
  machine: ASRockRack B665D4U-1L, Ubuntu 24.04.3 LTS, AMD Ryzen 9 7950X3D 16-Core Processor, x86_64, 32 threads
  === m2-gates --server tx-data watcher
  PASS  G11 tx-data (PLANE-5, PLANE-1, WRITE-15) on server on p16
        ok    idle disk: the session's part before the preview (plan, check, pre-check), under 5 ms at p95: p50 1.0 ms, p95 1.3 ms, max 1.5 ms (n=200)
        ok    idle disk: from the request to the first block that plays it (its preview), under 20 ms at p95: p50 5.5 ms, p95 9.4 ms, max 10.3 ms (n=200)
        ok    with another writer syncing 4 MB chunks: the session's part before the preview (plan, check, pre-check), under 5 ms at p95: p50 1.0 ms, p95 1.4 ms, max 3.0 ms (n=200)
        ok    with another writer syncing 4 MB chunks: from the request to the first block that plays it (its preview), under 20 ms at p95: p50 5.1 ms, p95 9.3 ms, max 10.9 ms (n=200)
        ok    idle disk, 5,000 files in assets/: the session's part before the preview (plan, check, pre-check), under 5 ms at p95: p50 1.1 ms, p95 1.4 ms, max 1.9 ms (n=200)
        ok    idle disk, 5,000 files in assets/: from the request to the first block that plays it (its preview), under 20 ms at p95: p50 5.5 ms, p95 9.7 ms, max 10.7 ms (n=200)
  PASS  G11 watcher (WRITE-19) on server on p16 with 5,000 files in assets/
        ok    idle: the CPU its ticks used over 10 s, under 1% of a core: 0.10% of a core (199 ticks in 10 s)
  === m2-gates --server --reference 50 --at 180 add-effect
  PASS  G5 and G7a add effect, through the server (RENDER-8, RENDER-25, RENDER-25a) on server on p50 from 180 s
        ok    audible within compile + 100 ms: past the session's work: p50 44.0 ms, p95 50.1 ms, max 50.1 ms (n=10)
        ok    G7a: heard within 200 ms of the request, its compile included: p50 108.8 ms, p95 118.5 ms, max 118.5 ms (n=10)
        ok    with-plugins: audible within compile + 100 ms: past the session's work: p50 13.3 ms, p95 14.6 ms, max 14.6 ms (n=2)
  === m2-gates --reference 50 --at 180 add-effect tails
  PASS  G5 add effect (RENDER-25, 25a, 8) on engine (step 7) on p50
  PASS  G7b tails of untouched tracks (RENDER-25, 25a) on engine (step 7) on p50
  === m2-gates --server export
  PASS  G7e export (RENDER-1, RENDER-6a, HOME-4, HOME-6) on server on examples/with-plugins, real time
  === m2-gates store --reference 50
  PASS  G7f store (RENDER-32) on the runtime on p50
        ok    the same music: every renderer's timed blocks the same bits: the same 2880000 samples (hash 22cd1b3810530499) from the app's layout, its control and S in every round, peak -1.0 dB
        info  block, starting a turn: of the judged ones, each turn's first half over its second, against S's: -0 µs (the app's layout), +4 (its control); each turn's first block over the mean: +1559 µs, +1535, +2611 (S)
        info  machines: S's record is from MacBookPro18,1, macOS 26.6.2, Apple M1 Pro, aarch64, 10 threads; this run is on ASRockRack B665D4U-1L, Ubuntu 24.04.3 LTS, AMD Ryzen 9 7950X3D 16-Core Processor, x86_64, 32 thread
        skip  block within S + 80 µs, by the mean: would pass: -472 µs (5 pairs, -498 to -460) over S, measured here (bound 80). p99, not judged: -1210 µs (5 pairs, -1408 to -1044); not judged off the reference Mac (PLA
        skip  footprint within S + 64 MB: recorded, not judged: S's record is from another machine (PLAT-1)
        skip  full build within 1.5 × S: would pass: 64 ms against S's 183 ms, each its process's first, measured here; not judged off the reference Mac (PLAT-1)
  === m2-gates arena --reference 50 --at 180
  PASS  G7f the arena's callback (RENDER-32, RENDER-25) on the player on p50 from 180 s
        ok    the same music: every player's timed callbacks the same bits: the same 2856960 samples (hash cdb85ca0bbb29381) from the app's layout, its control and S in every round, peak -1.0 dB
        info  callback, starting a turn: of the judged ones, each turn's first half over its second, against S's: +42 µs (the app's layout), +35 (its control); each turn's first callback over the mean: +4389 µs, +4306, +
        info  machines: S's record is from MacBookPro18,1, macOS 26.6.2, Apple M1 Pro, aarch64, 10 threads; this run is on ASRockRack B665D4U-1L, Ubuntu 24.04.3 LTS, AMD Ryzen 9 7950X3D 16-Core Processor, x86_64, 32 thread
        skip  callback within S's + 320 µs, by the mean: would pass: -1868 µs (5 pairs, -1918 to -1804) over S, measured here (bound 320). p99, not judged: -3887 µs (5 pairs, -5738 to -3312); not judged off the referenc
        ok    the slowest activation block under half the device buffer's period: 2153 µs, p50 1373; the block before it p50 1793, max 2865 (bound 5333 µs; the run's p99 callback 8423)
  === m2-gates realtime --reference 50 --minutes 1
  PASS  G12 live (RENDER-28, RENDER-28a) on the silent clock on p50, 128 frames

logs     /home/jerry/m2-runs/check-bus.tgz
```

## Pictures

From the desktop app on the invisible screen `:98` (the window 1440x900), the bus fixture
`core/session/tests/fixtures/buses`, built at `3a0f504` (`~/m2-runs/app-shots/bus-3a0f504/`, with `captions.txt`):

| File | Shows |
|---|---|
| `1-add-bus.png` | Add bus offers a Reverb, a Delay or a Group, each with one line on what it is for |
| `2-send-menu.png` | A send knob's menu: its level, before the fader, Automate, Remove send, Edit in code |
| `3-output-picker.png` | A track's output picker: the Master and every bus, a tick on where it goes now |
| `4-buses-playing.png` | The song playing: the buses' meters move with what the tracks send them |

## For the owner's Mac

By ear: [mac-run.md](../../mac-run.md) steps 126 to 132.
