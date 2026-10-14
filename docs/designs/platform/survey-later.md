# Survey: the OS-specific pieces that steps 8a, 8b and 9 will add

For the platform layer that the owner approved right after step 7. This survey covers only what the
later steps **add**. The macOS-only code that exists today (`tx/stage.rs`, `lock.rs`, the watchers,
the plugin main thread, the app's menus, the gates' Mach measures, the CLAP search path, the paths,
the bundle) is in the companion surveys.

Read for this: `docs/architecture.md` (normative; Deferred, Order of work, PluginHost, Render),
`.git/m2-step8/design-8a.md` (rev 2), `design-8b.md` (rev 2), `understand-8a.md` §6–7, `taste.md`
§5, and these sources in `~/.cargo/registry` and the SDK: cpal 0.18.2 (the version in `Cargo.lock`
at HEAD), clap-sys 0.5.0, the macOS SDK headers `os/workgroup_object.h` and
`os/os_sync_wait_on_address.h`, Rust std's thread parker (1.99), and the current `core/play`,
`core/plugins`, `apps/music2-desktop` and `web/src/lib/native.ts`. Web facts are marked *(web)*.
Facts from memory that I could not check are marked *(unverified)*. Sizes are *estimates*. Nothing
was built or run, and no file in the working tree was touched.

---

## 0. The short version

**What the later steps add that differs by OS** (the full table is §1):

| Step | OS-specific piece |
|---|---|
| 8a | realtime engine threads; the device's scheduling group (the workgroup); the thread count and core topology; sleeping and waking; xruns and device changes; device buffer sizes; background threads for exports beside playback; the machine description in gate reports |
| 8b | menu chords and gesture modifiers; the DSP popover's device facts; a paced player for the suites; MIDI input if Q3 is yes (8b.10) |
| 9 | the plugin host process (spawn, shared memory, a cross-process wake, realtime in the child, crash detection); plugin windows (window API, GUI event loop, scale, focus); VST3 and AU; the latency the device reports; recording and input monitoring (input streams, clock domains, microphone permission); disk streaming (cache dir, mapped files, readahead, I/O priority); keeping the machine awake |

**Already the same on every OS, so not in the layer.** cpal 0.18.2 already gives one interface for:
- xruns, as `ErrorKind::Xrun` through the error callback, on CoreAudio, ALSA, PipeWire, JACK and
  WASAPI capture (CHANGELOG 0.18.2);
- device changes, as `DeviceChanged`, `StreamInvalidated` and `DeviceNotAvailable`;
- the stream's buffer size, as `StreamTrait::buffer_size()`;
- the stream clock, as `StreamTrait::now()`;
- output and input latency, as `playback − callback` and `callback − capture` in the callback
  timestamps. On CoreAudio these include the buffer, the device latency and the safety offset
  (`host/coreaudio/macos/device.rs:761-921, 1042-1050`).

Rust std already gives one clock (`Instant`), sub-millisecond `sleep` (also on Windows since 1.75 *(web)*)
and a realtime-safe thread parker. midir covers MIDI, and memmap2 covers mapped files. The layer
should hold only what these leave out.

**Findings that change a design, or are bugs today:**

1. **Any xrun makes the player look stalled (a bug at HEAD).** Since cpal 0.18.2 reports CoreAudio
   overloads as errors, the error callback in `Player::start` sets `device_error` for every one of
   them (`core/play/src/lib.rs:553` at HEAD). `stalled()` returns true on it (`:888`), and the
   server's `device()` then retires the player at the next action with "the audio device stopped
   responding" (`crates/server/src/graph.rs:431`). That cuts every tail. On ALSA the same callback
   runs **on the realtime thread**, and its `eprintln!` allocates and locks there.
   - The fix is OS-independent: sort cpal's errors by kind. Count an xrun and play on; treat a
     device change as a new player; only invalidated or unavailable means stalled.
   - That also makes design-8a's own `kAudioDeviceProcessorOverload` listener (§6.3 step 5)
     redundant, because cpal already registers it.
2. **On Linux, engine threads aren't realtime today, and cpal's switch for it is unsafe.** cpal is
   built with no features, and on Linux its `realtime` feature does nothing without `realtime-dbus`.
   With `realtime-dbus`, audio_thread_priority sets RLIMIT_RTTIME's *soft* limit to one buffer
   period (`budget_us = buffer_frames * 1_000_000 / rate`, *(web, source)*). At 128 frames that is
   2.67 ms of realtime CPU without blocking. Going over it sends SIGXCPU, whose default action
   kills the process, and a block that runs long (a dropout) does exactly that. 8a's device thread
   spins up to `stuck` (one period) and runs four blocks back to back at 512 frames. **So the layer
   must own Linux promotion**: set RLIMIT_RTTIME itself, at 200 ms for both limits, and ignore
   SIGXCPU (§2.4).
3. **SCHED_FIFO threads don't time-slice.** If two engine threads at equal FIFO priority share a
   core, the one spinning holds the core until its `patience` runs out (8a §5.5). macOS
   time-constraint threads are preemptible, so 8a never had to consider this. On Linux the thread
   count must respect the affinity mask and the cgroup quota, count physical cores, and yield in the
   spin (§2.4, §2.6).
4. **The workgroup has a Windows analogue and no Linux one.** Windows threads can share an MMCSS
   task: pass the same `TaskIndex` to `AvSetMmThreadCharacteristicsW` *(web, docs)*. Linux has no
   group, but workers should match the device thread's FIFO priority. So the trait's `Group` is an
   associated type: an `os_workgroup_t`, an MMCSS task index, or the device thread's priority (§2.5).
5. **8b's chords collide off macOS.**
   - ⌃S (Solo) becomes Ctrl+S, which is already Save, so it collides; ⌃M becomes Ctrl+M.
   - ⌥-drag (copy) is Alt-drag, which moves the window under several Linux window managers.
   - ⌘. (panic) becomes Ctrl+., which IBus takes for emoji input on GNOME *(unverified)*.
   - ⌥← and ⌥→ (markers) are back and forward in WebView2 *(unverified)*.
   - ⌥⌘B becomes Ctrl+Alt+B, which acts as AltGr on European Windows layouts.

   The fix is a key table rendered per OS, in which ⌃ chords exist only on macOS (§3.1). It is a UI
   piece, not part of the Rust trait.
6. **Plugin windows on Linux depend on the host's event loop.** Linux has no shared main run loop,
   so the host must provide `clap.posix-fd-support` and `clap.timer-support` for plugin GUIs. They
   embed by X11 only: CLAP says Wayland embedding "is currently not supported, use floating windows"
   (clap `gui.h`). The host process also needs a cross-process wake for each OS:
   - Linux: a futex in shared memory;
   - macOS: a Mach semaphore passed by port, because `os_sync_wait_on_address`'s shared flag needs
     macOS 14.4 and the app supports 11.0;
   - Windows: an Event handle, because `WaitOnAddress` works only inside one process.

   macOS can hand its workgroup to the child with `os_workgroup_copy_port` (§4.1).
7. **Recording needs a microphone permission on macOS.** That means `NSMicrophoneUsageDescription`
   in Info.plist, which today's bundle doesn't have (`tauri.conf.json`), and an entitlement once the
   app is hardened. Windows has a privacy switch for desktop apps. Each OS also needs its own answer
   for input and output on different clocks (§4.6).
8. **Disk streaming differs in four places.** The cache directory differs on each OS. Windows can't
   delete or rename a mapped file. Readahead and I/O priority calls differ. And the engine must
   never `mlockall`: with wasmtime's 4 GiB reservations per memory, `MCL_FUTURE` would try to lock
   them all (§4.7).
9. **wasmtime handles traps differently on each OS.** Its macOS default is Mach exception ports, so
   a plugin's own signal handlers can't take wasm traps. On Linux it uses signals, and an in-process
   plugin that installs a SIGSEGV handler without chaining would turn a wasm out-of-bounds trap into
   a crash *(unverified)*. That adds a Linux reason for the host process.

**The trait, in one breath.** It is one crate, `m2-os` (`core/os`), with one trait `Os` and one
implementation per OS, chosen by `cfg`:

```rust
pub type This = mac::Mac;   // or linux::Linux, windows::Windows
```

It has sections for engine threads (8a), the host process (9), plugin windows (9) and services (9).
Each later step implements its own section for macOS and Linux in the same slice. Windows compiles
from day one and returns a typed `Unsupported` until its own step; Linux CI checks it with
`cargo check --target x86_64-pc-windows-msvc -p m2-os`. Nothing in the trait wraps cpal, std, midir
or memmap2. §5 has the sketch.

---

## 1. Inventory

