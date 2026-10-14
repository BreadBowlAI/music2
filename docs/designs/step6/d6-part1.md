# Step 6 design: Tx and the write path

Against `docs/architecture.md` (normative) at HEAD `933f4de`. Revision 2: the first draft revised after
a 27-point critique; the decisions come first (R), then the whole design. Step 6's line in the order of
work:

> The `save`/`Tx` split, with validated paths, the `Clips` op and base texts; the project lock (no
> forwarding), its read-only open and `lock.unsupported`; content-hash bases. `GraphDelta` with node names
> an insertion can't shift, and `Identical`, `Values`, `Confined` and `Seen`; the journal, with host names,
> staging in `.music2/tx/`, and recovery as a diagnostic. `(code_rev, data_rev)`, and a data-only path that
> skips QuickJS, chosen by path class; the watcher's settle rule; `data.claimed` from Tx. `Session::open` on
> a broken draft. Port the 7 gestures, step 1's two fixes and the bypasses; fixes become Tx; delete the
> guess at `graph.rs:341-348`; make `write_whole` private. First new builders: Relink of a whole module
> folder, rename label, automate a knob. Then the move into `crates/editor`.

Gate: the 7 gestures and step 1's fixes are ported, and their existing tests pass with the same meaning.
Property tests over Tx hold: bytes outside the declared ranges are unchanged, the inverse restores the
original, and hostile paths and ids are refused. `check(tx)` then `commit(tx)` write the same bytes, with
equal delta hashes. Inserting one unconnected call at the top of `song.ts` gives a delta of exactly that
call's nodes, with the same hash in two processes. A lane file changed by an outside writer is accepted
without running QuickJS, and a lane file written in two halves never produces a revision in which that lane
is empty. Undoing a clip edit after an outside change to that clip is a `Conflict` naming the lane and the
clip. A CLI write while the app holds the lock fails, changes nothing and names the holder; `m2 bounce`
without `--place` succeeds. Killing the process at every commit step and reopening offers `tx.interrupted`,
and both Finish and Discard leave a project that opens. A project with broken code opens, the code editor
saves, and playback starts when the code compiles. `write_whole` is private, and the guess at
`graph.rs:341-348` is gone.

Rules this step builds or touches: WRITE-1…WRITE-21 (all of Writes but leases, which change only where
their commit becomes a `Values` Tx and their ids become anchors, WRITE-22), PLANE-1, PLANE-2, PLANE-3,
PLANE-5, RENDER-26 (the delta decides `SwapLane`), FMT-5, FMT-11, FMT-12, FMT-14, FMT-16, FMT-17, FMT-20,
ID-2 (the key helper), ID-10, ID-12, ID-17 (relink of a folder), HOME-2 (automate), DIAG-1…DIAG-6,
P2, P3, P4, P10.

Hard constraints carried from the brief: offline renders stay byte-identical (`scratchpad/baseline/`),
RENDER-33, the allocation trap, the step-3 and step-5 gates. Nothing in this step changes the runtime's
DSP, a kernel, std or the SDK's sound; the only `core/script` change is a counter of QuickJS runs (for
the "no QuickJS" gate) and the move of two source helpers out of it.

---

## R. Review decisions (the critique of the first draft)

Each point accepted or rejected, with the reason in one line and where the design now says it.

