# Plan: make step 7's engine as fast as S again (label design)

2026-10-16. Linux numbers are information only (PLAT-1). Only the Mac judges.

## The plan in one table

| # | Cut | Where | Linux saving, a 512-frame callback | Risk | Rules it touches | Decision |
|---|---|---|---|---|---|---|
| 1 | Keep each kernel memory's address in the instance. Stop asking the store for it twice a call. | `core/runtime/src/lib.rs`: `KernelInst`, `KernelInst::new`, `KernelNode::process`, `Graph::prewarm` | about −180 µs alone (5 runs, −217 to −146) | low | RENDER-32, RENDER-28, ABI-1, RENDER-22 | **do it, first** |
| 2 | Keep the kernel instance inline, not in a `Box`. | `lib.rs`: `KernelNode::inst`, `Builder::node` | about −60 to −80 µs more | low | RENDER-25, RENDER-28, RENDER-16 | **do it, second** |
| 1+2 | Both together, measured today | | **−247 µs** (6 runs, −276 to −202); in G7f's own gate **−239** (+223 → −16) | | | |
| 3 | One wasmtime `Store` per voice pool | | about −95 µs more | medium | changes RENDER-32's words | **not now**: the owner's call, only if the Mac needs it |
| 4 | The non-finite check at activations, not every block | | −14 to −21 µs | low to medium | RENDER-25, RENDER-10 | **not now**: too small to see |
| – | The rest of step 7's bookkeeping | | under 5 µs | | | nothing to cut |
| – | O1's bounds checks (the owner's layout) | | +85 to +105 µs (Mac +141) | | | stays, as the owner chose |

**Expected result with cuts 1 and 2:**

| G7f, the app against S, by the mean | Linux today (pinned) | Linux, prototype of cuts 1+2 (pinned) | Mac today (estimate) | Mac with cuts 1+2 (expected) | Bound |
|---|---|---|---|---|---|
| part 2, a callback | +223 µs (+207 to +240) | **−16 µs** (−31 to −10) | +328 | **about +20** (plausible worst about +175) | 320 (aim 270) |
| part 1, a block | +58 µs (+55 to +59) | **−1 µs** (−9 to 0) | +73 | **about +20** (plausible worst about +57) | 80 |

No sound changes. The prototype of cuts 1 and 2 plays the same bits as S and HEAD. The 8 renders match. The p16 render is `ebc87e0b…`. The runtime and play tests pass, 174 of 174.

## 1. What I measured today

The machine was quiet (load 1.3 to 2). Every timed process was pinned to the half of the chip with the 96 MB cache (CPU 4, or CPUs 0-7,16-23). I built one prototype: cuts 1 and 2 on HEAD (`hv15`). I timed it two ways.

**A. The profile's lockstep harness** (6 runs; each run uses fresh processes taking turns of 94 callbacks; p50 from 3:00; 30 s each). Differences are paired run by run: median (lowest to highest).

| Build : layout | Against S | Against HEAD, same layout |
|---|---|---|
| HEAD : P2 (the app) | +251 (+224 to +265) | |
| HEAD : P1 | +157 (+144 to +182) | |
| cuts 1+2 : P2 | **+5** (−23 to +48) | **−247** (−276 to −202) |
| cuts 1+2 : P1 | **−102** (−120 to −94) | **−262** (−292 to −247) |
| all four cuts : P2 (for comparison) | −105 (−118 to −61) | −355 (−367 to −308) |

S's mean callback: 6,995 µs. O1's own cost (P2 against P1): +85 (+67 to +109) at HEAD, +105 (+84 to +157) with the cuts.

**B. G7f's new fresh gate** (branch s7-sg at `8177eb2`: S in its own process, 5 rounds). It ran from two scratch copies: the branch as it is, and the branch with cuts 1 and 2. They ran one after the other, alternating.

| | Branch as it is | With cuts 1+2 |
|---|---|---|
| part 2: the app against S | +223 µs (+207 to +240) | **−16 µs** (−31 to −10) |
| part 2: P1 against S | +140 (+125 to +153) | −108 (−119 to −86) |
| part 2: the app against P1 (O1) | +87 (+70 to +95) | +87 (+75 to +104) |
| part 2: S's mean callback | 6,992 µs | 6,981 µs |
| part 2: slowest activation block (bound 5,333) | 2,525 µs | 2,226 µs |
| part 1: the app against S | +58 µs a block (+55 to +59) | **−1 µs** (−9 to 0) |
| part 1: P1 against S | +36 (+35 to +40) | −28 (−32 to −26) |
| part 1: the app against P1 (O1) | +22 (+18 to +24) | +26 (+23 to +27) |
| part 1: full build of p50 | 216 ms (S 181) | 215 ms (S 183) |
| the same music (all three sides, every round) | yes | yes, the same hashes |

