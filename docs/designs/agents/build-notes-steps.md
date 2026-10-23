# Agents beside the musician: build notes for track 1, `ag-steps`

**In short**
- Track 1 of the [agents design](design.md) is built on branch `ag`. Every change the app accepts is one undo step with an id and a source. A text file saved in another program while the song is open is one of them now.
- You see such a change as it lands: a notice with Undo, and an outline on what changed. Undo's name says it was an outside change.
- Undo and redo name the step they mean by its id. A step that isn't the newest any more is refused, saying which is.
- The contract changed first: AGENT-2 and AGENT-5, and the text of WRITE-19 and WRITE-21 ([architecture.md](../../architecture.md#agents)).
- 16 Rust tests and one browser suite (26 checks) were written first and seen failing. Then the code made them pass.
- Left for later: agents as a source of their own (track 2), and a few small things in [gaps.md](../../gaps.md).

## What was built

| Layer | What |
|---|---|
| `core/session` | `TxStep` has an `id` and a `source` (`Source::Page` or `Source::Outside`). Ids come from a counter that never goes back. Undo and redo carry both to the step they push, so a redone step is the same step. `StepName` says the id, the revision the step made, its source and its files. `Session::last_step()` says the newest id given. |
| `core/session`, the watcher's batch | `Session::take_in_batch(snapshot, paths, git)` is `take_in_only`, then one outside step when a revision was accepted. Its inverse writes back each text file the batch changed or removed, guarded by the hash the batch left, and removes each text file it made. No step for a batch git wrote, one that changed or removed a binary file, or one that only made new files under `assets/`. It answers what it did (`take::Outside`). `take_in_only` still records nothing, so the app's own imports make only their Tx's step. |
| `core/session`, the settle rule | A batch remembers whether git was writing at any poll while it settled; `Settle::by_git()` says so after `Ready`. |
| `crates/editor` | `changed.rs`: what a step changed, from the delta between the revision before it and the accepted one. Tracks by module key, lanes as `[module, lane]`, knobs by the view's ids now. |
| `crates/server` | The watcher's job calls `take_in_batch`. A batch git wrote, or one that changed a binary, says so in a message. `undo_name` and `redo_name` carry `step` and `source`; the Edit menu says "Undo outside change: …". `POST /api/graph/undo` and `/redo` take `{session, step}` and answer 409 with `newest` when the step isn't the newest. Every gesture answers with the `step` it made; an undo with the step it took back and the tops after it. After each job that records a step, every window hears a `step` event. |
| `web` | `lib/notice.ts` and the notice in `Toasts.tsx`: one notice for each step not made by the page, a newer one replacing it. The glow: `data-glow` on track headers, clips and knobs (Device and Mix), one rule in `index.css`, ended by the next pointer or key press. Undo's tooltip says the source. The top bar, ⌘Z, a toast's Undo and the notice's Undo send the step id they name. |
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

Section 7 lists 11, 4 and 1 Rust tests: 16, not 15.

## Choices against the design

Where the design was unclear, or didn't fit today's code, this is what was done and why.

1. **Two sources in track 1, not three.** `Source` is `Page` or `Outside`. An agent's source needs the tools key (AGENT-10, track 2): without it, nothing can tell an agent's request from the page's, and AGENT-10 says the source never comes from what a request says about itself. So until track 2, an agent's write through the HTTP API is the page's step, as before. AGENT-2's Today says so. Track 2 adds `Source::Agent(name)`.
2. **The step event is sent after the job, from the actor's loop.** One place covers every job that records a step: routes, the watcher, and lease commits the timers make. The revision before the job is kept for it (an `Arc`, which the engine holds anyway). The event can reach a window just after the edit's own answer.
3. **What changed comes from two revisions, not the commit's delta.** An outside batch has no commit, so `changed` reads `delta_between` (the revision before the job, the accepted one). One path for both kinds of step. Knobs: the values of both revisions' controls, each old knob found in the new revision where its call's first character went. So a knob whose number changed is found, and a call inserted above renumbers it but doesn't lose it.
4. **An outside step's name is the plain name of its file**, as every unnamed step's is (`steps.rs`'s `plain`, which the design cites): "the code in tracks/bass.ts", "Kick notes". Several files: "2 files". The notice says "Changed outside the app: …", Undo "Undo outside change: …". The design's examples used the bare path; `plain` is what the rest of the app says.
5. **The id check is in the server's job, not in core.** `Session::undo` and `redo` stay as they were. The server's undo job reads the session's top step and refuses before undoing, in the same job, so nothing can come between. Without `step`, the routes take the newest, for scripts and for track 2's `m2 undo`.
6. **⌘Z right after your own edit.** The design has the top bar and ⌘Z send the id they show. The page shows a step a moment late: its state is read again after each edit. So a ⌘Z pressed right after an edit (or held down) named the step before, and was refused, which broke three browser suites. Now a refusal whose newest step is the page's own is sent again for that step, since ⌘Z means "take back what I just did". Presses in a row go one after another, each with the tops the last answer gave. A newest step made outside is still said first, as the design wants.
7. **The notice is beside the toasts, not one of them.** It lasts as a toast with an action does (6 s, held while hovered), so a toast never replaces it. It offers Undo while its step is the newest as far as the page knows. When the page hears of a newer step (its own), the notice says "A newer change came after it." and drops Undo: that is its "says why once it isn't". A refusal from the server (a step made in a race) says the server's words in the notice.
8. **A toast's Undo sends its step id**, which every gesture's answer carries now (`step`). The page's revision check in `undoAction` is gone (5.7).
9. **No step, said in a message.** A batch git wrote ("Git changed 3 files of this song. Undo doesn't take that back: git can.") or one that changed a binary ("assets/kick.wav changed outside the app. That can't be undone here.") makes no step, so it is a toast, not the notice. A batch that only made new assets says nothing.
10. **Git is known by its lock.** A batch is git's when git's `index.lock` was fresh at any poll while it settled. That is what the settle rule already waits for, so it needs nothing new.
11. **The glow is one CSS rule.** Components only set `data-glow`; `index.css` draws an accent ring with a soft inner glow on headers and knobs, and an accent outline on each clip of a changed lane. It ends at any pointer or key press in the window, the notice's Undo included.
12. **The import in the browser suite uses Import Audio's file chooser,** not a drop: headless Chrome can't drop a file from disk. Both go to the same route. The server test imports onto a new track, since first-song has no audio lane.
13. **The text before is shared, not copied.** An outside step's inverse holds the old text's `Arc<str>`, counted against undo's 64 MiB like any step's.

## Checked

- The crates touched (`m2-session`, `m2-editor`, `m2-server`): every test passes.
- Browser suites: `ag-notice`, `undo`, `nx-undo`, `notes`, `pl-notes`, `sx-follow`, `s6-write` and `code` pass; every PR-tier suite was run once (the three that ⌘Z right after an edit failed, which choice 6 fixed).
- `ci/check.sh --pr` at `f2e1942`: `music2 check --pr: PASS` (quick, clippy, rust, suites, renders; 8 min, on the Linux build server, with the test plugins).
- Screenshots of the notice and the glow: `~/m2-runs/sag/shots3/` and `~/m2-runs/sag/shots5/` (made by `AG_SHOT=<folder>` with the suite). Looking at them changed two things: the glow got a soft inner glow so it reads as a highlight, not a focus ring, and the notice's "a newer change" words moved into its sentence, since side by side they broke its one line into two columns.

## Left for later

- Agents as a source of their own, by name, with their words: track 2 (two keys, forwarding, `m2 undo`).
- Small things, each a line in [gaps.md](../../gaps.md) under "Scripts, agents and the command line".
