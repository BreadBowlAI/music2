---

## 13. `crates/editor` (WRITE-9, WRITE-10, P10)

### 13.1 What moves, what stays

| Moves to `crates/editor` | From |
|---|---|
| `view.rs` (the canvas view, knobs, ports, read-only values) | `core/session/src/view.rs` |
| `timeline.rs` (the timeline view, with each lane file's hash; `Revision::tempo` stays in core, it is engine timing) | `core/session/src/timeline.rs` |
| `lens.rs` (`LiteralLens`, `Insertion`, `propose`) | `core/session/src/edit/lens.rs` |
| `controls.rs`: `Control`, `Declared`, `Edit`, `Audition`, the `file#call/arg` ids (debug labels, WRITE-7), `controls()`, `control_at`, `written_in`, `inherited_at`, `not_insertable`, `options_of`, `declared`, `component_of` | `core/session/src/edit.rs` |
| `cable.rs` (outlets, inlets, connect, disconnect, `rewire`, `excise`, `refs`, `canonical`, the default spellings) | `core/session/src/edit/cable.rs` |
| `effect.rs` (effects, `effect_at`, add, remove, `wraps`, `insert_args`, `calls_of`) | `core/session/src/edit/effect.rs` |
| `notes.rs` (write a notes lane, `tidy`, `describe`, `place`) | `core/session/src/lanes.rs` |
| `data.rs` (relink, relink a folder, delete data, apply a fix, plugin state, place a bounce) | `core/session/src/edit/relink.rs`, `apps/m2` |
| `label.rs` (`summarize`, `shown_call`, `fold`) | `core/session/src/edit.rs` |
| `sites.rs` (`imported_names`, `import_insertion`, and the new `fn_params`) | `core/script/src/sites.rs` |
| `render.rs` (Export Audio, `m2 render`, `refuse_in_assets`) | `core/session/src/render.rs` |
| `keys.rs` (ID-2's one key proposer) | new |
| `rename.rs` (rename a label), `automate.rs` (automate a knob) | new |
| `EditError`, `Edited`, `Committed` (gesture results) | `core/session/src/edit.rs` |
| the gesture and view tests, `tests/forms/` | `core/session/src/{edit/*_tests,view_tests,gate5_tests}.rs`, `core/session/tests/forms` |

What stays in `core/session` (WRITE-10's list): `Session` (open, access, take-in, `save`, `check`,
`commit`, undo, the journal, the lock), the `Tx` engine (`tx/`), `GraphDelta` and its checkers
(`delta_between` included), `text.rs`, `texts.rs`, `watch.rs`, `plane.rs` (class → plane),
`unclaimed.rs` (`data.unclaimed` with its fixes as Tx), the raw tables (`Compiled`: IR with origins and
frames, `sites`, `outputs`, `components`, `units`, `data`, `problems`), `Home`, `compile`, `catalog`,
`diagnostic`/`problem`, `reference.rs` (the p16/p50 generator), and the step-4 gate tests that use only
these.

Core gains nothing gesture-shaped: no core file names a gesture, a knob id or an effect. A test in
`crates/editor` (`core_never_depends_on_the_editor`) reads every `core/*/Cargo.toml` and fails if one
names `m2-editor` in any dependency table (dev-dependencies included), and a grep test fails if
`core/session/src` defines `fn connect`, `fn add_effect`, `fn set(`, `fn write_notes` or `fn view`.

### 13.2 The read API the editor uses

All already public, or made public read-only: `Session::accepted() -> &Revision` (`snapshot.files`,
`snapshot.sha(path)`, `project`, `compiled` with `ir`, `flat`, `sites`, `outputs`, `components`, `units`,
`data`, `problems`), `accepted_arc()`, `packages` (kernel and plugin signatures), `catalog()`,
`problems()`, `draft_errors`, `waiting()`, `access()`, `carry_range(code_rev, file, range)`,
`past_sites(code_rev)`, `text(sha)` (the store), `can_undo`, `can_redo`, `history_labels()`. The editor
writes only through `check`, `commit`, `save`, `undo`, `redo`, and never calls `refresh()` (R2).

### 13.3 One trait for the gestures

```rust
/// Gestures, as Tx builders on a session: what the server, the CLI and the tests call.
pub trait Gestures {
    fn controls(&self) -> Vec<Control>;
    fn controls_by_id(&self) -> HashMap<String, Control>;
    fn carry(&self, at: u64, id: &str, current: &HashMap<String, Control>) -> Result<Control, EditError>;
    fn preview(&self, at: u64, id: &str, value: f64) -> Result<Audition, EditError>;
    fn set(&mut self, at: u64, id: &str, value: f64, expect: Option<f64>) -> Result<Committed, EditError>;
    fn outlets(&self) -> Vec<Outlet>;  fn inlets(&self) -> Vec<Inlet>;
    fn connect(&mut self, at: u64, from: &str, to: &str) -> Result<Edited, EditError>;
    fn disconnect(&mut self, at: u64, id: &str) -> Result<Edited, EditError>;
    fn effects(&self) -> Vec<EffectInfo>;
    fn add_effect(&mut self, at: u64, call: &str, component: &str) -> Result<Edited, EditError>;
    fn remove_effect(&mut self, at: u64, call: &str) -> Result<Edited, EditError>;
    fn write_notes(&mut self, module: &str, lane: &str, base: Option<Sha>, clips: &[NoteClip]) -> Result<Edited, EditError>;
    fn relink(&mut self, from: &Path, to: &Path) -> Result<Edited, EditError>;
    fn delete_data(&mut self, path: &Path) -> Result<Edited, EditError>;
    fn apply_fix(&mut self, fix: &Fix) -> Result<Edited, EditError>;
    fn rename_label(&mut self, module: &str, label: &str) -> Result<Edited, EditError>;
    fn automate(&mut self, at: u64, id: &str) -> Result<Automated, EditError>;
    fn undo(&mut self) -> Result<Edited, EditError>;   fn redo(&mut self) -> Result<Edited, EditError>;
    fn view(&self) -> GraphView;  fn timeline(&self) -> TimelineView;
}
impl Gestures for Session { … }
```

The method names and argument lists are today's `Session` methods, so the moved tests change only their
`use` lines and the few results that are now Tx outcomes (`EditError::Conflict` gains the file and clip a
`Refusal::Conflict` names, and `From<Refusal> for EditError` maps the rest). `write_notes` takes the
lane's base hash instead of a revision number (12). `EditError` keeps its variants (`NotFound`, `Stale`,
`Conflict { current }`, `DraftInvalid`, `Rejected`, `WriteConflict`, `Io`) and adds `Changed(Vec<ConflictFile>)`
for a `Refusal::Conflict` and `ReadOnly(String)`.

Today's gestures begin with `self.refresh()` (`edit.rs` `set` and `step`, `edit/cable.rs` `ready`,
`lanes.rs` `write_notes`), and the actor's `set_value` refreshes before a value edit. All of these go
(R2): a gesture plans on the accepted revision, and an outside change in a file it touches enters through
the baseline (4.2 step 6), never by reading the whole folder. Tests that wrote a file and relied on the
next gesture to take it in gain an explicit `refresh()` (20.1), which is what they meant.

