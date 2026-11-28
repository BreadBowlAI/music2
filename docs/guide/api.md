# The local API

**In short**
- While the app runs, programs on your computer can talk to it over HTTP, with the tools key from `~/.music2/session.json`.
- A request with that key is an agent's: its writes are one undo step each, and the musician's own controls answer 403.
- `m2 call` sends any route; `/api/ws` tells what happens, as JSON events.

## The tools key

While the app runs, it writes `~/.music2/session.json` (only you can read it). It holds the app's local URL, its process id and the **tools key**, which changes at every launch. Every `/api` call needs that key in an `x-m2-token` header (or `?token=`). A request with it is an agent's: its undo steps are an agent's, named by the `as` it gives (in a Tx's body, or the `x-m2-as` header; else `agent`) and its `why` (or `x-m2-why`; a header's words are UTF-8, percent-encoded, as `m2 call` sends them), as plain text of at most 80 characters, and shown to the musician as they land. The app's own window has a key of its own, which no file holds, so nothing else can pass as the musician.

## Busy: 423

An agent's write is refused with 423 and `{"busy": {what, file, words, taken_back}}` while the musician holds what it touches: a file with a knob they drag (`knob`), a lane they drag a clip on (`lane`), the arrangement while they drag the tempo (`tempo`), a plugin's state while its window has changes not yet saved (`plugin`), a file with unsaved text in the code editor (`code`), or anything while a take records (`take`) or V's Compare pill is open (`compare`, "you're comparing Claude: brighter hats"). It is checked when the write arrives and again just before it is heard; `taken_back` says the musician began holding it as it landed, or that it would have ended what they hold (a drag its change can't carry, such as a knob in a track the change takes out, or a plugin window not yet saved whose plugin it takes out), whatever file it touched, and it was taken back. Nothing of an agent's write is heard before then, and nothing is shown of one taken back.

## The musician's own: 403

The musician's own controls answer the tools key 403, with plain words: play, stop, seek, the loop and Stop All Sound; recording; muting and soloing while listening; the view; plugin windows; Look again for plugins (it loads each one where the song's plugins play); auditions; drags (agents don't drag: an agent's `/api/graph/set` is one edit, busy while a drag lives); redo and skipping a step; naming steps; the code editor's list of unsaved files and its drafts, the text it keeps against a crash (`/api/graph/draft`, `/api/graph/draft/left`); the library's sample folders (`POST /api/library/folders`); **Retry** on a song open read-only (`/api/graph/lock/retry`); posting the focus record and Compare; Export Audio and stems, which may pause playback (an agent renders with `m2 render` or `m2 bounce`); saving a preset into the musician's own; settings; and opening, closing, making, saving as or renaming songs. What a WebSocket opened with the tools key sends is ignored. Every read stays open to agents but the drafts' text, which `GET /api/graph/files` leaves out for them, and so does every change to the song the app's window can make.

What the musician looks at and what an agent points at are attention, not song: kept in memory only, never in a file or an undo step, and gone when the song closes. Neither is a write, so both work read-only.

```bash
S=~/.music2/session.json; URL=$(jq -r .url $S); H="x-m2-token: $(jq -r .token $S)"
curl -s -H "$H" $URL/api/graph | jq '{name, session, revision: .view.revision}'
body=$(curl -s -H "$H" $URL/api/graph | jq '{session, revision: .view.revision, id: "song.ts#2/0.hats.pan", gesture: "script", value: 0.45}')
curl -s -H "$H" -H 'content-type: application/json' $URL/api/graph/set -d "$body"
```

`m2 call` sends any route for agents, named by the song's folder, and fills in its session:

```bash
m2 call ~/Music/music2/song /api/graph | jq .view.revision          # say it is 12
m2 call ~/Music/music2/song /api/graph/effect/bypass '{"revision": 12, "node": "tracks/keys.ts#1", "on": true}' --as Claude --why "dry keys"
```

## Edits

Edits name the open project's `session` and the `revision` they were planned against, so a stale edit is refused rather than landing on the wrong thing. The refusal says who changed it since, when it knows: "the agent Claude changed it", "it was changed outside the app", or "it was changed in the app". While the song is open read-only (another program holds it), every route that writes answers 423 with the reason, and changes nothing.

Every change answers, at its top:

