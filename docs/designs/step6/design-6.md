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
accepted)`, reusing the last `Accepted` delta, or a `Committed` one whose B was the accepted revision,
when it started from exactly the revision that plays (the usual case, so no second walk; only the kind
of change matters here, not the names, so either coordinate choice serves):

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

---

## 6a. Commit, journal and recovery (WRITE-15, WRITE-16, PLANE-5, FMT-20)

### 6a.1 Layout (`.music2/tx/`, class Local)

```
.music2/lock                        the project lock (7)
.music2/.gitignore                  "*" (7.1)
.music2/tx/<jid>.json               a journal; <jid> = <unix ms>-<pid>-<n>, named by core (WRITE-3a)
.music2/tx/<jid>/<k>                staged new bytes of the journal's k-th written target; after a
                                    swap, the bytes it replaced (6a.2 step 7)
.music2/tx/blobs/<sha hex>          before-images of binary targets, stored once by content
.music2/tx/aside/<jid>.json         a journal set aside: a target matches neither side, or it doesn't parse (WRITE-16)
.music2/tx/save/                    where save() stages its one file (8.1)
```

Journal (`core/session/src/tx/journal.rs`):

```json
{"journal": 1, "id": "1760351234567-4121-3", "tx": "tx-7k2m9q4c",
 "host": "Jerrys-MacBook-Pro", "host_id": "8C1F0D2A-…", "pid": 4121, "program": "music2",
 "label": ["hats pan 0.3", "hats pan automated"],
 "targets": [
   {"path": "song.ts", "before": "sha256:…", "after": "sha256:…", "text": "…before…", "staged": 0},
   {"path": "data/song/hats-pan.json", "before": null, "after": "sha256:…", "staged": 1},
   {"path": "data/lead/lead.state", "before": "sha256:…", "after": "sha256:…", "blob": true, "staged": 2},
   {"path": "data/kick/old.json", "before": "sha256:…", "after": null, "text": "…before…"},
   {"rename": ["data/hat", "data/hats"], "before": "sha256:<dir hash>", "after": "sha256:<dir hash>"}]}
