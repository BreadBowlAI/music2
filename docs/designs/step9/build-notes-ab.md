# Track ab: build notes

Branch `ab`, from `main` at `258275b`, 2026-10-18. The plan is [ab-plan.md](ab-plan.md); what changed while building is written there, under each slice.

## In short

- **Slice 1, the layout:** one sandbox for each box (idea 3) beat today's layout on every measure, so it stays. RENDER-32 holds the numbers.
- **Slice 2, kernel ABI v1:** final and written down ([kernel-abi.md](../../kernel-abi.md)). Kernels can read the song's audio files.
- **Slice 3, the sampler:** std 0.2.1 has a **Sampler**. It plays a WAV on every note, or one WAV for each range of notes. Drop a WAV on its card in Device to give it one.
- No existing render changed. The new example adds 2 renders: 12 of 12 match.
- Linux timings are information only. The Mac's run is owed (PLAT-1).
- **Ready to land:** `main` (track `io`) merged in, the review's big finding fixed, and `ci/check.sh --gates` passed at `7399cb8`. See [Finishing the track](#finishing-the-track-2026-10-18).

## What you can do now

| You can | Where |
|---|---|
| Add a Sampler track | + Track ▸ Sampler, or the side browser's Sampler group |
| Give it a WAV file | drop the file on its card in Device |
| Take that back | Undo ("Undo setting Sampler to kick.wav"); the file stays in `assets/` |
| Play a file per range of notes, stop with the note, tune it | in its code: `zones`, `mode: "gate"`, `tune` |
| Hear an example | Open an Example ▸ Sampler |

## Slice 3, piece by piece

| Piece | Where | Test (each red first) |
|---|---|---|
| The kernel: picks its file when a note starts, plays it at the note's speed with the clips' cubic Hermite, one-shot or gated | `std/kernels-src/src/sampler.rs` | `at_its_root_a_sample_plays_its_own_samples`, `an_octave_up_plays_every_second_sample`, `a_note_picks_its_zone`, `one_shot_plays_past_the_note_off`, `a_missing_sample_is_silence` |
| The kit's calls for settings and files | `std/kernels-src/kernel/src/lib.rs` | the kernel's tests use them |
| The instrument, std 0.2.1 | `std/src/sampler.ts` | `the_sampler_gives_its_kernel_its_files` |
| The example and its 3 WAVs | `examples/sampler`, `examples/make-sampler-wavs.mjs` | `the_sampler_example_gives_the_same_bits_on_1_to_7_threads` |
| The builder: one Tx, one undo step | `crates/editor/src/sampler.rs` | `dropping_a_wav_sets_the_sample_and_undo_takes_it_back` |
| The route | `POST /api/graph/sample` | `a_wav_dropped_on_a_sampler_sets_its_file_through_the_route` |
| The card and the drop | `web/src/lib/sampler.ts`, `drop.ts`, `NodeGraph.tsx` | browser suite `ab-sampler` (23 checks) |

## Pictures

From the app on the invisible screen `:95`, in `$SCRATCH/app-shots/ab-sampler/`:

| Picture | Shows |
|---|---|
| `new-sampler-drop-a-wav.png` | a new Sampler track; its card says "Drop a WAV here" |
| `keys-pluck.png` | the example's Keys: its card names `pluck.wav` |
| `drums-zones.png` | the example's Drums: "2 files: kick.wav, snare.wav" |

## Worth knowing

- **std.wasm changed** in slice 3: it now imports `m2.asset_read`. So every std kernel's hash changed. No sound changed.
- **The desktop app's page** in a dev build is a copy made when the app is built (`target/debug/web`). After `npm run build`, copy `web/dist` there or build the app again, or the app shows the old page.
- **std's kernel tests** run in release in `ci/check.sh`. One of them, `odd_db_values`, fails in a debug build (a NaN's sign bit), on `main` too (gaps).

## Finishing the track (2026-10-18)

**In short:** `main` moved (track `io`: clips now stream from disk), so it was merged in. The review's one big finding is fixed, with a test shown red first. Its four small ones are one line each in `docs/gaps.md`. Then the full check.

| What | Done |
|---|---|
| Merge `main` (23 commits from `io`) | merge commit `c68d203`: `audio.rs` and `HANDOFF.md` conflicted; the sampler's import now makes its peaks as it copies, like `io`'s |
| Review 1 (big): after the merge, a kernel's file was decoded again at every live edit | `7399cb8`: kernels' files have their own map in the audio cache, by content, kept while builds read them. Test `a_kernels_file_is_decoded_once_across_edits`: red on the merge (3 decodes for 3 builds), green after (1) |
| Review 2 (small): a module for a later ABI with another import is refused for the import, not its version | gaps (fix before v1 ships); ABI-3's *Today* says so |
| Review 3 (small): a WAV dropped on a Sampler that refuses it is still copied into assets/ | gaps |
| Review 4 (small): a Sampler file that won't decode plays silence, no message | gaps |
| Review 5 (small): a kernel asking for memory while playing is told "over 256 MB" | gaps |

### Decisions

- **Kernels' files are decoded whole, not streamed.** `m2.asset_read` copies from memory, so a kernel's file stays whole, once per content, shared by the old and new renderer. `io`'s `decoded` map (emptied at every settle) is only for a warp's one render.
- **The decode count includes kernels' files** (`Cache::work`): one number for "files opened".

### Rules settled

| Rule | What changed |
|---|---|
| ABI-2 | *Today*: kernels' files are decoded once into the cache's own map; clips stream from disk, kernels don't yet |
| ABI-3 | *Today*: not yet for every module: one for another version that also has another import, or a 64-bit memory, is refused for that (gaps) |

### The final check

`ci/check.sh --gates` at `7399cb8`, nothing passed only on a retry:

```
music2 check: PASS
commit   7399cb8539e8 on ab (clean)
machine  Ubuntu 24.04.3 LTS, x86_64, 32 threads
tools    rustc 1.99.0, node v24.21.0, 36 min
plugins  ~/.local/share/m2-test-plugins/clap (the builds ci/plugins.sha256 records)

toolchain        PASS  rustc 1.99.0
web build        PASS  
build            PASS  
app              PASS  the UI connected: first-song in 1 s, with-plugins in 1 s
rust tests       PASS  1609 passed, 0 failed
clippy           PASS  
pool models      PASS  29 passed, 0 failed
std kernels      PASS  tests and clippy
std.wasm         PASS  rebuilt to the same bytes in another folder
mac lint         PASS  the macOS code type-checks and lints
browser suites   PASS  68/68 suites ok
renders          PASS  12 of 12 match e2e/renders.sha256
old engine (S)   PASS  built for G7f from a clean copy of 8aa301e, in ~/.cache/m2-check/s-8aa301e
gates            PASS  the 7 runs ci/gates.txt names
repo unchanged   PASS
```

Pictures from the app on `:95` (1440×900): `$SCRATCH/app-shots/ab-7399cb8/` (`drums-zones.png`, `keys-playing.png`, `captions.txt`).
