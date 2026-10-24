# Agents beside the musician: build notes for track 3, `ag-see`

**In short**
- Track 3 of the [agents design](design.md) is built on branch `ag-see`. An agent can now see what the musician looks at (`m2 focus`), follow everything that happens in the app (`m2 watch`), and point at things to show where they are (`m2 point`).
- The musician presses **V** to hear before and after the newest change an agent or another program made. A pill says "Before: Claude: brighter hats", then "After". While it shows, every agent's change is refused, busy, so nothing can take After away.
- What the musician looks at and what an agent points at are attention, not song: memory only, never a file, a revision or an undo step, gone when the song closes (AGENT-6).
- A pointer never moves the view or the selection. Out of view, it waits at the edge with Show; only that click moves the view.
- The contract changed first: AGENT-6, and AGENT-3's text and Today ([architecture.md](../../architecture.md#agents)); then once more for two refinements (below).
- 15 Rust tests and one browser suite (54 checks) were written first and seen failing, then the code made them pass. Small loose ends are in [gaps.md](../../gaps.md).

## What was built

| Layer | What |
|---|---|
| `crates/server` | `graph/focus.rs`: the focus record (`Focus`), posted by the page (`POST /api/focus`, the page key only) and served with what the server knows (`GET /api/focus`: playing, beat, loop, unsaved code, Compare, the MIDI target); the pointer (`POST /api/point`), its target checked against the accepted revision (`track:`, `clip:`, `lane:`, `knob:`, `bars:`), one at a time, the page's OK clearing it. Both kept beside the unsaved list, dropped when the song closes or another opens, told as `focus` and `point` events; the state carries `point` and `comparing` for a page that loads later. `graph/compare.rs`: what the Compare pill compares, set by V's undo or redo (`compare: true`), ended by the page's close (`POST /api/graph/compare`), any other change to undo's history, the page's socket closing, the song closing, and a batch the watcher takes in; each end is a `compare` event with why. `busy.rs` holds everything while it is set. Undo and redo are told as `undo` and `redo` events, after the steps before them. `lib.rs`: a toast is kept until a page's socket hears it; a tools-key socket takes none. `routes.rs`: a socket that falls behind hears `lagged`; Compare and posting the focus answer the tools key 403. `socket.rs`: a small WebSocket client (`m2 watch`, the tests). |
| `apps/m2` | `see.rs`: `m2 focus`, `m2 watch` (every event as one JSON line, `t` first; the transport read every 250 ms; `--meters`; `closed` and exit 0 when the app goes away) and `m2 point`. `app.rs`: `looking` reaches the app that has the song open, to read or point. |
| `web` | `lib/focus.ts`: the record, posted 200 ms after the last change and when the socket connects; the roll reports its selected notes, the timeline the beats it shows, the code view its file and line, any knob a press or the keyboard reaches. `lib/pointer.ts` and `PointerNote.tsx`: the note at its target, beside a header, under the ruler's bars, above a clip or a knob, with a dashed ring (the glow is solid); at the edge with Show when out of view. `lib/compare.ts` and `ComparePill.tsx`: V, the pill in the top bar, its flips, and what closes it. The timeline gives the pointer where a beat is, its lanes' box, and a way to show some beats. The shortcuts sheet lists V. |
| `e2e` | `ag-focus.mjs` (54 checks, the `export` kind for its `m2`). The suites' list of the page's writes leaves out `/api/focus` and `/api/point`, as it leaves out naming a step. |
| Docs | The contract; the manual (V, pointers, the three commands, the routes, the events, the busy `compare`); README's In short; e2e/README; gaps; HANDOFF. |

## The tests, and how they failed first

Each was written against the new API with bodies that did nothing, and run before the code (logs in `~/m2-runs/sag/see-logs/` on the build server).

| Where | Tests | Failed first |
|---|---|---|
| `crates/server/src/graph/focus_tests.rs` | `focus_is_memory_only_and_goes_with_the_song`, `a_focus_for_another_song_is_refused`, `a_point_is_not_a_write` | all 3 (`fail-first-server.log`) |
| `crates/server/src/graph/compare_tests.rs` | `while_comparing_an_agents_write_is_busy`, `an_outside_batch_ends_compare`, `any_other_change_ends_compare`; later `v_compares_only_a_change_made_elsewhere` | all 3; the fourth on its own (`fail-first-server-elsewhere.log`) |
| `crates/server/src/lib.rs` | `a_toast_a_tools_socket_heard_still_waits_for_the_page` | yes |
| `crates/server/src/routes_see_tests.rs` | `a_socket_that_falls_behind_hears_lagged`, `the_tools_key_reads_focus_but_cant_post_it_or_compare` | both |
| `apps/m2/tests/watch.rs` | `m2_watch_prints_one_json_line_an_event`, `m2_watch_ends_when_the_app_quits`, `m2_focus_reads_what_the_page_posted`, `m2_point_shows_a_note_and_clears_it` | all 4 (`fail-first-m2.log`) |
| `e2e/ag-focus.mjs` | Bass and two notes are what `m2 focus` says, by key and by note; focus changes no file; a pointer at Kick's header leaves the view and the selection; zoomed in, a pointer at bar 8 waits at the right edge with Show, and only Show moves the view; OK clears it for everyone; while the loop plays, V on an outside save says Before then After, the file follows, Space stops and plays with the pill there; an agent's Tx is busy and `m2 tempo` exits 3, After stays; Esc closes and keeps what plays; V on the musician's own change says what V is for; a file written outside closes the pill, saying so; lint | on the page before this track: 12 of its first 19 checks failed, then it stopped at the missing Show (`fail-first-ag-focus-old-web.log`) |

