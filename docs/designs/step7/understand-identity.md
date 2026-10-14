# Step 7: identity today, and what SiteId, InstancePath and RuntimeId are built from

Read-only survey of HEAD `cbf5eb9` (the working tree's uncommitted data-plane changes in
`core/session` and `crates/server` were not relied on; `crates/server/src/graph/actor.rs` is cited as
HEAD has it). Nothing was built or run with cargo. Two small Python simulations of the text diffs
(scratchpad only, not in the repo) confirmed the worked examples in section 3.

## 0. The answer in short

- **Don't number sites.** ID-13's `SiteId(u32)`, assigned by the session as it accepts each code
  revision, adds state that must stay coherent across failed builds, undo, reopen and the 64-revision
  window. Worse, if it went into delta names it would make `DeltaHash` depend on the session's
  history: two processes would hash one Tx differently. That breaks step 6's gate ("the same hash in
  two processes") and WRITE-11's `Seen(hash)`.
- **Step 6 already built the identity we need: the `GraphDelta` node name** (`core/session/src/tx/delta.rs`).
  A node is named by its module key, its frame chain, its own call site and its occurrence. Every
  position in a name is a point in the comparison's reference coordinates (`text::Coords`). The old
  side's names are just its own byte offsets, so they don't depend on what it is compared with. The
  new side is named in the old side's coordinates. So "the node in revision B that continues node X of
  revision A" means "the node with the same name in `delta_between(A, B)`".
