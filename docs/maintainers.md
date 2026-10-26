# For the maintainer: one-time GitHub set-up

**In short**
- This is the owner's checklist for opening music2 to everyone. Each step is done once, in GitHub's settings or on the owner's own machine.
- Do part 1's steps before the repository is public. Parts 2 to 5 can follow in any order, but do them soon after.
- Nothing here is done by the code or by an agent: each step changes a setting only the owner should change.

## 1. Before it goes public

Done on 2026-10-23 to 2026-10-25, the owner's choices:
- The repository stays on the `BreadBowlAI` account. GitHub's merge queue needs an organization, so part 2's ruleset asks instead that each pull request is up to date with `main`. Moving to an organization later keeps the issues, the stars and the links (GitHub redirects the old address).
- The Code of Conduct's contact is `mxudev@gmail.com`.
- A new app icon, drawn for music2 (`apps/music2-desktop/icons/source.html`; its credit is in [the guide's Credits](guide/credits.md)).
- `.github/CODEOWNERS` names `@BreadBowlAI` (part 2).
- Commit emails stay as they are, as most projects show them. No secrets are in the history (two scans of every commit).
- The two leftover `refs/claude-teleport/*` refs of an early session are deleted.
- The macOS bundle id is `ai.breadbowl.music2` (`apps/music2-desktop/tauri.conf.json`). Changing it again means macOS asks for the microphone again.
- The Airwindows test plugin is hosted: the pre-release **Test plugins (not a music2 release)**, tag `test-plugins`, holds the zip from `~/.local/share/m2-test-plugins/dl/`. Upstream deleted that zip, and CI needs it: `ci/plugins.sh` falls back to this release when upstream fails, and checks the file against `ci/plugins.sha256` wherever it came from. CI reads the release with its own read-only token, so it works while the repository is private; anyone else can download it once it is public. Airwindows is MIT, so it may be shared. (Surge XT's package is still upstream. It is GPL-3.0: sharing a copy would also mean offering its source, so leave it there.) When `ci/plugins.sha256` names a new Airwindows zip, upload that zip to the same release.

| # | Step | Why |
|---|---|---|
| 1 | **Make it public:** **Settings → General → Danger Zone → Change visibility**. | The nightly tier (`nightly.yml`) starts only then: Actions minutes are free only for public repositories, and a private one's included minutes would go on the nightly alone. |

## 2. Pull requests and merging

Repository **Settings**:

| Where | Set | Why |
|---|---|---|
| **General → Pull Requests** | Allow merge commits. Untick squash merging and rebase merging. Tick "Automatically delete head branches". | The project merges; it never rebases. |
| **Rules → Rulesets → New branch ruleset**, for the default branch | Require a pull request before merging (0 approvals while you are the only maintainer: GitHub doesn't let you approve your own pull request; raise it when there are more). Require status checks to pass: **ci-ok** only. Inside it, tick **Require branches to be up to date before merging**. Block force pushes. Restrict deletions. Do **not** require linear history. (In an organization, use **Require merge queue** instead, merge method merge commit, and untick up to date.) | `ci-ok` is the one check that sums up all the others. Up to date means each pull request was tested on top of the newest `main` before it merges. |
| **Actions → General → Fork pull request workflows from outside collaborators** | Require approval for first-time contributors. | A newcomer's first run waits for your click, so spam can't use the shared runners. |
| **Actions → General → Workflow permissions** | Read repository contents and packages permissions. Don't allow GitHub Actions to create or approve pull requests. | The workflows ask for more only where they need it. |
| **Actions → General → Actions permissions** | Allow actions made by GitHub, plus `Swatinem/rust-cache`, the one other action the workflows use. Tick **Require actions to be pinned to a full-length commit SHA**. | An action outside that list can't run, even if a pull request adds one. A tag like `v7` can be moved by the action's maker; a full commit can't. Every action in the workflows is pinned that way already. |

**CODEOWNERS** (`.github/CODEOWNERS`) names `@BreadBowlAI` for the files that decide whether a pull request passes: `/ci/`, `/.github/`, `/deny.toml`, `/.config/` and `/e2e/run.mjs` (the checks, the workflows, the licence rules, how the Rust tests run, and how the browser suites are judged). Those scripts run from the pull request's own files, so a change to them needs your careful review, and GitHub now asks you for it.

To make that review a must, make a second branch ruleset for the default branch, with only **Require a pull request before merging** and, inside it, **Require review from Code Owners** (0 approvals). Add **Repository admin** to its bypass list, for pull requests only, so your own pull requests aren't stuck: GitHub doesn't let you approve your own. Keep the first ruleset without a bypass, so `ci-ok` holds for everyone.

## 3. Security

Repository **Settings → Advanced Security** (called "Code security" on some accounts):

| Turn on | Why |
|---|---|
| Private vulnerability reporting | [SECURITY.md](../SECURITY.md) tells people to use it. |
| Dependabot alerts, and Dependabot security updates | `.github/dependabot.yml` asks for weekly version updates: the minor and patch bumps of each ecosystem in one pull request, each major bump alone, and only releases at least 7 days old. These add alerts and fixes for known holes, which come at once: the 7 days hold only version updates. |
| Secret Protection (secret scanning), and push protection | A pushed token is caught, or refused before it lands. |

A fix Dependabot can't make: when a library pins an exact version of one it uses, an alert on that one stays open
until the library moves. Then `overrides` in `web/package.json` names the fixed version. Today: Monaco 0.57 pins
DOMPurify 3.4.15, so `web/package.json` asks for 3.4.17 (five alerts, October 2026). Once a Monaco update pins 3.4.17
or later, delete that `overrides` entry.

## 4. The community

| Where | Do | Why |
|---|---|---|
| **Settings → General → Features** | Turn on Discussions. | The issue form's "A question" link points there. |
| **Issues → Labels** | Make sure these exist: `bug`, `idea`, `good first issue`, `help wanted`. | The issue forms add `bug` and `idea`. GitHub ignores a label that doesn't exist. |
| **Issues** | Label a few small gaps from [gaps.md](gaps.md) `good first issue`. | Newcomers and agents start there. |

## 5. The timing gates, on a quiet machine

The timing gates measure real-time audio, so they need a quiet machine. GitHub's shared runners can't judge them, and the repository has no self-hosted runner (a pull request from a fork could reach it). So they run on a maintainer's own machine, every night, with `ci/nightly.sh`.

That script builds and runs whatever is on `main`, and every dependency's build script, with nobody watching. If merged code or a dependency ever turned bad, it could take whatever the user running it holds. So it runs as a user of its own, with one small token.

| Step | Do | Why |
|---|---|---|
| 1 | Pick a machine you trust that is quiet at night. Make an OS user just for this, with no other credentials: no `gh` login, no ssh keys, no cloud keys. On Linux: `sudo adduser --disabled-password m2-nightly`. On the Mac: a standard user that stays logged in (fast user switching). | A bad build could then take nothing of yours. The Mac needs a login session for the app's test and G12. |
| 2 | As that user: install [rustup](https://rustup.rs) and Node.js 24, clone the repository, and add the test plugins (on Linux `ci/plugins.sh`; on the Mac in `~/Library/Audio/Plug-Ins/CLAP`). | The check needs them, as on any machine. |
| 3 | Try it once by hand: `ci/nightly.sh`. It fetches `origin/main`, clones it fresh into `~/.cache/m2-nightly/clone`, runs `ci/check.sh --gates` there, and writes each night's logs and `summary.txt` under `~/.cache/m2-nightly/runs/`. `last.txt` is the newest summary. | It never touches the checkout it lives in, `~/Music` or `~/.music2`. Its summary warns if the user holds other credentials. |
| 4 | To show the result on GitHub, make a **fine-grained** personal access token: your GitHub **Settings → Developer settings → Personal access tokens → Fine-grained tokens**. Resource owner: the account that owns music2 (`BreadBowlAI` today; an organization, if it moves, which may ask you to approve the token). Repository access: only music2. Permissions: **Commit statuses: Read and write**, nothing else. Save it as that user's `~/.cache/m2-nightly/token`, then `chmod 600` it. | Then `ci/nightly.sh --report` posts the verdict as the commit status **nightly/gates**. That is all the token can do. The script refuses any other kind of token (one that doesn't start `github_pat_`). |
| 5 | On Linux, add one line to that user's crontab (`crontab -e`), for every night at 02:17: `17 2 * * * cd /path/to/music2 && ci/nightly.sh --report >> "$HOME/.cache/m2-nightly/cron.log" 2>&1` | |
| 6 | On the Mac, start it from a LaunchAgent in that user's login session instead of cron. | The app's test and G12 need the session. |
| 7 | Renew the token before it expires (GitHub emails you first). | An expired token makes `--report` fail; the check itself still runs. |

A failed night shows as a red **nightly/gates** status on that commit. Its description is the check's first line; `summary.txt` has every row.

## 6. How many checks can run at once

GitHub runs a limited number of jobs at the same time for the whole account. Standard runners, per the [limits page](https://docs.github.com/en/actions/reference/limits):

| Plan | Jobs at once | of them on macOS | Pull request runs at once (7 jobs each) |
|---|---|---|---|
| Free | 20 | 5 | about 2 |
| Pro | 40 | 5 | about 5 |
| Team | 60 | 5 | about 8 |
| Enterprise | 500 | 50 | about 70 |

One run of the pull request tier is 9 jobs: `changes` first, then 7 at once (`quick`, `clippy`, three `rust` parts, two `suites` parts), then `ci-ok`. A docs-only run is 2 jobs: `changes`, which finds that it is docs only and checks the docs rule, and `ci-ok`.

Each pull request that merges runs the tier twice: on the pull request, and on the push to `main`. The push repeats the tree the pull request just tested; it is there to save the build caches every pull request starts from. So one merged pull request costs about 18 jobs, plus one run each time it is pushed again or brought up to date with `main`. (With a merge queue, in an organization, the queue adds a third run: about 27 jobs.) The rest wait their turn.

When the limit binds, runs wait in a queue: nothing fails, it is only slower. What helps, cheapest first:

1. Every run of a pull request cancels its older runs (already set in the workflow).
2. Docs-only pull requests run one quick job (already set).
3. First-time contributors wait for your approval (part 2).
4. In an organization, the merge queue tests pull requests in groups, so many don't each need bringing up to date: raise its group size in the ruleset.
5. Run only the parts that save caches on the push to `main` (a small change to `ci.yml`, listed in [gaps.md](gaps.md)).
6. A bigger plan (Team, Enterprise), or ask GitHub Support to raise the limit.
