# Packs: instruments, effects, sounds and samples anyone can publish

| | |
|---|---|
| **Status** | Draft: open for comments. Nothing here is built. |
| **Written** | 2026-10-29, from `main` at `e38c131` |
| **Gaps** | #39 (shared sound packages), with #22 (the rest of the Sampler) and #18 (more audio file types) |
| **Decides** | the owner, after comments ([§17](#17-how-this-proposal-goes)) |

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
