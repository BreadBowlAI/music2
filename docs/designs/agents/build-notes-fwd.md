# Agents beside the musician: build notes for track 2, `ag-fwd`

**In short**
- Track 2 of the [agents design](design.md) is built on branch `ag-fwd`. With the song open in the app, every `m2` command that writes it hands its edit to the app, which makes it one undo step, the agent's, named by `--as` and `--why`.
- The app has two keys: its window's own, and the tools key it publishes in `~/.music2/session.json`, which it now writes whole. A request with the tools key is an agent's, whatever it says about itself.
- The musician wins every clash. An agent's change to what they hold (a drag, a take, a plugin window not yet saved, unsaved code) is refused, busy; it is checked when it arrives and again just before it is heard, and a change the musician began to hold while it landed is taken back. `m2` waits up to 30 s, then exits 3, and says not to write the file directly instead.
- New commands: `m2 edit` and `m2 write` (a text file's change as one checked edit) and `m2 undo` (an agent takes back only its own newest step).
- The notice shows an agent's change by its name: "Claude: brighter hats · Undo".
- The contract changed first: AGENT-1, AGENT-3, AGENT-4, AGENT-9, AGENT-10, owner decision 10, WRITE-18, WRITE-22's text and Deferred ([architecture.md](../../architecture.md#agents)).
- 40 Rust tests and 6 browser checks were written first. 39 of the tests and 3 of the checks were seen failing; one test passed before its code, for a reason said below. Small loose ends are in [gaps.md](../../gaps.md).

## What was built

| Layer | What |
|---|---|
| `core/session` | `Source::Agent(name)`. `Session::act_as(by, why)`: who the steps recorded now are by, and their name. `Session::forward_to(Box<dyn Forward>)` (`forward.rs`): a read-only session's `commit_with` checks, judges and asserts as a writer's does, then sends the Tx with its own expectation, the hash of that check (`seen`), the files under `assets/` it took in since, and what each target holds once it landed; then it takes the touched files in from disk. `commit` in such a session is `commit_with` with no assertion; undo and redo are refused. `Session::retract()`: takes back the last commit, undo or redo, and leaves undo and redo as they were; `last_change()` names its files, `keep_change()` forgets it. |
| `core/os` | `process::alive(pid)`: `m2` waits for a slow app while its process lives. |
| `crates/server` | `caller.rs`: `Caller::Page` or `Caller::Agent { name, why }`. `auth` decides it by the key alone and serves the request as it (a task-local); a route's blocking work and `Graph::call` carry it to the actor's job, which acts as it. `busy.rs`: what the musician holds (knob, lane and tempo leases, plugin windows not yet saved, unsaved code, a take) and the check. The actor checks on arrival (`Actor::arrive`) and at the start of the settle that follows an agent's change, under the control plane's lock; a change met there is retracted, the settle plays what was there, and the job answers 423 `busy` with `taken_back`. An agent's job runs without the preview hook. Agents can't begin a drag. `POST /api/tx/commit` takes `seen`, `assets`, `as` and `why`; `/api/graph/fix` takes `as` and `why`; `/api/graph/undo` takes `as`. An agent's undo takes back only its own newest step (`NotTheirs`, 409). `undo_name`, `redo_name` and the `step` event carry `by`; every edit answers with its `undo_name`. `ServeOptions` has `token` (the tools key) and `page_token`. |
| `apps/music2-desktop` | Two keys per launch: the page's in the window's start script, the tools key in `session.json`, written to a new file with mode 0600 and renamed over the old one. |
| `crates/server/examples/headless.rs` | `<token>` is the page's key; it makes the tools key, and writes both into the session file (whole), `page_token` for the browser suites. |
| `apps/m2` | `app.rs`: the open app. `locked()` asks who holds the lock; when it is `music2`, it reaches it through `session.json` (same pid, same song open), opens the song read-only (no wait for the lock), lowers its priority, and forwards. Busy, refusals, and an app that went away are said in words; busy exits 3. `--as`, `--why` and `M2_AS` are taken off the command line first. `m2 fix` posts the app's own fix; `bounce --place` reaches the app before it renders and reads the song after. `text.rs`: `m2 edit` and `m2 write`. `m2 undo`. |
| `web` | The notice says an agent's change by its name; Undo's tooltip and the Edit menu say "Undo Claude: brighter hats". A notice counts steps in a row from one author only. |
| `e2e` | The suites give their pages `page_token`; `ag-notice` checks an agent's notice (53 checks). |
| Docs | The contract; the manual (agents' changes, the busy refusal, the new commands, the two keys, the routes, "Or just edit the files"); README's In short; e2e/README; gaps. |

## The tests, and how they failed first

Each was written against the new API, with bodies that did nothing, and run before the code (logs in `~/m2-runs/sag/fwd-logs/` on the build server).

| Where | Tests | Failed first |
|---|---|---|
| `core/session/src/tx/forward_tests.rs` | `a_forwarding_session_sends_its_checked_tx_with_seen`, `a_forwarded_tx_keeps_its_own_expectation`, `after_forwarding_the_session_reads_what_landed`, `a_forwarding_session_refuses_undo`, `retract_leaves_undo_and_redo_as_they_were` | all 5 (`fail-first-core.log`) |
| `crates/server/src/graph/busy_tests.rs` | `the_tools_key_makes_a_request_an_agents`, `a_write_with_the_page_key_is_the_musicians`, `a_forwarded_tx_whose_delta_isnt_seen_is_refused`, `an_agents_step_is_judged_when_undone`, `a_drag_begun_while_an_agents_change_compiles_goes_on_and_the_change_is_taken_back`, `unsaved_text_posted_while_an_agents_change_lands_takes_it_back`, `an_agents_change_is_not_heard_before_its_settle`, `an_agents_undo_is_checked_like_its_writes`, `a_forwarded_import_plays_its_asset_from_the_start_and_makes_one_step` | 8 of 9 (`fail-first-server.log`) |
| `crates/server/src/graph/record_tests.rs` | `while_a_take_records_every_agents_write_is_busy` | yes |
| `crates/server/src/graph/plugin_window_tests.rs` | `a_plugin_window_not_yet_saved_makes_its_state_busy` | yes |
| `apps/music2-desktop/src/main.rs` | `the_session_file_is_private_even_when_one_was_there` gained: the file is written whole, never over the old one | yes (`fail-first-desktop.log`) |
| `apps/m2/tests/forward.rs` | `a_write_while_the_app_holds_the_lock_lands_in_the_app`, `every_write_command_forwards` (25 rows, 5.2's list with `edit` and `write`), `the_step_is_named_by_as_and_why`, `a_forwarded_write_doesnt_wait_for_the_lock`, `a_song_held_by_an_app_m2_cant_reach_fails_naming_it`, `a_write_on_a_file_with_a_knob_drag_is_busy_and_the_drag_goes_on`, `a_write_to_a_file_with_unsaved_editor_text_is_busy`, `busy_exits_3_and_says_not_to_write_the_file`, `m2_fix_with_the_app_open_applies_the_apps_fix`, `bounce_place_with_the_app_open_reads_the_song_after_its_render`, `the_app_quitting_before_the_commit_changes_nothing`, `the_app_killed_mid_commit_names_its_fixes`, `the_app_quitting_after_the_commit_says_it_landed`, `the_app_quitting_after_another_change_says_it_cant_tell`, `m2_undo_takes_back_only_its_own_step`, `m2_undo_with_no_name_never_takes_back_the_musicians_step`, `m2_undo_with_the_app_closed_says_undo_lives_in_the_app` | all 17 (`fail-first-m2.log`) |
| `apps/m2/tests/edit.rs` | `m2_edit_replaces_text_found_once_as_one_named_step`, `m2_edit_refuses_text_found_twice`, `m2_edit_on_a_stale_file_reads_again_once`, `m2_write_makes_a_new_file_as_one_step`, `m2_write_refuses_a_binary_file`, `m2_edit_works_with_the_app_closed` | all 6 |
| `apps/m2/tests/lock.rs` | `a_write_while_another_process_holds_the_lock_fails_and_changes_nothing` keeps its meaning: its holder is `music2` with no app behind it, so `m2` can't reach it and fails, naming it | (kept passing) |
| `e2e/ag-notice.mjs` | an agent's Tx: its notice names it, with Undo; Undo's tooltip names it; Bass glows; the notice's Undo takes it back | 3 of the 6 on track 1's web code (`fail-first-ag-notice-old-web.log`) |

`an_agents_step_is_judged_when_undone` passed before its code: the server then ignored `seen`, so the step was judged anyway. It guards the other way to build it, the one revision 2 of the design rejected (the review's m2): turning `seen` into `Expect::Seen` would make every agent step raw, and the test would fail. `caller.rs`'s `an_agents_words_are_plain_and_short` was written with the code it tests.

Every `m2()` helper in `apps/m2/tests` now sets `HOME` to `$TMPDIR/m2-tests-no-app`, which nothing writes, so a real app's `session.json` is never read (the review's m7).

## Choices against the design

Where the design was unclear, or didn't fit today's code, this is what was done and why.

1. **The `Forward` trait takes one `Sent`**: the Tx, `seen`, the assets, and what each target holds once it landed (`after`). The design's trait had the first three. `m2` needs the fourth, from its own check, to tell whether a commit landed when the app went away mid-call (5.2's table).
2. **The caller travels by a task-local and a thread-local**, not by a new argument to each of the ~60 `Graph` methods: `auth` serves the request as its caller, every route's blocking work (`caller::blocking`) runs as it, and `Graph::call` hands it to the actor's job. Tests act as an agent with `caller::as_caller`.
3. **An agent's name and words go in the body** for the Tx commit (as the design says), the fix and the undo; `auth` also reads `x-m2-as` and `x-m2-why` for every route, for track 4's `m2 call`. `m2` sends the body: a header can't carry every name.
4. **On arrival, a gesture route's files aren't known**: it is planned first. So there the check meets only a take; a Tx, an undo, a fix, a code save and `set` (its knob's file) are checked by their files. The settle's check is the same either way, after the compile; a drag found there takes the change back. `m2` always sends a Tx, so it is refused at once (gaps).
5. **What the settle checks is the job's own change**: each job starts by forgetting the last change (`keep_change`), and the settle asks for the files the newest change touched (`last_change`, from its undo step). So only a change this agent's job made can be taken back, and only once.
6. **Retract**: a commit is undone and the redo it ended comes back; an undo is redone; a redo undone. Either way the step keeps its id, and the files read as before, in a new revision, to which the drag is carried. When it can't be retracted (an outside write came between), it stays, and the drag meets it as an outside write (a message on stderr).
7. **`m2 edit` and `m2 write` choose the expectation**: a lane file's change is confined to its lane, so its undo is judged and heard at once; anything else is committed on the hash its check showed, as `m2 tx --seen` is, since an edit of code may change whatever reaches it. Its undo is raw: heard at its settle (5.12 allows it, as for an outside step).
8. **Busy exits 3 by a flag the forwarding sets**, not a new `Refusal` variant: revision 3 of the design dropped `Refusal::Busy` from core.
9. **Words when taken back**: "nothing was changed: this was taken back as it landed, because you're dragging Bass cutoff in the app." The server says what the musician holds; `m2` says it was taken back.
10. **`M2_BUSY_WAIT`** (seconds) shortens the 30 s wait. Only the tests set it.
11. **Agents can't begin a drag now** (AGENT-3: agents don't hold leases): the knob, lane and tempo drags refuse the tools key. The 403 for every one of the musician's controls stays track 4's (AGENT-11; gaps).
12. **Every edit answers with its `undo_name`**, so `m2` says "in the open app: one undo step, Claude: brighter hats" without asking again.
13. **A notice counts one author's steps**: "Claude: 2 changes" right after an outside change said a count across two authors. The browser suite found it.
14. **`bounce --place` reaches the app before it renders** and reads the song after: an app it can't reach still fails before anything is written, as WRITE-18 says.
15. **`a_forwarded_write_doesnt_wait_for_the_lock` is timed**: no file or answer tells a wait for the lock apart from a slow start. It is in `ci/nightly-tests.txt` (speed), unchanged, as the rules say.
16. **The plugin window's test waits for the window's change to be heard**: it reaches the app as a host event, a moment after the block that made it.

## Gaps added

Under "Scripts, agents and the command line" in [gaps.md](../../gaps.md): `m2` can't edit a song whose code doesn't build (the review's m8); a knob drag holds its whole file for agents; a gesture route is checked on arrival only against a take; one list of unsaved code for the app (the review's m5); the musician's controls are still open to the tools key until track 4; the Compare key doesn't hold agents back until track 3. The track 1 gap "an agent's write is the page's step" is closed.

## Checked

- The crates touched: every test passes (`m2-session` 188, `m2-server` 436, every test of `m2`, `music2-desktop`'s session test), and clippy on the whole workspace.
- `e2e/ag-notice.mjs`: 53 of 53. Screenshots of the agent's notice: `~/m2-runs/sag/fwd-shots/agent-notice.png` and `agent-notice-near.png` (made by `AG_SHOT=<folder>` with the suite).
- Every browser suite, 4 at a time (`e2e/run.mjs all`): 75 of 75, `live3b` and `ph-window` with the test plugins.
- `ci/check.sh --pr` at `c9b3429` failed `suites`: `nx-settings` gave its first page no key (the suites read `page_token` now), and `st-stems` passed only on a retry (its poll looked at a part file the export renamed meanwhile). Both fixed at `f2610f6`, the cause of each.
- `ci/check.sh --pr` at `a1bd3b9`: `music2 check --pr: PASS` (quick, clippy, rust, suites, renders, repo unchanged; 5 min), nothing passed only on a retry. Logs: `~/m2-runs/check-fwd.tgz`.

## Left for later

- Track 3 (`ag-see`): the focus record, `m2 watch`, `m2 point`, the Compare key, and agents busy while you compare.
- Track 4 (`ag-routes`): the one route table, the 403 for the musician's controls, `m2 routes` and `m2 call`.
- Track 5 (`ag-listen`): `m2 listen` and G13.
