# The docs plan: design

**In short**
- The owner said yes to the outline on 2026-10-25: a short `AGENTS.md` in every new song, `m2 docs`,
  a website on GitHub Pages, one source, docs split by reader, no MCP. This note settles the details,
  so one builder can do it in one track.
- The user docs move to `docs/guide/`: one Markdown file a page, 20 pages, in four parts. The 107 KB
  manual is split into them, section by section, and goes. The contract, the design notes, the goals and
  the gap list stay where they are, for contributors.
- The website is made with **mdBook**: one pinned binary, menus from `SUMMARY.md`, search built in. A new
  workflow builds it on every push to main and puts it on GitHub Pages, at
  https://breadbowlai.github.io/music2/ once the repo is public.
- `m2 docs` prints the same pages, built into `m2`, so they match its version and work offline.
- Two pages are made from code, so they can't drift: every `m2` command (from one new command table)
  and every API route (from the route table). A test fails when the committed page differs.
- A new song gets `AGENTS.md` (for Codex, Cursor, Copilot and others) and `CLAUDE.md`, one line,
  `@AGENTS.md` (for Claude Code). The text has one source, `templates/song-agents.md`, which is also the
  guide's "Working on a song as an agent" page.
- A new link check covers every Markdown file in the repo. It and the site's build run in the docs job,
  which every pull request runs, docs only or not.
- `music2.app` carries `m2` beside its own program (`Contents/MacOS/m2`), built by the same build. A
  song's `AGENTS.md` names the exact path of the `m2` that made it (section 2).
- About 9 commits. The owner answered the three choices on 2026-10-25 (section 11), all as
  recommended, and moved `m2` inside `music2.app` into this track.
- Built on branch `docsite`: what was built and decided is in [build-notes.md](build-notes.md).

## 1. What is there today

| Thing | Today |
|---|---|
| User docs | `docs/manual.md` (107 KB, 17 sections) and `docs/music-basics.md` (14 KB) |
| Contributor docs | `docs/architecture.md` (the contract), `goals.md`, `gaps.md`, `kernel-abi.md`, `maintainers.md`, `mac-run.md`, `song-as-program.md`, `code-graph-editing-*.md`, `designs/` |
| Links into the manual | README.md (8), song-as-program.md (4), gaps, goals, maintainers, music-basics, HANDOFF, one design note. No code, test or app menu links to it |
| The app's Help menu | Make Your First Song, Shortcuts, About. No link to any doc |
| Tests that read docs | One: `the_refusals_read_as_the_contract_quotes_them` (`crates/server/src/api_tests.rs`) reads `docs/architecture.md` |
| `m2`'s commands | One hand-written `USAGE` string in `apps/m2/src/main.rs`, and a second, longer table in the manual |
| The routes | One table, declared once with words and an example (`routes!`, AGENT-8); `m2 routes` prints it |
| New songs | `create_at` and `create` copy a template or an example (`copy_tree`: all but dot files and `template.toml`/`example.toml`) |
| m2 in the app | `music2.app` carries std, templates and examples, but not `m2`. `m2` isn't on PATH unless the user puts it there |
| Docs-only pull requests | Skip the builds (`ci/docs-only.sh`: all of `docs/` is free); they run the docs job, seconds |
| Broken links | 31 of 838 relative links, all in `docs/designs/` (30 anchors meant for the contract, one path) |

## 2. What a song folder gets

| File | Text | Who reads it |
|---|---|---|
| `AGENTS.md` | `templates/song-agents.md`, its one m2 line naming this computer's `m2` (below) | Codex, Cursor, GitHub Copilot's agent; Gemini CLI and Aider when told to |
| `CLAUDE.md` | the one line `@AGENTS.md` | Claude Code, which imports the file named after `@` |

- Not a symlink: `copy_tree` skips symlinks, and Windows makes them hard.
- No `GEMINI.md`: two files are enough clutter in a musician's folder. Gemini CLI can be set to read
  `AGENTS.md`.