### 13.4 The shape of every builder

```rust
let tx = plan(session, request)?;                 // reads Session::accepted(), returns a Tx
let checked = session.check(&tx)?;                // core: bases, B, candidate, delta, expectation
assert_meaning(&checked)?;                        // the planner's own assertion on B → candidate (6.6)
let done = session.commit(tx)?;                   // reuses the checked candidate (4.2)
Ok(Edited::from(done, label))
```

Tx ids from the app's builders are `tx-` + 12 random Crockford characters (outside a sandbox, FMT-11);
the CLI's are the same; `m2 tx` keeps the id it is given.

### 13.5 Labels (P10)

Undo labels are the Tx's `label`, made by the editor: a value edit's call before and after
(`shown_call`, as today), a lane edit's notes or clips (`describe`), a relink's "unused notes" → "Kicks ›
hits". Core stores and returns them; it never makes one.

### 13.6 Keys (ID-2)

`keys::propose(label, taken: impl Fn(&str) -> bool) -> String`: fold to ASCII (NFKD, drop marks), lower
case, every run of other characters one `-`, trimmed to 32 and to the key rule, then `-2`, `-3`… when
empty or taken; the fallback for an empty result is the caller's (`track-2`, `lane-2`). Used once, when a
thing is created (automate's lane key now; add track in 8b), and the key is shown and written into code.

---

## 14. The gestures as Tx builders

Each is today's planner, with its candidate, write and undo record replaced by a Tx and the steps of 13.4.
Its verification closure becomes the expectation of 6.6 plus the planner's assertion where it has one.
Every position a builder writes into a Tx is in the base text it planned on (the accepted revision's file).

### 14.1 Set a value (WRITE-22's commit too)

The control is resolved as today (by id at revision `at`, carried by `carry_range` when the code changed
without touching its call). A written literal: `TextEdit [{lens.start, lens.end, expected: literal, text}]`,
`expect: Values([(Written { file, range: lens }, value)])`. An inherited control's first edit:
`TextEdit [{insertion.start, insertion.end, expected: "", text: insertion.text(n)}]`,
`expect: Values([(Slot { site: the call's range, slot }, value)])`. The early returns stay in the builder
(the value equals the current one: nothing is written and no step is recorded, keeping `0x320`, `8e2` and
`-0`; `expect` that no longer holds: `EditError::Conflict { current }`). The label is the call before and
after. The inverse (10.2) is `Values` with the old value, at the literal's new range, or the inserted
argument removed with `Values([(Slot …, default)])`.

### 14.2 Connect, disconnect

Connect: one `TextEdit` replacing the value's expression with the outlet's name, or an insertion of the
name into the call (`Control::planned`); `Confined { ranges: { file: [the edit's range] } }`; assertion:
`canonical(candidate.ir) == canonical(rewire(B.ir, module, consts, outlet.port))`.

Disconnect: the probe ("write a 0 and see what it feeds") is `session.check(&probe_tx)` with
`Expect::Seen` (R26), which never refuses on its expectation, read through `Checked::candidate()`; it is
never committed. Then one `TextEdit` for the name and one for the import a spelled default needs;
`Confined { ranges: [the name's range, the import's insertion point] }`; assertion: rewiring the new consts
back to the outlet gives B's graph.

### 14.3 Write a notes lane

Per 12: a `Clips { lane, put, delete }` with base the lane file's hash at the client's base, or a
whole-file `Write` when that lane isn't addressable or doesn't exist; `Confined { lanes: [lane] }`;
assertion: the candidate reads exactly the requested clips. The checks before it stay in the builder: a
declared notes lane of the accepted revision; a lane whose file doesn't read is never written over. The
label is `describe`'s.

### 14.4 Add and remove an effect

Add: two insertions around the call, `"<name>("` at `site.start` and `"<args>)"` at `site.end`, plus the
import insertions (joined per point, as today); `Confined { ranges: { file: [[site.start, site.end]] plus
each import point } }`. The effect's nodes span from inside the first insertion to inside the second, so
their spans are `[site.start, site.end]`, inside the declared range by 2.4's inclusive rule (R5;
`an_added_effect_is_confined`). Assertion: `wraps(candidate, B.ir, the new call's site)`. The new call's
site is found in the candidate by its start and callee, as today. Because the wrap is two insertions, the
wrapped call's characters are unchanged and it keeps its node names (5.2): the delta is the effect's nodes
added and the wrapped output's readers rewired, which is what step 7's arena needs to keep the wrapped
instrument's state.

Remove: two deletions, `[site.start, input.start)` and `[input.end, site.end)`;
`Confined { ranges: { file: [[site.start, site.end]] } }`; assertion: `wraps(B, candidate.ir, site)`.

### 14.5 Code editor save

`save()` (8.1); the server's route passes the base text's hash.

### 14.6 Step 1's fixes and the strip

`relink(from, to)` finds the `relink` fix the accepted revision offers for `from` with that `to` and
commits its Tx; when none is offered (the CLI may name any declared lane), it builds the same Tx itself
(a `Rename` with `Confined { lanes }`) after today's checks (the target is declared, reads that kind, has
no data, the name is free in any capitals). `delete_data(path)` commits the offered `delete-data` fix.
`apply_fix(fix)` commits `fix.tx`. The server's `/api/graph/fix` takes the fix as the problem carried it
(`{code, title, likely, tx}`) and commits its Tx: no lookup by equality, since the bases guard it.

