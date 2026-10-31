<!-- Made by `m2 docs routes` from the app's route table (crates/server/src/routes*.rs): change it there, then run ci/guide.sh gen. -->
# API routes

**In short**
- Every route of the app's local API, from its one route table: the ones for agents first, then the musician's own.
- `m2 call <song> <path>` sends one as an agent. `m2 routes` prints this table in the terminal, `m2 routes --json` as JSON.
- A route that writes makes one undo step, the agent's, and waits while the musician holds what it changes.

How to reach the app, its keys and its answers: [The local API](api.md). The song's `session` is filled in by `m2 call` when the body leaves it out, and a GET's keys go in its query. A body may be up to 2 MiB, unless the route says more.

## For agents

### `GET /api/audio/inputs`

The sound inputs there are, the default first.

### `GET /api/focus`

What the musician looks at, as m2 focus prints it: the view, the selected track, clips and notes, the knob last touched, the bars shown, where playback is.

### `GET /api/graph`

The open song as the app shows it: its session, the graph view with its revision and every knob's id, the timeline, the Mix view, problems, undo's names and where playback is.

### `POST /api/graph/audio/import`

Bring audio files on this computer in, and place their clips: on a lane, on a new track after one, or on new tracks below. It writes the song: one undo step. Bodies up to 1024 MiB.

An example body:

```json
{"session": 7, "paths": ["/Users/you/Samples/vox.wav"], "to": {"kind": "below", "beat": 0}}
```

### `GET /api/graph/audio/peaks`

An audio file's waveform peaks, as bytes.

Its query, as JSON:

```json
{"session": 7, "asset": "vox.wav"}
```

### `POST /api/graph/audio/track`

Add an audio track. It writes the song: one undo step.

An example body:

```json
{"session": 7, "label": "Vox", "after": "keys"}
```

### `POST /api/graph/automate`

Automate a knob: a new, empty lane plays its value. It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "id": "tracks/keys.ts#1/@brightness"}
```

### `POST /api/graph/automate/remove`

Take a knob's automation out: the lane's value at the beat (else its fallback) goes back into the code. It writes the song: one undo step.

An example body:

```json
{"session": 7, "module": "bass", "lane": "cutoff", "beat": 16}
```

### `POST /api/graph/clips`

A clip gesture: create, move, resize, split, join, duplicate, paste, delete, quantize, notes, points, audio, set, stamp (a chord) or groove. It writes the song: one undo step.

An example body:

```json
{"session": 7, "gesture": {"op": "move", "clips": [{"module": "keys", "lane": "chords", "id": "k3f9q2zt"}], "by": 4}}
```

### `POST /api/graph/connect`

Connect a named signal to a knob's input: a cable, written in code. It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "from": "kick.env", "to": "song.ts#5/0"}
```

### `POST /api/graph/data/delete`

Delete data no lane or plugin reads. It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "file": "data/kick/old.json"}
```

### `POST /api/graph/disconnect`

Take a cable out: the input's default is written in its place. It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "id": "song.ts#5/0"}
```

### `POST /api/graph/effect/add`

Put an effect after a call's sound, by its component id as the state's effects list it. It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "call": "tracks/keys.ts#1", "component": "@m2/effects:delay"}
```

### `POST /api/graph/effect/bypass`

Turn an effect off (on: true bypasses it), or on again. It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "node": "tracks/keys.ts#1", "on": true}
```

### `POST /api/graph/effect/move`

Move an effect before the one written around it (earlier), or after the one around it. It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "node": "tracks/keys.ts#1", "earlier": true}
```

### `POST /api/graph/effect/remove`

Take an effect out, keeping what it was applied to. It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "call": "tracks/keys.ts#1"}
```

### `POST /api/graph/effect/sidechain`

