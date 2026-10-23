# Agents beside the musician: build notes for track 1, `ag-steps`

**In short**
- Track 1 of the [agents design](design.md) is built on branch `ag`. Every change the app accepts is one undo step with an id and a source. A text file saved in another program while the song is open is one of them now.
- You see such a change as it lands: a notice with Undo, and an outline on what changed. Undo's name says it was an outside change.
- Undo and redo name the step they mean by its id. A step that isn't the newest any more is refused, saying which is.
- The review's 14 findings are answered (below): undo never takes back your own save with someone else's change, ⌘Z never takes back a step the window didn't make or show, a step undo can't take back can be skipped, and git is known by its index, not only its lock.
- The contract changed first: AGENT-2 and AGENT-5, and the text of WRITE-19 and WRITE-21 ([architecture.md](../../architecture.md#agents)); then again for the review's fixes.
- 16 Rust tests and one browser suite (26 checks) were written first and seen failing. Then the code made them pass. The review added 11 Rust tests and 21 browser checks; the 9 in core and the editor, and the browser checks, were seen failing first (the server's two need its new routes).
- Left for later: agents as a source of their own (track 2), and a few small things in [gaps.md](../../gaps.md).

## What was built

| Layer | What |
|---|---|
| `core/session` | `TxStep` has an `id` and a `source` (`Source::Page` or `Source::Outside`). Ids come from a counter that never goes back. Undo and redo carry both to the step they push, so a redone step is the same step. `StepName` says the id, the revision the step made, its source and its files. `Session::last_step()` says the newest id given. |
| `core/session`, the watcher's batch | `Session::take_in_batch(snapshot, paths, git)` is `take_in_only`, then one outside step when a revision was accepted. It holds only what outside writers changed: `OutsideWas` keeps, for each path a batch changed while it waits, what the session knew there before, and a save takes its file out. Its inverse writes back each such file, guarded by the hash the batch left, and removes each file they made, other than under `assets/`. Each of its paths is checked as a commit would check it (`taken_by`, `spelling`). No step for a batch git wrote, one undo couldn't put every file of back (`Cant`: a binary, a link, a name in other capitals, more than undo keeps), one that only made new files under `assets/`, or in a read-only window. It answers what it did (`take::Outside`). `take_in_only` still records nothing, so the app's own imports make only their Tx's step. |
| `core/session`, skipping | `Session::undo_stuck()`: undo can't take the newest step back, because a file it puts back changed since (`blocked` says `Changed`), or its last undo was refused as a conflict. `skip_undo(id)` takes it out of the history then, writing nothing. |
| `core/session`, the settle rule | A batch remembers whether git's lock was seen at any poll while it settled; `Settle::by_git()` says so after `Ready`. `watch::git_has` then asks git (`git --no-optional-locks status`) whether each changed file is as git's index has it. |
| `crates/editor` | `changed.rs`: what a step changed, from the delta between the revision before it and the one after, pure on the two. Tracks by module key, lanes as `[module, lane]`, knobs by the view's ids now: a knob is its call (found by the bracket that opens it, ID-13) and its argument or slot, so a default written now is the same knob. |
| `crates/server` | The watcher's job calls `take_in_batch`. A batch git wrote, or one that changed a binary, says so in a message. `undo_name` and `redo_name` carry `step` and `source`; the Edit menu says "Undo outside change: …". `POST /api/graph/undo` and `/redo` take `{session, step}` and answer 409 with `newest` when the step isn't the newest, or with `stuck` when undo can't take it back; `POST /api/graph/undo/skip` skips it, and the state says `undo_stuck`. Every gesture answers with the `step` it made (the knob's lease commit, the code save and `/api/tx/commit` too); an undo with the step it took back and the tops after it. After each job that records a step, every window hears a `step` event, with `undoable`; what it changed is worked out on a thread of its own (`m2-changed`), in order. |
| `web` | `lib/notice.ts` and the notice in `Toasts.tsx`: one notice for each step not made by the page, a newer one replacing it, with Undo only while undo can take its step back. The glow: `data-glow` on track headers, clips and knobs (Device and Mix), one rule in `index.css`, ended by the next pointer or key press. Undo's tooltip says the source. The top bar, ⌘Z, a toast's Undo and the notice's Undo send the step id they name (0 when none shows). `lib/history.ts` keeps the ids of the steps this window's edits made (`madeHere`) and names each at once. A stuck step's refusal offers Skip it. |
| Docs | The contract (AGENT-2, AGENT-5, WRITE-19, WRITE-21, the acceptance gates' agents row); the manual (outside undo steps, the notice, the API, and the mistakes of the design's section 8); the e2e README's table. |

## The tests, and how they failed first

All 16 Rust tests of section 7 were written against the new API with empty bodies, and run before the code: every one failed for the reason it tests (the log is `~/m2-runs/sag/logs/failing-first-rust.log` on the build server). The browser suite was run on the code before this track, with the suite in place, and failed 5 of its first 9 checks, then stopped (`~/m2-runs/sag/e2e-fail-first/ag-notice-fix.log`).

| Where | Tests |
|---|---|
| `core/session/src/tx/steps_tests.rs` | `an_outside_edit_that_builds_is_one_undo_step`, `an_outside_batch_that_waits_is_one_step_once_it_builds`, `undoing_an_outside_edit_puts_back_its_text`, `undoing_an_outside_edit_after_a_change_without_a_step_conflicts`, `an_outside_binary_change_makes_no_step`, `a_batch_that_only_adds_assets_makes_no_step`, `a_batch_git_wrote_makes_no_step`, `a_code_editor_save_is_still_one_step`, `the_sessions_own_commit_is_not_an_outside_step`, `a_layout_step_has_its_own_id`, `a_redone_step_keeps_its_id_and_source` |
| `core/session/src/watch/tests.rs` | `git_holds_a_batch_while_it_writes` gained two checks: a batch held for git is git's, and a stale lock isn't git writing |
| `crates/server/src/graph/steps_tests.rs` | `an_import_in_the_app_makes_one_step_and_no_outside_step`, `undo_says_the_source_of_the_newest_step`, `an_undo_for_a_step_that_is_no_longer_newest_changes_nothing`, `a_step_event_names_what_its_delta_changed` |
| `crates/editor/src/changed_tests.rs` | `changed_names_tracks_lanes_and_knobs` |
| `e2e/ag-notice.mjs` (PR tier, 26 checks) | An outside save shows one notice that takes no focus; Bass's header and its knob glow until a click; Undo's tooltip names the outside change; the notice's Undo puts the text back; after a newer change the notice says so and offers no Undo, and ⌘Z takes back both in turn; Import Audio makes one step and no notice; lint |

The review's tests, written first and seen failing (`~/m2-runs/sag/logs/fail-first-review-core.log`, `fail-first-s5.log`, and `fail-first-review-ag-notice.log`: the suite on the web code before the fixes, 8 checks failing):

| Where | Tests |
|---|---|
| `core/session/src/tx/steps_tests.rs` | `an_outside_batch_that_makes_a_waiting_save_build_leaves_the_save_on_undo`, `undoing_an_outside_fix_of_a_waiting_save_puts_the_save_back` (S1); `an_outside_link_makes_no_step_and_older_steps_still_undo`, `an_outside_rename_in_capitals_makes_no_step`, `a_step_whose_file_changed_since_can_be_skipped` (S3); `a_save_while_git_runs_is_one_step` (S4); `a_read_only_window_records_no_outside_step` (S8); `an_outside_plugin_state_is_removed_by_undo` (S11). `a_batch_git_wrote_makes_no_step` and the conflict test use a real git checkout now; `an_outside_binary_change_makes_no_step` counts the other files |
| `crates/editor/src/changed_tests.rs` | `an_inherited_knob_written_is_named` (S5) |
| `crates/server/src/graph/steps_tests.rs` | `the_undo_route_answers_409_naming_the_newest_step` (S9, S13: over HTTP), `a_stuck_undo_answers_409_and_can_be_skipped` (S3, over HTTP); the Tx commit answers with its step (S12); the event's `undoable` |
| `e2e/ag-notice.mjs` (47 checks now) | The notice's Undo sends its step's id; ⌘Z while Undo names an older step (the page's state held back) takes back nothing and names the Tx another program sent (S2); a stuck step: Undo still answers, says which file changed, with Skip it, which writes nothing (S3); a lane changed outside glows on its clips, until a key press (S13) |

Section 7 lists 11, 4 and 1 Rust tests: 16, not 15.

## Choices against the design

Where the design was unclear, or didn't fit today's code, this is what was done and why.

1. **Two sources in track 1, not three.** `Source` is `Page` or `Outside`. An agent's source needs the tools key (AGENT-10, track 2): without it, nothing can tell an agent's request from the page's, and AGENT-10 says the source never comes from what a request says about itself. So until track 2, an agent's write through the HTTP API is the page's step, as before. AGENT-2's Today says so. Track 2 adds `Source::Agent(name)`.
2. **The step event is sent after the job, from the actor's loop.** One place covers every job that records a step: routes, the watcher, and lease commits the timers make. The revision before the job is kept for it (an `Arc`, which the engine holds anyway). A plugin window's commit or autosave, which the loop hears before each job, is told on its own, against the revision before it (review S10). The event can reach a window just after the edit's own answer.
3. **What changed comes from two revisions, not the commit's delta.** An outside batch has no commit, so `changed` reads `delta_between` (the revision before the job, the accepted one). One path for both kinds of step. Knobs: the values of both revisions' controls, each old knob's call found in the new revision where the bracket that opens its arguments went (what names a call across edits, ID-13), and the knob in it by its argument or its slot. So a knob whose number changed is found, a default that is written now (`@ceiling` to `1.ceiling`) too, and a call inserted above renumbers it but doesn't lose it (review S5).
4. **An outside step's name is the plain name of its file**, as every unnamed step's is (`steps.rs`'s `plain`, which the design cites): "the code in tracks/bass.ts", "Kick notes". Several files: "2 files". The notice says "Changed outside the app: …", Undo "Undo outside change: …". The design's examples used the bare path; `plain` is what the rest of the app says.
5. **The id check is in the server's job, not in core.** `Session::undo` and `redo` stay as they were. The server's undo job reads the session's top step and refuses before undoing, in the same job, so nothing can come between. Without `step`, the routes take the newest, for scripts and for track 2's `m2 undo`.
6. **⌘Z right after your own edit.** The design has the top bar and ⌘Z send the id they show. The page shows a step a moment late: its state is read again after each edit. So a ⌘Z pressed right after an edit (or held down) named the step before, and was refused, which broke three browser suites. The page now names each of its own edits' steps the moment the edit answers, with the id the answer carries, and keeps those ids (`madeHere`). A refusal whose newest step is one of them, made a moment before the window showed it, is sent again for that step, since ⌘Z means "take back what I just did". Any other newest step (another window's, an agent's Tx, a plugin window's autosave) is named in the refusal and never taken back (review S2). Presses in a row go one after another, each with the tops the last answer gave. A ⌘Z answered before its own edit's answer reaches the window (the edit's answer is sent first, so this is rare) is refused with words, never wrong.
7. **The notice is beside the toasts, not one of them.** It lasts as a toast with an action does (6 s, held while hovered), so a toast never replaces it. It offers Undo while its step is the newest as far as the page knows and undo can take it back (the event's `undoable`, then the state's `can_undo`), never in a read-only window (review S3, S8). When the page hears of a newer step (its own), the notice says "A newer change came after it." and drops Undo: that is its "says why once it isn't". A refusal from the server (a step made in a race) says the server's words in the notice.
8. **A toast's Undo sends its step id**, which every gesture's answer carries now (`step`; the knob's lease commit, the code save and `/api/tx/commit` since the review, S12). The page's revision check in `undoAction` is gone (5.7).
9. **No step, said in a message.** A batch git wrote ("Git changed 3 files of this song. Undo doesn't take that back: git can.") or one undo couldn't put every file of back ("assets/kick.wav and 1 other file changed outside the app. That can't be undone here.", with ": tracks/x.ts is a link", ": another file has the name … in other capitals" or ": it's more than undo keeps") makes no step, so it is a toast, not the notice. A batch that only made new assets says nothing.
10. **Git is known by its lock and its index.** A batch is git's when git's `index.lock` was seen at a poll while it settled, and `git status` (with `--no-optional-locks`, so it takes no lock the watcher would see) says each changed file is as git's index has it, or unmerged with git's conflict marks. A `git status` alone takes the lock, so the lock alone said too much (review S4).
11. **The glow is one CSS rule.** Components only set `data-glow`; `index.css` draws an accent ring with a soft inner glow on headers and knobs, and an accent outline on each clip of a changed lane. It ends at any pointer or key press in the window, the notice's Undo included.
12. **The import in the browser suite uses Import Audio's file chooser,** not a drop: headless Chrome can't drop a file from disk. Both go to the same route. The server test imports onto a new track, since first-song has no audio lane.
13. **The text before is shared, not copied.** An outside step's inverse holds the old text's `Arc<str>`, counted against undo's 64 MiB like any step's. One that alone is more than 64 MiB makes no step, since it would push every other step out (review S7).
14. **An outside step's "before" is what the app last knew on disk** (review S1). Two ways were weighed: make a save that didn't build its own step when an outside batch makes it build, or build the outside step from what outside writers changed only. The second is simpler and right in every order: the session keeps, for each path an outside batch changed while it waits, its entry before the first such batch (`OutsideWas`), and a save takes its file out. A path that went into a revision since by any other way (a save or an undo that made the draft build) is dropped, so it rides along as before. The first way would also need the save's step to undo after the draft broke again, which the draft rule refuses.
15. **A stuck step is skipped, never dropped by itself** (review S3). A step whose file changed since in a way that made no step (git's checkout, a batch with a binary) can't apply until that change is undone outside. Undo still answers then: it says which file changed, with Skip it, which takes the step out of the history and writes nothing, so Undo reaches the steps before it. The session allows that only for the newest step, and only while undo would be refused (`undo_stuck`). A stuck redo stays until the next change clears redo (gaps).
16. **"What changed" runs on a thread of its own** (review S6): 12 ms a step on the 50-track song in release, 10.8 ms of it the delta between the two revisions. That is above the 2 ms asked, so it left the actor: the actor sends the two revisions (shared, not copied) and the event's words to `m2-changed`, which sends the events in order. The event can reach a window a few milliseconds later.
17. **Git by its index alone, not also by HEAD moving** (review S4). Every git command that writes files (checkout, reset, restore, pull, merge, rebase) leaves them as its index has them, or unmerged with conflict marks, so the index says it for all of them; reading HEAD and its ref (packed refs, worktrees) would be more code that decides nothing more. `git status` of the changed paths only, so it is quick, and only when the lock was seen.


## The review's findings

The skeptic's review (`~/m2-runs/sag/review-steps.json`): 4 majors, 10 minors. The guiding rule for the majors: undo never loses the musician's work, never takes back a change they weren't shown, and a step that can't be undone never blocks the steps under it.

| # | Done | How |
|---|---|---|
| S1 major | fixed | Outside steps hold only what outside writers changed, from what the app last knew there (choice 14) |
| S2 major | fixed | ⌘Z's retry only for this window's own steps, named at once (choice 6) |
| S3 major | fixed | Checked at take-in as a commit checks; a stuck step can be skipped; the notice and Undo offer only an undo that can work (choices 9, 15, 7) |
| S4 major | fixed | Git's lock and git's index (choices 10, 17) |
| S5 | fixed | Knobs by the call's opening bracket and slot (choice 3); a rewrite of more than 500 lines: gaps |
| S6 | fixed | Off the actor's thread (choice 16) |
| S7 | partly | A step over 64 MiB is not recorded (choice 13); merging autosaves: gaps. Redo ending at a new outside step stays: every new change ends redo |
| S8 | fixed | A read-only window records no outside step, and its notice offers no Undo |
| S9 | fixed | No step shown sends 0, which the server names the newest for |
| S10 | fixed | Plugin windows' steps told on their own (choice 2); no test: it needs a plugin's window |
| S11 | fixed | A plugin state made outside is removed by undo; the message counts the other files |
| S12 | fixed | `/api/tx/commit`, the knob's lease commit and the code save answer with `step` |
| S13 | fixed | The route's 409 bodies over HTTP; the notice's Undo sends its id; the lane glow and its end on a key, without `AG_SHOT` |
| S14 | fixed | The manual's undo text; the test's comment says why its change makes no step |

## Checked

- The crates touched (`m2-session`, `m2-editor`, `m2-server`): every test passes, and clippy.
- Browser suites: `ag-notice`, `undo`, `nx-undo`, `notes`, `pl-notes`, `sx-follow`, `s6-write`, `code` and `attention` pass (`ag-notice` and `attention` twice in a row).
- `ci/check.sh --pr` at `f2e1942`, before the review: `music2 check --pr: PASS` (quick, clippy, rust, suites, renders; 8 min).
- `ci/check.sh --gates=auto` at `6a2be20`, the review's fixes: every row passed but two. `attention`, a nightly suite the PR tier doesn't run, still said an outside edit makes no undo step; `36b015d` brings it up to date. And `advisories`, below.
- `ci/check.sh --gates=auto` at `36b015d`: `music2 check: FAIL (1 of 19 checks)`, 26 min, nothing passed only on a retry. Passed: 1,799 Rust tests, 75/75 browser suites, 12 of 12 renders, the 7 gate runs, clippy, the web build, the Mac lint. The one failure is `advisories`: RUSTSEC-2026-0386, a new advisory against tokio 1.53.2 (fixed in 1.53.3). `main` fails it the same way: it needs `cargo update -p tokio` on `main`, which this track doesn't do. Log: `~/m2-runs/logs/check-ag2-1023-190941-303978.log`.
- Screenshots of the notice and the glow: `~/m2-runs/sag/shots3/` and `~/m2-runs/sag/shots5/`; of a stuck step's Skip it: `~/m2-runs/sag/shots6/skip-it.png` (made by `AG_SHOT=<folder>` with the suite). Looking at the first ones changed two things: the glow got a soft inner glow so it reads as a highlight, not a focus ring, and the notice's "a newer change" words moved into its sentence, since side by side they broke its one line into two columns.

## Left for later

- Agents as a source of their own, by name, with their words: track 2 (two keys, forwarding, `m2 undo`).
- Small things, each a line in [gaps.md](../../gaps.md) under "Scripts, agents and the command line".
