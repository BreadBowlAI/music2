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
| 8 | **Host the Airwindows test plugin.** Make a release whose tag is `test-plugins`, and upload the Airwindows zip from `~/.local/share/m2-test-plugins/dl/` as its file. `ci/plugins.sh` falls back to that release when upstream fails. | Upstream deleted the zip, and CI needs it: until the release is there, CI's `rust` and `suites` jobs fail at `ci/plugins.sh` whenever their cache is empty. CI reads the release with its own read-only token, so this works while the repository is still private. Anyone else can download it only once the repository is public (step 9). Airwindows is MIT, so it may be shared. (Surge XT's package is still upstream. It is GPL-3.0: sharing a copy would also mean offering its source, so leave it there.) |
| 9 | **Make it public:** **Settings → General → Danger Zone → Change visibility**. | The nightly tier (`nightly.yml`) starts only then: Actions minutes are free only for public repositories, and a private one's included minutes would go on the nightly alone. |

## 2. Pull requests and merging

Repository **Settings**:

| Where | Set | Why |
|---|---|---|
| **General → Pull Requests** | Allow merge commits. Untick squash merging and rebase merging. Tick "Automatically delete head branches". | The project merges; it never rebases. |
| **Rules → Rulesets → New branch ruleset**, for the default branch | Require a pull request before merging (0 approvals while you are the only maintainer: GitHub doesn't let you approve your own pull request; raise it when there are more). Require status checks to pass: **ci-ok** only. Require merge queue (merge method: merge commit; only merge pull requests that pass). Block force pushes. Restrict deletions. Do **not** require linear history. | `ci-ok` is the one check that sums up all the others. The queue tests each pull request on top of the newest `main` before it merges. |
| **Actions → General → Fork pull request workflows from outside collaborators** | Require approval for first-time contributors. | A newcomer's first run waits for your click, so spam can't use the shared runners. |
| **Actions → General → Workflow permissions** | Read repository contents and packages permissions. Don't allow GitHub Actions to create or approve pull requests. | The workflows ask for more only where they need it. |
| **Actions → General → Actions permissions** | Allow actions made by GitHub, plus `Swatinem/rust-cache`, the one other action the workflows use. Tick **Require actions to be pinned to a full-length commit SHA**. | An action outside that list can't run, even if a pull request adds one. A tag like `v7` can be moved by the action's maker; a full commit can't. Every action in the workflows is pinned that way already. |

**CODEOWNERS** isn't in the repository yet: whose GitHub login reviews couldn't be confirmed when it was written (the account the server pushes as is `BreadBowlAI`). The scripts that decide whether a pull request passes run from that pull request's own files, so a change to them needs your careful review. To make GitHub ask for it:

1. Create `.github/CODEOWNERS` with these lines, each with your login: `/ci/ @<login>`, `/.github/ @<login>`, `/deny.toml @<login>`, `/.config/ @<login>`, `/e2e/run.mjs @<login>`. These are the checks, the workflows, the licence rules, how the Rust tests run, and how the browser suites are judged.
2. Make a second branch ruleset for the default branch, with only **Require a pull request before merging** and, inside it, **Require review from Code Owners** (0 approvals). Add **Repository admin** to its bypass list, for pull requests only. A pull request that changes those files then needs your approval. Your own can still merge: GitHub doesn't let you approve your own pull request. Keep the first ruleset without a bypass, so `ci-ok` holds for everyone.

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

The timing gates measure real-time audio, so they need a quiet machine. GitHub's shared runners can't judge them, and the repository has no self-hosted runner (a pull request from a fork could reach it). So they run on a maintainer's own machine, every night, with `ci/nightly.sh`.

That script builds and runs whatever is on `main`, and every dependency's build script, with nobody watching. If merged code or a dependency ever turned bad, it could take whatever the user running it holds. So it runs as a user of its own, with one small token.

| Step | Do | Why |
|---|---|---|
| 1 | Pick a machine you trust that is quiet at night. Make an OS user just for this, with no other credentials: no `gh` login, no ssh keys, no cloud keys. On Linux: `sudo adduser --disabled-password m2-nightly`. On the Mac: a standard user that stays logged in (fast user switching). | A bad build could then take nothing of yours. The Mac needs a login session for the app's test and G12. |
| 2 | As that user: install [rustup](https://rustup.rs) and Node.js 24, clone the repository, and add the test plugins (on Linux `ci/plugins.sh`; on the Mac in `~/Library/Audio/Plug-Ins/CLAP`). | The check needs them, as on any machine. |
| 3 | Try it once by hand: `ci/nightly.sh`. It fetches `origin/main`, clones it fresh into `~/.cache/m2-nightly/clone`, runs `ci/check.sh --gates` there, and writes each night's logs and `summary.txt` under `~/.cache/m2-nightly/runs/`. `last.txt` is the newest summary. | It never touches the checkout it lives in, `~/Music` or `~/.music2`. Its summary warns if the user holds other credentials. |
| 4 | To show the result on GitHub, make a **fine-grained** personal access token: your GitHub **Settings → Developer settings → Personal access tokens → Fine-grained tokens**. Resource owner: the organization. Repository access: only music2. Permissions: **Commit statuses: Read and write**, nothing else. Save it as that user's `~/.cache/m2-nightly/token`, then `chmod 600` it. If the organization asks to approve such tokens, approve it there. | Then `ci/nightly.sh --report` posts the verdict as the commit status **nightly/gates**. That is all the token can do. The script refuses any other kind of token (one that doesn't start `github_pat_`). |
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

Each pull request that merges runs the tier three times: on the pull request, in the merge queue, and on the push to `main`. The push repeats the tree the queue just tested; it is there to save the build caches every pull request starts from. So one merged pull request costs about 27 jobs, plus one run for each new push to it. The rest wait their turn.

When the limit binds, runs wait in a queue: nothing fails, it is only slower. What helps, cheapest first:

1. Every run of a pull request cancels its older runs (already set in the workflow).
2. Docs-only pull requests run one quick job (already set).
3. First-time contributors wait for your approval (part 2).
4. The merge queue tests pull requests in groups: raise its group size in the ruleset.
5. Run only the parts that save caches on the push to `main` (a small change to `ci.yml`, listed in [gaps.md](gaps.md)).
6. A bigger plan (Team, Enterprise), or ask GitHub Support to raise the limit.
