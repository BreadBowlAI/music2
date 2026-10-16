# Where step 7's callback spends its extra time (label prof)

Linux only (PLAT-1: information, not a verdict). The owner's server, AMD Ryzen 9 7950X3D, Ubuntu 24.04,
Rust 1.99.0, wasmtime 49.0.2. 2026-10-16. Read-only on the repo: every change below is a prototype in a
scratch copy under this folder. Nothing was committed.

## In short

- On p50 from 3:00, 512-frame callbacks, steady play, step 7 (HEAD `ed7fc8e`) costs this much more than
  S (`8aa301e`), measured fresh, in the same runs:
  - with P1: **+144 to +158 µs** a callback (about 2.1% of S's 7,020 µs);
  - with the app's layout (P2): **+204 to +212 µs** (about 3.0%).
- The platform step in between costs nothing (`b90bacf` against S: -2 µs, -42 to +39).
- **All of step 7's own cost is in one place:** each kernel instance now has a wasmtime `Store` of its
  own (about 6,000 of them), and every kernel call (about 9,250 a callback) touches that instance's
  store, which is no longer in the cache. Most of it is the two `memory.data_mut(&mut k.store)` lookups
  in `KernelNode::process`; the rest is the call itself, and the box the instance lives in.
- **It can be cut with no change to any sound and no change to the owner's O1 layout.** Four changes
  together (keep each memory's address; one store per voice pool; the instance inline, not boxed; the
  poison check done at activations) make HEAD with P2 **139 µs faster than S** (-157 to -114), 339 µs
  faster than HEAD today. The first three carry almost all of it. Every hash, render and test checked is
  unchanged.
- What is left over S is O1 itself: its bounds checks add 9.6 million instructions a callback (+12%) to
  the kernels' own code. That stays.

## The numbers, fresh, all in the same runs (lockstep, CPU 4)

How: one process per build, all pinned to CPU 4, taking turns of 94 callbacks (about 0.7 s), the order
turning each turn. Each run is fresh processes playing p50 from 3:00 for 30 s. The table gives each
build's difference from the reference in the same run, the median over runs, then the lowest and highest.
C2's first run is left out: something else ran then (every build in it was 400 µs slower).

| Campaign (runs) | Build : layout | Against | Median µs | Range |
|---|---|---|---|---|
| C1 (8) | platform base `b90bacf` | S | -2 | -42 to +39 |
| C1 (8) | HEAD : P1 | S | +158 | +136 to +173 |
| C1 (8) | HEAD : P2 | S | +204 | +125 to +240 |
| E (6) | HEAD : P1 | S | +144 | +122 to +195 |
| E (6) | HEAD : P2 | S | +212 | +153 to +222 |
| D (6) | HV1 : P1 (memory address kept) | HEAD : P1 | -192 | -219 to -145 |
| D (6) | SV1 (S with the same change) | S | -36 | -326 to +31 (one disturbed run) |
| D (6) | HV3 : P1 (one store per voice pool) | HEAD : P1 | -133 | -151 to -103 |
| D (6) | HV1+HV3 : P1 | HEAD : P1 | -255 | -402 to -198 |
| C2 (5) | HV1 : P2 | HEAD : P2 | -181 | -217 to -146 |
| C2 (5) | HV3 : P2 | HEAD : P2 | -118 | -159 to -86 |
| C2 (5) | HV4 : P2 (poison mark at activation) | HEAD : P2 | -14 | -27 to +13 |
| C2 (5) | HV1+HV3 : P2 | HEAD : P2 | -251 | -273 to -193 |
| C2 (5) | HV1+HV3+HV4 : P2 | HEAD : P2 | -263 | -286 to -233 |
| E (6) | HV1+HV3+HV4 : P2 | S | -48 | -83 to -23 |
| E (6) | **HV1+HV3+HV4+HV5 : P2** (all cuts) | S | **-139** | -157 to -114 |
| E (6) | all cuts : P2 | HEAD : P2 | -339 | -355 to -310 |
| E (6) | all cuts : P1 | S | -184 | -219 to -170 |

S's mean callback here is about 7,020 µs (the Mac's: 8,936). CPU 4 is on the half of the chip with the
96 MB cache: the other half was in use by another agent's gate runs, so I stayed off it. A bigger cache
hides cache misses, so the Mac likely pays more for the same misses (its P2 over P1 is +141 µs; here
+50). Logs: `logs/ls-C1.log`, `ls-C2.log`, `ls-D.log`, `ls-E.log`. Runs from campaigns A, B and C on the
other half are not used (disturbed; see Problems): `logs/discarded/`, `vg/discarded/`.

## What step 7 added, call by call (from reading S and HEAD)

