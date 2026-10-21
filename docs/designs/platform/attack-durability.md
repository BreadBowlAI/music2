# Attack: crash safety and durability on Linux

Label `attack-durability`. Written 2026-10-14 on the Linux server, against `design.md` revision 1 and `main` at `f126d03`.
Every claim was checked in source or measured here, unless it is marked *(unverified)*.

## In short

Three real problems. The first one can lose a file, and the design's own fix for it can't work as written.

| # | Problem | How bad | Fix in one line |
|---|---|---|---|
| 1 | A save stopped halfway on a volume without an atomic swap (NFS, SMB, exFAT, ecryptfs). S7 says the missing file "counts as at its before hash". Then Finish always fails, Discard deletes the file for good, and a half-saved `music2.toml` makes the project unopenable. | high | Drop the "move aside" step. Without a swap, compare the file, then rename the new one over it in one step. The file is then never missing, and S7 needs no new state. |
| 2 | Bounced and exported audio is never synced, but the Tx that places it is. After a power cut, the lane points at an empty file. | medium | Push the new file and sync its folder before anything names it. |
| 3 | When a project opens, the tidy removes a killed commit's journal after a "barrier". On Linux that barrier makes none of the commit's renames durable. | low | Push the journal's folders before removing it. |

---

## 1. A save stopped halfway can't be recovered as S7 designs it (high)

**Where.** `design.md` §8.2 S7 and §6.1 (the WRITE-16 edit). Code: `core/session/src/tx/stage.rs` 668-679 (the fallback),
`core/session/src/tx/journal.rs` 454-522 (Finish and Discard), `core/session/src/session.rs` 233-250 (open).

**The setup.** Take a volume without `RENAME_EXCHANGE`. Examples: an NFS home folder, an SMB share, an exFAT stick, or an ecryptfs
home. Every replace there takes the fallback:

| Step | What happens |
|---|---|
| a | `song.ts` is renamed to `.music2/tx/<jid>/old-0` |
| b | `old-0` is compared with the before hash |
| c | the staged file `.music2/tx/<jid>/0` is renamed to `song.ts` |

The app is killed between a and c: a crash, a kill, or a power cut after step a reached the disk. Now `song.ts` is missing.

**What S7 does, as written.** In `at()`, a target that is missing while `old-<k>` holds its before bytes "counts as at its before
hash".

| Fix offered | What really happens |
|---|---|
| Finish | `finish_tx` builds a Write whose base is the before hash (`journal.rs:466`). The commit compares that base with the disk, where nothing is (`tx/mod.rs:1047`). So it is refused as a conflict, every time. |
| Discard | `discard_tx` only touches targets at their after hash (`journal.rs:490`), so it writes nothing for `song.ts`. Committing it still tidies the journal (`stage.rs:558-560`). That deletes `old-0` (the before bytes), `0` (the after bytes) and the journal (the before text). **`song.ts` is gone for good**, after the user clicked "put every file back as it was". |
| "rename `old-<k>` back" (the S7 text) | Can't be written as a fix. A Tx can't name a path under `.music2/` (`core/song/src/tx.rs:246`). |

**Worse: `music2.toml`.** The manifest is a text file that `save()` and any Tx may write (`tx.rs:286-294`). Suppose it is the
missing file. Then `Session::opening` refuses the folder for having no `music2.toml` (`session.rs:233-237`). That happens before it
takes the lock and reads the journals (`session.rs:245-250`). So the app and `m2 fix` both say "not a project", and no fix is ever
offered. The kill test's five cases never write `music2.toml`, so the planned `plain: true` run can't catch this.

**What the planned test does catch.** For `song.ts`, the `plain: true` kill test would fail: Finish panics (`kill_tests.rs:176`), and
Discard's "before bytes" check fails (`kill_tests.rs:201`). So the build would stop there. But the new WRITE-16 sentence is wrong, and
the design gives no mechanism that works.

**Fix (simpler than S7).**
- **Replace without moving aside.** Without a swap, read the target and compare it with its base, as today. Then rename the staged file
  over it, in one plain `rename`. That replaces the file in one step on every POSIX volume, and `MoveFileExW` with `REPLACE_EXISTING` does
  the same on NTFS. So the target always holds its before or its after bytes.
- **Rollback in the same process.** Check that the target still holds the after bytes. Stage the before bytes (in memory as
  `Before.bytes`, and in the journal or `blobs/`). Rename them over the target.
