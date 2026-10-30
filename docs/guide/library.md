# Getting packs from the library

**In short**
- The library is music2's list of packs anyone can get: instruments, effects, sounds and samples that people made and shared.
- In ⌘F, after what you have, **From the library** shows what you can get. **Space** plays it. **Get** (or Enter) gets it and adds it to your song.
- A song that uses a pack you don't have offers **Get** for it, and `m2 pack get` gets it from the command line.
- What you type is never sent anywhere: the search runs on your computer.

## Words

| Word | Meaning |
|---|---|
| **the library** | the list of packs anyone can get, kept by music2 (the registry) |
| **pack** | a folder of instruments, effects, sounds and recordings someone made ([Packs](packs.md)) |
| **sample** | one recording, a WAV or FLAC file: a kick, a loop ([Samples](samples.md)) |
| **catalog** | the list of everything in the library, which music2 keeps on your computer |
| **Get** | download something from the library, check it, and add it |

## Find, hear, get

1. Press **⌘F** (Ctrl+F on Linux) and type a word: `piano`, `kick`, `warm pad`.
2. What you have comes first, in its groups. Below them, **From the library** lists what you can get. Each row says its pack, such as `@alice/felt-piano`; music2's own packs say `music2`.
3. Move to a row with ↓ and press **Space** to hear it. Press Space again to stop.
4. Press **Enter**, or click **Get** in its row. The row shows how far it is, and the foot says how much has come. To stop it, press **Esc** or click **Cancel**: nothing of it is kept.
5. When it is here, the panel closes and it is added, as Enter adds anything:

| You get | What happens |
|---|---|
| an instrument | its pack is installed, and a new track plays it |
| a sound | its pack is installed, and a new track plays its instrument with that sound |
| an effect | its pack is installed, and it goes on the selected track |
| a sample | only that one file comes, into your song's `assets/` folder, on a new audio track at the playhead. A FLAC comes in as a WAV |

Each is one undo step. The song's `music2.toml` and `music2.lock` then name the pack and its exact version, as when you add an installed pack's instrument ([Packs](packs.md)).

One Get runs at a time. If a Get fails, the foot says why in plain words: no internet, the server didn't answer, or the file wasn't what the library said.

## A song that needs a pack you don't have

A song made with a pack you haven't got doesn't play until you get it. The status pill says the code doesn't compile; open it, and the pack is there with **Get**. Once it is here the song builds and plays, with the very version it was made with.

From the command line, in the song's folder:

```
m2 pack get
```

It gets every pack the song's `music2.lock` names that isn't installed, and says what it got.

| It says | Why |
|---|---|
| got @alice/drums 1.0.0 | it is installed now |
| was yanked | the author hid that version from search; a song that uses it still gets it |
| was taken down | the version was removed from the library (for a licence or legal reason): its files are gone |
| has other files than the ones this song was made with | the library's version isn't the one in the song's lock, so it isn't got |

## From the command line

| Command | What it does |
|---|---|
| `m2 pack get @alice/felt-piano` | get the newest version of a pack |
| `m2 pack get @alice/felt-piano@1.2.0` | get that version |
| `m2 pack get` | in a song's folder: get every pack the song needs and doesn't have |
| `m2 search felt piano` | find things, the library's too (marked `library`) |

## Where the library is

music2 reads the library at one address:

| Where | Example |
|---|---|
| `M2_REGISTRY` | `M2_REGISTRY=https://packs.example m2 pack get @alice/drums` |
| `registry` in `~/.music2/settings.json` | `{"registry": "https://packs.example"}` |

music2 has no default address yet: until one is set, ⌘F shows no From the library, and `m2 pack get` says that no registry is set. A folder works too (`file:///Users/you/bucket`): a copy of the library on a drive serves the same files.

## What music2 sends, and what it keeps

| When | What music2 downloads |
|---|---|
| the app starts, then once a day | the catalog: `catalog.json`, then the catalog it names if it changed |
| you press Space on a library item | that item's preview |
| you Get something | its pack, or its one sample |

Nothing else. No account, no id, and never what you type: search runs over the catalog on your computer. Every file is checked by its fingerprint (its hash) before it is used, so a file changed on the way is never kept. A pack goes in whole or not at all.

Offline, the catalog from last time is used. The catalog and the previews you played are kept in music2's cache folder (`~/Library/Caches/music2` on a Mac, `~/.cache/music2` on Linux).
