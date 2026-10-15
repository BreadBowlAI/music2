# Step 7's finish: build notes

What each part of step 7's finish did, where it left the design, and what its checks said. The design is
`design-7.md` (revision 2; §18 for the slices, §19 for the owner's answers). Slices 1 to 3 and half of slice 4 were
built on the Mac and came over as one commit, `5079233` on `origin/wip/step7`. Each agent adds its own section.

## Rebase

**In short:** step 7 now sits on `main` (the platform layer) as one commit on branch `step7`. It builds. Two tests
needed a one-line fix each to compile against slice 4's half-done types. Nothing was cut.

### The rebase

`git cherry-pick 5079233` onto `main` at `b90bacf`, in the worktree `~/m2-runs/wt/s7`. Committed as `c8117c3`
(`step7` = `main` + that commit). Three files conflicted:

| File | Conflict | How it was resolved |
|---|---|---|
| `docs/architecture.md` | PLANE-5's rule: the platform step changed the sync's words (`m2_os::fs::barrier`) and added the Linux numbers; step 7 changed "through a new renderer" to "through an activation that keeps every instance" | Both kept: the platform step's text, with step 7's words for RENDER-26's path |
| | The acceptance-gates table: the platform step added row 6b; step 7 changed row 7 (the count G7f measured, 6,058) | Both kept: row 6b, then step 7's row 7 |
| `core/session/Cargo.toml` | Both sides added a dev-dependency on the same line: `libc` (the kill tests, OS-72) and `m2-plugins` (the probe in `continues`' tests) | Both kept |
| `Cargo.lock` | Neighbouring lines (`m2-os` and step 7's edges) | `main`'s lock, then `cargo metadata` added exactly step 7's two edges: `m2-session` → `m2-plugins`, `music2-desktop` → `m2-play`. No `cargo update`. |

What the merge notes asked to check, and what was found:

| Where | Found |
|---|---|
| `core/runtime/src/audio.rs` | Merged clean. `write_new_wav` still pushes the file and syncs its folder (S7, `audio.rs:80-88`). Every transcendental call outside tests goes through `libm`; `host_math.rs` checks it (results below). |
| `core/plugins/src/probe.rs` | Merged clean. Step 7's new thread counter calls `main_thread::token()`, which `main` keeps as a wrapper. |
| `core/kernel/` → `std/kernels-src/kernel/` | Step 7 doesn't touch it. |
| `crates/server/src/graph/fsevents.rs` (gone on `main`) | Step 7 doesn't touch it. Nothing to carry by hand. |
| `crates/gates/src/store.rs` | Merged as it was: its Mach `task_info` and QoS calls sit under `cfg(target_os = "macos")`. `core/os/tests/one_place.rs` fails on them until they move into `m2_os::process` (platform design §8). Not done here. |
| `apps/music2-desktop` | Merged clean: the debug-build allocation counter (`m2_play::alloc`). |

### Making it build

`cargo build --workspace` built at once, with no warnings. `cargo test --workspace --no-run` failed in two tests
that slice 4 had not caught up with. Each is a one-line fix to follow a type slice 4 changed; neither changes what
the test checks:

| Test | Error | Fix |
|---|---|---|
| `core/runtime/src/tests/live.rs:491` (`prewarming_keeps_kernel_memory_flat`) | The build callback now returns `Make` (new, moved, or lent to an export), not `Option<u64>` | `Make::New(0)`: a fresh instance, as `Some(0)` meant |
| `crates/server/src/graph/tests.rs:1834` (`plays_handles`) | `PluginAt` is now a struct (`node`, `handle`, `inst`, `blob`), not a pair | `p.handle` for `p.1` |

Nothing of slice 4 had to be cut: everything it started compiles.
