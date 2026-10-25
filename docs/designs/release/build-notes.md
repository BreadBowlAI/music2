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
| `557bb17` | `mainBinaryName: music2`; the `.deb` depends on `libc6 (>= 2.39)` |
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

- **build**: `music2_0.1.0_amd64.deb` 34 MiB, `music2-0.1.0-linux-x86_64.AppImage` 105 MiB, `m2-0.1.0-linux-x86_64.tar.gz` 14 MiB, about a minute warm. The `.deb` depends on `libc6 (>= 2.39), libasound2, libdbus-1-3, libwebkit2gtk-4.1-0, libgtk-3-0` (Tauri adds the last two). `ldd` of `/usr/bin/music2` names no other library outside what WebKitGTK and GTK bring; `m2` needs only glibc and ALSA. The newest glibc symbol is 2.39, so Ubuntu 24.04 or newer, Debian 13 or newer.
- **check**, on each of the three files, unpacked (`dpkg-deb -x`, `--appimage-extract`, `tar`), from a folder outside the source tree with `env -i`: the files are there (std's `m2pkg.toml`, `std.wasm` the same bytes as committed, `src`, `presets`, every template and example, the UI); `m2 --version` says `m2 0.1.0`; `m2 docs` lists the guide; `m2 new` and `m2 check` for every template and example and the check's own; a new song's `AGENTS.md` names that `m2`; `m2 render` of first-song; `with-plugins` rendered with Surge XT and Airwindows hosted by `m2` run again. 15 s in all. `PASS`.
- **app**: the `.deb`'s `/usr/bin/music2` on Xvfb, three times: its start screen, a new song, and with-plugins with the test plugins. Each UI connected in 1 s; nothing panicked; no "plugins can't be hosted".
- **By hand, the AppImage itself** (`APPIMAGE_EXTRACT_AND_RUN=1`, as this server has no FUSE 2), with `xdotool`: the start screen; **New Song…** listed Starter beat and Empty from the bundle; **Create** opened "My song" with its six tracks and devices; Play moved the playhead; **Export → Song → Save** wrote `~/Music/music2/Exports/My song.wav` (5.8 MB) in the scratch HOME. The song's `AGENTS.md` named the source tree's `target/release/m2`, not the AppImage's mount: FMT-23's order, as this server has the source tree. Pictures: `$SCRATCH/release/appimage-*.png` and `deb-*.png`.
- The mac lint (`cargo clippy --target aarch64-apple-darwin --workspace --all-targets -- -D warnings`, as `ci/check.sh` runs it) passed.

This server lacks `file`, which the AppImage's `appimagetool` needs, and `xdotool`: both were unpacked from Ubuntu's packages into the scratch folder (`apt-get download`), no sudo. GitHub's runner has `file`; `ci/apt.sh release` installs it anyway.

## What the first Mac run must show

The workflow's `mac` job, on its first run (a pull request that touches the bundle, or the tag):

1. **The build**: `tauri build --ci --bundles app` from `apps/music2-desktop`, the log saying `bundle-m2.sh: target/release/m2 is ready for the bundle` and then `Bundling music2.app`. No signing step but the ad hoc one.
2. **The zip**: `dist/music2-0.1.0-macos-arm64.zip`, some tens of MB, like the `.deb`.
3. **The check**: `codesign --verify --deep --strict` passes on `music2.app` and on `Contents/MacOS/m2`; the app carries `Contents/MacOS/music2`, `Contents/MacOS/m2`, and in `Contents/Resources` std (with `std.wasm` the same bytes), every template and example, and `web/index.html`; then `m2 --version` says `m2 0.1.0`, and every template and example becomes a song that `m2 check` passes, the first-song render is written, and `release.sh: PASS: music2-0.1.0-macos-arm64.zip`. `with-plugins` is skipped there ("which isn't installed"), as no Mac copy of the test plugins is pinned.
4. **The artifact** `music2-macos`, one zip.

If step 1 fails on the program's name: `mainBinaryName` makes it `Contents/MacOS/music2`; `resources.rs` doesn't care about the name. If the check's `codesign --verify --deep --strict` fails on the ad hoc signature, that is a finding for the notes' Open Anyway step, not a reason to drop the check. Then by hand, mac-run steps 137 to 141: Open Anyway as the notes say it, New Song, play, export, an example, `m2 --version` from Terminal inside the downloaded app (whether Gatekeeper lets it run without `xattr`), and the song's `AGENTS.md`.

## Decided along the way

| # | What | Why |
|---|---|---|
| 1 | The Linux layout is a second path rule in `bundled()`, tried on every OS | Tauri's own rule (`tauri-utils`), and no `cfg`: PLAT-2 |
| 2 | m2 for the AppImage is a `.tar.gz` with `bin/m2` and `lib/music2/` (std, templates, examples), made from the `.deb`'s files | The same lookup rule finds them; one more file is simpler than teaching the AppImage to act as `m2` |
| 3 | A mounted AppImage names no `m2` in a new song's `AGENTS.md` | Its folder is gone when the app quits; the general line names the `.tar.gz` |
| 4 | `mainBinaryName: music2` | So the `.deb` installs `/usr/bin/music2`, not `music2-desktop`; `cargo build` and the checks still make `music2-desktop` |
| 5 | The `.deb` depends on `libc6 (>= 2.39)`, and `check` compares it with the newest glibc symbol the programs use | Without it apt would install it on Debian 12 and it would fail to start |
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
| Licence notices of the libraries inside the downloads | gaps, Releases and installing |
| The owner: push the tag (`git tag v0.1.0 && git push origin v0.1.0` on `main` after this merges), check the draft, publish it | |

## The check

`M2_CHECK_OUT=$SCRATCH/release/check-pr ci/check.sh --pr`, after these notes:

```
(filled in below)
```