| # | Point | Decision | Why | Where |
|---|---|---|---|---|
| 1 | The barrier came before the folder syncs, staging folders were never synced, and renamed targets' folders were never pushed | **Accept** | A drive-cache flush covers only what was pushed before it; the renames must be pushed before the next barrier can drain them | 6a.2, 6a.3, 19 |
| 2 | Expectations judged against the accepted revision when a touched file changed on disk but isn't taken in yet; gestures still call `refresh()` | **Accept** | An outside change in the touched file isn't the Tx's own change, and a gesture must never bypass the settle rule | 4.2 (baseline B), 13.3, 16, 17.1 |
| 3 | Hybrid revisions split one settled batch into a data half and a code half | **Accept** | WRITE-19 takes one snapshot as one revision; only a batch with no code change may take the data path while the code is broken | 3.3, 3.5, 8.3, C8 |
| 4 | The engine path came from the delta between accepted revisions, not from what plays | **Accept** | After a failed build the next lane-only change would swap into a stale renderer and the structural change would never play | 5.5, 17.1 |
| 5 | `Confined` mapping at range boundaries was undefined, and add-effect would be refused | **Accept** | A Tx's own edits must map inclusively, while carrying across outside changes and naming nodes stay strict | 2.4, 6.2, 6.3 |
| 6 | `Clips` can't address anonymous, invalid-id or duplicated clips, so the piano roll would duplicate notes | **Accept** | A clip with no usable id has no address; such a lane is written whole, and a duplicated id is a `Conflict` | 2.5, 12, 14.3 |
| 7 | C9 made every crash after an edit an Error that blocks editing | **Accept** | A journal whose every target is at its after hash changes no project file, so removing it is tidying, not recovery (P3) | 6a.3, 6a.4, C9 |
| 8 | Failures during staging and renames had no defined handling | **Accept** | Rolling back a half-done commit in the process that holds the lock is the commit's own atomicity | 6a.6 |
| 9 | The window between compare-and-swap and rename was wide, and creates could clobber | **Accept, with fallbacks** | Checking at each rename, `RENAME_EXCL` and `RENAME_SWAP` narrow the race to what WRITE-19 says can't be closed | 6a.2 |
| 10 | Hard-linked before-images share the inode with the live file | **Accept** | An in-place writer would corrupt the only copy; `clonefile` shares no inode | 6a.1 |
| 11 | `gethostname` changes with the network on macOS | **Accept** | Identity by `gethostuuid`; the name only for messages | 6a.4, 7.1, C18 |
| 12 | The watcher's rebase folded outside writes into its baseline, and `save()` read the whole folder | **Accept** (`save` takes in the settled draft plus its own file) | Own writes may only update their own paths' baseline, and a save must not take in someone's half-written files | 3.5, 8.1, 8.2, 17.4 |
| 13 | `.git/index.lock` had no staleness rule and was looked for only at the project root | **Accept** | A stale lock would stop every take-in silently, and worktrees and enclosing repos have no `.git/` at the root | 8.2, C2 |
| 14 | The poll stats and hashes everything, unrelated growing assets hold every take-in, and stat alone misses same-size rewrites | **Accept; FSEvents deferred** | Quiet is judged over revision inputs, binaries are hashed once stable, text files follow FMT-16's racy rule; FSEvents waits for a measured walk cost | 8.2, 19, 24 |
| 15 | Op paths could overlap, and disk order differed from op order | **Accept** (refuse overlaps) | No builder needs overlapping paths, and refusing them makes order irrelevant in the candidate and on disk | 1.3, 19 (c) |
| 16 | Read-only readers never saw an interrupted commit | **Accept** (the lock held for the scan only, writers retry briefly) | DIAG-4 promises diagnostics to project reads, which are how agents read | 6a.4, 7.1, 18, C19 |
| 17 | Delta names used absolute offsets in the new text, so `Seen` broke when someone typed elsewhere; a second read of a lane renamed the first | **Accept** | WRITE-12 wants the same outcome whatever versions a session saw; names move to the base text's coordinates, and lane nodes always carry their site | 2.4, 5.2, 5.4, C17 |
| 18 | `F_FULLFSYNC` on the critical path threatens the 20 ms budget, and G10 measured only an idle disk | **Accept, as an owner question** | Measure under load; both ways out (an ordering barrier, or a preview swap) change PLANE-5's wording | 19 (G10), C20 |
| 19 | Fix Tx ids lost the file's content as an input, so derived clip ids repeat across contents | **Accept** | The same fix on different contents must not mint the same ids (FMT-11) | 11 |
| 20 | Kill points under `cfg(test)` are invisible to an integration test | **Accept** (the test moves into the crate's own test module) | Integration tests link the library built without `cfg(test)`; no kill switch is compiled into release builds | 6a.5, 19 |
| 21 | Porting gestures one by one ran two undo stacks side by side | **Accept** | Interleaved undo order must hold at every checkpoint | 10.1, 22 |
| 22 | An inverse's expectation mirrored the forward's and refused legitimate data restores | **Accept** | A data-only inverse is guarded by its bases; mirroring `Identical` would refuse restoring bytes a lane now reads | 10.2 |
| 23 | `"ranges": {"song.ts": []}` meant the whole file | **Accept** | An empty list reads as "no bytes"; "whole" becomes explicit | 1.1, 1.2, 6.3, C1 |
| 24 | Non-UTF-8 text files were handled lossily | **Accept in part** | `TextEdit` and `Clips` on such a file are refused; `save()` and a whole `Write` stay allowed, since re-saving as UTF-8 is the remedy | 1.3, 1.5, 2.1 |
| 25 | The draft rule blocks a code Tx while an unrelated file is broken | **Reject** | Three lease tests (C5 of step 3b) depend on a commit being Pending while another file is broken, and the gate keeps their meaning; offered to the owner as a later relaxation (C21) | 4.2 step 3 |
| 26 | `Session::candidate` duplicated `check` | **Accept** | `check` under `Seen` already returns the unverified candidate, so core needs no second entry (P10) | 4.1, 14.2 |
| 27 | `flock` can succeed on a network volume without excluding other Macs | **Accept** (a Notice) | The user must know the lock stops only this Mac's programs | 7.1, C2 |

---

## 0. Summary of the shape

```
            ┌──────────── crates/editor (new) ─────────────┐
 web ─────► │ builders: set, connect, disconnect, add/remove│ ──► Tx ──┐
 m2 CLI ──► │ effect, write notes, relink, delete data,     │          │
 server     │ rename label, automate, plugin state, place   │          ▼
            │ planners' assertions (wrap, excise, rewire)   │   core/session: check(tx) / commit(tx)
            │ controls & ids (labels), view, timeline,      │     validate (WRITE-3a)
            │ lens, render/export, source helpers           │     bases: CAS / carry (WRITE-12)
            └───────────────────────────────────────────────┘     baseline B: accepted + touched files as on disk
 agents ──► POST /api/tx/check|commit, m2 tx ──── Tx JSON ───►     candidate: compile, or data-only
                                                                  GraphDelta B → candidate, base-text names (WRITE-8)
 text editors, git, agents' raw writes ──► watcher (settle) ──►   expectation (WRITE-6)
 code editor ──► save(file, base_sha, text) ──────────────────►   journal, staging, syncs, barrier, checked renames
                                                                  accept (code_rev, data_rev)
                                                                  inverse Tx onto the undo stack
                                                                         │
                                       server actor ◄── Accepted { rev, plane, delta } (Arc<Revision>)
                                       delta(what plays → accepted) → SetCell | SwapLane | Activate
```

- **The Tx type lives in `core/song`** (`m2_song::tx`): it is data, the same JSON for the server,
  `m2 tx`, MCP and later TypeScript tools, and `m2_song::Fix` must carry one (DIAG-1), and fixes are built
  in `core/song` too. Validation of a Tx's shape and paths (WRITE-3a) is there, beside `DataPath` (FMT-5).
- **`core/session` runs Tx**: bases, the baseline, candidates, the delta, the expectations, the journal,
  the lock, undo, `save()`, the watcher's take-in and the settle rule. It holds no gesture: after the move
  it has no `set`, `connect`, `add_effect` or `write_notes`, no knob ids, no labels and no view.
- **`crates/editor` builds Tx**: each gesture is a function from the accepted revision (read through
  `Session::accepted()`) and a request to a `Tx`, plus the planner's own assertion run on the checked
  candidate. Core never depends on it (a test checks the crate graph). No gesture takes in the disk first:
  only the watcher, a save and a test's `refresh()` do (R2).
- **The server** drives the engine from the delta between **what plays** and the accepted revision
  (values → cell writes, lanes → `SwapLane`, anything else → a new renderer), so `Open::single` (the guess)
  is deleted and a failed build is retried at the next settle (R4).
- **Revisions are `(code_rev, data_rev)`**: a lane write, from a Tx or from outside, takes the data path
  (no QuickJS) and leaves `code_rev`, control ids and leases alone. A settled batch is taken in whole or
  waits whole (R3).
- **Delta names are in the base text's coordinates** (R17): two equal changes hash equally in every
  process, whatever else changed in the file meanwhile.

---

## 1. The Tx type (`core/song/src/tx.rs`, new)

### 1.1 Rust

```rust
/// A verified edit (WRITE-3): every file it reads or writes, with the content it was planned on; the
/// ops; what it may change (WRITE-6). One Tx is one journal entry and one undo step.
pub struct Tx {
    pub id: TxId,                              // metadata, ^[0-9a-z-]{1,32}$ (WRITE-3a)
    pub base: BTreeMap<TxPath, Base>,          // exactly one per file an op touches
    pub label: (String, String),               // what it said before and says after, for history
    pub ops: Vec<Op>,
    pub expect: Expect,
    pub hint: Option<u64>,                     // a code revision; only a hint (WRITE-12)
}

pub enum Base { Sha(Sha), Absent, WithText { sha: Sha, text: Arc<str> } }

pub enum Op {
    TextEdit { file: TxPath, edits: Vec<Edit> },
    Write { file: TxPath, bytes: Payload },
    Remove { file: TxPath },
    Rename { from: TxPath, to: TxPath },               // a file, or a folder data/<module>/
    Clips { lane: LaneAddr, put: Vec<serde_json::Map<String, Value>>, delete: Vec<String> },
}

pub struct Edit { pub start: Pos, pub end: Pos, pub expected: String, pub text: String }
pub enum Pos { Byte(u32), LineCol { line: u32, utf16_col: u32 } }    // line and column count from 0 (C5)

pub enum Payload {
    Text(Arc<str>),
    Bytes(Arc<[u8]>),
    /// A file under `.music2/tx/` with this content (C3): a staged write or a stored before-image.
    /// Only recovery fixes and the undo of a binary write carry one; core checks the hash.
    Local { path: String, sha: Sha },
}

pub enum Expect {
    Identical,
    Values(Vec<(Anchor, f64)>),
    Confined(Confined),
    Seen(DeltaHash),
    /// C3: the Tx restores exactly what journal `id` recorded; checked against the journal, never the
    /// graph. Only FinishTx and DiscardTx carry it.
    Journal(JournalId),
}
pub struct Confined { pub ranges: BTreeMap<TxPath, Declared>, pub lanes: Vec<LaneAddr>, pub modules: Vec<ModuleChange> }
/// What a file may change (R23): the whole file, or these ranges of its base text (`[]`: nothing).
pub enum Declared { Whole, Ranges(Vec<Range>) }
pub struct Range { pub start: Pos, pub end: Pos }
pub enum Anchor { Written { file: TxPath, range: Range }, Slot { site: Spot, slot: String } }
pub struct Spot { pub file: TxPath, pub range: Range }
pub enum ModuleChange { Added(Key), Removed(Key), Rekeyed { from: Key, to: Key }, Muted { module: Key, muted: bool } }
pub struct DeltaHash(pub Sha);                 // "sha256:<64 hex>"
pub struct TxId(String);
pub struct JournalId(String);                  // named by core, never by the Tx (WRITE-3a)
/// A path WRITE-3a admits, with its parse.
pub struct TxPath { text: String, path: DataPath }
```

`ModuleChange::Muted` is in the type now and inert until `Module.muted` exists (MON-5, step 8b). Every
position in a Tx (edits, ranges, anchors) is in its file's **base text** (WRITE-3).

### 1.2 JSON (one schema: server, `m2 tx`, MCP, TS tools)

Strict: every object denies unknown fields (FMT-8's rule applied to Tx), and a malformed Tx is
`Rejected` with `tx.invalid` (C2), never a parse panic.

```json
{"id": "tx-7k2m9q4c",
 "base": {"song.ts": "sha256:4be1…", "data/song/hats-pan.json": null,
          "tracks/bass.ts": {"sha": "sha256:09ab…", "text": "…the base text…"}},
 "label": ["hats pan 0.3", "hats pan automated"],
 "ops": [{"op": "text_edit", "file": "song.ts",
          "edits": [{"start": 327, "end": 329, "expected": "()", "text": "({ lane })"},
                    {"start": {"line": 14, "utf16_col": 37}, "end": {"line": 14, "utf16_col": 40}, "expected": "0.3",
                     "text": "lane.control(\"hats-pan\", \"bipolar\", 0.3)"}]},
         {"op": "write", "file": "data/song/hats-pan.json", "text": "{\"unit\": \"bipolar\", \"clips\": []}\n"},
         {"op": "write", "file": "data/lead/lead.state", "base64": "…"},
         {"op": "remove", "file": "data/kick/old.json"},
         {"op": "rename", "from": "data/hat", "to": "data/hats"},
         {"op": "clips", "lane": ["kick", "hits"], "put": [{"id": "k3f9q2zt", "start": 0, "length": 16, "notes": []}],
          "delete": ["q7m2c8xa"]}],
 "expect": {"confined": {"ranges": {"song.ts": [[327, 329], [422, 425]], "tracks/bass.ts": "whole"},
                         "lanes": [["song", "hats-pan"]], "modules": []}}}
```

- `expect` is `"identical"`, `{"values": [[{"written": {"file": "song.ts", "range": [422, 425]}}, 0.45],
  [{"slot": {"site": {"file": "song.ts", "range": [300, 340]}, "slot": "q"}}, 0.7]]}`,
  `{"confined": {…}}`, `{"seen": "sha256:…"}` or `{"journal": "<journal id>"}`. `modules` entries are
  `{"added": "probe"}`, `{"removed": "kick"}`, `{"rekeyed": ["kick", "kicks"]}`, `{"muted": ["kick", true]}`.
- A `ranges` value is a list of ranges, or the string `"whole"` (R23). `[]` declares no bytes of that file.
- `write` has exactly one of `text`, `base64` (standard alphabet, padded) or, only for C3's payload,
  `{"local": ".music2/tx/blobs/<hex>", "sha": "sha256:…"}`.
- A position is a byte offset (a number) or `{"line", "utf16_col"}`; a range is `[start, end]`.
- `hint` is optional; `label` is optional on input (default `["", ""]`).
- The canonical form of a Tx (for the check cache and tests) is its JSON in RFC 8785 form (1.4).

### 1.3 Validation (WRITE-3a), before anything is read

`Tx::validate(&self) -> Result<(), Vec<Diagnostic>>` (pure; `core/song`):

1. `id` matches `^[0-9a-z-]{1,32}$`, else `tx.invalid` ("a Tx id is 1 to 32 of `a-z`, `0-9` and `-`"; `tx.path`
   is for paths, C2).
2. Every path (the keys of `base`, `file`, `from`, `to`, a `Clips` lane's file, the files of
   `ranges`, anchors and spots) is relative, `/`-separated, has no empty, `.` or `..` segment, no `\`,
   NUL or control character, is UTF-8, and **round-trips**: `DataPath::parse(p).path() == p` (so
   `data//kick` and `data/aux/` for the key `aux` are refused; ID-9's `aux-` spelling is the only one).
   Its class must be Code, Manifest, Arrangement, Module, Lane, PluginState, Asset or Presentation;
   `Local` and `Unknown` are `tx.path`. A `Clips` lane is two keys (`Key::new`), and its file is
   `DataPath::lane(addr)`.
3. Shape (`tx.invalid`, C2): every path an op touches has exactly one base, and every base is touched
   (a stray base is a mistake an agent should hear about); a `Module` path appears only in a `Rename`,
   whose ends are both `Module`; a `Rename`'s ends have the same `DataPath` kind (a lane to a lane, a blob
   to a blob, a sidecar to a sidecar, code to code); a `Rename`'s `to` and an `Asset` `Write` have base
   `Absent` (FMT-14: assets are created, never replaced); `Clips` at most once per lane, every `put` clip
   has an id that `ClipId::valid` accepts (core never mints, FMT-11), ids are unique in `put`, and no id
   is both put and deleted; `Values` numbers are finite; anchors and `ranges` name files that have a base.
4. **No overlap (R15).** The op paths (a `TextEdit`'s, `Write`'s or `Remove`'s file, a `Clips` op's lane
   file, and each end of a `Rename`) are pairwise disjoint: none equals another, and none lies inside a
   folder another names (`data/kick` and `data/kick/hits.json` overlap, so do `data/kicks` and
   `data/kicks/hits.json`). Ops are then independent, and their order changes nothing, in the candidate or
   on disk.
5. `Declared::Ranges([])` for a file that a `Write`, `Remove` or `Rename` touches is `tx.invalid` (R23):
   a whole-file op on a Code or Manifest file must declare it `Whole`.
6. Positions are resolved and checked against the base text later (2.4), since `LineCol` needs it.

Checks that need the disk run under the lock (4.2 step 4): a target that is a symbolic link, or whose
nearest existing parent resolves outside the project, is `tx.path`; a created name that the project has
in other capitals (`data::taken`, moved into `core/song`) is `tx.path` with today's "on a disk that
ignores capitals" message; a `TextEdit` or `Clips` target whose current bytes aren't UTF-8 is `tx.invalid`
naming the file ("isn't UTF-8: re-save it as UTF-8 first", R24). A `Write` over such a file is allowed: it
is how the file gets fixed.

### 1.4 Canonical JSON and hashes (`core/song/src/jcs.rs`, new)

`DeltaHash` is SHA-256 of the delta's JSON in RFC 8785 (JCS) form (WRITE-8). A small serializer over
`serde_json::Value`: object keys sorted by UTF-16 code units, no white space, strings escaped as JCS says,
numbers in ECMAScript `Number#toString` form (shortest round trip, `1e+21` from 1e21 up, `1e-7` below 1e-6,
no `-0`). Tested against the RFC's examples and a table of floats, including every value that appears in
the fixtures' consts. No dependency: about 80 lines. The same function hashes a Tx for the check cache.

### 1.5 Other `core/song` additions

- `Sha` serialises as `"sha256:<hex>"` and parses that form (`Sha::parse_prefixed`).
- `dir_hash(project_files, module) -> Option<Sha>` moves from `unclaimed.rs::folder_hash` into
  `core/song/src/path.rs` beside `DataPath` (WRITE-12's directory hash is a path rule TS tools must
  reproduce), working on a list of `(relative path, sha)` so the session can hash a folder on disk too.
- `Snapshot.not_utf8: BTreeMap<PathBuf, Sha>` (R24): the raw-byte hash of each text file that isn't UTF-8
  (its `files` entry stays the lossy text, for the compile and the code editor), and `Snapshot::sha(path)`,
  which returns the hash of the bytes on disk for every text file. Builders take bases only from it.
- `LaneData::clip_lines() -> Vec<(Option<&str>, String)>`: each clip's canonical line (`Clip::line`,
  made public through this) with its id, which the `Clips` rule compares byte for byte;
  `LaneData::addressable() -> bool`: every clip has a valid id and no id repeats (12).
