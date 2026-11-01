<!-- Made by `m2 docs commands` from apps/m2/src/commands.rs: change it there, then run ci/guide.sh gen. -->
# m2 commands

**In short**
- Every `m2` command: how to call it, and what it does. `m2` alone prints the same, shorter.
- `m2` works on song folders, with the app open or closed. A command that writes ends with `--as <your name> --why "<a few words>"`.
- This page is made from `m2`'s own table, so `m2 docs commands` prints it as that `m2` has it.

From source, `cargo build --locked --release -p m2` builds `m2` as `target/release/m2`; in `music2.app` it is `music2.app/Contents/MacOS/m2`, and from the Linux `.deb` `/usr/bin/m2`. In the examples below it is called `m2`. `<project>` is a song's folder.

## How every command behaves

Every command that reads a project prints what needs attention in it, such as data no lane reads, with the commands that fix it.

One program writes a project at a time (its lock is `.music2/lock`). While the app has the song open, a command that writes it hands its edit to the app, which makes it one undo step, named by `--as <name>` (or `M2_AS`) and `--why <words>`, which go last, after the command's own arguments: the musician sees it there, and `m2 undo` takes it back. While the musician drags what it changes, records, or has unsaved code in its file, the app refuses it: `m2` waits up to 30 s, then exits 3, changing nothing. Don't write the song's files directly while the app has it open: the app can't refuse that, and it ends a drag on what it changes. With the app closed, `m2` writes the files itself, each write an undo step of the song's history, yours by `--as`, which the app reads back when it opens the song (`m2 undo` needs the app open). Reading, rendering and `bounce` without `--place` work all the same. What is the musician's own (play, stop, seek, the loop, record, mute and solo while listening, the view, plugin windows, redo, exports, settings, opening and closing songs) no agent can do: `m2 call` refuses it, and `m2 point` shows them where instead.

std, the templates and the examples are found at `$M2_STD`, `$M2_TEMPLATES` and `$M2_EXAMPLES`, in the app or package `m2` came in (`music2.app/Contents/Resources`, or `lib/music2` beside the `bin` that holds `m2` on Linux, as in `/usr/lib/music2`), or else in the source tree it was built from.

`m2` alone, `m2 help` or `m2 --help` prints this, and `m2 --version` (or `-V`) which `m2` this is. `m2 <command> --help` prints that command's part. A command `m2` doesn't know is refused, with exit status 2. So is a word a command doesn't take, before anything is read or written, and `m2` says which: a flag it hasn't got, a flag without its value, a word too many or too few. A word that starts with `-` is a flag, unless it is a number (`-6`), has a space in it, or is `-` alone: standard input, where the usage says `| -`. A flag's value is the next word, unless that is a flag; a `<text>` is the next word whatever it is. Flags can go anywhere after the command, but `--as` and `--why` go last. `m2 docs` prints the guide, built into this `m2`.

## m2 new

```
m2 new <folder> [--template ID | --example ID]
```

Make a new song in exactly `<folder>` from a template (`starter`, the default, or `empty`) or a copy of an example (`--example first-song`). A folder that is there, in any capitals, is refused. The song is named after its folder. It gets `AGENTS.md`, for agents working on it, which names this `m2` by its full path, and `CLAUDE.md`, which imports it for Claude Code. Try the commands below on such a copy, never on the examples themselves.

## m2 check

```
m2 check <project> [--json]
```

Run the TypeScript, check the graph and the data, and print a summary; `--json` prints the compiled graph instead. `m2 build` is the same. A problem is printed as the file, line and column, then what is wrong; one in the sound graph also names the module's key and the node in square brackets ([How a song is written](code.md#when-somethings-wrong)).

## m2 render

```
m2 render <project> <out.wav> [--from S | --from-bar N] [--seconds N] [--preroll P] [--threads N]
```

