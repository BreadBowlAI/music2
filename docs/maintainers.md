# For the maintainer: one-time GitHub set-up

**In short**
- This is the owner's checklist for opening music2 to everyone. Each step is done once, in GitHub's settings or on the owner's own machine.
- Do part 1 before the repository is public. Parts 2 to 5 can follow in any order, but do them soon after.
- Nothing here is done by the code or by an agent: each step changes a setting only the owner should change.

## 1. Before it goes public

| # | Step | Why |
|---|---|---|
| 1 | **Move the repository to an organization.** Make a free GitHub organization, then the repository's **Settings → General → Danger Zone → Transfer ownership**. Then change `BreadBowlAI/music2` in `README.md`, `CONTRIBUTING.md` and `.github/ISSUE_TEMPLATE/` to the new name. | Today the repository belongs to a user account. The merge queue works only for public repositories owned by an organization. GitHub also redirects the old address. |
| 2 | **Fill in the Code of Conduct's contact.** In `CODE_OF_CONDUCT.md`, replace `[INSERT CONTACT METHOD]` with a private way to reach you, such as an email address used only for this. | People need a private way to report bad behaviour. |
| 3 | **Say where the app icon came from** (`apps/music2-desktop/icons/`). If it was drawn from Lucide's `music` icon, Lucide's ISC licence notice must be kept with it; if you made it, it is yours. | It is the one file in the repository whose origin isn't written down. |
| 4 | **Decide about the commit emails.** Every commit's author email becomes public with the repository. Old commits keep theirs unless the history is rewritten. For new commits, turn on **Settings → Emails → Keep my email addresses private** and **Block command line pushes that expose my email** in your own GitHub account, and set `git config user.email` to the `users.noreply.github.com` address GitHub shows there. | Your choice: rewriting history changes every commit's id. |
| 5 | **Delete the two leftover `refs/claude-teleport/*` refs** on GitHub (`git ls-remote origin` lists them; `git push origin :<ref>` deletes one). | One holds unfinished work that is not on `main`. They would become public too. |
| 6 | **Decide on the wasmtime upgrade** (49.0.2 to 49.0.4: a change to `Cargo.lock`, then the full check). | RUSTSEC-2026-0379 is a Cranelift miscompile on Apple silicon. [SECURITY.md](../SECURITY.md) lists it as known. |
| 7 | **Decide on the macOS bundle id** `com.suite2.music2` (`apps/music2-desktop/tauri.conf.json`). Changing it means macOS asks for the microphone again. | It names an unreleased project. Keeping it is fine. |
| 8 | **Host the Airwindows test plugin.** Make a release whose tag is `test-plugins`, and upload the Airwindows zip from `~/.local/share/m2-test-plugins/dl/` as its file. `ci/plugins.sh` falls back to that release when upstream fails. | Upstream deleted the zip, and CI needs it. Airwindows is MIT, so it may be shared. (Surge XT's package is still upstream. It is GPL-3.0: sharing a copy would also mean offering its source, so leave it there.) |
| 9 | **Make it public:** **Settings → General → Danger Zone → Change visibility**. | |

## 2. Pull requests and merging

Repository **Settings**:

| Where | Set | Why |
|---|---|---|
| **General → Pull Requests** | Allow merge commits. Untick squash merging and rebase merging. Tick "Automatically delete head branches". | The project merges; it never rebases. |
| **Rules → Rulesets → New branch ruleset**, for the default branch | Require a pull request before merging (1 approval). Require status checks to pass: **ci-ok** only. Require merge queue (merge method: merge commit; only merge pull requests that pass). Block force pushes. Restrict deletions. Do **not** require linear history. | `ci-ok` is the one check that sums up all the others. The queue tests each pull request on top of the newest `main` before it merges. |
| **Actions → General → Fork pull request workflows from outside collaborators** | Require approval for first-time contributors. | A newcomer's first run waits for your click, so spam can't use the shared runners. |
| **Actions → General → Workflow permissions** | Read repository contents and packages permissions. Don't allow GitHub Actions to create or approve pull requests. | The workflows ask for more only where they need it. |
| **Actions → General → Actions permissions** | Allow actions made by GitHub, plus `Swatinem/rust-cache`, the one other action the workflows use. | An action outside that list can't run, even if a pull request adds one. |

**CODEOWNERS** isn't in the repository yet: your GitHub login couldn't be read when it was written. To add it, create `.github/CODEOWNERS` with the line `* @<your-login>`, and tick "Require review from Code Owners" in the ruleset.

## 3. Security

Repository **Settings → Advanced Security** (called "Code security" on some accounts):

| Turn on | Why |
|---|---|
| Private vulnerability reporting | [SECURITY.md](../SECURITY.md) tells people to use it. |
| Dependabot alerts, and Dependabot security updates | `.github/dependabot.yml` asks for weekly version updates; these add alerts and fixes for known holes. |
| Secret Protection (secret scanning), and push protection | A pushed token is caught, or refused before it lands. |

## 4. The community

| Where | Do | Why |
|---|---|---|
| **Settings → General → Features** | Turn on Discussions. | The issue form's "A question" link points there. |
| **Issues → Labels** | Make sure these exist: `bug`, `idea`, `good first issue`, `help wanted`. | The issue forms add `bug` and `idea`. GitHub ignores a label that doesn't exist. |
| **Issues** | Label a few small gaps from [gaps.md](gaps.md) `good first issue`. | Newcomers and agents start there. |

## 5. The timing gates, on a quiet machine

The timing gates measure real-time audio, so they need a quiet machine. GitHub's shared runners can't judge them, and the repository has no self-hosted runner (a pull request from a fork could reach it). So they run on a maintainer's own machine, every night:

| Step | Do |
|---|---|
| 1 | Pick a machine you trust that is quiet at night, with the test plugins (on Linux `ci/plugins.sh`; on the Mac in `~/Library/Audio/Plug-Ins/CLAP`). Clone the repository there. |
| 2 | Try it once by hand: `ci/nightly.sh`. It fetches `origin/main`, clones it fresh into `~/.cache/m2-nightly/clone`, runs `ci/check.sh --gates` there, and writes each night's logs and `summary.txt` under `~/.cache/m2-nightly/runs/`. `last.txt` is the newest summary. It never touches the checkout it lives in, your `~/Music` or your `~/.music2`. |
| 3 | To show the result on GitHub, log `gh` in with your own account (it needs the `repo` scope). Then `ci/nightly.sh --report` also posts the verdict as the commit status **nightly/gates** on that commit. |
| 4 | On Linux, add one line with `crontab -e` (every night at 02:17): `17 2 * * * cd /path/to/music2 && ci/nightly.sh --report >> "$HOME/.cache/m2-nightly/cron.log" 2>&1` |
| 5 | On the reference Mac, start it from a LaunchAgent in your login session instead of cron: the app's test and G12 need the session. |

A failed night shows as a red **nightly/gates** status on that commit. Its description is the check's first line; `summary.txt` has every row.

## 6. How many checks can run at once

GitHub runs a limited number of jobs at the same time for the whole account. Standard runners, per the [limits page](https://docs.github.com/en/actions/reference/limits):

| Plan | Jobs at once | of them on macOS |
|---|---|---|
| Free | 20 | 5 |
| Pro | 40 | 5 |
| Team | 60 | 5 |
| Enterprise | 500 | 50 |

One pull request uses up to 9 jobs: `changes`, `quick`, `clippy`, three `rust` parts, two `suites` parts, and `ci-ok`. A docs-only pull request uses 2: `changes`, which finds that it is docs only and checks the docs rule, and `ci-ok`. So on Free about 2 pull requests are checked at once, on Team about 6; the rest wait their turn.

When the limit binds, runs wait in a queue: nothing fails, it is only slower. What helps, cheapest first:

1. Every run of a pull request cancels its older runs (already set in the workflow).
2. Docs-only pull requests run one quick job (already set).
3. First-time contributors wait for your approval (part 2).
4. The merge queue tests pull requests in groups: raise its group size in the ruleset.
5. A bigger plan (Team, Enterprise), or ask GitHub Support to raise the limit.
