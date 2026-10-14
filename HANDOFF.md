# Handoff: moving music2's work to the Linux server

Written 2026-10-14, when work moved from the owner's MacBook to the Linux server `jerry-ax102`
(Ubuntu 24.04, Ryzen 9 7950X3D, 32 threads, 124 GB). Read this first, then
`docs/architecture.md` (the contract) and the design for whatever you pick up.

## Where everything is

| Branch | Commit | State |
|---|---|---|
| `main` | this commit | Steps 1–6 and the fast note edits (8aa301e). Verified. |
| `wip/step7` | 5079233 | Step 7, the engine: slices 1–3 done, slice 4 half done. Not verified as a whole. |
| `taste` | 4804247 | The new look (T and S), merged with 8aa301e; 8b.1 Tracks done (a0d5f91); 8b.2 song bar unfinished. |
| `sounds` | 916127d | 20 new std sounds (polysynth, FM, drum kit). Final independent check was cut short. |

Designs, all in `docs/designs/`:
- `step6/` the Tx and write path (C1–C21 are referred to from the contract).
- `step7/design-7.md` (revision 2, the one to build; §18 build slices, §19 owner questions).
- `step8/design-8a.md`, `design-8b.md`, `taste.md` (the look), `audit-ui.md` (98-shot audit).
- `platform/survey-code.md` (85 OS-specific pieces) and `survey-later.md` (what 8a, 8b and 9 add per OS).
  The platform design itself was not written yet.
- `workflows/`: the workflow scripts used, as templates. Their paths are the Mac's.

## What to do next, in order

1. **The platform layer, first, on the server.** The cross-platform layer was planned for after step 7,
   but the work now lives on Linux, so Linux has to be green first. Write the design from the two surveys
   (design, attack, revise), then build it on `main`. Today on Linux, at 8aa301e:
   - `cargo build --workspace --exclude music2-desktop` works (48 s); the desktop app's build script fails.
   - `cargo test`: 686 of 696 pass. The 10 failures: the FSEvents watcher (5), `stage.rs` durable sync and
     `RENAME_SWAP` (3), the plugin main thread (`core/plugins/tests/main_thread.rs`), and
     `fixture_renders_change_only_with_stds_version`: `examples/resample` renders other bits on x86 Linux.
     The audio clip path uses the platform's libm. Use deterministic math so a song exports the same bits
     everywhere.
   - Browser suites: 21 of 29 ok. The failures are ⌘ shortcuts (Linux uses Ctrl), a case-sensitive
     filesystem ("exports" vs "exports 2"), and the watcher.
   - Keep the Mac the reference: the timing gates are judged on Apple silicon. The Mac side needs checking
     somewhere: GitHub Actions macOS runners (the owner hasn't decided; they cost paid minutes on a
     private repo) or the owner's MacBook now and then.
2. **Finish step 7** on top of the platform layer: rebase or merge `wip/step7`, check that it builds, then
   finish slice 4 (design-7 §18: state loads into kept instances in prepare, restart from the committed
   blob, export with `Hold`, G7e). Then verify every gate, review, confirm, fix, as the earlier steps did.
   CLAP plugins: the Mac had Surge XT and Airwindows Consolidated. Install their Linux CLAP builds on the
   server for the with-plugins gates, or run those on a Mac.
3. **Merge `sounds`**: re-run its full checks first (see its commit message), then merge.
4. **Merge `taste`**: finish 8b.2 (song bar) and build 8b.3 (Mix faders) per design-8b, then merge. Expect a
   conflict in `core/session/src/tx/delta.rs`: 8b.1 places voice nodes where their poly is, and step 7
   changed delta naming. Many e2e suites changed on `taste` (testids moved; see its commits).
5. Then **8a** (all CPU cores; design-8a), **8b.4–8b.10**, and **step 9**, as in the contract's order of work.

## Open owner decisions (ask, with a recommendation)