### 14.7 The bypasses (WRITE-1)

- `write_whole`: private (8.1).
- `PUT /api/graph/file`: `save()`.
- `m2 bounce --place`: the lock is taken first (7.3); the render and the create-only asset and provenance
  as today; then a Tx on the audio lane putting one clip (id derived from the Tx id, FMT-11; the asset's
  pin, FMT-15): a `Clips` op when the lane is addressable, else a whole `Write` (12), base the lane file's
  hash read at the start (a lane changed during the render is a `Conflict`, as `Place::put`'s check is
  today), `Confined { lanes: [the lane] }`. If the Tx is refused, the asset stays (it is valid and
  create-only) and the message says how to place it by hand.
- `m2 plugin-state <project> <track>/<key> clap:<id>`: one Tx writing the blob (`Payload::Bytes`) and the
  sidecar (text) together (FMT-17), bases at their current hashes or `Absent`, `Confined {}` (C1). It
  replaces existing state as one undoable step, so `--force` is deleted.

---

## 15. The first new builders

### 15.1 Relink of a whole module folder (ID-17, DIAG-6)

What exists: `data.unclaimed` already offers the folder whole when one folder has no module and one
module has no folder (`folder_warning`), and `Session::relink` moves it by a session edit. New in step 6:
it is one `Rename { from: data/<old>, to: data/<new> }` Tx with a **directory base** (WRITE-12's
directory hash, shared with TS tools through `m2_song::path::dir_hash`), and its expectation is
`Confined { lanes }` with every lane of the new module the folder holds data for, so a folder that would
also feed a lane of another module, or change the graph, is refused. A folder holding a file `DataPath`
can't parse as Lane or PluginState (other than dot files) is refused, naming the file. On disk it is one
`renamex_np(…, RENAME_EXCL)`, its hash taken before and after (6a.2). Undo renames it back while the
folder's hash is the one the relink left; the inverse is data-only, so a module that claims the old key
again meanwhile doesn't block it (10.2).