"Covered by" names what makes the piece the same on every OS: **cpal**, **std**, a **crate**, the
**trait** (`m2-os`), or **UI** (the key table in `menu.rs` and `web/src/lib/native.ts`). "Before"
says whether the platform-layer step must already decide its shape, so that the later step writes
it once for every OS.

| # | Piece (where it is designed) | macOS (as designed) | Linux | Windows | Covered by | Before |
|---|---|---|---|---|---|---|
| 1 | Device thread realtime (8a §6.2) | CoreAudio's IO thread is already realtime and in the device's workgroup | ALSA, PipeWire: cpal's own thread, **not** realtime without our help; JACK: libjack's thread, already realtime | WASAPI: cpal's thread, MMCSS only with cpal's `realtime` feature; ASIO: the driver's thread | trait (`on_device_thread`) | yes |
| 2 | Xruns and overloads (8a §6.3.5, §9.1) | cpal: `kAudioDeviceProcessorOverload` listener → `Xrun` | cpal: ALSA `EPIPE`, PipeWire io position, JACK xrun callback → `Xrun` | cpal: capture only; render glitches go unreported, so 8a's `late` and `gaps` counts are the measure | cpal, plus 8a's own counts | yes (bug 1) |
| 3 | Device changes (8a §6.3, end) | cpal reroutes default-output streams and reports `DeviceChanged` | PipeWire reports `DeviceChanged`; ALSA `DeviceNotAvailable` | `StreamInvalidated` and `DeviceNotAvailable` | cpal | yes |
| 4 | Buffer size (8a §6.1) | `BufferSize::Fixed` → `kAudioDevicePropertyBufferFrameSize`, range-checked | ALSA: a period-size request, rounded by the hardware; PipeWire: a `node.latency` request into a quantum shared by every client, which can vary per cycle; JACK: the server's, fixed | WASAPI shared: about 10 ms, whatever is asked; IAudioClient3 or exclusive mode for less (not in cpal); ASIO: the driver's | cpal, plus the engine's existing any-size path | no |
| 5 | Realtime workers (8a §6.3) | `THREAD_TIME_CONSTRAINT_POLICY`, period and computation | `SCHED_FIFO` directly when RLIMIT_RTPRIO allows, else rtkit `MakeThreadRealtime`; RLIMIT_RTTIME set by us | MMCSS "Pro Audio" plus `AvSetMmThreadPriority` | trait (`engine_thread`) | yes |
| 6 | The group (8a §6.3) | `os_workgroup_join` on the device's `'oswg'` | none; match the device thread's FIFO priority | a shared MMCSS `TaskIndex` | trait (`type Group`) | yes |
| 7 | Thread count (8a §6.3.4, §7) | `os_workgroup_max_parallel_threads` ∩ `hw.perflevel0.cpusperl2` | sysfs caches, SMT siblings and core types, ∩ affinity ∩ cgroup quota | `GetLogicalProcessorInformationEx` / CPU sets (`EfficiencyClass`, L2 masks) | trait (`cores`) | yes |
| 8 | Sleep and wake (8a §5.1 `Sem`) | Mach semaphore | futex | `WaitOnAddress` | trait (`type Wake`) | yes |
| 9 | Clock (8a §9.1 `now_ns`) | `Instant` (CLOCK_UPTIME_RAW, which is Mach time) | `Instant` (vDSO CLOCK_MONOTONIC) | `Instant` (QPC) | std: nothing to do | — |
| 10 | Floating-point mode (8a §2.3) | FPCR | FPCR (aarch64), MXCSR (x86_64) | the same, by architecture | `m2-runtime/fpenv.rs`, per architecture | — |
| 11 | Background renders (8a §7) | `QOS_CLASS_UTILITY`, E-core count | nice 10 plus `SCHED_BATCH`; efficiency cores by affinity where present (`cpu_atom`, `cpu_capacity`) | EcoQoS (`ThreadPowerThrottling`) plus below-normal priority, or CPU sets with `EfficiencyClass` 0 | trait (`background_thread`) | yes |
| 12 | Machine in gate reports (8a §9.2) | `hw.model`, perf levels | DMI or device-tree model, `lscpu` facts | registry or WMI model | trait (`machine`) | no |
| 13 | VM lock on unmap (8a R13) | the VM map lock | `mmap_lock`; per-VMA locks since 6.4 let faults avoid it *(unverified)* | the address-space lock *(unverified)* | measure only | — |
| 14 | First thread-local touch on a worker (8a R10, R12) | mallocs outside Rust | glibc allocates a dlopen'd library's TLS on first access per thread (the same hazard) | `DLL_THREAD_ATTACH` at thread start, off the realtime path | measure only | — |
| 15 | Silent and paced players (8a §6.4, 8b §10.6) | `thread::sleep` | the same; timer slack 50 µs | the same (high-resolution since Rust 1.75) | std | — |
| 16 | Menu chords (8b §2.6, taste §5) | ⌘ and ⌃ | Ctrl and Alt, with collisions | Ctrl and Alt, with collisions | UI | yes (before 8b off macOS) |
| 17 | Gesture modifiers (8b §5.5, §6.7, §7) | ⌥-click reset, ⌥-drag copy, hold ⌘ to bypass snap, ⌘-click toggle | WMs take Alt-drag; use Ctrl-drag to copy, Alt to bypass snap (Live's PC keys) | the same as Linux | UI | yes |
| 18 | DSP popover and Settings (8b §10.4, gaps #31) | one host (CoreAudio) | a host to choose: PipeWire, PulseAudio, JACK or ALSA | WASAPI or ASIO | cpal (`available_hosts`) | no |
| 19 | MIDI input (8b.10 = Q3; step 9) | CoreMIDI | ALSA sequencer (also JACK, PipeWire) | WinMM; Windows MIDI Services (GA February 2026, multi-client) | midir, plus timestamp conversion in the trait | no |
| 20 | Plugin host process (HOST-3) | `posix_spawn`; shm; a Mach port channel; the workgroup by port; kqueue `NOTE_EXIT` | `posix_spawn`; `memfd` over SCM_RIGHTS; a shared futex; `pidfd`; `PR_SET_PDEATHSIG` | `CreateProcess`; a section handle; an Event; the process handle; a kill-on-close Job | trait (host section) | shape only |
| 21 | Plugin windows (HOST-3, HOST-7) | `cocoa` NSView in the child's own NSWindow; an accessory app | `x11` only; the child runs an X11 loop with `posix-fd-support` and `timer-support`; XWayland under Wayland | `win32` HWND; a message pump; per-monitor DPI and `set_scale` | trait (`WINDOW_API`, `GuiLoop`) | shape only |
| 22 | VST3 and AU (Deferred row) | VST3 bundle; AU only here | VST3 `.so` inside the bundle; the host's `IRunLoop` | VST3 `.vst3` DLL inside the bundle | per-OS paths in the trait; formats per OS | no |
| 23 | Device latency (RENDER-19, recording) | cpal timestamps: buffer, device latency and safety offset | cpal: ALSA `delay`, PipeWire time, JACK port latency | cpal: the QPC position | cpal | no |
| 24 | Recording and monitoring (Deferred row) | an input AUHAL; TCC microphone prompt; aggregate devices for two clocks | a PipeWire or JACK graph with one clock; ALSA `snd_pcm_link` | WASAPI capture, a separate clock per endpoint; ASIO duplex; privacy toggle | cpal, plus the trait (`input_access`) | no |
| 25 | Disk streaming (RENDER-30) | `~/Library/Caches/music2`, mmap, `F_RDADVISE`, `setiopolicy_np` | `$XDG_CACHE_HOME/music2`, mmap, `posix_fadvise` / `madvise`, `ioprio_set` | `%LOCALAPPDATA%\music2\cache`, file mapping (no delete while mapped), `PrefetchVirtualMemory`, I/O priority hints | memmap2, plus the trait (services) | no |
| 26 | Staying awake (not yet in any design) | `IOPMAssertion`; App Nap (`beginActivity`) | logind `Inhibit("sleep:idle")` | `SetThreadExecutionState`; power throttling opt-out | trait (`keep_awake`) | no |
| 27 | Mono, legato, glide; kernel ABI v1; the sampler; stretching; latency compensation itself | — | — | — | engine, the same on every OS | — |

---

## 2. Step 8a

### 2.1 The device thread

- **macOS.** CoreAudio makes its HAL IO thread time-constrained and joins it to the device's
  workgroup (understand-8a §6.1). 8a relies on this.
- **Linux.** It depends on the cpal host.
  - **ALSA:** cpal spawns its own worker thread (`host/alsa/mod.rs:1321`). It promotes the thread
    only under `feature = "realtime"` (`:973-981`), and that needs `realtime-dbus` on Linux
    (CHANGELOG 0.18.2: "`realtime` alone is a no-op without `realtime-dbus`").
  - **PipeWire:** the callback runs on cpal's own main-loop thread, not on PipeWire's data thread
    (`host/pipewire/stream.rs:788-791`), and cpal promotes it through a background `RtPromoter`
    using rtkit (`rt_promote.rs`).
  - **JACK:** the process callback runs on libjack's thread, which jackd makes realtime.
  - **Default host:** cpal picks PipeWire, then PulseAudio, then ALSA, among the features that are
    built (`platform/mod.rs:832-848`). m2 builds none of them, so today **ALSA is always the host**,
    reaching PipeWire through its ALSA plugin on modern desktops.
- **Windows.**
  - WASAPI: cpal's thread, promoted with `audio_thread_priority` under `realtime`, which uses MMCSS
    "Audio" and falls back to `THREAD_PRIORITY_TIME_CRITICAL` (`host/wasapi/stream.rs:715-729`). It
    is done at thread start, outside the callback.
  - ASIO (cpal feature `asio`): the driver's own thread, already high priority. The SDK has been
    dual-licensed, proprietary or GPLv3, since October 2025 *(web)*.

**What the trait needs.** One non-blocking call at the top of the device thread's first callback,
`Os::on_device_thread(&group)`:
- **macOS:** does nothing.
- **Linux:**
  - stores `gettid()` and the thread's current policy and priority (a syscall, once) into the group;
  - if the thread isn't realtime (ALSA, PipeWire), signals the layer's promoter thread, which calls
    `sched_setscheduler(tid, SCHED_FIFO, prio)` or rtkit for that TID. That is cpal's own
    `RtPromoter` pattern, because rtkit is a blocking D-Bus call with a 10 s timeout;
  - with JACK, it only records the priority, so workers can match it.
- **Windows:** enable cpal's `realtime` feature for the Windows target only, so cpal promotes the
  thread before the first callback. Workers form their own "Pro Audio" task (§2.5).

**Do not enable `realtime-dbus` on Linux** (finding 2). The `dbus` crate is already in the Linux
tree through `tao` (`Cargo.lock`), so the layer's rtkit call adds no new system library.

### 2.2 Device events: xruns, device changes, stalls

cpal 0.18.2 reports these through the error callback on every host (§0). Design-8a counts dropouts
three ways (§9.1): overloads, callbacks longer than their period, and gaps between callbacks.

- **Overloads** become the count of `ErrorKind::Xrun`. This holds on macOS (cpal registers
  `kAudioDeviceProcessorOverload`, `host/coreaudio/macos/mod.rs:100-118, 183-192`), on ALSA, on
  PipeWire (`stream.rs:290-302`) and on JACK. Windows render reports none, so there the `late` and
  `gaps` counts carry the measure.
- **Device changes:**
  - `DeviceChanged` on a rerouted default output (macOS, PipeWire) means a new player. The new
    device brings a new workgroup and a new period, so 8a's "a new player brings a new pool" still
    holds; it is triggered by the event, not by a stall.
  - `StreamInvalidated` and `DeviceNotAvailable` mean stalled.
- **The callback is not always on a quiet thread.** On ALSA cpal calls it from the realtime worker
  itself (`alsa/mod.rs:948-951`). CoreAudio's overload listener says "overload notifications fire on
  the RT thread" (`macos/mod.rs:183`). Either way, it runs behind cpal's `try_lock`
  (`host/error_emit.rs:25-39`), so an error that meets contention is dropped. So the callback must
  only bump per-kind atomics: never format, never print. The m2-play thread can log the counts
  later.

This is all `core/play`, not `m2-os`, because cpal already speaks one language. It fixes bug 1 on
every OS at once, and deletes 8a's five lines of listener code and its `objc2-core-audio` use for
that purpose. The workgroup still needs it.

### 2.3 Device buffer size

`Options.frames` → `BufferSize::Fixed(n)` means different things per host:
- **CoreAudio:** sets the IO buffer after a range check (`device.rs:1009-1036`). 8a asks for 128 and
  64.
- **ALSA:** a period-size request that the hardware may round (`alsa/mod.rs:50-93`). cpal sets no
  period count under `Default`, because PipeWire's ALSA plugin misbehaves with two periods.
- **PipeWire:** sets `node.latency = n/rate` (`pipewire/device.rs:178-180`). It is a *request*: the
  graph runs at the smallest request from all clients, within `clock.min-quantum` and
  `clock.max-quantum`. The defaults are 32 to 2048, with 1024 by default *(unverified)*. Callbacks
  can vary in size from cycle to cycle ("quantum jitter", CHANGELOG 0.18.2), and
  `StreamTrait::buffer_size()` reports the last quantum (`stream.rs:158`).
- **JACK:** the server's size; the reported range is min = max = current (`jack/device.rs:56-58`).
- **WASAPI shared:** only the buffer duration is requested; the engine period stays at the default
  of about 10 ms. Lower needs `IAudioClient3::InitializeSharedAudioStream` or exclusive mode, which
  cpal doesn't use (it is always `AUDCLNT_SHAREMODE_SHARED`, `wasapi/device.rs:867, 968`), or ASIO.

**Consequences:**
- The engine already accepts any callback size (RENDER-33, and 8a §6.2's 64-frame grid). It must
  also accept a size that changes between callbacks on PipeWire. 8a's `period` for the policy and
  for `patience` and `stuck` should come from `stream.buffer_size()` at each callback, not from
  the request.
- The DSP popover and the future Settings window (gaps #31) should show the *actual* size, and say
  "requested 128, running 256" where they differ.
- On Windows, small buffers mean ASIO, or a cpal patch for IAudioClient3. That decision belongs to
  the Windows step.

### 2.4 Realtime workers

8a's start hook (§6.3) becomes `Os::engine_thread(period, &group)`, called on each worker before its
first job. It is allowed to block there.

| | macOS (8a as designed) | Linux | Windows |
|---|---|---|---|
| Policy | `thread_policy_set(THREAD_TIME_CONSTRAINT_POLICY)`: period, half of it as computation, the period as constraint, preemptible | `SCHED_FIFO` at the device thread's priority (read at its first callback). The default is 10, which cpal and Firefox use. Try `sched_setscheduler` first; it works when RLIMIT_RTPRIO allows (the `audio` or `realtime` group with PAM limits, systemd `LimitRTPRIO`, or `CAP_SYS_NICE`). Else rtkit `MakeThreadRealtime(tid, prio)` on the system bus | `AvSetMmThreadCharacteristicsW(L"Pro Audio", &task)` with one shared `task` index, then `AvSetMmThreadPriority(AVRT_PRIORITY_HIGH)`; revert with `AvRevertMmThreadCharacteristics` |
| Grant limits | none; demotion if a thread overruns its constraint repeatedly | rtkit by default: priority at most 20; RLIMIT_RTTIME hard limit at most 200 ms (it refuses unless the limit is set and below that); 25 grants per 20 s burst; a canary that demotes every realtime thread if the system starves, then refuses for 5 minutes (`rtkit-daemon.c` 0.14 *(web)*) | MMCSS gives realtime-range priority up to `SystemResponsiveness` (by default 20% of the CPU is kept for others) *(unverified)* |
| Runaway guard | time-constraint demotion | RLIMIT_RTTIME: SIGXCPU at the soft limit, SIGKILL of **the whole process** at the hard limit, counted per thread since its last blocking call | MMCSS throttling |
| Failure | `Refused`; the pool runs, and G12 doesn't pass | `Refused` (no rtkit, a container, the burst limit hit); the pool runs at normal priority, reported | `Refused` (MMCSS service off) |

**Linux rules the layer must keep** (findings 2 and 3):
1. **RLIMIT_RTTIME is set once, at player start, before any grant.** Both limits are 200 ms (rtkit's
   maximum), and SIGXCPU is set to `SIG_IGN`. The hard limit then still kills a thread that spins
   200 ms without blocking, a plugin stuck in a loop on an engine thread for example. In-process
   that takes music2 down, which is one more reason for the host process (§4.1). 8a's bounded waits
   (`patience` half a period, `stuck` one period, then park) keep every engine thread far below it.
2. **The rtkit burst limit.** A player has the device thread and three workers, so four grants. 8a
   §6.5 closes and reopens a player on every device change, so 25 grants per 20 s allows about six
   players per 20 s. Plugging headphones in and out quickly could hit it; the refusal is reported,
   never fatal.
3. **FIFO co-location.** Never wake more engine threads than there are physical cores in the
   process's affinity mask, after the cgroup quota (§2.6). In the spin loops of `help`, call
   `sched_yield()` every few hundred iterations. On a core shared with another engine thread at the
   same priority, that hands the core over; otherwise it costs about a microsecond.
4. **Containers.** Realtime inside Docker needs `--cap-add SYS_NICE --ulimit rtprio=95`. CI never
   needs realtime: the tests run on plain threads and assert that a refusal is reported, not
   failed.

**Windows notes for later.** MMCSS has to be set by the thread itself, so there is no promoting by
thread id. The device thread's MMCSS index under cpal's `realtime` is unknown to us, so workers form
their own task. Whether joining the device thread's task matters is a measurement for the Windows
step.

### 2.5 The group

8a joins workers to the device's `os_workgroup`, so the scheduler and the frequency governor treat
the device thread and the workers as one job. The other OSes:
- **Linux:** no equivalent. schedutil already runs the CPU at full frequency for realtime tasks
  (`sched_util_clamp_min_rt_default` = 1024 *(unverified)*). What workers must share with the device
  thread is its **priority**, so the group is that priority, learned at the first callback.
- **Windows:** threads that pass the same MMCSS `TaskIndex` are one task *(web, docs)*. The group is
  the index.

So `type Group` in the trait is `os_workgroup_t` (released with `os_release`), a FIFO priority, or a
`TaskIndex`. On macOS `group()` reads `'oswg'` when the stream opens, through the UID translation 8a
designed. On Linux and Windows it is filled at the first callback.

### 2.6 The thread count and the core topology

8a uses `min(os_workgroup_max_parallel_threads, hw.perflevel0.cpusperl2)`: the fast cores that share
one L2, the device thread included. The trait's `cores(&group) -> Cores { engine, fast, efficient }`:
- **macOS:** as designed.
- **Linux:** start from `std::thread::available_parallelism()`, which already respects
  `sched_getaffinity` and cgroup quotas, then from sysfs:
  - physical cores only: one per `topology/thread_siblings_list`, because two engine threads on SMT
    siblings slow each other;
  - the L2 cluster: `cache/index*/level == 2`, then `shared_cpu_list`;
  - fast or efficient: `cpu_capacity` on ARM big.LITTLE; `/sys/devices/cpu_core/cpus` and
    `/sys/devices/cpu_atom/cpus` on Intel hybrid.

  In a Docker Desktop VM the vCPUs have no meaningful topology, so treat them as equal. Leave one
  core for the sound server and interrupts when the mask is small (*estimate*, to measure).
- **Windows:** `GetLogicalProcessorInformationEx`, which gives `RelationProcessorCore` (SMT flag,
  `EfficiencyClass`) and `RelationCache` level 2 group masks, or `GetSystemCpuSetInformation`.
  Processor groups only matter above 64 logical CPUs.

8a's default for exports beside playback ("as many as there are E-cores") becomes `cores().efficient`,
or half of `fast` where a machine has no efficient cores (*estimate*).

### 2.7 Sleeping and waking

8a's `Sem` (§5.1: "a Mach semaphore on macOS, a thread park elsewhere") becomes `Os::Wake`:
- **macOS:** a Mach semaphore (`semaphore_signal` / `semaphore_timedwait`), as 8a designs it.
- **Linux:** a futex on an `AtomicU32` (`FUTEX_WAIT` and `FUTEX_WAKE` with the private flag;
  `rustix` 1.1.5 is in the tree).
- **Windows:** `WaitOnAddress` / `WakeByAddressSingle`.

Why not `std::thread::park`, which is realtime-safe on all three (std 1.99: futex on Linux,
`WaitOnAddress` on Windows, a dispatch semaphore on Darwin)? Because `park()` calls
`thread::current()`, and on a thread std didn't spawn (cpal's device thread) the first call
allocates the `Thread` handle. That would happen in a callback the first time the device thread
parks on a stuck task. A `Wake` that isn't tied to thread identity avoids it, and costs about 30
lines per OS (*estimate*). The cross-process cousin in §4.1 uses the same futex code on Linux,
without the private flag.

### 2.8 Nothing to do: the clock and the floating-point mode

- `now_ns()` is already `Instant`-based (`core/play/src/lib.rs:52-55`). 8a's "`mach_absolute_time`
  underneath" holds on macOS, and the same code reads vDSO `CLOCK_MONOTONIC` on Linux and QPC on
  Windows with no system call.
- `fpenv::canon()` is per architecture: FPCR on aarch64 (macOS, Linux aarch64, Windows on ARM),
  MXCSR = 0x1F80 on x86_64. Nothing is per OS. On x86_64, the `_mm_setcsr` intrinsic is deprecated
  in Rust, so use a two-instruction `asm!`.

### 2.9 Background threads

`Os::background_thread()`, called at the start of each thread of a render beside playback (8a §7):
- **macOS:** `pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0)`.
- **Linux:** `setpriority(PRIO_PROCESS, tid, 10)` and `SCHED_BATCH`. On hybrid or big.LITTLE
  machines, an affinity mask of the efficiency cores. Optionally `sched_setattr` with a low
  `uclamp.max`, which steers the thread to little cores under energy-aware scheduling.
- **Windows:** `SetThreadInformation(ThreadPowerThrottling, EXECUTION_SPEED)` (EcoQoS, which steers
  the thread to E-cores on hybrid machines) and `THREAD_PRIORITY_BELOW_NORMAL`.

### 2.10 Gates

- G12's verdict stays macOS only: base M1, 128 frames.
- On Linux, `m2-gates realtime --offline` (the sweeps, partition profiles) and the 1-vs-N tests run
  as they are.
- A Linux run on a device is *recorded*. It prints `Os::machine()` (model from
  `/sys/class/dmi/id/product_name` or `/proc/device-tree/model`, the core types, the L2 clusters,
  the kernel's preemption model) and the realtime result (granted, by rtkit or directly, or
  refused).
- `--like-m1` stays macOS only.

### 2.11 Memory hazards that differ by OS

- **R13, the unmap lock.** Linux takes `mmap_lock` for writing during `munmap`. Since 6.4, page
  faults mostly use per-VMA locks, so a faulting engine thread waits less than on macOS
  *(unverified)*. The mitigations are the same everywhere: prewarm, pace the dropper.
- **wasmtime's reservations** (RENDER-32: one renderer reserves 24,611 GiB, two reserve 49,637 GiB):
  - Linux x86_64 and Ubuntu's arm64 kernels have a 47–48-bit user space (about 128 TiB), like
    macOS's 131,068 GiB.
  - An arm64 kernel built with 39-bit VA (some Raspberry Pi and Android kernels) has 512 GiB and
    cannot hold even one p50 renderer.
  - A `ulimit -v` (RLIMIT_AS) in a CI runner fails the same way.
  - The layer's `machine()` should report VA bits, and the store-layout question (owner decision,
    RENDER-32) gets one more input.
  - Windows x64 user space is 128 TiB.
- **No `mlockall`.** Linux pro-audio advice often says to `mlockall(MCL_CURRENT | MCL_FUTURE)`.
  With wasmtime's PROT_NONE reservations and per-instance memories, that is wrong. Prefault the
  engine's buffers instead, as the arena already sizes them off the audio thread.
- **R12, first-touch thread-locals in plugins.** glibc allocates a dlopen'd library's TLS block on
  its first access per thread (`__tls_get_addr`), so 8a's probe and its measurement apply on Linux
  as on macOS. Windows runs each DLL's `DLL_THREAD_ATTACH` when a thread starts, which is off the
  realtime path.
- **Stack sizes.** Rust spawns 2 MiB stacks on every OS. The main thread has 8 MiB on macOS and
  Linux, but **1 MiB on Windows**, and the plugin main thread is the real main thread (HOST-2). The
  Windows build should link with `/STACK:8388608`.

### 2.12 The silent and paced players

`Player::silent` (`core/play/src/lib.rs:582` at HEAD) and 8b's `M2_AUDIO=paced` are plain threads with
`thread::sleep`. That is accurate on Linux (50 µs default timer slack) and on Windows 10 1803 or
later (a high-resolution waitable timer since Rust 1.75 *(web)*). No layer code is needed.

For a **device path in Linux CI**, which the browser suites don't need and later steps' smoke tests
do:
- a PipeWire daemon with a null sink, driven by its dummy driver at real time;
- `jackd -d dummy -r 48000 -p 128`;
- on the owner's real headless server, `modprobe snd-aloop` (or `snd-dummy`), which gives real ALSA
  devices clocked by a kernel timer.

ALSA's userspace `null` PCM doesn't pace, so it can't stand in.

---

## 3. Step 8b

### 3.1 Chords and gesture modifiers

`menu.rs` already uses Tauri's `CmdOrCtrl` (`apps/music2-desktop/src/menu.rs:36-64`). The web
already has `isMac` and `kbd()` (`web/src/lib/native.ts:16-19`), and accepts `metaKey || ctrlKey`
(`App.tsx:47`, `NoteView.tsx:346`). 8b adds chords and modifier gestures that don't survive a
plain ⌘ → Ctrl substitution:

| 8b / taste §5 | Plain substitution | Problem | Proposal for Linux and Windows |
|---|---|---|---|
| Mute ⌃M, Solo ⌃S ("also, for Logic hands") | Ctrl+M, Ctrl+S | Ctrl+S is Save | **⌃ chords exist on macOS only**; M and S remain |
| Panic ⌘. | Ctrl+. | IBus's default emoji hotkey on GNOME is Ctrl+. *(unverified)*, and it also eats Monaco's quick fix | keep Ctrl+.; the shortcut sheet names the IBus setting; panic is also in the Transport menu |
| Markers ⌥← ⌥→ | Alt+← / Alt+→ | back and forward in WebView2 unless browser accelerator keys are off *(unverified)* | turn them off in the Windows webview, or use Ctrl+Alt+← / → |
| Browser ⌥⌘B | Ctrl+Alt+B | Ctrl+Alt is AltGr on many European Windows layouts | Ctrl+Shift+B |
| Dock views ⌥1–⌥4, sheet ⌥K | Alt+1–4, Alt+K | Alt-letter can open menu mnemonics on Windows; Alt+1–4 are fine | keep; give menus no mnemonic on K |
| Delete track ⌘⌫ | Ctrl+Backspace | in text fields it deletes a word (taste already routes it by focus) | keep, with the focus routing |
| ⌥-drag copies a clip | Alt-drag | moves the window under Xfce, MATE, Cinnamon and others *(unverified per WM)* | **Ctrl-drag copies** (Windows convention; Live's PC key *(unverified)*) |
| hold ⌘ to bypass snap | hold Ctrl | Ctrl is now copy | **hold Alt** (Live: "Alt (Win) / Cmd (Mac)" *(web, forum quoting the manual)*) |
| ⌥-click resets a knob or fader; ⌥-click S solos exclusively | Alt-click | the same Alt capture by WMs | **Ctrl-click** (click versus drag keeps it apart from Ctrl-drag) |
| ⌘-click toggles a clip in the selection | Ctrl-click | — | keep |
| Settings ⌘, (taste) | Ctrl+, | lives in the app menu on macOS; Edit ▸ Preferences on Linux; File or Tools on Windows | per-OS menu placement, like Quit (File ▸ Quit, File ▸ Exit) |

**Design.**
- One table of chords and modifiers, keyed by command id, with a macOS form and a PC form. It is
  read by `menu.rs` (accelerator strings) and by `native.ts` (`isCopyDrag(e)`, `isSnapBypass(e)`,
  `isReset(e)`, and labels for tooltips and the ⌥K sheet).
- This isn't Rust platform code, so it stays out of `m2-os`. It has to exist before 8b ships off
  macOS, and it is cheapest to land in taste's slice S, where the chord rules are first applied.
- **The browser suites will run on Linux in chrome-headless-shell,** so the page sees "Linux" and
  takes the PC forms. The suites that press `Meta` should press the platform's modifier, or the
  page should take a `?platform=mac` override for suites that check macOS labels.
- **Fonts.** `system-ui` resolves to SF Pro on macOS (taste §2.1). In a container it resolves to
  whatever fontconfig has, possibly nothing. Install one fixed font package in the CI image, so
  layout-dependent checks are reproducible.

### 3.2 The DSP popover and the future Settings window

The popover (8b §10.4) shows the device, its rate and its buffer size: names from
`DeviceTrait::description()` and the size from `stream.buffer_size()` (§2.3). The Settings window
(gaps #31) gains a **host** choice off macOS: `cpal::available_hosts()` gives PipeWire, PulseAudio,
JACK and ALSA on Linux (as features are built), and WASAPI and ASIO on Windows. macOS has only
CoreAudio. Nothing here is in the trait.

### 3.3 Audition's sources

8b.6's source model (a window today, a device later, §8.1) is OS-independent. The MIDI device source
is §4.5.

---

## 4. Step 9

### 4.1 The plugin host process (HOST-3)

HOST-3 says "shared-memory audio and event rings, message-based main-thread calls and its own AppKit
main thread for windows; when it crashes, the affected instances become silence with
`plugin.crashed`". Each part, per OS:

| Part | macOS | Linux | Windows |
|---|---|---|---|
| Spawn | `posix_spawn` of a helper in the bundle. A helper app with `LSUIElement` (an accessory, so its windows show without a Dock icon). With the hardened runtime, the helper needs `com.apple.security.cs.disable-library-validation` to load other teams' plugins; keep that entitlement off the main app | `posix_spawn` of a binary next to ours | `CreateProcessW` of an `.exe` next to ours |
| Bundling | Tauri `externalBin` (sidecar, by target triple) on all three | the same | the same |
| Shared memory | `shm_open` (names at most 31 characters) or `mach_make_memory_entry` sent by port | `memfd_create`, the fd passed over a Unix socket (`SCM_RIGHTS`); anonymous, so nothing to clean up | `CreateFileMappingW` with no name, the handle inherited or duplicated into the child |
| Per-block wake, in both directions | a Mach semaphore whose send right goes to the child over the port channel (how JACK2 does it on macOS *(unverified)*); `os_sync_wait_on_address` with `OS_SYNC_WAIT_ON_ADDRESS_SHARED` needs macOS 14.4 (SDK header), and the app supports 11.0 | a futex in the shared memory, without `FUTEX_PRIVATE_FLAG`: the same code as `Wake` | a named or duplicated Event (`SetEvent` / `WaitForSingleObject`); `WaitOnAddress` works only inside one process |
| Realtime in the child | the child's audio thread joins the **parent's workgroup**: `os_workgroup_copy_port` in the parent, `os_workgroup_create_with_port` in the child (macOS 11, SDK header) | the child calls `engine_thread` itself (direct, or rtkit; rtkit checks that the thread belongs to the caller's process) at the parent's priority, passed in the handshake | the child sets MMCSS itself; a task index is per process *(unverified)* |
| Crash detection | kqueue `EVFILT_PROC` / `NOTE_EXIT`, plus the wake's timeout | `pidfd_open` + poll (5.3 and later), plus the wake's timeout | `WaitForSingleObject` on the process handle, plus the wake's timeout |
| The child dies with us | the child watches the parent with kqueue `NOTE_EXIT` on the parent's pid, or the channel's end-of-file | `prctl(PR_SET_PDEATHSIG, SIGKILL)` and a recheck of `getppid()` | a Job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` |
| Architecture bridging (a bonus) | x86_64-only plugins in an x86_64 child under Rosetta | — | x64 plugins in an x64 child on Windows on ARM |

**The engine side is the same everywhere.** A plugin node's `process` writes inputs into the ring,
wakes the child, and waits on the shared wake with a deadline. A missed deadline outputs silence for
that block, and a dead child gives `plugin.crashed`. So the trait needs `type Shared`,
`type SharedWake`, `type Child` and `spawn_host`. The handshake carries the shared memory, the wakes
and the group (macOS: the workgroup port; Linux: the priority; Windows: nothing). That is the one
place macOS needs a Mach port channel; `posix_spawnattr` special ports or a bootstrap check-in are
the two ways *(unverified which is simpler)*.

The host process on Linux also contains two in-process hazards: rtkit's 200 ms SIGKILL (§2.4) and
plugin signal handlers that don't chain to wasmtime's (finding 9).

Size *estimate*: about 300 to 500 lines per OS for the process kit.

### 4.2 Plugin windows

CLAP's window APIs (clap-sys `ext/gui.rs:8-11`; comments from clap `gui.h` *(web)*):

| | macOS | Linux | Windows |
|---|---|---|---|
| API | `cocoa`: an NSView, logical size; never call `set_scale` | `x11`: an X window id, physical size, embedded by XEmbed. `wayland`: "embed is currently not supported, use floating windows"; most Linux CLAP plugins support X11 only *(unverified)* | `win32`: an HWND, physical size, embedded with `SetParent` |
| The host process's window | an NSWindow per plugin; an accessory app | an X11 top-level per plugin (x11rb or xcb), running under XWayland in Wayland sessions; on a server with no display, windows refuse with words | a top-level HWND per plugin; per-monitor DPI v2 awareness |
| Scale | the OS does it | `set_scale` from Xft.dpi or the monitor scale | `set_scale` from `GetDpiForWindow`, again on `WM_DPICHANGED` |
| The child's main loop (HOST-2 inside the child) | `CFRunLoop` / the main dispatch queue, as `main_thread.rs` does now | **our own `poll`/`epoll` loop**: the X connection fd, the fds plugins register through `clap.posix-fd-support`, timers from `clap.timer-support` (a timerfd), and an eventfd for our jobs. JUCE's CLAP wrapper relies on these two extensions on Linux *(unverified)* | a Win32 message loop (`MsgWaitForMultipleObjectsEx` + `DispatchMessage`), woken by `PostThreadMessage` or our Event; plugin `SetTimer`s work inside it |
| Keeping it above the main window | can't parent across processes; use a floating level, raised when the app activates | `WM_TRANSIENT_FOR` the app's XID, which works across processes on X11. When Tauri's GTK window runs on Wayland it has no XID, so no transient (or run the app with `GDK_BACKEND=x11`) | owner across processes with `GWLP_HWNDPARENT`, which attaches input queues *(unverified)*; else topmost while the app is active |
| Keys while a plugin window has focus | the menu bar belongs to the active app, so with a plugin window focused the menu is the helper's. Forward ⌘ chords and Space to the app over the channel | forward unhandled keys | forward unhandled keys |

**The trait** gets `const WINDOW_API: &CStr` and `type GuiLoop` (post a job, register and modify an
fd, add a timer, run). Linux's `GuiLoop` doubles as the plugin main thread in the child, and
in-process too. That replaces the "one thread running jobs from a channel, off macOS" kind in
`main_thread.rs:42-44`, which can't serve `posix-fd-support`.

**VST3 and AU** (the Deferred row; the VST3 SDK has been MIT since 3.8 *(web)*):
- VST3 bundles are the same folder layout on every OS: `Contents/MacOS/`, `Contents/x86_64-linux/`
  or `aarch64-linux/`, and `Contents/x86_64-win/`.
- Search paths: macOS `~/Library/Audio/Plug-Ins/VST3` and `/Library/…`; Linux `~/.vst3`,
  `/usr/lib/vst3` and `/usr/local/lib/vst3`; Windows `%COMMONPROGRAMFILES%\VST3`.
- On Linux, a VST3 GUI needs the host's `Steinberg::Linux::IRunLoop` (fd handlers and timers),
  which is the same `GuiLoop`.
- AU exists only on macOS. The trait's `plugin_formats()` lists `[Clap, Vst3, Au]` on macOS and
  `[Clap, Vst3]` elsewhere, and `plugin_dirs(format)` holds the paths, CLAP's included: Linux
  `~/.clap` and `/usr/lib/clap`; Windows `%COMMONPROGRAMFILES%\CLAP` and
  `%LOCALAPPDATA%\Programs\Common\CLAP`.

Size *estimate*: about 200 to 400 lines per OS for the window and loop code.

### 4.3 Latency: what the device reports

Plugin latency and compensation (RENDER-17 to 21) are CLAP calls and planner work, the same on every
OS. The device's own latency matters for two things: the playhead ("live playback reports the
transport position minus `L`", RENDER-19, to which the output latency should be added for a playhead
that matches what is heard) and recording alignment (§4.6). cpal already gives both on every host:
- output: `OutputCallbackInfo::timestamp().playback − callback`. CoreAudio: the buffer plus
  `kAudioDevicePropertyLatency` plus the safety offset. ALSA: `snd_pcm_delay`. PipeWire: its stream
  time. JACK: port latency (0.18.2). WASAPI: the QPC position;
- input: `InputCallbackInfo::timestamp().callback − capture`.

**Nothing for the trait.** One caveat: cpal's CoreAudio sum leaves out
`kAudioStreamPropertyLatency` *(read: only the device latency and the safety offset are added,
`device.rs:1042-1050`)*. Most built-in devices report 0 there; some USB interfaces don't. A recorded
loopback measurement (out → in) is the honest check on each OS.

### 4.4 Nothing per OS

Mono, legato and glide (RENDER-16), kernel ABI v1 (ABI-2), the sampler's asset blobs (read-only, so
mapped through §4.7), stretching as an offline kernel, latency compensation, state autosave and
plugin output events. They are all engine or format work.

### 4.5 MIDI input (8b.10 if Q3 is yes; step 9's list)

| | macOS | Linux | Windows |
|---|---|---|---|
| API | CoreMIDI (`MIDIClientCreate`, an input port, `MIDIPortConnectSource`) | ALSA sequencer (`/dev/snd/seq`); JACK MIDI under JACK; PipeWire bridges the sequencer | WinMM `midiIn*` (single-client before MIDI Services); **Windows MIDI Services**, GA 17 February 2026, in-box on Windows 11 24H2 and 25H2, which makes WinMM multi-client and adds MIDI 2.0 through a separate App SDK *(web)* |
| Crate | midir (CoreMIDI) | midir (ALSA, or JACK) | midir (WinMM, or WinRT) |
| Timestamps | host time (Mach ticks), the clock CoreAudio's `mHostTime` uses | sequencer real-time queue stamps, relative to queue start, or stamp on receipt with `CLOCK_MONOTONIC` | WinMM: milliseconds since `midiInStart`; MIDI Services: QPC ticks |
| Hot-plug | CoreMIDI notifications (object added or removed) | the sequencer's System:Announce port | MIDI Services endpoint watcher; with WinMM, re-enumerate on `WM_DEVICECHANGE` |
| Permission | none (Bluetooth MIDI needs `NSBluetoothAlwaysUsageDescription`) | membership in the `audio` group, or logind seat ACLs; none in a sandbox without a portal | none |
| Virtual ports for tests | yes | yes | no in midir; MIDI Services loopback endpoints |

midir's `stamp` means something different per backend, so **the trait gets one function**:
`midi_instant(raw) -> Instant`. It maps each backend's time into the clock `Instant` reads (all three
OSes' MIDI clocks are, or convert to, that one). Placing the note in a block then uses the stream's
`now()` and the callback's timestamp, as for 8b.6's live notes, the same on every OS. At first,
8b.6's "merged at frame 0 of the block line" is enough (2.67 ms of jitter at 128 frames). Sub-block
placement comes with RENDER-14's events.

In headless CI there is no `/dev/snd/seq`. MIDI must turn up absent, not as an error, and tests
inject at the source level (8b.6's sources), not through an OS port.

### 4.6 Recording and input monitoring

| | macOS | Linux | Windows |
|---|---|---|---|
| Input stream | cpal input (an AUHAL for input) | cpal input (ALSA capture, PipeWire, JACK) | cpal input (WASAPI capture; ASIO) |
| In and out on one clock | the same device's input and output share its clock; two devices (a USB mic and the built-in output) drift, so use an **aggregate device** with drift correction (`AudioHardwareCreateAggregateDevice`) or our own drift-tracking resampler | PipeWire and JACK run one graph on one clock and resample other devices adaptively; plain ALSA needs `snd_pcm_link` on one card | WASAPI endpoints have separate clocks; ASIO is duplex with one callback (cpal 0.18.2 fixed its duplex streams) |
| Duplex | cpal has no duplex stream API (ASIO aside): input and output callbacks run on different threads, so input goes through an SPSC ring read by the output callback, one period plus a margin behind | the same | the same |
| Permission | TCC microphone prompt. **Needs `NSMicrophoneUsageDescription`** in Info.plist (none today), and `com.apple.security.device.audio-input` once the hardened runtime is on (`"hardenedRuntime": false` today); without permission, input is silent | none on the desktop; a portal under Flatpak | Settings ▸ Privacy ▸ Microphone ▸ "Let desktop apps access your microphone"; when off, capture is silent or refused *(unverified which)* |
| Writing takes | a non-realtime writer thread draining a ring, the same everywhere; free space via `statvfs` | `statvfs` | `GetDiskFreeSpaceExW` |

**The trait** gets `input_access() -> Access { Granted, Denied, Unknown }` (macOS
`AVCaptureDevice authorizationStatus`, Windows the capability-access registry or a probe *(unverified)*,
Linux `Granted`) so the UI can say why a take is silent. The rest is cpal plus engine work.
Alignment uses §4.3's input and output latencies.

### 4.7 Disk streaming (RENDER-30)

| | macOS | Linux | Windows |
|---|---|---|---|
| Cache directory | `~/Library/Caches/music2/` (RENDER-30's text) | `$XDG_CACHE_HOME/music2` (`~/.cache/music2`) | `%LOCALAPPDATA%\music2\cache` (Local, not Roaming) |
| Mapping | `mmap` | `mmap` | `CreateFileMapping` + `MapViewOfFile`; a mapped file **can't be deleted or renamed** until unmapped, so cache eviction unmaps first, and files are opened with `FILE_SHARE_DELETE` |
| Readahead (the prefetch thread) | `fcntl(F_RDADVISE)`; `madvise(MADV_WILLNEED)` | `posix_fadvise(POSIX_FADV_WILLNEED)`; `madvise(MADV_WILLNEED)`; `readahead()` | `PrefetchVirtualMemory` |
| I/O priority | `setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_IMPORTANT)` for the streaming thread, `IOPOL_UTILITY` for building the cache | `ioprio_set` best-effort high for streaming, idle for building (the realtime class needs `CAP_SYS_ADMIN`) | `FileIoPriorityHintInfo`; `THREAD_MODE_BACKGROUND_BEGIN` for building |
| Bypassing the cache (optional) | `F_NOCACHE` | `O_DIRECT` (alignment rules) | `FILE_FLAG_NO_BUFFERING` |

RENDER-30 already keeps the engine threads on ring buffers. The prefetch thread copies from the map
into the ring, so page faults happen only there, on every OS. memmap2 hides the mapping. The trait
gets `cache_dir()`, `will_need(&[u8])` and `io_priority(Io::Stream | Io::Build)`. RENDER-30's
macOS-only path needs a contract edit to "the OS's cache directory".

### 4.8 Staying awake (not yet in any design)

A ten-minute take or a long export must not be cut by idle sleep, and a hidden window shouldn't be
throttled while it exports:
- **macOS:** `IOPMAssertionCreateWithName(kIOPMAssertionTypePreventUserIdleSystemSleep)`, and
  `NSProcessInfo beginActivityWithOptions:` (user-initiated) against App Nap. A playing app isn't
  napped, an exporting one may be *(unverified)*.
- **Linux:** logind `Inhibit("sleep:idle", …, "block")` over D-Bus, or the portal's `Inhibit` under
  Flatpak. Headless servers rarely sleep.
- **Windows:** `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`, and opting the process
  out of power throttling while recording or exporting.

**The trait** gets `keep_awake(why) -> Awake`, a guard dropped to release. It is needed with
recording, and useful as soon as exports take minutes (8a §7: about 3.5 minutes on one core for p50
*estimate*).

---

## 5. One trait for all three

### 5.1 What stays out of it

| Concern | Who already makes it the same | Layer code |
|---|---|---|
| Audio I/O, hosts, devices, configs | cpal | none |
| Xruns, device changes | cpal `ErrorKind` | a classifier in `core/play` (§2.2), the same on every OS |
| Buffer size, stream clock, device latency | cpal | none |
| Monotonic clock, sleeping, available parallelism | std | none |
| Floating-point mode | per architecture | `m2-runtime/fpenv.rs` |
| MIDI ports | midir | `midi_instant` only |
| Mapped files | memmap2 | `will_need` only |
| Windows, menus, the webview | Tauri / muda | the key table (UI, §3.1) |

### 5.2 The sketch

```rust
// core/os/src/lib.rs — crate m2-os. What differs by OS and isn't already the same through std,
// cpal, midir or memmap2. One implementation per OS, chosen at compile time; no `dyn`.
#[cfg(target_os = "macos")]   pub type This = mac::Mac;
#[cfg(target_os = "linux")]   pub type This = linux::Linux;
#[cfg(target_os = "windows")] pub type This = windows::Windows;

pub trait Os {
    // ── engine threads (8a) ─────────────────────────────────────────────────────────────
    /// What engine threads share with the device's thread: the device's os_workgroup (macOS),
    /// the device thread's FIFO priority (Linux), an MMCSS task index (Windows).
    type Group: Send + Sync + 'static;
    /// At stream open, on the m2-play thread.
    fn group(device: &cpal::Device) -> Self::Group;
    /// At the top of the device thread's first callback. Never blocks: Linux records the TID and
    /// priority and, if the thread isn't realtime, hands it to a promoter thread.
    fn on_device_thread(g: &Self::Group);
    /// On each worker before its first job, and on the child's audio thread (9). May block.
    fn engine_thread(period: Period, g: &Self::Group) -> EngineThread;   // Joined | PolicyOnly | Refused(why)
    fn end_engine_thread(t: EngineThread);                                // leave, revert
    /// On each thread of a render beside playback.
    fn background_thread();
    /// Engine threads (the device's included): fast physical cores sharing one L2, inside our
    /// affinity mask and quota; and the fast and efficient counts.
    fn cores(g: &Self::Group) -> Cores;
    /// One waiter, a signaller that never blocks (8a's Sem): Mach semaphore, futex, WaitOnAddress.
    type Wake: Default + Send + Sync;
    fn signal(w: &Self::Wake);
    fn wait(w: &Self::Wake, timeout: Option<Duration>) -> bool;
    /// For gate reports: model, core types and clusters, VA bits, kernel and preemption model.
    fn machine() -> Machine;

    // ── the plugin host process (9) ─────────────────────────────────────────────────────
    type Shared: Send + Sync;        // memfd (Linux), shm or memory entry (macOS), section (Windows)
    type SharedWake: Send + Sync;    // shared futex (Linux), Mach semaphore (macOS), Event (Windows)
    type Child;                      // pidfd (Linux), kqueue proc (macOS), process handle (Windows)
    fn spawn_host(exe: &Path, shm: &Self::Shared, wakes: &[Self::SharedWake], g: &Self::Group)
        -> Result<Self::Child>;      // dies with us; the group goes along (macOS: workgroup port)
    fn wait_shared(w: &Self::SharedWake, deadline: Instant) -> bool;
    fn signal_shared(w: &Self::SharedWake);
    fn exited(c: &Self::Child) -> Option<ExitStatus>;

    // ── plugin windows and formats (9) ──────────────────────────────────────────────────
    const WINDOW_API: &'static CStr;            // "cocoa" | "x11" | "win32"
    /// The plugin main thread's loop in the host process: post jobs, fds, timers, windows.
    type GuiLoop: GuiLoop;
    fn plugin_formats() -> &'static [Format];   // AU on macOS only
    fn plugin_dirs(f: Format) -> Vec<PathBuf>;

    // ── services (9) ────────────────────────────────────────────────────────────────────
    fn cache_dir() -> PathBuf;
    fn will_need(map: &[u8]);
    fn io_priority(p: Io);                      // Stream | Build
    fn keep_awake(why: &'static str) -> Awake;  // released on drop
    fn input_access() -> Access;                // Granted | Denied | Unknown
    fn midi_instant(raw: MidiStamp) -> Instant;
}
```

The step-7 survey's pieces belong in the same trait, and their names are its business: the
durability barrier, the swap and exclusive renames, the host id, the local-filesystem check, the
watcher, the app paths, the gates' RSS. One trait means a single file per OS to read for "what is
different on Linux".

### 5.3 Per method, per OS

| Method | macOS | Linux | Windows (designed, built later) |
|---|---|---|---|
| `group` | UID → `AudioDeviceID` → `'oswg'` (8a §6.3) | an empty priority cell | an empty task index |
| `on_device_thread` | no-op | `gettid`, `sched_getscheduler`; promoter if not realtime | no-op (cpal's `realtime` set MMCSS) |
| `engine_thread` | time-constraint policy + `os_workgroup_join` | RLIMIT_RTTIME at 200 ms both (once), SIGXCPU ignored; `sched_setscheduler` FIFO at the group's priority, else rtkit | MMCSS "Pro Audio" with the shared index; `AvSetMmThreadPriority` |
| `background_thread` | utility QoS | nice 10, `SCHED_BATCH`, efficiency-core affinity | EcoQoS, below-normal priority |
| `cores` | workgroup recommendation ∩ `cpusperl2` | sysfs ∩ affinity ∩ quota | `GetLogicalProcessorInformationEx` |
| `Wake` | Mach semaphore | futex (private) | `WaitOnAddress` |
| `machine` | `hw.model`, perf levels | DMI or device tree, sysfs | registry |
| `Shared` / `SharedWake` / `Child` | shm + Mach semaphore by port / kqueue | memfd + futex (shared) / pidfd | section + Event / process handle + Job |
| `WINDOW_API` / `GuiLoop` | `cocoa` / CFRunLoop + dispatch | `x11` / poll over X fd, plugin fds, timerfd, eventfd | `win32` / message loop |
| `plugin_formats` / `plugin_dirs` | CLAP, VST3, AU / `~/Library/Audio/Plug-Ins/…` | CLAP, VST3 / `~/.clap`, `/usr/lib/clap`, `~/.vst3`, … | CLAP, VST3 / `%COMMONPROGRAMFILES%\…` |
| `cache_dir` | `~/Library/Caches/music2` | `$XDG_CACHE_HOME/music2` | `%LOCALAPPDATA%\music2\cache` |
| `will_need` / `io_priority` | `F_RDADVISE` / `setiopolicy_np` | `madvise` / `ioprio_set` | `PrefetchVirtualMemory` / I/O hints |
| `keep_awake` | IOPM assertion + activity | logind inhibit | `SetThreadExecutionState` |
| `input_access` | AVCaptureDevice status | `Granted` | the privacy setting *(unverified)* |
| `midi_instant` | host ticks → `Instant` | receipt or queue time → `Instant` | ms / QPC → `Instant` |

### 5.4 When each section is written

The owner's aim is to build each OS piece once for every platform, with no abstraction for its own
sake:
- **The platform-layer step** creates `m2-os` with the trait. It implements the parts that exist
  today (the companion surveys' list) for macOS and Linux. It *declares* the 8a section, because 8a
  is next, and leaves the step-9 sections as commented signatures, so nothing is coded before its
  step. The Windows impl compiles with `Unsupported` everywhere, checked by
  `cargo check --target x86_64-pc-windows-msvc -p m2-os` in the Linux CI container. That needs no
  MSVC linker for `check`, and windows-sys is already in `Cargo.lock`.
- **8a** implements the engine-thread section on macOS and Linux in its slice 2 (live playback).
  Design-8a's `core/play/src/rt.rs` becomes `core/os/src/mac.rs` plus `linux.rs`. Its `Setup.start`
  and `stop` hooks call `engine_thread` and `end_engine_thread`. Its `Sem` becomes `Os::Wake`.
  Its overload listener is deleted for cpal's `Xrun` (§2.2). Its §7 QoS becomes `background_thread`
  and `cores`. Linux adds about 150 to 250 lines (*estimate*).
- **8b** adds no Rust platform code. The key table (§3.1) lands with taste's slice S.
- **9** implements the host-process, window and services sections on macOS and Linux, each in the
  slice that needs it: the host process before windows, recording with `input_access` and
  `keep_awake`, disk streaming with `cache_dir`.
- **The Windows step** fills in every `Unsupported`, after deciding WASAPI versus ASIO and
  IAudioClient3 (§2.3).

### 5.5 Edits these imply in the 8a and 8b designs (for their owners)

- **8a §6.3 step 5** (overload listener): delete it; count cpal's `ErrorKind::Xrun` (§2.2).
- **8a §6.3** (rt.rs): it becomes `m2-os`'s engine-thread section, with Linux beside macOS (§2.4,
  §2.5).
- **8a §5.1** (`Sem`): becomes `Os::Wake`, which has no per-thread identity (§2.7).
- **8a §5.5** (the spin loops): add a `sched_yield` every few hundred spins (needed for Linux FIFO,
  harmless elsewhere), and cap `want` by `cores().engine`, which already respects affinity.
- **8a §6.1** (`Options.frames`): read the period from `stream.buffer_size()` per callback, since
  PipeWire may vary it (§2.3).
- **8a §9.2** (G12's set-up line): print `Os::machine()` and the realtime result on any OS. The
  verdict stays base-M1 only.
- **8a RENDER-28a's wording**: "joined to the output device's audio workgroup" becomes "scheduled
  with the device's thread as the OS allows: its workgroup on macOS, its priority on Linux, its
  MMCSS task on Windows".
- **8b §2.6** and **taste §5**: ⌃ chords are macOS only; the PC forms are in §3.1's table; the
  browser suites press the platform's modifier.
- **The contract**: RENDER-30's cache path becomes "the OS's cache directory"; HOST-2 and HOST-3
  say "the OS's main loop (AppKit, our poll loop with CLAP's fd and timer extensions, the Win32
  message loop)" instead of AppKit only.

---

## 6. What these later steps need from Linux CI and the owner's server

The CI container itself is the companion survey's subject. These are the needs that come from 8a,
8b and 9 only:
- **No realtime in CI.** The tests run engine threads on plain threads. A `Refused` result is
  asserted to be reported, never fatal, and the 1-vs-N bit tests don't care.
- **Build dependencies the later steps add on Linux:**
  - `libasound2-dev` (cpal's ALSA, already required today);
  - `libdbus-1-dev` (rtkit, and `tao` already needs it);
  - only if the features are turned on: `libpipewire-0.3-dev` with clang (bindgen) and
    `libjack-jackd2-dev`;
  - with MIDI, nothing beyond ALSA;
  - with plugin windows, `libxcb1-dev` or a pure-Rust x11rb.
- **A device clock for smoke runs** (G12's recorded Linux run, recording tests): a PipeWire null
  sink and null source under the dummy driver, or `jackd -d dummy`, in the container. On the
  owner's server, `snd-aloop` or `snd-dummy`.
- **Plugin windows:** Xvfb for a smoke test that opens and closes a window in the host process.
- **Timings:** Docker Desktop on this Mac runs an arm64 Linux VM whose scheduling and topology
  aren't the hardware's. Linux timings are recorded only on the owner's Linux machine, and every
  judged timing stays on the base M1.

---

## 7. Open questions and checks

| # | Question or check | Why it matters | Suggested answer |
|---|---|---|---|
| O1 | Fix bug 1 (an xrun marks the player stalled) now, or in 8a slice 2? | it cuts tails and fails an action on any CoreAudio overload today | now: it is a few lines in `core/play`, the same on every OS |
| O2 | The default FIFO priority on Linux when the device thread isn't realtime | must sit at or below the sound server's data thread | 10 (cpal's and Firefox's), then measure under PipeWire and JACK |
| O3 | Should Linux pin engine threads to distinct cores? | the alternative to `sched_yield` against FIFO co-location | no: an affinity mask over the cluster plus yields; pin only if measured |
| O4 | Windows low latency: ASIO (dual-licensed) or a cpal patch for IAudioClient3 | WASAPI shared mode stays near 10 ms | decide in the Windows step |
| O5 | Under Wayland, do Linux plugin windows run on XWayland and the app on Wayland (no transient), or both on X11 (`GDK_BACKEND=x11`)? | window stacking versus native Wayland | XWayland for plugins; the app native; revisit if users ask |
| O6 | The Mach port channel to the child: `posix_spawnattr` special ports or a bootstrap name? | carries the workgroup port and the semaphores | check in step 9's first slice |
| O7 | WebView2's Alt+←/→ navigation, IBus's Ctrl+., and Alt-drag in KDE and GNOME today | the key table's PC forms | check on each when the key table lands |
| O8 | `cpal` default host on Linux: ALSA through PipeWire's ALSA plugin, or the `pipewire` feature? | latency control (`node.latency`) and xruns | stay on ALSA until 8a measures; the `pipewire` feature adds libpipewire and clang to the build |

---

## Sources

Read locally: cpal 0.18.2 (`CHANGELOG.md`; `src/platform/mod.rs`; `src/host/{alsa,pipewire,jack,wasapi,coreaudio/macos}/*`;
`src/host/error_emit.rs`; `Cargo.toml` features), clap-sys 0.5.0 (`src/ext/gui.rs`,
`posix_fd_support.rs`), the macOS SDK (`os/workgroup_object.h`, `os/os_sync_wait_on_address.h`),
Rust std 1.99's `sys/sync/thread_parking`, and the repository at HEAD and in the working tree
(read only).

Web:
- [audio_thread_priority, `rt_linux.rs`](https://raw.githubusercontent.com/mozilla/audio_thread_priority/master/src/rt_linux.rs) and [`rt_win.rs`](https://raw.githubusercontent.com/mozilla/audio_thread_priority/master/src/rt_win.rs)
- [rtkit 0.14, `rtkit-daemon.c`](https://sources.debian.org/src/rtkit/0.14-1/rtkit-daemon.c) and [README](https://sources.debian.org/src/rtkit/0.14-1/README)
- [`AvSetMmThreadCharacteristicsW`](https://learn.microsoft.com/en-us/windows/win32/api/avrt/nf-avrt-avsetmmthreadcharacteristicsw)
- [CLAP `gui.h`](https://raw.githubusercontent.com/free-audio/clap/main/include/clap/ext/gui.h)
- [Windows MIDI Services GA, Windows blog, 2026-02-17](https://blogs.windows.com/windowsexperience/2026/02/17/making-music-with-MIDI-just-got-a-real-boost-in-windows-11/), [midi.org](https://midi.org/midi-2-0-coming-to-windows-11)
- [Steinberg ASIO dual licence](https://www.headlinerhub.com/steinberg-extends-its-asio-licensing-model-to-include-both-proprietary-and-open-source-options.html), [heise on VST3 MIT and ASIO GPLv3](https://heise.de/-10963451)
- [Rust std high-resolution sleep on Windows (PR #116461 log)](https://git.dreamy.place/mirrors/rust/log/library/std/src/sys/windows/thread.rs?showmsg=1)
- [Ableton forum on bypassing snap (Alt on Windows, Cmd on Mac)](https://forum.ableton.com/viewtopic.php?p=1804223)
- [libpipewire-module-rt](https://www.mankier.com/7/libpipewire-module-rt)