The same-music hashes were the same in both copies: `24943acbd30d0419` for callbacks and `876855c9f5f9baad` for blocks.

**Sound checks on the prototype:**

- Live output, 31 s of p50 from 3:00, hash `a22ffc5a2040407d` for S, HEAD P2 and the cuts on P2 and on P1.
- `e2e/baseline.sh`: 8 of 8 match `e2e/renders.sha256`.
- The p16 render is `ebc87e0b57c0cbb5a019ef3bdf2157d36c5343d25559268327ff538dfb56da5e`.
- `cargo test -p m2-runtime -p m2-play`, debug, with the test plugins: 174 passed, 0 failed. That includes `bit_identity`, `no_alloc` and `churn`.

## 2. The target, and the arithmetic

**The aim:** on the Mac, part 2's verdict (the app against S, by the mean) at **270 µs or less**, clearly under the 320 µs bound. Part 1 stays clearly under 80 µs a block.

### What the Mac shows today

At `ed7fc8e` the gate added two numbers:

- part 2: +141 µs (O1, measured fresh) + 187 µs (step 7 at P1, recorded 2026-10-14) = **+328 µs**;
- part 1: +41 + 32 = **+73 µs** a block.

The +187 is old. Today's real step-7 cost on the Mac is unknown.

### Linux-to-Mac ratios, from costs measured on both

| Cost | Mac | Linux | Ratio, Mac ÷ Linux |
|---|---|---|---|
| Step 7's own cost (P1 against S), a callback: the cost the cuts remove | +187 (old) | +140 (gate, today), +157 (lockstep, today); other runs +122 to +182 | 1.34, 1.19 (range 1.03 to 1.53) |
| O1's own cost (the app against P1), a callback | +141 (+111 to +177) | +87 (gate), +85 (lockstep); one earlier run +50 | about 1.6 to 1.7 (up to 2.8) |
| Step 7's own cost, a block | +32 (old) | +36 (today); earlier +29 to +41 | 0.89 (0.78 to 1.10) |
| O1's own cost, a block | +41 (+37 to +48) | +22 (today); earlier +18 to +20 | about 1.9 to 2.3 |
| A whole S callback | 8,936 µs | 6,990 µs | 1.28 |

### Part 2 on the Mac, two ways

**Way A: scale the saving.** Mac after = Mac today − ratio × Linux saving.

- With the gate's numbers: 328 − 1.34 × 239 = 328 − 320 = **+8 µs**.
- With the lockstep numbers: 328 − 1.19 × 247 = 328 − 294 = **+34 µs**.
- With the worst ratio and the smallest saving: 328 − 1.03 × 202 = 328 − 208 = +120 µs.

**Way B: build it from its parts.** Mac after = O1 on the Mac + ratio × (P1 against S after the cuts, on Linux).

- In the gate, O1 did not change with the cuts (+87 and +87). In the lockstep it grew from +85 to +105 (× 1.24). So O1 on the Mac is 141 to 175 µs.
- P1 against S after the cuts, on Linux: −108 (gate), −102 (lockstep). Times 1.19 to 1.34, that is −121 to −145 µs.
- So the Mac lands at 141 − 145 = **−4** up to 175 − 121 = **+54 µs**.

**Expected: about +20 µs a callback** (Way A: +8 to +34; Way B: −4 to +54).

**Pessimistic cases:**

- Suppose none of the gain below S carries over to the Mac, and only step 7's own cost goes. Then the Mac lands at O1 alone: 141 to 175 µs. That is still under 270.
- To miss 270, the cuts would have to remove less than half of step 7's own cost on the Mac: P1 would still have to be more than about 95 µs over S. On Linux they remove 1.7 to 1.8 times that cost (262 of 157 µs; 248 of 140 µs). On the Mac, step 7's cost is the same kind of cache misses, and it costs more there than on Linux, not less. So a miss is unlikely. Only the Mac run can rule it out.

### Part 1 on the Mac

- Way A: 73 − 0.89 × 59 = 73 − 52 = **+21 µs** a block.
- Way B: O1 is 41 to 48 µs (it grew × 1.18 here). P1 against S is 0.89 × −28 = −25. So +16 to +23 µs.
- Pessimistic: no gain below S carries over, and O1 is at the top of its range and grows as it did here: 48 × 1.18 = **+57 µs**. That is still under 80.
- **Expected: about +20 µs a block.**

Without the cuts, part 1 is at risk on the Mac. The old method gave +73 against a bound of 80, and Linux's fresh number is +58. The cuts fix part 1 too.

### What Linux must show before the Mac run

Measure with G7f's gate by hand, pinned to CPUs 0-7,16-23, one run of 5 rounds.

