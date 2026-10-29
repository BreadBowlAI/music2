# Packs: the library, the registry and search

Written 2026-10-29, from `main` at `e38c131`. A plan, waiting for the owner's yes. Nothing here is built.
It is gap #39 (shared sound packages), grown into the whole library.

**In short**
- The app ships small: std (its instruments and effects, about 220 KB of code), std's presets, and one
  small drum kit, so a new song makes sound with no internet.
- Everything else comes as **packs**: a pack is one folder of instruments, effects, sounds (presets) and
  samples, with one file that says what is in it. Anyone can make one. It is a GitHub repo.
- The **library** is everything you can search: what came with the app, the packs you have, and the packs
  you could get.
- The **registry** is one public GitHub repo with a small file per pack: who made it, where it is, each
  version and its fingerprint (a hash). music2's own packs and the community's are in the same list, with
  a badge that says which.
- **⌘F** opens one search over all of it, like Spotlight: instruments, sounds, effects and samples. Space
  plays a preview. Enter or a drag adds it. A sample can be got on its own, without its whole pack.
- A sample you add is copied into your song, with a note of where it came from and its licence. A song
  pins the exact version of each pack it uses, so it sounds the same on every computer, for years.
- A pack can't hurt your computer: no programs in it, only sandboxed code (the same sandbox std runs in),
  presets and audio.

| Part | Like | What it is here |
|---|---|---|
| Pack | an Ableton Pack, a Splice pack | a folder with `m2pkg.toml`; one download |
| Library | Ableton's browser | everything search can find |
| Registry | crates.io, Homebrew's list, Obsidian's plugin list | one GitHub repo of small files; its robot checks every pack |
| Search | Spotlight, Splice's search | ⌘F; runs on your computer, works offline for what you have |
| Lock | `Cargo.lock`, `package-lock.json` | `music2.lock` in each song: the exact pack versions it uses |

## 1. Words