- The source is named `song-agents.md`, not `AGENTS.md`, so a contributor's agent working in
  `templates/` doesn't take it as its own instructions.
- Who writes them: New Song and Examples in the app (`project::create`), and `m2 new` (`create_at`).
  Both write the two files after the copy, when the copy has none, so a template with its own wins.
  Save As (`copy_into`) copies the song as it is and adds nothing.
- The text is built in: `pub const SONG_AGENTS: &str = include_str!("../../../templates/song-agents.md")`
  in `crates/server/src/project.rs`. So it can't be missing at run time, and `templates/` is already
  outside the docs and gates rules' free paths.
- Every template and every example gets them, through the code. None is committed into
  `templates/*/` or `examples/*/`.
- Songs made before this have none. `m2 docs agents > AGENTS.md` writes one (section 6), its m2 line
  naming that `m2`.
- The app ignores both: the build never reads them, and they are no part of a revision. The builder
  checks how the watcher's `DataPath` classes them, and pins with a test that editing one while the app
  has the song open makes no build and no undo step.

### Which m2 it names (the owner's decision, 2026-10-25)

The template's "Finding m2" section has one line that starts `m2 for this song: `. On the site it says
where m2 is in general (`music2.app/Contents/MacOS/m2` in the app, `target/release/m2` in music2's
source folder). In a song's copy, the writer puts the exact path there, found in this order:

| # | Where | When |
|---|---|---|
| 1 | the running program itself, when it is `m2` | `m2 new`, `m2 docs agents` |
| 2 | `m2` beside the running program | the app: `music2.app/Contents/MacOS/m2`; from source, `target/release/m2` beside `target/release/music2-desktop` |
| 3 | the source tree's `target/release/m2`, then `target/debug/m2` | the app run from source (Linux has no `.app`: this is its path) |
| 4 | the source tree's `target/release/m2`, not built yet, with the line that builds it | from source, before m2 is built |
| 5 | none: the general line stays | an app whose bundle has no m2, on a computer without the source |

The path is the real one (links resolved). The finder is `m2_server::resources::m2_program`, beside
how std and the templates are found, with the same kind of tests (a fake `music2.app`).

**`m2` inside `music2.app`.** `tauri build --bundles app` (the one command that makes the app) runs
`build.beforeBundleCommand` after the app's own build: `apps/music2-desktop/bundle-m2.sh`, which builds
`m2` (`cargo build --locked --release -p m2 --target-dir target`, so the path is fixed) and, on a Mac,
signs it ad hoc as the app is signed. `bundle.macOS.files` then copies `target/release/m2` to
`Contents/MacOS/m2`. Not `bundle.externalBin`: Tauri's build script copies an external binary at every
`cargo build` of the app, so a plain `cargo build` or `cargo test` would fail before `m2` is built.
The bundled `m2` finds std, the templates and the examples in `Contents/Resources`, as `resources.rs`
already does for a program in `Contents/MacOS`, and runs itself again as the plugin host. A test pins
the config to the finder; the Linux check's mac lint type-checks the finder for the Mac.

### What the page says

About one page. A test caps it at 80 lines. It has no relative links: it is read inside a song folder.

```
# Working on a music2 song
**In short**: this folder is a song made in music2, a music studio where a song is a small program.
  You help from outside with the m2 command; the musician sees each change you make land in the app.
  For more, run `m2 docs` (built into m2, it matches this version). Online: <site>/agents.html
## What is here         song.ts, tracks/ (how it sounds), data/ (what plays), assets/, music2.toml,
                        layout.json; .music2/ is this machine's: never touch it
## Finding m2           one line, "m2 for this song: `<path>`", the exact m2 found when the song was
                        made (below); if it doesn't run: ask the musician
## The commands you'll use most   a table of about 12: check, controls, set, add-track, add-effect,
                        clips, edit, write, listen, point, focus, undo, routes and call
## Rules                1. Every command that writes ends with --as <your name> --why "<a few words>".
                        2. While the app has the song open, change files only with m2, never directly.
                        3. Run m2 check . after every edit, and fix what it says.
                        4. You can't hear. m2 listen measures; say its numbers as numbers, then ask the
                           musician to listen (m2 point shows them where; V compares before and after).
                        5. Explain each change to the musician in plain words: they may be new to music.
                        6. Exit status 3 means the musician is busy with that part: wait, then try again.
                        7. Playing, recording, solo and the view are the musician's. Ask.
```

