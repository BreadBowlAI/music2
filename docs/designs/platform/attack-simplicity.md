# Attack on the platform design: simplicity and fit

Label `attack-simplicity`. Written 2026-10-14 on the Linux server, against `design.md` revision 1 at `f126d03`.
The lens: abstractions not needed today, APIs too big or too small, code moved for no gain, and fit with 8a, 8b,
step 9, Windows and the three branches. Each claim was checked in source or measured here, unless marked
*(unverified)*.

## In short

| # | Problem | Severity | Fix, in one line |
|---|---|---|---|
| 1 | On a Mac, the barrier's "fallback" calls the same `F_FULLFSYNC` again. On a volume that refuses it, every save fails. | medium | Fall back to `fsync`, and say so in PLAT-3. |
| 2 | Two `fs` items can't be built on Windows as specified. The Windows step would have to edit callers again. | medium | `fs::stamp(path)` instead of `change_stamp(&Metadata)`; decide what `sync_dir`'s `Unsupported` means now. |
| 3 | The new contract rules say more than is true. PLAT-7 is broken by the design's own API on day one, and PLAT-2 writes a file layout into the contract. | low–medium | Narrow PLAT-7 to the folders music2 writes; take the layout out of PLAT-2; fewer files. |
| 4 | 8b.10 (MIDI keyboard, decided) has no place in the layer. Its design says CoreMIDI, which PLAT-2 forbids outside `m2-os`. | low–medium | Say now: MIDI goes through `midir`, outside `m2-os`; a sequencer that won't open means "no devices". |
| 5 | `ci/check.sh` starts with `rm -rf "$OUT"`, with no guard. | low (outside this lens) | Refuse a folder that the check did not make. |

What held up (no finding): the merges (no branch touches the files this step rewrites, checked with
`git diff main...origin/<branch>`); the plugin main loop's shape for step 9; the free functions instead of a trait;
the shared inotify instance (128 instances per user is a real limit when several test runs share this server).

---

## 1. The macOS barrier's fallback is the same call

**What the design says.** `fs::barrier`: "F_FULLFSYNC on macOS (its own sync on a volume without it)". OS-02:
"Moved (S6) … as today". PLAT-3: `m2-os` "never falls back by itself".

**What the code does.** `stage.rs:141-153`: try `fcntl(F_FULLFSYNC)`; if it fails, call `f.sync_all()`. But on Apple
platforms, Rust's `sync_all` *is* `F_FULLFSYNC`. Rust 1.99.0's own source, `std/sys/fs/unix.rs` lines 1264-1271:

```rust
#[cfg(target_vendor = "apple")]
unsafe fn os_fsync(fd: c_int) -> c_int { libc::fcntl(fd, libc::F_FULLFSYNC) }
```

`stage.rs`'s own comment at line 90 says the same ("`File::sync_all` is `F_FULLFSYNC` there").

**What goes wrong.**

| Step | What happens |
|---|---|
| A project sits on a volume that refuses `F_FULLFSYNC`. Which volumes do is *(unverified)*: Apple's `fcntl(2)` lists it for HFS, FAT, UDF and APFS only, and SQLite falls back to `fsync` on network volumes for this reason. | WRITE-17 lets such a project open, with the Notice `lock.local`. |
| The user edits a note. | The commit reaches `disk.barrier(&jfile).map_err(io)?` (`stage.rs:527`). |
| `F_FULLFSYNC` fails, then `sync_all` fails the same way. | The commit fails. The preview is taken back. Every save fails. |

It also makes S7 moot on the Mac. S7 makes a save stopped halfway recoverable for "the Mac on exFAT or SMB". On a
volume that also refuses `F_FULLFSYNC`, the commit stops at the barrier before any rename.

The bug is old, not new. But this design moves it, describes it as working, and adds a rule (PLAT-3) that it breaks.

**Fix (small, macOS only, inside S6).**
- `mac/fs.rs` `barrier`: `F_FULLFSYNC`; when that fails with `ENOTSUP`, `ENOTTY` or `EINVAL`, `libc::fsync`.
- PLAT-3 says what is allowed: "`m2-os` uses the strongest call the volume accepts (`F_FULLFSYNC`, else `fsync`). It
  never emulates: no copy for a clone, no move-aside for a swap. Those stay with the caller."