```

The before bytes of every text target are inline; a binary target's are in `blobs/<before hex>`, made by
`clonefile(2)` from the target (APFS copy-on-write: no data copied, and no inode shared, so a writer that
rewrites the target in place can't change the blob; R10), else by a copy. The clone is hashed, and that
hash is the target's compare-and-swap: a target that changed between the pre-check and the clone is a
`Conflict`. When `blobs/<sha>` already exists it is hashed before it is relied on, and rewritten from the
clone if it differs. A blob is never hard-linked anywhere, and never linked back into the project: undo and
Discard restore from it by a clone into staging (6a.2). `layout.json` targets are journaled like any other.

### 6a.2 Steps and durability (R1, R9)

Every disk call goes through `trait Disk` (`tx/stage.rs`), whose real implementation is the only one
outside tests; tests record its calls, inject errors and abort at any of them (6a.5, 6a.6).

1. **Pre-check** every target (4.2 step 12).
2. **Journal**: write `<jid>.json` under a temporary name in `.music2/tx/`, `fsync` it, rename it to
   `<jid>.json`.
3. **Stage** every new content in `<jid>/<k>` and `fsync` each (the file descriptors stay open until the
   commit ends); clone each binary target's before-image into `blobs/` and `fsync` it.
4. **Folders**: `fsync` `<jid>/`, `blobs/` and `.music2/tx/` (their new entries).
5. **One barrier**: `fcntl(F_FULLFSYNC)` on the journal. On macOS it flushes the drive's cache, so
   everything steps 2–4 pushed is durable before anything in the project changes. Elsewhere `sync_all`.
6. **The previous commit's tidy** (6a.3): unlink its journal and staging folder, and blobs no journal or
   undo step names. Not synced now: the next barrier makes it durable, and the previous commit's renames
   were drained by this barrier (they were pushed at its step 8).
7. **Checked renames, in op order** (the ops are independent, 1.3 rule 4):
   - a replaced file (base `Sha`): `renamex_np(staged, target, RENAME_SWAP)`, then the bytes now at the
     staged path (the target's old content) are compared with the base: a text file by its hash; a binary
     by its `(inode, size, mtime)` against the clone's source (a rename moves the inode and keeps its
     mtime), re-hashed when they differ or the mtime is racy (FMT-16's rule). If they differ, an outside writer replaced the file since the
     pre-check: swap back, and the commit rolls back and is a `Conflict` (6a.6);
   - a created file (base `Absent`): `renamex_np(staged, target, RENAME_EXCL)` (creating parent folders);
     `EEXIST` is a `Conflict` and a rollback;
   - a removed file: renamed to `<jid>/removed-<k>`, then compared with the base like a swap's old
     content; a mismatch renames it back (with `RENAME_EXCL`, so a file created there meanwhile is never
     clobbered) and rolls back;
   - a folder `Rename`: the folder's hash is taken again, then `renamex_np(from, to, RENAME_EXCL)`; the
     folder at `to` is hashed once more, and a mismatch (a file written into it during the rename) renames
     it back and rolls back;
   - where the volume refuses `RENAME_SWAP` or `RENAME_EXCL` (`ENOTSUP`, `EINVAL`: some network and FAT
     volumes), the target is hashed again just before a plain `rename(2)`.
   Then empty folders a remove or a rename left are pruned up to `data/` (`rmdir`, which fails harmlessly
   on a folder someone wrote into).
8. **Push the renames**: `fsync` each target's parent folder and each pruned folder's parent (no flush).
   The next barrier (the next commit's step 5, or close) makes them durable.
9. **Accept** (4.2 step 14).

The journal is written before staging, as WRITE-15 orders; both become durable at the one barrier, so a
commit costs one `F_FULLFSYNC` (PLANE-5 measures it, 19). A crash before the barrier can leave a journal
whose staged files are incomplete; recovery checks every staged file's hash, so it offers only what it can
do. The race WRITE-19 says can't be closed is now only an in-place writer between a swap's compare and the
end of the commit; a writer that replaces files by rename (editors, agents, git) is caught at the swap.

### 6a.3 Tidy (C9, revised by R7)

A commit's journal must not be removed durably before its renames are, or a power loss could leave a
half-applied commit with no journal. So a commit's journal stays until a later barrier covers its renames:
it is unlinked in the **next** commit's step 6, right after that commit's barrier and before its own
renames; at close, after one more barrier; or, after a crash, when the next session takes the lock and
finds every target at its after hash (6a.4): it then issues one `F_FULLFSYNC` and removes the journal and
its staging **silently**, since that changes no project file (P3: it is tidying, not recovery). So a
routine crash after an edit reopens with no diagnostic and nothing blocked.

### 6a.4 Recovery as a diagnostic (WRITE-16, DIAG-5)

When a session takes the lock (open, `retry_lock`, a CLI write command), it reads every
`.music2/tx/*.json`, **oldest first** (by `<unix ms>-<pid>-<n>`):

- a journal that doesn't parse, or whose staging is unreadable, moves to `aside/` and is reported as
  `tx.interrupted` with no fix, naming the journal file (R8);
- `host_id` isn't this machine's (`gethostuuid()`, R11; elsewhere `/etc/machine-id`, else the host name)
  → Warning `tx.foreign`, no fix, the journal left alone; its `host` is the name the message gives;
- otherwise, every target is at its before hash, at its after hash, or neither, where a target at the
  before or after hash of a **newer** journal that lists the same path, with that journal's before equal
  to this one's after, counts as at this one's after hash (an older completed journal shadowed by a newer
  one, R7);
  - every target at **after** → a completed commit awaiting its tidy: one `F_FULLFSYNC`, then the journal
    and its staging are removed, with no diagnostic (6a.3);
  - any target at **neither** → the journal moves to `aside/` (a Local file, not a project file) and is
    reported as `tx.interrupted` with no fix, naming each changed file;
  - else (some target still at **before**) `tx.interrupted` (Error, `src` = `.music2/tx/<jid>.json`) with
    fixes:
    - **FinishTx** (likely when every staged file is present and correct): a Tx with `Expect::Journal(jid)`
      that, for each target at its before hash, writes the staged bytes (`Payload::Local` of
      `.music2/tx/<jid>/<k>` with its after hash), removes, or renames, with bases at the before hashes.
      Offered only if every such target has its staged file with the after hash.
    - **DiscardTx**: for each target at its after hash, writes the recorded before bytes (inline text, or
      `Payload::Local` of `blobs/<hex>`), recreates a removed file, or renames back, with bases at the
      after hashes. Always possible, since before-images are durable before any rename and never share an
      inode with a project file.
- While a `tx.interrupted` journal stands, a Tx touching any of its targets is `Rejected` with it (4.2
  step 2); others go ahead. Recovery never runs by itself.

The diagnostics are the session's (like `pkg.invalid`): first in `Session::problems()`, in the app's
"Needs attention" strip, in the news, and in every CLI command that opens the project, with
`m2 fix <project> .music2/tx/<jid>.json finish-tx` and `discard-tx`.

**Readers too (R16).** `open_read_only` tries the lock without blocking. If it gets it, it scans the
journals as above but changes nothing (no tidy, nothing set aside), reports `tx.interrupted` with its fixes
as the `m2 fix` commands (which need the lock), then releases the lock: it is held for the scan only, a
few milliseconds, never while the project is read. If the lock is held, a journal may be a commit in
progress, and the holder reports its own: the reader says nothing about journals. A writer that finds the
lock held retries for up to 250 ms before opening read-only, so a reader's scan never turns a writer
read-only (C19).

### 6a.5 Kill points (R20)

The kill test lives in the crate's own test module, `core/session/src/tx/kill_tests.rs`, so it is
compiled with `cfg(test)` and nothing of it reaches the library other crates link. Its child is the test
binary itself, re-run (`std::env::current_exe()` with `--exact tx::kill_tests::child --nocapture`) with
`M2_TX_KILL_AT=<n>` and the copy's path: it opens a session whose `Disk` is a `cfg(test)` wrapper that
calls `std::process::abort()` at the `n`-th disk call (no destructors; the OS releases the lock, as a
crash would). A first run with a counting wrapper gives `N`, the number of disk calls a commit makes, so
every step, every staged file and every rename is a kill point without naming them by hand. The parent
asserts the child died of `SIGABRT` (so the kill point fired; a child that exits normally fails the test)
before it reopens.

### 6a.6 Failures (R8)

- **Before the first rename** (ENOSPC while staging a 50 MB blob, EACCES, an unreadable target): the
  journal, its staging and any new blob are removed, and `commit` returns `Refusal::Io` with the system's
  error. Nothing in the project changed, so nothing is left to recover. If even that cleanup fails, the
  journal stays with every target at before, and the next lock reports it (as for a crash there).
- **After some renames** (a later rename fails, or a checked rename finds an outside change): the process
  still holds the lock and knows exactly what it did, so it **rolls back in process**, newest first:
  swaps back, unlinks what it created (checked by inode), renames removed files and folders back. Then it
  pushes the folders (`fsync`), issues one `F_FULLFSYNC` (so the rollback is durable before the journal
  can disappear; rollbacks are rare), removes the journal (every target is at before again), and returns
  `Conflict` or `Io`. This is the commit's own atomicity, not P3 recovery: no file anyone else wrote is
  touched (each step is checked as the forward step was).
- **A rollback that fails**: the journal stays, `tx.interrupted` is raised at once in the session's
  problems (the session scans that journal as 6a.4 would), and the accepted revision is unchanged; the
  watcher then takes in what is on disk like any outside change, as after a crash.
- **Volumes.** At lock time `.music2/tx/` must be on the same volume as the project (`st_dev` of both);
  otherwise the project opens read-only with `lock.unsupported` ("…/.music2 is on another volume than the
  project, so a commit couldn't rename into place"). A target under a folder that resolves elsewhere is
  already `tx.path` (1.3).

---

## 7. The project lock (WRITE-17, WRITE-18, owner decision 10)

### 7.1 `core/session/src/lock.rs`

```rust
pub struct Holder { pub pid: u32, pub program: String, pub host: String, pub host_id: String,
                    pub token: String, pub since: u64 /* unix s */ }
