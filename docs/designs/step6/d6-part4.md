---

## 19. Gates and how each is measured

Tests name the rule ids they check in a comment, as every test does. Child-process tests re-run their own
test binary (`std::env::current_exe()` with `--exact <name> --nocapture` and an env var naming the child's
part); the child does its part and exits, or aborts, and the parent asserts. Every such test lives in a
`cfg(test)` module of its crate or in a test binary of its own, so `cargo test --workspace` runs it with
no feature flag (R20).

| Gate | Measurement |
|---|---|
| The 7 gestures and step 1's fixes are ported; their tests pass with the same meaning | Every test in `edit/tests.rs`, `restricted_tests.rs`, `insert_tests.rs`, `cable_tests.rs`, `effect_tests.rs`, `relink_tests.rs`, `lanes.rs`'s tests, `view_tests.rs`, `gate5_tests.rs` and the server's `graph/tests.rs` passes, moved to `crates/editor` (or left in the server) with only `use m2_editor::Gestures`, outcome types and explicit `refresh()` calls where a test relied on a gesture's implicit one (20.1) changed. The lease tests that a broken file elsewhere makes Pending (`a_commit_while_the_draft_is_broken_is_saved_once_it_compiles`, `a_pending_commit_is_taken_over_by_its_own_window_only`, `a_release_during_a_broken_save_plays_on_and_saves_once_fixed`) keep their meaning, since the draft rule stays (R25). The tests whose meaning the contract changes are listed in 20.1 with what replaces them. `apps/m2/tests/relink.rs` passes against the Tx-based commands. |
| Property tests over Tx (`core/session/src/tx/props.rs`, seeded `Xorshift` as the carry test has, 500 cases each, the seed printed on failure) | **(a) bytes outside the declared ranges are unchanged:** random base texts (ASCII, `é`, `ベース`, an emoji, CRLF lines), random sorted disjoint edits given as bytes or as `LineCol`; `apply` gives the same text for both forms, and every byte outside the edits is where the mapping says. Through `commit` on `examples/first-song`: random knob literals (`Values`), comment insertions anywhere a comment may go (`Confined` with their points), effect wraps (`Confined` with the wrapped call), and the commit's file differs from the base only inside the declared ranges. **(b) the inverse restores the original:** random Tx on a fixture copy mixing literal edits, comment insertions, `Clips` (random put, change and delete of clips with random notes), `Write` of an unread lane file, `Remove` of one, a folder `Rename` of unclaimed data and back; after each commit, `undo` leaves a tree whose every file (path list and bytes, `.music2/` excluded) equals the tree before, and `redo` the tree after. **(c) hostile paths and ids are refused:** generated `..`, `/abs`, `C:\`, `a//b`, `.`, `.music2/lock`, `node_modules/x.ts`, `data/aux/x.json` (unsuffixed), NUL, U+202E, a `to` that exists, ids failing the pattern, `Payload::Local` outside `.music2/tx/` or with a wrong hash, overlapping op paths (`Rename data/kick → data/kicks` with a `Write` or `Remove` under either end, a `Clips` op and a `Write` of one lane file, R15), `"ranges": {"song.ts": []}` with a `Write` of `song.ts` (R23), a `TextEdit` or `Clips` on a file that isn't UTF-8 (R24), plus on-disk traps: a target that is a symbolic link out of the project, a parent folder that is one, a name in other capitals. Each is `Rejected` with `tx.path` or `tx.invalid` and the project tree (hashed whole) is unchanged. **(d) `check` then `commit`:** for every valid Tx of (b), `check`'s hash equals `commit`'s, and the files on disk after the commit equal the candidate's files `check` computed; a second run inserts a random comment into a touched file between them, outside the Tx's ranges, and the hashes are still equal (R17). Also: the data path equals the full compile on every lane edit of (b) (data, problems, order). |
| Inserting one unconnected call at the top of `song.ts` gives a delta of exactly that call's nodes, with the same hash in two processes | `delta_of_an_inserted_call` (`core/session/src/tx/delta_tests.rs`): 5.6's Tx, `check`ed on a copy of `examples/first-song`; the delta has `modules: [added probe]`, `added` = every node of module `probe` and only those, and every other list empty. A second process (the test binary with `M2_DELTA_OF=<copy>`) checks the same Tx on its own copy and prints the hash; the two are equal (hash maps are seeded per process, so an order bug would show). Also `delta_names_are_unique_on_every_fixture`, `a_value_edit_is_one_value_change`, `an_inserted_argument_is_a_value_change` (WRITE-6), `a_wrap_keeps_the_wrapped_calls_names` (two insertions: `added` = the effect's nodes, `rewired` = the readers, `removed` empty), `a_second_read_of_a_lane_renames_nothing` and `seen_survives_an_unrelated_edit_in_the_same_file` (R17). |
| A lane file changed by an outside writer is accepted without running QuickJS | `m2_script::builds()`, a process-wide counter of QuickJS build runs (the only `core/script` addition). `an_outside_lane_write_takes_the_data_path` (core): open, read the counter, rewrite `data/kick/hits.json` by hand, `refresh()`; the counter is unchanged, `code_rev` unchanged, `data_rev` moved, the delta's `lanes` is `[kick/hits]` and nothing else. The same through the server (`an_outside_lane_write_is_swapped`): the renderer's generation is unchanged and a `SwapLane` was applied. |
| A lane file written in two halves never produces a revision in which that lane is empty | Unit tests of `Settle` with a fake clock (a change every 40 ms keeps it settling; quiet 150 ms with two equal polls makes it ready; a fresh `index.lock` holds it). Server test `a_lane_written_in_two_halves_never_plays_empty`: a hook records every accepted revision's data for `kick/hits`; a thread truncates the file, writes its first half, sleeps 60 ms, writes the rest (and a second run sleeps 400 ms, longer than the quiet window). No recorded revision has the lane empty; the 60 ms run makes exactly one revision; the 400 ms run's middle revision keeps the last data with `lane.invalid` (FMT-9). Also `a_broken_save_and_a_lane_move_in_one_batch_keep_the_lane` (R3): one batch renames a lane key in `song.ts` with a syntax error and moves its file; no revision is accepted, the lane plays its accepted data, and fixing the syntax takes both in as one revision. |
| Undoing a clip edit after an outside change to that clip is a `Conflict` naming the lane and the clip | `undoing_a_clip_after_its_outside_change_conflicts` (`crates/editor`): `write_notes` moves clip `a`; an outside writer rewrites the lane with clip `a`'s notes changed; `refresh`; `can_undo()` is false and `undo()` is `Changed([{file: "data/kick/hits.json", clip: Some("a")}])` whose message names "kick › hits" and the clip. A second case changes only clip `b`: undo succeeds and clip `b` keeps the outside change. |
| A CLI write while the app holds the lock fails, changes nothing and names the holder; `m2 bounce` without `--place` succeeds | `apps/m2/tests/lock.rs`: the test binary, re-run with `M2_HOLD=<copy>`, opens a writer `Session::open_as(…, "music2")`, prints `ready` and waits on stdin. Then `m2 set`, `connect`, `relink`, `delete-data`, `fix`, `plugin-state`, `tx` and `bounce --place` each exit 1, print "nothing was changed", "music2" and the holder's pid (and, for `--place`, the lane file), and the project tree's hash is unchanged after each. `m2 bounce <copy> k --seconds 0.1` exits 0 and creates `assets/k.wav` and its provenance; `m2 check`, `graph` and `render` exit 0. After the holder exits, `m2 set` succeeds. |
| Killing the process at every commit step and reopening offers `tx.interrupted`; both Finish and Discard leave a project that opens | `core/session/src/tx/kill_tests.rs` (6a.5): two Tx, one multi-file (a code `TextEdit`, a new lane file, a `Remove`, a folder `Rename`) and one with a 4 MB state blob. For each disk call `n` of the commit (every one is a kill point), on two fresh copies: the child commits and aborts at call `n`; the parent checks it died of `SIGABRT`, then opens. Killed before the journal is written: nothing is offered and nothing changed. Killed after it and while some target is still at its before hash: `tx.interrupted` with DiscardTx, and FinishTx exactly when every staged file was complete; copy A commits FinishTx (every target holds its after bytes, no journal is left, `Session::open` succeeds), copy B commits DiscardTx (every target holds its before bytes, the same). Killed after the last rename: every target is at its after hash, so the reopen tidies the journal silently and opens holding the commit (R7; C9 asks the owner to read the gate this way). Also: a target changed by hand before reopening sets the journal aside, with no fix and no project file touched; a completed commit's journal is gone once the next commit's barrier has run; two journals sharing a target (an older completed one and a newer interrupted one) report only the newer (R7); a journal that doesn't parse is set aside and reported (R8); a read-only open of an interrupted project reports `tx.interrupted` and changes nothing (R16). |
| A project with broken code opens, the code editor saves, and playback starts when the code compiles | Core: `a_broken_project_opens_silent` (`code_rev == 0`, `draft_errors` names the file and line, the view and timeline are empty, a data Tx is `Rejected`, `save` of the fixed text gives `code_rev == 2`). Server: `a_broken_project_opens_and_plays_once_saved_fixed`: `Graph::open` succeeds with diagnostics and nothing loaded into the driven player; `PUT /api/graph/file` with broken text is saved and stays silent; the fixed text loads a renderer and the driven player's next blocks aren't silent. The old `a_project_that_does_not_compile_says_where_without_its_folder` becomes this test's diagnostics check (20.1). |
| `write_whole` is private, and the guess is gone | `the_write_bypasses_are_closed` (a grep test in `crates/editor`): no `pub fn write_whole` and no `pub(crate) fn write_whole` in `core/`; no `fn single(` and no `revision == before + 1` in `crates/server/src`; no `std::fs::write` in `apps/m2/src`; no `refresh()` call in `crates/editor/src` or in the actor's gesture paths (R2). The crate-graph test (13.1). |
| PLANE-5, and the data plane's 20 ms | `m2-gates --server tx-data` (G10), recorded on the reference Mac with p16 and p50 and the driven player: 200 piano-roll writes (one note moved, a `Clips` Tx through the server route), measuring `commit()`'s call to the `Applied` stamp of its `SwapLane`, p50/p95/max, and separately the barrier's `F_FULLFSYNC` time. Run twice (R18): on an idle disk, and with a concurrent writer (a child process writing and `fsync`ing 1 GB in 4 MB chunks in a loop, as a bounce or an export would). Also the watcher's stat walk on p50 with a 5,000-file `assets/` (8.2). Recorded in PLANE-5's Today. If p95 is over 20 ms in either run, the owner chooses among C20's options; the sync is never skipped. |
| Kept: renders, RENDER-33, the allocation trap, step 3 and 5 gates | Offline renders of the 4 pure fixtures match `scratchpad/baseline/SHA256SUMS`; `core/play/tests/bit_identity.rs`, `no_alloc.rs`, `no_blocking.rs`; `fixture_renders_change_only_with_stds_version`; `m2-gates` G1–G9 re-run (G8/G9 in particular, since commits now journal). |

### 19.1 Other tests this step adds

- `core/song`: Tx JSON round trips and refusals (unknown field, two payloads, a bad pin, `"whole"` and
  `[]`), `jcs` against RFC 8785's vectors, `dir_hash` against a hand-computed value (the TS tools' fixture
  later), `Snapshot::sha` of a file that isn't UTF-8 (the raw bytes' hash).