Written with the code: `socket.rs`'s `base64_is_rfc_4648s`, and `see.rs`'s `a_line_says_its_t_first` and `the_transport_is_said_when_it_changes`. The design lists 10 Rust tests and the browser suite; `any_other_change_ends_compare`, `v_compares_only_a_change_made_elsewhere`, `the_tools_key_reads_focus_but_cant_post_it_or_compare` and `m2_point_shows_a_note_and_clears_it` are added.

## Choices against the design

Where the design was unclear, or didn't fit today's code, this is what was done and why.

1. **No `tungstenite`.** The design says `m2 watch` uses it, "already in `Cargo.lock`"; but neither `m2` nor `m2-server` depends on it, so using it would change `Cargo.lock`, which this track may not do. `m2_server::socket` is a small RFC 6455 client of its own (text messages, a pong for each ping, a close; 127.0.0.1 only), shared by `m2 watch` and the server's tests. Moving to `tungstenite` later is a pull request of its own, with the lock change.
2. **`m2 focus <song>` and `m2 watch <song>` name the song**, as every other `m2` command does (the design wrote `m2 watch [--meters]`). They reach the app that has that song open, through the same checks as a write, and say plainly when it hasn't.
3. **Clips by `{track, lane, id}`**, in `clips` and the notes' clip (the design wrote `{lane, id}`): the track's key and the lane's, as `m2 clips` names a clip. A note's pitch is its MIDI number.
4. **Compare's flips and close, and posting the focus, refuse the tools key now** (403), not in track 4. Each is the musician's (AGENT-3, AGENT-6), and an agent that opened Compare would hold every other agent back.
5. **Compare ends by a rule on the history**, not by a list of write routes: it stays open while undo's newest step (on After) or redo's (on Before) is the one V compared. Any write with the page key, a ⌘Z, a plugin window's save, ends it, and so do the explicit ends (the page's close, its socket, the song, an outside batch). The end is decided in the job, before its reply, so what the caller and the next job read sees it; the step event is told there too (it was told after the job before). Each end is a `compare` event with why: `closed`, `changed`, `page`, `song` or `outside`.
6. **V only on a change made elsewhere, decided by the server.** The browser suite found the page a moment behind when an agent's step had just landed, so V judged "yours" from a stale state. Now the server refuses a compare undo of the musician's own newest step (409 with `newest`, "The newest change is yours"), and the page's V, refused because its step isn't the newest, goes on once with the step the server names. The contract says so (AGENT-3).
7. **Listening keeps the pill open.** The design closes it on "any other action"; but comparing is listening, and Space would close it before you heard the second half. Play and stop, back to the start, the loop, the metronome and the ruler keep it; any other key or press closes it, Esc only closes it. The contract says so.
8. **The pill flips when clicked**, as V does, and has no popover (taste's pills open one): there is nothing more to say about it than its words, which name the step as Undo does ("outside change: the code in tracks/bass.ts").
9. **An agent's undo meets busy first**: while a take records or Compare is open, `m2 undo` is busy, not "nothing to undo".
10. **Undo and redo events are told for every undo and redo**, ⌘Z and `m2 undo` included, with `compare` when V made them, through the thread that tells the steps, so they come after the steps before them.
11. **Pointer targets**: `bars:9` (one bar) besides `bars:9-16`; `clip:` by id only. A knob of a track the dock doesn't show waits at the dock's top; its Show selects that track and shows Device, which is the musician's click. The page's OK clears the pointer for every window and agent (`{clear: true}`).
12. **The page takes `point` and `comparing` from the state only when it connects or another song opens**, not at every read: a read begun before a pointer was posted would otherwise hide it. Events keep both current.
13. **Toasts kept for the page**: a toast is kept while no page's socket listens (it was: while no socket at all). An agent's socket hears it live but never takes one, kept or not.

## Gaps added

Under "Scripts, agents and the command line" in [gaps.md](../../gaps.md): one focus record for the app, not one per window; a note named by pitch, start and length; `m2 watch` reads the transport every quarter second and follows the app, not the song; a pointer at a lane hidden one by one; V compares only the newest change. The track 2 gap "the Compare key doesn't hold agents back yet" is closed.

## Checked

- The crates touched: `m2-server`, `m2-editor` and `m2` (`watch.rs` among them): every test passes, and clippy.
- `e2e/ag-focus.mjs`: 54 of 54, several runs in a row. Every PR-tier browser suite, 4 at a time: 54 of 54 (1,927 checks). Every nightly-tier suite: 22 of 22 (1,322 checks).
- Screenshots, taken by the suite under Xvfb (`AG_SHOT=~/m2-runs/sag/see-shots`): `pointer.png` and `pointer-near.png` (a note beside Kick's header), `pointer-edge.png` (out of view, at the right edge with Show), `pointer-shown.png` (after Show, under bar 8), `compare-before.png`, `compare-before-near.png`, `compare-after.png` and `compare-after-near.png` (the pill). Looking at them changed three things: the note sat on the header it pointed at, and the edge note on the zoom tools (the card's rise animation overrode its placing; an outer box places it now), and "Before:" and the name were two items, which read as a double space.
- The PR tier: below.

## Left for later

- Track 4 (`ag-routes`): the one route table, the 403 for the musician's other controls, `m2 routes` and `m2 call`.
- Track 5 (`ag-listen`): `m2 listen` and G13.
