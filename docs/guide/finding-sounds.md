# Finding sounds (⌘F)

**In short**
- Press **⌘F** (Ctrl+F off the Mac) anywhere outside a text field, type a few words, and press **Enter** to add what you found.
- One search finds everything you can add: instruments, sounds (presets), effects, and your plugins. Later, samples from packs.
- **Space** plays a short preview first. **Esc** closes.

## The panel

⌘F, or **View ▸ Find Sounds…**, opens a search over the window. Type, and what matches shows as you type, in groups: **Instruments**, **Sounds**, **Effects**, **Samples** and **Plugins**. A group with nothing in it is hidden.

Each row says what it is called, then quietly what kind of sound it is and where it comes from: "music2" for the sounds music2 comes with, "Yours" for presets you saved.

| Key | What it does |
|---|---|
| ⌘F | Open the panel, or close it |
| Type | Find; every word must match |
| ↓ ↑ | Move from row to row, across the groups |
| Space | Play the row's preview, after you moved with ↓ or ↑. Again, or moving, stops it |
| Enter | Add it, and close |
| Esc | Close |

A row also drags onto the timeline or a track, as the side browser's rows do.

In the code editor ⌘F finds text in your code, as it always did. In a text field it does nothing.

## What Enter does

| Row | Enter |
|---|---|
| An instrument | adds a new track that plays it, below the selected track |
| A sound (a preset) | loads its settings into the selected track's instrument or effect it is for. If the selected track doesn't play it, the panel says so: select a track that does, or add one first |
| An effect | puts it after the last effect on the selected track |
| A plugin | as an instrument or an effect, by what the plugin is |

Each is one step that **Undo** (⌘Z) takes back, and a note at the bottom says what it did.

## How it finds things

- Each word you type must match somewhere: the name, the start of a word in it, a word of what it is (its group, what code calls it, a drum the kit plays), or a word of its description. Capitals don't matter.
- The best match is selected: the name itself, then names that start with what you typed, then a word of the name, a tag, the description.
- music2's own sounds come before yours, and yours before plugins.

Some words are filters, not words to find:

| Type | Finds only |
|---|---|
| `instrument`, `sound`, `effect`, `sample`, `plugin` (or `fx`, `preset`, and the plurals) | that kind |
| `loop`, `one-shot` | samples of that type |
| `90 bpm` or `90bpm` | samples at that tempo |
| `Am`, `F#`, `Bbm`, `C major` | samples in that key (a capital letter, then a ♯, a ♭ or `m`, or `major`/`minor` after it) |

So `pad sound` finds the pad's presets, and `delay effect` the Delay. Samples come with packs, later.

## Previews

A preview is about 4½ seconds of the item playing, made by music2 itself: it writes a tiny song and renders it, as Export does.

| Item | What you hear |
|---|---|
| Drums | a two-bar beat: kick, snare and hats |
| A bass | a bass line |
| A lead | a short melody |
| Anything else (keys, pads, strings, plucks) | four chords |
| A sound | the same, with the sound's settings |
| An effect | a pluck and drums: one bar without the effect, then one bar through it |

Every preview is brought to the same loudness, so you can compare them. The first time takes a moment; after that it is kept (in music2's cache folder, never in your song) until the sound or music2 changes. Previews play in the window, never through your song, so they never change what your song plays.

Plugins have no preview yet.

## The side browser

The browser's search field (⌥⌘B shows it) asks the same search: its rows keep their groups, with the best matches first.

## From the terminal

`m2 search <words>` asks the same search: through the app when it is open, else by itself, from music2's sounds and your presets. `--kind effect` finds only effects; `--json` prints every item with all it knows. See [m2 commands](commands.md).