| Field | Meaning |
|---|---|
| `revision` | The revision it made. Absent when it made none: a change of `layout.json` alone (the track order, colours, markers) never does. |
| `step` | Its undo step's id: what `m2 undo` and the Edit menu name it by. |
| `moved` | The files whose ids it moved: every code file it changed, but for a value written where it is (a knob, a menu, a switch), and none for a change of data alone (clips, notes). An undo or a redo moves what its change moved. An id you read before it, in one of these files, may name something else now: read the song again before the next edit there. |
| `beats` | Only when true: it moved where music sits in time (bars inserted or deleted, a tempo or a meter change), so a beat you read before it may be other music now. An undo or a redo says it as its change did. |
| `mixed` | Only when true: the revision also took in a file saved outside the app (by you, say) while the change, an undo or a redo was made. |

A clip gesture (`/api/graph/clips`), bars (`/api/graph/sections`) and a take picked (`/api/graph/take`) may say the `revision` they were planned on: then they are refused, saying who, when what they name changed since (a clip, the bars, the Beat grid's lane), or when a change since moved beats. Markers (`/api/graph/markers`) and a track order (`/api/graph/layout`) may say the `base` list they were made from: then they are refused when that list changed since, so nothing is lost.

An agent can also send any edit as a **Tx**: the files it reads, each with the hash (or the text) it was planned on, the changes, and what they may do to the song. The app checks it as it checks its own edits, writes it as one undo step, and refuses it if a file changed under it or the song would change in any other way ([WRITE-3](https://github.com/BreadBowlAI/music2/blob/main/docs/architecture.md#the-two-write-primitives)). For example, deleting a data file no lane reads:

```bash
sha=$(shasum -a 256 data/kick/old.json | cut -d' ' -f1)       # in the song's folder
tx='{"id": "tx-delete-old", "base": {"data/kick/old.json": "sha256:'$sha'"},
     "ops": [{"op": "remove", "file": "data/kick/old.json"}], "expect": "identical"}'
curl -s -H "$H" -H 'content-type: application/json' $URL/api/tx/check -d "{\"tx\": $tx}" | jq .checked.hash
curl -s -H "$H" -H 'content-type: application/json' $URL/api/tx/commit -d "{\"tx\": $tx}"
```

`m2 tx <song> <file>` sends a Tx too (`-` for standard input), with the app open or closed, and `--check` checks it and writes nothing: try one there first. A Tx is an object of these fields:

| Field | What it holds |
|---|---|
| `id` | a name for it: 1 to 32 of `a-z`, `0-9` and `-` |
| `base` | every file an op touches, with what it held when you planned the change: `"sha256:<hex>"` (the SHA-256 of its bytes), `{"sha", "text"}` (its text too), or `null` for a file the Tx makes |
| `ops` | the changes, in order (below) |
| `expect` | what they may change in the song (below). A Tx that would change anything else is refused |
| `label` | if you like, `["<before>", "<after>"]`: how the step reads |

| Op | What it does |
|---|---|
| `{"op": "write", "file", "text"}` | writes a whole file (`base64` instead of `text` for bytes) |
| `{"op": "text_edit", "file", "edits": [{"start", "end", "expected", "text"}]}` | replaces parts of a file: from `start` to `end`, each a byte offset or `{"line", "utf16_col"}` counted from 0, where the file says `expected` now |
| `{"op": "remove", "file"}` | deletes a file |
| `{"op": "rename", "from", "to"}` | renames a file, or a track's data folder `data/<track>/`; `to`'s base is `null` |
| `{"op": "clips", "lane": ["<track>", "<lane>"], "put": [<clip>…], "delete": ["<id>"…]}` | puts whole clips on a lane, each replacing the clip with its id or added, and deletes clips by id: at most one for a lane. A clip is written as its lane file writes it ([Notes](code.md#notes)) |

| `expect` | The Tx may change |
|---|---|
| `"identical"` | nothing that plays: a comment, the layout, a file no lane reads |
| `{"values": [[<anchor>, <number>]…]}` | only these numbers in the code, to these values: what turning knobs makes. An anchor is `{"written": {"file", "range": [<start>, <end>]}}`, a number written in a file, by where it is; or `{"slot": {"site": {"file", "range"}, "slot": "<control>"}}`, a control a call leaves out, by where its call is |
| `{"confined": {"ranges", "lanes", "modules"}}` | only what is written inside `ranges` (`{"<file>": [[<start>, <end>]…]}`, or `"whole"` for a whole file), the notes, points and clips of `lanes` (`[["<track>", "<lane>"]…]`), and the tracks in `modules`: `{"added": "<key>"}`, `{"removed": "<key>"}`, `{"rekeyed": ["<from>", "<to>"]}` or `{"muted": ["<key>", true]}`. Each of the three may be left out |
| `{"seen": "sha256:…"}` | exactly what its check showed: the hash `--check` printed (`m2 tx --seen <hash>` adds it) |
| `{"journal": "<id>"}` | only what the `m2 fix` commands send, to finish or put back an edit cut off |

Two files changed as one undo step: a new bar 9 for the keys and the bass, in a copy of the `first-song` example ([Try it on a copy](changes.md#try-it-on-a-copy)). Each `base` is `"sha256:"` and what `shasum -a 256 data/keys/chords.json` prints, in the song's folder:

```json
{"id": "tx-bar-9",
 "base": {"data/keys/chords.json": "sha256:641f…", "data/bass/notes.json": "sha256:1cca…"},
 "ops": [{"op": "clips", "lane": ["keys", "chords"], "delete": [],
          "put": [{"id": "bar9keys", "start": 32, "length": 4, "notes": [
                    {"pitch": "E3", "start": 0, "length": 4}, {"pitch": "G#3", "start": 0, "length": 4}, {"pitch": "B3", "start": 0, "length": 4}]}]},
         {"op": "clips", "lane": ["bass", "notes"], "delete": [],
          "put": [{"id": "bar9bass", "start": 32, "length": 4, "notes": [{"pitch": "E1", "start": 0, "length": 4}]}]}],
 "expect": {"confined": {"lanes": [["keys", "chords"], ["bass", "notes"]]}}}
```

```bash
m2 tx . /tmp/bar9.json --check                              # what it would change; writes nothing
m2 tx . /tmp/bar9.json --as Claude --why "bar 9: E major"   # one undo step
```

A knob's change, as `m2 set` makes it: the hats' level in `first-song`'s `song.ts`, `db(-6)`, whose `-6` is bytes 412 to 414 (`grep -bo 'db(-6)' song.ts` says 409; the number starts 3 bytes on):

```json
{"id": "tx-hats-down", "base": {"song.ts": "sha256:5535…"},
 "ops": [{"op": "text_edit", "file": "song.ts", "edits": [{"start": 412, "end": 414, "expected": "-6", "text": "-9"}]}],
 "expect": {"values": [[{"written": {"file": "song.ts", "range": [412, 414]}}, -9]]}}
```

`m2 listen . --tracks hats --with /tmp/hats-down.json` measures the song as it is and as it would be with a Tx, and writes nothing: the hats 3 dB quieter, here ([Listening](listening.md)).

## Clip gestures as JSON

**In short**
- `m2 clips <song> json -` takes one clip gesture as JSON, from standard input, and makes it one undo step, with the app open or closed. `POST /api/graph/clips` takes the same, as `{"gesture": …}`.
- Each gesture below has one example. They run on a copy of the `first-song` example, whose clip ids they name ([Try it on a copy](changes.md#try-it-on-a-copy)).

```bash
echo '{"op": "delete", "clips": [{"module": "hats", "lane": "hits", "id": "92gr2959"}]}' | m2 clips . json - --as Claude --why "no hats"
```

- **A lane** is named by `module`, its track's key, and `lane`, the lane's key: the lane file is `data/<module>/<lane>.json`.
- **A clip** is `{"module", "lane", "id"}`, its `id` read from that file. A clip with no id is `{"module", "lane", "at": [<its start>, 0]}`. A gesture that makes clips prints each new one: `made keys/chords:c93zjdz7`.
- **Times** are beats from the song's start (a beat is a quarter note; bar 9 of a 4/4 song starts at beat 32). Inside a clip, a note's `start` and a point's beat count from the clip's start.
- **A note** is `{"pitch", "start", "length", "velocity"}`: `pitch` a name (`"C4"`) or a number (60 is middle C), `velocity` from 0 to 1 (0.8 when left out), as in a lane file ([Notes](code.md#notes)).
- A gesture that doesn't read is refused, naming the field that is missing or wrong.

`create`: a new empty clip on a notes or automation lane, from `start`, `length` beats long (a bar when left out). An automation clip holds the lane's value there.

```json
{"op": "create", "module": "keys", "lane": "chords", "start": 32, "length": 8}
```

`move`: clips moved by `by` beats (less than 0 is earlier). `"to": {"module", "lane"}` moves them to another lane of the same kind; `"copy": true` leaves them where they are and moves a copy, as ⌥-drag does.

```json
{"op": "move", "clips": [{"module": "keys", "lane": "chords", "id": "6ryexzev"}], "by": 4}
```

`resize`: a clip's new `start` and `length`. A new start moves its left edge, and what is inside stays where it plays.

```json
{"op": "resize", "clip": {"module": "bass", "lane": "notes", "id": "db9f9t5s"}, "start": 0, "length": 16}
```

`split`: each clip that crosses beat `at` cut in two there. The second half gets a new id.

```json
{"op": "split", "clips": [{"module": "keys", "lane": "chords", "id": "6ryexzev"}], "at": 16}
```

`join`: notes clips of one lane made one; the earliest keeps its id. Here, the two halves of the split above (the second's id is the one the split printed).

```json
{"op": "join", "clips": [{"module": "keys", "lane": "chords", "id": "6ryexzev"}, {"module": "keys", "lane": "chords", "id": "9dhjt3ft"}]}
```

`duplicate`: a copy of the clips right after them, their span rounded up to a whole bar; `times` copies (1 to 64), each after the one before. `"range": [<start>, <end>]` copies after those beats instead.

```json
{"op": "duplicate", "clips": [{"module": "keys", "lane": "chords", "id": "6ryexzev"}], "times": 2}
```

`delete`: clips taken out.

```json
{"op": "delete", "clips": [{"module": "hats", "lane": "hits", "id": "92gr2959"}]}
```

`notes`: a notes clip's notes, all of them: the list replaces the ones it has. `length`, when given, is the clip's new length. To change some notes of a clip, read the clip from its lane file, change them in the list, and send the whole list: one undo step, and the clip keeps its id.

```json
{"op": "notes", "clip": {"module": "keys", "lane": "chords", "id": "6ryexzev"}, "notes": [{"pitch": "A3", "start": 0, "length": 2, "velocity": 0.7}, {"pitch": "C4", "start": 0, "length": 2, "velocity": 0.7}, {"pitch": "E4", "start": 0, "length": 2, "velocity": 0.7}, {"pitch": "G3", "start": 2, "length": 2}], "length": 4}
```

`set`: whole clips on one lane, as its lane file writes them. Each clip in `put` replaces the clip with its id, or is added (a clip with no id gets one); `delete` takes clips out by id. This changes several clips of a lane as one undo step.

```json
{"op": "set", "module": "keys", "lane": "chords", "put": [{"id": "6ryexzev", "start": 0, "length": 4, "notes": [{"pitch": "A3", "start": 0, "length": 4}]}, {"start": 4, "length": 4, "notes": [{"pitch": "F3", "start": 0, "length": 4}]}], "delete": []}
```

`paste`: copied clips put at beat `at`, each as its lane file writes it, with the lane it came from. They go on that lane, or on lane `to` (`{"module", "lane"}`) when it fits them all.

```json
{"op": "paste", "clips": [{"module": "keys", "lane": "chords", "clip": {"start": 0, "length": 4, "notes": [{"pitch": "E4", "start": 0, "length": 1}]}}], "at": 32}
```

`paste` with `into`: copied notes put into a notes clip, the earliest copied clip's start at beat `at`, inside that clip. `"fit": true` moves each note to the nearest note of the song's key.

```json
{"op": "paste", "clips": [{"module": "keys", "lane": "chords", "clip": {"start": 0, "length": 4, "notes": [{"pitch": "E4", "start": 0, "length": 1}, {"pitch": "G4", "start": 1, "length": 1}]}}], "into": {"module": "keys", "lane": "chords", "id": "6ryexzev"}, "at": 8, "fit": true}
```

`stamp`: a chord on beat `at`, inside a notes clip, built from the song's key, so every note of it is in the key: what the roll's **Chord ▾** places. Set the key first (`m2 key <song> A minor`). `root` is the chord's lowest note in root position, a note of the key (`"D4"`, or 62); `chord` is one of the chords below; `inversion` moves its lowest notes up an octave (0, the default, moves none; a triad has 0 to 2, a `seventh` 0 to 3); `length` is in beats (1 when left out), `velocity` 0.8 when left out. In A minor this places D4 F4 A4 C5, Dm7, on beat 4 (bar 2):

```json
{"op": "stamp", "clip": {"module": "keys", "lane": "chords", "id": "6ryexzev"}, "at": 4, "root": "D4", "chord": "seventh", "length": 2}
```

`groove`: a feel on the clips' notes (one of the grooves below). `"notes": [0, 3]` picks notes of one clip by their place in its list, from 0.

```json
{"op": "groove", "clips": [{"module": "hats", "lane": "hits", "id": "92gr2959"}], "groove": {"name": "swing", "amount": 0.6, "step": "eighths"}}
```

`quantize`: each note's start moved to the nearest line of a grid `grid` beats apart (0.25 for sixteenths, 0.5 for eighths); `"ends": true` moves its end too. `notes` picks notes of one clip, as for `groove`.

```json
{"op": "quantize", "clips": [{"module": "bass", "lane": "notes", "id": "db9f9t5s"}], "grid": 0.5}
```

`hits`: the beat grid's steps on a Drum track's notes lane. A step starts at each beat of `at`, on a grid of `step` beats counted from each bar's start; `to` says what each becomes: `"on"`, a hit a step long where none starts; `"off"`, every hit that starts in it taken out; `{"velocity": 0.5}`, every hit in it at that velocity (above 0, at most 1). Here, two more kick hits, halfway after the third and the fourth beat of bar 1:

```json
{"op": "hits", "module": "kick", "lane": "hits", "at": [2.5, 3.5], "step": 0.25, "to": "on"}
```

`points`: an automation clip's points, all of them, `[<beat in the clip>, <value>]`, with straight lines between: numbers in the lane's unit, or on a lane of a choice or a switch, a name, `true` or `false`, each held until the next.

```json
{"op": "points", "clip": {"module": "bass", "lane": "cutoff", "id": "adp0sw76"}, "points": [[0, 300], [8, 1200], [16, 2200]]}
```

`audio`: an audio clip's settings; those left out stay. `gain_db` −48 to +12; `fade_in` and `fade_out` in seconds, together at most the clip; `reverse`; `follow`, whether it follows the song's tempo; `offset`, where in its file it starts, in seconds. This one is the `resample` example's first chop:

```json
{"op": "audio", "clip": {"module": "chops", "lane": "clips", "id": "a3s9vpxb"}, "gain_db": -3, "fade_out": 0.05}
```

`versions`: one act on a notes clip's versions ([Versions](versions.md)): several sets of notes kept in the clip, of which one plays and the others wait, never heard. `act` says which. `add` adds versions that wait, several in one undo step; on a clip with none yet, the notes that play are first named `original`. A version you add carries your name (`--as`); a `by` in the gesture is never read. With `"play": true`, the last version added then plays, in the same undo step: how the Playground's **Duplicate** makes a copy. Leave it out when you propose ideas: which one plays is the musician's choice.

```json
{"op": "versions", "clip": {"module": "bass", "lane": "notes", "id": "db9f9t5s"}, "act": "add", "versions": [{"name": "busy", "notes": [{"pitch": "A1", "start": 0, "length": 0.25}, {"pitch": "A2", "start": 0.5, "length": 0.25}]}, {"name": "sparse", "notes": [{"pitch": "A1", "start": 0, "length": 4}]}]}
```

`play` makes a version the one that plays: its notes become the clip's, and the notes that played wait under their name. `star` marks one (`"star": false` takes the mark off), `rename` names it `to`, and `remove` takes out one that waits.

```json
{"op": "versions", "clip": {"module": "bass", "lane": "notes", "id": "db9f9t5s"}, "act": "play", "name": "busy"}
```

```json
{"op": "versions", "clip": {"module": "bass", "lane": "notes", "id": "db9f9t5s"}, "act": "star", "name": "busy"}
```

```json
{"op": "versions", "clip": {"module": "bass", "lane": "notes", "id": "db9f9t5s"}, "act": "rename", "name": "busy", "to": "busy 2"}
```

```json
{"op": "versions", "clip": {"module": "bass", "lane": "notes", "id": "db9f9t5s"}, "act": "remove", "name": "sparse"}
```

`keep` removes every version but the one that plays, and the clip is a plain clip again. `m2 versions` sends each of these, and lists a clip's versions.

```json
{"op": "versions", "clip": {"module": "bass", "lane": "notes", "id": "db9f9t5s"}, "act": "keep"}
```

`find_notes` finds the notes of a sung audio clip, for Tune's note editing: [Edit a voice's notes](audio.md#edit-a-voices-notes).

What the gestures name, so you needn't ask the app (`GET /api/theory` lists the same, while it runs):

| Kind | Names |
|---|---|
| Chords, for `stamp` | `triad` (three notes: D F A), `seventh` (a fourth on top: D F A C; the app's **7th**), `sus2` (D E A), `sus4` (D G A), `add9` (D F A E). Each is built from the key's own notes, so whether it is major or minor depends on its root |
| Grooves, for `groove` | `{"name": "swing", "amount": 0.6, "step": "eighths"}`: every second eighth (or `"sixteenths"`) later, `amount` from 0.5 (straight) to 0.71, about 0.66 a shuffle; `{"name": "laid-back"}`: the notes between beats 1/32 of a beat late; `{"name": "pushed"}`: 1/32 early; `{"name": "accents"}`: velocities only, 0.9 on the beat, 0.75 on the half beat, 0.6 between |
| Scales, for the key | `major`, `minor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian`, `harmonic-minor`, `melodic-minor`, `major-pentatonic`, `minor-pentatonic`, `blues`, `chromatic`. A pentatonic or blues key stamps only the chords whose notes are all in it; a chromatic key has none ([Music basics](music-basics.md)) |
| Roots, for the key | `C`, `C#`, `D`, `Eb`, `E`, `F`, `F#`, `G`, `Ab`, `A`, `Bb`, `B` |

## The routes

Every route is in one table, declared once with its method, its path, the largest body it takes, whether it writes, whether agents may call it, what it does and an example body; a path the table hasn't got answers 404. `m2 routes` prints it, and `m2 routes --json` as JSON; `m2 call <song> <path> [json]` sends one, with the song's `session` filled in. The ones you will use most (all of them: [API routes](routes.md)):

| Route | What it does |
|---|---|
| `GET /api/graph` | The open song: name, folder, `session`, the graph view (with `revision` and every knob's id), the timeline, the Mix view, the effects and instruments it can add, problems with their fixes, undo's names, where playback is, and the drags in progress |
| `GET /api/graph/status` | Where playback is: playing or not, the beat, the tempo there and the loop, with the open song's `session` |
| `POST /api/tx/check`, `/api/tx/commit` `{session?, tx}` | Check a Tx, or commit it as one undo step (`m2` sends its writes here) |
| `POST /api/graph/undo` `{session}` | With the tools key, take back the newest change only while it is this agent's (`m2 undo`) |
| `GET /api/focus`, `POST /api/point` | What the musician looks at; a pointer (`m2 focus`, `m2 point`) |
| `POST /api/graph/effect/add` | Add an effect after a call's sound (`call`), or on one wire, as its **+** does: `reader` and `input` in place of `call` (`{"reader": "song.ts#2", "input": "0.keys.in", "component": "@m2/effects:delay"}`) |
| `POST /api/graph/picture` | A node's picture as numbers: its shapes in its axes' units, each dot's values with the knobs that move it, and its words. Ask again within 10 s to keep it |

## Events

`/api/ws` is a WebSocket. Each event is JSON, its type in `t`, with the `session` of the song it is about (null when none is open):

| `t` | When |
|---|---|
| `toast` | a message (`text`). Toasts sent while no window of the app was connected, such as why the folder given at launch didn't open, go to the next window that connects. A socket opened with the tools key (`m2 watch`) hears them as they come, but never takes one from the window. |
| `graph` | the song changed and should be fetched again: with its `session`, `revision`, `problems`, `diagnostics` and the drags in progress (`leases`) |
| `step` | a change that is one undo step landed, whoever made it: its id (`step`), the revision it made (`made`), `source` (`page`, `agent` with its name in `by`, or `outside` for a file saved in another program), its `name`, its `files`, whether undo can take it back now (`undoable`), and the `tracks`, `lanes` (`[track, lane]`) and `knobs` (by the view's ids) it changed |
| `undo`, `redo` | a step went or came back: its id (`step`), `source`, `by` and `name` |
| `compare` | the Compare pill opened, flipped or closed: the `step` it compares, `open`, `after` (true: After plays, the song as it is; false: Before), its `name`, `n` (counting up: the newest wins), and when it closed, `why`: `closed` (Esc, which on Before undid the step and says `after: false`, or another action), `changed` (another change to undo's history), `outside` (a file saved in another program), `page` (the window that opened it went away), `song` (the song closed or another opened) or `failed` (Before couldn't be got ready to play) |
| `focus` | what the musician looks at changed: `focus`, as `GET /api/focus` says it |
| `point` | a pointer shown, or taken away (`point` is null) |
| `lagged` | this socket fell behind and missed `missed` events: read the song again (the window reads its pointer and the Compare pill again too). `m2 watch` says its own, with `"by": "m2"`, when whoever reads it falls behind |
| `export` | while a song is exported, with the `song`'s name and its folder (`dir`): `progress` (0 to 1, at most ten a second), then `done` with `path`, `seconds`, `peak_db` and `lufs` (the file's loudness), or `error` |
| `take` | a recording's count-in, its end, and the take written |
| `midi` | a MIDI keyboard came or went, or what it plays changed |
| `meters` | the meters, about 30 times a second while the song sounds, to a window's socket, and to an agent's that asks for them (`m2 watch --meters`): each track's (`tracks`) and each mixer channel's (`channels`) loudest sample, linear, after its fader; the song's (`out`); and its `loudness` (`short`, the last 3 s in LUFS, and `true_peak` in dBTP) while someone asks for it with `POST /api/graph/loudness`; and `dsp`, the load and the dropouts. A message that heard no whole block of sound (the engine ran late) holds only `dsp` |
| `looks` | with the meters, to the window that asked for looks with `POST /api/graph/look` alone: each look's `module`, `node`, `kind` and `at`, and what it saw ([Looks](#looks)), or `why` it can't show now. An agent gets the same in each answer to its ask |
| `audition` | why a note this window asked to hear plays nothing |
| `feed` | a feed's state ([Playing live](live.md#events)): with `why` `low` when its buffer drops under the low mark, `empty` when it runs out, `jump` at Stop, a seek or the loop going round, `open` and `closed`; with none, at most every quarter second while playing |

## Looks

`POST /api/graph/look` asks to look at a box (a call, by its `module` and its id in the view) while the song plays. Ask again within 10 s to keep it, or send `"on": false` to stop. What it sees comes in the `looks` message of the window that asked, beside the meters, and in each answer, `{"look": {…}}`.

| `kind` | What each `looks` message (or answer) brings | `at` |
|---|---|---|
| `scope` (the default) | `frames`: the last 1,024 frames, a list per channel | `out` (the default): the call's own sound (a call turned off: what plays through it). `in`: the one sound going into it |
| `spectrum` | `bins`: how loud the sound is at each pitch, in dB, for bands a twelfth of an octave wide. Band `k` is centred on 20 × 2^(k/12) Hz: 120 bands at 44.1 kHz and over. A sine at full scale reads 0 dB: within half a dB from 80 Hz up, within 1 dB below. Below about 100 Hz two notes a semitone apart run together, and below about 150 Hz noise reads louder than it is. The answer to the ask gives each band's centre, `hz` | `out` or `in` |
| `level` | `peak` and `rms` for each channel since the message before (an agent: since its answer before, as far back as about a third of a second), linear (1.0 is full scale) | `out` or `in` |
| `meter` | `now` and `most`: a number the sound's own code sends, as it is now and the most since the message before (an agent: since its answer before), in its `unit` (the answer names it) | the meter's name: `reduction` for std's `compress`, `duck` and `limit` and `@m2/mixing`'s `gate` and `deesser`, how far they turn the sound down, in dB; the multiband's `low_reduction`, `mid_reduction` and `high_reduction`; the gate's `open`, 1 while it is open; Tune's `off` and `left`, how far the voice it hears is from its note and how far its move leaves it, in semitones (+ sharp, − flat; 0 with no pitch); the Clipper's `pushed`, the loudest it heard after its gain, in dB (down to −100); the Flanger's `phase`, where its sweep is, 0 to 1; Grains' `reads`, how far back the newest grain reads, in seconds (0 when none plays); the Arpeggiator's `step`, the step playing, in beats (−1 while no note is held) |

- A plan shows 31 points of sound at once (looks at one point share it) and 256 meters. Past that, an ask is refused in words.
- A call with no sound going in, or more than one, has no `in`. A meter the call doesn't give is refused, naming the ones it gives.