- **Recovery needs nothing new.** Every target is at its before or its after hash, and `journal.rs` already handles both. S7 shrinks to
  this: the kill test runs every case with `Recorder { plain: true }` too, plus one new case that writes `music2.toml`.
- **The race with an outside writer stays the same size.** Today, an outside file made between the move aside and the second rename is
  overwritten. After the fix, one made between the compare and the rename is. WRITE-19 already says compare-and-swap narrows this race
  but can't close it.
- **Bonus.** The file that goes missing for up to 0.19 ms during each save (`ground/e2e.md`) is then gone on NFS, SMB, exFAT and Windows
  too, not only on ext4.
- **If the move aside is kept anyway:** give `at()` a third state ("moved aside": missing, with `old-<k>` at its before hash). Make
  Finish and Discard create-only writes (base Absent): Finish from `.music2/tx/<jid>/<k>`, Discard from the before image.
  `valid_local` already accepts both paths (`tx.rs:333`). Open must also read the journals before it refuses a folder whose
  `music2.toml` a journal moved aside. That is more code, and readers still find the file missing.

## 2. Bounced and exported audio is never synced, but the Tx that places it is (medium)

**Where.** `core/runtime/src/audio.rs` 73-89, `apps/m2/src/main.rs` 454-493, `crates/editor/src/render.rs` 247-262 and 446-457,
`crates/server/src/export.rs` 127-129, `crates/server/src/project.rs` 298-345. `design.md` §3.1 OS-07 edits `name_new` but adds no sync.

**The setup.**
- `m2 bounce --place` renders a WAV into `assets/`. `write_new_wav` creates it, writes it and never syncs it (`audio.rs:73-89`).
- It writes the provenance file, also not synced (`main.rs:480`).
- Then it commits a Tx that places a clip pinning the WAV's hash (`main.rs:488-493`). That Tx is journaled and synced.
- Export Audio into the project's `assets/` works the same way: an unsynced part file, then a hard link (`render.rs:247-262`, `446-457`).
- A new song's copied files are never synced either (`project.rs:298-345`).

**What goes wrong on ext4.** ext4 delays choosing disk blocks for new data. The Tx's own syncs force a journal commit. That commit
makes the WAV's name and its file entry durable, while its bytes are still only in memory.

**Measured here** (`$SCRATCH/logs/ad-delalloc-1014-222842-1284066.log`, script `ad-delalloc-probe.sh`):

| Step | Result |
|---|---|
| Write a 40 MB file and hard-link it the way `name_new` does | the data has no disk blocks yet (`delalloc, unknown_loc`) |
| Then sync a journal file and two folders, as a Tx does | took 0.46 ms |
| Is the WAV on disk after that? | no: still `delalloc, unknown_loc` |
| When did its bytes reach the disk? | 35.2 s after the Tx's syncs (`dirty_expire_centisecs` is 3000 here, plus the 5 s flush interval) |

**After a power cut in that window:** the lane is durable and points at an asset with no bytes. ext4 writes the size to disk only once the
blocks exist, so the file would be empty. Reading the disk needs root, so that size is *(unverified)*. The clip's pinned hash fails, and
the bounce is lost.

The step 6 design assumed that "a bounce or an export" writes "and `fsync`s 1 GB in 4 MB chunks" (`step6/design-6.md:1776`). The code
never syncs.

**Fix.**
- Any file music2 creates outside a Tx is pushed (`m2_os::fs::push`) before it gets its name. Its folder is synced (`fs::sync_dir`)
  after, before any Tx names it.
- Four places need it:
  - `render.rs` `write_as`: push the part before `name()`, then sync the folder. S6 already edits `name_new` there.
  - `audio.rs` `write_new_wav`.
  - The provenance file in `main.rs`.
  - `project.rs` `create_at`: push each copied file and sync each folder (about 20 files).
- On macOS the push reaches the drive, and the placing Tx's `F_FULLFSYNC` makes it durable. On Linux the push alone is durable.
- Cost: one `fsync` of the WAV. That takes about as long as writing it (tens of ms here for 40 MB), next to a render that takes seconds.
- Contract: add one sentence to WRITE-1 (or FMT-14): "a file created outside a Tx is made durable before anything names it". Step 9's
  recorded takes then follow the same rule.

## 3. The open-time tidy trusts a barrier that makes nothing durable on Linux (low)

**Where.** `core/session/src/tx/journal.rs` 291-302, `core/session/src/tx/stage.rs` 552-556 and 603.