Make a Duck listen to another track. It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "node": "song.ts#4", "track": "kick"}
```

### `PUT /api/graph/file`

Save a code file over the text it had, as the code editor does (an agent's lands only if the song then builds). It writes the song: one undo step. Bodies up to 144 MiB.

An example body:

```json
{"session": 7, "path": "tracks/keys.ts", "base": "the text it had", "text": "the new text"}
```

### `GET /api/graph/files`

The song's code files, as the code editor lists them.

### `POST /api/graph/fix`

Apply a fix a problem offers, as the state's problems list it, whole. It writes the song: one undo step. Bodies up to 144 MiB.

An example body:

```json
{"session": 7, "fix": {"code": "delete-data", "title": "Delete it", "likely": false, "tx": {"id": "fix-delete-old", "base": {"data/kick/old.json": "sha256:9b74c9897bac770ffc029102a200c5de7d2a5d3e5d4c0f2f8b1c4e3a7d6f5e4c"}, "ops": [{"op": "remove", "file": "data/kick/old.json"}], "expect": "identical"}}}
```

### `POST /api/graph/label`

Rename a track's label (its key stays). It writes the song: one undo step.

An example body:

```json
{"session": 7, "module": "keys", "label": "Piano"}
```

### `POST /api/graph/lane`

Write every clip of a notes lane (pitches as numbers or names, notes from their clip's start). It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "module": "keys", "lane": "chords", "clips": [{"id": "k3f9q2zt", "start": 0, "length": 4, "notes": [{"pitch": "C4", "start": 0, "length": 1}]}]}
```

### `POST /api/graph/layout`

Reorder the tracks, or recolour one, in layout.json. It writes the song: one undo step.

An example body:

```json
{"session": 7, "colour": {"module": "keys", "colour": "teal"}}
```

### `GET /api/graph/leases`

The drags in progress: knobs, clips and the tempo.

### `POST /api/graph/loudness`

Measure the song's loudness as it plays (LUFS, and its true peak), in the meters and each answer, while you ask again within 10 s; or stop.

An example body:

```json
{"session": 7, "client": "w1", "on": true}
```

### `POST /api/graph/markers`

Write the ruler's markers, the whole list, into layout.json. It writes the song: one undo step.

An example body:

```json
{"session": 7, "markers": [{"beat": 32, "name": "Chorus"}]}
```

### `POST /api/graph/meter`

Set the time signature at a beat, or add or remove a change there. It writes the song: one undo step.

An example body:

```json
{"session": 7, "at": 0, "beats": 7, "unit": 8}
```

### `POST /api/graph/midifile/export`

Write the song, or a clip, as a MIDI file: to the path given, else to a new file in Exports.

An example body:

```json
{"session": 7, "clip": {"module": "keys", "lane": "chords", "id": "k3f9q2zt"}}
```

### `POST /api/graph/midifile/import`

