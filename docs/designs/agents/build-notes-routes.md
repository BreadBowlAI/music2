# Agents beside the musician: build notes for track 4, `ag-routes`

**In short**
- Track 4 of the [agents design](design.md) is built on branch `ag-routes`. Every route of the app's API is declared once, in one table: its method, its path, whether it writes, whether agents may call it, plain words and an example body.
- The router, the read-only window's 423 and the tools key's 403 are all made from that table. The eleven hand lists of the routes that write, the list of the musician's routes, and the eleven copies of the 423 check are gone.
- The tools key now gets 403 on every one of the musician's controls: play, seek, the loop, recording, mute and solo while listening, the view, plugin windows, drags, redo, exports, settings, and opening, closing or making songs. What an agent's WebSocket sends is ignored.
- `m2 routes` lists the table (`--json` too), with no app running. `m2 call <song> <path> [json]` sends one route as an agent, so every song change the window can make is reachable from `m2`, those with no command of their own too: an effect turned off, markers, the song's key.
- Two holes in AGENT-3 were found on the way, and fixed: an agent's set could commit the musician's knob drag, and an agent's change taken back at its settle answered as if it had landed.
- The contract changed first: AGENT-8 and AGENT-11, and the text of AGENT-3, AGENT-10, WRITE-17 and the acceptance gates' agents row ([architecture.md](../../architecture.md#agents)).
- 14 tests were written first and seen failing, then the code made them pass. No UI changed, so there are no screenshots.
- The review's ten findings are fixed ([below](#the-reviews-fixes)): a tools socket names no window, an agent's refused import leaves nothing in the song, `m2 import` goes through the app's own import, each route carries its body limit, and no route outside the table answers.

## What was built

