# Attack: the file watcher on Linux

Written 2026-10-14 on the Linux server, against `design.md` revision 1 and `main` at `f126d03` (label `attack-watcher`).
It covers §3.4 (the inotify source), §2.5 (why in-tree, why one instance) and slice S8. The code read is
`crates/server/src/graph/{watcher,fsevents,watch_tests,tests}.rs`, `core/session/src/watch.rs`, `core/session/src/tx/stage.rs`,
`crates/server/src/{graph,lib}.rs`. Probes ran here. Their sources and output are in `$SCRATCH/logs/aw-probes.txt`.

## In short

Three real problems. None loses data. Each has a small fix.

| # | Problem | How bad | Fix |
|---|---|---|---|
| 1 | Every write to a file wakes the watcher. FSEvents waits 10 ms to group events; the inotify design doesn't. | medium | The reader thread waits 10 ms after each read. |
| 2 | After a lost event ("Rescan"), new folders stay unwatched. The loss is then counted against events. | medium | After an overflow, watch the folders again. If a watch can't be added, switch to walks. A walk after a Rescan counts no misses. |
| 3 | No planned test would fail if watching new or moved folders were broken. The 5 s walk hides it. | medium | 4 tests that turn the 5 s walk off, so only events can find the change. |

What holds (checked): "flush" is right, the session's own writes stay quiet, tests 1 to 5 should pass with the design,
one shared instance is justified, idle CPU doesn't change. See the last section.

---

## Problem 1: every write wakes the watcher

**What the design says.** One reader thread "blocks in `poll` on the inotify fd, then reads and dispatches" (§3.4). The mask
includes `IN_MODIFY`. Each event goes to `heard`, which adds a hint and calls `Wake::wake` (design §4.1 test 4).

**What happens.**
- inotify merges two events only when the second is the same as the last one *still unread* (`facts.md` §3c).
  A reader that reads at once never leaves one unread, so every `write` call becomes its own event.
- While no batch is open, each wake makes the watcher tick at once (`graph.rs` `watch_wait`; `watcher.rs` `wake_at`).
  So the watcher ticks once per write.
- On the Mac, FSEvents groups events over 10 ms (`fsevents.rs:159-164`, `LATENCY` with `NO_DEFER`). That is at most
  about 100 wake-ups a second *(from Apple's flags; not measured on a Mac)*.

**Scenario.** You bounce a stem from another app into `assets/`, or convert samples into it with ffmpeg. The file is
not one any lane plays, so it opens no batch (R14). The writer uses 4 KB writes. Each write wakes the watcher, which
stats the file, finds nothing to do, and waits again. That repeats for as long as the write lasts.

**Measured** (C probe: one inotify fd with the design's mask, a reader thread, a writer thread):

| Write | Reader reads at once | Reader waits 10 ms after each read |
|---|---|---|
| 1 GB in 4 KB writes (0.8 s) | 161,874 to 182,735 wake-ups; the reader thread alone used 0.50 s of CPU | 76 wake-ups; 0.001 s |
| 256 MB in 4 KB writes | 31,574 to 41,979 wake-ups | 20 to 31 wake-ups |
| 20 MB at 2,000 writes a second (2.7 s) | 4,884 wake-ups (1,800 a second) | 272 (100 a second) |

On top of the reader's cost, each wake-up is one watcher tick (a stat of the path, git's lock check, two locks). Its
cost per tick is not measured, since there is no inotify watcher yet *(unverified)*. At 200,000 a second the watcher
thread would be busy the whole time. music2's own exports cost the reader too: their part file starts with a dot, so
it never wakes the watcher, but every 8 KB write is still read and dropped.

**Fix.** After each read and dispatch, the reader thread sleeps 10 ms before it polls again. This is FSEvents'
`LATENCY`, so both OSes behave alike.
- The first event after a quiet spell still goes out at once, like `NO_DEFER`.
- `flush` doesn't wait: it takes the mutex and reads at once, as designed.
- No overflow from the pause: 8,000 new folders with 8 files each (105,000 events) went through with no overflow and
  every folder watched (measured).
- Keep `IN_MODIFY`. It names a file that is still open, so a long write is not counted as a miss.

Add to §3.4: "After each read, the reader waits 10 ms (FSEvents' latency). The kernel merges a writer's events
meanwhile: 76 wake-ups instead of 180,000 for 1 GB in 4 KB writes."

---

## Problem 2: a Rescan doesn't repair the watches, and is blamed on events

**What the design says.** "Rescan on `IN_Q_OVERFLOW` … on `IN_UNMOUNT`, and on the root's `IN_MOVE_SELF` or
`IN_DELETE_SELF`." "A failed watch later gives `Rescan`; the watcher's existing rule ('events that missed changes give
way to walks') covers a folder left unwatched." Only a folder move rebuilds the watch map.

**Three gaps.**

| Gap | What goes wrong |
|---|---|
| a. Overflow loses folders | A new folder gets a watch only when its `IN_CREATE` is read. After an overflow those events are gone. The Rescan walk takes the files in, but the folders never get a watch. |
| b. A later watch fails, or the root's watches go | `ENOSPC` (an IDE or Dropbox holds most of the user's watches), or the volume is unmounted (`IN_UNMOUNT`, then `IN_IGNORED` for every watch). The source still says "events", but they can no longer cover the project. |
| c. The loss counts as a miss | A walk that a Rescan asked for finds the burst's changes. No hint names them, because they were lost, or dropped past 4,096 (`fsevents.rs:91-94`). So `apply` records them as "unnamed" (`watcher.rs:405-417`). After 60 s each burst counts as one miss. Two in 10 minutes and the project is walked for the rest of the session. This one happens on the Mac too. |