Bring a MIDI file on this computer in: into a lane, on new tracks after one, or on new tracks below (tempo: keep or use the file's). It writes the song: one undo step. Bodies up to 16.0 MiB.

An example body:

```json
{"session": 7, "path": "/Users/you/Downloads/riff.mid", "to": {"kind": "below", "beat": 0}, "tempo": "keep"}
```

### `POST /api/graph/midifile/where`

Where a MIDI file of the song, or of a clip, would go: a new name in the songs folder's Exports.

An example body:

```json
{"session": 7}
```

### `POST /api/graph/mix/bus`

Add a bus: reverb, delay or group. It writes the song: one undo step.

An example body:

```json
{"session": 7, "kind": "reverb", "label": "Room"}
```

### `POST /api/graph/mix/insert`

Write a mixer channel's first pan into its entry. It writes the song: one undo step.

An example body:

```json
{"session": 7, "channel": "keys", "control": "pan", "value": -0.2}
```

### `POST /api/graph/mix/route`

Send a channel to a group bus, or back to the master (to: null). It writes the song: one undo step.

An example body:

```json
{"session": 7, "channel": "keys", "to": "group"}
```

### `POST /api/graph/mix/send`

Add or remove a send from a channel to a bus, or put it before or after the fader (pre). It writes the song: one undo step.

An example body:

```json
{"session": 7, "channel": "keys", "bus": "reverb", "action": "add"}
```

### `POST /api/graph/preset/load`

Load a preset into a sound: std, yours or the plugin's. It writes the song: one undo step.

An example body:

```json
{"session": 7, "at": 42, "node": "tracks/keys.ts#1", "from": "std", "name": "Glass"}
```

### `GET /api/graph/presets`

A sound's presets: std's, yours and the plugin's own, by name.

Its query, as JSON:

```json
{"session": 7, "node": "tracks/keys.ts#1"}
```

### `POST /api/graph/relink`

Move data no lane or plugin reads to where it is read now: a lane file, a track's folder or a plugin state. It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "from": "data/kick/hits.json", "to": "data/kicks/hits.json"}
```

### `POST /api/graph/sample`

Give a recording on a card (a Sampler's file, or `setting` of the call) a WAV file on this computer. It writes the song: one undo step. Bodies up to 1024 MiB.

An example body:

```json
{"session": 7, "at": 42, "call": "tracks/drums.ts#2", "file": "/Users/you/Samples/kick.wav"}
```

### `POST /api/graph/scope`

Show a box's sound as it plays: its last frames, to the window that asks (and in each answer), while you ask again within 10 s; or stop.

An example body:

```json
{"session": 7, "client": "w1", "module": "bass", "node": "tracks/bass.ts#3", "on": true}
```

### `POST /api/graph/sections`

A section gesture, on whole bars of every track: paste, duplicate, move, delete, insert-bars or delete-bars. It writes the song: one undo step.

An example body:

```json
{"session": 7, "gesture": {"op": "duplicate", "start": 32, "end": 64}}
```

### `POST /api/graph/sections/copy`

Copy whole bars of every track: each lane's kind and the clips that overlap them, for a section paste.

An example body:

```json
{"session": 7, "start": 32, "end": 64}
```

### `POST /api/graph/set`

Write a knob's value, or a setting's (a number, one of its options, true or false, a file in assets/, a track), into the code, as one undo step (for an agent, never a drag's commit). It writes the song: one undo step.

An example body:

```json
{"session": 7, "revision": 42, "id": "tracks/bass.ts#3/@shape", "gesture": "set", "value": "square"}
```

### `POST /api/graph/song-key`

Set the song's key, or take it away (null). It writes the song: one undo step.

An example body:

```json
{"session": 7, "key": {"root": "A", "scale": "minor"}}
```

### `GET /api/graph/status`

Where playback is: playing or not, the beat, the tempo there and the loop, with the open song's session.

### `POST /api/graph/stems/check`

Whether the song's stems can be exported now, and why not.

An example body:

```json
{"session": 7}
```

### `POST /api/graph/take`

Choose which take an audio clip over a loop plays (from 1). It writes the song: one undo step.

An example body:

```json
{"session": 7, "module": "vox", "lane": "clips", "id": "k3f9q2zt", "take": 2}
```

### `POST /api/graph/tempo`

Set the tempo at a beat, or add or remove a tempo change there. It writes the song: one undo step.

An example body:

```json
{"session": 7, "at": 0, "bpm": 96}
```

### `POST /api/graph/title`

Name the song: its name in music2.toml. It writes the song: one undo step.

An example body:

```json
{"session": 7, "name": "Night drive"}
```

### `POST /api/graph/track/add`

Add a track playing an instrument, by its component id as the state's instruments list it. It writes the song: one undo step.

An example body:

```json
{"session": 7, "component": "@m2/synths:synth-pluck", "label": "Pluck", "after": "keys"}
```

### `POST /api/graph/track/delete`

Delete a track (with cascade, out of the effects that read it too). It writes the song: one undo step.

An example body:

```json
{"session": 7, "module": "keys", "cascade": false}
```

### `POST /api/graph/track/duplicate`

Copy a track: its code, its clips with new ids, its plugin states and its channel. It writes the song: one undo step.

An example body:

```json
{"session": 7, "module": "keys"}
```

### `POST /api/graph/track/instrument`

Change a track's instrument, by its component id as the state's instruments list it. It writes the song: one undo step.

An example body:

```json
{"session": 7, "module": "keys", "component": "@m2/synths:synth-pad"}
```

### `POST /api/graph/track/mute`

Mute tracks in the song, written in their code, or unmute them. It writes the song: one undo step.

An example body:

```json
{"session": 7, "modules": ["keys"], "muted": true}
```

### `POST /api/graph/track/rekey`

Change a track's ID (its key). It writes the song: one undo step.

An example body:

```json
{"session": 7, "module": "keys", "key": "piano"}
```

### `POST /api/graph/track/rekey/check`

Whether a track's ID can be changed to this one.

An example body:

```json
{"session": 7, "module": "keys", "key": "piano"}
```

### `GET /api/graph/types`

The type declarations the code editor checks with.

### `POST /api/graph/undo`

Take back the newest change (an agent's only when it is its own). It writes the song: one undo step.

An example body:

```json
{"session": 7}
```

### `GET /api/library`

Search everything you can add: instruments, sounds (presets), effects, samples and plugins, best first; q's words may name a kind, loop, one-shot, a tempo or a key; fits (a beat) keeps the loops that fit the song there.

Its query, as JSON:

```json
{"q": "warm pad"}
```

### `GET /api/library/folders`

Your sample folders, each with how many samples it has and whether it is being indexed.

### `POST /api/library/get`

Get a pack or a sample from the library (the registry): a pack's whole version installed, checked, or one sample added to the song with its note; or a version the open song locks and is missing. Answers once it is done; `get` events say how far. It writes the song: one undo step.

An example body:

```json
{"id": "library:@alice/felt-piano:felt-piano"}
```

### `POST /api/library/get/cancel`

Stop the Get running: nothing of it is kept.

### `GET /api/library/missing`

The packs the open song's music2.lock names that aren't installed, each with whether the library has that very version to get.

### `GET /api/library/preview`

An item's preview: a short WAV of it playing, rendered offline and kept; a sample's own file (a FLAC's as the WAV of it).

Its query, as JSON:

```json
{"id": "@m2/synths:synth-pluck"}
```

### `POST /api/library/sample`

Add a sample from the library to the song: copied into assets/ with a note of where it came from, then a clip at `to` (a loop follows the song's tempo), or the recording of call `call` (a Sampler's file, or its `setting`). It writes the song: one undo step.

An example body:

```json
{"session": 7, "id": "sample:std:samples/kit/kick.wav", "to": {"kind": "below", "beat": 0}}
```

### `POST /api/library/store-changed`

Tell the app packs were installed or removed outside it (m2 pack get does): the open song's lists are made again, and a song that waited for a pack builds.

### `GET /api/packs`

The packs installed on this computer: each one's name, versions (newest last), licence, description and items.

### `GET /api/plugins`

The installed CLAP plugins: id, name, vendor, version, and whether each is an instrument or an effect.

### `POST /api/point`

Point at a track, a clip, a lane, a knob or bars, with words, as m2 point does; or clear the pointer.

An example body:

```json
{"session": 7, "target": "bars:5-8", "words": "listen here"}
```

### `GET /api/project/recent`

The songs opened lately, newest first.

### `GET /api/project/save-as`

Where Save As starts: the folder the song is in, and the first free name.

### `GET /api/project/templates`

The templates and examples a new song can start from, and the songs folder.

### `GET /api/settings`

This computer's settings: the sound output, where new songs go, Record's options, the MIDI keyboards and the sound input.

### `GET /api/theory`

The scales a key can have, the chords a stamp places and the grooves, each with its name and what it does.

### `POST /api/tx/check`

Check a Tx: its delta, the delta's hash and the diagnostics; nothing is written. Bodies up to 144 MiB.

An example body:

```json
{"session": 7, "tx": {"id": "tx-delete-old", "base": {"data/kick/old.json": "sha256:9b74c9897bac770ffc029102a200c5de7d2a5d3e5d4c0f2f8b1c4e3a7d6f5e4c"}, "ops": [{"op": "remove", "file": "data/kick/old.json"}], "expect": "identical"}}
```

### `POST /api/tx/commit`

Commit a Tx, as one undo step: m2 sends its writes here. It writes the song: one undo step. Bodies up to 144 MiB.

An example body:

```json
{"session": 7, "tx": {"id": "tx-delete-old", "base": {"data/kick/old.json": "sha256:9b74c9897bac770ffc029102a200c5de7d2a5d3e5d4c0f2f8b1c4e3a7d6f5e4c"}, "ops": [{"op": "remove", "file": "data/kick/old.json"}], "expect": "identical"}}
```

### `GET /api/ws`

Hear what happens in the app as it happens: a WebSocket of events, one JSON object each, as m2 watch prints them.

## The musician's own

The tools key gets 403 for these (AGENT-11): ask the musician, or show them where with `m2 point`.

### `POST /api/focus`

Say what the musician looks at (the page posts it).

### `POST /api/graph/arm`

Arm a track to record, or disarm it, and choose the input channels it records.

### `POST /api/graph/audition`

Play or let go of notes on a lane, as the piano roll and the keyboard do.

### `POST /api/graph/close`

Close the open song.

### `POST /api/graph/compare`

Hear the newest change made elsewhere Before or After it (V), or close the pill.

### `POST /api/graph/draft`

Keep a window's text of a code file not yet saved, so a crash doesn't lose it (text null takes it out).

### `POST /api/graph/draft/left`

Say that unsaved code the song opened with was restored or discarded, so it isn't offered again.

### `POST /api/graph/export`

Export the song's audio, which may pause playback (agents render with m2 render).

### `POST /api/graph/input/inject`

What the sound input would hear next (only with M2_AUDIO_INPUT=inject, for the browser suites).

### `POST /api/graph/lane/lease/begin`

Begin a drag on clips, heard as it goes.

### `POST /api/graph/lane/lease/cancel`

End a clip drag without saving.

### `POST /api/graph/lane/lease/commit`

Save a clip drag, as one undo step.

### `POST /api/graph/lane/lease/keepalive`

Keep a clip drag going while it is held still.

### `POST /api/graph/lane/lease/update`

Hear a clip drag's newest gesture.

### `POST /api/graph/lease/begin`

Begin a knob drag.

### `POST /api/graph/lease/cancel`

End a knob drag without saving.

### `POST /api/graph/lease/commit`

Save a knob drag's value, as one undo step.

### `POST /api/graph/lease/keepalive`

Keep a knob drag going while the knob is held still.

### `POST /api/graph/lease/update`

Hear a knob drag's newest value.

### `POST /api/graph/lock/retry`

Try the song's lock again, in a window open read-only.

### `POST /api/graph/loop`

Set the loop's beats, or turn it on or off.

### `POST /api/graph/midi/inject`

What a MIDI keyboard would send (only with M2_MIDI=inject, for the browser suites).

### `POST /api/graph/open`

Open a song.

### `POST /api/graph/overlay`

Mute or solo tracks while listening, which never changes the song.

### `POST /api/graph/panic`

Stop all sound at once.

### `POST /api/graph/plugin/latch`

Let a lane or a cable drive a plugin's parameter again.

### `POST /api/graph/plugin/window`

Open or close a plugin's own window.

### `POST /api/graph/preset/save`

Save a sound's settings as one of the musician's own presets.

### `POST /api/graph/preview`

Hear a knob's new value while it is dragged (the first one begins the drag).

### `POST /api/graph/preview/cancel`

End a knob drag without saving, so the code's value plays again.

### `POST /api/graph/record/cancel`

Drop the take, or stop counting in.

### `POST /api/graph/record/start`

Record a take on the armed tracks, or on a notes lane or an audio lane; punch on the loop, overdub notes.

### `POST /api/graph/record/stop`

End the take where it is heard; play goes on.

### `POST /api/graph/redo`

Put back the change undone last.

### `POST /api/graph/seek`

Move the playhead to a beat.

### `POST /api/graph/stems`

Export the song's stems, which may pause playback (agents render with m2 bounce).

### `POST /api/graph/step`

Name the undo step the window's own edit made.

### `POST /api/graph/tempo/lease/begin`

Begin a drag of the tempo field, heard as it goes.

### `POST /api/graph/tempo/lease/cancel`

End a tempo drag without saving.

### `POST /api/graph/tempo/lease/commit`

Save a tempo drag, as one undo step.

### `POST /api/graph/tempo/lease/keepalive`

Keep a tempo drag going while it is held still.

### `POST /api/graph/tempo/lease/update`

Hear a tempo drag's newest value.

### `POST /api/graph/transport`

Play, stop, toggle or restart.

### `POST /api/graph/undo/skip`

Skip the newest step, which undo can't take back.

### `POST /api/graph/unsaved`

Say which files have changes not yet saved in the code editor.

### `POST /api/graph/view`

Set the timeline's view (zoom, follow, track heights, hidden lanes, the grid, audition, the metronome and the guide).

### `POST /api/library/folders`

Add a folder of samples to the library, or remove one.

### `POST /api/plugins/scan`

Look for installed plugins again (Look again), which loads each one in the host where the song's plugins play.

### `POST /api/project/new`

Make a new song from a template or an example, and open it.

### `POST /api/project/recent/forget`

Take a song off the recent list.

### `POST /api/project/rename`

Rename the song, and its folder too with folder.

### `POST /api/project/save-as`

Save a copy of the song, and open the copy.

### `POST /api/settings`

Change this computer's settings.