## 3. The page layout

The guide is flat: every page in `docs/guide/`, named by its topic, so `m2 docs window` is
`docs/guide/window.md`. The site's menu groups them by part, from `SUMMARY.md`. Each page starts
with `# Title`, then **In short** (AGENTS.md rule 10). `###` headings move up to `##`.

| Part (reader) | Page | From |
|---|---|---|
| (first page, the site's home) | `welcome.md` Welcome to music2 | README's In short and the screenshot; who each part is for; links to CONTRIBUTING and the contract on GitHub |
| **Getting started** (new to music) | `install.md` Install and run music2 | manual: Run the app |
| | `first-song.md` Your first song | manual: Your first song |
| | `music-basics.md` Music basics | `docs/music-basics.md`, moved |
| | `words.md` Words you'll see | manual: Words you'll see |
| **The manual** (musicians) | `what-you-can-do.md` What you can do | manual: its top, and In short: what you can do |
| | `window.md` The window | manual: The window |
| | `audio.md` Audio, recording and MIDI files | manual: the same |
| | `songs.md` New songs, the songs folder and export | manual: the same |
| | `saving.md` Saving and undo | manual: the same |
| | `shortcuts.md` Shortcuts | manual: the same |
| | `sounds.md` Sounds and effects | manual: Sounds and effects (std) |
| | `plugins.md` CLAP plugins | manual: the same |
| | `code.md` How a song is written | manual: the same |
| | `credits.md` Credits | manual: Credits |
| **Working on a song as an agent** (agents) | `agents.md` Working on a music2 song | `{{#include ../../templates/song-agents.md}}`: the song's AGENTS.md |
| | `changes.md` How your changes reach the song | manual: the paragraphs after m2's table (the app open, `--as` and `--why`, waiting, exit 3), and "Or just edit the files" |
| | `listening.md` Listening | manual: Listening |
| | `api.md` The local API | manual: Scripts and agents (the tools key, busy, the musician's controls, Tx, the routes used most, the WebSocket events) |
| **Reference** (agents and scripts) | `commands.md` m2 commands | made by `m2 docs commands` (section 5) |
| | `routes.md` API routes | made by `m2 docs routes` (section 5) |

- Also in `docs/guide/`: `SUMMARY.md` (the menu) and `images/screenshot.png` (moved from `docs/`).
- `docs/book.toml` sits beside it: `src = "guide"`, and the site is built into `target/guide`.
- The manual's "Working on music2" and "Design docs" sections go: the welcome page links
  CONTRIBUTING.md instead.
- **Stays where it is, contributor only:** `architecture.md`, `goals.md`, `gaps.md`, `kernel-abi.md`,
  `maintainers.md`, `mac-run.md`, `song-as-program.md`, `code-graph-editing-*.md`, `designs/`, and the
  top-level files. The guide links to them by full GitHub URL
  (`https://github.com/BreadBowlAI/music2/blob/main/docs/gaps.md`), never by a relative path: the site
  serves only `docs/guide/`.
- `docs/manual.md` is deleted, with no stub. `docs/designs/README.md` gets one line: notes before
  2026-10-25 say "the manual"; it is now the guide.

### Every link keeps working

| Where | What changes |
|---|---|
| README.md | Its 8 links go to the guide's pages (`docs/guide/first-song.md`, `install.md`, `code.md`, `api.md`, `credits.md`…), its picture to `docs/guide/images/screenshot.png`. One new line: the guide is also a website, and `m2 docs` prints it |
| goals, gaps, maintainers, song-as-program, HANDOFF, `designs/step8/build-notes-studio.md` | Their links to `manual.md#…` and `music-basics.md` point at the new pages |
| CONTRIBUTING.md, AGENTS.md | `docs/` row: the guide, then the rest. AGENTS.md's commands table gets the three `ci/guide.sh` commands |
| Issue templates | Link `gaps.md` and `goals.md` by URL: unchanged, and now checked |
| App Help menu, code, tests | Nothing links to the manual. The one test that reads a doc reads the contract, which stays |
| 31 broken links in `docs/designs/` | Fixed first, so the link check starts green |

The link check (section 7) proves it: a link that breaks fails the pull request.

## 4. The site

### mdBook or MkDocs Material

| | mdBook | MkDocs Material |
|---|---|---|
| What it needs | one static binary, Linux and Mac builds on GitHub | Python, and a dozen pip packages pinned with hashes |
| Menus | from `SUMMARY.md`, with parts | from `mkdocs.yml` |
| Search | built in, runs in the browser | built in, better |
| Looks | plain and readable, light and dark | nicer |
| Fits music2 | Rust's own; pinned like `ci/tools.sh`'s tools | a new language in CI; its team is building a successor (Zensical) |

**Pick mdBook.** It is one checked download, plain Markdown, and links between `.md` files work both
on GitHub and on the site. A look closer to the app (the taste guide's fonts and colours) can come
later as one small CSS file.

### Settings (`docs/book.toml`)

```toml
[book]
title = "music2"
language = "en"
src = "guide"

[build]
build-dir = "../target/guide"   # under target/, which git ignores
create-missing = false          # a page SUMMARY.md names but isn't there fails the build

[output.html]
site-url = "/music2/"
git-repository-url = "https://github.com/BreadBowlAI/music2"
edit-url-template = "https://github.com/BreadBowlAI/music2/edit/main/docs/guide/{path}"

[output.html.search]
enable = true
```

The keys are checked against the pinned version's own docs when it is pinned.

### The pinned binary

- `ci/tools.sh` gets a fourth tool, `mdbook`: the newest release at least two weeks old, the Linux
  x86-64 and Apple silicon archives, each checked against the sha256 GitHub records for it, as
  nextest, cargo-deny and actionlint are. `M2_TOOLS=path` works for it too.
- `ci/guide.sh` (new, bash 3.2) has three commands:

| Command | What it does |
|---|---|
| `ci/guide.sh check` | `node ci/links.mjs`, then `mdbook build docs` into `target/guide`. Any warning mdBook prints fails it |
| `ci/guide.sh gen` | rewrites `commands.md` and `routes.md` with `m2 docs commands` and `m2 docs routes` (a debug `m2`, run with a scratch HOME, `M2_AUDIO=none`, `M2_MIDI=none`) |
| `ci/guide.sh serve` | `mdbook serve docs`: a preview at http://localhost:3000 while you write |

### The workflow (`.github/workflows/pages.yml`)

- **When:** every push to main, and by hand (`workflow_dispatch`). Never on a pull request: the
  docs job already builds the site there (section 7).
- **build** job: `actions/checkout` (no credentials kept), `ci/guide.sh check`, then
  `actions/upload-pages-artifact` with `target/guide`.
- **deploy** job: `actions/deploy-pages`, in the `github-pages` environment, with `pages: write` and
  `id-token: write` for this job only. The workflow's own token is `contents: read`.
- Only GitHub's own actions, each pinned by its full commit with the version in a comment, as
  `ci.yml` does. Dependabot's github-actions group already covers a new workflow file.
- `concurrency: pages`, never cancelled, so two pushes deploy in order.
- Both jobs run only while the repo is public (`if: ${{ !github.event.repository.private }}`), so
  main shows no red cross before then.
- `actionlint` checks it in the lint step, as every workflow. It is not part of `ci-ok`: it runs
  after the merge, and the docs job already proved the same build.

### What the owner clicks

1. Make the repo public (Settings → General → Danger Zone). GitHub's free plan has Pages only for
   public repos; a site is public either way.
2. Settings → Pages → Build and deployment → Source: **GitHub Actions**. Nothing else to pick.
3. Actions → Pages → **Run workflow**, once (or wait for the next push to main).
4. Optional: on the repo's home page, the gear by About → tick "Use your GitHub Pages website".

The site is then at **https://breadbowlai.github.io/music2/**. No custom domain was found (no CNAME).

## 5. Pages made from code

Both are committed under `docs/guide/`, so the site and GitHub show them. Their first line says
they are made, and by what. `m2 docs` makes them at run time from the code itself.

| Page | Made from | How |
|---|---|---|
| `commands.md` | a new command table, `apps/m2/src/commands.rs` | each command's usage line in a code block, then its words; an intro and examples from consts in the same file |
| `routes.md` | `m2_server::api::all()` | the routes for agents first: method and path as a heading, its words, whether it writes, its body limit if over 2 MiB, its example body; then the musician's |

### The command table

Today `m2`'s commands are written twice: `USAGE` and the manual's longer table. Both become one table:

```rust
pub struct Command { name: &'static str, usage: &'static str, words: &'static str, more: &'static str }
pub const COMMANDS: &[Command] = &[ /* new, check, render, listen, … in USAGE's order */ ];
```

- `usage` is the line as USAGE has it; `words` is USAGE's text; `more` is what the manual adds (it
  can be empty). The builder merges the two texts and keeps the better words in one place.
- `m2`'s usage text is made from the table (wrapped as today), and so is `commands.md`. USAGE's
  closing paragraphs become a const both print.
- `m2 docs` is a row of it too.
- An unknown command says so and exits 2 (today it prints the usage and exits 0). The test below
  needs that, and it helps agents.

### Tests (`apps/m2/tests/docs.rs`, AGENT-12)

| Test | It fails when |
|---|---|
| The made pages are fresh | `m2 docs commands` or `m2 docs routes` differs from the committed page. Its message names `ci/guide.sh gen` |
| One table | the names `main.rs`'s match and `edits::handles` take differ from the table's (read from the source files) |
| Every flag is shown | a `"--flag"` string in `apps/m2/src` is in no command's usage line, but for a short list of refused ones (`--sr`) |
| The guide names real things | a page shows `m2 <word>` that is no command, or an `/api/…` path the route table hasn't got |

Not made from code yet, and written down as gaps: the shortcuts page (from the web app's own table),
the WebSocket events, and std's sounds and their knobs.

## 6. `m2 docs`

```
$ m2 docs
music2's guide, built into this m2 (0.1.0). Print a page: m2 docs <topic>. All of it: m2 docs all.
Online, for the newest music2: https://breadbowlai.github.io/music2/

Getting started
  welcome            Welcome to music2
  install            Install and run music2
  …
Working on a song as an agent
  agents             Working on a music2 song
  …
Reference
  commands           m2 commands
  routes             API routes

A link to another page, such as window.md, is m2 docs window.
```

- `m2 docs <topic>` prints that page's Markdown exactly, includes filled in: what the site shows,
  as source. No pager, no wrapping. So `m2 docs commands > docs/guide/commands.md` is how a made page
  is written.
- `m2 docs all` prints every page in menu order, so an agent can search it
  (`m2 docs all | grep -n automate`).
- An unknown topic: `there's no page <x>: m2 docs lists them`, exit 1.
- It needs no song and no app, and it reads and writes nothing: it is answered before `m2` starts
  anything else.
- **How pages get in:** a build script, `apps/m2/build.rs`, reads `docs/guide/SUMMARY.md`. For each
  page in order it notes its part, its topic (the file name) and its title (its first `# ` line). It
  fills each whole-line `{{#include path}}` with that file, and refuses any other `{{#…}}`, so no
  mdBook command reaches the terminal. It writes the table into `OUT_DIR`, each page by
  `include_str!`, and tells cargo to build again when anything in `docs/guide/` or an included file
  changes. So a page added to `SUMMARY.md` is in `m2 docs` with nothing else to edit.
- `commands` and `routes` are made from code when asked (section 5), not from the committed copies.

Its other tests, in the same file: `m2 docs` lists every page of `SUMMARY.md` in order; every topic
prints its page with includes filled in; `all` prints them all; an unknown topic exits 1;
`m2 docs agents` is `templates/song-agents.md` with its m2 line naming that `m2`; and `m2 new`
(scratch HOME) makes a song whose `AGENTS.md` is that text and whose `CLAUDE.md` is `@AGENTS.md\n`.

## 7. The checks

### The link check (`ci/links.mjs`, Node, no packages)

| It checks | In |
|---|---|
| Every relative link names a file that is there, and every `#anchor` a heading in it (GitHub's ids; mdBook makes the same for plain headings) | every `.md` file git tracks |
| A link to `https://github.com/BreadBowlAI/music2/blob/main/<path>` (or `/tree/main/`) names a path in the tree | every `.md` file |
| A `https://breadbowlai.github.io/music2/<page>.html` link names a page of the guide | every `.md` file, and `templates/song-agents.md` |
| A guide page links out of `docs/guide/` only by full URL | `docs/guide/` |
| Every `.md` file in `docs/guide/` is in `SUMMARY.md` once; names are unique; each page starts with `# Title` and **In short**; an include names a file that is there | `docs/guide/` |
| `templates/song-agents.md` has no relative link, and is 80 lines at most | that file |

It never fetches a web page: the answer is the same on any machine, any day. Code blocks and code
spans are skipped.

### Where each check runs

| Check | `ci/pr.sh` step | CI | `ci/check.sh --pr` and the full check |
|---|---|---|---|
| Links, the site's build, the docs rule's cases | `docs`, and `lint` | the changes job on every pull request; the quick job | yes, both |
| `m2 docs`, the made pages, the command table (AGENT-12) | `rust` | rust (3 parts) | yes, both |
| The song's two files (FMT-23) | `rust` | rust | yes, both |
| `pages.yml` | `lint` (actionlint) | quick | yes, both |

### The docs rule must change

`ci/docs-only.sh` lets a pull request that touches only `docs/` skip the builds. Two paths under
`docs/` are now read by a build or a test, so they must run everything:

- `docs/guide/`: `m2`'s build script embeds it, and its tests read it.
- `docs/architecture.md`: the test in `api_tests.rs` reads it today already. This is a hole now:
  a docs-only change to the contract skips the test that checks it.

The rule gets a list of these, checked before `DOCS_FREE`, with cases in `ci/docs-only-test.sh` (a
guide page: full; the contract: full; a design note: docs). Its audit comment is checked again and
dated. `docs/book.toml` stays free: only mdBook reads it, in the docs job, which always runs.
`ci/gates-auto.sh` needs no change: `m2-gates` builds `m2-server`, not `m2`, and `m2-server` reads only
`templates/song-agents.md`, which isn't free already. The builder checks this again, as the file says.

## 8. The contract

Written first, in the commit before any code (AGENTS.md: the contract first).

- **FMT-23** (Presentation and local files). A new project, from a template or an example (New Song,
  Examples, `m2 new`), MUST get `AGENTS.md`, the text of `templates/song-agents.md` with its m2 line
  naming the exact path of the `m2` found (section 2), and `CLAUDE.md`, the line `@AGENTS.md`, unless
  what it is copied from has its own. Save As copies a song as it is. The build never reads them, and
  they are no part of a revision. `music2.app` MUST carry `m2` beside its own program
  (`Contents/MacOS/m2`), built by the build that makes the app. The layout block gets their line.
- **AGENT-12** (Agents). `m2 docs` MUST print the guide built into that `m2`: the pages `SUMMARY.md`
  names, in its order, includes filled in. Pages made from code MUST be printed from it, and a test MUST
  fail when the committed page differs. Every `m2` command MUST be declared once, in one table that
  `m2`'s usage and `m2 docs commands` are made from.

## 9. Order of work and size

Each step is one commit that passes `ci/check.sh --pr`. One full `ci/check.sh` at the end.

| # | Step | Size |
|---|---|---|
| 1 | The contract: FMT-23, AGENT-12, the layout line | small |
| 2 | `ci/links.mjs`; fix the 31 broken links; run it in `ci/pr.sh docs` and `lint` | small |
| 3 | mdBook in `ci/tools.sh`; `docs/book.toml`; `ci/guide.sh`. Split the manual into `docs/guide/` (section 3): a mechanical move, one `##` section a page, then each page's In short and links. Move music-basics and the screenshot. Write `SUMMARY.md`, `welcome.md` and `templates/song-agents.md`. `commands.md` and `routes.md` hold the manual's hand tables for now. Update every link; delete `manual.md` | medium: most of the text moves, little is new |
| 4 | The docs rule: `docs/guide/` and `docs/architecture.md` run everything; its cases | small |
| 5 | `m2`: the command table, usage made from it, exit 2 for an unknown command, `m2 docs`, `build.rs`, the two made pages (`ci/guide.sh gen`), `apps/m2/tests/docs.rs` | the largest: about 900 lines, half of it text moved from USAGE and the manual |
| 6 | New songs get `AGENTS.md` and `CLAUDE.md` (`project.rs`), with tests: a template, an example, a template with its own, Save As adds nothing, the watcher ignores them. The m2 finder (`resources::m2_program`) and its m2 line | small |
| 6b | `m2` inside `music2.app`: `beforeBundleCommand`, `apps/music2-desktop/bundle-m2.sh`, `bundle.macOS.files`; a test that pins the config to the finder | small |
| 7 | `.github/workflows/pages.yml` | small |
| 8 | The rest of the docs: README, AGENTS.md, CONTRIBUTING, HANDOFF, `designs/README.md`, new gaps (below); the full check | small |

About 1,700 lines of code, scripts and tests, plus the move of about 120 KB of docs. One builder,
one track.

## 10. Not in this track

| What | Why not now |
|---|---|
| MCP | The owner's no, for now |
| Help → "music2 Guide" in the app, opening the site | The site isn't public yet; a menu row needs the taste guide's review |
| `m2` on PATH (a link in `/usr/local/bin`, or a menu row that makes one) | The song's AGENTS.md names m2's exact path, so an agent needs no PATH. A new gap. (`m2` inside `music2.app` moved into this track: the owner, 2026-10-25) |
| The shortcuts page, the WebSocket events and std's sounds made from code | Each needs its own generator. New gaps |
| The site in the app's look | One CSS file later |
| A search command (`m2 docs --find`) | `m2 docs all \| grep` does it |
| A copy of the site per release | There are no releases yet. The site follows main; `m2 docs` follows the binary |

## 11. The owner's decisions

The owner answered on 2026-10-25, each as recommended, and moved one row of section 10 into the track:

| # | Decided | So |
|---|---|---|
| D1 | Yes: a change to `docs/guide/` runs the whole PR tier (builds, tests, browser suites), not just the docs job, since `m2` embeds the pages | the docs rule lists `docs/guide/` as not free (section 7) |
| D2 | Yes: a change to the contract alone runs the whole PR tier too, because a test reads it | the docs rule lists `docs/architecture.md` as not free |
| D3 | Yes: the song's text lives in `templates/song-agents.md`, and the guide page includes it | `agents.md` is one `{{#include}}` line |
| Packaging | `m2` ships inside `music2.app`, beside the app's own program, built and bundled by the same build. The song's `AGENTS.md` names the exact path of the `m2` it finds; on Linux, the source tree's `target/release/m2` | section 2's "Which m2 it names", FMT-23, step 6b |

Also yours, outside the build: making the repo public, then the clicks in section 4.
