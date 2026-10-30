# Credits

**In short**
- What music2 uses that others made, and under which licence.
- Everything else in the repository is music2's own, under MIT OR Apache-2.0.

| What | Where | Licence |
|---|---|---|
| [Inter](https://rsms.me/inter/) 4.1, by the Inter Project Authors: the app's font on Linux and Windows (the Mac uses its own) | `web/public/fonts/` | SIL Open Font License 1.1 (`web/public/fonts/OFL.txt`) |
| The Lead's saved state in the With plugins example: Surge XT's "Init Saw" template, by the Surge Synth Team | `examples/with-plugins/data/lead/lead.state` | CC0 (it says so inside the file) |
| The app icon: the word "music2" in [Liberation Sans](https://github.com/liberationfonts/liberation-fonts), narrowed, drawn for music2 (`icons/source.html`) | `apps/music2-desktop/icons/` | The drawing is music2's own; the font is under the SIL Open Font License 1.1, which lets it be drawn into images |

Everything else in the repository is music2's own, under MIT OR Apache-2.0: the test sounds in `examples/sampler/assets/` are made by `examples/make-sampler-wavs.mjs`, `examples/resample/assets/keys-bounce.wav` is music2's own render of that example's keys, std's kit (`std/samples/kit/`) is music2's own render of std's drum machine (`std/samples/make.sh`), and `std/kernels/std.wasm` is built from `std/kernels-src` (`std/build.sh --check` proves it). Some of std's sound code is written after published methods, each named in its comments: Freeverb's comb and allpass lengths for the reverb, Cytomic's state-variable filter, PolyBLEP oscillators, and WSOLA for stretching audio. The test plugins (Surge XT, GPL-3.0; Airwindows Consolidated, MIT) are downloaded by `ci/plugins.sh` and never stored in the repository.

The app, once built, also carries the code of its dependencies (Rust crates and npm packages), each under its own licence. `deny.toml` and `ci/npm-licenses.mjs` list the licences allowed, and every pull request checks them.