- `LaneData::put_clips(&mut self, put, delete)`: the `Clips` op on parsed data (2.5).
- `Project::empty(dir, name)` for the revision a broken project opens with (9).
- `data::taken` and `data::under` move from `core/session/src/edit/data.rs` to `core/song` (they are
  path rules).

---

## 2. Bases, the base-text store and mapping (WRITE-12)

### 2.1 What "current" is

Every base is compared with the file **on disk now**, read under the lock (the session holds it for its
life, 7): text files by the SHA-256 of their bytes (never of a decoded string, R24), binaries (state blobs,
assets) by `Sha::of_file` when they are a target (exact, not the hash cache), and a `data/<module>/`
folder by its directory hash over what is on disk (dot files skipped; any other file that `DataPath`
doesn't parse as Lane or PluginState makes the `Rename` refuse, naming it).

### 2.2 The rule

For each `(path, base)`:

- base equals current → the ops apply to the current content;
- `Absent` but the path exists → `Conflict`; a name in other capitals exists → `tx.path` (1.3);
- the path is **waiting** (a settled outside change held back with code that doesn't compile yet, 3.3) →
  `Conflict` for that file, whose message says it changed on disk together with code that doesn't compile
  yet (R3: a Tx never takes in half of a waiting batch);
- otherwise the base text is `WithText.text` (whose hash core checks; a mismatch is `tx.invalid`), else
  the store's text for that hash (2.3), else the text of that hash in the code revisions the session
  remembers (`hint` first); with none, `Conflict { file, current }`;
- with a base text, only text ops carry: a `TextEdit`'s ranges are mapped from the base text to the
  current one by `Edits::between(base, current).carry` (2.4); an edit that a change touched is `Conflict`
  for that file. `Confined` ranges and anchors are **not** carried: they stay in base coordinates, where
  the delta's names are (5.2). `Write`, `Remove` and `Rename` never carry: a base that doesn't equal
  current is `Conflict`.
- `Clips` has its own rule (2.5).

`Conflict { files: Vec<ConflictFile> }` lists every base that doesn't apply, not just the first.
`ConflictFile { file, current: Option<Sha>, clip: Option<String>, why: Option<String> }` (C4: a `Clips`
conflict names its clip, WRITE-21; `why` says "changed on disk and doesn't compile yet" and the like).

### 2.3 The base-text store (`core/session/src/texts.rs`)

`Texts { by: HashMap<Sha, Arc<str>>, order: VecDeque<Sha>, bytes: usize }`: recent text files by content
hash, least recently used out first, bounded at 32 MiB or 8,192 texts. It takes every text file each
accepted revision changes (code, lanes, arrangement, sidecars, manifest), every `WithText` base a Tx
brought, and every text the session wrote. It is only a cache (WRITE-12): agents and `m2 tx` SHOULD send
`WithText`, and the app's builders always plan against the accepted revision, whose texts are always in
it. Code revisions keep their code texts anyway (3.4), so a code base is found even after eviction. A
file that isn't UTF-8 is never stored (its decoded text isn't its bytes).

### 2.4 Text mapping (`core/session/src/text.rs`, new; replaces `carry`, `carry_ends`, `carry_lines`)

```rust
/// What changed from one text to another: hunks, each a byte range of the old text replaced by one of
/// the new, sorted and disjoint.
pub struct Edits(Vec<Hunk>);
struct Hunk { old: Range<usize>, new: Range<usize> }

impl Edits {
    /// Myers' diff over whole lines (as today's `kept_lines`, capped at DIFF_LINES, beyond which the
    /// span between the common prefix and suffix is one hunk), each hunk then trimmed by the bytes its
    /// old and new text share at both ends, so a changed literal is a hunk of the literal alone.
    pub fn between(old: &str, new: &str) -> Edits;
    /// The hunks a Tx's own edits make (sorted, non-overlapping; checked before).
    pub fn of(edits: &[(usize, usize, &str)]) -> Edits;
    /// `self`, then `next` (old → mid → new).
    pub fn then(&self, next: &Edits) -> Edits;
    /// STRICT. Where `r` is after the change, when no hunk touches its inside: the carry of WRITE-12,
    /// used to carry a Tx's edits across outside changes, and for undo and leases. A hunk that only
    /// inserts at `r.start` or `r.end` is outside it.
    pub fn carry(&self, r: Range<usize>) -> Option<Range<usize>>;
}

/// A file's reference coordinates (R17): its base text, with the hunks that lead from it to the text
/// being named. Used for delta names (5.2) and for checking `Confined` ranges and `Values` anchors (6).
pub struct Coords { outside: Edits /* base → current */, own: Edits /* current → candidate */ }
pub enum Point { Base(usize), Own { edit: usize, at: usize }, Outside { hunk: usize, at: usize } }
impl Coords {
    /// Where a position of the text named lies, read by the character it starts (`Side::Start`) or
    /// ends after (`Side::End`): in unchanged text a base offset; inside a hunk's new text, that hunk
    /// and the offset in it. So a call around an edited literal keeps its base range, and a call
    /// inserted around another (a wrap) never shares its range.
    pub fn point(&self, side: Side, at: usize, of: Text /* Current | Candidate */) -> Point;
    /// INCLUSIVE, for a Tx's own edits only (R5): the base range a point stands for. A base offset is
    /// itself; a point inside own edit `i` stands for the base range that edit replaced (an insertion's
    /// is its point); a point inside an outside hunk stands for nothing a Tx can declare.
    pub fn span(&self, p: &Point) -> Option<Range<usize>>;
}
```

There are three mappings, each named where it is used, so a boundary never means two things (R5):

1. **Carry (strict)**: a Tx's own edits across outside changes (base → current), undo's inverse edits,
   and leases. A change that touches a range's inside, or replaces it, stops it.
2. **Points (strict at the character)**: naming nodes. A range `[a, b)` is named by the point of the
   character at `a` and of the character before `b`. A wrapped call keeps its base range because its own
   characters are unchanged, and the wrapping call's range starts and ends inside the inserted text, so the
   two never collide.
3. **Spans (inclusive)**: checking that a node, an edit or an anchor lies inside a declared range. A point
   inside own edit `i` stands for the base range that edit replaced; an insertion at `p` stands for `[p,p]`,
   which lies inside a declared `[s, e]` whenever `s ≤ p ≤ e`. So an effect wrapped by insertions at
   `site.start` and `site.end` is inside `[site.start, site.end]` (`an_added_effect_is_confined`).

`carry` is exactly today's rule where today's applies (a range a change touched never carries), and it
also carries a range past a change elsewhere on its own line, which today's line rule refused; the
existing carry property test (`carrying_finds_a_range_wherever_no_edit_touched_it`) keeps its assertions
for whole-line edits and gains same-line cases. `Session::carry_range(code_rev, file, range)` exposes it
to the editor for ids and leases (16).

`LineCol` positions resolve against the base text: `line` counts `\n` from 0, `utf16_col` counts UTF-16
units from the line's start; a column inside a surrogate pair, past the line's end, or a byte offset off
a character boundary is `tx.invalid`. Edits must be sorted and disjoint after resolving (two insertions
at one point are `tx.invalid`: their order would be a guess). `expected` must equal the base bytes of the
range, else `Rejected` with `tx.expected` (C2), which names the file, the range and both texts. A `Write`
of a text file over an existing one counts, for naming and spans, as the own edits
`Edits::between(current, written)`; a new text file (base `Absent`) as one insertion at 0.

### 2.5 The `Clips` rule

The lane file's base text (from `WithText`, the store, or the file itself when base equals current) and
current text are read as the lane's declared kind (the accepted code revision's declaration; a lane no
accepted revision declares is `Rejected`, since its kind is unknown). The op applies to the **current**
data when every clip it puts or deletes has the same canonical line (`clip_lines`) in base and current, or
is absent from both; otherwise `Conflict` names the file and each such clip id. A put or delete whose id
occurs **more than once** in the base or the current file is a `Conflict` naming the clip (R6: which copy
it means is a guess; the RenumberClips fix comes first). Applying replaces the clips with those ids (a put
whose id is absent is added), drops the deleted ids (an absent one is fine: it is absent from both), sorts
and writes `m2_song::canonical`. A clip without a valid id in the file is kept as written (FMT-12). A put
clip must parse as the lane's kind; if it doesn't, `Rejected` with `lane.invalid` naming the clip. A
current file that doesn't parse is never written over (today's
`a_lane_whose_file_doesnt_read_is_never_written_over`): `Rejected` with the file's `lane.invalid`, which
says to fix the file first.

