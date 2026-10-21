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
 "host": "my-mac", "host_id": "…", "pid": 4121, "program": "music2",
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
     by its `(inode, size, mtime, ctime)` stamp against the clone's source, re-hashed when that stamp
     differs or is racy (FMT-16's rule). If they differ, an outside writer replaced the file since the
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
  pushes the folders (`fsync`), removes the journal (every target is at before again), and returns
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
with this one file replaced, taken in by 3.3's table as a one-path batch; the watcher is told the path
and its new stamp (`Settle::wrote`), and every other changed path is left to the watcher. It is recorded
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