Render to a 24-bit WAV, as the app's Export Audio does: to the last clip's end and on until it falls silent (10 s more at most), at the project's `sample_rate`. `--from` starts S seconds in, or `--from-bar` at bar N (counted from 1), replaying the song up to there; `--seconds` renders exactly N seconds. `--preroll` starts only P seconds earlier instead of replaying: fast for listening late in a long song, but approximate (notes held from before then, and the tails of what played earlier, are missing), so it is never written into `assets/`. `--threads` renders on N threads (1 to 64, or `M2_THREADS`), the same file on any number. It is the same file the app's Export (Song) writes, which falls silent when the sound has been silent for half a second. 3 hours at most. If a part breaks while it renders, it fails and names it.

## m2 listen

```
m2 listen <project> [--bars 9-16] [--tracks kick,bass] [--as-heard] [--with <tx.json | ->] [--png out.png] [--json]
```

Measure the song as an agent can't hear it: loudness (LUFS), true peak, crest, ten octave bands, stereo and masking, for the mix and each track, its own sound and as it reaches the mix. The bars count from 1 (else the loop when one is on, else the song), with 2 bars of preroll; `--tracks` measures those alone; `--as-heard` with the musician's listening mute and solo; `--with` a Tx (from a file, or standard input) checked and never written, before and after; `--png` draws the bands over time. Its own process, at background priority, one at a time; it writes nothing into the song. It works with the app open or closed. [Listening](listening.md) says what each number means.

## m2 kernels

```
m2 kernels [--json]
```

List every kernel and its typed ports: std's, and those of the packs music2 comes with.

## m2 bounce

```
m2 bounce <project> <asset-name> [--track KEY] [--export out] [--from S] [--seconds N] [--place track.lane@beat] [--threads N]
```

Render a track, named by its key (or the whole song), to a new `assets/<asset-name>.wav` (never over one that exists), optionally placing it as a clip on an audio lane (both by key). `--export` names which of the track's outputs to render (`out` unless you say). `--place keys.lane@beat` puts the new file on an audio lane, as one edit.

## m2 graph

```
m2 graph <project> [--json]
```

