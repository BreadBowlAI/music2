# Ground facts: what the Linux server really does

Measured on 2026-10-14 on the owner's Linux server, at commit `f126d03`. Each fact below was
checked on this machine with a small probe program or a command, unless it is marked *(unverified)*.
The probes were C programs (`cc -O2`) and small scratch Rust crates (rustc 1.99.0, own target dir).
They were deleted afterwards; the "Rerun" notes at the end say how to rebuild each one.

## In short

| # | Question | Answer here | What it means for the platform layer |
|---|---|---|---|
| 1 | Exchange and no-replace renames | Both work on ext4 and tmpfs. A bad flag gives `EINVAL`. `FICLONE` gives `EOPNOTSUPP`. | Use `renameat2` for Swap and Excl. Probe each volume once; don't read every `EINVAL` as "unsupported". |
| 2 | Cost of a durable commit | About **0.6 ms** idle and **1.5 ms** p95 while another process writes 1 GB. | Far inside PLANE-5's 20 ms. But this disk is a server disk with power-loss protection; a laptop disk is slower *(unverified)*. |
| 3 | Is an inotify event there as soon as the write returns? | **Yes.** 0 misses in 2.8 million checks. | "Flush" on Linux is a non-blocking drain. Nothing is ever "on its way". |
| 4 | Lock, ids, names | `flock` works per open file, is freed at exit and at `SIGKILL`. machine-id exists. Host name is `Ubuntu-2404-noble-amd64-base`. | The lock code works as on macOS. Messages will show that host name, not "the Linux server". |
| 5 | Audio | No sound card, no PipeWire, no rtkit, no realtime rights. cpal's default device exists but **fails to open**, and ALSA prints 8 lines to stderr. | "No device" is not `None` here. It is an error from `default_output_config`. Fall back to silent on it. |
| 6 | Host math vs the `libm` crate | glibc and `libm` differ on 0.04% to 9.7% of inputs, always by 1 ulp. `libm` here equals the math inside wasm, bit for bit. | Only `examples/resample` differs from the Mac. With `libm` it renders `39f94764…` here. That should be the hash on every OS. |
| 7 | Is `std.wasm` reproducible? | **Yes**, byte for byte, once the build path is remapped to the Mac's. The committed file is also stale: it was built before a 10-line doc edit in `core/kernel/src/lib.rs`. | Remap the path in `std/build.sh`, pin the toolchain, then rebuild once on purpose. |
| 8 | Room for wasmtime's 4 GiB reservations | Max map count 1,048,576. One process fits **32,261** reservations (the 128 TiB address space), about 5 of step 7's typical renderers. `ulimit -v` breaks them. | The address space is the limit here, not the map count. |
| 9 | What the app needs at run time | WebKitGTK 4.1, GTK 3, libsoup 3, GLib, cairo, gdk-pixbuf, D-Bus, ALSA. glibc **2.39** or newer. `m2` needs only glibc 2.35. | A `.deb` must list these. An AppImage or an older build base reaches more distros. |

---

## The machine

| Part | Value |
|---|---|
| OS | Ubuntu 24.04.3 LTS, kernel 6.8.0-138-generic (PREEMPT_DYNAMIC), x86_64 |
| CPU | AMD Ryzen 9 7950X3D: 16 cores, 32 threads, two CCDs |
| Memory | 124.9 GiB |
| Root `/` | ext4 on `/dev/md2`, software RAID 0 over two Samsung PM9A3 NVMe (`MZQL21T9HCJR`) |
| ext4 options | `data=ordered`, `barrier`, `auto_da_alloc`, `delalloc`, `journal_checksum`, `commit=5`, `relatime`, `stripe=256` |
| Disk write cache | `write through` on both NVMe: no volatile cache (power-loss protected). So a flush has nothing to empty. |
| `/tmp` | on `/` (ext4), not tmpfs |
| `/dev/shm` | tmpfs |
| Toolchain | rustc 1.99.0 (`b940084d7`), wasm32 target installed, glibc 2.39, gcc 13.3 |
| Other load | Other agents ran builds and browser suites during these runs (load average up to 21). "Idle" below means "no writer of mine". |

---

## 1. Renames, clones, hard links, directory sync

