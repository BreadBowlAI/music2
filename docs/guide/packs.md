# Packs

**In short**
- A pack is a folder of instruments, effects, sounds and recordings that someone made, ready to use in your songs. music2's own sounds are packs that come with it.
- You install one from its folder with `m2 pack install <folder>`. Its instruments and effects then show in the browser and + Track, with music2's.
- A song keeps the exact version of each pack it uses in `music2.lock`, so it sounds the same years later.
- A pack made for `@m2/core` 2 keeps working in every later music2 with `@m2/core` 2.x: music2's one promise to packs.

## What a pack is

| Word | Meaning |
|---|---|
| **pack** | a folder of sounds others made: instruments (notes in, sound out), effects (sound in, sound out), sounds (an instrument's settings under a name) and recordings |
| **pack name** | `@owner/name`, such as `@alice/felt-piano`: who made it, then what it is. music2's own are `@m2/…`; its toolbox, std, is just `std` |
| **version** | which release of the pack: `1.2.0`. A version never changes once it is out |
| **the store** | where installed packs are kept on your computer: `~/Music/music2/Packs`, one folder a version |
| **music2.lock** | a file in your song that says which version of each pack it uses, and that version's fingerprint (its hash) |

A pack is safe to get from a stranger: it holds only code that runs in music2's sandbox (no files, no network), kernels in a sealed sandbox, data and audio. Never a program, and never a link to a file outside it: music2 refuses a pack that holds one.

## The packs that come with music2

music2's sounds are packs too, made by music2 and kept in the repository at `packs/@m2/`:

| Pack | What is in it |
|---|---|
| `@m2/synths` | the polysynth's sounds (supersaw, lead, pad, pluck, 808 sub, bass, strings), the FM synth's (keys, bell, metal), each synth with every knob, and three simple instruments to read and copy |
| `@m2/drums` | an 808/909 drum machine: the whole kit, and One drum, which plays one drum, picked from its menu |
| `@m2/effects` | delay, reverb, chorus, phaser, tremolo, drive, bitcrush and widen |
| `@m2/mixing` | the tools a mix needs: an eight-band EQ, a gate, a transient shaper, a de-esser and a multiband compressor |

They come with the app and with `m2`. When either starts, it installs each one your store doesn't have yet, as `m2 pack install` would; a start that finds them there writes nothing. From then on they are packs like any other: a song lists them in `music2.toml`, locks them in `music2.lock`, and keeps the version it was made with when a newer music2 brings a newer one. The templates and examples come locked to them.

If your store holds an `@m2/` pack of that version with other files (someone else's), music2 leaves it as it is, says so in its log, and starts all the same. A new song locked to music2's own then says `pkg.changed`: run `m2 pack remove <pack> <version>` (the error names both), then start music2 again, and it puts its own back. A pack named `@m2/…` is music2's only when its files are a version music2 comes with: `m2 pack install` refuses any other.

std itself is no pack: it is the toolbox the packs are built from ([Sounds and effects](sounds.md)).

## Installing a pack

1. Get the pack's folder (from its author, or a GitHub repository).
2. Run `m2 pack install <folder>`. It checks the pack first, and says in words what is wrong if anything is.
3. Open your song again. The pack's instruments and effects are in the browser, under their categories. Hover one: its tooltip says which pack it is from.

| Command | What it does |
|---|---|
| `m2 pack install <folder>` | check the pack and copy it into your store, read-only. Installing a version you have again puts back a copy that was changed |
| `m2 pack get @owner/name` | get a pack from the library, checked, into your store ([Getting packs from the library](library.md)) |
| `m2 pack list` | the packs you have, each with its versions |
| `m2 pack remove @owner/name <version>` | take a version out of your store |

The browser's **Packs** list, at its foot, shows the packs you have and what is in each.

## Using a pack's instrument

Double-click it in the browser (or pick it in + Track), as with music2's own. music2 writes three things, as one undo step:

- the track's code, importing the instrument by the pack's name: `import { feltPiano } from "@alice/felt-piano";`
- a line in `music2.toml` under `[dependencies]`: `"@alice/felt-piano" = "1.2"` (this version or a newer one that keeps working the same way)
- an entry in `music2.lock`: the exact version, `1.2.0`, and its fingerprint

Effects work the same way: drag one onto a track, or use the Device view's **+**.

## What music2 promises a pack

A pack is written against `@m2/core`, the code music2 gives every song and pack. From `@m2/core` 2.0, it only grows: a change that could break a pack's code comes only as a new major version (3), and music2 says in words when a pack was made for another one. So a pack made for `@m2/core` 2 runs in every music2 that has 2.x. Songs get no such promise before music2 1.0: a new music2 may ask a song to change, and says how.

## How a song keeps its version

A song uses the version its `music2.lock` names, even when you install a newer one. Before a song uses an installed version, music2 checks every file of it against the fingerprints it was installed with. Nothing moves a song to another version but you:

| To | Do |
|---|---|
| move to another version | change the line in `music2.toml` (say `"1.3"`), then run `m2 pack lock <song>`. It locks the newest installed version the line allows that this music2 can run |
| move to the newest version the line allows (and this music2 can run) | `m2 pack update <song>`, or `m2 pack update <song> @alice/felt-piano` for one pack |

Both work while the song doesn't build, and each is one undo step. The song may sound a little different after.

When something is wrong, the song says so, in words, and doesn't build until it is fixed:

| Problem | What it means | What to do |
|---|---|---|
| `pkg.missing` | the version the lock names isn't installed | install that version: `m2 pack install <folder>`. Or, when another version the line allows is installed, `m2 pack lock <song>` moves to it |
| `pkg.changed` | a file of the installed version was changed after it was installed (the message names it) | install it again from its folder: `m2 pack install <folder>` puts it back |
| `pkg.changed` | the installed version isn't the copy the song was made with | put that one back (`m2 pack remove @owner/name <version>`, then `m2 pack install <its folder>`), or `m2 pack update <song> @owner/name` to use the installed one |
| `pkg.unsatisfied` | no installed version fits `music2.toml`'s line, or the lock names one the line no longer allows. Or the versions that fit need a newer music2: a newer music2 on this computer installed them (the message says what they need) | install one that fits, or change the line; then `m2 pack lock <song>`. For a newer music2's version: update music2 |
| `pkg.broken` | the pack needs a newer std or `@m2/core` than this app has | update music2, or use another version of the pack |
| `pkg.broken` | the pack is made for another major version of `@m2/core` (`is made for @m2/core 1, and this app has @m2/core 2.0.0`) | get a version of the pack made for this app's: the message names it |
| `sdk.unsatisfied` | the song asks for another major version of `@m2/core` than this app has | for an older one, the message says the line to write first (`"@m2/core" = "2"`); the song's code may need changes. For a newer one, update music2 |

Removing a pack can't change a song: a song that locks it says `pkg.missing` until it is installed again.
