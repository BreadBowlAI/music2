# The first release, 0.1.0: design and build notes

**In short**
- music2 0.1.0 (alpha) is made by GitHub Actions from a pushed tag `v0.1.0`: `.github/workflows/release.yml` builds the Mac and Linux files, checks each, and makes a **draft pre-release**, "music2 0.1.0 (alpha)". The owner publishes it by hand.
- The files: `music2-0.1.0-macos-arm64.zip` (Apple silicon, signed ad hoc), `music2_0.1.0_amd64.deb`, `music2-0.1.0-linux-x86_64.AppImage`, `m2-0.1.0-linux-x86_64.tar.gz` (the command line, for the AppImage's users), and `SHA256SUMS`. The notes are `docs/releases/0.1.0.md`.
- A downloaded app needs no source tree: it carries std (with `std.wasm`), the templates, the examples, the UI and `m2`, and finds them beside its own program. The plugin host is the program itself run again, so nothing more is carried for it.
- The contract changed first: a new rule, PLAT-8, and FMT-23's `m2` in the Linux packages.
- Proved on Linux here: the three Linux files built, unpacked and run from outside the source tree under Xvfb, with a scratch HOME. The Mac build is proved only by the workflow's first run: what it must show is below.

## The design

### What the owner decided

Version 0.1.0 everywhere (the workspace, `tauri.conf.json`, `web/package.json`; `Cargo.lock` untouched). The Mac app unsigned for this alpha (ad hoc is fine), with a clean switch to sign later from secrets. Apple silicon only, on a standard `macos-15` runner. Linux x86_64, built on `ubuntu-24.04`, as an AppImage and a `.deb`. A draft pre-release from a tag; on a pull request that touches the bundle, both builds as workflow artifacts only. Only `actions/*` and `Swatinem/rust-cache`, pinned by commit. `ci-ok` stays the only required check.

### How a bundle finds what it carries

`crates/server/src/resources.rs` finds std, the templates and the examples, in order: the override (`M2_STD`, `M2_TEMPLATES`, `M2_EXAMPLES`); the bundle the program is in; the source tree it was built from. Before this track it knew one bundle layout, the Mac's (`Contents/MacOS/music2` → `Contents/Resources/<name>`). Tauri puts a Linux package's resources in `<prefix>/lib/<productName>`, beside `<prefix>/bin` (`tauri-utils`' `resource_dir`): `/usr/lib/music2` in the `.deb`, the same inside the AppImage. So `bundled()` now tries both layouts from the folder above the program's. Plain path rules, no `cfg`: PLAT-2 holds. The same rule serves `m2`'s `.tar.gz`, laid out as `bin/m2` and `lib/music2/`.

### Where m2 is

- **Mac**: `Contents/MacOS/m2`, as `docsite` built it.
- **`.deb`**: `/usr/bin/m2` (`bundle.linux.deb.files`), so `m2` works from any shell once installed. The app is `/usr/bin/music2` (`mainBinaryName`), so `music2` does too.
- **AppImage**: `/usr/bin/m2` inside it too (`bundle.linux.appimage.files`), but an AppImage runs from a folder mounted only while it runs, so nothing outside can reach it. The simplest clean way: a fourth file, `m2-<v>-linux-x86_64.tar.gz`, made from the `.deb`'s own `m2`, std, templates and examples (no app, no UI). A song made in the AppImage's app keeps the general `m2` line (FMT-23 now skips an `m2` under `$APPDIR`).
- **`m2-plugin-host`** isn't carried: the app and `m2` host plugins by running themselves with `--m2-plugin-host` (HOST-1's process). `m2-plugin-host` is for the tests only.

### Signing, later

Ad hoc today (`tauri.conf.json`'s `signingIdentity: "-"`, hardened runtime off). The switch: the repo secrets `APPLE_CERTIFICATE` (a base64 `.p12`), `APPLE_CERTIFICATE_PASSWORD`, and for notarizing `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` (optionally `APPLE_SIGNING_IDENTITY`). The workflow passes them on a tag only. `ci/release.sh build` then imports the certificate into a keychain of its own (as `bundle-m2.sh` needs: it refuses `APPLE_CERTIFICATE` itself), sets `APPLE_SIGNING_IDENTITY`, and builds with an overlay that turns the hardened runtime on with `apps/music2-desktop/entitlements.plist` (JIT and unsigned executable memory for wasmtime, library validation off for plugins, the microphone). `bundle-m2.sh` signs `m2` the same way, and Tauri signs and notarizes the app. None of it is tried: no secrets exist (gap #43).

### Why a zip on the Mac, not a .dmg

Tauri's `.dmg` runs `hdiutil` and Finder scripting, which fail now and then on hosted Mac runners; `ditto` makes the zip, which macOS unpacks with a double-click. A `.dmg` can come later.

## What was built

| Commit | What |
|---|---|
| `2fe28c9` | The contract: PLAT-8 (a release from a tag; what each file carries; how a bundle finds it, with the Linux layout), FMT-23 (`/usr/bin/m2` in the Linux packages; no `m2` named from a mounted AppImage), the deferred row on Linux packaging narrowed to the `XDG_*` folders |
| `ee7ce4e` | `resources::bundled()` with the Linux layout, `find_m2`'s `appdir`; `tauri.conf.json`: targets app, deb and appimage, `/usr/bin/m2` in both Linux packages, the `.deb`'s depends; `m2 --version` (and `-V`); `m2`'s notes and the commands page name the Linux paths; the song `AGENTS.md`'s general `m2` line names them too |
| `557bb17` | `mainBinaryName: music2`; the `.deb` depends on `libc6 (>= 2.39)` (2.35 since the review) |
| `7cbdd2e` | `ci/release.sh` (`version`, `build`, `check`, `app`, `sums`), `.github/workflows/release.yml`, `apps/music2-desktop/entitlements.plist`, `ci/apt.sh release` |
| (these notes) | The release notes `docs/releases/0.1.0.md`; README's Download part; the guide's install page; gaps #43 to #45 and their loose ends; mac-run steps 137 to 141; SECURITY.md; HANDOFF.md |

## The tests, and how they failed first

| Test | Failed first with |
|---|---|
| `resources::tests::a_linux_package_finds_its_resources_in_lib_music2` | `left: ".../release/std"`, `right: ".../usr/lib/music2/std"`: the source tree's, not the package's |
| `resources::tests::an_appimage_names_no_m2_inside_it` | `left: Some(At(".../mount/usr/bin/m2"))`, `right: None` |
| `resources::tests::a_linux_package_names_the_m2_beside_the_app` | passed at once: "beside the program" already held; it pins `/usr/bin/m2` |
| `apps/m2/tests/docs.rs`: `m2_says_its_version` | `error: m2 has no command --version` |
| `apps/music2-desktop/tests/bundle.rs`: `the_linux_packages_carry_the_m2_its_build_makes` | "bundle.linux.deb.files names /usr/bin/m2"; later, after a change to the test, `left: ["libasound2", "libdbus-1-3"]` against the glibc line |

`ci/release.sh check` is the test of the files themselves (below). It proves they read their own resources, not a source tree's that the program's build path could still find (the CI checkout is at that path while it checks): it adds a template and an example to the unpacked copy, which no source tree has, and `m2` must make a song from each.

## The Linux proof, on the build server

`ci/release.sh build`, then `check`, then `app`, with a scratch HOME (`$SCRATCH/release/home`), `M2_AUDIO=none`, `M2_MIDI=none`, the test plugins in `CLAP_PATH`:

- **build**: `music2_0.1.0_amd64.deb` 34 MiB, `music2-0.1.0-linux-x86_64.AppImage` 105 MiB, `m2-0.1.0-linux-x86_64.tar.gz` 14 MiB, about a minute warm. The `.deb` depends on `libc6 (>= 2.35), libasound2, libdbus-1-3, libwebkit2gtk-4.1-0, libgtk-3-0` (Tauri adds the last two). `ldd` of `/usr/bin/music2` names no other library outside what WebKitGTK and GTK bring; `m2` needs only glibc and ALSA. (First written as 2.39, from weak symbols: corrected after the review, L1, below.)
- **check**, on each of the three files, unpacked (`dpkg-deb -x`, `--appimage-extract`, `tar`), from a folder outside the source tree with `env -i`: the files are there (std's `m2pkg.toml`, `std.wasm` the same bytes as committed, `src`, `presets`, every template and example, the UI); `m2 --version` says `m2 0.1.0`; `m2 docs` lists the guide; `m2 new` and `m2 check` for every template and example and the check's own; a new song's `AGENTS.md` names that `m2`; `m2 render` of first-song; `with-plugins` rendered with Surge XT and Airwindows hosted by `m2` run again. 15 s in all. `PASS`.
- **app**: the `.deb`'s `/usr/bin/music2` on Xvfb, three times: its start screen, a new song, and with-plugins with the test plugins. Each UI connected in 1 s; nothing panicked; no "plugins can't be hosted".
- **By hand, the AppImage itself** (first with `APPIMAGE_EXTRACT_AND_RUN=1`; after the review, L2, mounted by FUSE too), with `xdotool`: the start screen; **New Song…** listed Starter beat and Empty from the bundle; **Create** opened "My song" with its six tracks and devices; Play moved the playhead; **Export → Song → Save** wrote `~/Music/music2/Exports/My song.wav` (5.8 MB) in the scratch HOME. The song's `AGENTS.md` named the source tree's `target/release/m2`, not the AppImage's mount: FMT-23's order, as this server has the source tree. Pictures: `$SCRATCH/release/appimage-*.png` and `deb-*.png`.
- The mac lint (`cargo clippy --target aarch64-apple-darwin --workspace --all-targets -- -D warnings`, as `ci/check.sh` runs it) passed.

This server lacks `file`, which the AppImage's `appimagetool` needs, and `xdotool`: both were unpacked from Ubuntu's packages into the scratch folder (`apt-get download`), no sudo. GitHub's runner has `file`; `ci/apt.sh release` installs it anyway.

## What the first Mac run must show

The workflow's `mac` job, on its first run (a pull request that touches the bundle, or the tag):

1. **The build**: `tauri build --ci --bundles app` from `apps/music2-desktop`, the log saying `bundle-m2.sh: target/release/m2 is ready for the bundle` and then `Bundling music2.app`. No signing step but the ad hoc one.
2. **The zip**: `dist/music2-0.1.0-macos-arm64.zip`, some tens of MB, like the `.deb`.
3. **The check**: `codesign --verify --deep --strict` passes on `music2.app` and on `Contents/MacOS/m2`; the app carries `Contents/MacOS/music2`, `Contents/MacOS/m2`, and in `Contents/Resources` std (with `std.wasm` the same bytes), every template and example, `web/index.html`, and `LICENSE-MIT`, `LICENSE-APACHE` and `THIRD-PARTY-NOTICES.txt` (the build's log says `notices: … 407 + 0 crates`, or near it); then `m2 --version` says `m2 0.1.0`, and every template and example becomes a song that `m2 check` passes, the first-song render is written, and `release.sh: PASS: music2-0.1.0-macos-arm64.zip`. `with-plugins` is skipped there ("which isn't installed"), as no Mac copy of the test plugins is pinned.
4. **The artifact** `music2-macos`, one zip.

If step 1 fails on the program's name: `mainBinaryName` makes it `Contents/MacOS/music2`; `resources.rs` doesn't care about the name. If the check's `codesign --verify --deep --strict` fails on the ad hoc signature, that is a finding for the notes' Open Anyway step, not a reason to drop the check. Then by hand, mac-run steps 137 to 141: Open Anyway as the notes say it, New Song, play, export, an example, `m2 --version` from Terminal inside the downloaded app (whether Gatekeeper lets it run without `xattr`), and the song's `AGENTS.md`.

## Decided along the way

| # | What | Why |
|---|---|---|
| 1 | The Linux layout is a second path rule in `bundled()`, tried on every OS | Tauri's own rule (`tauri-utils`), and no `cfg`: PLAT-2 |
| 2 | m2 for the AppImage is a `.tar.gz` with `bin/m2` and `lib/music2/` (std, templates, examples), made from the `.deb`'s files | The same lookup rule finds them; one more file is simpler than teaching the AppImage to act as `m2` |
| 3 | A mounted AppImage names no `m2` in a new song's `AGENTS.md` | Its folder is gone when the app quits; the general line names the `.tar.gz` |
| 4 | `mainBinaryName: music2` | So the `.deb` installs `/usr/bin/music2`, not `music2-desktop`; `cargo build` and the checks still make `music2-desktop` |
| 5 | The `.deb` depends on `libc6 (>= 2.35)`, and `check` computes that from the programs' symbols that aren't weak | So apt refuses it on a system whose glibc can't run it, and on no other. (First 2.39, counting weak symbols, which shut out Ubuntu 22.04 and Debian 12 for nothing: the review's L1) |
| 6 | `m2 --version` (and `-V`) prints `m2 <version>` | The check asks it, and a bug report needs it. Declared as words like `--help`, not a command of the table |
| 7 | The check adds a template and an example of its own to the unpacked copy | On CI the source tree is at the build's path while the check runs, so a lookup that fell back to it would pass unseen |
| 8 | The Linux job also fetches the test plugins (`ci/plugins.sh`) | So the release proves plugin hosting from inside the bundle |
| 9 | The release's title is "music2 <version> (alpha)", from the tag; the job refuses a tag that isn't the app's version or has no notes | One place says the version |
| 10 | `actions/download-artifact` v8.0.2 is new to the repo, pinned by commit | The release job collects both platforms' files |

## What is left

| What | Where |
|---|---|
| The Mac build: the workflow's first run, then mac-run 137 to 141 | above |
| Signing and notarizing | gap #43 |
| Intel Mac, ARM Linux, `.rpm`, Windows; updates | gaps #44, #45 |
| Licence notices of the AppImage's system libraries beyond linuxdeploy's copyright files | gaps, Releases and installing |
| The owner: push the tag (`git tag v0.1.0 && git push origin v0.1.0` on `main` after this merges), check the draft, publish it | |

## The check

`M2_CHECK_OUT=$SCRATCH/release/check-pr ci/check.sh --pr` at `25e18a8` (main merged in: `docsite` had landed as #15), with a scratch HOME and `CLAP_PATH` naming the test plugins:

```
music2 check --pr: PASS
commit   25e18a858866 on release (clean)
machine  Ubuntu 24.04.3 LTS, x86_64, 32 threads
tools    rustc 1.99.0, node v24.21.0, 6 min
plugins  /home/jerry/.local/share/m2-test-plugins/clap (the builds ci/plugins.sha256 records)

changes          PASS  .github/workflows/release.yml changed since e847f521092d, and it isn't documentation (ci/docs-only.sh's DOCS_FREE): the whole PR tier runs
pr quick         PASS  0 min 27 s
pr clippy        PASS  0 min 4 s
pr rust          PASS  3 min 22 s
pr suites        PASS  1 min 52 s
pr renders       PASS  0 min 7 s
repo unchanged   PASS
```

1,950 Rust tests (48 skipped, none passed only on a retry), 54/54 browser suites (1,966 checks), 12 of 12 renders. The first run, with the scratch HOME alone, failed only its two plugin rows: the test plugins live in the real HOME, so `CLAP_PATH` names them. The commit after it changes only these notes.

## Answers to the review

The skeptic's review of `release` had eleven findings (`$SCRATCH/release/review.json`). Each is fixed at its cause; the tests changed first were seen failing (`libc6 (>= 2.39)` against 2.35, "no Resources/LICENSE-MIT"). The contract changed first (`482477a`).

| Id | Answer |
|---|---|
| L1 | Fixed: `ci/release.sh`'s `floor` counts only version needs that aren't weak (`readelf -V`); `check` refuses a `.deb` whose `libc6 (>= …)` differs from its programs' floor (2.35) and notes that don't say each file's floor ("glibc 2.35 or newer" for the `.deb` and the `.tar.gz`, "glibc 2.38 or newer" for the AppImage, computed over everything inside it). The release's `m2` ran on Ubuntu 22.04's glibc 2.35 here (its `ld.so`, jammy's ALSA and libgcc): `--version`, `new`, `check`, a 2 s render. The app on 22.04 and Debian 12 is still untried, and the notes and gaps say so |
| L2 | Fixed: the notes and the guide say the AppImage needs FUSE's `fusermount` (the `fuse3` package), not FUSE 2, with `--appimage-extract-and-run` without it. Proved: this server has no `libfuse.so.2`, and the AppImage, mounted by FUSE (no extract-and-run), reached its start screen under Xvfb (`$SCRATCH/release/appimage-fuse-start-screen.png`) |
| L3 | Fixed: `ci/notices.mjs` makes `THIRD-PARTY-NOTICES.txt` as `tauri build` bundles (`bundle-m2.sh`), from `cargo metadata` of the app's and m2's normal dependencies for the target, std.wasm's workspace, the UI's npm packages that aren't dev ones, Rust's standard library and Inter's OFL: 436 crates on Linux, 407 on the Mac, 10 npm packages, 288 licence texts, each printed once. A package with no licence file gets the standard text of each licence it names, from `ci/licenses/` (SPDX's texts); a licence with no text there stops the build. No new crate, no new tool. `music2.app` carries it and `LICENSE-MIT`, `LICENSE-APACHE` in `Contents/Resources`, the `.deb` in `/usr/share/doc/music2` with a DEP-5 `copyright`, the AppImage in the same folder inside it (Tauri copies nothing to an AppImage's top folder, tried), the `.tar.gz` beside `bin`. `check` requires them, music2's licences byte for byte |
| L4 | Fixed: both `version` steps take the tag through `env: TAG`, run as `"$TAG"`; no `${{ }}` is left in a `run:` line |
| L5 | Written down, for when signing is turned on: the secrets go in an environment `release` limited to `v*` tags with the owner as reviewer, signing in a job of its own, and a ruleset on who may create `v*` tags (`release.yml`'s head, gap #43). No secret exists yet, so nothing can reach one today |
| L6 | Fixed: the notes, the guide and the README give macOS 15's steps as such ("Move to Trash"), and macOS 11 to 14's: Control-click → Open, and System Preferences → Security & Privacy → General on 11 and 12. Still owed: the words on a real Mac (mac-run 138) |
| L7 | Fixed: the Rust cache is saved only on `main` (`save-if: github.ref == 'refs/heads/main'`), by a weekly scheduled run there, so tags and pull requests start from it |
| L8 | Fixed: `ci/appimage-tools.sha256` pins linuxdeploy, AppRun, linuxdeploy-plugin-appimage (`1-alpha-20250213-1`, not continuous) and the AppImage runtime (type2-runtime `20251108`), each by URL and sha256; `build` fetches and checks them into Tauri's cache, which Tauri then uses as they are (no "Downloading" in the build's log), and hands the runtime over with `LDAI_RUNTIME_FILE`. The AppImage now says runtime `dd6cebe`. The GTK plugin script is inside the pinned Tauri CLI |
| L9 | Fixed: `bundle.publisher` "The music2 authors <mxudev@gmail.com>" (the address CODE_OF_CONDUCT.md already gives) is the `.deb`'s Maintainer; `longDescription` is its long description; `icons/256x256.png` (the same 256-pixel picture) replaces `128x128@2x.png`, so the icon lands in `hicolor/256x256`. A test holds all three |
| L10 | Fixed: the workflow also runs every Monday on `main` and by hand, and its pull-request paths now take in std, the templates, the examples, `Cargo.lock`, `web/package-lock.json` and the new scripts, so a change that breaks a download shows before a tag |
| L11 | Fixed: the `.tar.gz`'s `README.txt` (its glibc floor computed as it is made) and the notes say `m2` needs glibc 2.35 or newer and ALSA's library (`libasound2`, or `libasound2t64`), even with no sound |

After the fixes, on the build server: `ci/release.sh build`, `check` (all three files, with the test plugins) and `app` (start screen, new song, with-plugins) passed again. Then `M2_CHECK_OUT=$SCRATCH/release/check-pr2 ci/check.sh --pr` at `edf338f`:

```
music2 check --pr: PASS
commit   edf338f9a328 on release (clean)
changes          PASS  .github/workflows/release.yml changed since e847f521092d, and it isn't documentation (ci/docs-only.sh's DOCS_FREE): the whole PR tier runs
pr quick         PASS  0 min 27 s
pr clippy        PASS  0 min 5 s
pr rust          PASS  2 min 18 s
pr suites        PASS  1 min 49 s
pr renders       PASS  0 min 7 s
repo unchanged   PASS
```
