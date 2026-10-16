# S's runner (G7f)

G7f ([RENDER-32](../../../docs/architecture.md#engine-shape-step-7)) asks what step 7 costs. It judges the app against **S**: HEAD `8aa301e`, the commit before step 7. Each G7f run measures S fresh, beside the app, over the same music, on the same machine. So no old number is added in. As for every timing, only the reference Mac judges it ([PLAT-1](../../../docs/architecture.md#platforms)): the Mac that the machine lines of `../store-S.txt` and `../callback-S.txt` name. Other machines print it. A Mac the records don't name also says what to do if it is the reference Mac now, and `ci/check.sh` marks its gates row PART, and its first line too.

S's code can't be built into today's m2-gates. So this folder holds a small program that plays S in a process of its own:

| File | What it is |
|---|---|
| `main.rs` | The runner. Written against S's API only, so it builds in a copy of S. It plays a player (G7f part 2) or a renderer (part 1) and times each callback or block. |
| `wire.rs` | What m2-gates and the runner say to each other: one line a command, one line an answer, over the runner's stdin and stdout. m2-gates compiles the same file (`baseline::wire`). |
| `build.sh` | Builds the runner (below). |

## How it is built

`build.sh` takes a clean copy of S out of git (`git archive 8aa301e`), so the repo and `.git` stay as they are. It adds this folder's runner to S's m2-gates as the example `s_runner`, and builds it in release with S's own `Cargo.lock` (`--locked`, and `--offline` first). It uses the Rust that `rust-toolchain.toml` names, so both sides have the same compiler. S builds as it is on macOS and Linux: no patch.

The copy and its build live in a cache folder, outside the repo:

- on a Mac: `~/m2-check/s-8aa301e`
- on Linux: `~/.cache/m2-check/s-8aa301e`

The copy is taken again only when the runner's code changes. On the owner's Linux server the first build takes about 40 s; after that, about a second.

`ci/check.sh` builds it as its own row ("old engine (S)") before the gates, and tells m2-gates where it is.

## How the two take turns

m2-gates starts the runner, sends a command, and waits for the answer. So only one of them runs at a time. Each round, three players (or renderers) take turns of about 2 s each, over the same music: the app's layout and P1 in m2-gates, and S in the runner. The order changes every turn, and no side takes two turns in a row. A round is 18 turns of each side: three whole cycles of the six orders. So in every round each side follows each other side as often, and the app's layout and S wait alike for their turns. (Some side must come back sooner than the others; it is P1, which isn't judged.) Each side hashes what its judged callbacks or blocks played. The gate checks that all three played the same bits.

A turn starts on the caches the side before it left, so its first steps are slow. On Linux the first callback of a turn took 4 to 7 ms more than the rest, and the first block 2 to 3 ms more. That costs each side a different amount. Most of it is gone after a few steps, but not all. One look by position showed this, held on the 96 MB-cache die of the Linux server's CPU. The app's layout has bounds checks, which cost more once the caches are warm. So over a turn's first 50 callbacks, its cost over S rose from about 0 to +60 µs a callback (and to +11 µs a block over the first 200 blocks). So each turn starts with half a second (47 callbacks, or 188 blocks) that is timed but not judged. It is sent in the same command as the turn, so nothing runs between them.

At first, half a second was shown to be enough only on that die. It wasn't tried on the other die, whose cache is 32 MB. Not held on one die, as `ci/check.sh` and the Mac run, S's first judged callback was still 42 to 121 µs slower, against its own mean, than the other sides' (three runs). One step can't say how long that lasts. So the gate prints two things about starting a turn, in one line, the first first (the gates' summary keeps 220 bytes of a line):

| Printed | What it says |
|---|---|
| of the judged steps, each turn's first half over its second, the app's layout's and P1's against S's | whether any start is left in the judged steps |
| each turn's first step, over the side's mean | what starting a turn costs each side (left out) |

If a side's start lasted into its judged steps, its first half would be slower than its second. So a value under 0 means S's start lasted longer: the judged means then favour the app's layout, by about half the value (when the start is gone by the second half). Each half holds a thousand steps or more a round, so this is far steadier than one step. (The music differs from step to step, by far more than that, but alike on every side, so only the comparison with S tells.)

What it read on Linux, one run each, at `33812c4` (the medians of 5 rounds, the app's layout first, then P1):

| Run | A callback | A block |
|---|---|---|
| Held on the 96 MB die | −6, −1 µs | −1, −0 µs |
| Held on the 32 MB die | −21, +60 | |
| Not held | +4, +23 | +7, +8 |
| The A/A control, not held | −48, −23 | −0, −10 |

So no start was seen left in the judged steps, held or not, beyond about 25 µs a callback on the mean (half the A/A's −48), and a few µs a block. Far under 0, below about −100 µs a callback or −30 µs a block, would say S's start lasted past the half second.

Before 2026-10-16 every step was judged, and the turns went through every order one after another. Then S always followed another side, while the app's layout and P1 each followed themselves in one turn of six. On Linux that made the app's layout look about 20 µs a callback faster than it was, and made most of the p99's difference.

## How far off the method itself can be

A control plays the app's own engine on both sides: in m2-gates, and in a runner built from the same code (an "A/A" run). It should come out at 0. Here is what it gave on the Linux server, the app's layout in m2-gates against the same engine in the runner, by the mean, as the gate judges (5 rounds a run):

| Turns | Held on one CPU die | Not held (as `ci/check.sh` runs it) |
|---|---|---|
| 2 s turns after half a second not judged (98fc493 to 651cad4): a block | −0 µs (−2 to +1) | |
| The same: a callback | +9 µs (+1 to +18); +14 (+4 to +17) | +73 µs (−105 to +152) |
| Today's: 18 turns a round, three whole cycles (`1b8ce99`; at `33812c4`): a block | | −1 (−20 to +28) |
| Today's: a callback | | −74 (−207 to −48) |
| 1 s turns, every order one after another (before 2026-10-16; the second of each pair with each turn's first step left out): a block | −4 (−5 to +1); +2 (+1 to +3) | +11 (−3 to +20); −6 (−8 to +11) |
| The same: a callback | −10 (−14 to −3); +7 (−5 to +18) | +130 (+36 to +199); +50 (+11 to +175) |
| Scratch tries in between (no side following itself, 1 s turns): a callback | −145 (no step left out); −53, +20 (2 left out) | |

Blocks come out at 0 within a few µs. Callbacks don't always. Each process runs a little faster or slower as a whole, and that lasts for the whole run: in one run the runner's copy was 145 µs a callback slower, through all 5 rounds. The rounds share the same two processes, so they can't average that out, and their range doesn't show it. (Starting the runner again for each round, in a scratch try, spread S's rounds by about 60 µs, as that would.) Not held on one die, where the scheduler puts the two processes adds more: the same tree gave +14 µs a block held and +43 not held. On the Mac none of this has been measured. So treat one run's result as good to about ±150 µs a callback and ±30 µs a block: near a bound, run it again before deciding.

The runner says which code it is when it starts (a hash of `main.rs` and `wire.rs`) and which commit it was built at. m2-gates refuses a runner built from other code, or at another commit.

The runner plays S's own std, whatever `$M2_STD` says.

## Run G7f by hand

```bash
export M2_GATES_S=$(crates/gates/s/build.sh | tail -1)
cargo run -p m2-gates --release -- store --reference 50
cargo run -p m2-gates --release -- arena --reference 50 --at 180
```

`build.sh` takes another cache folder as its argument, if you want one. Without `M2_GATES_S`, both gates stop at once and say how to build the runner.