| Layer | What |
|---|---|
| `crates/server/src/api.rs` | `Route {method, path, writes, agents, words, example}`; the `routes!` macro, which makes a file's `PART`, its part of the table and its router, from the same lines; `PARTS`, each file named once; `all()` (every route, by path, then method), `find(method, path)` (a HEAD is a GET's), `router()` (every file's router, and the 404 for any other `/api` path); `Route::refusal()`, the 403's words; and `gate`, the one middleware: 403 for the tools key on a route not for agents, then 423 for a write in a window open read-only. |
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
8. **"No hand list is left" is read from the source**: outside `api.rs`, no file of the server may call `.route(`, `.route_layer(`, `.merge(` or `.nest(`, or hold a `const WRITES` or `MUSICIANS` list. The macro makes each file's router and its part of the table from the same lines, and `api.rs` names each file once, so the two can't drift; `every_example_body_reads` also shows the router has every route of the table. (The PLAT-2 test reads the source the same way.)
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
- `ci/check.sh --pr` at `d95d892`: `music2 check --pr: PASS` (quick, clippy, rust, suites, renders, repo unchanged; 6 min), the first run, nothing failed or passed only on a retry.
- After it, `api.rs` was made to name each file of routes once (`e435c65`). `ci/check.sh --pr` at `e435c65`: `music2 check --pr: PASS`, 1,866 Rust tests, 54 of 54 browser suites (1,966 checks), 6 min, the first run, nothing failed or passed only on a retry. Logs: `~/m2-runs/check-routes.tgz` and `~/m2-runs/check-routes-2.tgz` on the build server.

## The review's fixes

The review of `ag-routes` (ten findings, F1 to F10, in `~/m2-runs/sag/review-routes.json` on the build server) is fixed at `31633f5`, after its contract (`f2cb132`: AGENT-1, AGENT-3, AGENT-8, AGENT-10 and AGENT-11, and the MIDI file export's rule). Each finding got a test first, from the skeptic's experiments (`~/m2-runs/sag/skeptic-routes/`) where there was one, and each was seen failing before its fix (`~/m2-runs/sag/routes-logs/review-fail-first-*.log`).

| Id | What was wrong | The fix | Test, failed first |
|---|---|---|---|
| F1 | A WebSocket opened with the tools key and a window's `client` ended every drag of that window when it closed: knobs, clips, the tempo, and its auditions. | A socket names a window only when the page key opened it: `routes::ws` ignores a tools socket's `client`, so its closing ends nothing. | `a_tools_socket_naming_a_window_ends_none_of_its_drags` (it ended all three drags, and the next update was "this gesture ended") |
| F2 | An agent's audio import or Sampler file copied its file into `assets/` before the busy check, so a refused import left it there, the watcher's batch closed the Compare pill, and `m2`'s retry then landed. `m2 import` did the same. | One shape for every file an agent brings in: the arrival check first, before anything is copied (`Graph::arriving`); then a copy to a hidden part file, which nothing of the song reads (`m2_editor::import::stage`); then, in the job that lands the change, after its own check, the part named into `assets/` (`Staged::name`). Refused, the part goes, and the folder when the import made it. One import at a time, from its copy to its naming. `m2 import` with the app open sends the files' paths to the app's own import (`App::import`), so a retry copies nothing until the change lands. MIDI files never copied a file in. | `an_agents_import_while_comparing_copies_nothing` (an import and a Sampler's file while comparing), `a_take_begun_while_an_agents_import_copies_leaves_nothing` (a take begun while it copied), `m2_import_while_comparing_copies_nothing` (`m2 import` exited 0 after the pill closed under it) |
| F3 | `m2 call /api/graph/undo` said "one undo step", named after the musician's step under it. | `m2` says a step only when the answer's step is undo's newest, and "took back Claude: A minor" when it is redo's newest (`app::done_in_app`, for every forwarded write too). | `m2_call_undo_says_what_it_took_back` |
| F4 | The routes the server names a step for (audio import, Sampler file, audio track, MIDI file) renamed the agent's step in a second job: `--why` was lost, after its event and the answer had said it. | Each names its step in the job that makes it, by the agent's words when it gave some, else by its own name ("importing vox.wav"). | `an_agents_step_is_named_by_its_words_in_the_same_job`, `m2_call_names_every_step_by_its_why` |
| F5 | `POST /api/graph/midifile/export`, a read for agents, wrote into the open song's folder, a read-only window's too; under `assets/` it closed Compare. | A file picked inside the open song's folder, through a link too, is refused in words, whoever asks: what the contract already said ("outside the song only"). It stays a read, so a read-only window still exports MIDI, as the musician expects. | `a_midi_file_is_never_written_into_the_song` |
| F6 | Look again (`POST /api/plugins/scan`) was open to agents, and loads every plugin library in the host where the song's plugins play. | The musician's: 403 for the tools key. `GET /api/plugins`, the list kept, stays open. | `the_tools_key_gets_403_on_every_route_not_for_agents` (45 controls now) |
| F7 | The table had no body limit, so `m2 call` sent bodies the route refuses, then said it couldn't tell whether they landed. | Each route carries `limit` (`api::BODY`, 2 MiB, unless its line says `up to`); the macro gives the router the same number; `m2 routes --json` lists it; `m2 call` refuses a bigger body before sending: "nothing was sent: this body is 3.0 MiB, more than POST /api/graph/title takes (2 MiB)". A refusal that isn't JSON is said once, on standard error. | `m2_call_refuses_a_body_too_big_before_sending`, `m2_routes_lists_every_route` (no `limit`); `each_route_takes_bodies_up_to_its_limit` (written with the field it reads) |
| F8 | "No route outside the table" was a search of the source for five spellings, and the gate let through any path it didn't find. | Deny by default: every `/api` path reaches the table's routes only through the gate (`routes::served`), which answers a path or a method the table hasn't got 404 or 405, whoever asks; every other path is the page's files. | `a_route_outside_the_table_answers_no_one` (routes added with `route`, `route_service`, `merge` and `nest` answered) |
| F9 | The 403 read "is the musician's: play or stop: play, stop, toggle or restart", and the contract quoted words the app never says. A line break in an agent's name glued its words. | The transport's, the view's and a knob drag's cancel words hold no colon, and a test reads AGENT-11's Today for the transport's very refusal. A control character in an agent's words reads as a space. | `the_refusals_read_as_the_contract_quotes_them`, `a_line_break_in_an_agents_words_reads_as_a_space` |
| F10 | An agent's step with no words was named after `layout.json`, "the track order, colours or markers", for a song key change, and for `m2 import`. | `m2 import` is named by the import now (F2, F4). `layout.json`'s plain name says the key too: "the track order, colours, markers or the song's key". | `a_step_made_elsewhere_gets_a_plain_name`, `m2_import_while_comparing_copies_nothing` |

`HELD` still holds: the gate's paths and methods (each odd spelling is still 404, 405 or 400, and playback never starts; a wrong method on a table path is now the gate's 405 with words, not axum's empty one), the static files, the keys, a tools socket's `midi` and `audition` ignored, a read-only window's 423, the set that never commits a drag, `m2 call`'s headers and refusals, and the builder's choices.

One test changed what it points at, not what it checks: `the_song_and_a_clip_are_written_where_picked` picked files in the song's folder, which F5 refuses; it picks them in a folder beside it.

### Where the leanings were left

- **F1: the key decides, and the page still names its window.** A socket opened with the tools key names none, so no agent can end a drag by a name. A page-key socket can still give any window's id, but only the musician's own pages hold that key; the server handing each page its id would touch the page and every suite that opens a socket, for no case an agent can reach (gaps). Window ids stay in `GET /api/graph/leases`, the state and the `graph` event: the page needs them, and after F1 no route open to agents takes one.
- **F2: through the app's own import, not inside a forwarded Tx.** A Tx carries text; an audio file is up to 1 GiB. So `m2 import` hands the paths to the import the page uses, and the app makes the copy, which becomes the song's only inside the change that lands. The same staging serves both import routes and the Sampler's; a MIDI file never put a file in the song. `/api/tx/commit` still takes `assets`, which no `m2` command sends now (gaps).
- **F5: refused inside the song, for everyone.** Simpler than marking it a write, which would take MIDI export from a read-only window, and the contract already said "outside the song only".
- **F8: a test of behaviour, not of text.** axum can't list a router's routes, so the test adds routes outside the table in each way axum allows and sees none answer. The scan of the source for route spellings is gone; the scan for hand lists of paths stays.
- **The taken-back case of F2 is a gap**: an import the musician's drag meets at its settle is taken back, and its new file stays in `assets/`, named by nothing (music2 never deletes an asset).

### Checked

- The crates touched, `m2-editor`, `m2-server` and `m2`: every test passes, and clippy on them with every warning an error. The browser suites ag-focus, ag-notice, undo, nx-undo, s6-write, s8-leases, code, notes, attention, st-audio, st-midi, st-stems, ab-sampler, br-plugins, start and s8-tracks: each ok.
- `ci/check.sh --gates=auto` at `31633f5`: `music2 check: PASS`, the first run, with the gates (AGENTS.md changed since `main`): 1,918 Rust tests, 76/76 browser suites (3,288 checks), 12 of 12 renders, the 7 gate runs, 27 minutes, nothing failed or passed only on a retry. Logs: `~/m2-runs/check-routes.tgz` on the build server.
- No UI changed, so there are no screenshots.