- Step 7 slice 3 went beyond the design twice, both forced by gates: new nodes' audio inputs fade in over
  5 ms (G6 caught a real click at a new delay's first echo), and `text::slide` re-aligns inserted tokens.
- G7f: no Store layout passes the rule's absolute address-space bound (HEAD fails it too). Layout P1, the
  default settings, was kept. G7f part 2's p99 rule still failed in slices 2 and 3.
- `std:chorus`, phaser and tremolo wobble on engine time, so strings and keys after a stop differ from the
  export. Fixing it changes `templates/starter`'s render and needs a std version bump.
- GitHub Actions macOS runners: yes or no.

## Decisions already taken for the owner

Step 7's Q1–Q9 as design-7 §19 recommends (a SiteId is a call's `(` point; export is a tape pause). 8a:
gate 8 on this M1 Pro is "provisional" until a base M1 exists; the device buffer keeps its default. 8b:
tempo drag lets audio drift until release; a MIDI keyboard slice (8b.10) is added; the level slider lives
only on the Mix strip. The data plane keeps its 20 ms budget, met by playing the edit first (PLANE-5).

## How the owner wants the work done

- **Quality over speed.** The owner chose full checks over lighter ones. For each step: design, attack,
  build in green slices, verify (all tests, clippy, every browser suite, timing gates, offline
  byte-identity), multi-lens review, skeptic confirmation, fix. Commit and push each step once it passes.
- **Simple yet elegant**, a tasteful professional DAW (good enough for A. G. Cook). Don't be afraid to change
  earlier work when it makes things simpler.
- **Writing to the owner:** they are dyslexic and new to music. Short sentences, plain words, tables,
  "In short" first, no walls of text.
- No built-in AI in the app. No backwards compatibility.
- Never launch music2-desktop for tests; never use the real `~/.music2` or `~/Music` (use a scratch
  `HOME`). On the server, run long jobs where the owner can watch (tmux).

## Checks that worked on the server

```sh
. ~/.cargo/env
cargo build --workspace --exclude music2-desktop
cargo test --workspace --exclude music2-desktop --no-fail-fast
(cd web && npm ci && npm run build)
(cd e2e && npm ci)
cargo build -p m2-server --example headless && cargo build -p m2
E2E_CHROME=$HOME/.cache/puppeteer/chrome-headless-shell/linux-133.0.6943.141/chrome-headless-shell-linux64/chrome-headless-shell \
  M2_HEADLESS=target/debug/examples/headless node e2e/run.mjs all
```

The Mac's offline render fingerprints at 8aa301e (`e2e/baseline.sh`; the `-from4` cases are
`--from 4 --seconds 6`). On Linux, record your own baseline at a commit before changing anything, since
`examples/resample` differs there today.

```
6e61b87f783d56d2ae61e01872c23e2ecd1b5a09b4b07b726a09030f922bf7d0  examples-first-song.wav
ef0fadca83cb2b16661be3a3bb1cd679d0ddf22f2edac9dc0e58f2102ecf7b7c  examples-first-song-from4.wav
6ac6fcb117fe5b1dd1545bf3cf1fd38e03a0fc09864d0b339a499a32c925eea7  examples-resample.wav
a360d04c9fba10e4eee079360fa93e52578a2c2b3312dc209f192b609f9c1dbf  examples-resample-from4.wav
835aff5365170465b2fc8d32e5bcfe057708fc1f13030d8adda95d784a5fa745  templates-empty.wav
a0249c1e5459543eab98b7b46bfd442fca94950c0cca8c04fb92defd322bc062  templates-empty-from4.wav
413d35cfa285d2f65876bdf47aec9816e3ebc9894c3c2fbeb7708adcdaf486a7  templates-starter.wav
71bb7191c5aac5ed138221dd05966e02e4e36e2a00c990429275bfb7b23f4448  templates-starter-from4.wav
```

## Known flakes and small issues

- `files::tests::diagnostics_say_where_in_the_project_each_problem_is` fails in a worktree with a long path
  (the message is cut before the file name). Not a bug in the code under test.
- `s5-knobs` failed 3 of 11 release runs (page detached after a double-click on a knob at its default).
- `live3a` failed one ruler-drag check once under load.
- G3 (note edit within compile + 20 ms) sits near its line under load: p95 16–22 ms at n=10.