**The setup.**
- A commit is killed after its renames, but before step 8 (`push_folders`, `stage.rs:556`). Its renames exist only in memory.
- The next session takes the lock and scans. It finds every target at its after hash.
- It calls the barrier on the journal file, then removes the journal and its staging folder (`journal.rs:291-302`).
- WRITE-15's *Today* promises: "the removal is never durable before the renames are".

**What the barrier does on Linux.** It runs `fsync` on that journal file, which hasn't changed since it was written.

**Measured here** (`$SCRATCH/logs/ad-jbd2.log`, script `ad-jbd2-probe.py`), 300 rounds each:

| What each round did | Forced journal commits |
|---|---|
| two renames only | 0 |
| two renames, then `fsync` of an unchanged file (the scan's barrier) | **0** |
| two renames, then `fsync` of the folder | 300 |

So the barrier makes none of those renames durable.

- **ext4 is safe, by luck of its design.** It has one journal for the whole disk and commits it in order. So the removal can't reach the
  disk before the renames.
- **Some file systems log per file** (btrfs, Fedora's default). There, the next commit's sync of `.music2/tx/` could make the removal
  durable while the renames are still only in memory *(unverified: no btrfs volume here)*. After a power cut, the project would hold the
  before bytes and no journal, though the user saw the after bytes.
- **A second way into the same gap.** `push_folders` ignores errors (`stage.rs:603`). The journal is still tidied at the next barrier,
  and on Linux that barrier doesn't cover the folder.

**Fix.**
- Before its barrier, the scan pushes a completed journal's target folders, with the same `push_folders` that step 8 uses. It removes
  the journal only if every push worked. A folder `fsync` with nothing new costs 0.000 ms here (`ground/facts.md` §2), and the scan runs
  once per lock.
- `push_folders` reports whether every push worked. A journal whose push failed stays until a later push of its folders works (at step
  6 or at close).
- WRITE-15's *Today*: say that the tidy follows a push of the commit's folders, not only a barrier.

---

## Checked, and no problem found

| What | Result |
|---|---|
| `fsync` or `fdatasync`, and the cost | `fsync` per push is right, and on Linux each push is durable alone. The costs are as in `ground/facts.md` §2 (best case: the drives here have power-loss protection). On Linux the cost is spread over every push, not in the barrier. G11's "durable sync" times one new small file, not a whole commit. That is information only (PLAT-1). |
| Reading `EINVAL` from `renameat2` as `Unsupported` | Harmless. A real `EINVAL`, such as a folder moved into itself, fails again in the plain rename and is rolled back. No per-volume probe is needed. |
| The clone fallback (ext4 copies) | The copy is compared with the before hash after it is made, and it is synced. Correct. A blob is written twice on ext4. |
| Two sessions in one process | On local Linux the lock belongs to the open file (`ground/facts.md` §4). The second session is read-only, as on the Mac. |
| The lock on NFS | The design's "becomes a per-process lock" is likely wrong: the kernel keeps the open file as the lock's owner *(unverified)*. Either way, `lock.local` is raised. |
| The lock on SMB (Linux 5.5 and later) | Its locks are mandatory (`flock(2)`, "CIFS details"). `read_holder` reads through another descriptor and gets `EACCES`. So the message says "another program" and doesn't name it. Cosmetic. |
| Machine id | This server has `/etc/machine-id`. A container without one falls back to its host name, a container id. So an earlier container's journal is `tx.foreign`, which is what WRITE-16 says. Cloned VMs that share an id and a synced folder are rare: not worth code now. |
| The local-volume check | It is a deny-list, so a type it doesn't know counts as local. VirtualBox's `vboxsf` is missed (and Parallels' shared folders, *unverified*), so no `lock.local` notice appears there. The 5 s backstop walks still catch outside edits. If wanted: an allow-list of local disk types, and every other type handled like a network volume. That is the design's own "safe side" rule for FUSE. Low. |
| Case-insensitive volumes | The Tx's `spelled()` lists the folder and compares the names byte by byte. So it works on vfat, exFAT and casefold folders, on Linux too. |

## Probes

All are kept in `$SCRATCH/logs/`: `ad-delalloc-probe.sh` with its run `ad-delalloc-1014-222842-1284066.log`, and `ad-jbd2-probe.py`
with `ad-jbd2.log`. The scratch folder is deleted. No cargo target dir was made.
