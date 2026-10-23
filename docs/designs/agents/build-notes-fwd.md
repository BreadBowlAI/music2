# Agents beside the musician: build notes for track 2, `ag-fwd`

**In short**
- Track 2 of the [agents design](design.md) is built on branch `ag-fwd`. With the song open in the app, every `m2` command that writes it hands its edit to the app, which makes it one undo step, the agent's, named by `--as` and `--why`.
- The app has two keys: its window's own, and the tools key it publishes in `~/.music2/session.json`, which it now writes whole. A request with the tools key is an agent's, whatever it says about itself.
- The musician wins every clash. An agent's change to what they hold (a drag, a take, a plugin window not yet saved, unsaved code) is refused, busy; it is checked when it arrives and again just before it is heard, and a change the musician began to hold while it landed is taken back. `m2` waits up to 30 s, then exits 3, and says not to write the file directly instead.
- New commands: `m2 edit` and `m2 write` (a text file's change as one checked edit) and `m2 undo` (an agent takes back only its own newest step).
- The notice shows an agent's change by its name: "Claude: brighter hats · Undo".
- The contract changed first: AGENT-1, AGENT-3, AGENT-4, AGENT-9, AGENT-10, owner decision 10, WRITE-18, WRITE-22's text and Deferred ([architecture.md](../../architecture.md#agents)).
- The review's twelve findings are fixed, each with a test seen failing first: "The review's fixes" below. Checked at `0d3cf13`.
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
- `ci/check.sh --pr` at `a1bd3b9`: `music2 check --pr: PASS` (quick, clippy, rust, suites, renders, repo unchanged; 5 min), nothing passed only on a retry.
- The review's fixes, `ci/check.sh --gates=auto` at `0d3cf13`: `music2 check: PASS`, 1,859 Rust tests, 75/75 browser suites (4 at a time), 12 of 12 renders, the 7 gate runs (they ran: CONTRIBUTING.md changed on this branch), 27 minutes, nothing passed only on a retry. Before it, the touched crates' tests (`m2-song`, `m2-session`, `m2-server`, `m2-editor`, `m2`), clippy, and 12 browser suites by name (`ag-notice`, `undo`, `nx-undo`, `s6-write`, `code`, `notes`, `pl-notes`, `sx-follow`, `attention`, `s8-leases`, `st-record`, `fixes-graph`). Logs: `~/m2-runs/check-fwd.tgz` (this check's; the `--pr` run's before it were there too). No UI text changed: the web is as it was, so no screenshots were taken.

## The review's fixes

The review of `ag-fwd` (twelve findings, F1 to F12, in `~/m2-runs/sag/review-fwd.json` on the build server) is fixed at `0d3cf13`, after its contract (`8fed759`: AGENT-1, AGENT-3, AGENT-10, WRITE-2 and WRITE-22's note). Each finding got a test first, from the skeptic's experiments where there was one, and each was seen failing before its fix (`~/m2-runs/sag/fwd-logs/review-failfirst-1.log` and `review-failfirst-2-m2-forward.log`; after: `review-after-1.log`, `review-after-2.log`).

| Id | What was wrong | The fix | Test, failed first |
|---|---|---|---|
| F1 | An agent's change to one file could end a drag the check matched to another: taking the bass out of `song.ts` ended a knob drag in `tracks/bass.ts`; renaming the track dropped a clip drag. | The check at the settle asks what decides each hold's fate: the knob leases' carry (the same decisions the settle then acts on), `lanes::carried` (now the one function that decides a clip drag's fate, used by the settle and the check), and the plugin instances the new plan keeps. Anything that would end is taken back, whatever file it touched. The check on arrival stays by file. | `an_agents_change_elsewhere_that_would_end_a_knob_drag_is_taken_back`, `an_agents_rename_that_would_end_a_clip_drag_is_taken_back`, `an_agents_change_that_would_drop_a_window_not_saved_yet_is_taken_back` |
| F2 | A drag begun with the tools key at an old revision went through the actor, where the check read the caller off the actor's own thread (always the page): the agent held a lease. | The caller is an explicit value: `register` and `Actor::begin` take it, the job passes the caller it was given, a plugin window's gesture passes the page. `caller::now()` is read only where a request is served; a debug assertion says so on the actor's thread. The `m2-changed` thread reads no caller. | `an_agents_drag_named_at_an_old_revision_is_refused_too` |
| F3 | The check spelled a plugin's state path by hand, so a plugin keyed `aux` (a Windows device name, `aux-.state` on disk) was never held. | `DataPath::plugin_state` names it, as the song spells it; the check calls it. No other hand-built state path was found (the reference songs' generated keys are never device names). | `a_plugin_keyed_aux_is_held_by_its_window` |
| F4 | The settle's check saw the plugin windows as of the job's start: a change made in a window while an agent's state write was on its way was overwritten. | Before the check, `plugin_window::hear_now` drains the host's events and each instance's changed flag, as a tick does (`changed`, shared with the tick); other events wait in `Actor::unheard` for the next tick, in order. | `a_window_change_made_while_an_agents_state_lands_takes_it_back` |
| F5 | Taking a change back didn't bring back the oldest step it pushed out of a full history. | `push_history` returns what it pushed out; the commit's change keeps it beside the redo it ended; `retract` puts both back, `keep_change` drops them. Undo's blobs count them meanwhile. | `retract_brings_back_what_a_full_history_pushed_out` |
| F6 | A forwarded write over axum's 2 MB default failed with "error: null". | `POST /api/tx/commit`, `/api/tx/check`, `/api/graph/fix` and `PUT /api/graph/file` take up to 144 MiB (`TX_BODY`): room for a file's old and new text at what undo keeps, 64 MiB each. `m2` checks the size before it sends and says why; an answer that isn't JSON is said with its status (a 4xx: nothing was changed; anything else: it can't tell). | `a_change_to_a_big_file_lands_in_the_app`, `an_answer_that_isnt_json_is_said_with_its_status` (and `what_the_app_wont_take_is_said_plainly`, written with the code) |
| F7 | An agent's name and words kept bidi and zero-width characters, and a name could read "Changed outside the app". | `words()` drops what a Tx's path may not hold (`m2_song::tx::hidden`, now public: one list); a name whose letters begin "changed outside" or "outside change" is `agent`. The 80-character cap stays. | `an_agents_words_hide_nothing_and_its_name_is_its_own`, `an_agents_name_and_words_cant_pass_for_the_apps_own` |
| F8 | The tools key could cancel the musician's drag and clear their unsaved-code list. | `auth` answers the tools key 403 on `MUSICIANS`: a drag's update, keepalive, commit and cancel (knob, clip, tempo), `/api/graph/preview/cancel`, `/api/graph/unsaved`, `/api/graph/undo/skip`, and a take's stop and cancel. The other controls stay track 4's (gaps). | `the_tools_key_cant_end_the_musicians_drags_or_clear_their_unsaved_code` |
| F9 | `--as` and `--why` were taken from anywhere, so `m2 edit … --new "--why"` failed. | They are read at the end of the command line only; one before the end that isn't another option's value is refused, saying where they go. | `m2_edit_takes_text_that_reads_like_a_flag` |
| F10 | A change taken back at its settle could still leave notices (a state that didn't load, a renderer that didn't build). | The settle's preamble keeps what it would say, and what it would remember having said, until the check passed. | `a_change_taken_back_leaves_no_notice` |
| F11 | Keys were compared with `==`. | `routes::same_key`: a fold over the XORed bytes, through `black_box`, every byte looked at; an empty key is no key. No new dependency; `Cargo.lock` untouched. | `keys_are_compared_whole` (a correctness test, since timing can't be tested in one: it failed first against a stub doing today's `==`, on the empty key) |
| F12 | While the code on disk didn't build, every `m2` write refused, so an agent couldn't fix the song through the app. | Below. | `m2_edit_fixes_a_song_that_doesnt_build_with_the_app_open`, `m2_write_that_leaves_a_song_broken_changes_nothing`, `m2_edit_fixes_a_song_that_doesnt_build_with_the_app_closed` (and `an_agents_save_lands_only_as_written` in core, written with the code) |

