# The docs plan: build notes

**In short**
- The [docs plan](design.md) is built on branch `docsite`, in one track, with the owner's decisions of 2026-10-25 (D1, D2 and D3 as recommended, and `m2` inside `music2.app` moved into the track; the note's section 11).
- The user docs are **the guide**, `docs/guide/`: 21 pages in four parts, with `SUMMARY.md` as the menu. `docs/manual.md` is gone, split section by section; `music-basics.md` and the screenshot moved in. Every link that named the manual names its page now.
- **`m2 docs`** prints the guide built into that `m2` (`apps/m2/build.rs`), so it matches its version and works offline. Two pages are made from code: every `m2` command, from **one command table** (`apps/m2/src/commands.rs`, which `m2`'s usage is made from too), and every route, from the route table. A test fails when the committed page differs.
- **Every new song gets `AGENTS.md`** (the text of `templates/song-agents.md`, its m2 line naming the exact `m2` found) **and `CLAUDE.md`** (`@AGENTS.md`). The app ignores both.
- **`music2.app` carries `m2`** at `Contents/MacOS/m2`, built by the same `tauri build`; from source, on Linux too, a song names `target/release/m2`.
- The **website** is built by mdBook 0.5.4 (pinned, sha256 checked) and published by `.github/workflows/pages.yml` from `main`, once the repo is public. A **link check** (`ci/links.mjs`) covers every Markdown file; it found the 31 broken links the note counted, now mended.
- The contract changed first: FMT-23 and AGENT-12, and the layout block. The docs rule runs the whole PR tier for a change to `docs/guide/` or the contract (D1, D2).
- Tests were written first and seen failing where the behaviour was new (below). `ci/check.sh --pr` passed (section "The check").

## What was built

| Commit | What |
|---|---|
| `b14457f` | The note: the owner's decisions in section 11, `m2` inside `music2.app` in section 2 ("Which m2 it names"), section 10's row moved into the track |
| `5685162` | The contract: FMT-23 (a song's two files, the m2 they name, `m2` in the bundle), AGENT-12 (`m2 docs`, the made pages, one command table), the layout block's line |
| `b441a28` | `ci/links.mjs`, in `ci/pr.sh docs` and `lint`; the 31 broken links in `docs/designs/` mended (30 contract anchors now name `../../architecture.md`, one path), nothing else in those notes changed |
| `d2e7d64` | The guide: the manual split into `docs/guide/` (each page's In short written, `###` moved up), `SUMMARY.md`, `welcome.md`, `templates/song-agents.md` and `agents.md` (one include line); mdBook in `ci/tools.sh`; `docs/book.toml`; `ci/guide.sh` (`check`, `gen`, `serve`); every link moved; `manual.md` deleted |
| `450329d` | The docs rule: `DOCS_READ` (`docs/guide/`, `docs/architecture.md`), checked before `DOCS_FREE`, with 9 new cases; the audit comments dated again (the gates rule needs no change) |
| `7ad1281` | `m2`: the command table, the usage made from it, `m2 docs`, `build.rs`, the made `commands.md` and `routes.md`, exit 2 for an unknown command; `apps/m2/tests/docs.rs` |
| `408e9cb` | New songs get `AGENTS.md` and `CLAUDE.md` (`project::for_agents`, `agents_text`); the finder `resources::m2_program`; `m2 docs agents` names this `m2` |
| `fb8429b` | `m2` inside `music2.app`: `apps/music2-desktop/bundle-m2.sh`, `build.beforeBundleCommand`, `bundle.macOS.files`; `apps/music2-desktop/tests/bundle.rs` |
| `c0ff2d2` | `.github/workflows/pages.yml` |
| `9568d9a` | AGENTS.md, CONTRIBUTING, new gaps #40 to #42, mac-run.md steps 134 to 136 |
| `941d887` | `code.md`'s folder tree reworded (the check's one failure, below) |

## The tests, and how they failed first

| Where | Tests | Failed first |
|---|---|---|
| `ci/links.mjs` | run on the tree before any fix | yes: the 31 broken links, all in `docs/designs/`; and by hand on a scratch copy, a missing file, a missing anchor, a repo URL and a site URL that name nothing |
| `ci/docs-only-test.sh` | a guide page, a new page and the menu, a picture in the guide, a guide page beside a design note, the contract, the contract deleted (all `full`); a design note, `book.toml`, a file named like the contract elsewhere (all `docs`) | yes: the six `full` cases said `docs` |
| `apps/m2/tests/docs.rs` | the made pages are fresh; `m2 docs` lists the menu in order; each page prints as its Markdown with includes filled in, and `all` all of them; unknown pages (exit 1) and commands (exit 2), and nothing written; the guide and README name only real commands and routes | the freshness test, on the hand-written `commands.md` |
| `apps/m2/src/commands.rs` | every command `main.rs` runs is in the one table, and no other; every flag `m2` reads is shown; the usage fits 100 columns | the flag test: `--help` was read but shown nowhere |
| `apps/m2/tests/docs.rs` | `m2 docs agents` names this `m2`; `m2 new` gives a song `AGENTS.md` naming it, and `CLAUDE.md` | both |
| `crates/server/src/resources.rs` | the finder: `m2` names itself; the app names the `m2` in its bundle; from source, beside the app, then release, then debug, then not built yet; nothing on a computer without the source | written with the finder (new API) |
| `crates/server/src/project.rs`, `project_tests.rs` | the one m2 line; a template's and an example's new song, and `m2 new`'s, get both files; a template's own wins; Save As adds nothing, and copies a song's own as it is | written with the code |
| `core/session/src/gate_tests.rs`, `core/song/src/path.rs`, `crates/server/src/graph/steps_tests.rs` | the two files make no revision, no undo step and no event, and `DataPath` ignores them | pins: they passed at once, as the design expected |
| `apps/music2-desktop/tests/bundle.rs` | the config bundles `target/release/m2` as `Contents/MacOS/m2`, made by `bundle-m2.sh`, and no external binary | yes, on the old config |

## Decided along the way

| # | What | Why |
|---|---|---|
| 1 | `m2` gets into `music2.app` through `build.beforeBundleCommand` (`bundle-m2.sh`) and `bundle.macOS.files`, not `bundle.externalBin` | Tauri's build script copies an external binary at every `cargo build` of the app, so a plain `cargo build` or `cargo test` would fail before `m2` is built; and a `--config` overlay would change the one command that makes the app. The hook runs only in `tauri build`. The script builds into the source tree's `target/` whatever `CARGO_TARGET_DIR` says, so the path in the config always holds; it signs `m2` ad hoc on a Mac (the linker signs arm64 binaries already; this makes sure), and refuses a `--target` build of the app, whose `m2` would be for the wrong CPU |
| 2 | The song's `AGENTS.md` names its `m2` on one line that starts `m2 for this song: `, which the writer fills in | The site shows the general line (where `m2` is in the app and from source); a song's copy, the exact path. `m2 docs agents` fills it with its own path too, so `m2 docs agents > AGENTS.md` mends an old song; so that page alone is printed differently from the site |
| 3 | The finder's order: the running `m2`; `m2` beside the program; the source tree's release, then debug; release not built yet, with its build line; else the general line | The design's packaging decision, made testable: `find_m2` takes the program's paths and the tree, as `resources::find` does |
| 4 | `fill` got a flag, `new`: Save As's `copy_into` shares it | Without it, Save As would have added the two files to a copy (the design says it adds nothing) |
| 5 | A made page's first line is an HTML comment, "Made by `m2 docs commands` from …" | The design wants the first line to say it is made; the link check lets one comment line come before the title, and the site doesn't show it |
| 6 | A command's `usage` holds every form, one a line (`clips`' nine gestures, `layout`'s three, `point`'s two) | So the flag test checks usage lines only, as the design says; `--as`, `--why` and `--help` are in the notes both print; `--sr`, `--force` (refused, with words) and `--bras` (a test's misspelling) are listed as never shown |
| 7 | `m2`, `m2 help`, `m2 --help` and `-h` print the usage, exit 0; they are not commands of the table | An unknown command now exits 2; asking for help isn't unknown. `--m2-plugin-host` passes too: `m2` runs itself again as the plugin host |
| 8 | The table's own tests are unit tests in `commands.rs`; the rest are in `apps/m2/tests/docs.rs` | They read the table's consts, which an integration test can't reach |
| 9 | `routes.md` shows each example body on one line, as the route table holds it | Pretty JSON made the page 1,026 lines |
| 10 | `edit-url-template` is `…/edit/main/docs/{path}` | mdBook's `{path}` is the page's path from the book's folder (`guide/window.md`) |
| 11 | The issue forms (`.github/ISSUE_TEMPLATE/*.yml`) are checked too, for their full URLs | The design says they are "now checked"; they aren't Markdown |
| 12 | README links `templates/song-agents.md`, not `docs/guide/agents.md` | On GitHub, `agents.md` shows its include line as it is; the site and `m2 docs` fill it in |
| 13 | The `changes` job fetches mdBook each time (5 MB), without a cache | A cache shared with the quick job's key would keep whichever job saved first |
| 14 | The made pages are printed from code even though `build.rs` embeds the committed ones too | AGENT-12: the code is the source; the committed copies are for the site and GitHub |

## The check

`M2_CHECK_OUT=/home/jerry/m2-runs/docsite/check-pr ci/check.sh --pr` at `941d887` (the commit before these notes, which change only docs), on the build server:

```
music2 check --pr: PASS
commit   941d88784d6b on docsite (clean)
machine  Ubuntu 24.04.3 LTS, x86_64, 32 threads
tools    rustc 1.99.0, node v24.21.0, 5 min
plugins  /home/jerry/.local/share/m2-test-plugins/clap (the builds ci/plugins.sha256 records)

changes          PASS  .github/workflows/ci.yml changed since 45942df9e05c, and it isn't documentation (ci/docs-only.sh's DOCS_FREE): the whole PR tier runs
pr quick         PASS  0 min 27 s
pr clippy        PASS  0 min 5 s
pr rust          PASS  2 min 19 s
pr suites        PASS  1 min 49 s
pr renders       PASS  0 min 7 s
repo unchanged   PASS  

```

1,938 Rust tests (48 skipped, none passed only on a retry), 54/54 browser suites (1,966 checks), 12 of 12 renders. The run before it, at `9568d9a`, failed one test: `the_guide_names_only_real_commands_and_routes` found "where m2 is" in `code.md`'s folder tree, which reads as a command `m2 is`; the words changed (`941d887`), not the test.

The mac lint (`cargo clippy --target aarch64-apple-darwin --workspace --all-targets`, as `ci/check.sh` runs it on Linux) passed too, with the desktop app's bundle test and the finder in it.

The site was built once with the pinned mdBook (`ci/guide.sh check`): 25 HTML pages, the agents page with the song's text filled in, the made pages with their comment hidden, search on.

## What is left

| What | Where |
|---|---|
| The owner's clicks: make the repo public, then Settings → Pages → Source: GitHub Actions, then run the Pages workflow once | the note's section 4 |
| `m2` inside a built `music2.app`, by hand on a Mac | mac-run.md steps 134 to 136; gaps' "Owed on a Mac" |
| The guide from inside the app, the site in the app's look, a site per release | gap #40 |
| The shortcuts page, the WebSocket events and std's sounds made from code | gap #41 |
| `m2` on PATH | gap #42 |
| A song's `AGENTS.md` naming an old path after the app moves | gaps, Scripts and agents |
| The full `ci/check.sh` (the design's last step) | not run in this track: the PR tier was asked for. The timing gates read `crates/server` (changed here), so `--gates=auto` would run them |

## Answers to the review

The skeptic's review of `docsite` had six findings (R1 to R6). Each is fixed at its cause, with a test that failed first; then `main` was merged in (`4ebfa3c`).

| Id | Answer |
|---|---|
| R1 | Fixed (`03a35e9`): `README.md` is `DOCS_READ`, since `apps/m2/tests/docs.rs` reads it, so a README-only change runs the whole PR tier; the audit comment is dated again. New cases: README alone, and beside a design note, are `full`; the cases that used README as a free document use HANDOFF. (The design's "the one test that reads a doc reads the contract" stopped being true at `7ad1281`.) |
| R2 | Fixed (`767f278`): the manual's details are back in the table's `more` for `m2 watch` (its first line, `transport` lines, the closed lines and exit 0), `m2 focus` (every field), `m2 routes --json` (the shape and `limit`), `m2 call` (standard error, refused bodies, what it reaches) and `m2 point` (their OK, the bar count); `commands.md` made again by `ci/guide.sh gen`. A test holds each on the made page. |
| R3 | Fixed (`767f278`): `m2 clips` says its `json` sentence once; `more` names only the extra gestures. The same test counts it. |
| R4 | Fixed (`5034e72`): Tauri signs only its own programs, not `bundle.macOS.files`, and then seals the bundle, and gives no hook between; so `bundle-m2.sh` (the `beforeBundleCommand`) signs `m2` first with the app's own settings: `APPLE_SIGNING_IDENTITY`, else `signingIdentity`; a secure timestamp for a real identity; the hardened runtime, with the app's entitlements, unless it is off. `APPLE_CERTIFICATE`, which Tauri imports into its own keychain only as it bundles, is refused with words. `bundle-m2.sh --sign-args` prints the arguments, and `tests/bundle.rs` checks them on any OS. Still owed on a Mac: a bundle actually built (mac-run steps 134 to 136). |
| R5 | Fixed (`0ce644a`): `main` passes a leading `--as` or `--why` on, so `take_who` says they go at the end (exit 1, not 2); with nothing else given, `m2` asks for a command. A new case holds it. |
| R6 | Fixed (`115d874`): before `m2` is built, the song's line says `cargo build --locked --release -p m2 --target-dir target`, so it lands where the line points, as `bundle-m2.sh` builds it. The test pins the text. |
