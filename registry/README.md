# The music2 pack registry

**In short:** the list of packs anyone can get in [music2](https://github.com/BreadBowlAI/music2): instruments, effects, sounds and samples. One small file a pack, in `packs/`. To list your pack, open a pull request that adds its file. A robot checks it, plays it, and copies its files into music2's storage, where the app finds them.

This folder is a template, kept in music2's own repo at `registry/`. The owner copies it into its own repo, `BreadBowlAI/music2-registry`, once the storage is set up (see [Setting it up](#setting-it-up)).

| Word | Meaning |
|---|---|
| **pack** | a folder of instruments, effects, sounds (presets) and samples, with an `m2pkg.toml` that names it |
| **version** | one release of a pack, such as `1.2.0`. Once listed, it never changes |
| **hash** | a fingerprint of a version's files (its `m2pack.json`'s SHA-256). A song keeps it in `music2.lock`, so it always gets exactly those files |
| **bucket** | the storage that holds every file, named by its hash, for the app to download |
| **robot** | `m2 registry`, run by this repo's CI on every pull request and after every merge |
| **yank** | hide a version from search. A song that already uses it still gets it |
| **take down** | remove a version's files for good, for a licence or legal problem |

## What is here

| Path | What |
|---|---|
| `packs/@owner/name.toml` | one file a pack: its name, where its code is, and each version with its hash and where to get it |
| `tags.txt` | the words a pack's tags may use |
| `removed.toml` | the versions taken down, which only BreadBowlAI lists (there once one is) |
| `.github/workflows/pull-request.yml` | the robot's advice on a pull request |
| `.github/workflows/main.yml` | the robot after a merge: it checks against what is published, then fills the bucket |
| `.github/CODEOWNERS` | who must review what: BreadBowlAI for everything outside `packs/` |

A pack file:

```toml
name = "@alice/felt-piano"
repository = "https://github.com/alice/felt-piano"

[[versions]]
version = "1.2.0"
hash = "sha256:3b1f…"
source = "https://github.com/alice/felt-piano/releases/download/v1.2.0/felt-piano-1.2.0.tar.gz"
# yanked = "why"     hidden from search
```

## How to list your pack

You need `m2` from a music2 release that has `m2 registry` (the first is the one after 0.1.0), and a GitHub account. Your pack's name starts with your GitHub name: `@alice/…` for the account `alice`.

