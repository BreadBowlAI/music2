# S's runner (G7f)

G7f ([RENDER-32](../../../docs/architecture.md#engine-shape-step-7)) asks what step 7 costs. It judges the app against **S**: HEAD `8aa301e`, the commit before step 7. Each G7f run measures S fresh, beside the app, over the same music, on the same machine. So the comparison holds on any machine, and no old number is added in.

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

m2-gates starts the runner, sends a command, and waits for the answer. So only one of them runs at a time. Each round, three players (or renderers) take turns of about a second each, over the same music: the app's layout and P1 in m2-gates, and S in the runner. The order changes every turn. Each side hashes what its timed callbacks or blocks played. The gate checks that all three played the same bits.

The first callback or block of each turn is played but not timed. It meets the caches the side before it left. The app's layout and P1 share one thread, so they always do; S, in a process of its own, may find its own data still there. Timed, that first step made the same engine seem 7 µs a block slower in m2-gates than in the runner (on Linux, 5 pairs, +3 to +39 µs). Untimed, the difference was -6 µs (-8 to +11).

The runner says which code it is when it starts (a hash of `main.rs` and `wire.rs`) and which commit it was built at. m2-gates refuses a runner built from other code, or at another commit.

The runner plays S's own std, whatever `$M2_STD` says.

## Run G7f by hand

```bash
export M2_GATES_S=$(crates/gates/s/build.sh | tail -1)
cargo run -p m2-gates --release -- store --reference 50
cargo run -p m2-gates --release -- arena --reference 50 --at 180
```

`build.sh` takes another cache folder as its argument, if you want one. Without `M2_GATES_S`, both gates stop at once and say how to build the runner.