- **The definitions, all made from the delta names:**
  - **SiteId** = (file, the point of the call's opening `(`) in the comparison's coordinates.
  - **InstancePath** = module key, then the frame chain. Each `key()` frame is `key:<k>`. Each
    component frame is `<ComponentId>@<file>:<( point>#<occurrence>`. Everything above the innermost
    `key()` frame is dropped; this is what "key() pins" means.
  - **RuntimeId** = InstancePath / `<op>@<file>:<( point>` / `#<n>`, for the nodes that hold state
    (kernel, plugin, poly, feedback). A voice instance adds its allocator slot, on the runtime side.
- **Four defects must be fixed before these names can key the arena.** All four are in the
  revision-to-revision path that the engine uses, not in the Tx path, which is why step 6's tests
  don't catch them:
  1. `Edits::between` diffs whole lines. If one line hunk covers two calls, both calls are renamed.
     So two literal edits on adjacent lines rename the calls around them.
  2. A call is named by its first and last byte. When an effect wraps a call, the wrapped call is
     renamed: its first byte falls inside the trimmed hunk, and its closing `)` lines up with the wrong
     `)` in the run of brackets. This happens for every writer, the app's own `add_effect` included,
     because the actor recomputes `delta_between` and doesn't reuse the Tx's coordinates.
  3. The ComponentId is missing from chain segments.
  4. ID-14's rule "a changed count resets every occurrence" isn't applied.
  The fixes: refine each line hunk with a token-level Myers diff, reusing the generic `kept_lines`;
  name a call by its `(`; add the ComponentId; and compare occurrence counts during matching.
- **About 250 new lines in total.** No new identity table, no new persistent state, and the audio
  thread never sees a name.
- **What this replaces:**
  - `CarryKey` and the builder's ordinals, along with `Carry`'s matching.
  - `same_layout` and `value_writes`, which match cells by position. This one is optional.
  - The editor's second text carry (`controls::carry_text`, `carry_ends`, `carry_lines`).
  - `m2_ir::control_id`, which only a test uses.

## 1. Identity today: every number, and how long it lives

### 1.1 Call sites: `core/script/src/sites.rs`, `core/script/src/lib.rs`

- `sites::instrument(file, source, first_id)` parses each project or package file with oxc and
  rewrites every call so that it records its site just before it runs (`__m2s(ID, lastArg)`).
  `Site { id, file, ordinal, start, end, line, callee, binding, constant, in_project, args: Vec<Arg> }`.
- `id` is a u32 numbered across **the whole build, in module load order**. `Load::load` (lib.rs:446)
  starts each file at `st.sites.last().id + 1`. Within a file, `Collect::visit_call_expression` (sites.rs:237)
  gives ids in pre-order (the outer call before its arguments). Package files such as
  `std/src/fx.ts` are numbered too; the SDK `@m2/core` isn't instrumented.
- `ordinal` is the call's index among the file's calls, sorted by `(start, -end)` (sites.rs:308).
  Editor control ids use it: `file#ordinal/arg`.
- `start..end` is the byte range of the whole call expression in the original text. `args` holds each
  argument's shape and span (`Number`, `Object`, `Name`, `Other`).
- The SDK's `takeSite(arguments)` (core.ts:97) takes a site only when the arguments it received end
  the way the instrumented call's arguments did. So a function that native code calls
  (`xs.map(lowpass)`) gets **no site**: no origin, and no frame.

### 1.2 Nodes and origins: `core/ir/src/lib.rs`

- `Node { id, op, src, origin: Option<Origin>, frame: Option<u32> }`.
  - `id` is `n<k>` from a **per-module counter** in walk order (`push` in core.ts:232; `poly` note
    fields share the counter).
  - `src` is `file:line:col` from the JS stack, used only when the node has no origin.
- `Origin { site, arg: Vec<String>, scale, control: Option<Slot> }`.
  - The `kernel()`, `plugin()`, `poly()` and `feedback()` nodes carry `{ site }` of their own call.
  - A unit const (`hz(800)`) carries `{ site: hz call, arg: ["0"] }`.
  - A declared control's const (PKG-3, step 5) also carries `control: Slot { frame, slot, inherited, smooth }`.
    `frame` is its instance, an index into `Module.frames`; `slot` is like `cutoff` or `channels.t05.db`.
  - A plain JS number gets no origin, and the checker folds it into `Input::Const`.
- `Origin::same_value` (one literal, or one inherited slot of one call) groups cells for leases
  (`CellMap` in `core/runtime/src/cells.rs`).

### 1.3 Frames: `Module.frames`, SDK `openFrame` (core.ts:480)

- `Frame { site, component: Option<ComponentId>, occurrence, key, parent, options, sidechain }`. They are
  listed per module in the order they opened, and a parent always comes before its children.
- A frame is opened by:
  - a component call (`instance`, core.ts:1221), with `component = <package>:<id>` or a project
    component's bare id (PKG-9, from `__m2_home`), so an alias or a moved definition changes nothing;
  - an old-form component (`paths`, core.ts:~907), with no ComponentId;
  - `key()` (core.ts:518), with `key = Some(k)`.
- `occurrence` counts evaluations of one site within one parent frame. It is keyed by
  `${parent index}:${site}` (`info.runs`), so it is build-local, but the same for the same code.
- `kernel()`, `plugin()`, `poly()` and `feedback()` open **no** frame. Their nodes name the innermost
  open frame (`Node.frame`).
- `Module::chain(frame)` gives outermost-first chains. `m2_ir::control_id` spells
  `module/site:occ[:key]/…/@slot`. It is used only in a test (check.rs:1494), so it is dead in
  practice.

### 1.4 `key()` frames and plugin keys

- The SDK keeps a separate stack of **key** frames per module: `keyFrames(m)`, which is the top level
  plus each open `key()`.
- A `key()` key is unique within the innermost open key frame (`parent.frames`). Component frames in
  between don't count. A plugin key is unique within its innermost key frame (`frame.plugins`).
  Either clash is `key.duplicate` (ID-5).
- So **(module key, key-frame keys from outermost to innermost) identifies one key-frame evaluation**.
- `Op::Plugin.frames` lists the key-frame keys. `Op::state_path()` gives the frames, then the key
  (ID-8). The checker refuses two stateful plugins that resolve to one path (`plugin.state-collision`).

### 1.5 Modules: `Module { key, label, track, … }`

- The key roots every address (ID-11). The label is display text, and only the IR holds it (ID-10).
- `__build` runs `song` first, then the tracks in the order they were registered (core.ts:~1670).
- Each module has its own node counter and frame table, so an edit inside one track doesn't renumber
  another track's nodes or frames. It does renumber the **site ids** of every file loaded after it.

### 1.6 The checked graph: `core/ir/src/check.rs`

- `FNode { name: "<module>/<node id>", module, src, origin, op, outputs }`. **The frame is dropped**,
  and nodes inside a voice are named the same way.
- A flat index is global across modules.
- A `CellId` is a cell's index in the flat cell order (`cells::cells`).
- `CellMap` groups cells by origin, indexed by the build-local site u32.
- All of these are build-local (ID-12).

### 1.7 Today's matchers between revisions (six of them)

| # | Where | Matches what | Keyed by | Diff |
|---|---|---|---|---|
| a | `text::Edits::carry` (`core/session/src/text.rs`) | a Tx's ranges across outside changes; undo's inverse edits (WRITE-12) | byte range, strict | `Edits::between`: Myers over lines, each hunk trimmed by its common prefix and suffix |
| b | `delta.rs` `Side::graph` + `text::Coords` | every IR node, old against new (WRITE-8) | `Name` (§1.8) | a Tx's own edits (`Coords::tx`), or `Edits::between` between revisions (`Coords::revisions`) |
| c | `crates/editor/src/controls.rs` `carry`, `carry_text`, `carry_ends`, `carry_lines` | control ids and leases (WRITE-22), cables (`cable.rs:393`) | `file#ordinal` carried by the call's range | its own prefix/suffix check, then a line run through `kept_lines`: stricter than (a) |
| d | `core/runtime/src/plugins.rs` `CarryKey` + `Builder.ordinals` (`core/runtime/src/lib.rs:581,675`) | plugin instances (HOST-5) | module, plugin id + version, state path, ordinal among equals in flat order, state hash | none |
| e | `crates/server/src/graph/actor.rs` `same_layout`, `value_writes` (lines 151, 160) | cells for value-only changes | position: equal `CellAt` lists and equal bare `Flat`s | none |
| f | `delta.rs` `modules()` | a re-keyed module | equal graphs once the module key is stripped from every name | — |

### 1.8 Delta names as built (`delta.rs` `Side::chain` :250, `segments` :266, `name_all` :353)

`Name = Vec<String>`: the module key, then:

| Node | Segments |
|---|---|
| declared control const | chain of `control.frame`, then `@<slot>` |
| other node with an origin | chain of `Node.frame`, then `<op>@<file>:<start>-<end>`, then `[<arg path>]` for a const with one |
| lane | `lane:<key>@<file>:<start>-<end>` |
| time | `time:beats` or `time:seconds` |
| import | `import:<module>.<export>` |
| no origin | chain, then `<op>~<kernel or plugin id>` |
| inside a voice | the poly's name, `voice`, then the inner node's segments |
| every name, last | `#<n>`: its rank in walk order among nodes whose other segments are equal |

- A chain segment is `key:<k>` for a `key()` frame, and `@<file>:<start>-<end>#<occurrence>` for a
  component frame. It has **no ComponentId**.
- `<start>` is the point of the call's first character and `<end>` the point of its last.
  `Positions::point` makes each one a `text::Point`:
  - `Base(n)`, an offset in the reference text;
  - `Own{edit, at}` (`e<i>.<k>` for a Tx, `o<i>.<k>` between revisions);
  - `Outside{hunk, at}`.
- Package files are never in `coords`, so their points are always `Base`.
- **On the old side of a revision comparison, every point is `Base(own offset)`.** `Coords::revisions`
  has no outside hunks, so `current(at)` is `Base(at)`. So a revision's old-side names are a function
  of that revision alone, and can be cached with it.
- `compare` (:450) pairs names. A name on both sides with a different `sig` is removed and added. The
  `sig` is the op without its inputs: kernel id; plugin id, key and frames; poly voices; feedback
  kind; a lane's kind and default.
- Expectations are judged by **spans** (`Facts.spans`, `written`, `slot`), never by how a name is
  spelled (`expect.rs`). So the spelling can change without changing what `Confined` or `Values`
  accept.

## 2. What survives what

Key: "kept" means the same value or name, or the same match. "Shifts" means another node may take
it, so it is wrong if used across revisions. "New" means no match, so the instance resets.

| Change | `Site.id` | `ordinal` | `Node.id`, frame index | flat index, `CellId` | delta name (HEAD, between revisions) | `CarryKey` |
|---|---|---|---|---|---|---|
| a call inserted above, same file (step 6's gate) | shifts | shifts (that file) | shifts (that module) | shifts | **kept** (old offsets) | kept, unless it is a plugin of the same id in the same module before it (ordinal) |
| an import or call added in a file loaded earlier | shifts | kept | kept | shifts | kept | kept |
| label renamed (`{ name: "Lead Vox" }`) | kept | kept | kept | kept | kept | kept |
| one literal edited | kept | kept | kept | kept | kept (a `ValueChange`) | kept |
| two literals edited on adjacent lines, one save | kept | kept | kept | kept | **new** for every call whose first or last byte lies between them (§3, F1) | kept |
| effect added by the app (Tx: two insertions) | shifts after it | shifts | shifts after it (that module) | shifts | the Tx's delta: kept. **The engine's** (`delta_between`): **new** for the wrapped call and everything in it (F2) | kept |
| effect added by an agent or the code editor (file write) | shifts | shifts | shifts | shifts | **new**, as above (F2) | kept |
| file reformatted (prettier) | kept | kept | kept | kept | new inside every changed line | kept |
| an import alias renamed in the call (`delay(x)` → `echo(x)`, the same component) | kept | kept | kept | kept | new (the first character changed), though PKG-9 says it is the same component | kept |
| component swapped at one call (`delay(` → `chorus(`) | kept | kept | kept | may shift | new (the first character changed), but only by luck: `reverb(` → `rev2(` keeps its name (F3) | kept |
| loop count changed (`[a,b]` → `[a,b,c]` mapped to a component) | kept | kept | shifts | shifts | kept by index; **ID-14 not applied** (F5) | ordinal: by index |
| loop reordered | kept | kept | kept | kept | kept by index (a neighbour's state; ID-14 accepts this) | by index |
| a block cut and pasted elsewhere in the file | shifts | shifts | shifts | shifts | new (ID-13's rule 4 isn't built) | kept |
| a block inside `key("x", …)` moved | shifts | shifts | — | shifts | new: segments above `key:x` are positional (F6) | keyed plugins kept (state path) |
| a call wrapped in `key()` (first state save, ID-8) | shifts | shifts | shifts | shifts | new (a `key:k` segment is added) | new (the state path changes) |
| `key: "k"` added to a plugin call (first state save) | kept | kept | kept | kept | kept name, sig changed (removed and added) | new (the state path changes) |
| a track key renamed | kept | kept | kept | kept | new, but detected as `Rekeyed` | new (module changed) |
| a track file renamed (`git mv`) | shifts | kept | kept | kept | new (the file is in every name) | kept |

## 3. Findings

**F1. Line hunks rename whole regions.** `Edits::between` (text.rs:79) runs Myers over lines, then
trims each hunk by its common prefix and suffix. Example: `lowpass(x, { cutoff: 800 })` on one line
and `delay(a, { mix: 0.3 })` on the next, with both literals edited in one save, give one hunk, old
bytes 31..68:

```
800 });\nconst b = delay(a, { mix: 0.3
```

That hunk holds lowpass's `)` (byte 36) and delay's start (byte 49). Both calls are renamed: they are
removed and added, a structural change. Today that only means a rebuild, which happens anyway. With
the arena it means **a fresh delay, and its tail is cut by a value edit**. Agents and code-editor saves
hit this. A token-level diff over the same text keeps both calls (simulated).

**F2. A wrap renames the wrapped call on the engine's path.** Wrapping the p50 track `tracks/t02.ts`
(`… => pluck(lane.notes("notes")));`) in `delay(…)`:

- The trimmed hunk is old 169..193 = `pluck(lane.notes("notes"`, new 169..200.
- Old pluck: 169..195. Its start is `Base(169)`.
- New pluck: its start, 175, is inside the hunk, so it is `o0.6`. Its last `)`, at 200, maps back to
  old 193. That is `lane.notes(...)`'s `)`, not pluck's (194).

So pluck, its poly voice and its lane read are all renamed. `a_wrap_keeps_the_wrapped_calls_names`
(`delta_tests.rs:145`) passes only because it checks `check(tx)`, which uses the Tx's own two
insertions (`Coords::tx`). The server's `reach` and `way` (actor.rs:376) always call
`delta_between(&p.revision, rev)`, which uses `Coords::revisions` over `Edits::between`. So even the
app's own `add_effect` would reset the instrument it wraps. **Producers would hear this: adding a delay
to a ringing pluck cuts its notes.** RENDER-25's "Why" forbids it, though the gate only checks
untouched tracks.

A token diff alone isn't enough when calls are named by their ends. In a run of `)`, Myers puts the
inserted `)` at an arbitrary position. In the simulation of `reverb(pad(lane.notes("n")))` →
`reverb(delay(pad(lane.notes("n"))))`, `pad`'s and `notes`' last `)` shifted by one. Naming a call by
the point of its opening `(` is immune: the `(` of the wrapped call is a kept token whatever Myers does
with the closing brackets. In the simulation it was kept for `reverb`, `pad`, `notes`, `pluck` and
`lowpass`/`delay`.

**F3. Chain segments omit the ComponentId** that ID-15 puts in the InstancePath. With start/end names,
a component swap at one call is caught only when its first or last character changes. Naming by `(`
needs the ComponentId in the segment: `delay(` → `chorus(` keeps the `(`. With the ComponentId,
`import { delay as echo }` followed by `delay(x)` → `echo(x)` correctly **keeps** the instance (PKG-9),
which a name made from the callee's characters would not.

**F4. A numbered SiteId can't go into delta names.** WRITE-8 says "From step 7, SiteIds replace the
mapped ranges". If they are session-assigned u32s, a process that opened at revision R and one that
reached R through edits number R's sites differently. The two hash one Tx differently, which fails
step 6's gate and `Seen`. Normalising the numbers would just rebuild the points. And the engine never
needs identity across more than one comparison: it always compares **what plays** with **what is
accepted** (the step 6 design's 5.5, `Plays.revision`).

**F5. ID-14 isn't applied.** Names keep `#0..#k-1` when a group grows from k to k+1. That is right for
the delta, but the arena needs ID-14: "if the count changes, every occurrence resets". Otherwise
prepending to `[a, b].map((x) => pluck(x))` hands a's state to the new element. Consider std's mixer
(`std/src/mix.ts`): `pan()` and `gain()` run per channel inside a `.map`, so adding a channel resets
every strip. That is inaudible: `std:gain`'s smoother "starts at its first target"
(`std/kernels-src/src/lib.rs:353`), and pan holds no state. It is still worth knowing.

**F6. `key()` doesn't pin yet.** Inside `key("x", …)`, the segments of component frames *above* `key:x`
are positional. Keys are unique per parent **key** frame (§1.4), so those segments add nothing and
cost the pin.

**F7. Keyed plugins lose a property `CarryKey` has.** `CarryKey` matches keyed plugins by state path
wherever their call is. Names by site don't. A second pass by state path keeps it (§4.3).

**F8. A kept note consumer whose notes input is rewired must release its notes.** Today
`carry_from`'s `same_lane` check does this. With the arena, a poly or plugin kept by name whose
`events` or `notes` input changed lane (`poly(lane.notes("a"), v)` → `"b"`) keeps voices that the new
lane's scheduler doesn't know about. Those notes would stick. RENDER-26a only covers a *replaced*
consumer. The delta's `rewired` list names exactly these nodes.

**F9. `FNode` drops the frame,** so the runtime can't compute InstancePaths itself. That's fine: the
session computes names from the IR, and the join to flat nodes is `FNode.name == "<module>/<IR id>"`,
which holds for nodes inside voices too.

## 4. The proposal

### 4.1 SiteId: a point, not a number

A site is the pair (file, the point of the byte where the call's argument list opens). The point is in
the reference coordinates of the comparison that asks.

1. **Add `Site.open: u32`** in `sites.rs` `visit_call_expression`. Start scanning at the callee's
   span end, or at `it.type_arguments`' span end when the call has type arguments. Skip whitespace,
   comments and `?.` to reach the `(`. About 15 lines.
   - It is unique: every call has exactly one `(`. Chained calls `a.b(x).c(y)` and curried calls
     `f(x)(y)` have different ones.
   - Zero-argument sites (`f()`, `obj.m()`) have one too.
2. **Rule 1, the Tx's own correspondence:** `Coords::tx`, as built. It is used for `check` and
   `commit`, to judge the expectation.
3. **Rule 2, `key()`:** a naming rule (§4.2). No matcher.
4. **Rule 3, token-level Myers:** refine `Edits::between` in `text.rs`:
   - Run the line Myers as now. It is fast and finds the unchanged lines.
   - Then, within each hunk, run `kept_lines` (already generic over `T: PartialEq`) over the hunk's
     **tokens**: identifiers and numbers, string literals, and single punctuation characters.
     Whitespace and comments are skipped; they fall into the gaps.
   - Emit one sub-hunk per gap between consecutive kept tokens whose bytes differ, each trimmed by
     its common prefix and suffix.
   - Past `DIFF_LINES` differing tokens, keep the hunk whole, as today.
   - A lexer that is wrong about a regex or a template string only makes a token coarser. The hunks
     still cover every differing byte, so `carry` and `Coords` stay exact.
   - About 80 lines. Callers, with HEAD line numbers in `core/session/src/tx/mod.rs`:
     `delta_between`'s `Coords::revisions` (:544), the Tx planner's `outside.carry` (:834), the
     `Write` op's own edits (:884), and the two quick pre-checks that carry a Tx's ranges (:1468,
     :1536). All of them get finer hunks: never wrong, sometimes more lenient where today's whole-line
     trim conflicts.
5. **Rule 4, a unique structural match:** not built. Move it to the existing Deferred row "Keyed
   iteration helpers and better structural matching", whose trigger ("users hear state reset after
   refactors") is exactly what it fixes. What it would recover (cut and paste) resets today too, and
   an ambiguous match resets anyway.
6. **The engine uses rule 3 for every writer.** It keeps calling `delta_between(plays, accepted)`.
   - This gives an agent's file write the same instances as the app's Tx (P2, PLANE-1).
   - For every gesture the two agree on which `(` points survive. Gestures insert or replace whole
     tokens: a literal, a wrap made of two insertions, a cable's argument. The token diff finds the
     same edits, and naming by `(` makes the placement of `)` irrelevant.
   - Reusing the `Committed` delta (as 5.5 suggested) stays an optimisation, not a second semantics.

### 4.2 InstancePath: the chain, spelled for identity

`Side::chain` (delta.rs:250) becomes:

```rust
for f in m.chain(frame) {
    match &f.key {
        Some(k) => segs.push(format!("key:{k}")),
        None => segs.push(format!("{}@{}#{}", f.component.as_deref().unwrap_or(""), spell(open_point(f.site)), f.occurrence)),
    }
}
// key() pins (ID-7, ID-13 rule 2): keys are unique per parent key frame, so nothing positional above
// the innermost key frame is identity.
if let Some(last) = chain_frames.iter().rposition(|f| f.key.is_some()) {
    segs = keys_up_to(last).chain(segs[last + 1..]).collect();
}
```

- `places` (the spans that judge `Confined`) still lists every frame's call range, so judging doesn't
  change.
- Uniqueness still holds. Within a module, a key chain is unique (`key.duplicate`), and below it the
  names are built as today.
- Keyed iteration works with no helper: `for (const k of ["kick", "snare"]) key(k, () => drum(k))` keeps
  every drum's state through reorders and growth. A `key:` segment has no occurrence, so ID-14 never
  touches it.

ControlId (ID-15) is the name of a declared control's const: InstancePath / `@<slot>`. It is already
built (the first row of §1.8).

### 4.3 RuntimeId: the name of a node that holds state

- **Which nodes:** kernel, plugin, poly (the voice allocator and its voice graphs) and feedback (its
  ring). ID-15 lists only `kernel()` and `plugin()`; poly and feedback hold state too.
  - Lanes are identified by `LaneAddr` (saved keys); their schedulers and audio lanes can persist by
    it.
  - Consts are cells and stay per plan (RENDER-23).
  - Time and import nodes hold no state.
- **The name:** InstancePath / `<op>@<file>:<( point>` / `#<n>`, which ID-15 already spells. A body that
  calls `kernel("std:svf")` at two sites has two names.
- **A match from old to new needs all of:**
  1. equal names, the new side named in the old side's coordinates (`delta_between`);
  2. equal **occurrence counts along the path** (ID-14). `Facts` gets `counts: Vec<u32>`: for each
     chain segment with an occurrence, how many frames share its (parent, site); then the size of the
     node's own `#n` group. Names equal but counts differ: fresh. About 25 lines in `chain` and
     `name_all`.
  3. an equal **shape**, which the runtime checks: kernel id, the package's wasm SHA-256
     (`Package.files["std/kernels/std.wasm"]`), `FOp::Kernel.channels`, the ABI version and settings;
     for a plugin, its id and version (`m2_plugins::Info.found`); for a poly, `voices` and channels;
     for a feedback, its channels. This, not the delta's `sig`, decides keeping. So adding
     `key: "k"` to a plugin call (ID-8's first save) changes the delta's sig, but keeps the instance.
     Its new state file holds the state it just saved, so HOME-6's hash check skips the `LoadState`.
- **Second pass, keyed plugins (rule 2 for plugins):** an unmatched new plugin with
  `state: Some(path)` takes the unmatched old plugin of the same module with the same path, id and
  version. It is unambiguous by `plugin.state-collision`. This keeps what `CarryKey.state_path` gave.
- **Optional third pass, for step 8b's key rename:** for each `ModuleChange::Rekeyed { from, to }`
  that the delta found, match `[from, rest…]` with `[to, rest…]`. The same trick works for a track
  file renamed with identical content.
- **Voices:** a kept poly keeps its allocator, which is its voice slots and the notes on them. In each
  slot, inner nodes are matched by their inner names under the same rules. "A voice's RuntimeId is its
  allocator slot" is the slot index, on the runtime side.
- **F8:** for a kept poly or plugin whose `events` or `notes` input is in `delta.rewired`, the arena
  releases its sounding notes at activation, as `carry_from` does today when `same_lane` is false.

### 4.4 Where it runs, and the handoff to the audio thread

```
settle (actor, control thread)
  rev = session.accepted(); p = plays (Arc<Revision> it was built from or adopted)
  (delta, cont) = m2_session::tx::continues(&p.revision, rev)        // new; delta_between + the join
      old Graph: cached on p.revision (its names are its own offsets, §1.8)
      new Graph: named in p.revision's coordinates
      cont: for each new flat stateful node (Top(i) | Voice{poly, node}) -> Option<old flat node>
            by name + counts, then keyed plugins by state path
  if delta is values-only / lanes-only: SetCell / SwapLane as today
  else: Plan::prepare(rev.flat, data, packages, &arena_view, &cont)
            per node: Keep(old slot) when the shape matches, else a fresh instance
            (plugins created and activated on the plugin main thread while the old plan plays)
        send Activate(plan, slots)                                   // slots: Vec<Option<u32>> + fresh boxes
audio thread, at a block line
  for each new slot: take old_arena[k] if Keep(k), else the prepared fresh instance  // index moves only
  old leftovers -> retired queue (off-thread drop)
```

- **Names never reach the audio thread.** It sees dense slot indexes. RENDER-28 holds: no hashing, no
  allocation (the new arena's `Vec` is allocated off the thread, inside `slots`).
- **The contract's `arena: Map<RuntimeId, Slot>` is, in practice, `Vec<Slot>` aligned with the plan,**
  with `cont` mapping each new slot to an old one.
- **`LoadState` needs no RuntimeId.** The delta lists states by `module/frames/key`, and the plan's
  `FOp::Plugin.state` has the same path. So the actor finds the instance by state path (saved keys)
  and hands the blob to the plugin main thread (RENDER-26b, HOME-6).
- **Cost:** two IR walks, about 3,000 nodes at p50 (estimated), plus `format!` per segment. The old
  side is cached. Measure this inside the 200 ms gate.

### 4.5 ID-13, as the code will then have it

1. The Tx's own edits judge the Tx.
2. `key()` frames are named by key and cut the chain above them.
3. A token-level diff refines line hunks, and sites are named by their `(` point.
4. Deferred.

"An ambiguous match gets a fresh id" holds by construction: names are unique (the debug assertion in
`scope`, and `delta_names_are_unique_on_every_fixture`), and a changed count resets the whole group.

## 5. What this lets us delete or unify

- **`CarryKey`, `Builder.ordinals`, `Carry`'s pool matching and `InstanceSource::take`'s key**
  (`core/runtime/src/plugins.rs`, `core/runtime/src/lib.rs:581` and `:670-690`).
  - The plugins.rs header already plans this: "Step 7's arena replaces `Carry`, `carry_from` and
    lending".
  - The state hash in `CarryKey` gives way to `LoadState` (RENDER-26b).
  - `carry_from`'s handle pairing gives way to `cont`. Its note logic (adopt, release) stays, keyed by
    `cont`, until schedulers persist by `LaneAddr`. Then `NoteScheduler::adopt` goes too: a scheduler
    that persists keeps its sounding notes.
- **`same_layout` and `value_writes`** (actor.rs:151,160) are optional. A values-only delta names each
  changed const. Join each one to the playing plan's `CellId` by name: through `cont` extended to
  consts, then `CellAt`.
  - If every changed const is a cell there, write it; otherwise activate. This replaces the clone and
    comparison of the whole `Flat`.
  - `adopt`'s `CellMap::of(new)` re-key comes from the same join, so the "inherited control's first
    edit" case needs no layout equality.
  - Do this only once `cont` exists and the step 6 tests (`a_failed_build_then_a_lane_edit_rebuilds`
    and the lease tests) are green on it.
- **`controls::carry_text`, `carry_ends`, `carry_lines`** (crates/editor) duplicate WRITE-12's carry
  with different line rules. Use `m2_session::text::Edits::between(old, new).carry(start..end)`, so
  leases, cables and control ids carry by the same correspondence as Tx ranges and undo. WRITE-22's
  "by its whole call" stays: carry the call's range.
- **`m2_ir::control_id`**, which only a test uses. Delete it, or redefine it as the control const's
  name.
- **RENDER-25a's crossfade gets its unit for free.** A module whose stateful nodes all continue, and
  which no delta entry names, needs no fade: its instances are processed once, in the new plan. So
  the arena doesn't need step 3a's whole-renderer fade, nor step 3b's `lend` (copying a shared
  instance's output during the fade), for untouched modules. Only replaced subgraphs fade.

## 6. Proposed contract edits

| Rule | Now | Proposed |
|---|---|---|
| ID-13 | "the session MUST give each site a `SiteId(u32)` when it accepts a code revision, by matching against the previous code revision in this order: 1. the Tx's own correspondence; 2. `key()`; 3. carrying through a token-level Myers diff; 4. a unique structural match…" | "A site's identity across two revisions (its SiteId) is its file and the point of its argument list's `(` in the reference coordinates of the comparison (WRITE-8). Nothing numbers sites across revisions. A Tx is judged in its own edits' coordinates; every other comparison, the engine's included, uses `Edits::between`: a line diff whose hunks a token-level Myers diff refines. A `key()` frame is named by its key and cuts the positional chain above it. An ambiguous match gets a fresh identity, and its state resets." Rule 4 moves to Deferred. *Why:* a numbered SiteId would make `DeltaHash` depend on session history (F4), and the engine compares only what plays with what is accepted. |
| ID-14 | as is | add: "Applied when the arena matches instances; a `key()` frame has no occurrence." |
| ID-15 | "RuntimeId = InstancePath / (the site of the `kernel()` or `plugin()` call …, its occurrence)" | "… of the `kernel()`, `plugin()`, `poly()` or `feedback()` call …. InstancePath segments are `key:<k>`, or `<ComponentId>@<file>:<point>#<occurrence>`. A keyed plugin unmatched by RuntimeId matches by its state path." |
| WRITE-8 | "each frame a call site given as its byte range in the new text … From step 7, SiteIds replace the mapped ranges." | "each frame its ComponentId and its call's SiteId (ID-13); a node made at a site, that site's SiteId … The points stay; there is no separate SiteId." Folds in C17 (base coordinates). |
| WRITE-12, WRITE-22 | "by line diff" | "by `Edits::between` (ID-13)" |
| RENDER-25 | "Activation keeps every instance whose RuntimeId and shape match." | add: "The match is computed off the audio thread, between the plan that plays and the new one, and sent as slot indexes." The shape includes the plugin version (already listed) and the package's wasm hash. |
| RENDER-26a | "An activation that replaces a note consumer first sends an Off …" | "… that replaces a note consumer, or keeps one whose notes input now reads another lane, …" (F8) |
| RENDER-23 *Today* | "cells live and die with their renderer until the arena keeps them across plans (step 7)" | Cells stay per plan; a lease re-targets on activation, as it does today. |
| Engine shape | `arena: Map<RuntimeId, Slot>`, `LoadState(RuntimeId, Arc<Blob>)` | `arena: Vec<Slot>` (in plan order), `Activate(Plan, Slots)` where `Slots` holds the old slot each new one takes; `LoadState` addressed by module and state path. |
| HOST-5 | "matched by module …, plugin id, ordinal and state hash. From step 7 … by RuntimeId." | Drop the step 3 matching sentence once `CarryKey` is gone. |
| DIAG-3 | "from step 7 the notice also compares SiteIds" | "… compares the SiteId of each `track()` call", which needs `Module.site` (the `track()` call's site; `takeSite` already has it in core.ts:1610). |

## 7. Tests (each names its rule)

`core/session/src/text.rs`:

- `token_hunks_are_an_edit_script`: a property test that applying the refined hunks to old gives new,
  and that the hunks are sorted, disjoint and on character boundaries. Extend the existing random
  carry property: carried bytes equal.
- `two_literals_on_adjacent_lines_are_two_hunks` (F1).
- `a_reformat_changes_no_token`: whitespace and newlines moved, so no hunk holds a token start.

`core/session/src/tx/delta_tests.rs`, all through `delta_between` (the engine's path):

- `a_wrap_by_a_file_write_keeps_the_wrapped_names`, on the p50 track text above, and the nested
  `reverb(pad(…))` → `reverb(delay(pad(…)))` (F2). Add the same for the unwrap.
- `adjacent_value_edits_are_values` (F1): exactly two `ValueChange`s, nothing structural.
- `a_component_swap_at_one_call_is_new_and_an_alias_is_not` (F3, PKG-9).
- `names_ignore_everything_above_a_key_frame`: move a `key("x", …)` block between two component calls
  (F6, ID-7).
- `old_side_names_are_own_offsets`: the old-side graph equals the one computed against any other
  revision (the cache's premise).
- Keep `delta_names_are_unique_on_every_fixture` (its spelling changes) and the step 6 two-process
  hash test.

`m2_session::tx::continues`:

- `an_insertion_above_continues_everything` (the step 6 gate's edit).
- `a_label_rename_continues_everything` (ID-10).
- `a_loop_that_grows_resets_its_occurrences` and `a_keyed_loop_keeps_through_reorder_and_growth` (ID-14).
- `a_moved_keyed_plugin_continues`, and `adding_a_plugin_key_continues_its_instance` (ID-8, F7).
- `a_rewired_notes_input_is_reported` (F8).

Runtime, through the arena:

- `a_rebuilt_kernel_module_starts_from_init`: same kernel id, new wasm hash (the step 7 gate,
  RENDER-25).
- On p50, the `add-effect` gate run at 3:00 in `crates/gates` (`server.rs:269`): every untouched
  track's and the wrapped pluck's kernels continue with no reset. Count `Renderer::resets` per module,
  and check sample continuity at the swap block on untouched tracks' outputs, bit-equal to a run
  without the edit. The wrapped track crossfades.
- Measure the cost of `continues` at p50 into the gate's 200 ms.

## 8. Risks and open points

- **Myers can still align repeated tokens wrongly** in pathological edits, such as many identical
  `kernel(` calls in a row with one deleted from the middle. The worst case is a neighbour's state,
  where today everything resets. The sig and shape checks bound it to same-kind instances, and the
  ID-14 counts catch the common loop case. This is the same exposure ID-14 accepts for reorders.
- **Package TS edited during a session** (developers editing std): package files are never in `coords`,
  so moved package sites rename their nodes, and the instances reset. That is conservative. Mapping
  package files through `Edits::between` needs their old texts; defer it.
- **Wrapping a call in `key()`** (ID-8's first save, when `key:` can't be inserted at the call) adds a
  `key:` segment, so the plugin restarts with the state it just saved. Its tail and voices are lost.
  This is rare, and no worse than step 3b. A Tx-side hint could bridge it if producers notice.
- **The token lexer is TypeScript-shaped, not exact.** That is safe by construction (§4.1), but a
  string or comment lexed wrongly makes coarser hunks, and so more resets in that region.
- **For the owner:** confirm F4's reading, that SiteId is a point and not a counter, before ID-13 is
  edited. The rest follows from it.