**Scenario.** You unzip a drum library (2,000 kit folders, 8 files each) into `assets/` while the machine is busy, and
the reader thread is held up for 300 ms. The queue overflows at 16,384 events. The Rescan walk takes everything in.
Later you replace a sample in one of the unwatched kit folders, or an agent rewrites a lane file in a folder made in
that window.
- No event comes. The change waits for the 5 s walk instead of about 0.2 s.
- After 60 s the log says "file system events never named …".
- If that happens twice in 10 minutes, the watcher walks everything until the project is opened again.
- If it happens less often, every edit in those folders keeps waiting up to 5 s.

**Measured** (C probe: a design-like source that watches each new folder when it reads its `IN_CREATE`, watching it
before listing it):

| Reader held up | Overflows | Folders with no watch (of 2,000) | Events for a later write in one |
|---|---|---|---|
| 0 ms (reads at once) | 0 | 0 | — |
| 50 ms | 0 | 0 | — |
| 300 ms | 1 | 668 to 679 | **0** |

So overflow needs the reader held up for a few hundred ms (a busy or swapping machine, a paused process). That is rare,
but the damage is silent and lasts the whole session.

**Fix** (one routine, one new variant, one condition):
1. **Re-walk on overflow.** On `IN_Q_OVERFLOW`, walk the root's folders again, then give `Rescan`. Watch each folder
   *before* listing it. Adding a watch to a folder that has one returns its old descriptor, so this only adds the
   missing ones. Remove the descriptors the walk didn't reach. This is the same routine the design already uses for a
   moved folder, and the same order `start` and new folders must use. (Listing first, then watching, loses a folder
   made in between.)
2. **Say when events can't cover the project.** Add `Heard::Lost`. The source gives it when a watch can't be added after
   start (`ENOSPC`, `ENOMEM`) and when the root's own watch goes (`IN_DELETE_SELF`, `IN_UNMOUNT`, `IN_IGNORED` on the
   root). The watcher then switches to walks at once. The three lines that do that already exist in `count_misses`.
   FSEvents never sends it. Without this, fix 1 could loop: re-walk, fail with `ENOSPC`, Rescan, re-walk.
3. **A Rescan is not a miss.** A walk that a Rescan asked for records no "unnamed" changes, and clears the ones waiting.
   That is one condition in `apply` (`plan.rescan`), in the code `graph/events.rs` and `watcher.rs` keep for every OS.
   Misses then mean what the rule says: events that never named a change.

Replace in §3.4: "A failed watch later gives `Rescan` …" with "On `IN_Q_OVERFLOW` the source walks its folders again
(watch, then list), then gives `Rescan`. When a watch can't be added, or the root's watch goes, it gives `Lost`, and
the watcher walks from then on. A walk a Rescan asked for counts no misses."

---

## Problem 3: no planned test would catch a broken inotify source

**What the design says.** S8 is done when tests 1 to 5 pass (§8.2: "tests 1–5; lints 4–6: Linux fully green").