| Step | What you do |
|---|---|
| 1. Make your pack | `m2 pack new felt-piano --name @alice/felt-piano`, then your instruments, sounds and samples ([Making a pack](https://github.com/BreadBowlAI/music2/blob/main/docs/guide/making-a-pack.md)) |
| 2. Check it | `m2 pack check felt-piano`: every problem in words |
| 3. Say where its code is | in `m2pkg.toml`, under `[package]`: `repository = "https://github.com/alice/felt-piano"` |
| 4. List it | fork this repo and clone your fork, then `m2 registry add music2-registry felt-piano --source https://github.com/alice/felt-piano/releases/download/v1.2.0/felt-piano-1.2.0.tar.gz` (the address its `.tar.gz` will have). It checks the pack again, writes its `m2pack.json`, and writes your pack's file in `packs/` |
| 5. Pack it up | from the folder that holds it: `tar -czf felt-piano-1.2.0.tar.gz --exclude='.*' felt-piano` (the whole folder, recordings too). Change nothing in it after step 4 |
| 6. Put it on GitHub | make a release of your repo, `v1.2.0`, and attach the `.tar.gz` to it |
| 7. Ask | open a pull request to this repo with your pack's file |

The robot answers on the pull request, with advice. A person from BreadBowlAI looks at every pull request (the name, the licence, what is in it) and merges it. Nothing merges by itself: letting a later version of a pack merge once the robot passes needs a check that a pull request can't change, which is later work.

A new version is a new `[[versions]]` table: run `m2 registry add` again after raising `version` in `m2pkg.toml`. A version, once listed, never changes: to change anything, list a new version.

## The rules

| # | Rule | Checked by |
|---|---|---|
| D2 | A pack is named `@owner/name`: `owner` is the GitHub account that publishes it, and its repository and its `.tar.gz` are under `https://github.com/<owner>/`, written plainly (letters, digits, `-._/`). `@m2/` is music2's own, from BreadBowlAI's repos. A pull request may change only its author's own pack files, byte for byte | the robot (`m2 registry check`) |
| D6 | A licence that lets anyone share the pack and use it in a song they sell: for sounds `CC0-1.0`, `CC-BY-3.0` or `CC-BY-4.0`; for code `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause` or `ISC`. Nothing "non-commercial" | the robot |
| D9 | A version is 10 GB at most | the robot |
| P1 | Every instrument and effect says what it is by its shape (notes in or sound in, sound out), and has a label and a category | the robot (`m2 pack check`) |
| P2 | Every file is in its `m2pack.json`, with its size and hash, and nothing else is in the pack. The `.tar.gz` must be exactly the files the hash names | the robot |
| P3 | Only these files: `.ts`, `.wasm`, `.toml`, `.json`, `.md`, `.txt`, `.wav`, and `LICENSE`. No links, no programs | the robot (`m2 pack check`) |
| P4 | Kernels keep music2's real-time rules | loading them, sealed |
| P5 | A version is never changed or reused, and its hash is no other version's. Yank it, but never edit it | the robot (`m2 registry check`, and after a merge against what the bucket publishes) |
| P6 | A licence, as D6 says | the robot |
| P7 | Tags come from `tags.txt`. What can be measured (a sample's length, tempo, key) is measured, never typed | the robot |
| — | Every word a pack gives (labels, descriptions, tags, authors, file names) is on one line, with no control character | the robot (`m2 pack check`) |

To add a word to `tags.txt`, ask in an issue: music2's people add words to it.

## What the robot does

| When | It |
|---|---|
| A pull request | checks every pack file (no listed version changed; your own packs only; only files under `packs/` if you aren't of BreadBowlAI, and only `.github/` for Dependabot), fetches each new version's `.tar.gz`, checks its files are the ones its hash names, checks the pack as `m2 pack check` does, plays a short preview of each instrument, effect and sound, and says what it found in the job's summary. This is advice for the person who merges: a pull request can change its own workflow, so its tick proves nothing alone. It has no key to the bucket |
| After a merge | checks the registry against what the bucket publishes already: a published version changed, a take-down taken back, or a hash another version has, stops the whole build, whoever made the change. Then it builds each new version on its own: one whose `.tar.gz` is gone, too big, or not the files listed is left out and tried again next time, and the rest are published. A last job fails when one was left out, so it is seen. Then it copies the new files into the bucket, and a new catalog (what the app searches), and deletes the files of versions taken down |

Every file in the bucket is named by its hash, and the app checks every hash, so the bucket can move host, or be copied, without any song noticing.

What runs where:

| Job | Runs | Can | Has the bucket's key |
|---|---|---|---|
| Build (after a merge) | packs' code, in music2's sandbox (TypeScript in QuickJS, kernels sealed in wasm) | whatever a GitHub runner can, should code get out of the sandbox: it has `sudo` and the network | no |
| Copy into the bucket | no pack code and no `m2`: shell and `aws` | write the bucket | yes |

So the copy job trusts nothing the build made. It takes only files at the bucket's four kinds of path (`blobs/sha256/<hash>`, `versions/<hash>.json`, `catalog/<hash>.json`, `catalog.json`), checks each blob and catalog against the hash it is named by, never replaces a file the bucket has (only `catalog.json` changes), and deletes only blobs. The worst a broken build could do is publish a wrong catalog, which a person sees and puts right.

## Yank and take down

| | Who | How | What happens |
|---|---|---|---|
| Yank | the pack's author | add `yanked = "why"` to the version in its pack file, in a pull request | hidden from search; a song that uses it still gets it. Taking `yanked` out again offers it again |
| Take down | BreadBowlAI only, for a licence or legal reason | add the version to `removed.toml`: `[[removed]]` with its `name`, `version`, `hash` (as its pack file lists it) and `why`. Its pack file stays as it is | its files are deleted from the bucket, and a song that used it says so in words. Samples already in songs stay: they are the song's files. A take-down is never taken back: its files are gone |

After a take-down, the owner purges the deleted files from Cloudflare's cache too (in Cloudflare's dashboard, Caching, Purge by URL, each `https://packs.<our domain>/blobs/sha256/<hash>` the job says it deleted). The bucket's public address serves every file with a year's cache, so without a purge a deleted file can still be downloaded from Cloudflare's edge.

## Setting it up

For the owner, once: the storage (decision D1: Cloudflare R2) and its web address (decision D10).

1. Make the repo `BreadBowlAI/music2-registry` from this folder.
2. Make an R2 bucket. Give it a public address of our own for the app (`packs.<our domain>`), and turn on its own `r2.dev` address for the robot: `--from` must read the bucket itself, never a cache in front of it, or it could take a deleted file for one still there.
3. In the repo's settings, make an environment `bucket` that only `main` may use, with three secrets: `R2_ENDPOINT` (`https://<account>.r2.cloudflarestorage.com`), `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY` (a key that may read and write that one bucket).
4. In both workflows, set `M2_VERSION` and `M2_SHA256` to the first music2 release with `m2 registry` and the SHA-256 of its `m2-<version>-linux-x86_64.tar.gz` (from its `SHA256SUMS`), `BUCKET_URL` to the bucket's `r2.dev` address, and in `main.yml` `R2_BUCKET` to its name.
5. Protect `main` (Settings, Branches, a rule for `main`):

   | Setting | Why |
   |---|---|
   | Require a pull request before merging, with 1 approval | a person from BreadBowlAI reads every change |
   | Require review from Code Owners | a change outside `packs/` (the workflows, `tags.txt`, `removed.toml`) needs BreadBowlAI's own review: `.github/CODEOWNERS` |
   | Dismiss stale approvals when new commits are pushed | what was approved is what merges |
   | Do not allow bypassing the above settings, and no direct pushes | every change comes through a pull request |

   Leave "Allow auto-merge" off in the repo's general settings. Don't make the robot's check required: it is advice, which a pull request can change.

The robot reads the bucket over https (`--from`) with music2's download module, from the first music2 release that has it.

The rules for the robot and the bucket's files are music2's contract, [REG-1 to REG-10](https://github.com/BreadBowlAI/music2/blob/main/docs/architecture.md#registry), and the design behind them is [the packs design](https://github.com/BreadBowlAI/music2/blob/main/docs/designs/packs/design.md).