| Word | Meaning |
|---|---|
| **Pack** | One download: a folder with `m2pkg.toml` and the things it lists. Named `@owner/name`, as `@alice/felt-piano`. std is a pack too. |
| **Item** | One thing in a pack that search shows: an instrument, an effect, a sound or a sample. |
| **Instrument** | Code that turns notes into sound (a synth, a sampled piano). Written in TypeScript, like std's. |
| **Effect** | Code that changes sound (a reverb, a distortion). |
| **Sound** | An instrument or effect with its knobs set, under a name: a preset ([FMT-22](../../architecture.md#presets)). "Warm Pad" is a sound of std's synth. Most packs are sounds. |
| **Sample** | A short recording: a **one-shot** (one hit: a kick, a "hey") or a **loop** (a bar or more that repeats, with its own tempo). |
| **Kernel** | The fast part of an instrument or effect, compiled to WebAssembly (wasm), as std's are. Only new kinds of sound need one; most packs have none. |
| **Registry** | The list of packs you can get, kept in one GitHub repo. |
| **Store** | Where packs you got live on your computer: `~/Music/music2/Packs/`. |
| **Hash** | A fingerprint of a file's bytes (SHA-256). One changed byte gives another hash, so the app knows the file is the one the registry checked. |

## 2. What the app ships, and what comes as packs

| Ships with the app | Comes as a pack |
|---|---|
| std: its oscillators, filters, effects, synths, FM, drum machine and Sampler (code only) | sampled instruments: pianos, guitars, strings, choirs |
| std's presets | more sounds for std's synths |
| one small drum kit of samples, a few MB, so the Sampler has something to play offline | sample packs: drums, vocal chops, loops, textures, foley |
| | new instruments and effects with kernels of their own |

std stays in the app: every template and example uses it, and it is tiny. What makes a library big is
samples, and those are what move out.

## 3. The primitives

Nine building blocks. Each is small, and each has one job.

| # | Primitive | Its job | Section |
|---|---|---|---|
| 1 | **Pack** | a folder and its `m2pkg.toml`: one unit to make, check, publish and get | §4 |
| 2 | **Items and tags** | four kinds of item, each with fixed fields search can filter on | §5 |
| 3 | **Pack version** | a version never changes once published; it is named by the hash of its file list | §4.3 |
| 4 | **Registry** | one GitHub repo; a file per pack; a robot that checks | §7 |
| 5 | **Sources** | where packs come from: the registry, a GitHub repo given by hand, a folder on your disk | §7.3 |
| 6 | **Store** | packs on your computer, read-only, several versions side by side | §8 |
| 7 | **Lock** | `music2.lock`: each song's exact pack versions and hashes | §8 |
| 8 | **Search index** | a copy of the registry's list on your computer, so search is instant and private | §6 |
| 9 | **Provenance** | a note beside each sample you add: which pack, which version, who made it, its licence | §8 |

## 4. A pack

### 4.1 On disk

```
felt-piano/                 a GitHub repo: github.com/alice/felt-piano
  m2pkg.toml                what the pack is (below)
  README.md  LICENSE
  src/*.ts                  instruments and effects: components, as std/src/ has (PKG-1)
  kernels/*.wasm            optional: new DSP, kernel ABI v1 (docs/kernel-abi.md)
  kernels-src/              optional: the Rust the wasm is built from
  presets/<component>/*.json   sounds (FMT-22)
  samples/**/*.wav|flac     samples
  samples.toml              the samples' tags: made by `m2 pack init`, fixed by hand
  .github/workflows/publish.yml   made by `m2 pack new`: publishes on a tag (§7.4)
```

The publish robot adds `previews/` (§6.3) and `m2pack.json` (§4.3). Nobody writes those by hand.

### 4.2 `m2pkg.toml`

std's file today has `[package]` and `[[kernel]]`. A pack adds a few fields:

```toml
[package]
name = "@alice/felt-piano"      # @owner/name; owner is the GitHub account that publishes it
version = "1.2.0"               # semver; never reused
license = "CC-BY-4.0"           # SPDX; the registry accepts only some (§7.5)
description = "A soft felt piano, recorded close"
authors = ["Alice Example"]
repository = "https://github.com/alice/felt-piano"
tags = ["keys", "piano", "lofi"]
wasm = "kernels/felt.wasm"      # optional, as std's

[dependencies]                  # only "@m2/core" and std (§4.4)
"@m2/core" = "1"
std = "0.4"
```

A song imports from it as it imports from std: `import { feltPiano } from "@alice/felt-piano"`. Its
ComponentIds are `@alice/felt-piano:felt-piano` ([PKG-9](../../architecture.md#component-descriptors)),
and its kernels' ids `@alice/felt-piano:felt` (the "kernel ids relative to their package" the
[Deferred](../../architecture.md#deferred-with-triggers) table waits for).

### 4.3 A version never changes

- Publishing `1.2.0` makes `m2pack.json`: every file in the pack, with its size and hash. The hash of that
  list is the version's hash. The registry and every song's lock keep it.
- The download is one file, `felt-piano-1.2.0.m2pack`: a zip, its files stored as they are (no
  compression: FLAC is compressed already). So one file can be read out of it on its own: that is how one
  sample is got without the whole pack (§6.2).
- A published version is never changed or reused. A fix is `1.2.1`. This is crates.io's and npm's rule,
  and the reason a song from 2026 still sounds the same in 2030.

### 4.4 Packs don't depend on packs

A pack may use `@m2/core` and std, nothing else. So the app never has to solve which versions of five
packs fit together (npm's hardest problem), and removing a pack never breaks another. If packs need each
other later, that is its own design.

## 5. Items and their tags

Search is only as good as the tags. Two rules make them good:
- **What can be measured is measured**, by `m2 pack check` and the registry's robot, never typed: a
  sample's length, channels, rate, loudness and hash.
- **What can't is chosen from a short list** kept in the registry repo (`vocabulary.toml`), and the robot
  refuses a word not on it. Free tags are allowed too, for search only. Core holds no list
  ([PKG-7](../../architecture.md#component-descriptors)); the list lives with the registry.

| Kind | Fields (bold: required) | Search filters |
|---|---|---|
| Instrument | **label**, **category** (Drums, Bass, Keys, Pad, Lead, Pluck, Strings, Brass & Wind, Vocals, FX & Texture), description, tags | category |
| Effect | **label**, **category** (std's: Filter, EQ, Dynamics, Distortion, Delay & Reverb, Modulation, Utility), description, tags | category |
| Sound | **name**, **sound** (its instrument or effect), the category of that, character tags (warm, bright, dark, wide, dirty, soft) | category, character |
| Sample | **type** (one-shot or loop), **instrument** (kick, snare, hat, clap, perc, bass, keys, guitar, vocal, fx, texture, …); for a loop **bpm**; key if it has one; root note for a one-shot played by the Sampler; genre | type, instrument, bpm, key, "fits my song" |

Instruments and effects already have `label`, `editor.category`, description and keywords in their
descriptors; packs use the same fields. Sounds and samples get theirs from the preset file and
`samples.toml`.

`samples.toml` sets tags by folder, and by file where one differs:

```toml
[folder."drums/kicks"]
type = "one-shot"
instrument = "kick"

[folder."loops"]
type = "loop"

[file."loops/dusty keys 90 Am.wav"]
bpm = 90                  # m2 pack init guessed 90 and Am from the name; the author checks
key = "A minor"
tags = ["dusty", "lofi"]
```

## 6. Search: ⌘F

### 6.1 How it looks

- **⌘F** anywhere opens it (Live's key; free in [taste.md](../step8/taste.md)'s key table). The side
  browser's search field is the same search, so there is one, not two.
- One list, in groups: Instruments, Sounds, Effects, Samples, Packs. Typing filters as you type.
- ↓ and ↑ move. **Space** plays a preview. **Enter** adds it to the selected track (an instrument makes a
  track; an effect goes after the last one, as the browser does today; a sample goes on the track at the
  playhead, or onto a Sampler). Dragging works as the browser's does now.
- Filters are words you can type or click: `loop`, `one-shot`, `kick`, `90 bpm`, `Am`. One switch, **Fits
  my song**, keeps loops near the song's tempo and in a key that fits.
- What you have comes first. Below it, **From the library**: items in packs you don't have, each with
  **Get** and a badge (§7.3).

### 6.2 Getting one sample

Get on a sample fetches that one file out of its pack's download (an HTTP "range" read of the zip, §4.3),
checks its hash, and adds it. A 2 GB pack isn't needed for one clap. Get on an instrument or a sound
fetches its whole pack: its code and samples go together.

### 6.3 Previews

- A sample's preview is the sample itself (fetched as in §6.2, kept in the cache folder).
- An instrument's or a sound's preview is a short phrase the publish robot renders with the pack's own
  code (`m2 render`), at one loudness for all. A preview can't oversell: it is the pack playing.
- Later: a loop previewed at the song's tempo and key (std's stretch already does tempo, offline).

### 6.4 Where search runs

On your computer. The app keeps a copy of the registry's search list (a few MB), and fetches a fresh one
when it is a day old. So search is instant, works offline for what you have, and what you type is never
sent anywhere. Matching is by words and tags, ranked: name, then tags, then description; what you have
and music2's own packs first; then most downloaded (GitHub counts each release's downloads; no tracking
in the app). No AI ([goals](../../goals.md#non-goals)).

### 6.5 Your own folders

**Add a folder** puts a folder of your own samples in the library (Live's "Places"). The app measures each
file as the robot would, guesses bpm and key from names, and search finds them with everything else. They
are never copied until you add one to a song. This is the "I have 4,000 samples on a drive" case.

## 7. The registry

### 7.1 One repo of small files

A public repo, `BreadBowlAI/music2-packs`, with one file a pack:

```toml
# packs/@alice/felt-piano.toml
repo = "github.com/alice/felt-piano"
badge = "community"            # or "music2" for our own

[[version]]
version = "1.2.0"
hash = "sha256:…"              # of m2pack.json (§4.3)
url = "https://github.com/alice/felt-piano/releases/download/v1.2.0/felt-piano-1.2.0.m2pack"
size = 412_000_000
published = 2026-11-02
```

Its CI builds the search list (§6.4) from these files and the packs' `m2pack.json`, and puts it on GitHub
Pages. That is the whole server: no database, no accounts, no bill. Homebrew, Obsidian, Zed and Flathub
run their lists this way.

### 7.2 Where the files are

Each pack's download is a file on its own repo's GitHub Release. GitHub keeps release files of public
repos for free, with no bandwidth limit, up to 2 GB a file. music2's own packs are releases of
`BreadBowlAI` repos.

One risk: an author who deletes their repo takes their pack with it, and every song that used it goes
quiet. crates.io, npm and Go keep a copy of everything for this reason. So later the registry keeps a copy
of every version (a mirror, such as Cloudflare R2, which charges nothing for downloads). The registry's
files already allow several `url`s a version, and the hash checks whichever answers. Trigger: the first
pack that disappears, or 1.0, whichever comes first.

### 7.3 Three kinds of source

| Source | How it gets in | What the app shows |
|---|---|---|
| **music2** (ours) | made by the owner; the same checks | a music2 badge; ranked first |
| **Community** | a pull request to the registry repo; the robot checks every version, and a person looks at the first one | a Community badge |
| **By hand** (not listed) | you paste `github:owner/repo` | "Not in the music2 library: no one has checked its sounds or licence." It still runs in the sandbox (§9). Not in search for others |

A folder on your disk (§6.5) is a fourth, for your own files.

### 7.4 Publishing a pack

| Step | The author does | What happens |
|---|---|---|
| 1 | `m2 pack new felt-piano` | a folder with `m2pkg.toml`, `samples.toml`, a README, a licence to pick, and the publish workflow |
| 2 | adds sounds and samples; `m2 pack init` | tags guessed and written into `samples.toml` to check |
| 3 | `m2 pack check` | the same checks the registry runs (§7.5), on their computer |
| 4 | tries it in a song: `"@alice/felt-piano" = { path = "../felt-piano" }` | the song uses the folder as it is, with no publishing |
| 5 | pushes a tag, `v1.0.0` | music2's GitHub Action checks, builds the kernels the same way every time, renders the previews, makes the `.m2pack`, puts it on the release with an attestation (GitHub's proof it was built from that commit), and opens a pull request to the registry |
| 6 | — | first version: a person at music2 looks and merges. Later versions from the same repo merge themselves when the robot passes |

Agents can do every step too: they are `m2` commands.

### 7.5 What the robot checks

| Check | Why |
|---|---|
| `m2pkg.toml` reads strictly; the name's owner is the repo's owner | no one publishes as someone else |
| the version is new | a version never changes (§4.3) |
| only these files: `.ts`, `.wasm` (kernel ABI v1), `.json`, `.toml`, `.md`, `.txt`, `.wav`, `.flac` | no programs, no surprises |
| every kernel loads in the sandbox; every component builds; every preset loads on its sound | it works in the app |
| every sample decodes; tags are measured; words come from the vocabulary | search is right |
| size: a sample up to 100 MB, a pack up to 2 GB | GitHub's limit, and a sane download |
| a licence from the allowed list: code MIT, Apache-2.0, BSD, ISC; sounds and samples CC0-1.0 or CC-BY-4.0 | everything in the library can be used in a song you sell (owner decision D6) |
| a README; previews made | people know what they get |

### 7.6 Yank and take-down

- **Yank** (the author's choice, or ours for a bad version): hidden from search; a song that locks it can
  still get it, with a note.
- **Take-down** (a licence or legal problem): the registry drops it. A song that used it says so in words.
  Samples already in songs stay: they are the song's files.

## 8. Using a pack in a song

- **The store.** A pack you get goes to `~/Music/music2/Packs/@alice/felt-piano/1.2.0/`, read-only, under
  `$HOME` ([PLAT-7](../../architecture.md#platforms)). Several versions sit side by side, so an old song
  keeps its version while a new song uses the newest. (This also gives the "several std versions" the
  contract waits for, [FMT-3](../../architecture.md#manifest-and-versions).)
- **Adding an instrument, effect or sound** writes the pack into the song's `[dependencies]` and its exact
  version into `music2.lock`, with the edit: one Undo step.

  ```toml
  # music2.lock: made by music2. The exact pack versions this song uses.
  [[pack]]
  name = "@alice/felt-piano"
  version = "1.2.0"
  hash = "sha256:…"
  source = "registry"          # or "github:alice/felt-piano"
  ```
- **Adding a sample** copies the file into the song's `assets/` ([FMT-14](../../architecture.md#assets-and-hashes):
  a song holds its own audio), as a WAV or FLAC, with a note beside it, `assets/<file>.json`:
  `{"from": {"pack": "@alice/felt-piano", "version": "1.2.0", "file": "samples/…", "author": "Alice
  Example", "license": "CC-BY-4.0"}}`. The song never needs the pack for its samples, and `m2 credits`
  (later) lists who to credit.
- **A song whose pack you don't have** still opens. Its tracks that use the pack play silent, as a missing
  plugin does ([FMT-18a](../../architecture.md#plugin-state-and-parameters)), and the status says "This
  song uses Felt Piano 1.2.0" with **Get**. One click gets that exact version, checks the hash, and plays.
- **Updates are never automatic** for a song. The library shows "1.3 is out". A song moves when you choose
  Update in its pack list: one Undo step. **Clean up** lists versions no song locks.
- **From the command line** (for scripts and agents): `m2 search "vocal chop" --loop --key Am`,
  `m2 get @alice/felt-piano`, `m2 add <song> <item>`.

## 9. Why a pack can't hurt your computer

| A pack holds | Why it is safe |
|---|---|
| TypeScript | runs in the app's sandbox (QuickJS): no files, no network, no clock |
| Kernels | wasm in a sealed sandbox with one import, `asset_read` ([ABI-3](../../architecture.md#kernel-abi-reservations)); it can't grow memory or reach anything else |
| Presets, tags | data, read strictly |
| Audio | decoded by music2's own decoder, size-checked |
| A download | its hash checked before use; paths in the zip checked (nothing outside the pack's folder) |

**No native programs in packs, ever.** So no CLAP plugin inside a pack: CLAP plugins are programs, and
come from their makers ([goals](../../goals.md#non-goals): CLAP only). A pack may hold sounds for a CLAP
plugin; search then says "needs Vital" until you have it.

The same sandbox makes packs exact: wasm and std's maths give the same bits on every computer
([PLAT-4](../../architecture.md#platforms)), so a song that uses only std and packs exports the same bits
everywhere.

## 10. music2's own packs, to start

Content, not code: someone has to make or gather the sounds. Ideas:

| Pack | What | Where the sounds could come from |
|---|---|---|
| `@m2/synth-sounds` | 100+ sounds for std's synths, FM and drum machine | made in music2; tiny, no samples |
| `@m2/drums` | acoustic and electronic kits, one-shots, Sampler kits | recorded, or CC0 |
| `@m2/chops` | vocal one-shots and phrases, for chopping | recorded with singers, with their written OK |
| `@m2/keys` | piano, electric piano, organ (sampled) | recorded, or CC0 |
| `@m2/textures` | vinyl, tape, rooms, field recordings, foley | recorded, or CC0 |

CC0 means the maker gave up every right: anyone may pack and share it. Freesound has many such sounds.

## 11. Freesound, later

Freesound is a public library of hundreds of thousands of recordings under Creative Commons licences, with
a search API and previews. It could be a source in ⌘F: results under "Freesound", each with its licence.
Before building it: read its API terms (free use is for non-commercial projects), and getting a sound's
original file needs a Freesound login. Owner decision D8.

## 12. What changes in the contract

| Where | Change |
|---|---|
| a new section, Packs (`PACK-1` …) | the pack, its versions and hash, the store, the lock, sources, the checks, the sandbox rule (no native code) |
| [FMT-2](../../architecture.md#manifest-and-versions) | package names `@owner/name`; a `{ path = … }` dependency |
| Layout, [Deferred](../../architecture.md#deferred-with-triggers) | `music2.lock` and package fetching: their trigger ("the first package other than std") arrives |
| [FMT-14](../../architecture.md#assets-and-hashes) | an asset's `<file>.json` may say which pack it came from |
| [PKG-9](../../architecture.md#component-descriptors) | ComponentIds and kernel ids carry the pack's scoped name |
| [PLAT-7](../../architecture.md#platforms) | `~/Music/music2/Packs/` |
| [PKG-7](../../architecture.md#component-descriptors) | unchanged: the vocabulary lives in the registry repo, not in core |

## 13. Order of work

Each slice lands green on its own, as every track has.

| # | Slice | What you get | Size |
|---|---|---|---|
| 1 | **Packs from a folder** | the format, the store, `music2.lock`, `path` packs, scoped names; the browser lists a pack's instruments, effects and sounds | medium |
| 2 | **Samples as items** | `samples.toml`, measured tags, a Samples group, Space to preview, drag in with provenance; FLAC, AIFF and MP3 import (part of gap #18); Add a folder | medium |
| 3 | **⌘F search** | one search over everything you have; filters; Fits my song | medium |
| 4 | **The registry** | the registry repo and its robot, the publish Action, `m2 pack new/init/check`, Get from search, Get on open, one sample on its own. First: confirm GitHub's release downloads allow range reads (§6.2); if not, the mirror (§7.2) comes now, as it serves single files | large |
| 5 | **music2's packs** | the first packs, made with the tools of slice 4: the real test of the format | ongoing |
| 6 | **Later, each at its trigger** | packs by hand from GitHub (when someone asks), the mirror (§7.2), Freesound (D8), favourites, a loop previewed at the song's tempo | small each |

## 14. Decisions for the owner

| # | Decision | Recommended |
|---|---|---|
| D1 | Where the registry lives | one GitHub repo of small files (§7.1); downloads on each pack's GitHub Release; a mirror later |
| D2 | Pack names | `@owner/name`, owner = the GitHub account; ours under `@m2/`; std stays `std` |
| D3 | Programs in packs | never: no native code, no CLAP inside a pack |
| D4 | Packs that need other packs | not in version 1: only `@m2/core` and std |
| D5 | Packs by hand from any GitHub repo | yes, with a plain warning; after the registry works |
| D6 | Licences the registry accepts | only ones that let you sell a song made with it: CC0 and CC-BY for sounds; MIT, Apache, BSD, ISC for code. No "non-commercial" packs, so a beginner never gets caught out |
| D7 | The search key | ⌘F (Live's); the side browser's field is the same search |
| D8 | Freesound as a source | later, after its terms are read |
| D9 | What the app ships | std, std's presets, one small drum kit |

## 15. Where each idea comes from

| Idea | From |
|---|---|
| Packs, a browser over everything, your own folders as places | Ableton Live |
| Get one sample; filter by key, bpm, one-shot or loop; fits my song | Splice |
| A fixed list of tags; a preview for every sound | Native Instruments' NKS |
| Versions never change; yank; a lockfile; scoped names | crates.io, npm |
| The registry keeps a copy; the source is a repo | Go's module proxy |
| The list is a GitHub repo; a pull request to add | Homebrew, Obsidian, Zed, Flathub |
| Built by CI with an attestation | npm's and PyPI's trusted publishing |
| Licences you can trust | Creative Commons, Freesound |

## 16. Not in this design

- **Paid packs, a store, accounts in the app.** Free packs first; a paid store is its own design.
- **Ratings and comments.** GitHub's issues on each pack's repo serve for now.
- **AI tagging or AI search** ([goals](../../goals.md#non-goals)).
- **CLAP plugins from the registry** (D3).