- `core/session/src/text.rs`: points and spans at every boundary (an insertion at a range's start and end,
  a replacement that covers a range's first character, a wrap), the strict carry unchanged by them.
- `core/session/src/tx/`: `Clips` merge and conflict per clip, and a `Conflict` on a put whose id occurs
  twice in the current file (R6); `TextEdit` carry past an outside change above and below, and `Conflict`
  on one inside; `WithText` with a wrong hash; a base found only in the store; `Expect::Values` with one
  literal in two slots; `an_added_effect_is_confined` and an import insertion at a declared point (R5);
  `Confined`'s rule 5 (a stray arrangement write is refused); `data.claimed` on an added track whose key
  matches an orphan folder, and none on a relink; a fix's id differs for two contents of one lane file
  (R19).
- `core/session/src/tx/` baseline (R2): `an_outside_literal_edit_then_a_knob_commit_keeps_both` (an outside
  edit of another literal in `song.ts`, not taken in, then a `Values` commit in the same file: committed,
  both changes on disk, the delta is the knob's alone); the same with an outside edit that doesn't compile:
  `Conflict` with its `why`; a data Tx on a waiting path: `Conflict` (R3).
- `core/session/src/tx/stage.rs` (R1, R8, R9, R10): with a recording `Disk`, the order of calls: every
  journal, staged file and blob write and their `fsync`s, then the `fsync`s of `<jid>/`, `blobs/` and
  `.music2/tx/`, then the one `F_FULLFSYNC`, then the previous journal's unlink, then the renames, then each
  target's parent `fsync`; nothing written to the project before the barrier. With injected errors at each
  call: before the first rename, `Io` with the tree and `.music2/tx/` as before; after, a rollback leaving
  the tree as before; with the rollback failing too, `tx.interrupted` in `problems()` at once. A target
  replaced by rename between the pre-check and its swap: swapped back, `Conflict`, tree as the outside
  writer left it. A file created at an `Absent` target before its rename: `Conflict`, the file untouched.
  A state blob rewritten in place after the commit leaves `blobs/<sha>` intact, and Discard restores it.
- `core/session/src/watch.rs` (R12, R13, R14), with the fake clock: an outside lane write made between a
  commit and its `wrote` is still taken in, and an open batch survives `wrote`; a `.git` file with
  `gitdir:` is followed, and an enclosing repo's `index.lock` holds the batch; an `index.lock` older than
  10 s doesn't, and raises `git.locked` once; a file growing in `assets/` that no lane names never holds a
  code batch, and a growing state blob is hashed only once stable; a same-size rewrite within 2 s of the
  baseline is seen.
- `lock.rs`: held in another process → `Held` with the holder; in the same process → `Held` (C10);
  a volume that can't lock (an injected `ENOTSUP`) → `lock.unsupported`; `.music2/tx/` on another volume
  (an injected `st_dev`) → `lock.unsupported`; a volume without `MNT_LOCAL` (injected) → writer access and
  `lock.local`; a journal with another `host_id` and the same `host` name → `tx.foreign`, one with the same
  `host_id` and another name → this host's (R11); a writer retries while a reader scans; released when the
  holder dies.
- `crates/server` (R4): `a_failed_build_then_a_lane_edit_rebuilds` (a structural save with `fail_builds`,
  then a lane edit: a new renderer, not a swap into the stale one); `a_values_commit_writes_cells_from_what_plays`.
- `crates/editor`: rename a label in each form (with options, without, back to the key, a refused
  non-literal), `Identical` refuses one that changes anything else; automate a literal, a quantity and an
  inherited control, in `song.ts` and in a track file, with `() =>`, `({ notes }) =>` and `(ctx) =>`,
  each rendering the same bits before and after, and undo removing the lane file; relink of a folder by
  its fix and by the CLI, and its undo after the old key is declared again (R22); delete-data's undo after
  the lane is declared again (R22); plugin state as one Tx that undo puts back;
  `a_note_edit_in_an_anonymous_clip_keeps_one_clip` (R6); interleaved undo and redo across a ported and an
  unported gesture at checkpoint 5 (R21; deleted with `Legacy`).

---

## 20. Tests and behaviours whose meaning the contract changes

### 20.1 Tests adjusted, with the rule that changes them

| Test | Today | After step 6 |
|---|---|---|
| `cable_tests::a_cable_undo_refused_after_another_change_waits_until_that_change_is_taken_back` | a comment added above the cable blocks its undo | WRITE-21: the inverse is located by its carried range, so that comment no longer blocks. The test keeps its shape with the outside change made **on the cable's own bytes** (`osc.sine(wobble)` → `osc.sine(hz(700))`): refused, not offered, and offered again once taken back. A new `an_unrelated_change_in_the_file_never_blocks_a_cable_undo` covers the comment. |
| `lanes::an_edit_planned_before_the_lane_changed_is_refused` | any change to the lane file since revision `at` refuses the edit | WRITE-3's `Clips` rule: refused (`Conflict` naming the clip) when the outside change touched a clip the edit changes; a change to another clip merges. Both cases are tested. |
| `graph/tests::a_project_that_does_not_compile_says_where_without_its_folder` | `open` fails with the message | WRITE-20: `open` succeeds; the same message is the first diagnostic, still without the folder. |
| `relink_tests::a_data_problems_fix_is_applied_when_asked_and_undone` and the server's `data_a_rename_left_behind_is_a_problem_whose_fixes_play_and_undo` | `Session::fix(at, code, edit)` | `commit(fix.tx)` / `/api/graph/fix` with the Tx; a fix whose base is stale is a `Conflict` (was "isn't offered any more"). |
| tests calling `write_notes(at, …)` | a revision number | the lane's base hash from the timeline (12). |
| `edit::tests::carrying_finds_a_range_wherever_no_edit_touched_it` | line-diff carry | same assertions on `text::Edits::carry`, plus same-line cases now carried (2.4). |
| tests that write a file and then call a gesture without `refresh()`, relying on the gesture's own (found by the audit at checkpoint 5; WRITE-19) | the gesture takes in the whole folder first | the test calls `refresh()` before the gesture, which is what it meant; a test whose point is the outside change in the gesture's own file needs nothing (the baseline takes it in, 4.2 step 6). |

### 20.2 Behaviours that change (for the owner)

- An undo is no longer blocked by an unrelated edit elsewhere in the same file (WRITE-21).
- A piano-roll write merges with an outside change to other clips of the lane (WRITE-3).
- An outside lane write is taken in 150–250 ms after it settles rather than within one 300 ms poll
  (WRITE-19), and never through QuickJS (PLANE-1). A gesture no longer takes in the folder first.
- A batch of outside changes whose code doesn't compile waits whole, its data included (WRITE-19).
- A project with broken code opens (WRITE-20).
- A second app or CLI writer on an open project gets read-only or an error (WRITE-17, WRITE-18).
- `m2 plugin-state --force` is gone (WRITE-1).
- A crash after an edit reopens with no diagnostic; only a commit some of whose targets never changed is
  `tx.interrupted` (C9).

---

## 21. File-by-file plan

**Workspace**
- `Cargo.toml`: member `crates/editor` (already matched by `crates/*`), `m2-editor = { path = "crates/editor" }`,
  `libc = "0.2"` (`gethostuuid`, `gethostname`, `F_FULLFSYNC`, `renamex_np`, `clonefile`, `statfs`; all in
  the locked 0.2.190) and `base64 = "0.22"` (both already in the lock file).

**core/song**
- `src/tx.rs` (new): 1.1–1.3, serde with `deny_unknown_fields`, `Tx::validate` (overlaps, `Declared`),
  `TxPath`, `TxId`, `JournalId`, `DeltaHash`, `Payload` base64.
- `src/jcs.rs` (new): 1.4.
- `src/path.rs`: `dir_hash`; `taken` and `under` moved in from `core/session/src/edit/data.rs`.
- `src/hash.rs`: `Sha` "sha256:" serde and parse.
- `src/lane.rs`: `clip_lines`, `addressable`, `put_clips`; `Clip::line` reachable.
- `src/diag.rs`: `Fix { code, title, likely, tx }`; `Edit`, `Op` deleted.
- `src/data.rs`: the rewrite fixes and `delete-data` built as Tx, ids from 11's content-derived Tx id;
  `DataSet::update` for the incremental read.
- `src/lib.rs`: `Snapshot.not_utf8`, `Snapshot::sha`; `Project::empty`; re-exports.
- tests: `src/tx.rs`'s and `src/jcs.rs`'s test modules, `data/tests.rs` for fix Tx.

**core/script**
- `src/lib.rs`: `pub fn builds() -> u64` (an `AtomicU64` bumped in `build_full`).
- `src/sites.rs`: `imported_names` and `import_insertion` move out (to `crates/editor/src/sites.rs`).

**core/session**
- `src/lib.rs`: `Code`, `Compiled` with `Deref`, `build_code`, `read_data`, `compile`; `Compiled::none`;
  module list updated (no `edit`, `view`, `timeline`, `lanes`, `render`).
- `src/session.rs` (from `edit.rs`): `Session` (fields of 3, 7, 10; `accepted: Arc<Revision>`, `waiting`),
  `Revision`, `open`/`open_as`/`open_read_only`, `accepted`/`accepted_arc`, `problems`, `catalog`,
  `take_in`, `refresh`, `carry_range`, `past_sites`, `text`, `history_labels`, `can_undo`/`can_redo`,
  `undo`/`redo`, `retry_lock`, `access`, `waiting`. Deleted: `Control` and friends (to the editor),
  `Candidate`, `commit` (old), `write_file`, `exact`, `saved`, `Change`, `Undo` (with `Step::Legacy`, at
  the end of checkpoint 5), `site_by_id`, `resolve*`, `snapshot(at)`, every gesture's `refresh()`,
  `write_whole` (now private in `tx/stage.rs`).
- `src/tx/mod.rs` (`check`, `commit`, `Checked`, `Committed`, `Refusal`, `Patch`, the cache),
  `tx/bases.rs` (2.2, 2.5, waiting paths), `tx/baseline.rs` (4.2 step 6), `tx/apply.rs` (ops on a
  snapshot; renames as today's `data::apply`), `tx/delta.rs` (5, `delta_between`), `tx/expect.rs` (6),
  `tx/journal.rs` (6a.1, 6a.3, 6a.4, recovery fixes), `tx/stage.rs` (`trait Disk`, staging, syncs,
  barrier, checked renames, rollback, prune; the private writer), `tx/inverse.rs` (10.2),
  `tx/claims.rs` (8.4), `tx/props.rs`, `tx/delta_tests.rs`, `tx/kill_tests.rs`, `tx/tests.rs`.
- `src/text.rs` (2.4: `Edits`, `Coords`, points and spans), `src/texts.rs` (2.3), `src/lock.rs` (7, with
  `gethostuuid`, `statfs`, the retry), `src/watch.rs` (8.2, with the git dir lookup), `src/save.rs` (8.1).
- `src/plane.rs`: `Plane` and `Plane::of`; `code_changed` and `lane_swaps` deleted (the delta replaces
  them), their tests rewritten on deltas.
- `src/unclaimed.rs`: fixes as Tx; `folder_hash` → `m2_song::path::dir_hash`.
- `src/gate_tests.rs`: stays (step-4 gates through `Session`); the label gate uses
  `crates/editor`'s rename in its own test there.
- Moved out: `view.rs`, `view_tests.rs`, `timeline.rs`, `lanes.rs`, `render.rs`, `edit/*`,
  `gate5_tests.rs`, `tests/forms/`.

**crates/editor** (new): per 13.1; `src/lib.rs` (`Gestures`, `Edited`, `EditError`, `Committed`),
`controls.rs`, `lens.rs`, `label.rs`, `keys.rs`, `sites.rs`, `value.rs`, `cable.rs`, `effect.rs`,
`notes.rs`, `data.rs`, `rename.rs`, `automate.rs`, `view.rs`, `timeline.rs`, `render.rs`; tests moved and
added; `tests/forms/` (`planned_call_forms_are_current` writes there); `tests/crate_graph.rs`.

**crates/server**
- `src/graph/actor.rs`: 17.1, 17.3; `Plays` on `Arc<Revision>`; `single` and `lane_swaps` use deleted;
  `poll` replaced by the watcher job; `write_file` → `save`; no `refresh()` before gestures.
- `src/graph.rs`: `Graph::tx_check`, `tx_commit`, `label`, `automate`, `retry_lock`; `EngineView` and
  lease resolution through the editor's `Control`; `poll` kept for tests only.
- `src/graph/lease.rs`: `m2_editor::Control`; commit as 16 (Pending on either cause).
- `src/routes.rs`: 17.2.
- `src/files.rs`: `write` → `save`; `Conflict` from `Refusal::Conflict`; the non-UTF-8 base (8.1).
- `src/lib.rs`: the watcher thread (17.4) with `Settle::wrote`.
- `src/export.rs`: render through `m2_editor::render`; create-only into `assets/` (18).
- `Cargo.toml`: `m2-editor`.

**apps/m2**: `src/main.rs` per 18; `tests/lock.rs` (new); `tests/relink.rs` updated; `Cargo.toml`:
`m2-editor`.

**crates/gates**: `src/lib.rs` and `server.rs` through `Gestures`; G10 (`tx-data`, idle and loaded).

**apps/music2-desktop**: nothing but the read-only banner's menu state (Save disabled when read-only).

**web**: per 18a.

**docs/architecture.md** (in the same change): every **Today** line of WRITE-1…WRITE-21, PLANE-1,
PLANE-2, PLANE-5 (the measurements), RENDER-26, FMT-16 (the watcher's racy rule), FMT-17, FMT-20 (host
id), ID-2, ID-10, ID-17, HOME-2, DIAG-1, DIAG-4, DIAG-6, and the step-6 gates' status; the DIAG table's new
codes (C2); WRITE-3's `Payload::Local`, `Declared::Whole` and the `Journal` expectation (C1, C3); WRITE-5's
conflict `clip` and `why` (C4); WRITE-3's `LineCol` base (C5); WRITE-6's Confined rules 4 and 5 (C1);
WRITE-8's base-text coordinates (C17); WRITE-16's silent tidy and host id (C9, C18); WRITE-18's
readers' scan (C19); PLANE-5 as the owner decides (C20); the deferred table's row for the move into
`crates/editor` marked done.

---

## 22. Order of work (the workspace green at every checkpoint)

Each checkpoint ends with `cargo test --workspace`, `cargo clippy`, the 4 pure fixtures' renders against
`scratchpad/baseline/SHA256SUMS`, and `npm run typecheck` when `web/` or the forms changed. One shared
target dir (`CARGO_TARGET_DIR` from the brief).

1. **`core/song`, additive.** Tx types, JSON, validation, `jcs`, `Sha` form, `dir_hash`, `taken`/`under`
   (re-exported from the session's old place so nothing breaks), `clip_lines`, `addressable`,
   `put_clips`, `Snapshot::sha`, `Project::empty`. Tests.
2. **`core/session`, no behaviour change.** `Code`/`Compiled` with `Deref`; `build_code`/`read_data`
   with the incremental read and its equality test; `text.rs` with `carry` re-based on it (old carry
   tests pass) and `Coords`; `texts.rs`; `Revision.code_rev`/`data_rev`; `accepted: Arc<Revision>`;
   `PastCode` beside the old `Past`.
3. **The Tx engine.** `tx/` (apply, bases, baseline, delta, expect, inverse, stage with `Disk`, journal),
   `Session::check`/`commit` working alongside the old edit paths (gestures still use them). The history
   becomes one stack, `Step::Legacy | Step::Tx` (R21), so undo order holds across both. Property tests
   (a)–(d) on raw Tx, the delta tests, the two-process hash, the stage order and fault tests, the kill
   test.
4. **Lock, open, save.** `lock.rs`, `Access`, `open`/`open_as`/`open_read_only` (with the readers' scan),
   WRITE-20's empty revision, journal scan, silent tidy and recovery fixes, `save` (taking in the draft
   plus its file). Audit tests that open one project twice in one process (each now gets read-only: drop
   the first session, or open read-only). The CLI's readers move to `open_read_only`; the server's open
   handles the empty revision.
5. **Port the gestures in place.** Each `Session` gesture method in `core/session/src/edit/` becomes a Tx
   builder over 3's engine (set and insert, connect, disconnect, add and remove effect, write notes,
   relink, delete data), and loses its `refresh()` (with the audit of 20.1's last row); fixes become Tx
   (`core/song`, `unclaimed.rs`) and `Session::fix` goes; once the last is ported, `Step::Legacy`,
   `Change`, `Undo`, `Candidate`, `write_file` and `data::make` are deleted; `saved` → `save`;
   `write_whole` private. The server, the CLI and the web adapt to `Fix { tx }` and `save`. Every existing
   test passes, except 20.1's, adjusted here.
6. **Take-in and settle.** `take_in` with batches and waiting paths, the data path for outside writes;
   `watch.rs` (tracking, racy text, git dir, `wrote`); `plane.rs` reduced; `data.claimed`. The server's
   watcher thread, and the actor on deltas from what plays (`single` and `lane_swaps` deleted, `Plays` on
   `Arc<Revision>`, no `refresh()` before gestures). The outside-lane, two-halves, one-batch and
   failed-build tests.
7. **Server and CLI surfaces.** `/api/tx/*`, read-only state and retry, lock messages; `m2 tx`, the lock
   sorting, `bounce --place` and `plugin-state` as Tx; `apps/m2/tests/lock.rs`.
8. **The move into `crates/editor`.** Mechanical: move 13.1's files with their tests, introduce
   `Gestures`, point the server, the CLI and the gates at it, move `tests/forms` and update
   `web/package.json`; the crate-graph and bypass grep tests.
9. **New builders and their UI.** `keys.rs`, relink of a folder through its Tx (the CLI and the strip),
   rename a label, automate a knob; the web's label editing, "Automate" and Tx-shaped fixes.
10. **Gates and the page.** G10 (idle and loaded) and PLANE-5 recorded on the reference Mac; G1–G9
    re-run; every Today line, the DIAG table and C1–C21 as the owner decides them.

Checkpoints 1–4 add without changing behaviour; 5 is the largest and is done gesture by gesture (set
first, since leases use it; then cables, effects, lanes, fixes), keeping the suite green after each: the
single history stack makes interleaved undo tests pass at every one of them.

---

## 23. Clarifications to confirm (the step 6 design's)

- **C1.** `Confined` admits a plugin-state, asset or arrangement change only where the Tx's own ops wrote,
  removed or renamed the file that holds it; a Code or Manifest file a Tx writes whole must be declared
  `Whole` (rule 4); `ranges` values are a list or `"whole"`, and `[]` declares no bytes (R23). The
  contract's definition doesn't mention them.
- **C2.** New codes: `tx.invalid` (Error, on a Tx: malformed), `tx.expected` (Error, on a Tx: an edit's
  `expected` differs from its base), `tx.unexpected` (Error, on a Tx: its delta breaks its expectation),
  `lock.held` (Error, on a session or a CLI write: another process holds the lock; read-only),
  `lock.local` (Notice, on the session: the lock is local to this Mac on a network volume, R27; or the
  owner prefers `lock.unsupported` there) and `git.locked` (Notice, on the session: a stale
  `.git/index.lock` no longer holds outside changes back, R13).
- **C3.** `Expect::Journal(id)` (a fifth expectation, only for FinishTx and DiscardTx: checked against the
  journal, not the graph, and on success it removes the journal), and `Payload::Local` (a file under
  `.music2/tx/` with its hash, only for recovery and the undo of a binary write). WRITE-6 says a new kind
  of expectation needs this review.
- **C4.** `Conflict`'s entries are `{file, current, clip?, why?}`, and `commit` may also end in `Io`
  (6a.6), which WRITE-5's three outcomes don't list.
- **C5.** `LineCol` counts lines and UTF-16 columns from 0, as LSP does.
- **C6.** Undo carries a text edit's inverse past unrelated edits in the same file, and a `Clips` inverse
  past changes to other clips (20.2).
- **C7.** A Tx is built on the accepted revision: files it doesn't touch come from it, never from an
  unsettled draft; a file it touches is taken at its current bytes, and the expectation is judged from
  the baseline B (the accepted revision with those bytes), so an outside change in a touched file rides
  along without counting as the Tx's (R2); a Tx touching code needs the draft's code to compile.
- **C8.** Batches are taken in whole or wait whole; while the draft's code is broken, a batch with no
  Code, Manifest or Arrangement change is still taken in against the accepted code (WRITE-14's rule for
  Tx, extended to outside writes that change only data, R3).
- **C9.** A commit's journal is removed after the next commit's barrier (or at close). At the next lock,
  a journal whose every target is at its after hash is tidied silently, so a crash after a completed
  commit reports nothing (R7); the step 6 gate's "killing at every commit step offers `tx.interrupted`"
  is read as "at every step where some target hasn't changed yet".
- **C10.** A second session on one project in one process is read-only (flock locks belong to the open file).
- **C11.** Delta names as in 5.2: a declared control's const by its instance chain and slot, `key()` frames
  by key, nodes without an origin by op and occurrence, lane nodes always with their read's site, names as
  arrays of segments.
- **C12.** The rename-to-NFC fix for an asset (FMT-15) stays deferred: WRITE-3a refuses a `Rename` from an
  `Unknown` path, and a non-NFC name is one.
- **C13.** `data.claimed` compares by path: a module claiming a path that existed and was unclaimed. A
  relink, which moves data to a new path, raises none.
- **C14.** The delta ignores `Module.track` and `Frame.sidechain` (and, as WRITE-6 says, positions, labels,
  numbering and written-or-inherited).
- **C15.** A project whose `.music2/` can't be created (a read-only volume), or whose `.music2/tx/` is on
  another volume, opens read-only with `lock.unsupported`.
- **C16.** Tx ids from the app and the CLI are random; `m2 tx` keeps the one it's given; a fix's id is
  derived from its problem and the hashes of the files it was raised on (R19), so the fix is the same
  everywhere for the same files.
- **C17.** WRITE-8 says a site's range is given "in the new text". This design names it in the base
  text's coordinates instead (own and outside changes as hunk-relative points, 5.2), so equal changes hash
  equally whatever else changed in the file between `check` and `commit` (WRITE-11, WRITE-12's "same
  outcome in every process"). The contract's sentence changes with this step.
- **C18.** A machine is identified by `gethostuuid()`; the host name is recorded for messages only, and a
  journal is `tx.foreign` only when its `host_id` differs (FMT-20, WRITE-16).
- **C19.** Read-only CLI commands take the lock without blocking for the length of a journal scan, never
  while reading the project, and writers retry a held lock for 250 ms, so a reader never turns a writer
  read-only (WRITE-18 says readers "never take the lock").
- **C20.** PLANE-5's 20 ms with `F_FULLFSYNC` on the path: if G10 misses it (idle or loaded), the owner
  picks one, each changing PLANE-5's wording, neither skipping the sync: (1) `F_BARRIERFSYNC` between
  staging and the renames (ordering is all crash consistency needs: the journal is durable before any
  rename lands, though the newest commit may be lost on power loss), with `F_FULLFSYNC` when idle, at close
  and before removing journals; or (2) the `SwapLane` sent as a preview once the candidate passes and the
  pre-check holds, in parallel with the durable write, swapped back with a toast if the commit fails (as
  WRITE-22a's lane lease previews). Until then the design keeps `F_FULLFSYNC` on the path.
- **C21.** (From R25, not built.) The draft rule could later require only that the touched Code and
  Manifest files build in the candidate, so a knob release isn't Pending while another file has a typo;
  that changes C5 of step 3b and three lease tests, so it is the owner's call.

---

## 24. Risks

1. **Delta names on outside writes.** A point inside a hunk names a node by that hunk; a hand edit that
   reflows a whole track call renames everything in it, so the delta says "structural" and the engine
   activates a renderer, as it does today. Harmless for correctness; step 7's SiteIds replace points.
2. **The data budget.** `F_FULLFSYNC` alone may take several ms on an M1 and tens under load; with the
   incremental read (3.2), one barrier per commit (6a.2) and deferred tidying (6a.3), the rest is about
   2–3 ms at p50. G10 measures both idle and loaded; C20 holds the ways out. `Snapshot` and
   `Project::from_snapshot` still copy every lane's text per revision (about 1 ms per MB); if G10 shows it,
   snapshot texts become `Arc<str>`, a contained change.
3. **The size of checkpoint 5.** Every gesture changes at once in kind (Tx) though not in meaning;
   mitigated by porting one gesture at a time over a Tx engine that is complete and tested first (3), by
   one history stack (R21), and by keeping method signatures (13.3) so the tests move unchanged. Removing
   the gestures' `refresh()` needs an audit of tests that relied on it (20.1).
4. **In-process lock semantics.** Tests and the server that open one project twice in one process now get
   read-only. The audit is part of checkpoint 4; the actor releases a same-folder project before reopening.
5. **The settle rule's latency for agents.** 150–250 ms after the last write; agents that want it sooner
   use Tx. A file growing in `assets/` holds nothing unless a lane names it (8.2).
6. **Base texts evicted.** An agent that sends bare hashes for an old base can get a `Conflict` the store
   could have avoided; `WithText` fixes it, and `m2 tx` always sends it.
7. **Data accepted while code is broken.** A batch with only data changes is taken in against the accepted
   code while the draft's code is broken (C8); the timeline shows data the draft's new lanes may read
   differently. The "Needs attention" strip already says the draft doesn't compile. A batch that mixes
   code and data never splits (R3).
8. **Recovery while the code is broken by the interruption.** A half-applied code Tx may leave the draft
   broken; `Expect::Journal` doesn't need it to compile (6.5), so Finish and Discard still work.
9. **JCS across languages.** Number formatting is where Rust and JS differ; the vector tests and, later,
   the TS tools' shared fixture guard it.
10. **`Payload::Local` trust.** It names only `.music2/tx/` files, refuses links, and is verified by hash
    before use, so a hostile Tx can at most copy a file that's already ours to a path the Tx may write.
11. **Undo memory.** `WithText` inverse bases keep texts alive; bounded at 64 MiB with the 500-step cap.
12. **Export into the project.** Export Audio could replace a file in `assets/` today; it becomes
    create-only there (18). A user picking another project path still writes an Ignored file, as now.
13. **Volumes without `RENAME_SWAP`/`RENAME_EXCL`, `clonefile` or `F_FULLFSYNC`** (exFAT, some SMB): the
    fallbacks (re-hash then rename, a copy, `fsync`) are weaker but never unsafe; such volumes are rare for
    projects and are exercised by the injected-error tests.
14. **The watcher's walk.** A stat walk every 50 ms over a large `assets/` costs CPU; G10 measures it, and
    FSEvents replaces it if it is over 2 ms per poll (8.2).
15. **Two delta computations.** The expectation's delta (B → candidate, base coordinates) and the
    engine's (what plays → accepted) differ in what they start from; they are the same function with two
    coordinate choices, and the engine's reuses the commit's in the usual case, so it costs a walk only
    after a failed build or a lag.

---

## 25. What step 6 leaves to steps 7 and 8

- `LoadState`, `SetArrangement` and prepared clip renders (the delta lists states, assets and the
  arrangement; step 6 activates a renderer for them).
- SiteIds replacing points in delta names and the arena keeping instances by RuntimeId (ID-13,
  RENDER-25): the two-insertion wrap is ready for it.
- Lane leases (WRITE-22a) and the tempo lease (RENDER-27a): their commits are `Clips` and `Write` Tx with
  nothing new in core.
- Add, duplicate, delete and rename track (ID-2's key helper exists), mute in song (`ModuleChange::Muted`),
  Restore (DIAG-6), and the plugin window's literal Tx and state autosave (HOME-5, HOME-6): builders in
  `crates/editor`, no change to core's write API (the step 8 gate).
- Forwarding writes to the lock holder, undo across restarts, and FSEvents for the watcher (deferred).