**The gap.** FSEvents watches a whole tree on its own. inotify needs one watch per folder, kept right by hand: new
folders, moved folders, a refill after overflow. Those parts are new and the most likely to break. But:
- Tests 1 to 5 write only into folders that existed at open.
- `a_folder_moved_into_the_project_is_taken_in` and `an_assets_folder_replaced_by_a_rename_is_taken_in` check only the
  first take-in. That comes from the parent's event and a walk of the folder, not from the new folder's watch.
- Every change is still found by the 5 s walk, so a missing watch only makes things slower. A test with a 1.5 s
  window may still pass.

**Scenario.** The inotify code lists a new folder before watching it, or keeps the old path after a folder is
renamed. Every planned test passes, and S8 is declared green. In use, edits in new lanes' folders wait up to 5 s,
and then the session drops to walks.

**Fix.** Four tests in `watch_tests.rs`, the same on every OS (FSEvents passes them as it is). Each sets the 5 s walk to
60 s and `grace` to 300 ms, like test 3 already does. So only an event can make a tick look. Each then asserts the
change is taken in within 1.5 s, and that `misses == 0` and `events` is still true.

| Test | Steps | Catches |
|---|---|---|
| a folder made after open is heard | make `lib/more/`, write `lib/more/a.ts`, wait for it, **then write it again** | a new folder never watched (the second write needs the folder's own watch) |
| a folder made and filled at once is heard | `create_dir_all("lib/a/b/c")` and write `c/x.ts` right away; then write it again | listing before watching; missing the files written before the watch |
| a folder renamed inside the project is heard by its new name | rename `lib/more` to `lib/moved`, wait, then write `lib/moved/a.ts` | a watch that keeps the old path (its hint would name `lib/more/a.ts`) |
| after lost events, new folders are heard | make `lib/late/` with a file, then call a test-only `Events::lose_events()` (read and drop what is queued, then act as on `IN_Q_OVERFLOW`; on macOS it only gives `Rescan`); wait for the Rescan to take it in, then write `lib/late/a.ts` again | Problem 2's gap a; and gap c (`misses == 0` after the grace) |

Add them to S8's "Fixes" column, so "Linux fully green" includes them.

---

## Checked, and it holds

| Claim or worry | Verdict | Why |
|---|---|---|
| "Flush is a read that never waits" | holds | An event is queued before the writer's call returns (`facts.md` §3a, 0 misses in 2.8 M). Reading and dispatching run under one mutex, so `flush` also waits for a dispatch already running. Files written in a new folder before its watch never get an event. The folder's own hint covers them, and `covered()` (`watcher.rs:240`) counts a file as named when its folder is. |
| A new folder and its first files | holds, with the order in Problem 2's fix | Watch, then list, then hint the folder. The tick walks the folder after the watch exists. |
| The session's own writes | hold | `RENAME_EXCHANGE` gives `IN_MOVED_FROM`/`TO` for the target's name (`facts.md` §3c). The tick stats it. `own_stamp` (`stage.rs:280`) stats the file after the rename, so the new ctime is in the stamp, and `Settle::wrote` takes it out of any batch. Staging in `.music2/` is never watched. The fallback's moment with no file (exFAT) opens a batch that `wrote` then closes, as on the Mac today. |
| Tests 1 to 5 with the design | should pass *(unverified until S8)* | Test 1: reads give no masked events (`IN_ACCESS`, `IN_OPEN`, `IN_CLOSE_NOWRITE` are not in the mask). Test 2: see the line above. Test 3 uses `deaf`, whatever the source. Test 4: `fixture.put` returns before the test ticks, so the tick sees the whole file. Test 5 accepts one or two revisions in its event case (`tests.rs:1027`). |
| One shared instance, with counted descriptors | justified | A test binary runs up to 32 watchers at once. A few agents' runs on this server could reach the 128 instances per user. A project reopened in one process watches the same folders twice (the new source starts before the old watcher is dropped), so the count is needed. |
| Idle CPU | unchanged | The reader sleeps in `poll`. The watcher thread already ticks every 50 ms (`lib.rs:148`). G11 times the watcher thread's ticks only, on both OSes. |
| The root moved | as on the Mac | inotify watches follow the folder, so moving it away and back keeps them right. A project whose folder moved away fails the same way on both OSes; nothing Linux-specific. |
| Network and FUSE volumes | hold | `is_local` sends them to walks, as on the Mac. A local volume whose events miss other machines' writes (virtiofs, for example) falls back after two misses *(not tested here)*. |
