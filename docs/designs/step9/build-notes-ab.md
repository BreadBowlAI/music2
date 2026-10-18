# Track ab: build notes

Branch `ab`, from `main` at `258275b`, 2026-10-18. The plan is [ab-plan.md](ab-plan.md); what changed while building is written there, under each slice.

## In short

- **Slice 1, the layout:** one sandbox for each box (idea 3) beat today's layout on every measure, so it stays. RENDER-32 holds the numbers.
- **Slice 2, kernel ABI v1:** final and written down ([kernel-abi.md](../../kernel-abi.md)). Kernels can read the song's audio files.
- **Slice 3, the sampler:** std 0.2.1 has a **Sampler**. It plays a WAV on every note, or one WAV for each range of notes. Drop a WAV on its card in Device to give it one.
- No existing render changed. The new example adds 2 renders: 12 of 12 match.
- Linux timings are information only. The Mac's run is owed (PLAT-1).

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

From the app on the invisible screen `:95`, in `~/m2-runs/app-shots/ab-sampler/`:

| Picture | Shows |
|---|---|
| `new-sampler-drop-a-wav.png` | a new Sampler track; its card says "Drop a WAV here" |
| `keys-pluck.png` | the example's Keys: its card names `pluck.wav` |
| `drums-zones.png` | the example's Drums: "2 files: kick.wav, snare.wav" |

## Worth knowing

- **std.wasm changed** in slice 3: it now imports `m2.asset_read`. So every std kernel's hash changed. No sound changed.
- **The desktop app's page** in a dev build is a copy made when the app is built (`target/debug/web`). After `npm run build`, copy `web/dist` there or build the app again, or the app shows the old page.
- **std's kernel tests** run in release in `ci/check.sh`. One of them, `odd_db_values`, fails in a debug build (a NaN's sign bit), on `main` too (gaps).