pub struct ProjectLock { file: std::fs::File, pub holder: Holder }   // dropped: the OS releases it
pub enum Access {
    Writer(ProjectLock),
    /// Opened read-only: the lock is another process's (`holder`, as its file says), or the volume can't lock.
    ReadOnly { why: ReadOnly },
}
pub enum ReadOnly { Held(Option<Holder>), Unsupported(String), Asked }
pub fn acquire(dir: &Path, program: &str) -> Result<ProjectLock, ReadOnly>;
```

- `.music2/` is created if missing, with a `.music2/.gitignore` holding `*`, so git never lists it even in
  a project whose root `.gitignore` lacks the line (FMT-20; the sync services' marker comes later); then
  `.music2/lock` is opened create + write **without truncating**, and `File::try_lock()` (std's exclusive
  advisory lock, `flock(2)` on macOS, the one `render.rs` uses) is taken. Only then is the file truncated
  and `{pid, program, host, host_id, token, since}` written (JSON, for messages only). `token` is 16
  random hex digits.
- **Machine identity (R11).** `host_id` is `gethostuuid()` (the hardware UUID, stable across networks);
  `host` is `gethostname()`, kept only for messages, so a name that changes with the network no longer
  matters (FMT-20 still records it).
- `WouldBlock` → retried every 25 ms for up to 250 ms (a reader's journal scan, 6a.4), then
  `ReadOnly::Held(holder read from the file, if it parses)`.
- `ENOTSUP`, `EOPNOTSUPP`, `ENOLCK`, a `.music2/` that can't be created (`EROFS`, `EACCES`), or
  `.music2/tx/` on another volume (6a.6) → `ReadOnly::Unsupported(the system's error)` and the session's
  Error `lock.unsupported` (C15).
- **Network volumes (R27).** When the lock is taken on a volume without `MNT_LOCAL` (`statfs`), the
  session opens for writing and raises the session Notice `lock.local`: "this folder is on a network
  volume; the lock stops other programs on this Mac only, so another Mac editing it is an outside
  writer" (C2; the owner may prefer `lock.unsupported`, which would make such projects read-only).
- `flock` locks belong to the open file, so a second `Session::open` of one project in one process gets
  `Held` too (C10): two writers in one process race exactly as two processes would.
- No forwarding (owner decision 10).

### 7.2 Sessions and access

```rust
impl Session {
    /// Open for writing if the lock can be taken, else read-only (WRITE-17, WRITE-20).
    pub fn open(dir: &Path, packages: PackageSet) -> Result<Session, OpenError>;
    pub fn open_as(dir: &Path, packages: PackageSet, program: &str) -> Result<Session, OpenError>;
    /// Holds the lock only for a journal scan (6a.4): the CLI's reading commands.
    pub fn open_read_only(dir: &Path, packages: PackageSet) -> Result<Session, OpenError>;
    pub fn access(&self) -> &Access;
    /// Try again to take the lock (the app's Retry); scans journals when it succeeds.
    pub fn retry_lock(&mut self) -> bool;
}
```

A read-only session plays, takes in outside writes, checks Tx (a check writes nothing), and refuses
`commit`, `save`, `undo` and `redo` with Error `lock.held` (C2) or `lock.unsupported`, whose message is
WRITE-18's: "nothing was changed: this project is open for writing in music2 (pid 4121, since 14:02).
Make the edit there, or close the project in music2 and run this again." The session's problems carry
the same Error, so the app shows it (7.3) and an agent's read says it.

### 7.3 Who takes it

- The app: while the project is open (the actor's `Open` owns the `Session`). Opening a project the
  actor already has open releases the old session first (else it would find its own lock held); opening
  another folder keeps the old open until the new one opens. A read-only open shows a banner naming the
  holder, with **Retry** (`POST /api/graph/lock/retry`); edit affordances are disabled, and leases are
  refused at `begin` (their commit couldn't be written).
- The CLI (WRITE-18): `check`/`build`, `render`, `graph`, `controls`, `cables`, `effects` use
  `open_read_only` (which reports interrupted commits, 6a.4); `kernels`, `plugins` and `new` open nothing;
  `bounce` without `--place` opens read-only and creates its asset and provenance create-only (FMT-14), so
  it works while the app is open; `set`, `connect`, `disconnect`, `add-effect`, `remove-effect`,
  `bounce --place`, `plugin-state`, `relink`, `delete-data`, `fix` and `tx` open for writing and fail
  before anything else (before rendering, for `bounce --place`) when they get `ReadOnly`, with WRITE-18's
  message. For `bounce --place` the message also names the lane file and says the clip can be added by
  writing it directly.

---

## 8. `save()`, outside writers and the watcher (WRITE-2, WRITE-19, DIAG-3)

### 8.1 `save(file, base, text)`

```rust
impl Session {
    pub fn save(&mut self, file: &Path, base: Base, text: &str) -> Result<Saved, Refusal>;
}
pub struct Saved { pub taken: TakenIn, pub recorded: bool }
```

The file must pass WRITE-3a's path rules and be a text class (Code, Manifest, Arrangement, Lane, sidecar,
Presentation). Writer access required. Compare-and-swap of the disk's bytes against `base` (a `Sha` or
`Absent`; `WithText` is accepted and checked), else `Conflict` (the server answers 409 with the text on
disk, as `files::Conflict` does today). Then an atomic write: staged in `.music2/tx/save/`, `fsync`, the
same checked swap as a commit's (6a.2 step 7, with its fallback), and the parent folder pushed. It is never
rejected for errors (WRITE-2), and a file that isn't UTF-8 may be saved over (that is the remedy, R24).

Then the save is taken in **without reading the folder (R12)**: the new draft is the last settled draft
with this one file replaced, taken in by 3.3's table as a one-path batch; the path and its new stamp go
back in `Saved.wrote` for the watcher (`Settle::wrote`), and every other changed path is left to it. It is recorded
for undo only when the save was taken into an accepted revision (it compiled, or it was data): the undo
step is a `Write` of the text before, with base the hash just written (WRITE-21).

`write_whole` becomes the private staging writer used by `save` and the journal (in `tx/stage.rs`), and
`Session::saved` is deleted. `files::write` (the server's `PUT /api/graph/file`) becomes a call to `save`.
The server hashes the base text the web sends; for a file in `Snapshot.not_utf8`, a base text equal to
the lossy decoding of the current bytes stands for the raw hash, so such a file can be opened and
re-saved from the code editor.

### 8.2 The settle rule (`core/session/src/watch.rs`, new)

A pure state machine, driven by the server's watcher thread (17.4), testable with a fake clock:

```rust
pub struct Settle { quiet: Duration /* 150 ms */, baseline: BTreeMap<PathBuf, Seen>, batch: Option<Batch> }
/// What a poll knows of a path (R14): a revision input by its stamp (and hash once stable); any other
/// file under `assets/` only by being there.
pub enum Seen { Input { stamp: Stamp, taken: Instant, sha: Option<Sha> }, Present }
pub struct Stamp { size: u64, mtime: SystemTime, ctime: SystemTime, inode: u64 }
struct Batch { last_change: Instant, polls: BTreeMap<PathBuf, (Seen, u8 /* equal polls in a row */)> }
pub enum Poll { Idle, Settling, Ready(BTreeSet<PathBuf>) }
pub struct Git { pub lock_age: Option<Duration> }    // .git/index.lock of the project's git dir, if any
impl Settle {
    /// One poll: `listing` is a stat walk of the project with Snapshot::read's rules (no contents).
    pub fn poll(&mut self, now: Instant, listing: Listing, git: Git, hash: &dyn Fn(&Path) -> Option<Sha>) -> Poll;
    /// The session wrote these paths itself (a commit, a save): their baseline becomes these stamps
    /// (fstat of its own file after the rename), and they leave an open batch. Nothing else changes.
    pub fn wrote(&mut self, paths: &[(PathBuf, Option<Stamp>)]);
}
```

- **What is tracked (R14).** Revision inputs are tracked by stamp: the manifest, code, the arrangement,
  lane files, sidecars, state blobs, and the assets some lane of the draft names. Every other file under
  `assets/` is tracked only by being there (a growing render or a Finder copy of a sample pack changes
  nothing until it appears or disappears). Ignored and Presentation files aren't tracked (PLANE-3).
- **A batch.** A poll compares the listing with the baseline; a difference opens a batch (or extends it,
  moving `last_change`). The batch is **Ready** when no tracked path changed for 150 ms (WRITE-19's
  quiet, judged over what is tracked), every changed revision input has been equal on two polls in a row,
  and git isn't busy (below). Text inputs are hashed when their stamp changes; a binary input is hashed
  only once its stamp has been the same on two polls (never while it grows), through the hash cache,
  which this refreshes (FMT-16).
- **Racy text (R14).** A text input whose mtime is within 2 s of the moment its stamp was taken is
  hashed again on every poll until it isn't (FMT-16's rule), so a same-size rewrite within a coarse
  timestamp (exFAT, FAT, some SMB) is never missed, as today's content compare never misses it.
- **Git (R13).** At open the watcher finds the project's git dir by walking up from the project folder,
  following a `.git` file's `gitdir:` line (worktrees, submodules); none means no git rule. While that
  dir's `index.lock` exists and is younger than 10 s, a batch isn't Ready. An older one is stale (git
  leaves one after a crash or a killed command): the batch goes ahead, and the session raises the Notice
  `git.locked` naming the file, once until it goes away (C2).
- **Own writes (R12).** After a commit or a save the actor calls `wrote` with the session's own paths
  and the stamps of its own files (`fstat` on the staged file's descriptor after the rename: the inode
  moved with it), so its writes make no batch; the baseline of every other path, and any open batch, are
  kept. An outside write that landed on another path meanwhile is still taken in, and one that replaced a
  path the session just wrote differs from the recorded stamp (another inode) and is too.
- Then the server reads one `Snapshot` and calls `take_in`; if a changed path's hash in that snapshot
  differs from the stable one, the batch goes on instead. Polls run every 50 ms, so an outside write is
  taken in 150–250 ms after its last byte.

FSEvents (the `notify` crate) is not used in step 6: the stat walk is simple, deterministic and testable
with a fake clock. G10 measures the walk on p50 with a 5,000-file `assets/` folder; over 2 ms per poll,
FSEvents with a slow poll as fallback replaces it (24).

### 8.3 What the watcher feeds

`take_in` (3.5) returns what changed, and the actor settles the engine from what plays (5.5), so a lane
file written by an agent or by git takes the same `SwapLane` as the piano roll (PLANE-1). A lane file
caught half-written never empties its lane: the settle rule doesn't take in a file still changing, a
settled file that doesn't parse keeps its last accepted data (FMT-9), and a batch that also changes code
that doesn't compile waits whole (3.3). All three are tested (19).

### 8.4 `data.claimed` (DIAG-3)

`claims(rev) -> BTreeMap<module key, BTreeSet<path>>`: the lane and state files each module's declared
lanes and stateful plugins read. A Tx's check and commit raise the Notice `data.claimed` for each path that
existed before the Tx, no module claimed before it, and some module claims after it ("Kicks now plays
data/kicks/hits.json, which nothing played before"), on the Tx's result only (deterministic: it depends on
B and the Tx). A relink moves data to a new path, so it raises none; an added track whose key matches a
folder left behind does. `take_in` raises the same Notice in `Accepted.notices` for an outside write
(session-only; never in `problems()`, project reads or `m2 check`). Detecting a swap of keys within one
change waits for SiteIds (step 7).

---

## 9. Opening a project whose code doesn't compile (WRITE-20, owner decision 6)

`Session::open` fails only when the folder can't be read, has no `music2.toml`, or its `format` is
missing, older or newer (FMT-3), as `OpenError { diagnostics }`. Otherwise:

- the lock is taken or the session is read-only (7); journals are scanned (6a.4);
- the snapshot is read and compiled. If the code (or the manifest, or the arrangement) doesn't build, the
  accepted revision is the **empty revision**: `number = 1, code_rev = 0, data_rev = 0`, the snapshot as
  read, `Project::empty(dir, name)` (the manifest's `name` read leniently, else the folder's name) and
  `Compiled::none()` (an IR with no modules, an empty `Flat`, no data). `draft_errors` holds why, and
  every path of the snapshot is waiting (3.3);
- the view, the timeline and the controls of the empty revision are empty; `files::list` still lists the
  disk's code files (as `accepted: false`), so the code editor opens them;
- `save()` works (it needs only the lock); a data Tx is `Rejected` (WRITE-14: there is no accepted code
  revision); a code Tx is `Rejected` (the draft doesn't compile);
- once a save (or an outside write) makes the code compile, the draft is taken in whole as revision 2
  with `code_rev = data_rev = 2`, and the server builds a renderer: playback starts.

The server's `Actor::open` no longer fails on code: it publishes the empty revision, loads nothing into
the player (silence), and settles normally afterwards (`Way::Load` the first time a revision has code).

---

## 10. Undo as stored inverse Tx (WRITE-21)

### 10.1 The history

```rust
struct TxStep {
    label: (String, String),
    inverse: Tx,                       // bases: the hashes this Tx left (or Absent)
    texts: Vec<(TxPath, Arc<str>)>,    // the texts it left in files it edited, for the inverse's carry
    predict: Vec<Prediction>,          // what can_undo checks without compiling
    blobs: Vec<Sha>,                   // before-images it needs from .music2/tx/blobs/
}
/// One stack while gestures are ported (R21): a step of an unported gesture is today's record.
enum Step { Legacy(Change), Tx(TxStep) }
history: Vec<Step>, redo: Vec<Step>    // in memory (deferred: undo across restarts)
```

`undo` and `redo` dispatch on the variant, so steps of ported and unported gestures interleave in one
order at every checkpoint (22); `Legacy` and `Change` are deleted with the last gesture's port. At most
500 steps and 64 MiB of retained text; the oldest go first. A new commit clears `redo`.

### 10.2 The inverse of each op

Computed at commit from the pre-images the commit read:

| Op | Inverse | Applies while (WRITE-21) |
|---|---|---|
| `TextEdit` | a `TextEdit` per edit, at its range in the new text, `expected` = what it wrote, `text` = what it replaced; base `WithText` of the text the Tx left | its ranges carry (2.4, strict) and still read what the Tx wrote |
| `Write` over a file | `Write` of the old bytes (inline text; a binary as `Payload::Local` of its blob, cloned into staging, never linked) | the file's hash is the one the Tx left |
| `Write` of a new file | `Remove` | the same |
| `Remove` | `Write` of the old bytes, base `Absent` | the name is still free |
| `Rename` | `Rename` back, bases: `to` at the hash it left, `from` `Absent` | the same, and no name in other capitals |
| `Clips` | `Clips` putting the old version of every clip it changed or deleted and deleting the ids it added; base `WithText` of the lane file it left | every clip it touches has the canonical line the Tx left, or is still absent; else `Conflict` naming the lane and the clip |
| `save()` | `Write` of the old text | the file's hash is the one the save wrote |

The inverse's expectation (R22):

- **An inverse that touches only Data-class files** (lane files, states, sidecars, module folders):
  `Confined { lanes: every lane the restored, removed or moved files feed }`, computed when it is undone
  from the accepted revision's declarations (a data Tx can't change them), with no ranges, so no node may
  change. Its bases already guard the bytes; so undoing a delete after the lane was declared again, or a
  relink after a module claims its old path again, restores the bytes as today's `Undo::Data` and
  `Undo::Move` do.
- **Any other inverse** mirrors the forward's: `Values` with each anchor at its new range and its old
  value (so "the knob's units changed around it" is `Rejected`, not a wrong value: today's `Undo::Value`
  scale rule falls out); `Confined` with the forward's ranges and lanes in the new text; `Identical` for
  `Identical`.

Redo is the inverse of the undo, recorded when the undo commits, as today. Every inverse is an ordinary
Tx: checked, journaled, and taken to the engine through the delta from what plays.

### 10.3 `can_undo` without compiling

`can_undo` = writer access, not `draft_errors` for an inverse that touches code, every base applies
(hash equal, or the carry succeeds and reads the recorded text), no target waits (3.3), and each
`Prediction` holds: for a `Values` inverse, the anchor's carried range still names a const of the recorded
scale in the accepted revision (today's `revertible`); for a `Clips` inverse, the touched clips' lines are
as left. So undo is never offered when it would be refused for certain.

### 10.4 What changes in meaning, by the contract

WRITE-21 locates a text edit by its carried range, where today a structural edit's undo needs the whole
file exactly as the edit left it (`Undo::Exact`). So after a cable, an effect or a lane edit, an unrelated
change elsewhere in the same file no longer blocks its undo; a change that touches the edit's own bytes,
or its own clips, still does (`Conflict`). Two tests are adjusted (20.1).

---

## 11. Fixes become Tx (DIAG-1, DIAG-5, DIAG-6)

```rust
pub struct Fix { pub code: &'static str, pub title: String, pub likely: bool, pub tx: Tx }   // m2_song::diag
```

`diag::Edit` and `diag::Op` are deleted. Each builder of a fix now builds a Tx with its bases from the
snapshot that raised the problem (`Snapshot::sha`, raw bytes), its label, and its expectation:

| Fix | Built in | Ops | Expect |
|---|---|---|---|
| `relink` (a lane file) | `core/session/src/unclaimed.rs` | `Rename` | `Confined { lanes: [to] }` |
| `relink` (a module folder, ID-17) | `unclaimed.rs` | one `Rename` of `data/<k>` → `data/<new>`, base = `dir_hash` | `Confined { lanes: every lane of the new module the folder holds }` |
| `relink` (a plugin state) | `unclaimed.rs` | `Rename` of blob and sidecar | `Confined {}` (C1) |
| `delete-data` | `unclaimed.rs`, `core/song/src/data.rs` | `Remove` of each file | `Identical` |
| `assign-clip-ids`, `renumber-clips`, `repin-asset`, `fix-asset-name`, `retag`, `convert` | `core/song/src/data.rs` | `Write` of the canonical lane text | `Confined { lanes: [its lane] }` |
| `finish-tx`, `discard-tx` | `core/session/src/tx/journal.rs` | per 6a.4 | `Journal(jid)` |

A fix's Tx id is derived from the problem **and the content it was raised on** (R19): `fix-` + the first
12 Crockford characters of SHA-256(code ‖ NUL ‖ src ‖ NUL ‖ fix code ‖ NUL ‖ every base hash, sorted by
path), keeping the content input today's `mint` has (`core/song/src/data.rs`). Clip ids a fix creates are
derived from that Tx id (FMT-11). So a fix is the same in the app, the CLI and an agent's read for the
same files (DIAG-6), and the same fix on other contents (the clips replaced, another git branch) mints
other ids.

Applying a fix is `commit(fix.tx)` (DIAG-1): a stale base is a `Conflict` like any Tx, so the check that
"the fix is still offered" (`Session::fix`) is deleted, along with `Session::fix`, `relink`, `delete_data`
and `rewrite_lane` in core. The editor keeps `relink(from, to)` and `delete_data(path)` for the CLI's
commands and the strip's buttons (14.6). Restore (DIAG-6's third fix, rename the key back) needs
`ModuleChange::Rekeyed` and a key-literal edit; it is built in `crates/editor` with step 8b's key rename,
not here.

---

## 12. Lane writes from builders: ids and bases (R6)

The piano roll sends the whole lane (as today), with the lane file's hash it was read at
(`TimelineLane.sha`, `None` when the file doesn't exist); the web keeps the hash each answer returns. The
builder reads that base text from the store (a base that isn't there is a `Conflict`, and the web reloads
the lane), parses it as the lane's kind, and then:

- **An addressable base lane** (`LaneData::addressable()`: every clip has a valid id and no id repeats):
  the builder keeps the client's ids, mints random ones for clips without a valid id or with an id taken
  earlier in the request (FMT-11, FMT-12; outside a sandbox), and emits a `Clips` op that puts every clip
  whose canonical line differs from the base and deletes every base id missing from the request. Only
  what the user changed travels, so an outside change to other clips merges (WRITE-3).
- **A base lane with an anonymous, invalid-id or duplicated clip** (FMT-12 allows the first two; a text
  editor's copy-paste makes the third): no clip of it can be put or deleted by id with certainty, so the
  builder emits a whole-file `Write` of the canonical lane, ids assigned as today's `write_notes` does,
  with base the lane file's hash and `Confined { lanes: [the lane] }`. Editing one note of a lane whose
  only clip has no id leaves exactly one clip (`a_note_edit_in_an_anonymous_clip_keeps_one_clip`).
- A lane whose file doesn't exist yet: a `Write` of the canonical lane, base `Absent`.

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
