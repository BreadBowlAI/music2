# API routes

**In short**
- The routes of the app's local API that agents use most.
- `m2 routes` prints every route, from the app's one route table.

Every route is in one table, declared once with its method, its path, the largest body it takes, whether it writes, whether agents may call it, what it does and an example body; a path the table hasn't got answers 404. `m2 routes` prints it, and `m2 routes --json` as JSON; `m2 call <song> <path> [json]` sends one, with the song's `session` filled in. The ones you will use most:

| Route | What it does |
|---|---|
| `GET /api/graph` | The open song: name, folder, `session`, the graph view (with `revision` and every knob's id), the timeline, the Mix view, the effects and instruments it can add, problems with their fixes, undo's names, where playback is, and the drags in progress |
| `GET /api/graph/status` | Where playback is: playing or not, the beat, the tempo there and the loop, with the open song's `session` |
| `POST /api/tx/check`, `/api/tx/commit` `{session?, tx}` | Check a Tx, or commit it as one undo step (`m2` sends its writes here) |
| `POST /api/graph/undo` `{session}` | With the tools key, take back the newest change only while it is this agent's (`m2 undo`) |
| `GET /api/focus`, `POST /api/point` | What the musician looks at; a pointer (`m2 focus`, `m2 point`) |