UI: the "Needs attention" strip's likely button reads "Move all to Kicks" for a folder (its title from the
fix); the CLI is `m2 relink <project> data/kick data/kicks`; agents commit the fix's Tx.

### 15.2 Rename a label (ID-10)

`rename_label(module, label)`: the module's `track()` call is the project site with callee `track` whose
first argument's text is the module key as a string literal (keys are unique, ID-5); for `song` there is no
label to edit (refused). Then:

- `track("kick", { name: "Kick" }, fn)`: replace the `name` property's string literal;
- `track("kick", { other: … }, fn)`: insert `name: "<label>", ` after the brace;
- `track("kick", fn)`: insert `, { name: "<label>" }` after the key;
- a label equal to the key removes `name` (and the object if it had nothing else), since the label
  defaults to the key.

The string is written as a JSON string literal (valid TypeScript). `expect: Identical` (ID-10: labels are
ignored when graphs are compared), so a "rename" that would change anything else is refused. A call whose
options aren't an object literal in the call (`track("kick", opts, fn)`) is refused with "change it in
the code". Inserting `, { name: … }` after the key leaves the call's first and last characters unchanged,
so every node keeps its name and the delta is empty (5.2).

UI: double-click a track's name in the timeline header (`SongTimeline.tsx`) or the canvas module title,
edit in place, Enter commits (`POST /api/graph/label {session, revision, module, label}`), Escape cancels.
The undo label is "Kick → Kick Drum".

### 15.3 Automate a knob (HOME-2)

`automate(at, id) -> Automated { lane: LaneAddr, edited }`. For a control written in the code, or a
declared control a call leaves out:

1. **Where `lane` comes from.** The control's consts must all be in one module, and its call must lie
   inside that module's `track()` (or `song()`) call's function argument in the same file; otherwise
   refused ("this knob's call is shared by several tracks; automate it where it is called"). The
   function's parameter list is read by `sites::fn_params(source, span)` (oxc, in the editor):
   `() =>` → `({ lane }) =>`; `({ notes }) =>` → `({ notes, lane }) =>`; `({ lane }) =>` → nothing;
   `({ lane: l }) =>` → use `l`; `(ctx) =>` → use `ctx.lane`; anything else (rest patterns, defaults) is
   refused.
2. **The lane key.** `keys::propose` from the control's name: a dictionary entry's slot without its
   dictionary (`channels.hats.pan` → `hats-pan`), else the call's binding and the slot (`lead-cutoff`),
   else the component id and the slot (`bass-cutoff`); free among the module's declared lanes and its
   data files in any capitals.