Steady play, no edits, no plugins (p50 has none). Counted per block on both S and HEAD: 2,245 `k_process`,
59.8 `k_active` and 7.2 `k_reset` calls, so about 9,250 kernel calls a callback.

| Where | What step 7 added | Cost measured |
|---|---|---|
| per kernel call, `KernelNode::process` (`lib.rs`) | `k.memory.data_mut(&mut k.store)` twice: the store's id check, its instance table, the instance, the memory's definition, all in a store of the instance's own (S: one hot store) | about 156 µs (HV1's 192 less S's own 36) |
| per kernel call, `KernelInst::process/active/reset` | the call goes through the instance's own store: id check, stack limit and the saved entry and exit state in its `VMStoreContext`, its default caller, call hooks | about 120 µs, overlapping the row above (HV3) |
| per kernel call | the instance sits behind a `Box` (`inst: Option<Box<KernelInst>>`); S kept it inline in the exec list | about 82 µs (HV5 on top of the others: -116 to -68) |
| per top-level node, `Graph::process` | the poison scan: every output sample of every kernel, poly, plugin and feedback node checked for non-finite values, every block | about 14 µs; 316,000 instructions a callback |
| per block, engine and renderer | window and hold checks, `end_ramps`, carried cells, `Option`-wrapped lanes and pools, the fade version | under 0.1 M instructions a callback in all: not measurable |
| per callback, player | `held`, `quiet` and `refused` atomics | not measurable |
| fades, input fade-ins, ghosts | none in steady play: a fresh plan has no fade nodes and no window | 0 |
| epoch, fuel, limits | not enabled; `StoreLimits` is only asked on growth and instantiation | 0 |
| allocation counter | debug builds only | 0 in release |
| voice pools, `k_active` scans | the same calls as S | 0 |
| plugins | none in p50 | 0 |

## Where the time goes: kernels, plugins, host

Self time a callback, from a CPU-time sampler (1 kHz, 2 rounds of 2 passes of 30 s each, about 80,000
samples each; frame-pointer builds, about 1% slower). "Kernels" is the JIT code of std's kernels.

| | S | HEAD P1 | HEAD P2 | all cuts P2 (HV1+HV3+HV4) |
|---|---|---|---|---|
| callback (profile build) | 7,026 | 7,210 | 7,312 | 7,005 |
| wasm kernels (JIT) | 5,518 (78.5%) | 5,510 (76.4%) | 5,661 (77.4%) | 5,678 (81.1%) |
| JIT trampolines | 36 | 38 | 37 | 98 |
| plugins | 0 | 0 | 0 | 0 |
| wasmtime host code | 589 | 773 | 766 | 167 |
| m2-runtime (its own code) | 331 | 391 | 416 | 643 |
| port copies (memcpy) | 523 | 475 | 408 | 393 |
| other | 29 | 23 | 24 | 26 |

The rows don't move one to one: a cache miss shows up wherever the first touch is. With the cuts, the
misses left move into `KernelNode::process` and the trampolines. The totals are what count.

The host functions where HEAD P1 spends more than S (self time, µs a callback, `profiles/pf-*.json`):
`assert_belongs_to` +98 (the first read of each instance's store), `KernelNode::process` +112 (the boxed
instance), the store's instance-table entry +70, `Instance::offsets` +35, and a few smaller ones; less in
`Graph::process` (-56) and the copies (-47). In all, host code +184, kernels -8.

Instructions and simulated cache misses (callgrind, 372 callbacks: 1 s of warm-up and 3 s from 3:00, a
cache like an M1 Pro core's: 128 KB L1, 12 MB L2 with 128-byte lines), per callback:

| | S | HEAD P1 | HEAD P2 | HV1+HV3 P2 | HV1+HV3+HV4 P2 | all cuts P1 | all cuts P2 |
|---|---|---|---|---|---|---|---|
| instructions | 86.3 M | 86.7 M | 96.3 M | 95.9 M | 95.6 M | 86.3 M | 95.9 M |
| of which JIT | 78.2 M | 78.2 M | 87.8 M | 87.8 M | 87.8 M | 78.2 M | 87.8 M |
| L1 read misses, host code | 100,400 | 133,200 | 133,200 | 84,200 | 83,300 | 84,300 | 84,300 |
| L2 read misses, host code | 3,400 | 9,100 | 11,100 | 3,600 | 4,500 | 2,400 | 4,300 |

(`vg/m1b-*.out`, `vg/m1b-p1-diff.txt`, `vg/m1b-p2-diff.txt`, `vg/m1b-all-diff.txt`.)

So step 7 adds almost no instructions (+0.5%) but about a third more L1 misses and three times the L2
misses in host code. The cuts take them back below S. O1 adds the 9.6 M JIT instructions.

## The costs ranked, and how each could be cut

All µs are a callback on Linux (CPU 4), P2 unless said.

1. **The memory lookups through each instance's own store** (`core/runtime/src/lib.rs`,
   `KernelNode::process`, the two `k.memory.data_mut(&mut k.store)`). HV1 saves 181 µs (P2) and 192 (P1).
   S pays 36 of that already (SV1), so step 7 added about 150.
   - Cut: keep each memory's base address and size in `KernelInst` when it is built, and slice from them.
     No store is touched. The O1 layout never moves a memory (`memory_may_move(false)`, no growth
     room); P1's 4 GiB reservation never moves one either.
   - Risk: low. One small `unsafe` block and `unsafe impl Send for KernelInst`, sound only while memories
     never move, which O1 guarantees. Add a debug check that `data_ptr` is unchanged after `prewarm`.
     `patches/hv1.diff` (55 lines).
