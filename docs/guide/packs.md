# Packs

**In short**
- A pack is a folder of instruments, effects, sounds and recordings that someone made, ready to use in your songs, as std's are.
- You install one from its folder with `m2 pack install <folder>`. Its instruments and effects then show in the browser and + Track, with std's.
- A song keeps the exact version of each pack it uses in `music2.lock`, so it sounds the same years later.

## What a pack is

| Word | Meaning |
|---|---|
| **pack** | a folder of sounds others made: instruments (notes in, sound out), effects (sound in, sound out), sounds (an instrument's settings under a name) and recordings |
| **pack name** | `@owner/name`, such as `@alice/felt-piano`: who made it, then what it is. std, music2's own, is just `std` |
| **version** | which release of the pack: `1.2.0`. A version never changes once it is out |
| **the store** | where installed packs are kept on your computer: `~/Music/music2/Packs`, one folder a version |
| **music2.lock** | a file in your song that says which version of each pack it uses, and that version's fingerprint (its hash) |

A pack is safe to get from a stranger: it holds only code that runs in music2's sandbox (no files, no network), kernels in a sealed sandbox, data and audio. Never a program.

## Installing a pack

1. Get the pack's folder (from its author, or a GitHub repository).
2. Run `m2 pack install <folder>`. It checks the pack first, and says in words what is wrong if anything is.
3. Open your song again. The pack's instruments and effects are in the browser, under their categories. Hover one: its tooltip says which pack it is from.

| Command | What it does |
|---|---|
| `m2 pack install <folder>` | check the pack and copy it into your store, read-only |
| `m2 pack list` | the packs you have, each with its versions |
| `m2 pack remove @owner/name <version>` | take a version out of your store |

The browser's **Packs** list, at its foot, shows the packs you have and what is in each.

## Using a pack's instrument

Double-click it in the browser (or pick it in + Track), as with std's. music2 writes three things, as one undo step:

- the track's code, importing the instrument by the pack's name: `import { feltPiano } from "@alice/felt-piano";`
- a line in `music2.toml` under `[dependencies]`: `"@alice/felt-piano" = "1.2"` (this version or a newer one that keeps working the same way)
- an entry in `music2.lock`: the exact version, `1.2.0`, and its fingerprint

Effects work the same way: drag one onto a track, or use the Device view's **+**.

## How a song keeps its version

A song uses the version its `music2.lock` names, even when you install a newer one. Nothing moves it to a newer version but you:

- Change the line in `music2.toml` (say `"1.3"`), then run `m2 pack lock <song>`. The song may sound a little different.

When something is wrong, the song says so, in words, and doesn't build until it is fixed:

| Problem | What it means | What to do |
|---|---|---|
| `pkg.missing` | the version the lock names isn't installed | install that version: `m2 pack install <folder>` |
| `pkg.changed` | the installed version's files differ from the ones the song was made with | install the version the song locks, or run `m2 pack lock` to use this one |
| `pkg.unsatisfied` | no installed version fits `music2.toml`'s line | install one that fits, or change the line |
| `pkg.broken` | the pack needs another std or `@m2/core` than this app has | update music2, or use another version of the pack |

Removing a pack can't change a song: a song that locks it says `pkg.missing` until it is installed again.