3. **The Tx.** A `TextEdit` replacing the value's whole expression (the literal, or a quantity call such
   as `khz(2.5)`, which `lane.control`'s fallback takes as is) with `<lane>.control("<key>", "<unit>",
   <the expression as written>)`, or inserting it into the call for an inherited control (the default
   written as `disconnect` spells it); the parameter edit; a `Write` of `data/<module>/<key>.json` with
   the canonical empty lane `{"unit": "<unit>", "clips": []}` (base `Absent`); `expect: Confined
   { ranges: [the expression, the parameter list], lanes: [the new lane], modules: [] }`, WRITE-3's
   example exactly. The new lane has no clips, so it plays the fallback and the song sounds the same
   (HOME-2). The unit is the const's (`Unit::name()`); a control without a unit is refused ("give it a
   unit first").

The delta: the lane node added (its site inside the replacement, so in the declared range), the const
removed (written in it), the reader rewired to the lane. The new lane is declared only in the candidate,
so it rides on its lane node, not in `lanes` (5.1). Structural, so the engine activates a new renderer. A
test renders the song before and after and finds the same bits (the fallback equals the literal).

UI: the knob's context menu in the canvas (`NodeGraph.tsx`, `KnobControl`) gets "Automate"
(`POST /api/graph/automate {session, revision, id}`); the timeline then shows the new control lane under
its track, empty, drawing the fallback; a toast says "Automating hats pan in a new lane, hats-pan". Undo
removes the lane file and restores the literal in one step. A knob a lane or a cable already drives shows
no "Automate".

---

## 16. Leases on Tx (WRITE-22, the parts step 6 changes)

- **Commit** is one coalesced `Values` Tx built by `Gestures::set` (14.1), on the lease's control as the
  session last carried it. `Committed::Pending` (C5 of step 3b) stays, for both ways a `Values` Tx can be
  held up without being wrong: `Rejected` only because the draft doesn't compile (4.2 step 3), or a
  `Conflict` whose `why` is "changed on disk and doesn't compile yet" (4.2 step 6, R2). The value keeps
  playing and the commit is retried once the draft compiles or the watcher has taken the change in. An
  outside change in the knob's file that compiles no longer ends the lease: its commit carries across it
  and is judged against B (`a_knob_commit_after_an_outside_edit_in_its_file_keeps_both`).
- **Ids are anchors underneath.** A `Control` carries its `anchor: Anchor` (the literal's range or the
  call's range and slot). The `file#call/arg` id stays the web's handle within one code revision; carrying
  an id from an older code revision goes id → anchor in that code revision (`past_sites`) → `carry_range`
  → the control at the carried anchor. This is today's `Session::carry`, re-based on anchors and
  `text.rs`, so aliases, `begin` across a save, and the lease tests keep their meaning.
- **`begin` no longer refreshes** (R2): it resolves against the accepted revision, which the watcher keeps
  within 250 ms of the disk; a client behind is carried as today.
- **`code_rev`** comes from the session (3.1) instead of the actor's `plane::code_changed`, so a lane write
  never re-resolves a lease (WRITE-13).