- The Mac run at the end of the step covers it. Nothing on Linux changes.

---

## 2. Two `fs` items don't fit Windows, so callers would change again

The design's promise (§2.5, §9): "the Windows step only adds `src/windows/` and one `cfg_attr` line". Two items
break it.

| Item | The design | Why Windows can't do it that way | Checked |
|---|---|---|---|
| `fs::change_stamp(m: &Metadata) -> (i128, u64)` | Windows column (OS-16): "`ChangeTime` and the file index (by handle)" | A `Metadata` has no handle. On stable Rust, Windows' `MetadataExt::change_time`, `file_index` and `volume_serial_number` are unstable (`windows_by_handle`). | The rust-docs of the pinned 1.99.0 toolchain on this server mark all three unstable. |
| `fs::sync_dir` | Windows column (OS-05): "`Unsupported`: a folder can't be flushed" | `stage.rs:522-526` call it with `?`. An `Unsupported` there fails every commit. | Read in source. |

**What goes wrong.** In the Windows step, `change_stamp` must become path- or handle-based. Then both stamp callers
(`core/session/src/watch.rs:64, 165, 255` and `core/song/src/hash.rs:214`) are rewritten. And `stage.rs`'s commit is edited
so that saves work at all. That is the "move code again" this design exists to avoid, in the two files with the
strictest rules (WRITE-15, FMT-16).

**Fix (now, while S6 rewrites these lines anyway).**
- Replace `change_stamp(&Metadata)` with `fs::stamp(p: &Path) -> io::Result<Stamp>`: one no-follow stat inside
  `m2-os` (one system call on Unix, as today), and `Stamp` (size, mtime, ctime, id) lives in `m2-os`. That is also the
  natural home for the one `Stamp::of` the design wants instead of two.
- Decide `sync_dir`'s meaning now. Either: "Ok where the OS keeps new entries durable by itself" (Windows' body then
  returns Ok, with write-through renames), or: `stage.rs` treats `Unsupported` from `sync_dir` as done. One line, with
  a test through the `Disk` trait.

---

## 3. The Platforms rules promise more than is true

### 3a. PLAT-7 is broken on day one

PLAT-7: "Every folder `m2-os` returns MUST come from `m2_os::dirs::home()`".

| Folder in this design | Comes from `home()`? |
|---|---|
| `dirs::clap()`: `/Library/Audio/Plug-Ins/CLAP` (macOS), `/usr/lib/clap` (Linux) | no |
| `dirs::bundled(exe, name)`: next to the app | no |
| Windows' songs folder (OS-36: `FOLDERID_Music`, "often in OneDrive") and cache (`%LOCALAPPDATA%`) | no |
| Linux packaging's `XDG_MUSIC_DIR` and `XDG_CACHE_HOME` (deferred) | no |

So the rule is false at S5, and must be rewritten again at the Windows step and at Linux packaging. What it is for
is test isolation, and that is narrower.

**Fix.** PLAT-7: "Every folder music2 writes outside a project comes from `m2_os::dirs`. When `HOME` is set, each is
under it, so a test or a suite isolates itself by setting `HOME`. Folders music2 only reads (the system's CLAP
folder, bundled files) are exempt."

### 3b. PLAT-2 puts a file layout into the contract, and the layout is heavier than needed

