# Agents beside the musician: build notes for track 4, `ag-routes`

**In short**
- Track 4 of the [agents design](design.md) is built on branch `ag-routes`. Every route of the app's API is declared once, in one table: its method, its path, whether it writes, whether agents may call it, plain words and an example body.
- The router, the read-only window's 423 and the tools key's 403 are all made from that table. The eleven hand lists of the routes that write, the list of the musician's routes, and the eleven copies of the 423 check are gone.
- The tools key now gets 403 on every one of the musician's controls: play, seek, the loop, recording, mute and solo while listening, the view, plugin windows, drags, redo, exports, settings, and opening, closing or making songs. What an agent's WebSocket sends is ignored.
- `m2 routes` lists the table (`--json` too), with no app running. `m2 call <song> <path> [json]` sends one route as an agent, so every song change the window can make is reachable from `m2`, those with no command of their own too: an effect turned off, markers, the song's key.
- Two holes in AGENT-3 were found on the way, and fixed: an agent's set could commit the musician's knob drag, and an agent's change taken back at its settle answered as if it had landed.
- The contract changed first: AGENT-8 and AGENT-11, and the text of AGENT-3, AGENT-10, WRITE-17 and the acceptance gates' agents row ([architecture.md](../../architecture.md#agents)).
- 14 tests were written first and seen failing, then the code made them pass. No UI changed, so there are no screenshots.

## What was built