---

## 3. Revisions: `(code_rev, data_rev)` and the data path (WRITE-13, WRITE-14, WRITE-19, PLANE-1, PLANE-2)

### 3.1 Numbers

```rust
pub struct Revision {
    pub number: u64,       // the session's sequence: one per accepted revision (the hint clients send)
    pub code_rev: u64,     // the number of the revision whose Code and Manifest files this one has; 0: none
    pub data_rev: u64,     // the number of the revision whose Data and Arrangement files this one has
    pub snapshot: Snapshot,
    pub project: Project,
    pub compiled: Compiled,
}
```

A change to Code or Manifest files makes `code_rev = number`; a change to Data or Arrangement files makes
`data_rev = number`; both when both change. The pair names a revision (every revision moves at least one),
and each half orders its own kind. This is the numbering the server already keeps for `code_rev`
(`o.code_rev = number`), now owned by the session; `plane::code_changed` and the actor's own bookkeeping go.
The session keeps the accepted revision as an `Arc<Revision>` (immutable once accepted), so the server can
hold the one a renderer was built from (5.5) at the cost of a pointer; `Session::accepted()` still returns
`&Revision`, and `Session::accepted_arc()` the `Arc`.

### 3.2 `Compiled` split, without churn

```rust
pub struct Code {           // what QuickJS, the binder and the checker made: shared by every data revision
    pub ir: Ir, pub flat: Flat, pub sites: Vec<Site>, pub outputs: Vec<Output>,
    pub package_files: Vec<PackageFile>, pub components: BTreeMap<String, Arc<Descriptor>>,
    pub units: Arc<[UnitCtor]>, pub decls: Decls,
    pub kept: Vec<m2_ir::Diagnostic>,       // build problems that don't block: plugin.missing, param-renamed
}
pub struct Compiled { code: Arc<Code>, pub data: DataSet, pub problems: Vec<m2_song::Diagnostic> }
impl Deref for Compiled { type Target = Code; }   // `compiled.ir`, `compiled.sites` keep compiling
impl Compiled { pub fn code(&self) -> &Arc<Code>; }
```