2. **The calls into about 6,000 different stores** (`KernelInst::process`, `active`, `reset`; wasmtime's
   `invoke_wasm_and_catch_traps`). HV3 saves 118 µs (P2) and 133 (P1); with HV1, the two together save
   251 (P2), as both remove the same cold store reads.
   - Cut: one store per voice pool, shared by its voices' kernel instances. A pool is already one box: it
     is built, moved and dropped whole, so a shared store frees nothing late (design-7 §11.1's reason
     against shared stores doesn't apply to it). Top-level kernels keep a store each.
   - Risk: medium. `Graph::process`, `reset`, `active`, `prewarm` and `kernel_memory` take the pool's
     store; the debug ABI-1 input checks must get it too (the prototype skips them for pool members);
     a pool's `StoreLimits` counts many instances (the memory limit stays per memory). RENDER-32's text
     ("one Store per kernel instance") would change: the lead or owner decides. `patches/hv3.diff`.
3. **The instance behind a box** (`KernelNode::inst: Option<Box<KernelInst>>`). HV5 (on top of the rest)
   saves 82 µs (6 runs, -116 to -68).
   - Cut: `Option<KernelInst>`, inline. An activation still moves it by swap, with nothing allocated.
   - Risk: low. Each exec entry gets bigger (about the size S had). Two lines.
4. **The poison scan every block** (`Graph::process`, the `poisoned` marks). HV4 saves about 14 µs at P2
   (-27 to +13) and 21 at P1 (-82 to +12): at the edge of what this can see; 316,000 instructions.
   - Cut: a trap still marks the box at once; the non-finite check happens when an activation moves the
     box (`plan::take_from`), on the old plan's last outputs, which are still in place. The decision is
     the same.
   - Risk: low. The activation block does that scan instead (the boxes it moves, tens of µs, against a
     5.3 ms bound). `patches/hv4.diff`.
5. Everything else step 7 added per callback: not measurable here (under 0.1 M instructions).
6. **O1's own cost** (P2 over P1): +50 µs here, +141 on the Mac. It is the bounds checks in the kernels'
   code (+9.6 M instructions). It can't be cut without changing O1, so I didn't try.

**Share accounted for:** all of step 7's own cost (P1 over S, +144 to +158). Rows 1 to 4 together remove 338 µs
at P1 (6 runs, -367 to -323), more than twice that, because they also remove waste S already had. With P2, all cuts end 139 µs
below S, so on this machine the rule's bound (320 µs) would be met with room to spare. The Mac must say
how much of this holds there: its caches are smaller, so the misses cost it more, and the cuts likely
save it more, not less, but that is not measured.

## G7f itself, with all cuts (Linux, information)

The gates binary of each copy, run on CPU 4, one after the other (`gates/*.log`):

| G7f | HEAD | all cuts (HV1+HV3+HV4+HV5) |
|---|---|---|
| part 2: callback mean, P2 / P1 | 7,241 / 7,180 µs | 6,866 / 6,798 µs |
| part 2: P2 over P1, pair by pair | +59 µs (+58 to +80) | +72 µs (+53 to +78) |
| part 2: slowest activation block (bound 5,333) | 2,419 µs, p50 1,832 | 2,559 µs, p50 1,767 |
| part 1: block mean, P2 / P1 | 1,782 / 1,765 µs | 1,700 / 1,677 µs |
| part 1: build of p50's renderer | 220 ms | 199 ms |
| part 1: footprint of one renderer | 195 MB | 177 MB |
| part 1: drop | 84.6 ms | 76.3 ms |

So the cuts don't hurt the activation block, the build, the footprint or the drop. And, as said below, the
gate as built today doesn't see them: P2 over P1 stays where it was.

## No sound changed

- Live output, p50 from 3:00, 31 s: every build and layout gives the same hash (`d611a73d6944dd71`):
  S, the platform base, SV1, HEAD P1 and P2, and every cut on P1 and P2 (`scripts/hashes.sh`).
- The 8 recorded renders match `e2e/renders.sha256`, and the 4-minute p16 render is
  `ebc87e0b57c0cbb5a019ef3bdf2157d36c5343d25559268327ff538dfb56da5e`, for HEAD, HV1, HV3, HV4 and
  HV1+HV3+HV4 (`scripts/renders.sh`; p16 made with `cargo run -p m2-server --example reference -- <dir>
  --tracks 16`, then `m2 render`, as `~/m2-runs/g7f-linux/renders.sh` did).
- `cargo test -p m2-runtime -p m2-play` (debug, with the test plugins): 174 passed, 0 failed, on HEAD and
  on all cuts together (two test call sites adapted to the prototype's changed internal signatures).
  That includes `bit_identity` and `no_alloc` (RENDER-28).

## Problems and cautions

- Linux numbers only, on the 96 MB-cache half of the chip. Only the Mac's count.
- The gate as built (P2 over P1 here, plus P1's recorded cost) would not show these gains: they speed up
  P1 and P2 alike. It needs part (2) of the owner's decision, S measured in the same run.
- My first campaigns (A, B, C on CPU 12) are discarded: builds, my callgrind runs and another agent's gate
  runs on the same half of the chip disturbed them. Also, a target folder shared by two source copies
  mixed their builds up (cargo names a path crate by its path inside the workspace, so the same crate of
  two copies is one artifact), so some early binaries were the wrong code. Everything above was rebuilt
  with one target folder per copy, and the hashes, md5 sums and callgrind counts confirm the builds differ.
- Callgrind's L2 model indexes by virtual address. S's and P1's 4 GiB-aligned memories then collide in the
  model in a way real caches (physically indexed) don't, so the JIT rows' L2 misses are not to be trusted;
  the host rows are.
- The HV3 prototype skips the debug ABI-1 input checks for kernels in a pool.
- Another agent (sg) was running G7f against S on the other half of the chip at the same time. My runs on
  CPU 4 share memory bandwidth with it, not caches.

## Tools, and how to run it again

Built for this, kept:
- `~/m2-runs/tools/valgrind/inst/bin/valgrind`: valgrind 3.27.1 from source, two limits raised so it
  copes with thousands of 4 GiB reservations (`coregrind/m_aspacemgr/aspacemgr-linux.c`:
  `aspacem_maxAddr` 127 TiB, `VG_N_SEGMENTS` 400,000). Source in `~/m2-runs/tools/valgrind/src`.
- `~/m2-runs/tools/addr2line/bin/addr2line`: gimli's, for inlined frames' names.

In `/home/jerry/m2-runs/speed/prof`:
- `src-<tree>`: pristine `git archive` copies (S `8aa301e`, plat `b90bacf`, head `ed7fc8e`; S built on
  Linux with no change but the harness) and the variants: `sv1`, `hv1`, `hv3`, `hv4`, `hv13`, `hv134`,
  `hv1345`, and `-prof` copies (call counters, wasmtime's perf map on `M2_PERFMAP`, the sampler).
- `harness/cbench.rs`: arena.rs's `callbacks()` with S's API only, plus a lockstep mode and an output-hash
  mode; `harness/cprof/`: the sampler (`sampler.rs`, CPU-time `timer_create`, frame-pointer walk).
- `patches/`: `hv1.diff`, `hv3.diff` (and `hv3.py`), `hv4.diff` (and `hv4.py`), `hv134.diff`, `sv1.diff`, and
  `hv1345-all-cuts.diff` (all four against HEAD's `core/runtime/src`, with the two test call sites).
- `bins/`: the built binaries. The target folders were deleted.

Re-run:
1. Build: `./build.sh s head hv1345` (each copy into its own `target-<tree>`; never share one), and
   `./build-prof.sh head` for the profiling binaries.
2. Timing: `python3 scripts/lockstep.py <name> 6 4 s:s head:p1 head:p2 hv1345:p2`, then
   `python3 scripts/lsdiff.py logs/ls-<name>.log s:s`.
3. Profile: `scripts/profiles.sh <tag> 2 4 s:s head:p1 head:p2`, then
   `scripts/symprof.py --us <mean> --json <tag>-head-p1.json bins/cprof-head profiles/<tag>-r0-head-p1 profiles/<tag>-r1-head-p1`
   and `scripts/pfdiff.py owner <s>.json <head>.json`.
4. Instructions and misses: `scripts/vgall.sh <tag> 3 m1 s:s head:p2 hv1345:p2` (about 6 minutes), then
   `scripts/cgdiff.py 372 vg/<tag>-s-s.out vg/<tag>-head-p2.out vg/<tag>-hv1345-p2.out`.
5. Sound: `scripts/hashes.sh 30 s:s head:p2 hv1345:p2`, `scripts/renders.sh hv1345`, `scripts/tests.sh hv1345`.