| Layer | What |
|---|---|
| `crates/server/src/api.rs` | `Route {method, path, writes, agents, words, example}`; the `routes!` macro, which makes a file's `ROUTES` and its `router()` from the same lines; `all()` (every route, by path, then method), `find(method, path)` (a HEAD is a GET's), `router()` (every file's router, and the 404 for any other `/api` path); `Route::refusal()`, the 403's words; and `gate`, the one middleware: 403 for the tools key on a route not for agents, then 423 for a write in a window open read-only. |
| `crates/server/src/routes*.rs` | Each file declares its routes with `api::routes!`: 110 routes, 62 for agents (every read, and 40 song changes), 48 the musician's. Their `WRITES_*` lists, their 423 middlewares and `MUSICIANS` are gone. `routes::app` is the whole router: the table's routes behind the gate and `auth`, and the page. A WebSocket opened with the tools key hears every event, and what it sends is ignored. The test-only inject routes, when off, say so in words of their own. |
| `crates/server/src/caller.rs` | `encoded` and `decoded`: an agent's words in a header (and a GET's keys) as percent-encoded UTF-8, so any name fits. `auth` reads `x-m2-as` and `x-m2-why` so. |
| `crates/server/src/graph.rs`, `graph/actor.rs` | An agent's `set` is always a plain value edit, never a drag's commit. `Actor::answered` reads `taken_back` before it resets the caller. `Graph::may_end_song` is gone: the tools key gets 403 on opening, closing and making songs. |
| `apps/m2` | `call.rs`: `m2 routes [--json]` and `m2 call <song> <path> [<json> \| -] [--method M]`. `app.rs`: an answer is kept as it came (bytes, de-chunked), so `m2 call` can print a waveform's peaks; requests can carry headers; `App::call` sends `--as` and `--why` as headers, waiting while busy as every write does. |
| Docs | The contract; the manual (`m2 routes`, `m2 call`, the musician's controls, the route table now a pointer to `m2 routes` with five routes you will use most); README's In short; AGENTS.md's pointer to `m2 routes`; gaps; the design's status line; HANDOFF. |

## The tests, and how they failed first

Each was written against the new API, with bodies that did nothing (an empty table, no `m2 routes` or `m2 call`), and run before the code. Logs in `~/m2-runs/sag/routes-logs/` on the build server.

| Where | Tests | Failed first |
|---|---|---|
| `crates/server/src/api_tests.rs` | `every_route_is_declared_once` (no route declared twice; each has words and an example; no file of the server adds a route, merges a router or keeps a list of paths outside `api.rs`), `every_example_body_reads` (each example, sent by who may send it, never gets axum's "Failed to deserialize", and the router has every route), `a_read_only_window_answers_every_write_423` (whoever sends it, and only the writes), `the_tools_key_gets_403_on_every_route_not_for_agents` (the 44 controls AGENT-11 names among them, with the route's own words; nothing they would do happens; every read and the 40 song changes stay open) | all 4 (`fail-first-server.log`) |
| `crates/server/src/routes_8b_tests.rs` | `a_tools_socket_cant_audition_or_aim_the_midi_keyboard` | yes |
| `crates/server/src/graph/busy_tests.rs` | `an_agents_set_never_commits_the_musicians_drag`: it did, 200 and the drag gone | yes |
| `crates/server/src/graph/busy_tests.rs` | `an_agents_gesture_taken_back_at_its_settle_answers_busy`: it answered 200, though the change was taken back | yes (`fail-first-server-taken-back.log`) |
| `crates/server/src/routes_see_tests.rs` | `while_comparing_an_agent_cant_close_or_open_a_song` now expects 403 (it expected 423 busy) | yes, with 423 |
| `apps/m2/tests/call.rs` | `m2_routes_lists_every_route`, `m2_call_reaches_an_action_with_no_command` (an effect's bypass), `m2_call_refuses_the_musicians_controls` (before sending, with no app too), `m2_call_names_its_step` (a name that isn't plain ASCII, kept whole), `m2_call_method_picks_one_of_two` (and a GET's keys in the query), `m2_call_is_busy_as_every_write_is` | all 6 (`fail-first-m2.log`); `m2_call_is_busy_as_every_write_is` failed again on the first code, which found the taken-back hole (`fail-first-m2-call-busy-answered-200.log`) |

`caller.rs`'s `an_agents_words_cross_a_header_whole` was written with the code it tests. The design lists 4 server tests and 5 of `m2`; `a_tools_socket_cant_audition_or_aim_the_midi_keyboard`, the two holes' tests and `m2_call_is_busy_as_every_write_is` are added.

## Choices against the design

Where the design was unclear, or didn't fit today's code, this is what was done and why.

1. **`m2 call` names the song**: `m2 call <song> <path> [json]`, as `m2 focus`, `m2 watch` and `m2 point` do (track 3's choice 2); the design wrote `m2 call <path> [json]`. It reaches the app that has that song open, through the same checks as a write, and fills in the song's `session` when the route takes one and the body leaves it out. So an agent's call never lands on another song than the one it means.
2. **The code editor's save stays open to agents.** The design's 5.8 lists it as the musician's. Since the review of track 2 (F12), `m2 edit` and `m2 write` of a song whose code doesn't build save through it, and an agent's save lands only if the song then builds (AGENT-1). AGENT-11 says so. The code editor's list of unsaved files stays the musician's.
3. **Export Audio and stems are the musician's.** An export may pause playback to borrow the plugins' instances (RENDER-1), and one runs at a time; agents render in their own process (`m2 render`, `m2 bounce`). The MIDI file export pauses nothing, and stays open to agents.
4. **Saving a preset is the musician's**: it writes into their own presets, and a preset saved over another can't be undone. Loading one is a song change, one undo step, and stays open to agents.
5. **Busy exits 3.** The design's `m2 call` exits 1 on any status of 400 or more; a busy write waits and exits 3 as every `m2` write does (5.2). Other refusals exit 1.
6. **The test-only inject routes are in the table**, as the musician's: every route the router has is. Off (no `M2_MIDI=inject` or `M2_AUDIO_INPUT=inject`), they answered with the router's own "no such API"; now with words of their own, so a route the router hasn't got can be told apart.
7. **Closing, opening or making a song with the tools key is 403, always.** Track 3 made them busy while a take recorded or the Compare pill was open (`Graph::may_end_song`), "until track 4". That check is gone, and track 3's test expects 403 now: stricter, and what it checks (Compare stays open, on the same song) is unchanged.
8. **"No hand list is left" is read from the source**: outside `api.rs`, no file of the server may call `.route(`, `.route_layer(`, `.merge(` or `.nest(`, or hold a `const WRITES` or `MUSICIANS` list. The macro makes each file's router and its table from the same lines, so the two can't drift; `every_example_body_reads` also shows the router has every route of the table. (The PLAT-2 test reads the source the same way.)
9. **Examples are sent with a session that isn't the open song's**, so each body is read, then refused, and nothing changes; paths on this computer move into the test's folder, and the routes that end the song go last. The read-only and 403 tests send the open song's own session, so only the gate stands between the request and the song.
10. **A header carries an agent's words percent-encoded** (UTF-8, `%C3%A5` for å), read back by `auth`; a plain name sent as it is reads the same. No new crate.
11. **What `m2 call` prints**: the app's answer on standard output, as JSON (a waveform's peaks as their bytes), and "in the open app: one undo step, …" on standard error, so the output stays the answer alone. `m2 routes --json` gives each route's keys in the order JSON objects keep them here (by name).
12. **The 403's words** are made from the table: "POST /api/graph/transport is the musician's: play or stop. Agents can't call it (AGENT-11); ask the musician, or point at it with m2 point." `m2 call` says the same before it sends anything.
13. **Two holes in AGENT-3, fixed here** because `m2 call` reaches every gesture route: `/api/graph/set` found a lease by its gesture string, so an agent naming the musician's drag committed it; and `Actor::answered` cleared `taken_back` before reading it, so a gesture route's change taken back at its settle (a label while its file had unsaved code) answered 200 with what it would have written. `m2`'s own commands send a Tx, whose job answers busy by itself, so neither showed before.

## Gaps closed and added

In [gaps.md](../../gaps.md):
- **Closed:** "The musician's controls are still open to the tools key" (track 2's, until track 4).
- **Narrowed:** #37, the missing `m2` commands: with the song open, `m2 call` reaches each but Save As (the musician's); with it closed, they need a Tx or the files. #38, a self-describing API: every route is listed with words and an example; a JSON Schema of each, a summary of the song and MCP remain.
- **Updated:** an agent's write through a gesture route is checked on arrival only against a take: `m2 call` sends them, and the settle's check answers busy.
- **Added** (For developers): the browser suites keep a list of their own of the page's requests that write nothing; the table could give it.

## Checked

- The crates touched, `m2-server` and `m2`: every test passes (477 of the server's own, and every test of `m2`), and clippy on both, with every warning an error.
- The routes' writes are the same 53 the old hand lists held, so a read-only window refuses what it refused before; the musician's routes hold every one the old 403 list held.
- No UI text or page changed, so no screenshots were taken.
- `ci/check.sh --pr`: below.