`compile_sources` becomes `build_code(project, packages) -> Result<Arc<Code>, Vec<Diagnostic>>` (QuickJS,
bind, check, decls) and `read_data(code, project, seen, previous, changed) -> Compiled` (lanes, states,
problems, unclaimed). `compile` calls both; renders are untouched.

`read_data` with `changed: Some(paths)` reads only the lane and state files among `paths` and keeps the
previous `Arc<LaneData>` of every other lane (`DataSet::update`), so a lane write costs its own file, not
the project's (the 20 ms budget at p50). A property test checks that the incremental read equals a full
read for random lane edits on every fixture (data, problems, and their order).

### 3.3 Batches, waiting paths and the data path (C8, revised by R3)

A revision's snapshot is the accepted code part (Code, Manifest) and the accepted data part (Arrangement,
Lane, sidecars, binaries). The **draft** is the last settled read of the folder (8.2): what the watcher,
a save or `refresh()` last took in. A **batch** is the set of paths one take-in changed relative to the
draft. A batch is taken in **whole or not at all** (WRITE-19: one snapshot, one revision):

| The batch (by `DataPath::class`, PLANE-1) | Path | Revision |
|---|---|---|
| only Presentation or Ignored | nothing (PLANE-3) | none |
| no Code, Manifest or Arrangement change (lanes, sidecars, blobs, assets) | **data path**: `read_data` against the accepted `Code`, with `changed` = the batch's paths that aren't waiting (no QuickJS); taken even while the draft's code is broken | `data_rev` moves |
| a Code, Manifest or Arrangement change, and the draft builds (the batch with every waiting path) | full compile when Code or Manifest changed (PLANE-2: the slowest plane), else the data path with the new arrangement | `code_rev` and/or `data_rev` move; nothing waits any more |
| a Code, Manifest or Arrangement change, and the draft doesn't build | `draft_errors`; every path of the batch **waits** with the ones already waiting | none |
| any, with no accepted code revision (open broken) | data waits too (no declarations to read it against) | none until the code compiles |

`waiting: BTreeSet<PathBuf>` is the session's: paths of batches held back with code (or an arrangement)
that doesn't build. A later batch that touches a waiting path leaves it waiting. When a later batch makes
the draft build, everything waiting is taken in with it, in one revision. A Tx that touches a waiting path
is a `Conflict` (2.2). So an agent's checkout that renames a lane in code and moves its file in one batch,
with code that doesn't compile yet, never plays the lane empty: the lane plays its accepted data until the
whole batch is taken in (`a_broken_save_and_a_lane_move_in_one_batch_keep_the_lane`, 19).

What stays of C8: while the draft's code is broken, a batch with no code change (a piano-roll edit in
another app, an agent's lane file) is still taken in against the accepted code, as a data Tx is
(WRITE-14). An `arrangement.invalid` blocks only the batch that holds it (FMT-9); lanes that don't read
never block. `Plane::of(paths, needs_render)` (`plane.rs`, which keeps only this) names the plane (Code,
Arrangement, PreparedData, Data, None) for reporting and the server's timings.

### 3.4 What a revision keeps

- The accepted revision keeps its whole snapshot (bases are checked against it and the disk).
- `codes: VecDeque<PastCode>`, the last 64 **code** revisions: `{ code_rev, texts: BTreeMap<PathBuf,
  Arc<str>> (Code and Manifest only), sites }`, for carrying ids and anchors from a client a little behind
  (WRITE-12, WRITE-22). Data revisions keep nothing but their delta's lane list (WRITE-13's "lane diffs,
  not full snapshots"): no past data snapshot is ever read again, since bases are hashes now.
- `Session::snapshot(at)` (today's revision-number staleness checks in `write_notes`, `rewrite_lane` and
  `unclaimed`) is deleted: bases replace it.

### 3.5 Taking in a change from disk

```rust
pub struct Accepted {
    pub rev: (u64, u64, u64),        // number, code_rev, data_rev
    pub plane: Plane,
    pub delta: GraphDelta,           // from the revision accepted before (5), for reporting and reuse
    pub notices: Vec<m2_song::Diagnostic>,  // session-only: data.claimed for an outside write (DIAG-3)
}
pub struct TakenIn { pub draft_changed: bool, pub accepted: Option<Accepted> }
impl Session {
    /// The watcher's take-in (WRITE-19): `snapshot` was read once the batch settled (8.2).
    pub fn take_in(&mut self, snapshot: Snapshot) -> TakenIn;
    /// Read the folder now and take it in as one batch, without the settle rule: the CLI, tests, and
    /// nothing else in the app (R12). Whether the draft changed, as today, so its callers in tests keep
    /// their meaning; `last_taken()` has the `Accepted`.
    pub fn refresh(&mut self) -> bool;
}
```