- **What the engine hears at commit** is decided from what plays (5.5): a `Values`-only delta writes the
  committed value into the playing renderer's cells in the step that ends the lease (as `Install` does
  now); a delta with more (an outside change in the knob's file rode along) builds a renderer that carries
  the lease's value.
- A read-only session refuses `begin` (7.3).

---

## 17. The server (`crates/server`)

### 17.1 The actor on deltas (R4)

- `Open::single` (the guess, `actor.rs`; contract's `graph.rs:341-348`) is deleted, and so are the
  `before`/`after` revision arithmetic and the `plane::lane_swaps` call in `Open::swaps`.
- `Plays` keeps the `Arc<Revision>` it was built from, in place of its `flat`, `snapshot`, `data` and
  `revision` copies. `Open::way` computes `delta_between(plays.revision, accepted)` (reusing the last
  change's delta when it starts from the revision that plays) and follows 5.5: `Same` for an empty delta
  (adopt), values with `same_layout` → adopt and write the cells that differ, `Swap` for lanes only (with
  today's `caps` check), else `Activate`/`Replace`/`Load` as today. `Way::Failed` changes nothing in
  `Plays`, so the next settle compares from what really plays. `adopt()` moves `Plays` to the accepted
  `Arc` only after a send succeeded or nothing needed sending.
- `code_rev` is the session's; `Open.code_rev`, `Open.seen`, `Open.snapshot` and the "code changed"
  computation go.
- `Actor::edit`, `set_value`, `commit` (leases), `write_file` call the editor's builders and `save`, and
  then settle; none of them calls `session.refresh()` first (R2). `EditTiming` keeps its fields.

### 17.2 Routes

| Route | Body | Does |
|---|---|---|
| `POST /api/tx/check` | `{session?, tx}` | `check` → `{checked}` or a refusal (409 conflict, 422 rejected, 500 io) |
| `POST /api/tx/commit` | `{session?, tx}` | `commit` → `{committed}` or a refusal; settles and announces |
| `PUT /api/graph/file` | `{session, path, base \| base_sha, text}` | `save` (base text is hashed by the server, so the web needn't change; 8.1 for non-UTF-8 files) |
| `POST /api/graph/fix` | `{session, fix}` | commits `fix.tx` |
| `POST /api/graph/label` | `{session, revision, module, label}` | rename a label (15.2) |
| `POST /api/graph/automate` | `{session, revision, id}` | automate a knob (15.3) |
| `POST /api/graph/lane` | `{session, revision, module, lane, base, clips}` | write a notes lane (`base`: the lane's hash) |
| `POST /api/graph/lock/retry` | `{session}` | `retry_lock`, then the state |
| the rest | as today | as today, through the editor |

Every write route answers 423 with WRITE-18's message on a read-only session. The state (`GET
/api/graph`) gains `read_only: null | {why: "held" | "unsupported", holder?: {pid, program, host,
since}}`; the news carries the session's problems first (`tx.interrupted`, `tx.foreign`, `lock.*`,
`git.locked`), then the revision's, and an outside write's `data.claimed` notice as a toast.

### 17.3 Opening

`Actor::open`: if the same folder is open, close it first (its lock); `Session::open_as(dir, packages,
"music2")`; never fails on code (9); publish; build and load a renderer only when the revision has code.

### 17.4 The watcher thread

`spawn_graph_watch` keeps its thread: every 50 ms a stat walk of the open project (off the actor) feeds a
shared `Mutex<Settle>`; on `Ready` it sends one job to the actor that reads the snapshot and takes it in
(and settles). Lease expiry keeps its 300 ms cadence on the same thread. After each of its own commits and
saves, the actor calls `Settle::wrote` with the paths the session wrote and their stamps (R12): never a
fresh listing, so an outside write elsewhere, or an open batch, is never folded away. The app regaining
focus asks for an immediate poll, still under the settle rule. `Graph::poll` stays as an immediate
take-in without the settle rule for tests only (`assert!(!graph.poll())`).

---

## 18. The CLI (`apps/m2`)

- Readers and writers sorted as 7.3; writers fail first with WRITE-18's message and exit 1. Readers print
  `tx.interrupted` for an interrupted commit when the lock is free (6a.4), with the `m2 fix` commands.
- `m2 tx <project> <file.json | -> [--check]`: reads a Tx; `--check` opens read-only and prints the
  delta, hash and diagnostics; otherwise commits and prints the outcome as JSON on stdout (exit 0 when
  committed, 1 on a conflict, rejection or I/O failure), with the project's problems on stderr (DIAG-4).
  `m2 tx` always sends `WithText` bases it reads from the store or the disk.
- `set`, `connect`, `disconnect`, `add-effect`, `remove-effect`, `relink`, `delete-data` call the editor's
  `Gestures`; `fix <project> <src> <code>` commits the offered fix's Tx (a journal's `src` is its
  `.music2/tx/<jid>.json`); `attention()` prints each fix's command from its Tx's first op.
- `bounce --place` and `plugin-state` as 14.7; `plugin-state --force` is gone (a removed flag is an error
  naming why).
- Exact renders written into `assets/` (by `m2 render` or Export Audio) are created, never replacing a
  file (FMT-14's create-only exception of WRITE-1); approximate ones are refused there, as today.

---

## 18a. The web (`web/`)

- `lib/graph.ts`: `Fix = {code, title, likely, tx}` (`tx` the Tx JSON); `graphApi.fix(session, fix)`;
  `graphApi.label`, `graphApi.automate`, `graphApi.retryLock`; `TimelineLane.sha`; `writeLane` sends
  `base` (the lane's hash it was read at) and keeps the hash the answer returns; `GraphState.read_only`.
- `Attention.tsx`: describes a fix from its Tx's ops (a `rename` is a move, `remove` a delete, a lane
  `write` a rewrite); `tx.interrupted` shows Finish and Discard.
- `App.tsx`: a read-only banner naming the holder, with Retry; editing controls disabled while it shows.
- `SongTimeline.tsx`: in-place label editing on double-click.
- `NodeGraph.tsx`: "Automate" in a knob's context menu.
- `codeWorkspace.ts`: unchanged (the server hashes the base text); it applies `patches` from commits to
  open buffers that have no unsaved edits, instead of reloading them.
- `package.json`'s `typecheck` points at `../crates/editor/tests/forms` (moved with the planners).