`HELD` (what held in the review) still holds: the two keys, `session.json`, retract taking back only the job's own change, the paths, the app quitting mid-call, judged and raw undo, and `--as` never faking the musician. All of their tests pass unchanged.

### F12, in plain words

When the musician saves code with a mistake, the app keeps playing the last version that built. Before this fix, every `m2` write then refused, `m2 edit` included, so an agent asked to "fix the error in bass.ts" had to write the file directly: the app couldn't refuse that, showed it as "Changed outside the app", and `m2 undo` couldn't take it back.

Now, while the code on disk doesn't build, `m2 edit` and `m2 write` of a code file use the same path as the app's code editor: they save the file. With the app open the save goes to the app (`PUT /api/graph/file`, the code editor's own route), so it is checked against what the musician holds, like every agent write. One rule is added for agents: their save is written only if the song then builds. So a fix lands as one undo step, the agent's, named by `--why`, which ⌘Z or `m2 undo` takes back to the musician's text. A save that would leave the song broken writes nothing, and `m2` prints the errors. That keeps "every agent change is a named undo step" true, and the musician's own half-typed text is never replaced by an agent's that doesn't build either.

It fit the design and stayed small because the save path was already there: track 1's waiting saves (`OutsideWas`) and the save's undo needed nothing new. In core, taking in a draft was split into deciding (`settling`, which builds) and doing (`settle`), so a save that must build is judged before anything is written, with one build, not two. `Session::save_built` is that save; a forwarding session sends it to the app (`Forward::save`). Still not possible from `m2` while the code doesn't build (gaps): other writes, a fix that needs two files at once, and a new code file.

### Choices against the leanings

- **F2:** the caller is explicit wherever the actor uses it; the task-local and thread-local stay at the HTTP edge (`auth`, the routes' blocking work and `Graph::call`), which the review found sound. Passing a caller through all of `Graph`'s ~60 methods would be the big change the leaning said to avoid.
- **F1:** a tempo drag stays matched by its file (`data/arrangement.json`), since the arrangement lives only there today (gaps). A plugin window not saved yet was added to what the check asks about, beside drags: an agent's change that drops its plugin would lose it the same way.
- **F7:** names that read as the app's words become `agent` rather than being refused, so an agent's write still lands, under a plain name.
- **F8:** a take's stop and cancel joined the list: a take is something the musician holds (AGENT-3). Starting a take stays track 4's.

## Left for later

- Track 3 (`ag-see`): the focus record, `m2 watch`, `m2 point`, the Compare key, and agents busy while you compare.
- Track 4 (`ag-routes`): the one route table, the 403 for the musician's controls, `m2 routes` and `m2 call`.
- Track 5 (`ag-listen`): `m2 listen` and G13.