`take_in` compares the new snapshot with the draft (not the accepted revision): unchanged → nothing; else
the draft is replaced, and the batch is taken in by 3.3's table. `save()` (8.1) takes in the draft with
its one file replaced, never a fresh read of the folder.

---

## 4. `check` and `commit` (WRITE-4, WRITE-5, WRITE-11)

### 4.1 API (`core/session/src/tx/mod.rs`)

```rust
pub struct Checked {
    pub delta: GraphDelta,
    pub hash: DeltaHash,
    pub diagnostics: Vec<m2_song::Diagnostic>,
    candidate: Candidate,                 // in process only: what planners assert on (13.4)
}
impl Checked {
    pub fn candidate(&self) -> &Revision;   // the candidate as a revision would be
    pub fn baseline(&self) -> &Revision;    // B (4.2 step 6): what the delta starts from
}

pub struct Committed {
    pub code_rev: u64, pub data_rev: u64, pub number: u64,
    pub delta: GraphDelta,                  // the same as check's (4.3)
    pub diagnostics: Vec<m2_song::Diagnostic>,
    pub patches: Vec<Patch>,
    /// The paths this commit wrote, with the stamps of the session's own files after their renames:
    /// what the server hands the watcher so its own writes make no batch (8.2, `Settle::wrote`).
    pub wrote: Vec<(PathBuf, Option<Stamp>)>,
}
/// What a TextEdit did to a text file, in the coordinates the file had just before the commit, so an
/// open editor buffer follows it without reloading; a whole-file write of a Code file is one patch over
/// the whole old text.
pub struct Patch { pub file: String, pub start: u32, pub end: u32, pub text: String,
                   pub from: (u32, u32), pub to: (u32, u32) }            // (line, utf16_col) of start, end

pub enum Refusal {
    Conflict { files: Vec<ConflictFile> },
    Rejected { diagnostics: Vec<m2_song::Diagnostic>, unexpected: Option<GraphDelta> },
    /// Something failed on disk; nothing changed, or the commit was rolled back (6a.6).
    Io { message: String },
}

impl Session {
    pub fn check(&mut self, tx: &Tx) -> Result<Checked, Refusal>;     // writes nothing (WRITE-4)
    pub fn commit(&mut self, tx: Tx) -> Result<Committed, Refusal>;   // WRITE-5
}
```

There is no separate unverified entry (R26): a planner that needs to see what a probe would do checks it
under `Expect::Seen`, which `check` never refuses (6.4), and reads `Checked::candidate()`.

JSON of the outcome: `{"committed": {"code_rev", "data_rev", "revision", "delta", "hash", "diagnostics",
"patches"}}`, `{"conflict": {"files": [{"file", "current", "clip", "why"}]}}`, `{"rejected": {"diagnostics",
"unexpected"}}`, `{"io": {"message"}}`; `check` answers `{"checked": {"delta", "hash", "diagnostics"}}` or
the same refusals.

### 4.2 The pipeline

No step reads the folder beyond the Tx's own targets: neither `check`, `commit` nor any builder calls
`refresh()` (R2). Outside changes reach the session through the watcher (8.2) and the targets' current
bytes (step 6).