The graph the canvas shows: calls, their knobs and settings, connections. A card whose sound puts its values in groups prints each group as a heading over its rows, with More last; its views (an envelope, a filter's curve, a key map, a scope) are rows too.

## m2 controls

```
m2 controls <project>
```

Every number written in the project that the graph can edit, with its id, and every declared control a call leaves out (inherited: its default). An id names a file, a call in it and an argument: `song.ts#2/0.hats.pan` is, in `song.ts`, call number 2 (the mixer), its first argument (`0`), and `hats.pan` inside that. A control the call leaves out is named by its call and `@` with the control's name: `tracks/hats.ts#1/@decay`.

## m2 set

```
m2 set <project> <control> <value> [--expect V]
```

Change one, verified by the compiler (values in the control's units); `--expect` refuses unless it is V now. A knob that is a menu or a switch takes a word: one of its options' names (`square`), or true or false (On, Off). A setting is set the same way, by its id (a card's `settings` in `m2 graph <project> --json`, such as `tracks/bass.ts#3/@shape`): a number, true or false, or a word such as `square`.

## m2 automate

```
m2 automate <project> <control>
```

Make a control's value the fallback of a new, empty control lane, read where the value was, verified to sound the same. It is what the app's Automate does: the knob gets an automation lane of its own.

## m2 label

```
m2 label <project> <track> <name>
```

Show a track, named by its key, as `<name>`: its `track()` call's name, verified to change nothing that plays (its key as `<name>` takes the name out).

## m2 add-track

```
m2 add-track <project> <instrument> [--name N] [--key ID] [--after ID]
```

Add a track playing an instrument (`@m2/synths:synth-pluck`, or `synth.pluck`): its file in `tracks/`, its import and mixer channel in `song.ts`, an empty notes lane, and its place and colour in `layout.json`, verified. It is what + Track does. `--name "Lead Vox"` names it, `--key` gives its key, `--after kick` puts it below that track.

## m2 add-kit

```
m2 add-kit <project> <kit> [--after ID]
```

Add a drum kit by its name (`808 kit`, `909 kit`, `Basic kit`): a Drums bus, and the kit's tracks, each a Drum playing its drum from an empty lane of its own and going to the bus; one edit that undo takes back, verified. It is what + Track ▸ Drum kit does. `m2 add-kit . "808 kit" --after keys` puts the kit below that track. The kits are the packs' own: `@m2/drums` names three. Mix each drum on its own track, and the whole kit on the Drums bus. [Drum kits](sounds.md#drum-kits) says more.

## m2 import

```
m2 import <project> <file.wav|file.flac>… [--track ID] [--at BAR]
```

Bring WAV or FLAC files into `assets/` (a copy, or the same bytes already there; a taken name numbered; a FLAC as the WAV of its samples, with a note of what it was made from) and place their clips from bar BAR (1 if not given): on a track's audio lane one after another, or without `--track` each on a new audio track named after it, at the end; one edit that undo takes back. With the song open in the app, the app brings them in, as one undo step, and nothing is copied while the musician holds the song (a take, V's Compare): it waits, as every write does.

## m2 add-sample

```
m2 add-sample <project> <sample id> [--track ID] [--at BAR]
```

Add a sample from the library (its id, from `m2 search --kind sample`): its file copied into `assets/` with a note beside it of where it came from (a pack's name, version, author and licence; or that it is from your own folder), and its clip placed from bar BAR (1 if not given), on a track's audio lane or on a new audio track; a loop at another tempo follows the song's. One edit that undo takes back. With the song open in the app, the app adds it, as one undo step. The note is `assets/<file>.json`; the file and its note stay when the clip is undone. [Samples](samples.md) says more.

## m2 duplicate-track

```
m2 duplicate-track <project> <track>
```

Copy a track, named by its key: its code, its clips (with new ids), its plugin states and its mixer channel, verified.

## m2 delete-track

```
m2 delete-track <project> <track> [--cascade]
```

Take a track out: its file, its imports, its channel, its data and its layout; `--cascade` also takes it out of effects that read it (a duck's sidechain), which are otherwise listed and nothing is changed.

## m2 layout

```
m2 layout <project> order <IDs…>
m2 layout <project> colour|color <ID> <colour>
m2 layout <project> reset
```

Reorder or recolour tracks in `layout.json` (no revision), or write the default layout over one that doesn't read; the colours are rose, teal, amber, indigo, green, plum, olive, blue, coral, cyan, violet and lime. No revision: the layout never changes the sound.

## m2 tempo

```
m2 tempo <project> <bpm> [--at <bar>]
```

Set the song's tempo (20 to 999 BPM) in `data/arrangement.json`, or with `--at` a tempo change from bar N (counted from 1), verified.

## m2 meter

```
m2 meter <project> <n/d> [--at <bar>]
```

Set the song's time signature (`7/8`: 1 to 64 beats of a 1, 2, 4, 8, 16 or 32 note), or with `--at` a change from bar N, verified.

## m2 title

```
m2 title <project> <name>
```

Call the song `<name>`: its name in `music2.toml` (the folder keeps its own).

## m2 cables

```
m2 cables <project>
```

Named signals a cable can start from, and the inputs cables reach.

## m2 connect

```
m2 connect <project> <signal> <control>
```

Write a signal's name in place of a control's value, verified.

## m2 disconnect

```
m2 disconnect <project> <input>
```

Write the input's declared default in place of the name, verified.

## m2 effects

```
m2 effects <project>
```

The effects packages offer, by id (`@m2/effects:delay`), and the calls they can go after.

## m2 add-effect

```
m2 add-effect <project> <call> <effect>
```

Wrap a call's audio in an effect, named by its component id (`@m2/effects:delay`) or its export (`@m2/effects.delay`), verified.

## m2 remove-effect

```
m2 remove-effect <project> <call>
```

Take an effect out, leaving what it was applied to, verified.

## m2 relink

```
m2 relink <project> <from> <to>
```

Move data no lane or plugin reads (left behind when a key was renamed in code) to where it is read now, verified: a lane file (`data/kick/hits.json data/kicks/hits.json`), a track's folder (`data/kick data/kicks`) or a plugin state.

## m2 delete-data

```
m2 delete-data <project> <path>
```

Delete data no lane or plugin reads: a lane file, a folder, a state.

## m2 fix

```
m2 fix <project> <path> <fix>
```

Apply a fix a problem offers (`assign-clip-ids`, `repin-asset`, …). `finish-tx` and `discard-tx` finish or put back an edit that was cut off (its path is `.music2/tx/<id>.json`). Every command prints the problems it finds with the commands that fix them.

## m2 upgrade

```
m2 upgrade <project> [--check]
```

Move a song made for an older music2 to this one in one step: its `@m2/core`, std and packs lines and its lock, its imports of what moved into the packs, the single drums as the Drum with their knobs converted, and renamed knobs, keeping its sound. One undo step, written only if all of it converts exactly and the song then builds; `--check` says what it would change, and writes nothing. It prints each file it changes, the lines before (`-`) and after (`+`). What it can't convert exactly it names by file and line, with the line to write, and then writes nothing (exit 1): change those by hand, then run it again. What it converts as well as it can (the simple kick and hat, other sounds than the Drum's) it names to check. It never changes your presets, which are outside the song: it names those of sounds that moved. A song made for this music2 is left as it is. While the app has the song open, the app runs it, as one undo step there. See [Opening a song made with an older music2](songs.md#opening-a-song-made-with-an-older-music2).

## m2 tx

```
m2 tx <project> <tx.json | -> [--check] [--seen HASH]
```

A verified edit as Tx JSON: `--check` prints its delta, the delta's hash and the diagnostics, writing nothing; without it, it is committed (with `--seen`, only while its delta has that hash) and the outcome printed as JSON; exit 1 when it isn't committed. [The local API](api.md#edits) says what a Tx holds.

## m2 plugins

```
m2 plugins [clap:<id>]
```

List installed CLAP plugins, or one plugin's ports.

## m2 pack

```
m2 pack new <folder> --name @owner/name
m2 pack import-sfz <file.sfz> <folder> --name @owner/name [--label L] [--category C] [--license L]
m2 pack check <folder>
m2 pack build <folder>
m2 pack install <folder>
m2 pack get <@owner/name>[@<version>]
m2 pack get [<project>]
m2 pack list
m2 pack remove @owner/name <version>
m2 pack lock <project>
m2 pack update <project> [@owner/name]
```

Make, check and install packs: instruments, effects, sounds and recordings in a folder. `new` makes one with an example instrument; `import-sfz` makes one from an SFZ instrument (its recordings, as an instrument written as data), saying what it couldn't keep; `check` says in words what the registry would refuse (exit 1); `build` writes its `m2pack.json`; `install` checks, builds and copies it into your store, read-only; `get` gets one from the library (the registry), the latest version unless one is named, or every pack a song's `music2.lock` names that isn't installed; `list` and `remove` show and take out what is installed; `lock` writes the song's `music2.lock` from its `music2.toml`, moving only what must move; `update` moves the song to the newest installed version each line allows (or that pack's only). Your store is `~/Music/music2/Packs`, one folder a version (`Packs/@alice/felt-piano/1.2.0/`). A version is never changed: installing one that is there with other files is refused, and installing it again when its copy was changed puts it back. A song keeps the version it was made with in `music2.lock`; adding a pack's instrument or effect in the app writes it. To move a song to another version, change its line in `music2.toml`, then run `m2 pack lock`; both lock and update work while the song doesn't build. `get` reads the registry `M2_REGISTRY` names, else `registry` in `~/.music2/settings.json`; each file is checked by its hash, and a pack is installed whole or not at all. See [Packs](packs.md), [Getting packs from the library](library.md) and [Making a pack](making-a-pack.md).

## m2 registry

```
m2 registry check <registry> [--base <registry>] [--author <github-login>]
m2 registry add <registry> <pack-folder> --source <url>
m2 registry pending <registry> --out <bucket> [--from <bucket>]
m2 registry build <registry> --sources <folder> --out <bucket> [--from <bucket>] [--made <time>] [--to-delete <file>]
```

The pack registry's robot, and listing your pack in it. `check` reads every pack file strictly and says what a change broke: a listed version changed or a take-down taken back (`--base`: the registry before), a file changed by someone not its scope's (`--author`); `add` checks your pack, writes its `m2pack.json` and lists its version in `packs/@owner/name.toml`; `pending` prints the versions the bucket hasn't got yet, each with its hash and where to get it; `build` refuses to change anything the bucket publishes, then builds each pending version on its own from `<sources>/<its hash>/`, writing its files, previews and facts into `--out` by hash, then the catalog. A version that fails is left out and stays pending (exit 1). The registry is a folder: one file a pack, `packs/@owner/name.toml`, `tags.txt`, the words a pack's tags may use, and `removed.toml`, the versions taken down, which only music2's people list. `--from` is the bucket as it is: what it has isn't built again, and what it publishes never changes: `https://…` (the bucket's own address, never a cache), a folder or `file:///…`. `--made` is the catalog's time, `2026-10-30T12:00:00Z` (now, when it is left out), so one registry and one time always make the same bytes. `--to-delete` writes the bucket paths of taken-down versions' files that no other version uses, one a line. See [Making a pack](making-a-pack.md#listing-it-in-the-registry).

## m2 plugin-state

```
m2 plugin-state <project> <track>/<key> clap:<id>
```

Save a plugin's default state to `data/<track>/<key>.state`, with the `.state.json` sidecar that says which plugin wrote it, as one edit (replacing a state that is there, which undo puts back). It saves the plugin's default settings, not the values your code gives it ([CLAP plugins](plugins.md)).

## m2 mute-in-song

```
m2 mute-in-song <project> <track>… on|off
```

Mute tracks, named by their keys, in the song (every render and export), or unmute them: their `track()` calls' `muted` option, as one edit that undo takes back.

## m2 clips

```
m2 clips <project> create <track>/<lane> --at <place> [--length B] [--beats]
m2 clips <project> move <clip>… [--by B] [--to <track>/<lane>] [--copy] [--beats]
m2 clips <project> resize <clip> --start <place> --length B [--beats]
m2 clips <project> split <clip>… --at <place> [--beats]
m2 clips <project> join <clip>… [--beats]
m2 clips <project> duplicate <clip>… [--grid B] [--beats]
m2 clips <project> quantize <clip>… --grid B [--ends] [--beats]
m2 clips <project> delete <clip>… [--beats]
m2 clips <project> json <gesture.json | ->
```

One clip gesture, as one edit that undo takes back. A clip is `track/lane:id` (`bass/notes:k3f9q2zt`), or `track/lane@place` for the clip that starts there. Places are bar.beat.sixteenth (`9.3.1`), or beats with `--beats`; `--by`, `--length` and `--grid` are beats. `json` takes any gesture, as the app sends it. Besides the gestures above, `json` takes the others `POST /api/graph/clips` takes: `paste` (with `into`, copied notes into a clip), `notes`, `points`, `audio`, `set`, `stamp` (a chord built on the song's key) and `groove` (swing, laid back, pushed or accents). `m2 call <song> /api/theory` lists the chords and grooves.

## m2 sections

```
m2 sections <project> duplicate <bars> [--to <bar>]
m2 sections <project> move <bars> --to <bar>
m2 sections <project> delete <bars>
m2 sections <project> insert-bars <bars>
m2 sections <project> delete-bars <bars>
m2 sections <project> json <gesture.json | ->
```

One gesture on whole bars of every track at once, as one edit that undo takes back. `<bars>` are bars counted from 1 (`9-16`, or `9` for one), or a marker's name (`Chorus`: to the next marker). `duplicate` copies them right after themselves (or to bar `--to`) and `move` moves them, each in place of what was there; `delete` empties them; `insert-bars` opens as many empty bars where they start, and `delete-bars` takes them out: everything after moves, markers and tempo changes too. A clip that crosses the bars' edge is split there. `json` also takes `paste`, with a copied section, as `POST /api/graph/sections` does. See [Sections](sections.md).

## m2 unautomate

```
m2 unautomate <project> <track>/<lane> [--at <place>] [--beats]
```

Take a knob's automation out: what the lane plays at the place (else its fallback) is written back where the lane is read, and the lane file goes, as one edit that undo takes back.

## m2 edit

```
m2 edit <project> <file> --old <text> --new <text> [--all]
```

Replace text found once in a file of the song (every time, with `--all`), as one edit: how an agent changes a file while the app has the song open. While the song's code doesn't build, it fixes a code file: written only if the song then builds. Found no times, or more than once without `--all`, it changes nothing and says how many times. If the file changed after it was read, it reads it again and tries once more.

## m2 write

```
m2 write <project> <file> [<from> | -]
```

Write a whole text file of the song (from a file, or standard input), as one edit; while the code doesn't build, as `m2 edit`. It writes text only: `m2 import` brings audio in, `m2 plugin-state` saves a plugin's state.

## m2 undo

```
m2 undo <project>
```

Take back your own newest change in the open app (by `--as`): never the musician's, nor another agent's. With the app closed there is nothing to undo: undo lives in the open app.

## m2 focus

```
m2 focus <project>
```

What the musician looks at in the open app, as JSON: the view, the selected track, clips and notes, the knob last touched, the bars shown, the code's file and line, where playback is. In full: the dock's view, the selected track by its key, the clips picked (`{track, lane, id}`) and a marquee's bars (`range`), the roll's selected notes by `pitch`, `start` and `length`, the knob last touched, the beats the timeline shows (`seen`), the code view's file and line; and from the app, `playing`, `beat`, `loop`, the files with unsaved code, what V compares (`comparing`) and the MIDI keyboard's target (`midi`). Fields the window hasn't said yet are null. It writes nothing.

## m2 watch

```
m2 watch <project> [--meters]
```

Follow the open app: each change, undo, toast, focus, pointer and transport change as one JSON object a line, `t` first, until the app quits (then `closed`); meters too with `--meters`. First it prints `{"t":"watch","session":…}`, then each event the window hears, one JSON object a line with `t` first and the song's `session`: a change landed, an undo or a redo, a toast, the focus, a pointer, the Compare pill ([the local API](api.md#events) lists them), and `transport` when it plays or stops, the loop changes or the playhead jumps (read every quarter second). The meters only with `--meters`. When the app quits it prints `{"t":"closed","why":"app"}` and exits 0; when the app opens another song or closes this one, `{"t":"closed","why":"song"}`, and it prints nothing of the other song. While whoever reads it doesn't, it keeps 1,024 lines and drops the rest, then says `{"t":"lagged","missed":n,"by":"m2"}`; when they go away (`m2 watch … | head -1`), it ends quietly. It writes nothing, and it never takes a message from the musician's window.

## m2 point

```
m2 point <project> <target> [<words>]
m2 point <project> --clear
```

Show a note in the open app at `track:bass`, `clip:bass/notes:<id>`, `lane:bass/cutoff`, `knob:<id>` or `bars:9-16` (by `--as`); it never moves the musician's view: they press play. `--clear` takes it away; so does their OK. `bars:9` is one bar. A pointer is how an agent says "listen to bars 9 to 16": the musician presses play. It never selects anything. A target the song hasn't got is refused, saying so, and so are bars past its end, saying how many bars it has.

## m2 routes

```
m2 routes [--json]
```

Every route of the app's API, from its one table: the ones for agents first, each with what it does, whether it writes and an example body; the musician's own (play, record, the view…) last. No app needs to run. `--json` prints the table: `[{method, path, limit, writes, agents, words, example}]`, `limit` being the largest body the route takes (2 MiB, more for imports and Tx). [API routes](routes.md) is the same table.

## m2 call

```
m2 call <project> <path> [<json> | -] [--method M]
```

Send one route's request to the open app, as an agent (by `--as`): song changes with no command of their own (an effect's bypass, markers, the song's key) and every read. The song's session is filled in; a GET's keys go in its query; it prints the answer and exits 1 when the app refuses. The musician's controls are refused before anything is sent. The song changes it reaches include a track's instrument or ID, and a bus. `--method` picks one where a path takes two (`/api/settings`). Refused before anything is sent, too: a body bigger than the route takes. A write the musician holds waits, as every `m2` write does (exit 3). On standard error it says the undo step a write made, or what an undo took back.

## m2 search

```
m2 search [<words…>] [--kind <kind>] [--json]
```

Find what can be added to a song: instruments, sounds (presets), effects, samples and plugins, best first, with the search ⌘F asks in the app. One line each: its name, its kind, what it is and where it is from, and its id, which adding it names. With the app open it asks the app, whose list has the open song's own instruments and the plugins it has listed; with the app closed it searches music2's sounds, the packs you installed, your presets and your sample folders itself. Every word must match. `--kind` is `instrument`, `sound`, `effect`, `sample` or `plugin`; the same words typed in the search filter too, and so do `loop`, `one-shot`, a tempo (`90 bpm`) and a key (`Am`, `F#`), which only samples carry (a tempo within 2 bpm, a key or its relative). `--json` prints each item with all it knows: its `kind`, `id`, `label`, `category`, `description`, `tags`, `package`, `component` (what `m2 add-effect` or a new track takes), and a sound's `preset`. After what you have come the library's items of packs you haven't got (when a registry is set), each marked `library`, its id `library:<id>`, with `library`: its pack, version, licence and the bytes `m2 pack get` fetches; the search runs on this computer and sends nothing. [Finding sounds](finding-sounds.md) says more.

## m2 docs

```
m2 docs [<page> | all]
```

The guide, built into this `m2`: its pages, or one page as Markdown, or all of them. It needs no song and no app. A link to another page, such as `window.md`, is `m2 docs window`. `m2 docs commands` and `m2 docs routes` are made from the code when asked. `m2 docs agents` is the `AGENTS.md` a new song gets from this `m2`, naming it: `m2 docs agents > AGENTS.md` gives a song made before them its own.

## Try them on a copy

Commands that change a song write into its folder, so try them on a copy, not on the examples:

```bash
m2 new /tmp/m2-scratch --example first-song                    # a copy of the example
m2 check /tmp/m2-scratch
m2 render /tmp/m2-scratch out.wav
m2 controls /tmp/m2-scratch                                     # ids look like song.ts#2/0.hats.pan
m2 set /tmp/m2-scratch 'song.ts#2/0.hats.pan' 0.45
m2 set /tmp/m2-scratch 'tracks/hats.ts#1/@decay' 0.08            # a control the call leaves out: writes { decay: 0.08 }
m2 add-effect /tmp/m2-scratch 'tracks/hats.ts#1' @m2/effects:delay

m2 new /tmp/m2-resample --example resample
m2 bounce /tmp/m2-resample keys-again --track keys              # render the keys track to assets/keys-again.wav
```