PLAT-2: "one module per concern, one folder per OS with the same functions and signatures". That gives 26 files today:
8 portable files, `unix.rs`, and 8 files in each OS folder. About 10 of the 16 OS files would be under 25 lines
*(estimate from today's code)*: `host` (`gethostuuid`, or reading `/etc/machine-id`), `thread` (one call),
`dirs`, `process` (an empty function on macOS; one `prctl` on Linux), and two `mod.rs` files of re-exports.

The design's reason (Appendix A): "one `linux.rs` would split once 8a and step 9 grow it, which moves code again".
That doesn't hold:
- No branch touches `core/os`, so a split there conflicts with nothing.
- Nothing has to move anyway. `linux/mod.rs` can hold the small concerns, and 8a adds `linux/rt.rs` beside it.
  Measured with a scratch crate here: with the design's `#[cfg_attr(…, path = "linux.rs")] mod os;` a nested `mod rt;`
  is looked for at `src/rt.rs` (error E0583), but `#[path = "linux/mod.rs"]`, or `mod linux; use linux as os;`, finds
  `linux/rt.rs`.

`process::crash_quietly()` is a test helper in the product API. PLAT-2 already lets tests use OS fixtures, so the two
test children can call `prctl` themselves under `cfg(target_os = "linux")`.

**Fix.**
- PLAT-2 says only: "OS-specific code lives in `m2-os`; outside it, code other than tests names no OS." The layout is a
  design choice, not a rule.
- Per OS: `mod.rs` for the small concerns (`fs`, `host`, `thread`, `dirs`), plus `watch.rs` and `main_loop.rs`.
  8a adds `rt.rs`, step 9 `child.rs`. Nothing moves.
- `crash_quietly` stays in the test children.

---

## 4. 8b.10, the MIDI keyboard, has no place in the layer

- `HANDOFF.md`: "8b: … a MIDI keyboard slice (8b.10) is added" (decided).
- `design-8b.md` §19: "**8b.10 MIDI input** … CoreMIDI note input to the selected track's lane".
- This design, §9: "8b | nothing in Rust". MIDI appears only under step 9 (`time::midi_instant`; `midir` "stays
  outside").

**What goes wrong.** When 8b.10 is built after this step, `one_place.rs` has an empty allow-list. CoreMIDI calls
outside `m2-os` fail PLAT-2, and nothing in the layer is shaped for them. On Linux the sequencer is not reachable
here: `/dev/snd/seq` is `root:audio 0660` and the user is not in `audio`. Opening it gives `EACCES` (measured). A MIDI
backend that treats that as an error would fail every Linux test and every check run.

**Fix (a few lines in §9 and §2.4).**
- 8b.10 uses `midir`, like `cpal` a crate that already hides the OS (add it to §2.4's list). It stays outside `m2-os`.
  `design-8b`'s "CoreMIDI" reads "midir".
- Rule: an input that won't open (no sequencer, no permission) means "no MIDI devices", never an error.
- Tests inject notes at 8b.6's source level, never through an OS port.
- `time::midi_instant` waits until sub-block placement needs it, as survey-later §4.5 says.

---

## 5. `ci/check.sh` deletes its output folder without a guard (outside this lens)

```bash
OUT=${M2_CHECK_OUT:-$HOME/m2-check/$SHA}
rm -rf "$OUT" && mkdir -p "$OUT/home" || exit 2
```

§7.4 tells agents to pass `M2_CHECK_OUT=$SCRATCH/check-<label>`. One slip,
`M2_CHECK_OUT=$SCRATCH`, deletes `$SCRATCH`: the `tw` helper, the unpacked plugins in `$SCRATCH/plugins/clap`,
every log and every target folder.

**Fix.** If `$OUT` exists and has no `.m2-check` marker, stop with a message. Write the marker after `mkdir`.

---

## Checked and not a problem

| Claim looked at | Result |
|---|---|
| Merges: does any branch touch the files this step rewrites (`main_thread.rs`, `plugin_host.rs`, the AppKit test, `stage.rs`, `lock.rs`, `journal.rs`, `watch.rs`, `fsevents.rs`, `watcher.rs`, `core/kernel`)? | No (`git diff --name-status main...origin/<branch>`). step 7 adds OS code only in `crates/gates/src/store.rs`. |
| Does any branch call `use_process_main_thread` outside `plugin_host.rs`? | No. step 7's `probe.rs` calls `main_thread::token` and `is_process_main`, which stay as wrappers. |
| Does any branch reference `core/kernel` by path? | No. Only the root `Cargo.toml` and `std/kernels-src/Cargo.toml`, which this step edits. |
| Does `sounds` use `Math.` or `**` in TypeScript? | No. |
| A shell wrapper for Chrome instead of launch args | Needed: 20 suites each call `puppeteer.launch` with their own `args`. |
| Does `baseline.sh` give the renders a scratch `HOME`? | Yes (`r()` sets `HOME=$OUT/home`). |
| inotify instances in use by the user now | 2 of 128. |