1. **Access.** `commit` on a read-only session refuses with `lock.held` or `lock.unsupported` (7), before
   reading anything. `check` runs read-only too (it writes nothing; its reads in step 3 are then not under
   the lock, which only `commit`'s compare-and-swap needs).
2. **Shape.** `tx.validate()` (1.3). A Tx whose targets overlap an interrupted journal's (6a.4) is
   `Rejected` with that `tx.interrupted`: finish or discard it first.
3. **Draft rule (C7; kept, R25).** A Tx that touches a Code or Manifest file requires the draft's code to
   compile (`draft_errors` empty): else `Rejected` with the draft's errors, as `EditError::DraftInvalid`
   is today (a lease's commit is then Pending, C5 of step 3b). A Tx that touches only Data, Arrangement or
   Presentation works while the draft is broken (WRITE-14), and is `Rejected` when there is no accepted
   code revision (with the draft's errors). A `Journal` Tx (6.5) is exempt from both: recovery restores
   recorded bytes whatever the draft says. This needs no disk read, so it comes before them.
4. **Read the targets** under the lock: each base path's current bytes and hash, folder hashes, the disk
   checks of 1.3 (links, capitals, UTF-8).
5. **Bases** (2.2): carry each `TextEdit` into current coordinates, apply the `Clips` rule, or
   `Conflict`. Waiting paths are `Conflict`.
6. **Baseline B (R2).** When every touched file's current hash is the accepted revision's, B is the
   accepted revision itself (the usual case: nothing more is built). Otherwise B is the accepted snapshot
   with each such touched file at its current bytes, built by 3.3's rules (the data path when only Data
   or Arrangement files differ, else a compile). If B doesn't build, the Tx is a `Conflict` naming each such
   file with `why`: "changed on disk, and doesn't compile yet": a lease's commit then goes Pending instead
   of snapping back, and is retried once the watcher has taken the change in. Files the Tx doesn't touch
   stay as accepted: a Tx never takes in an outside write it doesn't touch (C7).
7. **Candidate.** B, with each touched file replaced by its op applied to the current content (2.4, 2.5),
   renames moving everything under a folder (today's `data::apply`), removes dropping it. Presentation
   writes are applied to the journal only, never to the snapshot.
8. **Build.** The plane of the touched paths decides (3.3): Code/Manifest → `build_code` then
   `read_data`; only Data/Arrangement → `read_data` with B's `Code` and `changed` = the touched lane and
   state files (no QuickJS, WRITE-4). A build error is `Rejected` with it.
9. **Delta** (5) from B to the candidate, named in each touched file's base coordinates (5.2), and its
   hash. This is the delta the expectation judges and that `check` and `commit` return: the Tx's own
   change, never an outside writer's.
10. **Expectation** (6). A failure is `Rejected { diagnostics: [tx.unexpected …], unexpected: Some(delta) }`.
11. **Diagnostics**: each problem of the candidate that B doesn't have (by code, src and message), then
    `data.claimed` (8.4).

`check` stops here and keeps the result in a one-entry cache keyed by the Tx's canonical hash, the
accepted `number`, and the current hashes read in step 3. `commit` runs 1–11 again, except that 6–11 come
from the cache when its key matches, so a builder's check-then-commit compiles once. Then:

12. **Pre-check** (WRITE-15 step 1): every target is read again; one that differs from step 3 is
    `Conflict` before anything is written (only an outside writer can have changed it). This only avoids
    journaling a doomed commit: the binding check is at each rename (6a.2 step 7).
13. **Journal, staging, syncs, barrier, checked renames** (6a.2), or a rollback and `Conflict` / `Io`
    (6a.6).
14. **Accept** the candidate as the new revision (`number + 1`, `code_rev`/`data_rev` per 3.1 against the
    accepted revision, so an outside change in a touched file moves them too), store its texts (2.3), push
    the inverse onto the undo stack and clear redo (10), return the paths written with their stamps
    (`Committed.wrote`, for the watcher, 8.2), recompute the session's problems.

A Tx whose ops leave every file as it was (a knob released at its value, already caught by the builder;
a `Clips` op that changes nothing) commits nothing: `Committed` with the current numbers, an empty delta
and no history entry.

### 4.3 What `check` then `commit` guarantees

The bytes `commit` writes are exactly the candidate files of `check`, and the delta hashes are equal,
unless a target or the accepted revision moved between them. Because names are in base coordinates
(5.2) and the delta starts from B, a change elsewhere in a touched file that the Tx's ranges don't
touch (a comment typed above the call) changes neither the delta nor its hash: an `Expect::Seen` from the
earlier `check` still commits (WRITE-11, WRITE-12: the same outcome whatever versions a session saw). When
the change does touch what the Tx changes, the carry fails (`Conflict`) or the delta differs (`Seen`
`Rejected`). Both are tested (19, property d, and `seen_survives_an_unrelated_edit_in_the_same_file`).

---

## 5. `GraphDelta` (WRITE-8)

### 5.1 Shape

```rust
pub struct GraphDelta {
    pub added: Vec<DNode>,          // sorted by name
    pub removed: Vec<DNode>,
    pub rewired: Vec<Rewire>,       // nodes in both whose inputs read other things; module exports and the root too
    pub values: Vec<ValueChange>,   // consts in both whose value changed
    pub lanes: Vec<LaneAddr>,       // lanes declared in both whose data plays differently
    pub states: Vec<StateRef>,      // plugin states loaded differently (module, key path)
    pub assets: Vec<AssetName>,     // assets a clip plays whose hash changed
    pub arrangement: bool,
    pub modules: Vec<ModuleChange>,
}
pub struct DNode { pub name: Name, pub op: Value /* op without its inputs: kernel id, const value+unit, lane key… */,
                   pub inputs: BTreeMap<String, Ref> }
pub struct Rewire { pub node: Name, pub input: String, pub from: Option<Ref>, pub to: Option<Ref> }
pub struct ValueChange { pub node: Name, pub from: f64, pub to: f64 }
pub type Name = Vec<String>;        // segments (5.2); JSON: an array of strings
pub struct Ref { pub node: Name, pub port: String }
impl GraphDelta {
    pub fn structural(&self) -> bool;   // added, removed, rewired or modules non-empty
    pub fn is_empty(&self) -> bool;
}
```

A node whose op changed under one name (another kernel id, `voices`, a plugin key) is listed as removed
and added; there is no third kind. Ignored, as WRITE-6 says, by construction of the comparison: source
positions (`src`, `Module.src`, `Frame.options`), labels (`Module.label`, `LaneDecl.label`), node ids,
site and frame numbers (names replace them), and whether a control was written or inherited (a declared
control's const is named by its slot, 5.2, and its origin isn't compared). Also ignored (C14): `Module.track`
(a host convention) and `Frame.sidechain` (only step 8 reads it; changing it rewires a node anyway).

`lanes` compares each lane declared in both revisions by its `DataSet` entry (`Arc::ptr_eq` first, then
value), as `plane::lane_swaps` does today; a lane declared in only one is covered by its lane node, added
or removed. `states` compares `DataSet::state_hash` per state path, `assets` the binaries' hashes of
assets some clip names, `arrangement` the parsed arrangement. When both revisions share one `Arc<Code>`
(a data revision), the graph lists are empty without a walk: only the data lists are compared.

### 5.2 Names an insertion can't shift (R17)

A node's name is its module key, then segments:

| Node | Segments after the module key |
|---|---|
| a declared control's const (`origin.control`) | the chain of its control's frame (`Module::chain`), then `@<slot>` |
| any other node with an origin | the chain of its `frame`, then `<op>@<file>:<start>-<end>`, then `[<arg path>]` for a const with one, then `#<n>` |
| a lane node | `lane:<key>@<file>:<start>-<end>` (always with its read's site, R17), then `#<n>` |
| a time node | `time:<beats|seconds>` (one per module) |
| an import node | `import:<module>.<export>`, then `#<n>` |
| a node with no origin (made by native code) | the chain of its `frame`, then `<op>~<kernel or plugin id>`, then `#<n>` |
| a node inside a `poly` voice | the poly node's name, `voice`, then the inner node's segments |

Chain segments are `@<file>:<start>-<end>#<occurrence>` for a component call and `key:<k>` for a `key()`
frame. `<file>` is a project file as the project spells it, or a package file as `std/src/dsp.ts`.
`#<n>` counts nodes with all other segments equal, in walk order (ID-14's occurrence, within that site).
Module-level wiring is named `exports` (one `Rewire` per export) and `root`.

**`<start>` and `<end>` are points in the file's reference coordinates** (2.4), never offsets in the new
text:

- For a Tx, a touched text file's reference is its **base text**. A position of B (whose touched file is
  the current text) is mapped back through the outside hunks (base → current); a position of the
  candidate first back through the Tx's own edits (current → candidate). The point is spelled `<n>` (a
  base byte offset), `e<i>.<k>` (byte `k` of own edit `i`'s new text, edits numbered in base order) or
  `o<i>.<k>` (byte `k` of outside hunk `i`'s new text).
- For a delta between two revisions (an outside write, a save, the engine's comparison, 5.5), the
  reference is the old revision's text, and the hunks are `Edits::between(old, new)`, spelled `o<i>.<k>`.
- An untouched file is its own reference (identical on both sides); package ranges don't move (packages
  don't change within a session).
- A range's start is the point of the character it starts at, its end the point of the character it ends
  after (2.4's mapping 2). A call around an edited literal keeps its name; a call whose own first or last
  character was replaced gets a new one (the node is removed, and the one made there is added).

So nothing an insertion elsewhere does can shift a name, a concurrent edit elsewhere in the file can't
either (R17), and two processes checking one Tx with one base text name every node alike. This is also why
the effect planner writes a wrap as two insertions (`delay(` before the call and `, …)` after it) rather
than replacing the call: the wrapped call's characters are unchanged, so its nodes keep their names, and
the delta is the effect's nodes and the rewired readers (14.4). From step 7, SiteIds replace these points
(ID-13).

Names are unique per graph; a name built twice is a bug, and a debug assertion plus the test
`delta_names_are_unique_on_every_fixture` (all 5 fixtures and the every-component song) guard it. A
second `lane.notes("hits")` read added anywhere adds one lane node and renames none
(`a_second_read_of_a_lane_renames_nothing`).

### 5.3 Comparing

Both graphs are walked once into `BTreeMap<Name, NodeSig>`, where `NodeSig` is the op with its inputs
rewritten as `Ref`s by name (a `"n12.lp"` reference becomes the referenced node's name and `lp`). Names
only in the new map are `added`, only in the old `removed`; in both, a different op detail is
removed + added, different inputs are one `Rewire` per input, a const whose value alone differs is a
`ValueChange`. Lane declarations ride on lane nodes (kind and default are part of the lane node's op), so
a new fallback is structural. Modules: present only in the new IR → `Added`, only in the old → `Removed`;
a removed and an added module whose node maps are equal once the module key is dropped from every name →
`Rekeyed` (step 8b's key rename needs it; detected now so the delta is complete). `muted` arrives with step 8.

`pub fn delta_between(old: &Revision, new: &Revision) -> GraphDelta` (`core/session/src/tx/delta.rs`) is
the same comparison with 5.2's revision-to-revision coordinates: pure, used by `take_in`, the server's
engine decision (5.5) and tests.

### 5.4 Hash

`DeltaHash` = SHA-256 of the JCS form (1.4) of the delta's JSON, every list sorted by name (by UTF-16 code
units of the segments, then segment count). Values are the consts' `f64`s, always finite. Nothing in a
delta depends on a path outside the project, a time, a process, a hash map's order, or the current text
of a touched file beyond what the Tx changes (5.2), so two processes agree (19: the two-process test), and
`check` and `commit` agree across unrelated outside edits (4.3).

### 5.5 What the engine hears: the delta from what plays (RENDER-26, RENDER-22; step 7 adds the rest; R4)

The engine path is decided by comparing **the revision the playing renderer was built from** with the
accepted revision, never by the last change's delta alone. The server's `Plays` keeps the `Arc<Revision>`
it was built from (or adopted, below); at every settle it takes `delta_between(plays.revision,
accepted)`, reusing the last `Committed` or `Accepted` delta when it started from exactly the revision
that plays (the usual case, so no second walk):

| Delta from what plays | Step 6's engine path |
|---|---|
| empty | nothing is sent; what plays adopts the accepted revision |
| `values` only, and the two `Flat`s number their cells alike (`same_layout`) | adopt, then write each cell whose const differs between the two `Flat`s (aligned by `same_layout`) |
| `lanes` only, of notes and control lanes that fit their schedulers | `SwapLane` per lane; on success what plays adopts |
| anything structural, `values` without `same_layout`, an audio lane, a state, an asset, the arrangement, or a new sample rate | a new renderer, activated where the engine is (as today; `LoadState`, `SetArrangement` and prepared renders are step 7) |

A send that fails (`Way::Failed`: the renderer didn't build, or there is no player) changes nothing in
`Plays`: what plays plays on, and the next settle compares from it again, so the structural change is
retried and never skipped by a later lane-only change (`a_failed_build_then_a_lane_edit_rebuilds`, 19).
As a guard, a `Values` or `Swap` path also requires that the renderer's own `Flat` equals the delta's
starting revision's `Flat` (or passes `same_layout` for values); otherwise it activates.

This replaces both `plane::lane_swaps` (whose `Flat` equality and path filter the delta now says
precisely) and `Open::single` (the guess, `crates/server/src/graph/actor.rs`), which are deleted.

### 5.6 Example: inserting an unconnected track at the top of `song.ts`

The Tx inserts `import { osc } from "std/dsp";` and `const probe = track("probe", () => osc.sine(440));`
at byte 0 of a copy of `examples/first-song/song.ts` (own edit 0), and adds `track` to its `@m2/core`
import (own edit 1). Every other site keeps its base range, so every old name is unchanged. The delta is
`modules: [added probe]` and `added`: probe's const (`["probe", "@song.ts:e0.<s>-e0.<e>#0", "@freq"]`,
its call's points inside own edit 0), its kernel nodes, and nothing else: no `removed`, `rewired`,
`values` or `lanes`. The same Tx checked in another process gives the same hash (the gate, 19).

---

## 6. Expectations (WRITE-6, WRITE-9, WRITE-10)

Each runs on the delta from B to the candidate (4.2) and the two revisions; none knows a gesture. Ranges
and anchors are in base coordinates, and "inside" is checked with spans (2.4's mapping 3): inclusive of
the Tx's own edits at a range's ends, strict for everything else.

### 6.1 `Identical`

The delta is empty. (A label rename; delete unused data; a comment.)

### 6.2 `Values(Vec<(Anchor, f64)>)`

The delta has nothing but `values`, and those are exactly the consts the anchors name, each now equal to
its anchor's number. An anchor names, in the candidate:

- `Written { file, range }`: every const whose origin's site is in `file` and whose argument (`Site::arg`
  of `origin.arg`) is the `Number` whose span is exactly `range` (an own edit that replaced exactly that
  range spans it). One literal reaching two slots names both consts.
- `Slot { site, slot }`: every const whose `origin.control.slot` is `slot` and whose control frame's call
  is named by `site`'s range (its characters unchanged, so an argument inserted inside the call keeps it):
  every evaluation of it.

An anchor that names no const is `Rejected` ("the value is no longer in the graph"). Writing an omitted
argument (inherited → written) changes no name and so counts as `Values`, as WRITE-6 requires.

### 6.3 `Confined { ranges, lanes, modules }`

All of these hold:

1. **Nodes.** Every added or removed node is *in a declared range*: the span of its origin (the
   argument's when its origin has an argument path, else its site's), or of the site of any frame of its
   chain, or its `src` position, lies inside a range declared for that file (a file declared `Whole`
   contains everything in it). Every rewired node is either in a declared range or differs only in edges
   whose old or new end is an added or removed node in a declared range. A `ValueChange` counts like a
   node: its const must be in a declared range. Added nodes are spanned in the candidate, removed ones in
   B; both in base coordinates.
2. **Lanes.** `delta.lanes` ⊆ `lanes`.
3. **Modules.** `delta.modules` equals `modules` as a set.
4. **Bytes.** Every `TextEdit` edit's base range lies inside a declared range of its file (with `[]`,
   none may), and a Code or Manifest file a `Write`, `Remove` or `Rename` touches is declared `Whole` (1.3
   rule 5; add track's new file is). Writes, removes and renames of data files need no ranges: rules 2 and
   5 cover what they change.
5. **States, assets, the arrangement (C1).** Each one in the delta is held by a file the Tx's own ops
   wrote, removed or renamed. (The contract's list doesn't name them; without this a code edit that moves
   a state path, or a stray arrangement write, would pass unseen.)

### 6.4 `Seen(hash)`

The delta's hash equals `hash` (WRITE-11). `check` always succeeds on `Seen` and returns the hash: that is
how an agent learns it, and how a planner probes a candidate without an expectation (R26).

### 6.5 `Journal(id)` (C3)

Only FinishTx and DiscardTx carry it. Core reads journal `id` and checks that every op restores what it
recorded (each target to its before or after hash, from the staged files or the before-images), that every
target still matches its before or after hash (WRITE-16), and that the Tx touches every target the journal
lists that isn't already where the fix puts it. The graph isn't consulted: recovery puts back recorded
bytes, whatever they compile to (P3), and the result is taken in like an outside write (a candidate that
doesn't compile becomes the draft, with `draft_errors`, not a refusal). On success the journal and its
staging folder are removed after this commit's barrier (6a.3).

### 6.6 How each gesture is expected (WRITE-10)

| Gesture | Expectation | The planner's own assertion (`crates/editor`) |
|---|---|---|
| set a value (a literal, or an inherited control's first edit) | `Values([(anchor, value)])` | — |
| connect | `Confined { ranges: [the value's expression, or the insertion point] }` | the candidate is B's graph with the value's consts removed and their readers reading the outlet (`rewire`, `canonical`) |
| disconnect | `Confined { ranges: [the name, the import insertion] }` | rewiring the new consts back to the outlet gives B's graph |
| add an effect | `Confined { ranges: [the wrapped call, the import insertions] }` | `wraps(candidate, B, new site)` |
| remove an effect | `Confined { ranges: [the effect call] }` | `wraps(B, candidate, site)` |
| write a notes lane | `Confined { lanes: [the lane] }` (a `Clips` op, or a whole `Write` for a lane that isn't addressable, 12) | the candidate's data for the lane is exactly the clips asked for |
| relink unused data | `Confined { lanes: [every lane it gives data to] }` | the target lane (or module) reads the moved file |
| delete unused data | `Identical` | — |
| a data problem's rewrite fix (clip ids, pins, unit) | `Confined { lanes: [its lane] }` | the lane reads the file and the graph is unchanged |
| plugin state (`m2 plugin-state`) | `Confined {}` (C1 covers the state) | — |
| place a bounce (`m2 bounce --place`) | `Confined { lanes: [the audio lane] }` | — |
| rename a label | `Identical` | — |
| automate a knob | `Confined { ranges: [the literal, the parameter list], lanes: [the new lane] }` | — |
| code editor save | `save()` (8.1) | — |