| | Target | Prototype gave |
|---|---|---|
| part 2: the app against S | **≤ +60 µs a callback** | −16 |
| part 2: P1 against S | ≤ 0 | −108 |
| part 1: the app against S | **≤ +20 µs a block** | −1 |
| part 1: P1 against S | ≤ 0 | −28 |
| the same music, both parts | the same bits on all three sides | yes |
| slowest activation block | under 5,333 µs, about as today (2.2 to 2.5 ms) | 2,226 |

Why these numbers:

- After the cuts, what remains is mostly O1. O1 has the highest Mac-to-Linux ratio we have seen, up to 2.8.
- Part 2: 60 × 2.8 = 168, under 270.
- Part 1: 20 × 2.8 = 56, under 80.
- The prototype is about 75 µs (callback) and 20 µs (block) under these targets. Pinned runs swing by about ±30 µs a callback, so that room is enough.

Unpinned Linux runs (ci/check.sh) swing much more (+19 to +125 a block, +144 to +396 a callback). They are information only.

## 3. Cut 1: keep each kernel memory's address

**The waste.** `KernelNode::process` calls `k.memory.data_mut(&mut k.store)` twice for each kernel call: once for the inputs, once for the outputs. That is about 9,250 calls a callback. Each lookup reads the store's id, its instance table, the instance and the memory's definition. At S all of that was one hot store. Since step 7 there are 6,058 stores, so these reads miss the cache. The profile counted this cost at +188 µs of host self time (`assert_belongs_to` +98, the store's instance entry +70, `Instance::offsets` +35, and smaller items).

**What changes** (`core/runtime/src/lib.rs`):

- A small `MemView { base: NonNull<u8>, len: usize }` field in `KernelInst`. It is taken at the end of `KernelInst::new` from `memory.data_ptr(&store)` and `memory.data_size(&store)`.
- `fn bytes<'a>(&self, memory: &Memory, store: &'a mut Store<StoreLimits>) -> &'a mut [u8]` returns the slice. It takes the store by `&mut` only as a borrow, never reading it, so the slice can't outlive the next call into the kernel, just like wasmtime's own `data_mut`. Debug builds assert there that `data_ptr` is still `base` and `data_size` is at least `len`.
- In `KernelNode::process`, both `k.memory.data_mut(&mut k.store)` become `k.view.bytes(&k.memory, &mut k.store)`.
- `Graph::prewarm`, after each kernel's first reset (the only reset allowed to grow its memory): a check in release builds too that `data_ptr` is unchanged. If it changed, building fails with a plain error ("a kernel memory moved", RENDER-32). This runs once per instance, off the audio thread.
- `unsafe impl Send for MemView` (only for the view, not the whole instance), with a SAFETY comment. There is one `unsafe` block, in `bytes`.
- The prototype (`/home/jerry/m2-runs/speed/design/hv15.diff`) uses bare fields and `unsafe impl Send for KernelInst`. The real code should use the narrower `MemView` above.

**Why it is safe.**

- `StoreLimits` caps every kernel memory at `KERNEL_MEMORY` (32 MiB).
- Both layouts reserve at least that much:
  - P2 (the app's) reserves exactly 32 MiB, with no growth room and `memory_may_move(false)`;
  - P1 reserves 4 GiB.
- So a memory always grows in place and never moves. A memory never shrinks.
- The port pointers were checked against the size at build time, so a longer memory later still holds them.
- The store sits in the same `KernelInst`. Moving the instance (cut 2, or an activation's swap) moves only the store's handle, not the memory.

**Why it is bit-identical.** The host reads and writes the same bytes at the same addresses the store would give. Nothing changes in arithmetic, call order, the compile settings, std.wasm or std's kernels. The Cranelift code is the same, because the engine's config is unchanged.

**Rules it touches.**

- RENDER-32: the layout is unchanged, but "never moves" becomes something the host relies on. Add a sentence to RENDER-32's Today line.
- RENDER-28: less work on the audio thread, nothing allocated, no lock, no wait.
- ABI-1: ports are checked as before; the debug input checks still go through the store.
- RENDER-22: constants are still written once.

**Tests.**

- Existing tests guard the sound: `core/play/tests/bit_identity.rs` (RENDER-33, G7c), `no_alloc.rs` (RENDER-28), `churn.rs`, the runtime tests (`prewarming_keeps_kernel_memory_flat` grows a delay's memory from 896 to 1664 KiB in place), G7b, the e2e renders, the p16 hash, and G7f's same-music check against S.
- New: `a_kernel_memory_never_moves` (RENDER-32) in `core/runtime/src/tests.rs`, using the `package(wat)` helper. A WAT kernel grows its memory by one page on every `k_process` call, up to the 32 MiB cap, and copies its input to its output. The test renders enough blocks to reach the cap, under both `Layout::P2` and `Layout::P1`. It checks that the output is right every block and that `data_ptr` never changed, with the debug assert on.
- Negative control, once, in a scratch copy: a layout that may move (P2's reservation set to 64 KiB with `memory_may_move(true)`). The new test must fail, through the prewarm error or the debug assert. That shows the guard can see a move.
- For the speed itself, the gate is the test. G7f fails on the Mac without the cuts (+328 against 320). No unit test can time it (PLAT-1).

**Expected saving.** About 180 µs a callback on Linux at P2 (campaign C2: −181, 5 runs, −217 to −146), and 192 at P1 (campaign D: −219 to −145). About 45 µs a block in part 1 (callback ÷ 4).

## 4. Cut 2: keep the instance inline

**The waste.** `KernelNode::inst` is `Option<Box<KernelInst>>`. Each call chases one more cold pointer. S kept the instance inline in the exec list.

**What changes** (`core/runtime/src/lib.rs`): `inst: Option<KernelInst>`. `Builder::node` builds `Some(KernelInst::new(…)?)` without `Box::new`. Everything else stays as it is: `verify`, `take_from`'s `mem::swap`, `reset_node`, `prewarm`, `active`, `kernel_memory`. The memory view stays valid, because the memory lives in the store's own allocation, not in the struct.

**Why it is safe.**

- An activation still moves the instance by `mem::swap`. It copies about 250 bytes instead of 8 for each moved kernel, a few hundred kernels at most. Nothing is allocated or freed (RENDER-25, RENDER-28).
- The empty slot still leaves with the old plan.
- Instances still drop with their plan, off the audio thread, and `LIVE_KERNELS` still counts them (§7.6).
- Voice pools stay boxed and move whole (RENDER-16).

**Why it is bit-identical.** The same code runs in the same order. Only where the struct sits in memory changes.

**Tests (existing).**

- `no_alloc.rs`: activations, with plugins too, allocate nothing.
- The runtime's RENDER-28 activation test in `tests/live.rs`.
- `churn.rs`: nothing leaks over 55 activations; p16's 200 edits stay flat.
- G7b: untouched tails are bit-identical through activations.
- `bit_identity.rs`'s G7c tests: activations change no bit.

**Expected saving.** About 60 to 80 µs a callback on top of cut 1. Measured −82 (−116 to −68) on top of cuts 1, 3 and 4. Cuts 1 and 2 together save 247 (lockstep) or 239 (gate), against about 180 for cut 1 alone.

## 5. Held back, and why

**Cut 3: one Store per voice pool** (profile `patches/hv3.diff`).

- It would save about 95 µs more on Linux. Lockstep: all four cuts give −105 against S, cuts 1 and 2 give +5; the difference is −95 (−166 to −82), and it includes cut 4's ~15.
- Not now. RENDER-32 says "one `Store` per kernel instance", and the owner chose that layout. design-7 §11.1 chose per-instance stores by elimination. A pool's store would leak nothing, because a pool is built, moved and dropped whole, but that is still a change to the owner's words.
- It is also medium risk. The store must go through `Graph::process`, `reset`, `active`, `prewarm` and `kernel_memory`. The debug ABI-1 checks need the pool's store; the prototype skips them. A pool's `StoreLimits` counts many instances.
- Bring it to the owner only if the Mac misses the aim after cuts 1 and 2.

**Cut 4: the non-finite check at activations** (profile `patches/hv4.diff`).

- It saves 14 to 21 µs. That is at the edge of what the timing can see, though it is 316,000 instructions a callback.
- Not now. It changes when RENDER-25's check runs. The prototype is not exactly the same as today in three cases:
  - a box that trapped stays marked after a later finite block;
  - a reset followed by an activation with no block between would scan stale outputs;
  - strict blocks.
- An exact version is possible: store the trap mark every block, and keep a "fresh since reset" flag beside the scan at the activation. But it is not worth the risk for a gain the Mac can't see on its own.

**Nothing else is worth cutting.** The rest of step 7's bookkeeping in the callback path costs under 5 µs and under 0.1 M instructions a callback: the player's atomics, window and hold checks, `end_ramps`, carried cells, `Option` lanes, the fade version. The platform step costs nothing (−2 µs, −42 to +39).

**O1 stays as built:** 32 MiB reservation, 64 KiB guard, no growth room, `memory_may_move(false)`, Spectre guard off. Its bounds checks are what is left over S: +9.6 M JIT instructions a callback.

## 6. Order of work

1. **Branch.** A new branch `s7-speed` from s7-sg at `8177eb2`, in worktree `/home/jerry/m2-runs/wt/s7-speed`. s7-sg is the base because it measures S fresh. The lead decides how it joins step7.
2. **Commit A, cut 1:** `MemView`, both lookups, the prewarm check, the new test `a_kernel_memory_never_moves`. Then run `cargo test -p m2-runtime -p m2-play` (debug, test plugins, scratch HOME, `M2_AUDIO=none`), and the negative control in a scratch copy.
3. **Commit B, cut 2:** the inline instance.
4. **Measure on Linux, pinned** (section 7), after each commit with the lockstep harness, and with the gate after both. Check against the targets in section 2.
5. **Sound:** live hashes, `e2e/baseline.sh` 8 of 8, the p16 hash `ebc87e0b…`, G7f's same-music lines.
6. **Docs:** build-notes.md rows (finding, what was done, test, commit). RENDER-32's Today line: "the host keeps each memory's address from its build; it never moves", with the new Linux numbers. Leave design-7's 2026-10-15 paragraph as the record it is.
7. **`ci/check.sh --gates` on Linux** (about 23 min).
8. **The owner's Mac run of `ci/check.sh`.** About 32 min the first time (sg's estimate; the cuts don't change the time), under 45.
9. **Clean up:** delete the scratch `target/` folders, keep the logs.

## 7. How to measure

All runs: scratch HOME, `M2_AUDIO=none`, `CARGO_HOME=/home/jerry/.cargo RUSTUP_HOME=/home/jerry/.rustup`. Jobs over 20 s go through `~/m2-runs/tw`.

- **G7f's gate by hand** (the number the Mac judges).
  - Make a scratch copy with `git archive`, build `m2-gates` into its own target, then run:
  - `/home/jerry/m2-runs/speed/design/gates.sh <tree> arena`, then `… store`.
  - The script pins to 0-7,16-23, uses S's runner from `~/.cache/m2-check/s-8aa301e`, and logs to `design/logs/`.
  - Or use sg's commands with `taskset -c 0-7,16-23` in front.
  - Read the "the app's layout against S", "P1 against S" and "the same music" lines.
  - About 5 min for arena and 6 for store.
- **The lockstep harness** (quick, paired, per commit).
  - Copy `crates/gates/src/bin/cbench/` from `/home/jerry/m2-runs/speed/design/src-hv15` into the scratch tree.
  - Build it with `design/build.sh <tree>`, which uses its own target.
  - Run `python3 scripts/lockstep.py <name> 6 4 s:s head:p2 <tree>:p2 head:p1 <tree>:p1` from `/home/jerry/m2-runs/speed/design`.
  - About 20 min. Expect `<tree>:p2` near +5 against S, and about −247 against `head:p2`.
- **Cache misses** (exact, optional). Use the profile's `scripts/vgall.sh <tag> 3 m1 s:s head:p2 <tree>:p2` and `cgdiff.py`. Host-side L2 misses should fall from about 11.1 K a callback toward S's 3.4 K. Host-side L1 misses should fall from 133 K toward 100 K or below.
- **Sound.**
  - `design/scripts/hashes.sh 31 s:s head:p2 <tree>:p2 <tree>:p1`: all the same hash.
  - `design/scripts/renders.sh <tree>`: 8 of 8, and p16 `ebc87e0b…`.
  - `design/scripts/tests.sh <tree>`: 174 or more passed, 0 failed.

## 8. If the Mac misses

- **Part 2 between 270 and 320, or either part within 60 µs (callback) or 30 µs (block) of its bound:** run again before deciding (sg's advice).
- **Part 2 still over 320:** bring cut 3 to the owner, because it changes RENDER-32's words. Then consider cut 4 (the exact version).
- **Part 1 over 80:** the same steps. Cuts 3 and 4 together look worth about 20 to 25 µs a block more. That comes from comparing two different runs: the profile's gate run saved −82 a block with all four cuts, and today's gate run saved −59 with cuts 1 and 2.

## 9. Risks

- **The Mac's ratio is inferred.** It rests on one old Mac number (+187, 2026-10-14) and Linux runs. The pessimistic cases above still land under 270, but only the Mac run can say for sure.
- **O1 may grow on the Mac** once the store misses are gone. The lockstep saw +85 → +105 on Linux; the gate saw no change. The estimates allow for up to × 1.24.
- **New `unsafe` code.** One block, and a `Send` claim on the view. It rests on "memories never move", which these guard: the cap is no bigger than the reservation, the release check at prewarm, the debug assert on each call, and the new test with its negative control.
- **A later layout.** RENDER-32's Deferred row ("another Store layout measured") must keep "never moves", or drop the view. The prewarm check would catch a layout that moves.
- **Linux only so far.** Every timing here comes from one machine, on its 96 MB-cache half.

## 10. Files

- This plan: `/home/jerry/m2-runs/speed/plan.md`.
- Prototype diff of cuts 1+2: `/home/jerry/m2-runs/speed/design/hv15.diff` (73 lines, against `ed7fc8e`'s `lib.rs`; it applies unchanged to s7-sg).
- Prototype trees: `/home/jerry/m2-runs/speed/design/src-hv15` (HEAD + cuts, with cbench) and `src-sg15` (s7-sg + cuts).
- Logs:
  - `/home/jerry/m2-runs/speed/design/logs/ls-F.log` (lockstep);
  - `gates-sg-{arena,store}-*.log` and `gates-sg15-{arena,store}-*.log` (G7f by hand);
  - `tests-hv15.log`;
  - `renders/hv15/`.
- Scripts: `/home/jerry/m2-runs/speed/design/{build.sh,build-gates.sh,gates.sh,scripts/}`.
- The profile's study: `/home/jerry/m2-runs/speed/prof/results.md` and `prof/patches/` (cuts 3 and 4).
- Scratch `target/` folders are deleted. `/home/jerry/m2-runs/speed/design` is 50 MB.

## Attack (label attack, 2026-10-16)

I tried to break the plan before anyone builds it. Linux numbers are information only (PLAT-1). What I ran is in
`/home/jerry/m2-runs/speed/attack/logs/`: clippy on the prototype, the sizes of the types, two lockstep campaigns,
and the recorded tests. The scratch `target/` is deleted.

### The verdict in one table

| Cut | Verdict | Why, in short |
|---|---|---|
| 1, keep the memory's address | **keep, with 5 changes** | Safe, no sound change, and the saving is real. It is bigger where the cache is small, so the Mac should get it too. The helper as written can hand out two `&mut` slices of one memory, so it needs a sound shape. |
| 2, the instance inline | **change: keep only with 3 fixes, and only if its own saving shows up** | As built it **fails clippy**, so it fails ci/check.sh. The plan's reason is wrong: S boxed its kernel node. Its own saving is smaller than the plan says, and on the small-cache die it is no bigger than the noise. The aim doesn't need it. |
| 3, one Store per voice pool | **hold, as planned** | Agree. It changes RENDER-32's words, so it is the owner's call. It is the next step if the Mac misses. |
| 4, the non-finite check at activations | **hold, as planned** | Agree. Too small to see, and not exact as prototyped. |

### What I ran

- **clippy on the prototype** (s7-sg + cuts 1 and 2), `cargo clippy --locked -p m2-runtime --all-targets -- -D warnings`:
  **fails**. `error: large size difference between variants` on `enum Exec` (lib.rs:763): "the largest variant
  contains at least 464 bytes", "the second-largest variant contains at least 52 bytes". With cut 1 alone it is
  clean. (`logs/clippy-sg15.log`)
- **Sizes**, from `size_of` in a release test: `Exec` is 64 bytes with cut 1 alone and **392 bytes** with cut 2.
  `KernelInst` is 344 bytes.
- **Lockstep**: the design's `lockstep.py` in my own folder, with the profile's binaries `cbench-s`, `-head` and
  `-hv1` (cut 1 alone) and the design's `-hv15` (cuts 1 and 2). Every side runs in its own process. 6 runs of 30 s,
  p50 from 3:00. The machine was quiet (load 0.7 to 1.9). Each cell is the median of the pairs (lowest to highest).

| | CPU 4 (the die with the 96 MB cache) | CPU 12 (the die with the 32 MB cache) |
|---|---|---|
| HEAD against S | +244 µs (+166 to +281) | +275 (+91 to +347); **+316** without runs 3 and 4 |
| cut 1 against HEAD | **−187** (−232 to −125) | **−246** (−341 to −206) |
| cut 2 on top of cut 1 (paired) | **−51** (−78 to −23) | **−22** (−91 to +20); 3 of 6 pairs worse |
| cuts 1 and 2 against HEAD | −249 (−270 to −161) | −318 (−345 to −194) |
| cut 1 alone against S | +53 (+28 to +78) | +64 (+35 to +86), without runs 3 and 4 |
| cuts 1 and 2 against S | **+5** (−30 to +36) | **+32** (−36 to +106), without runs 3 and 4 |

  In runs 3 and 4 on CPU 12, S's own process was 150 to 250 µs slow. The lines against HEAD and against cut 1 don't
  use S, so they keep all 6 runs. (`logs/ls-atk-ccd0.log`, `logs/ls-atk-ccd1.log`)
- **The recorded tests**, on the prototype, in release, with the test plugins and a scratch HOME:
  `cargo test -p m2-play --release --test bit_identity --test live --test churn -- --ignored`. **All 5 pass:**
  `a_no_op_activation_on_p50`, `live_is_the_export_on_all_of_p16`, `live_is_the_export_on_p50`,
  `p16_churns_200_edits_within_20_mb` (180.8 MB after 70 edits, then 175.4 MB: −5.4 MB) and
  `p16_loops_ten_minutes_without_drift`. The plan's test run skipped all 5 of them: they are `#[ignore]`, and
  ci/check.sh doesn't run them either. Yet they are the only tests of activations and churn on p50 and p16, which
  the plan names as cut 2's guards. (`logs/recorded-sg15.log`)

### Cut 1: keep, with changes

What I tried, and what held:

- **Can a memory move?** No, in either layout. I read wasmtime 49.0.2 (`runtime/vm/memory.rs`, `LocalMemory::grow`;
  `memory/mmap.rs`). A memory that grows inside its allocation never moves, and wasmtime asserts that itself.
  P2 allocates its 32 MiB up front, and `memory_may_move(false)` refuses any growth past it. P1 allocates 4 GiB,
  wasmtime's default reservation for a 32-bit memory. `StoreLimits` caps every memory at 32 MiB. The copy-on-write
  image only changes protections in place. Every engine comes from `engine_with`, so there is no third layout.
- **Ports.** They are checked against the size at build. The kept length is that size. A later growth, like the
  delay's first reset (896 to 1664 KiB), can't put a port out of reach.
- **Sound.** The same bytes are read and written at the same addresses, in the same order. No slice lives across the
  call into the kernel. The 8 renders, the p16 hash, the live hashes, the plan's 174 tests and my 5 recorded tests all
  agree.
- **The audio thread.** Nothing is allocated and nothing waits. It only does less reading.
- **Activations, fades, holds, exports, plugins, loops, seeks.** The view lives in its instance, beside its store,
  and moves with it. Nothing else holds it. The recorded p50 and p16 tests pass.
- **The saving.** It is real, and I reproduced it: −187 µs on the 96 MB die (the profile had −181). It is **bigger on
  the 32 MB die (−246)**. The Mac's caches are small too (12 MB of L2 for each group of 4 cores), so the Mac should
  save at least as much.

What to change:

1. **The helper must be sound by its shape.** As written, `fn bytes(&self, memory, store: &'a mut Store)` is a safe
   function. It hands out `&mut [u8]` of this memory to anyone who passes *any* `&mut Store`. With another
   instance's store, two `&mut` slices of one memory could exist. Tie the slice to its owner instead: a `KernelInst`
   method taking `&mut self` that returns the bytes together with the port lists (and the name, for the strict
   error). Then `&mut self` proves that nothing else holds the memory. The other way is to make `bytes` an
   `unsafe fn`, with "the store that owns this memory" as its SAFETY rule.
2. **One debug check per call: `data_ptr == base`.** A memory never shrinks, so `data_size >= len` adds nothing.
   With both checks, a debug build makes 4 store lookups a kernel call instead of today's 2, so
   `cargo run -p music2-desktop` (debug assertions on) would get slower than today.
3. **Say what the prewarm check covers.** It only sees a move during the first reset. A later move would be seen only
   by the debug check. What really guards release builds is the layout, the 32 MiB cap, and the new test under both
   layouts with its negative control.
4. **Add the recorded tests** (the command above, about 8 minutes) to commit A's checks, and again after commit B.
5. **Run `cargo clippy --locked --workspace --all-targets -- -D warnings` right after commit A**, not only in step 7.
   `NonNull` makes `KernelInst` no longer `Sync`. Nothing seems to need it: the prototype built m2, m2-gates and the
   server's example. But the desktop crate was not built.

### Cut 2: change

1. **It fails clippy as built.** ci/check.sh's clippy row and its mac lint row would both fail on
   `large_enum_variant`. Boxing the instance again would undo the cut. The fix that keeps it is
   `#[allow(clippy::large_enum_variant)]` on `enum Exec`, with a short "why" comment. That is the repo's style: see
   `#[allow(clippy::too_many_arguments)]`.
2. **The reason given is wrong.** S did not keep the instance inline. At 8aa301e it is `Exec::Kernel(Box<KernelNode>)`:
   one box for each kernel node, holding its memory, its functions and its port lists. HEAD today also follows one
   pointer to reach the instance, its `Box<KernelInst>`. So cut 2 is not "back to S". It goes past S, and it removes a
   cost that S paid too, not a cost step 7 added. §4 says "S kept the instance inline", "back to about the size S had"
   and "about 250 bytes". §2's Way B says the cuts remove step 7's cost. Correct all of these.
3. **Say what it costs.** Every entry of every exec list grows from 64 to 392 bytes (464 in debug). That includes
   constants and captures. One p50 renderer holds 2.4 MB more (195.4 to 197.8 MB in the design's gate logs). Each
   kernel an activation moves now copies about 344 bytes instead of 8. Only that swap happens on the audio thread, and
   it allocates nothing. All of this is fine, but the plan should say it.
4. **It saves less than the plan says.** Measured against cut 1 alone, in the same campaign, it saves −51 µs
   (−78 to −23) on the 96 MB die. On the 32 MB die it saves −22 µs (−91 to +20), and 3 of the 6 pairs were worse.
   The plan's "−60 to −80" came from subtracting two different campaigns, and from a run on top of cuts 3 and 4. The
   Mac's caches are small, so expect 0 to −50 µs from it there.
5. **The aim doesn't need it.** Cut 1 alone is +53 µs over S on the 96 MB die and +64 on the 32 MB die.

So: keep it as its own commit B, with the `allow`. Keep it only if the paired lockstep after commit B shows a saving
on **both** dies (cut 1 alone as the reference). If not, or if the lead wants no lint exception, drop it. Another
shape, only if the lead dislikes the `allow`: one `Vec` of instances per graph, with an index in each node. That keeps
`Exec` small and the instances side by side. But it touches `take_from`, `verify`, `reset_node`, `prewarm`, `active` and
`kernel_memory`, which is too much for about 20 to 50 µs. I don't recommend it now.

### Cuts 3 and 4: hold

I agree with holding both. One new fact: on the 32 MB die, what is left after cuts 1 and 2 is **+32 µs over S**, not
−16. The cost of calling into a cold store weighs more where caches are small, and that is cut 3's part. So if the Mac
misses, cut 3 is still the next step to bring to the owner.

### The arithmetic

- **Way A** starts from +328. That was made by the old method (O1 fresh, plus P1's old record). It is not what the
  fresh gate would say on the Mac today.
- **Way B** scales a negative number (the gain below S) by ratios measured for a positive cost. The 32 MB die shows
  that this gain shrinks when the cache does: +5 becomes +32.
- So expect **about +30 to +100 µs a callback on the Mac, not +20**. The plausible worst (about +175: O1 alone,
  grown) still stands. Both are under the 270 aim and the 320 bound. The conclusion holds. Only the point estimate is
  too precise.
- Part 1 was not measured here. The same caution applies.
- The 32 MB die is the nearer copy of the Mac: there, HEAD is +316 over S, close to the Mac's old +328.

### Add to "What Linux must show before the Mac run"

- The recorded tests in release, 5 of 5, after each commit.
- clippy `--workspace -D warnings` and the mac lint, after each commit.
- A lockstep run on **each** die after each commit (CPU 4 and CPU 12), with cut 1 alone as a side, so that cut 2's own
  number is paired.
- One pinned gate run on the 32 MB die too (`taskset -c 8-15,24-31`), the nearer Mac copy. For information, not a
  target.

### The sg comparison (G7f measured fresh against S)

I read `paired.rs`, `baseline.rs`, `arena.rs`, `store.rs`, `s/main.rs`, `s/wire.rs`, `s/build.sh`, `report.rs` and
ci/check.sh's rows. The method is sound. A few things lean the app's way, slightly.

1. **Is it fair across two processes? Mostly.** Both sides are timed the same way: a clock around `Driver::fill` or
   `render_block`, with the hashing and the drop off the clock. They take turns in every order, after a warm-up, and
   are built again each round. Both use the same wasmtime (49.0.2), the same crates (S's lock file differs only by
   `m2-os`), the same toolchain (1.99.0) and the same release profile. But:
   - S starts **every** turn just after a switch from the other process. The app and P1 do so for only half of their
     turns, and each gets one turn a cycle straight after its own. Any cost of waking a process (a cold core, the
     clock speeding up) falls on S more often. That makes the app look better.
   - The evidence says this lean is small. The pinned A/A gave −10 µs a callback. The gate is also kinder to the app
     than the lockstep, where every side has its own process: HEAD P2 +223 against +251, P1 +140 against +157, and
     with the cuts −16 against +5 and −108 against −102. These are different runs, so this only suggests it.
   - It was never measured on the Mac. Unpinned on Linux, the A/A gave +71 and +130, the other way.
   - So don't count the last 30 µs of margin as real. In the Mac log, tell the lead to read the "S as recorded" line:
     S measured now in its runner, beside S's record of 8,936 µs. If the two differ by more than about 1% (90 µs),
     doubt the runner before trusting a verdict near a bound. An A/A on the Mac (the `prep-controls.py` recipe) is
     worth one run only then.
2. **The same-music proof is sound, and it can fail.** A one-bit change fails it (unit test), and it compares every
   round with round 1. One gap: three sides that all played silence would pass it. p50 at 3:00 is not silent, and the
   printed means would show the missing work, so the risk is low. A "not silent" check would close the gap. It
   changes `wire.rs`, so the runner builds once more.
3. **A Mac verdict could pass without being judged.** If the Mac's machine line ever stops matching the records (a
   new Mac, or a different thread count), every fresh timing becomes a skip, and the gates row still PASSes, because
   `Outcome::verdict` passes on the other checks. On a Mac, this should fail the gates row and say: copy the machine
   line into both records. It is a small change in `paired::timed` and `store::rule`, or in check.sh. Not urgent: the
   line matched at ed7fc8e.
4. **It should build on the Mac.** The risk is low. S uses the same crates as HEAD, with `--locked`, offline first and
   online after. The first real link will be on the Mac, because Linux only type-checked macOS with stub-cc. If the
   build fails, check.sh fails the "old engine (S)" row and the gates row loudly. It never skips quietly.
5. **Can it fail when it should?** Yes. The busy-wait negative control failed both parts.
