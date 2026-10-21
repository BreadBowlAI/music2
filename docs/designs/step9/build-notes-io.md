# io: build notes

**In short**
- Step 9's audio turns 1 and 4 are built on branch `io`: long audio plays from the disk, and you can record
  your voice or a guitar on an audio track.
- How each slice was built, and each choice made while building, is in the plan beside this file:
  `io-plan.md`, its Decisions and its "Changed while building" tables.
- A review found 8 things. The 3 big ones are fixed, each with a test shown red first. Of the 5 small ones, 2
  are fixed and 3 are one line each in `docs/gaps.md`.
- Then `ci/check.sh --gates` ran once at `64380e1`: its summary is at the end, word for word.

## What was built

| Slice | What you get | Commits |
|---|---|---|
| 1. Disk streaming | Every audio clip plays from the disk, read a little ahead on its own thread (`m2-disk`). The sound thread only copies what is ready. Files up to 3 hours | `io1` |
| 2. The input and the take | The player reads a sound input into two rings. A take is written into `assets/` as it records, made safe on the disk, then placed as one clip | `io2` |
| 3. Recording in the app | R records on an audio track. Settings picks the input and "Hear yourself while recording" | `io3` |
| The review | 8 findings, below | `3e371de` to `3d226f2` |
| The final check | Two older tests that passed only on a retry, fixed | `8c85e92`, `64380e1` |

## The review

| # | Size | What was wrong | What changed | Test (red first, then green) |
|---|---|---|---|---|
| r1 | big | A new long audio lane's wait for the disk (up to 0.5 s) held the server's control-plane lock. Live notes, the piano and the meters waited for it | `Player::ready` waits before the lock is taken; only the send is under it. A new player loads at its beat with one wait (`Player::open_at`). Record makes the take's file before the lock too | `a_new_audio_lane_never_waits_for_the_disk_under_the_lock`: the lock was held 502 ms, now under 150 |
| r2 | big | The input's errors counted as the output's: unplugging a USB microphone stopped playback | Each input counts its own errors. An unplugged input ends the take with what it has and says so; the song plays on; the next R opens the input again | `an_unplugged_input_ends_the_take_and_the_song_plays_on`; through the routes, `the_inject_api_unplugs_the_input` |
| r3 | big | A failed write (a full disk) lost the whole take. A take past 4 GiB wrapped its WAV header | A failed write ends the take with every frame written before it, as a whole file. A take ends before 4 GiB (3.1 hours of 48 kHz stereo). Both say so | `a_full_disk_keeps_the_take_up_to_there`, `a_take_ends_before_its_wav_runs_out` |
| r4 | small | Hearing yourself drops out when the output buffer is much bigger than the input's | One line in gaps.md | |
| r5 | small | A warped render's cache name had a version written by hand | `WARP_VERSION`, a const beside the warp code (the same name as before) | |
| r6 | small | "The sound thread never reads a file" hung on a convention | The engine marks every renderer it plays, as it marks it live | |
| r7 | small | An unplugged chosen input records from another one, silently | One line in gaps.md | |
| r8 | small | A file that can't be read plays silence with no message | One line in gaps.md | |

## Choices made in the fixes

| Choice | Why |
|---|---|
| `Player::ready` is public; `open`, `replace` and `activate` still do it for a renderer not made ready | The player's own callers and tests stay the same; only the server, which holds a lock, calls it first |
| A take's `note` now reaches the window as a message | It never did: "The disk was too slow" was never seen |
| The tests play a full disk with a limit on the part file's length (`Growing::limit`) | A real partial write, through the real code, not a stub |
| `POST /api/graph/input/inject {"unplug": true}` | Browser suites and the app's pictures can unplug the silent input |
| An input's errors are logged by its own thread, once a second | Its callback never prints (RENDER-28), as the output's doesn't |

## Contract

| Rule | Change |
|---|---|
| RENDER-30 | Neither disk wait holds the server's control-plane lock (r1) |
| RENDER-30a | The input's errors are its own; an unplugged input ends the take and the output plays on (r2). A failed write keeps what was written; a take ends before its WAV's 32-bit sizes run out (r3) |

## The check

Three runs. The first (at `3d226f2`) passed, with two older tests only on a retry:
`a_device_player_let_go_stops_its_stream_and_frees_its_engine_elsewhere` (it counts the process's engine workers, and
another test starts some beside it: both now hold one test lock) and the `attention` suite (it read the song's revision
before the move's own was published: it now waits for the page's Undo to name the move). The second (at `8c85e92`)
failed on that new test lock, which `no_blocking`'s scan of `engines.rs` reads as a blocking call (now taken through a
helper), and, at load 9 to 10, on `audition_never_waits_for_a_compile` and `midi_never_waits_for_a_compile` once each
(older timing tests; in HANDOFF's Known flakes). The third, at `64380e1`, word for word:

```
music2 check: PASS
commit   64380e1f33bf on io (clean)
machine  Ubuntu 24.04.3 LTS, x86_64, 32 threads
tools    rustc 1.99.0, node v24.21.0, 44 min
plugins  ~/.local/share/m2-test-plugins/clap (the builds ci/plugins.sha256 records)

toolchain        PASS  rustc 1.99.0
web build        PASS  
build            PASS  
app              PASS  the UI connected: first-song in 1 s, with-plugins in 1 s
rust tests       PASS  1592 passed, 0 failed
clippy           PASS  
pool models      PASS  29 passed, 0 failed
std kernels      PASS  tests and clippy
std.wasm         PASS  rebuilt to the same bytes in another folder
mac lint         PASS  the macOS code type-checks and lints
browser suites   PASS  67/67 suites ok
renders          PASS  10 of 10 match e2e/renders.sha256
old engine (S)   PASS  built for G7f from a clean copy of 8aa301e, in ~/.cache/m2-check/s-8aa301e
gates            PASS  the 7 runs ci/gates.txt names
repo unchanged   PASS  
```

