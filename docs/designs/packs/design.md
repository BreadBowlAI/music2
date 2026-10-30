# Packs: instruments, effects, sounds and samples anyone can publish

| | |
|---|---|
| **Status** | Accepted by the owner on 2026-10-30 ("just implement it"), with the recommendations in §15. Built the same day (#33 to #38): §16.2's slices 1 to 4 and 7, with the contract's PACK, LIB and PKG-15/16 rules. FLAC is read since track flac (the `claxon` crate, [FMT-24](../../architecture.md#assets-and-hashes)), and downloads since track fetch (NET-1 to NET-6). Not built: slice 5 (streaming). Slice 6 (the registry): the owner chose Cloudflare R2 for the bytes and a GitHub repo for the list (D1, 2026-10-30); its robot side, the list's format, the bucket's files and `m2 registry`, is built by track `registry` (§19, the contract's REG rules); the app's side (the catalog, ⌘F's From the library, Get, `m2 pack get`) by track `library-get` (§20, REG-11 to REG-17); the bucket's address (D10) waits for the owner. |
| **Written** | 2026-10-29, from `main` at `e38c131` |
| **Gaps** | #39 (shared sound packages), with #22 (the rest of the Sampler) and #18 (more audio file types) |
| **Decided** | by the owner, 2026-10-30 |

**In short**
- **One interface for every sound.** To music2, an instrument is "notes in, sound out" and an effect is
  "sound in, sound out". What is inside is the pack's business: code, recordings, or both. So a synth, a
  sampled piano, and a sampled piano with modelled strings are the same kind of thing.
- **A song sounds the same forever because every file is named by its fingerprint (hash), not by where it
  is kept.** Any host that hands back the right bytes will do. Where files are kept matters for keeping
  them forever, not for exactness.
- **Three kinds of data, three homes.** The list of packs is a GitHub repo (people review it there). The
  bytes (recordings, code, previews) sit in a storage bucket, named by hash, copied there by a robot.
  The search catalog is built from the two by that robot. No server to run; storage is the only bill
  (about $15 a month a terabyte; downloads free).
- **The registry repo decides what search shows.** A pack is listed when its pull request is merged.
  Everything else is made from that.
- **Search finds items inside packs** (one sample, one piano, one sound), plays a preview the robot made,
  and gets only what that item needs, never the whole pack.
- **A pack can't hurt your computer.** No programs in it, only code that runs in music2's sandbox, data
  and audio.

## 1. The problem

The app should ship small, and most sounds should come from other people. Today:
- std is the only package. A song lists packages in `music2.toml`, and is told when one is missing, but
  nothing can fetch one (#39).
- A recording-based instrument is std's Sampler with files in the song's own `assets/`. It has zones, but
  no velocity layers, round robin, loops, pedal or release sounds (#22), and it holds every file in memory.
- Search covers what the app ships: std's instruments and effects, and installed plugins.

## 2. Goals and non-goals

| Goals | Non-goals (for this proposal) |
|---|---|
| Any kind of instrument or effect: code, recordings, or both | Paid packs, a store, accounts in the app |
| Anyone can publish from GitHub, with no special access | CLAP plugins in packs (plugins are programs: they come from their makers) |
| A song sounds the same on any computer, years later | Ratings and comments (GitHub issues serve) |
| Search over every item in every listed pack, with previews, before any download | AI search ([goals](../../goals.md#non-goals); see [§9.4](#94-later-search-by-sound)) |
| Cheap to run for an open source project | Packs that need other packs (later, its own design) |
| Safe to get a stranger's pack | |

## 3. First principles

Six facts the design follows from. Each later section points back to one.

| # | Fact | So |
|---|---|---|
| F1 | A song must sound the same forever. | Name every byte by its hash, and pin the hashes in the song. Exactness comes from hashes, never from a host. |
| F2 | music2 shouldn't care how a sound is made. | One interface by shape (notes or sound in, sound out); what is inside is free. |
| F3 | A stranger's pack must be safe, and exact on every computer. | Only sandboxed code (TypeScript in QuickJS, wasm kernels), data and audio. No native programs. |
| F4 | A 3 GB piano shouldn't be downloaded to be judged. | Previews, and getting one item with only the files it uses. |
| F5 | Someone must decide what is listed, in the open. | One reviewable list, in git. Everything else is built from it, never typed by hand. |
| F6 | It must be cheap to run forever. | Static files and a robot. No app server; pay only for storage. |

## 4. The model: items, engines, assets

This is the abstraction layer: what you search for, what makes the sound, and the bytes it reads.

```
  ITEMS: what you search and add        ENGINES: what makes sound        ASSETS: bytes
  ┌────────────────────────────┐        ┌───────────────────────┐        ┌──────────────┐
  │ Instrument  notes → sound  │──uses─▶│ code: a TypeScript    │─reads─▶│ recordings,  │
  │ Effect      sound → sound  │        │ component, and wasm   │        │ named by     │
  │ Sound       an item, set   │        │ kernels if it needs   │        │ hash         │
  │ Sample      one recording ─┼────────┼───────────────────────┼───────▶│              │
  └────────────────────────────┘        └───────────────────────┘        └──────────────┘
```

### 4.1 One interface

music2 already tells instruments and effects apart by shape, not by a label
(`core/script/sdk/core.ts`, in `component()`): a component whose first input is notes and whose output is
audio is an instrument; one whose first input is audio is an effect. CLAP plugins say so in their own
features. This proposal keeps that rule, and adds nothing to it. So every way of making sound fits:

| Instrument | Engine | Assets | To music2 |
|---|---|---|---|
| A synth | std's synth (code only) | none | notes → sound |
| A sampled piano | std's Sampler | 300 recordings | notes → sound |
| A piano with modelled strings | a pack's own kernel, and the Sampler | the hammer recordings | notes → sound |
| A granular texture | a pack's own kernel | one long recording | notes → sound |
| A drum kit | std's drum machine for the kick, the Sampler for the rest | the hits | notes → sound |

Later kinds follow the same rule: a **note effect** (notes → notes: an arpeggiator, chords) when kernels
get event ports ([ABI-2](../../architecture.md#kernel-abi-reservations)'s reserved shapes).

### 4.2 Two ways to write an instrument

| Way | For | What it is |
|---|---|---|
| **Code** | sound designers who write code | a TypeScript component, as std's are (`component(descriptor, body)`, PKG-1). Free to mix engines. |
| **Data** | musicians who record | a file that names an engine and its data, such as a sample map (below). music2 turns it into the same component, as if `sampler({ … })` were written in code. No code to write. |

Both end as the same thing to music2, so search, presets, the Device view and the lock don't know which.

### 4.3 The sample map

A recording-based instrument is a list of **regions**: which file plays for which notes, how hard, and
when. We don't invent this: SFZ, an open text format with thousands of free instruments, has had it for
20 years. The map uses SFZ's ideas, in TOML:

```toml
# instruments/felt-piano.toml: a data instrument
label = "Felt Piano"
category = "Keys"
engine = "std:sampler"

[[region]]
file = "samples/C4-soft-1.flac"
root = 60            # the note it was recorded at
keys = [58, 62]      # the notes it plays
velocity = [1, 60]   # how hard (1 to 127)
round_robin = 1      # take 1 of several, played in turn
release = "samples/C4-release.flac"   # the sound when the key is let go
```

`m2 pack import-sfz` (and later Decent Sampler's format) turns an existing free instrument into a pack.
The Salamander Grand Piano (SFZ, CC-BY 3.0, by Alexander Holm) is the first test.

### 4.4 What the engines need

| Need | Today | Where |
|---|---|---|
| Regions by note, velocity and round robin; release sounds; loop points; sustain pedal; choke groups | zones by note only | Sampler v2 (#22, grown) |
| A pack's own files as assets | assets are the song's `assets/` only | slice 1 |
| FLAC (half the size of WAV) | WAV only | slice 2 (#18) |
| Big instruments (1 to 10 GB) | every file decoded whole into memory | host streaming: keep the first moments of each file in memory, read the rest from disk. Kernel ABI v1 already allows it: live, a frame not loaded yet plays silence and is counted; an export never misses ([ABI-2](../../architecture.md#kernel-abi-reservations)) |

## 5. The contract

What each side promises. Each line becomes a numbered rule (`PACK-1` …) in the contract,
[architecture.md](../../architecture.md), in the pull request that builds it.

### 5.1 music2 promises a pack

| # | Promise |
|---|---|
| M1 | Your items run with the same API as std: the `@m2/core` version you name. |
| M2 | Your code runs the same on every computer: wasm with the features whose results differ by CPU turned off (relaxed SIMD; `core/runtime/src/packages.rs`), QuickJS with music2's own maths ([PLAT-4](../../architecture.md#platforms)). |
| M3 | Your files reach your code only through music2 (`asset_read`), by name in your pack; you never see a path. |
| M4 | Live, a recording not loaded yet plays silence and is counted. An export never misses one. |
| M5 | A song that uses your version keeps that version, until its owner moves it. |

### 5.2 A pack promises music2

| # | Promise | Checked by |
|---|---|---|
| P1 | Every item says what it is through its shape (§4.1) and has a label and a category. | loading it |
| P2 | Every file is listed in `m2pack.json` with its size and hash, and nothing else is in the pack. | the robot |
| P3 | Only these files: `.ts`, `.wasm` (kernel ABI v1), `.toml`, `.json`, `.md`, `.txt`, `.wav`, `.flac`. | the robot |
| P4 | Kernels keep the real-time rules ([ABI-5](../../architecture.md#kernel-abi-reservations)). | loading them sealed |
| P5 | A version is never changed or reused. | the registry |
| P6 | A licence that lets anyone share the pack and use it in a song they sell (D6). | the robot |
| P7 | Tags come from the registry's word list; what can be measured is measured, never typed. | the robot |

### 5.3 The registry promises the app

| # | Promise |
|---|---|
| R1 | The registry repo is the one list of what is listed. |
| R2 | Every byte is served by its hash, from a copy the registry keeps. It is never changed, and is removed only for a legal reason (§9.3). |
| R3 | Previews are made by the robot, from the pack itself, at one loudness for all. |

## 6. A pack

### 6.1 On disk

```
felt-piano/                      github.com/alice/felt-piano
  m2pkg.toml                     what the pack is
  README.md  LICENSE
  src/*.ts                       instruments and effects in code (optional)
  instruments/*.toml             instruments as data (optional)
  kernels/*.wasm  kernels-src/   new DSP (optional)
  presets/<item>/*.json          sounds (FMT-22)
  samples/**                     recordings: the instruments' and the loose samples
  samples.toml                   the loose samples' tags, made by `m2 pack init`, checked by hand
```

The recordings don't have to be in git: `m2 pack publish` uploads them (§8). The code and the small files
are in git, as for any project.

### 6.2 `m2pkg.toml`

```toml
[package]
name = "@alice/felt-piano"      # @owner/name: owner is the GitHub account that publishes it
version = "1.2.0"
license = "CC-BY-4.0"
description = "A soft felt piano, recorded close"
authors = ["Alice Example"]
repository = "https://github.com/alice/felt-piano"
tags = ["keys", "piano", "lofi"]

[dependencies]                  # only "@m2/core" and std, in version 1 of packs
"@m2/core" = "1"
std = "0.4"
```

A song imports it as it imports std: `import { feltPiano } from "@alice/felt-piano"`. Its ids carry its
name: `@alice/felt-piano:felt-piano` ([PKG-9](../../architecture.md#component-descriptors)).

### 6.3 What publishing makes

| File | Holds | Why |
|---|---|---|
| `m2pack.json` | every file: path, size, hash. Its own hash is the version's hash. | P2; a song's lock pins this one hash |
| `items.json` | every item: kind, label, category, tags, measured facts, and **the files it uses** | search, and getting one item alone (§10) |

Both are made by tools, never by hand. The registry's robot makes them again and refuses a pack whose
own copies differ.

## 7. Where things are kept

### 7.1 Three kinds of data

| Data | Size | Changes | Needs | Home |
|---|---|---|---|---|
| **The list** | a few lines a pack | at each publish | review, history, who did what | a GitHub repo, `BreadBowlAI/music2-registry` |
| **The bytes**: pack files, previews | KB to GB | never | cheap, permanent, one file at a time | a storage bucket, by hash: `blobs/sha256/<hash>` |
| **The catalog**: what search reads | a few MB | at each publish | fast to download | the same bucket, made by the robot |

### 7.2 Why not GitHub alone

GitHub stays where people work: the list, the packs' code, and each author's upload (§8). It isn't
enough for the bytes:
- **An author can delete a repo**, and every song that used the pack goes quiet. crates.io, npm and Go keep
  their own copy for this reason (R2 above).
- **Previews and single samples need one file at a time.** A release holds at most 1,000 files, each under
  2 GiB; a sampled piano has more files than that.
- **Git LFS** (GitHub's large files) charges by the gigabyte downloaded.

### 7.3 The bucket

Recommended: **Cloudflare R2** behind a name of our own (`packs.<our domain>`). Downloads cost nothing,
and storage $0.015 a GB a month, with 10 GB free:

| Stored | A month |
|---|---|
| 10 GB | free |
| 100 GB | about $1.50 |
| 1 TB | about $15 |

The design doesn't depend on R2. A blob's address is only its hash, and the app checks every hash, so any
bucket, a second copy, or a folder on a drive serves the same files. Moving host changes one URL.

## 8. Publishing

```
 author                 author's GitHub             registry repo                   bucket
 ──────                 ───────────────             ─────────────                   ──────
 m2 pack check
 m2 pack publish ─────▶ a release with the pack
                 ─────────────────────────────────▶ a pull request: the pack's file
                                                    robot, no secrets: fetch, check
                                                    hashes, load every item, render
                                                    previews, measure tags; report
                                                    on the pull request
                                                    merge (a person, the first time)
                                                    robot, on main, with the bucket's
                                                    key: copy each new blob ─────────▶ blobs/…
                                                    rebuild the catalog ─────────────▶ catalog/…
```

- **Identity is GitHub.** `@alice` may publish only from repos `alice` owns; the robot checks with
  GitHub's API. `@m2/` is ours.
- **The first version is looked at by a person**: the name, the licence, the content. Later versions
  from the same repo merge themselves when the robot passes, as Homebrew and Flathub do.
- **Secrets stay on `main`.** Checks on a pull request run with no key to the bucket; only the job after a
  merge can write. A pull request can't reach the key (GitHub's "pwn request" trap).
- **Once copied, the bucket's copy is the truth.** The author's release can go; songs don't notice.
- Agents can publish too: every step is an `m2` command.

## 9. What search shows

### 9.1 The source of truth

| Question | Answered by |
|---|---|
| Is a pack listed? | its file in the registry repo, merged |
| Which versions? | that file: each version and its hash |
| Is it ours or the community's? | its scope: `@m2/` is ours, with a music2 badge |
| What is in it? | its `items.json`, remade by the robot |
| Hidden? | `yanked` or `removed` in its file, with why |

The catalog is made from these by the robot. No one edits it, and git's history says who listed or
removed what, and when.

### 9.2 Search in the app

- **⌘F** anywhere (Live's key; free in [taste.md](../step8/taste.md)'s table). The side browser's field is
  the same search.
- It searches **items**, not packs: "felt piano" finds the instrument, "vocal chop Am" finds samples inside
  packs. Groups: Instruments, Sounds, Effects, Samples, Packs. Filters you can type or click: `loop`,
  `one-shot`, `kick`, `90 bpm`, `Am`; and **Fits my song** (the song's tempo and key).
- **What you have first**, then **From the library**, each with **Get** and its badge.
- **Space** plays a preview. **Enter** or a drag adds it.
- **It runs on your computer**, over the catalog the app downloads (a few MB, fresh once a day): instant,
  offline for what you have, and what you type is never sent. Ranked by name, then tags, then
  description; ours and what you have first; then downloads.

### 9.3 Yank and take-down

- **Yank**: hidden from search; a song that locks the version still gets it.
- **Take-down** (a licence or legal problem): removed from the bucket. A song that used it says so in
  words. Samples already in songs stay: they are the song's files.

### 9.4 Later: search by sound

Search by "sounds like this" works with numbers the robot computes for each preview (embeddings), and
the app compares them. The catalog's format has room for them. They come from a trained model, so they
need the owner to change the non-goal "no AI in the app" first, or to decide that a number made on the
registry is not AI in the app. Not in this proposal.

## 10. Previews, and getting only what you need

| Item | Preview (made by the robot) | Get fetches |
|---|---|---|
| Sample | its first seconds, small and compressed | that one file |
| Instrument | a short phrase for its category (chords for Keys, a line for Bass, a beat for Drums), played by the pack's own code | its code and the files it uses (`items.json`) |
| Sound | the same phrase, with its settings | its instrument, as above |
| Effect | a fixed loop, without it then with it | its code |

The robot renders previews with `m2 render`, so a preview can't oversell: it is the pack playing. Later:
**Try** an instrument from your keyboard before getting it, its recordings streamed (§4.4, M4).

## 11. In a song

- **The store.** What you get goes to `~/Music/music2/Packs/` ([PLAT-7](../../architecture.md#platforms)),
  read-only, by hash, so two versions share the files they share.
- **The lock.** Adding an item writes the pack into `[dependencies]` and its exact version and hash into
  `music2.lock`: one Undo step ([Deferred](../../architecture.md#deferred-with-triggers): this is the
  trigger it waits for).
- **Samples are copied into the song**, into `assets/` ([FMT-14](../../architecture.md#assets-and-hashes)),
  with a note beside it: the pack, version, author and licence. `m2 credits` (later) lists them.
- **A pack you don't have**: the song still opens; its tracks that use it play silent, as a missing
  plugin does ([FMT-18a](../../architecture.md#plugin-state-and-parameters)), with **Get**.
- **Updates are never automatic.** A song moves to a new version when you choose.
- **Collect and save** (later, as Live's): a copy of the song with every file it uses from packs, to play
  with no internet. The licences (D6) allow it.

## 12. Safety

| A pack holds | Why it is safe |
|---|---|
| TypeScript | runs in QuickJS: no files, no network, no clock |
| Kernels | wasm in a sealed sandbox, one import ([ABI-3](../../architecture.md#kernel-abi-reservations)) |
| Data | read strictly |
| Audio | decoded by music2, size-checked |
| A download | each file's hash checked before use; paths checked |

No native programs, ever (D3). A pack may hold sounds for a CLAP plugin; search then says which plugin
it needs.

## 13. Alternatives

| Alternative | Why not, for now |
|---|---|
| Our own upload server, as crates.io has | accounts, logins and a server to keep up; GitHub gives identity and review for free |
| GitHub alone (releases, LFS) | §7.2: packs vanish with their repos, previews need single files, LFS costs |
| Peer to peer (IPFS, BitTorrent) | slow to start, and a file is there only while someone shares it |
| Hugging Face's free storage | someone else's rules, made for AI models |
| Plugins (native code) in packs | not safe to get from strangers (F3) |
| A list only of ours, no community | the goal is that most sounds come from others |

## 14. Risks

| Risk | What we do |
|---|---|
| Someone uploads sounds they don't own | a person looks at each first version; take-down (§9.3); a licence on every pack |
| Storage grows fast | 10 GB a pack at first; the cost is in the open |
| Name squatting (`@m2-official`) | GitHub scopes; the registry's rules |
| A big piano is too heavy for a laptop | streaming (§4.4) comes before big instruments are listed |
| The robot's CI time | GitHub's runners are free for public repos; a pack over the runner's disk waits for a bigger runner |

## 15. Decisions for the owner

| # | Decision | Recommended |
|---|---|---|
| D1 | Where the bytes are kept | R2 (or any bucket with free downloads), behind our own name; the list on GitHub |
| D2 | Names | `@owner/name`, owner = a GitHub account; ours `@m2/`; std stays `std` |
| D3 | Programs in packs | never |
| D4 | Packs that need packs | not in version 1 |
| D5 | Packs from any GitHub repo, unlisted | later, with a plain warning |
| D6 | Licences | for sounds CC0, CC-BY 3.0 and 4.0; for code MIT, Apache-2.0, BSD, ISC. Nothing "non-commercial", so a beginner never gets caught out |
| D7 | The search key | ⌘F |
| D8 | What the app ships | std, std's presets, one small drum kit |
| D9 | The largest pack | 10 GB, and the first big ones only after streaming |
| D10 | A domain for the bucket | the owner's choice |

## 16. Plan

### 16.1 Spikes first

Small throwaway builds that answer what reading can't. Each ends in a note here.

| Spike | Answers |
|---|---|
| S1: the Salamander piano as a data instrument | is the sample map right? how much memory, and how much must streaming hold? |
| S2: a toy registry on a fork, end to end | publish, robot, bucket, catalog, search with preview: does each step hold? |
| S3: a catalog of 100,000 made-up items | its size, and search speed on a slow laptop |

### 16.2 Slices

Each lands green on its own, with its contract rules first.

| # | Slice | You get |
|---|---|---|
| 1 | Packs from a folder | the format, a pack's own files as assets, the store, `music2.lock`, scoped names |
| 2 | Samples | `samples.toml`, measured tags, the Samples group, preview, drag in with credits; FLAC; Add a folder of your own |
| 3 | ⌘F | one search over what you have |
| 4 | Sampler v2 and data instruments | regions, velocity, round robin, release, loops, pedal; `m2 pack import-sfz` |
| 5 | Streaming | big instruments in little memory |
| 6 | The registry | the repo, the robot, the bucket, the catalog; Get; previews; `m2 pack new/check/publish` |
| 7 | Our first packs | `@m2/salamander-grand` (with credit), `@m2/synth-sounds`, a drum pack |
| 8 | Open to everyone | the pack author's guide, the registry's rules, a call for packs |

## 17. How this proposal goes

As serious open source projects run a big change (Rust's RFCs, Python's PEPs), fitted to
[CONTRIBUTING.md](../../../CONTRIBUTING.md): a design note first, and the owner's yes.

| Stage | What happens | Where | Done when |
|---|---|---|---|
| 1. Idea | the problem, in a few lines | an Idea issue, linked to #39 | people can find it |
| 2. Draft | this note, open for comments | a draft pull request; talk in a Discussion | comments slow down |
| 3. Spikes | S1 to S3 (§16.1) | branches; results added here | each question answered |
| 4. Last call | the owner fills §15 and says "last call: 7 days" | the pull request | 7 days with no new problem |
| 5. Accepted | the note merges with Status: Accepted | `main` | merged |
| 6. Build | one issue a slice, in a tracking issue; claim first; one small pull request each | issues | every slice merged |
| 7. Launch | the guide's pages, release notes, a call for packs | a release | the first community pack is listed |

A change of mind after stage 5 is a new pull request to this note, with why.

## 18. Where each idea comes from

| Idea | From |
|---|---|
| Packs; a browser over everything; Collect and save | Ableton Live |
| One sample at a time; key, bpm and Fits my song | Splice |
| A fixed word list; a preview for every sound | Native Instruments' NKS |
| Regions, velocity layers, round robin | SFZ |
| Versions never change; yank; a lockfile; scoped names | crates.io, npm |
| The registry keeps a copy of everything | Go's module proxy, crates.io |
| Files named by their hash | git, Nix, container registries |
| The list is a repo; a pull request to add | Homebrew, Flathub, Obsidian |
| An RFC, then a last call | Rust, Python |

## 19. The registry, as built

**In short:** slice 6's robot side, built on 2026-10-30 by track `registry`, and hardened after its review the same day. The list is a folder of small TOML files, one a pack. The robot is `m2 registry`, run by the registry repo's CI. The bucket is plain files named by their hashes. A person from BreadBowlAI merges every pull request; the robot's run on a pull request is advice, and the real gate is the build after a merge, which holds the registry to what the bucket has published. The rules are the contract's [REG-1 to REG-10](../../architecture.md#registry); this section says the same in plain words, and what changed from the plan above. The app's side (fetching the catalog, ⌘F's From the library, Get) is §20.

### 19.1 The list

The registry repo starts from music2's own `registry/` folder: a README with the rules, `tags.txt` (the words tags may use, P7), `packs/` (empty), `CODEOWNERS`, and the two workflows. One file a pack:

```toml
# packs/@alice/felt-piano.toml
name = "@alice/felt-piano"
repository = "https://github.com/alice/felt-piano"

[[versions]]
version = "1.2.0"
hash = "sha256:3b1f…"     # its m2pack.json's hash: what a song's music2.lock pins
source = "https://github.com/alice/felt-piano/releases/download/v1.2.0/felt-piano-1.2.0.tar.gz"
# yanked = "why"          hidden from search; a song that locks it still gets it
```

A take-down is not in a pack file but in `removed.toml`, which only BreadBowlAI changes:

```toml
# removed.toml
[[removed]]
name = "@alice/felt-piano"
version = "1.2.0"
hash = "sha256:3b1f…"
why = "the recordings aren't the author's to share"
```

| Rule | So |
|---|---|
| Read strictly: an unknown key anywhere is refused, and no link, pipe or device anywhere | a typo never means something |
| The repository and every source are under `https://github.com/<scope>/` (`BreadBowlAI` for `@m2/`), in letters, digits and `-._/` only (no `%`) | a pack comes from its owner's own GitHub, an encoded part can't name another owner's repo, and no address can put anything into the CI's shell |
| A hash is listed once in the whole registry | no pack can take another's version |
| A listed version's hash and source never change; only `yanked` is added or taken back | P5: a song always gets the files it was made with |
| A take-down is BreadBowlAI's, in `removed.toml`, and never taken back | an author can't break the songs that use their version; a legal take-down stays down |
| A pull request may change only its author's own files, byte for byte (a comment and a dot file count); `@m2/`, `tags.txt` and `removed.toml` only music2's people | identity is GitHub's, checked from the scope, offline, and nobody edits another's words |

### 19.2 The bucket

| Path | Holds |
|---|---|
| `blobs/sha256/<hash>` | every file of every version (its `m2pack.json` too) and every preview |
| `versions/<hash>.json` | the robot's facts for one version, by the version's hash: what it says of itself, its size, every blob it uses, and its items, each as the library would list it, with its preview's hash and, for a sample, its one file |
| `catalog/<hash>.json` | a catalog, by its own hash: every pack, its versions (each with its hash and source), its latest's items. A version not built yet, or whose build failed, isn't in it |
| `catalog.json` | which catalog is current: the one file that changes |

Every JSON file is written in one form (RFC 8785, the form music2 already hashes JSON in), so the same inputs always give the same bytes and the same hash, and is read strictly: a newer format says to update music2, and an unknown field is refused. The readers are in `m2_song::registry`, for the app to use.

### 19.3 The robot

| Command | Who | Does |
|---|---|---|
| `m2 registry add <registry> <pack> --source <url>` | a pack's author | checks the pack (`m2 pack check`), writes its `m2pack.json`, and writes or extends its pack file |
| `m2 registry check <registry> [--base <before>] [--author <login>]` | the CI, on a pull request | reads every file strictly; with `--base`, no listed version changed and no take-down taken back; with `--author`, only their files changed |
| `m2 registry pending <registry> --out <bucket> [--from <bucket>]` | the CI | prints each version the bucket hasn't got yet, `hash name version source`, so the CI fetches only those, each into a folder named by its hash |
| `m2 registry build <registry> --sources <folder> --out <bucket> [--from <bucket>] [--made <time>] [--to-delete <file>]` | the CI | checks the registry against what the bucket publishes; then builds each pending version on its own: checks its source, copies its files in by hash, renders its previews, writes its facts; then the catalog and `catalog.json` |

What `build` refuses:

| Refused | How much |
|---|---|
| A published version changed, a take-down taken back, a hash a published version has | the whole build, whoever made the change: nothing is written |
| A source that isn't there, holds another version, or holds this one twice | that version alone |
| A link, a pipe or a device in it (found before any file is opened), or more than 10 GB by its files' sizes (before any byte is hashed, D9) | that version alone |
| Files that aren't the ones the hash names (checked next: nothing else is looked at then) | that version alone |
| What `m2 pack check` refuses (a word with a control character among it), a licence not in D6's list (as an SPDX expression: `CC0-1.0`, `CC-BY-3.0`, `CC-BY-4.0`, `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`), a tag not in `tags.txt` | that version alone |
| A version that takes longer than 10 minutes to check and build (a kernel that never returns: music2's engine sets kernels no time limit) | that version alone; its thread, which can't be stopped, ends with the robot |

A version that fails writes nothing into the bucket, is left out of the catalog, is tried again at the next build, and is reported (`m2` exits 1 once the catalog is written). The others are built all the same.

Previews are made by the app's own code: the robot writes the same song for an item that ⌘F writes (`library::preview::of`, now the one step both take) and renders it the same way, so its preview is the same bits the app makes from the installed pack, checked by a test. A sample's preview is its own file (LIB-6).

### 19.4 The registry repo's CI

| Workflow | Runs on | Has the bucket's key | Does |
|---|---|---|---|
| `pull-request.yml` | `pull_request` (never `pull_request_target`) | no; its token only reads | advice for the person who merges: checks the change's paths (someone outside BreadBowlAI only under `packs/`, Dependabot only under `.github/`), checks the pack files, fetches the new sources with `curl` and `tar`, builds them into a scratch folder, and puts what the robot said in the job's summary. A pull request can change this file, so its tick proves nothing alone |
| `main.yml`, job Build | a push to `main` | no | the gate: the same build, held to what the bucket publishes, into a folder kept as the job's artifact, with the list of files to delete |
| `main.yml`, job Copy into the bucket | after Build | yes, from the `bucket` environment | checks what Build made as a stranger's (the four kinds of path, each blob and catalog against its name, deletes only of blobs), then puts each file the bucket hasn't got (never replacing one), `catalog.json` last, then deletes what the list says. It runs no pack code and no `m2` |
| `main.yml`, job Every version built | after both | no | fails when a version was left out, so it is seen |

Every pull request is merged by a person: the owner protects `main` (a required review, code owners' review, no bypass, no auto-merge), and `CODEOWNERS` gives BreadBowlAI everything outside `packs/`, `removed.toml` named too. The build job runs strangers' code on a GitHub runner, which has `sudo` and the network; it has no key, and the copy job trusts nothing it made. m2's output is fenced with `::stop-commands::`, so a pack's words can't start a workflow command. The `m2` they run is a music2 release, pinned by version and SHA-256. music2's own lint checks these workflows with actionlint and zizmor too.

### 19.5 What changed from the plan

| The plan said | As built | Why |
|---|---|---|
| A pack carries `items.json` (§6.3) | the robot writes each version's items into the bucket, `versions/<hash>.json` | the robot makes them again anyway, and an author can't get them wrong |
| Get fetches an instrument's code and the files it uses (§10) | an instrument, an effect or a sound comes with its whole version; a sample alone, its one file | which files code reads can't be known before it runs |
| A sample's preview is its first seconds, compressed (§10) | its own file, as the app already previews it (LIB-6) | one way, and it can't oversell; none over 64 MB |
| The robot checks identity with GitHub's API (§8) | from the scope and the addresses alone | the same answer every time, offline, with no token |
| The robot reports on the pull request (§8) | in the job's summary and its tick or cross, as advice | a pull request from a fork gets a token that can't comment, the usual way round it (`workflow_run`) is the trap §8 warns of, and a pull request can change its own check |
| Later versions merge themselves when the robot passes (§8) | a person from BreadBowlAI merges every pull request | the robot's run on a pull request is the pull request's own workflow; merging by itself needs a gate it can't change, which is later work |
| A take-down is a line in the pack's file (§9.3) | a version listed in `removed.toml`, BreadBowlAI's, never taken back | an author must not be able to break the songs that use their version, or undo a legal take-down |
| — | the catalog's time is given (`--made`): the CI gives the registry commit's | one commit always makes the same bytes |
| — | a removed version's facts stay, and only its blobs no other version uses are deleted | what a pack said of itself stays; a file two versions share isn't lost |
| — | each version is built on its own, from its own folder, with a time limit; one that fails stays pending | one author's missing release or hung kernel never stops everyone's publishing |

`--from` reads the bucket over https through music2's download module (track H's `m2_editor::fetch`), from its `r2.dev` address, never the cached one; so the workflows run with the first music2 release that has `m2 registry`. A take-down needs the deleted files purged from Cloudflare's cache by hand (the README says how). A data instrument's own tags reach no catalog yet (gaps #49). The app's ⌘F with no song open, and its preview route, read instruments written as data since track `library-get`, as the robot does (it was gaps #47). A kernel that never returns has no bound in the app's own previews and plays (gaps #39).

## 20. The registry in the app, as built

**In short:** built on 2026-10-30 by track `library-get`. The app reads one registry (`M2_REGISTRY`, else the settings' `registry`; no default until D10 picks the bucket's address), keeps its catalog in the cache folder, and searches it on this computer: ⌘F shows **From the library** after what you have, Space plays a preview fetched by its hash, and **Get** gets a pack's whole version, or one sample, checked by hash, all or nothing. A song missing a locked pack is offered Get, and `m2 pack get` does the same from the command line. The rules are the contract's [REG-11 to REG-17](../../architecture.md#registry); the guide says it in plain words ([Getting packs from the library](../../guide/library.md)).

| The plan said | As built | Why |
|---|---|---|
| The catalog, fresh once a day (§9.2) | fetched as the app starts, when a day old, and ten minutes after a fetch that failed, in the background; a search never waits; `m2` with the app closed fetches only a catalog a day old | offline must just work, and a search must never wait on the network |
| — | the catalog is kept by a short hash of the registry's address, `catalog.json` written after its catalog, and a catalog that isn't the one `catalog.json` names is never used nor kept | a broken or changed catalog must never replace a good one |
| Ranked by name, tags, description; ours and what you have first; then downloads (§9.2) | From the library is ranked by LIB-1's one search, ours (`@m2/`) first, in a group after what you have; no downloads | the catalog counts no downloads yet; one search keeps both groups ranked alike |
| Get fetches what an item needs (§10) | an instrument, an effect or a sound gets its whole version, into a folder beside the store, then installed as `m2 pack install` does; a sample its one file | §19.5; and a pack's own check runs on this computer before anything is in the store |
| — | `m2pkg.toml` comes first: a version that needs a newer music2 stops there | nothing big is fetched for a pack this app can't run |
| A sound adds its preset (§9.2) | a sound of an instrument adds a track of its instrument, then loads the sound on it: two undo steps; an effect's sound loads as one you have does, onto the selected track's effect | a track that plays a sound in one step is gaps #47 |
| — | one Get at a time; progress in the row and the foot, `get` events for every window; Esc or Cancel stops it while its files come; once it is installing it can't be, and says so | a beginner must see it come, and be able to stop it; a cancel the install didn't heed would be a lie |
| — | From the library offers nothing of a pack the open song uses, and at most 20 rows, saying how many more; it is made once a catalog | moving a song to another version is `m2 pack update` (§11); a catalog of 100,000 items must not cost a keystroke a second |
| — | a catalog older than the one kept is refused | a cache or a host serving an old catalog would undo a yank or a take-down |
| — | a Get that ends after another song was opened adds nothing, and says so | an instrument must never land in a song it wasn't got for |