**Probe:** a C program that calls `renameat2` (raw syscall and glibc's wrapper), `ioctl(FICLONE)`,
`copy_file_range`, `link`, `fsync`, `fdatasync`, `syncfs` and `sync_file_range` in a folder, and prints
each result or errno. Run in `$SCRATCH/…` (ext4), `/tmp` (ext4) and `/dev/shm` (tmpfs).

| Call | ext4 (`/`, `/tmp`) | tmpfs (`/dev/shm`) |
|---|---|---|
| `renameat2(a, b, RENAME_EXCHANGE)`, both exist | ok, contents swapped | ok |
| `RENAME_EXCHANGE`, target missing | `ENOENT` | `ENOENT` |
| `RENAME_NOREPLACE`, target exists | `EEXIST` | `EEXIST` |
| `RENAME_NOREPLACE`, target missing | ok | ok |
| `RENAME_EXCHANGE \| RENAME_NOREPLACE` together | `EINVAL` | `EINVAL` |
| an unknown flag (bit 20) | `EINVAL` | `EINVAL` |
| `RENAME_NOREPLACE` of a folder into its own subfolder | `EINVAL` | `EINVAL` |
| glibc's `renameat2()` wrapper | present and works | works |
| `ioctl(dst, FICLONE, src)` | **`EOPNOTSUPP`** (95) | `EOPNOTSUPP` |
| `copy_file_range` | ok | ok |
| `link(2)`, new name | ok | ok |
| `link(2)`, name exists | `EEXIST` | `EEXIST` |
| `fsync` on a folder opened `O_RDONLY` | ok | ok |
| `fdatasync` on that folder | ok | ok |
| `fsync` on a folder opened `O_PATH` | `EBADF` | `EBADF` |
| `syncfs` | ok | ok |
| `sync_file_range(WRITE \| WAIT_AFTER)` | ok | ok |

Also seen: a non-root user may pass `RENAME_WHITEOUT` (it succeeded). Not needed by music2.

**Not checked here** *(unverified)*: what a filesystem *without* support returns (vfat, exfat, NFS, 9p, SMB).
No such writable volume exists here, and making one needs root. The survey's kernel-source reading says
`EINVAL`. `link(2)` giving `EPERM` on vfat and exfat is also unchecked.

**What it means**
- OS-03: `renameat2` gives Swap and Excl on ext4 and tmpfs, the volumes the tests and gates use here.
  The 3 Linux failures in `tx/` (`RENAME_SWAP` and the fallback) should go once Swap uses it.
- `EINVAL` has several causes: an unsupported flag, a bad flag mix, or a folder moved into itself. So
  the per-volume probe (survey §1) should try both flags once, on two scratch files in `.music2/tx/`,
  when the lock is taken. After that, an `EINVAL` during a commit is a real error, not "this volume
  can't".
- OS-04: ext4 can't clone. The fallback copy always runs on ext4. `copy_file_range` works and is what
  `std::fs::copy` uses.
- OS-05: `File::open(dir)` opens `O_RDONLY`, so `sync_dir` works as written. Never open a folder with
  `O_PATH` to sync it.
- OS-07: hard links work on ext4. The survey's plan (use `RENAME_NOREPLACE` as the "create-only name"
  instead of `link`) is still the simpler path on every OS.

---

## 2. What a durable write costs on this disk

