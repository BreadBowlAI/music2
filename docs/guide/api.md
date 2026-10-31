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

The musician's own controls answer the tools key 403, with plain words: play, stop, seek, the loop and Stop All Sound; recording; muting and soloing while listening; the view; plugin windows; Look again for plugins (it loads each one where the song's plugins play); auditions; drags (agents don't drag: an agent's `/api/graph/set` is one edit, busy while a drag lives); redo and skipping a step; naming steps; the code editor's list of unsaved files; posting the focus record and Compare; Export Audio and stems, which may pause playback (an agent renders with `m2 render` or `m2 bounce`); saving a preset into the musician's own; settings; and opening, closing, making, saving as or renaming songs. What a WebSocket opened with the tools key sends is ignored. Every read stays open to agents, and so does every change to the song the app's window can make.

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

Edits name the open project's `session` and the `revision` they were planned against, so a stale edit is refused rather than landing on the wrong thing. While the song is open read-only (another program holds it), every route that writes answers 423 with the reason, and changes nothing.

An agent can also send any edit as a **Tx**: the files it reads, each with the hash (or the text) it was planned on, the changes, and what they may do to the song. The app checks it as it checks its own edits, writes it as one undo step, and refuses it if a file changed under it or the song would change in any other way ([WRITE-3](https://github.com/BreadBowlAI/music2/blob/main/docs/architecture.md#the-two-write-primitives)). For example, deleting a data file no lane reads:

```bash
sha=$(shasum -a 256 data/kick/old.json | cut -d' ' -f1)       # in the song's folder
tx='{"id": "tx-delete-old", "base": {"data/kick/old.json": "sha256:'$sha'"},
     "ops": [{"op": "remove", "file": "data/kick/old.json"}], "expect": "identical"}'
curl -s -H "$H" -H 'content-type: application/json' $URL/api/tx/check -d "{\"tx\": $tx}" | jq .checked.hash
curl -s -H "$H" -H 'content-type: application/json' $URL/api/tx/commit -d "{\"tx\": $tx}"
```

## The routes

Every route is in one table, declared once with its method, its path, the largest body it takes, whether it writes, whether agents may call it, what it does and an example body; a path the table hasn't got answers 404. `m2 routes` prints it, and `m2 routes --json` as JSON; `m2 call <song> <path> [json]` sends one, with the song's `session` filled in. The ones you will use most (all of them: [API routes](routes.md)):

| Route | What it does |
|---|---|
| `GET /api/graph` | The open song: name, folder, `session`, the graph view (with `revision` and every knob's id), the timeline, the Mix view, the effects and instruments it can add, problems with their fixes, undo's names, where playback is, and the drags in progress |
| `GET /api/graph/status` | Where playback is: playing or not, the beat, the tempo there and the loop, with the open song's `session` |
| `POST /api/tx/check`, `/api/tx/commit` `{session?, tx}` | Check a Tx, or commit it as one undo step (`m2` sends its writes here) |
| `POST /api/graph/undo` `{session}` | With the tools key, take back the newest change only while it is this agent's (`m2 undo`) |
| `GET /api/focus`, `POST /api/point` | What the musician looks at; a pointer (`m2 focus`, `m2 point`) |

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
| `meters` | the meters, about 30 times a second while a socket listens and the song sounds: each track's (`tracks`) and each mixer channel's (`channels`) loudest sample, linear, after its fader; the song's (`out`); its `loudness` (`short`, the last 3 s in LUFS, and `true_peak` in dBTP); and each scope asked for with `POST /api/graph/scope` (`scopes`: its `module`, `node` and its last 1,024 `frames`, a list per channel) |
| `audition` | why a note this window asked to hear plays nothing |