**Probe:** a C program, 50 runs per line, in a fresh folder on `/` (ext4). Times are in milliseconds.
"Busy" means a second process writes a 1 GB file in 4 MB chunks with an `fsync` after each chunk, over
and over (G11's busy disk). That writer reached 1.15 to 1.2 GB/s. Each case ran 3 times; the table
gives the range of the 3 results.

The commit-like line does what a Tx commit does: write 3 files (1 KB, 4 KB, 64 KB) into a staging
folder, `fsync` each, `fsync` the staging folder, rename the 3 into the project folder, `fsync` the
project folder.

| Operation | Idle p50 | Idle p95 | Busy p50 | Busy p95 | Busy max |
|---|---|---|---|---|---|
| `fsync`, 4 KB overwritten in place | 0.030 | 0.04–0.06 | 0.030 | 0.04–0.12 | 0.13 |
| `fdatasync`, same | 0.030 | 0.035–0.05 | 0.030 | 0.03–0.08 | 0.13 |
| `fsync`, new 4 KB file | 0.11–0.12 | 0.12–0.14 | 0.11–0.12 | 0.14–0.74 | 0.86 |
| `fdatasync`, new 4 KB file | 0.11–0.12 | 0.13–0.15 | 0.11–0.12 | 0.13–0.69 | 0.73 |
| `fsync` of the folder after a new entry | 0.066 | 0.07–0.08 | 0.066–0.072 | 0.09–0.12 | 0.74 |
| `fsync` of the folder, nothing changed | 0.000 | 0.000 | 0.000 | 0.000 | 0.08 |
| journal: append 256 B, then `fdatasync` | 0.082 | 0.10–0.11 | 0.081 | 0.10–0.12 | 0.77 |
| journal: append 256 B, then `fsync` | 0.082–0.087 | 0.10–0.13 | 0.081 | 0.10 | 0.78 |
| **commit-like sequence** (`fsync` files) | **0.56** | **0.59–0.62** | **0.60–0.62** | **1.37–1.48** | 1.58 |
| commit-like, `fdatasync` files, no staging-folder sync | 0.56 | 0.61–0.63 | 0.58–0.64 | 1.26–1.46 | 1.54 |
| `syncfs` after a 4 KB write | 0.17 | 0.19–0.20 | 0.19–1.48 | 0.20–3.04 | 11.6 |

`syncfs` also hit **890 ms** and **150 ms** once each in "idle" runs, while other agents' builds wrote
to the same disk.

**What it means**
- PLANE-5's budget (heard within 20 ms; durable time measured beside it) is easy here. The whole commit
  costs 0.6 ms idle and 1.5 ms p95 busy. The Mac's `F_FULLFSYNC` alone costs about 5 ms idle and 8–14 ms
  busy (contract).
- This disk has no volatile cache, so an `fsync` never waits for a cache flush. A desktop NVMe without
  power-loss protection will be slower, maybe 1–10 ms per flush *(unverified; not measurable here)*. So
  Linux numbers from this server are a best case. Don't use them as the Linux reference for G11.
- `fsync` and `fdatasync` cost the same for small files here. The simplest OS-01 and OS-02 design holds:
  `fsync` per push, and the barrier adds nothing on Linux.
- **Don't use `syncfs` as the barrier.** It waited for other programs' writes (up to 890 ms), as the
  survey warned.

---

## 3. inotify

### 3a. Is the event readable right after the writer's call returns?

**Probe:** a C program with one inotify fd (`IN_NONBLOCK`) watching a folder with the survey's mask.
For each of 7 operations it drains the fd, does the operation, then does one non-blocking `read` and
checks the expected event is there. "Other process" means a forked child does the operation and then
tells the parent through a pipe. 100,000 rounds for each operation, each mode, on ext4 and on tmpfs.

| Operation | Expected event | Misses, same process | Misses, other process |
|---|---|---|---|
| create (`open O_CREAT`) | `IN_CREATE` | 0 | 0 |
| `write` | `IN_MODIFY` | 0 | 0 |
| `close` after writing | `IN_CLOSE_WRITE` | 0 | 0 |
| `chmod` | `IN_ATTRIB` | 0 | 0 |
| `rename` within the folder | `IN_MOVED_TO` | 0 | 0 |
| `renameat2 RENAME_EXCHANGE` | `IN_MOVED_TO` | 0 | 0 |
| `unlink` | `IN_DELETE` | 0 | 0 |

The same result on ext4 and on tmpfs: 0 misses in 2.8 million checks in all. It took 18 s (ext4) and
13 s (tmpfs).

### 3b. Limits

| Setting (`/proc/sys/fs/inotify/`) | Value | Note |
|---|---|---|
| `max_user_watches` | **997,324** | No sysctl file sets it. That is 1% of RAM at about 1.3 KB a watch: the RAM-scaled default. |
| `max_user_instances` | 128 | per user |
| `max_queued_events` | 16,384 | per instance |
| In use by the user now | 4 instances, 137 watches | |

fanotify's own limits are there too (`max_user_groups` 128, `max_user_marks` 1,048,576), but it still
needs `CAP_SYS_ADMIN` for whole-filesystem marks.

### 3c. What events say

**Probe:** a second C program that does each case and prints every event.

| Case | Events seen |
|---|---|
| rename from an unwatched folder into the watched one | `IN_MOVED_TO` only, with a cookie that has no partner |
| rename from the watched folder to an unwatched one | `IN_MOVED_FROM` only |
| rename inside the watched folder | `IN_MOVED_FROM` + `IN_MOVED_TO`, same cookie |
| `RENAME_EXCHANGE` of two files | two pairs: FROM p / TO q, then FROM q / TO p |
| rename over an existing file | FROM + TO only. **No `IN_DELETE`** for the file replaced |
| rename between two watched folders | FROM on one watch, TO on the other, same cookie |
| write through a shared `mmap`, then `msync` | **no event at all**. Only `IN_CLOSE_WRITE` at `close` |
| `ftruncate` | `IN_MODIFY` |
| 5 writes in a row, then `close` | one `IN_MODIFY` (identical events merge), one `IN_CLOSE_WRITE` |
| a watched subfolder is renamed | parent: `MOVED_FROM\|ISDIR`, `MOVED_TO\|ISDIR`; the subfolder's own watch: `IN_MOVE_SELF` (the watch stays valid) |
| a watched subfolder is removed | `IN_DELETE` of its file, `IN_DELETE_SELF`, `IN_IGNORED`, then `DELETE\|ISDIR` on the parent |
| the watched root is renamed | `IN_MOVE_SELF` only |
| the watched root is removed | `IN_DELETE_SELF`, `IN_IGNORED` |
| 16,484 events without reading | exactly 16,384 events, then **one `IN_Q_OVERFLOW` with wd −1**. The rest are lost. |
| one more change after draining that | normal events again |
| a second overflow | one more `IN_Q_OVERFLOW` |

**What it means**
- The survey's *(verify)* holds: the event is queued before the system call returns. So `flush` on
  Linux is "read the fd until `EAGAIN`". It is stronger than macOS's `FlushSync`.
- Treat every name as a hint. Never pair cookies. A rename over a file shows up only as `MOVED_TO` for
  that name, which is enough as a hint.
- Writes through `mmap` make no event until `close`. The settle rule and the walk fallback still matter.
- On `IN_Q_OVERFLOW`, walk everything. On `IN_MOVE_SELF` or `IN_DELETE_SELF` of the root, rescan.
- The watch limit is far away here. The 128-instance limit is per user and shared with editors, so use
  one inotify fd per watcher, not one per folder.

---

## 4. The lock, machine id, host name, filesystem type

**Probe:** a C program for `flock`, and a Rust program for `std::fs::File::try_lock`. Same results on
ext4 and tmpfs.

| Question | Answer |
|---|---|
| Two `open`s of the lock file in one process: does the second `LOCK_EX\|LOCK_NB` fail? | **Yes**, `EWOULDBLOCK` (also for `LOCK_SH`). Locks belong to the open file description. |
| A `dup` of the holder's fd | gets the lock (same description) |
| Closing a `dup` | keeps the lock |
| Holder exits without unlocking | lock is free |
| Holder killed with `SIGKILL` | lock is free |
| Holder forks, then closes its fd; the child lives on | **still held** until the child exits |
| Reading the file through another fd while it is locked | works (advisory lock) |
| `fcntl(F_SETLK)` write lock while `flock` is held | succeeds: the two kinds don't interact |
| Rust: `File::try_lock` twice in one process | second gives `TryLockError::WouldBlock` |
| Rust: `try_clone()` of the holder, then `try_lock` | `Ok` (same description) |
| Rust opens with `O_CLOEXEC`? | yes (`flags 02100002`); a spawned child had 0 fds of the lock file |

| Identity fact | Value |
|---|---|
| `statfs(/).f_type` | `0xEF53` (ext4) |
| `statfs(/dev/shm).f_type` | `0x01021994` (tmpfs) |
| `/proc/self/mountinfo` for `/` | fs type `ext4`, source `/dev/md2` |
| `/etc/machine-id` | present: 32 hex digits and a newline. `/var/lib/dbus/machine-id` is the same. |
| `/sys/class/dmi/id/product_uuid` | mode 0400, root only (reading it as the user: permission denied) |
| `gethostname` | **`Ubuntu-2404-noble-amd64-base`** (the Hetzner image name; the server's own name is not its host name) |
| time zone | `Europe/Berlin` (CEST); `TZ` unset |
| case | ext4 here is case-sensitive: `Exports` and `exports` are two files. The kernel supports casefold, but no folder here uses it. |

**What it means**
- OS-08: on local volumes, Linux `flock` is the same as macOS. C10 holds.
- `fork` without `exec` would keep a lock alive in the child. Rust's `Command` uses `exec` and
  `O_CLOEXEC`, so it is safe.
- OS-12: `/etc/machine-id` is the id on Linux; no need for root-only DMI.
- OS-15: both `statfs` magic and the mountinfo fs type work. mountinfo gives names (`ext4`), which reads
  better.
- WRITE-18 messages will say "Ubuntu-2404-noble-amd64-base". The owner may want to set a host name
  (`hostnamectl`; a system change, so not done).

---

## 5. Audio

| Check | Result |
|---|---|
| `/proc/asound/cards` | `--- no soundcards ---` |
| `/dev/snd` | only `seq` and `timer`, owned `root:audio`, mode 0660, no ACL for the user |
| `aplay` | not installed (`alsa-utils` absent) |
| kernel modules | AMD SOF audio modules loaded, but no card |
| PipeWire, PulseAudio, WirePlumber, JACK daemons | none running, none installed. `libpipewire-0.3-0` (the library only) is installed; `libpipewire-0.3-dev` is not. |
| rtkit-daemon | not installed; no RealtimeKit service on the system bus |
| user in `audio` group? | no (the group has no members) |
| `RLIMIT_RTPRIO` | 0 (soft and hard) |
| `RLIMIT_RTTIME` | unlimited |
| `RLIMIT_NICE` | 0 |
| `/etc/security/limits.d/` | empty; `limits.conf` has only comments |
| login session | `Type=tty`, no seat |
| `kernel.sched_rt_runtime_us` / `period` | 950,000 / 1,000,000 (realtime may use 95% of a CPU) |
| `kernel.sched_util_clamp_min_rt_default` | **1024** (survey-later marked it *(unverified)*: confirmed) |

**cpal probe:** a scratch crate with `cpal = "=0.18.2"`, default features, as music2 builds it. It
mirrors `Player::start` (`core/play/src/lib.rs` 534-580). Run with a scratch `HOME`.

| Step | Result |
|---|---|
| `cpal::available_hosts()` | `[Alsa]` |
| `default_host()` | `Alsa` |
| `output_devices()` | 1 device: "Discard all samples (playback) or generate zero samples (capture)" (ALSA's null PCM) |
| `default_output_device()` | **`Some("Default Audio Device")`**, not `None` |
| `default_output_config()` | **error:** "The requested audio device is not available. It may have been disconnected." |
| `build_output_stream` | never reached |
| stderr from alsa-lib | 8 lines, starting `ALSA lib confmisc.c:855:(parse_card) cannot find card '0'` and ending `ALSA lib pcm.c:2721:(snd_pcm_open_noupdate) Unknown PCM default` |

**CPU topology** (for step 8a's thread count):

| Fact | Value |
|---|---|
| SMT siblings | cpu *n* and *n*+16 (e.g. `0,16`) |
| L2 | 1 MB per core, shared only by its two SMT threads |
| L3 | CCD0 (cpus 0-7, 16-23): **96 MB** (3D V-Cache). CCD1 (cpus 8-15, 24-31): 32 MB. |
| `cpu_capacity` | absent (all cores are equal in kind) |
| cpufreq | `amd-pstate-epp`, governor `powersave` |

**What it means**
- On a machine without sound, `Player::start` fails at `default_output_config`, not at "no audio output
  device". The "no device → silent player, with a note" rule (OS-61) must catch this error too, and not
  only a `None` device.
- ALSA's 8 stderr lines will show up in every test or CLI run that tries a device. Keep `M2_AUDIO=none`
  for tests. For users, the layer may want to hide alsa-lib's messages (alsa-lib has
  `snd_lib_error_set_handler`; whether cpal leaves room for it is *(unverified)*).
- No realtime here: no rtkit, no `RLIMIT_RTPRIO`, no audio group. Step 8a's realtime path can't be
  tested on this server as it is. It would need rtkit installed, or a `limits.d` rule plus the audio
  group (system changes; not made).
- On Zen, "the cores that share one L2" is one core. The macOS idea "one L2 cluster" maps to one CCD
  (shared L3) here. Which CCD suits audio best is a thing to measure in 8a *(unverified)*.
- Enabling cpal's `pipewire` feature would need `libpipewire-0.3-dev` and clang (clang 18 is installed).

---

## 6. Floating point: glibc vs the `libm` crate

### 6a. Which glibc functions the render path really calls

**Probe:** the expressions from `core/runtime/src/audio.rs` compiled with `rustc --emit asm`.

| Source | Debug build calls | Release build calls (x86_64 and wasm32) |
|---|---|---|
| `x.cos()`, `x.sin()` on `f32` | `cosf`, `sinf` | `cosf`, `sinf` |
| `10f32.powf(x)` | `powf` | `powf` |
| **`2f64.powf(x)`** (pitch, audio.rs 253) | `pow` | **`exp2`** (LLVM rewrites `pow(2, x)`) |
| `x.sin()`, `x.cos()` on `f64` (resample.rs) | `sin`, `cos` | `sin`, `cos` |

So debug builds (the Rust tests) and release builds (`m2 render`, the e2e baseline) call **different
functions** for pitch. With glibc that is harmless: its `exp2(x)` and `pow(2, x)` gave the same bits on
all 192,001 pitch-grid points and all 268 million random points. On the Mac it is *(unverified)*.

### 6b. glibc vs `libm` 0.2.16 (the version in `Cargo.lock`)

**Probe:** a scratch Rust crate that calls glibc directly (`extern "C"`) and `libm` 0.2.16 on the same
inputs, on 32 threads. f32 functions are checked on **every** float in the range. f64 functions on 268
million random points, plus a pitch grid of −96 to +96 semitones in 0.001 steps.

| Function | Inputs | Differ | Share | Largest gap |
|---|---|---|---|---|
| `cosf`, every f32 in [0, 2π] | 1,086,918,620 | 424,030 | 0.039% | 1 ulp |
| `sinf`, every f32 in [0, 2π] | 1,086,918,620 | 1,010,738 | 0.093% | 1 ulp |
| `powf(10, x)`, every f32 x in [−6, 2] (gain −120 to +40 dB) | 2,160,066,562 | 12,419,579 | 0.575% | 1 ulp |
| `exp2`, random x in [−8, 8) | 268,435,456 | 246,483 | 0.092% | 1 ulp |
| `pow(2, x)`, random x in [−8, 8) | 268,435,456 | 26,022,188 | 9.69% | 1 ulp |
| `sin`, random x in [−4π, 4π) | 268,435,456 | 8,337,221 | 3.11% | 1 ulp |
| `cos`, random x in [−4π, 4π) | 268,435,456 | 8,315,580 | 3.10% | 1 ulp |
| `exp2` on the pitch grid p/12 | 192,001 | 170 | 0.09% | — |
| `pow(2, x)` on the pitch grid | 192,001 | 18,198 | 9.5% | — |

Inside `libm` itself, `exp2(x)` and `pow(2, x)` differ on 18,260 of the 192,001 pitch-grid points.
So the fix must name one function and use it in every build: `libm::exp2` is the natural one.

### 6c. glibc on two x86 machines

The same sweep with glibc's FMA and AVX2 code paths turned off (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2,-FMA`;
`ld.so --list-diagnostics` confirmed both bits cleared):

| Function | Same bits with and without FMA? |
|---|---|
| `cosf`, `sinf`, `powf`, `exp2`, `pow` | **yes** (output hashes equal) |
| `sin`, `cos` (f64) | **no** (output hashes differ) |

So the survey's *(verify)* is half right. glibc's f32 functions don't change with the CPU's FMA. Its
double `sin` and `cos` do, so two x86 machines can differ there.

### 6d. The inputs `examples/resample` really uses

The fixture runs at 48 kHz. Its clips use pitch 0, 12, −5 and 7; fades of 0.02, 0.1, 0.01 and 0.3 s;
warp (the WSOLA window); and the 5 ms declick crossfade (`len` 240).

| Set | Values | glibc ≠ libm |
|---|---|---|
| WSOLA window `cosf(TAU·i/2048)` | 2,048 | 22 |
| fade-in 0.02 s, `cosf(PI·i/960)` | 1,920 | 22 |
| fade-out 0.1 s | 9,600 | 134 |
| fade-out 0.01 s | 960 | 10 |
| fade-out 0.3 s | 28,800 | 372 |
| declick crossfade, `sinf` and `cosf` | 720 | 11 |
| gain at 0 dB | 1 | 0 |
| pitch −5, 7, 12, 0 (`exp2` and `pow`) | 8 | 0 |
| **total** | **44,057** | **571** |

`core/play/src/resample.rs` (live only): its f32 tables for 44.1↔48, 48↔96, 48→88.2 and 44.1→96 kHz came
out **identical** under glibc and `libm`. The 1-ulp gaps vanish when the f64 values are rounded to f32.

### 6e. Linux renders vs the Mac's

Linux at `f126d03` (the lead's run, `$SCRATCH/base-f126d03/renders/SHA256SUMS`) against the Mac's at
`8aa301e` (`HANDOFF.md`). `f126d03` is `8aa301e` plus documents, so the code is the same. Linux names
the `--from 4 --seconds 6` files `…-from4-for6.wav`.

| Render | Mac | Linux | Same? |
|---|---|---|---|
| examples/first-song | `6e61b87f…` | `6e61b87f…` | yes |
| examples/first-song, from 4 | `ef0fadca…` | `ef0fadca…` | yes |
| **examples/resample** | `6ac6fcb1…` | `d902bacf…` | **no** |
| **examples/resample, from 4** | `a360d04c…` | `ea243ccc…` | **no** |
| templates/empty | `835aff53…` | `835aff53…` | yes |
| templates/empty, from 4 | `a0249c1e…` | `a0249c1e…` | yes |
| templates/starter | `413d35cf…` | `413d35cf…` | yes |
| templates/starter, from 4 | `71bb7191…` | `71bb7191…` | yes |

As expected: only `examples/resample` differs. It is the only fixture with audio clips, so the only one
that reaches host math.

**What `libm` would give.** The `f126d03` release `m2` was run with an `LD_PRELOAD` library that
replaces glibc's `cosf`, `sinf`, `powf`, `exp2`, `pow`, `sin` and `cos` with `libm` 0.2.16:

| Render | glibc | with `libm` |
|---|---|---|
| examples/resample | `d902bacfd0127aef92850ea34f946a8cef1cc51f80d3584559cd0f2ae271a912` | **`39f9476492e997a86aec5f94b6d0edf56c098e275186260915a3830bd9cfe0da`** |
| examples/resample, from 4 for 6 | `ea243ccc39b0775d2d9e542ea2811e698e93a6185a49bbde0deeaba1a544aae0` | **`d752521b47b66bcb9076f43715baedaa5c018f428ec90546465e6cae031560eb`** |
| examples/first-song | `6e61b87f…` | `6e61b87f…` (no change) |
| templates/starter | `413d35cf…` | `413d35cf…` (no change) |

A second `libm` render gave the same hash again. A glibc render with FMA off gave `d902bacf…` again.

### 6f. Is `libm` the same code as the math inside `std.wasm`?

**Probe:** a tiny wasm32 module built by the same rustc with std's release settings (`opt-level 3`, LTO,
`panic = abort`). It uses Rust's float methods, which on wasm32 call compiler-builtins' copy of libm
(the module has 0 imports). wasmtime 49.0.2, set up as `packages.rs` sets it up, ran it. It hashed the
results over chunks of inputs; the host hashed `libm` 0.2.16's results over the same chunks.

| Function | Inputs | Chunks that differ |
|---|---|---|
| `cosf`, every f32 in [0, 2π] | 1,086,918,620 | **0** |
| `sinf`, every f32 in [0, 2π] | 1,086,918,620 | **0** |
| `powf(10, x)`, every f32 in [−6, 2] | 2,160,066,562 | **0** |
| `exp2`, `pow(2, x)`, `sin`, `cos`: random f64 | 67,108,864 each | **0** |

**What it means**
- The already-decided fix works. With the `libm` crate on the host, host math gives the same bits as
  the kernels' math in `std.wasm` and as `libm` on any other CPU. `libm` uses only IEEE operations; its
  `fma` and `sqrt` are exact on every target.
- Call `libm::exp2(p / 12.0)` for pitch by name. Then debug and release builds agree, on every OS.
- After the change, `examples/resample` should render `39f94764…` (full) and `d752521b…` (from 4 for 6)
  on Linux **and** on the Mac. That is a prediction for the macOS runner to confirm *(unverified on the
  Mac)*. It holds only if the code switches exactly these calls, with the same expressions.
- `fixture_renders_change_only_with_stds_version` records debug-build renders. Those then match release
  too, so one recording serves both.
- The `log10` in `crates/editor/src/render.rs:307` (an export's peak in dB) is a number for display,
  not audio. `core/kernel/src/lib.rs:235` (`10f32.powf`) is host math only in host tests; in use it runs
  inside `std.wasm`.

---

## 7. Is `std.wasm` reproducible?

**Probe:** `std/`, `core/kernel/` and the root `Cargo.toml` were copied to a scratch folder. Then
`cargo build --release --target wasm32-unknown-unknown` was run in `std/kernels-src`, with its own
target dir, three ways. Each build took 0.8 s.

| Build | sha256 | Size | vs committed `std/kernels/std.wasm` (`83cc5961…`, 143,471 B) |
|---|---|---|---|
| A: as `std/build.sh` does it, at HEAD | `5ebef4a8…` | 143,479 | differs: this machine's path is in it (8 bytes longer) |
| B: HEAD, with `--remap-path-prefix=<copy>=<repo>` | `807a28a2…` | 143,471 | **10 bytes differ** |
| C: like B, but the sources of `650f842` (the commit that last rebuilt `std.wasm`) | **`83cc5961…`** | 143,471 | **identical** |

Where B differs: the 10 bytes are panic-location **line numbers** in `core/kernel/src/lib.rs`, each 9 or
10 lines higher at HEAD (for example line 111 here, 102 in the committed file). Commit `933f4de` added
10 lines (a doc comment about ABI-1, and one comment) to that file after `std.wasm` was last built.

Host-specific strings in `std.wasm` (from `strings`):

| String | Where from | Same on every host? |
|---|---|---|
| `<repo>/core/kernel/src/lib.rs` | `m2-kernel` is a path dependency outside the `kernels-src` workspace, so cargo passes its absolute path | **no**: the only host-specific string |
| `src/fx.rs` | the crate being built: relative | yes |
| `/rustc/b940084d7…/library/…` (8 paths) | the prebuilt std of rustc 1.99.0 | yes, for the same rustc |
| `/rust/deps/dlmalloc-0.2.14/src/dlmalloc.rs` | std's allocator, prebuilt | yes, for the same rustc |

`std/kernels-src/Cargo.lock` lists only `m2-kernel` and `m2-std-kernels`, so no `$CARGO_HOME` path can
get in.

**What it means**
- The wasm output doesn't depend on the host OS or CPU. Linux x86_64 rebuilt the Mac's exact bytes. The
  only leak is the absolute path of `core/kernel`.
- **Fix for OS-64:** `std/build.sh` passes
  `--config 'target.wasm32-unknown-unknown.rustflags=["--remap-path-prefix=<repo>=<fixed>"]'`.
  This was checked: cargo appends it to the `-zstack-size` flag from `.cargo/config.toml`, and build C
  proves the stack flag still applied. Use one fixed prefix (for example `/m2`). Pin rustc 1.99.0
  (`rust-toolchain.toml`).
- Any remap changes the bytes once (the path string and its layout). So do one deliberate rebuild,
  which also brings in HEAD's line numbers. The `sounds` branch rebuilds `std.wasm` anyway: do it there,
  or before, and record that kernel hashes change.
- A CI check can then be: rebuild, `cmp` with the committed file.

---

## 8. wasmtime's address-space reservations

| Setting | Value |
|---|---|
| `vm.max_map_count` | **1,048,576**, set by `/etc/sysctl.d/10-map-count.conf` (Ubuntu's `procps` package) |
| `vm.overcommit_memory` | 0 (heuristic); `overcommit_ratio` 50 |
| `CommitLimit` / `Committed_AS` | 66.5 GiB / 58 GiB (mode 0 doesn't enforce it) |
| `ulimit -v` (`RLIMIT_AS`) | unlimited |
| user address space | 48-bit virtual (`address sizes: 48 bits virtual`): 128 TiB |
| transparent huge pages | `madvise` |

wasmtime 49.0.2's x86_64 defaults (from its source, `wasmtime-environ` `tunables.rs` and
`runtime/vm/sys/unix/mmap.rs`): 4 GiB reservation, 32 MiB guard, 2 GiB extra for growth; reservations
use `MAP_NORESERVE` on Linux.

**Probe:** a C program that makes wasmtime-shaped reservations until `mmap` fails. Each is 32 MiB guard
+ 4 GiB + 32 MiB guard, `PROT_NONE`, `MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE`; 64 KiB of it is made
read-write and touched.

| Point | Reservations | Mappings | Reserved | Time |
|---|---|---|---|---|
| step 7's typical renderer | 6,058 | 12,140 | 24,611 GiB (matches step 7's number) | 0.08 s |
| `mmap` fails (`ENOMEM`) | **32,261** | 67,000 | **128.0 TiB** | 0.37 s |
| the same with `ulimit -v 16000000` (16 GB) | **3** | 30 | — | — |

**What it means**
- On this machine the address space is the limit: about 5.3 typical renderers in one process. The map
  count (1,048,576) is far away.
- At the address-space limit the process held 67,000 mappings. That is just above the upstream default
  of 65,530. On a distro that keeps that default, the map count stops it at about the same point, so the
  default is only barely enough.
- Any `ulimit -v` stops the reservations almost at once. `vm.overcommit_memory = 2` most likely does
  *not*: Linux doesn't charge `PROT_NONE` private mappings, only the pages made writable *(unverified
  here: changing the sysctl is a system change)*. The survey's "breaks them outright" claim is likely
  too strong.
- The bounded-reservation or pooling option from step 7 still matters for 39-bit arm64 kernels
  (512 GiB), which this machine can't show.

---

## 9. What the desktop app links on Linux

From `readelf -d target/debug/music2-desktop` (built by the lead at 19:53; read-only), with each library
mapped to its Ubuntu 24.04 package by `dpkg -S`:

| Library (`NEEDED`) | Ubuntu 24.04 package |
|---|---|
| `libwebkit2gtk-4.1.so.0` | `libwebkit2gtk-4.1-0` (2.52.6) |
| `libjavascriptcoregtk-4.1.so.0` | `libjavascriptcoregtk-4.1-0` |
| `libgtk-3.so.0`, `libgdk-3.so.0` | `libgtk-3-0t64` (3.24.41) |
| `libsoup-3.0.so.0` | `libsoup-3.0-0` (3.4.4) |
| `libglib-2.0.so.0`, `libgobject-2.0.so.0`, `libgio-2.0.so.0` | `libglib2.0-0t64` |
| `libcairo.so.2` | `libcairo2` |
| `libgdk_pixbuf-2.0.so.0` | `libgdk-pixbuf-2.0-0` |
| `libdbus-1.so.3` | `libdbus-1-3` |
| `libasound.so.2` | `libasound2t64` |
| `libgcc_s.so.1` | `libgcc-s1` |
| `libc.so.6`, `libm.so.6` | `libc6`: needs **GLIBC_2.39** |

130 libraries in all once their own dependencies are loaded (`ldd`); none missing.

| Binary | Needs |
|---|---|
| `music2-desktop` | the table above; glibc ≥ 2.39 |
| `m2` (release, `f126d03`) | only `libc`, `libm`, `libgcc_s`; glibc ≥ 2.35. **No `libasound`**: the linker dropped it, since `m2` never opens a device. Building it still needs `libasound2-dev`. |
| `examples/headless` | `libasound.so.2`, `libc`, `libm`, `libgcc_s` |

Other run-time facts:
- No tray icon is built in (Tauri without the tray feature; no `appindicator` name in the binary). So
  `libayatana-appindicator3` is not needed at run time.
- "Reveal in folder" (`open` 5.4.4) tries `xdg-open`, then `gio open`, then `gnome-open`, then
  `kde-open`. **`xdg-open` is not installed here**; `gio` is, so the second try would work.
- No display here (`DISPLAY` unset), but `Xvfb` and `xvfb-run` are installed, with fontconfig, DejaVu and
  Liberation fonts (24 fonts). The desktop tests could run under `xvfb-run`.
- Build-time packages already present: `libwebkit2gtk-4.1-dev`, `libjavascriptcoregtk-4.1-dev`,
  `libgtk-3-dev`, `libsoup-3.0-dev`, `librsvg2-dev`, `libayatana-appindicator3-dev`, `libdbus-1-dev`,
  `libasound2-dev`, `libxdo-dev`, `libssl-dev`, `pkg-config`, `build-essential`, clang 18.
- Chrome for the browser suites: `navigator.platform` is **`Linux x86_64`** in the pinned
  chrome-headless-shell 133 here. That is why `isMac` is false in the suites (OS-74).

**What it means for a Linux user**
- A `.deb` for Ubuntu 24.04 needs: `libwebkit2gtk-4.1-0`, `libgtk-3-0t64`, `libsoup-3.0-0`,
  `libasound2t64`, and the GLib, cairo, gdk-pixbuf and D-Bus libraries that those pull in anyway.
  Recommend `xdg-utils`.
- Built here, the app needs glibc 2.39, so it won't start on Ubuntu 22.04 or Debian 12. Wider reach needs
  an older build base or an AppImage *(packaging choice; not tested)*.

---

## Survey claims checked

| Survey claim | Result here |
|---|---|
| ext4 supports `RENAME_EXCHANGE` and `RENAME_NOREPLACE` *(verify)* | **confirmed** (kernel 6.8) |
| tmpfs supports both *(verify version)* | **confirmed** on 6.8; the version it began is not checked |
| An unsupported flag gives `EINVAL` | confirmed for an unknown flag and a bad mix; a filesystem without support is *(unverified)* |
| ext4 can't reflink; `FICLONE` falls back | **confirmed**: `EOPNOTSUPP` |
| `fsync` on an `O_RDONLY` folder fd works on ext4 | **confirmed** (also tmpfs) |
| `syncfs` waits for every writer | **confirmed**: up to 890 ms while others wrote |
| inotify event queued before the syscall returns *(verify)* | **confirmed**: 0 misses in 2.8 M checks |
| `max_user_watches` RAM-scaled *(verify version 5.11)* | the scaling is **confirmed** (997,324); the version isn't checked |
| `max_user_instances` 128, `max_queued_events` 16,384 | **confirmed** |
| `IN_Q_OVERFLOW` wd −1 after a drop | **confirmed** |
| A rename from outside has no `MOVED_FROM` | **confirmed** |
| `flock` per open file description, freed at exit | **confirmed**, also at `SIGKILL` |
| glibc picks FMA variants by CPU, so x86 machines can differ | **half**: `cosf`, `sinf`, `powf`, `exp2`, `pow` don't change; f64 `sin`/`cos` do |
| `examples/resample` renders differently on Linux | **confirmed**, and it is the only one of 8 |
| `libm` is the same code as `std.wasm`'s math | **confirmed** bit for bit (exhaustive f32, 67 M f64 each) |
| `std.wasm` embeds the builder's path; remapping fixes it | **confirmed**: identical bytes with the Mac path remapped |
| `vm.max_map_count` raised on Ubuntu 24.04 | **confirmed**: 1,048,576 from `procps` |
| `ulimit -v` breaks the 4 GiB reservations | **confirmed** |
| `overcommit_memory = 2` breaks them | **likely wrong** *(unverified here)* |
| cpal's Linux default host without features is ALSA | **confirmed** |
| "No device" gives none, or a `default` PCM that fails | **the second**: a device exists, its config fails |
| alsa-lib prints `cannot find card '0'` | **confirmed** (8 lines) |
| `sched_util_clamp_min_rt_default` = 1024 *(unverified)* | **confirmed** |
| libloading opens with `RTLD_LAZY \| RTLD_LOCAL` | **confirmed** (libloading 0.8.9 source) |
| libloading's Windows flags *(verify)* | from source: `Library::new` is `LoadLibraryExW(…, 0)`, **without** `LOAD_WITH_ALTERED_SEARCH_PATH`. A plugin's own DLLs beside it then need `load_with_flags`. |
| core dumps from the kill tests (OS-72) | apport is the core handler and is active, but `RLIMIT_CORE` is 0 (soft) in the tmux session, and `/var/crash` stayed empty after the full test run |
| WebKitGTK needs `WEBKIT_DISABLE_DMABUF_RENDERER` on some GPUs *(verify)* | not checkable: no GPU or display here |
| Surge XT without `timer-support` *(verify)* | not checkable: no Linux CLAP plugins installed |

---

## Rerun notes

All probes were deleted after the run (scratch rule). To rebuild one, write it again from this list.
Use a scratch `HOME`, `M2_AUDIO=none` and an own `CARGO_TARGET_DIR`. Run long ones through `$SCRATCH/tw`,
and put commands that use quotes or brackets in a script file: `tw` writes the command into an `echo`
line, and a quote in it breaks the job silently.

| Item | Probe |
|---|---|
| 1 | C: `syscall(SYS_renameat2, AT_FDCWD, a, AT_FDCWD, b, flags)`, `ioctl(t, FICLONE, s)`, `copy_file_range`, `link`, `fsync`/`fdatasync`/`syncfs` on `open(dir, O_RDONLY\|O_DIRECTORY)` and `O_PATH`; print each errno. |
| 2 | C: `clock_gettime(CLOCK_MONOTONIC)` around each sync, 50 runs, sort, p50 = v[n/2], p95 = v[round(0.95(n−1))]. Busy writer: 256 × 4 MB `write` + `fsync`, looped. |
| 3 | C: `inotify_init1(IN_NONBLOCK)`; drain; do the op (or have a forked child do it and write a byte to a pipe); one `read`; check the mask. 100,000 rounds. |
| 4 | C: `flock(fd, LOCK_EX\|LOCK_NB)` on two `open`s, `dup`, a `fork`ed holder that exits or is `SIGKILL`ed. Rust: `File::try_lock` twice. |
| 5 | Rust crate, `cpal = "=0.18.2"`: `available_hosts`, `default_host`, `output_devices`, `default_output_device`, `default_output_config`, `build_output_stream::<f32>`. |
| 6 | Rust crate, `libm = "=0.2.16"` and `wasmtime = "=49.0.2"` (the workspace's `Cargo.lock` copied in). glibc through `unsafe extern "C" { fn cosf(x: f32) -> f32; … }`. A 32-thread sweep over f32 bit patterns. The wasm side: a `cdylib` with `hash_f32` and `hash_f64` exports (FNV-1a over result bits). Renders with `LD_PRELOAD` of a `cdylib` that exports `cosf`, `sinf`, `powf`, `exp2`, `pow`, `sin`, `cos` as `libm` calls. |
| 7 | `git archive 650f842 Cargo.toml core/kernel std \| tar -x -C <dir>`, then in `std/kernels-src`: `cargo build --release --target wasm32-unknown-unknown --config 'target.wasm32-unknown-unknown.rustflags=["--remap-path-prefix=<dir>=<repo>"]'`, then `cmp`. |
| 8 | C: loop `mmap(NULL, 32M+4G+32M, PROT_NONE, MAP_PRIVATE\|MAP_ANONYMOUS\|MAP_NORESERVE)` + `mprotect` 64 KiB read-write, count lines of `/proc/self/maps`. |
| 9 | `readelf -d`, `ldd`, `dpkg -S`, `objdump -T \| grep -o 'GLIBC_[0-9.]*' \| sort -uV \| tail -1`. |
