# Next: build notes

What each part of the next track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/s8n/plan.md`. Each part writes only under its own heading.

## P0

The lead's seams.

## Lane A

Hear it while you drag.

## Lane B

Play in: the metronome, the keyboard piano, the MIDI keyboard. Branch `next-b`, worktree `wt/nx-b`.

### B0. midir, first (X27)

**In short**

| What | Now |
|---|---|
| MIDI keyboards | `m2-os` can hear every MIDI keyboard on the machine, as they are plugged in and out. Nothing uses it yet (B3 does) |

**What changed**

- `Cargo.toml`: `midir = "=0.11.0"` after `cpal` (Q2, X26). `core/os/Cargo.toml`: `midir` optional, behind a new
  feature `midi = ["dep:midir"]`. Only `m2-server` turns it on (`crates/server/Cargo.toml`), so no other crate builds
  midir or the OS MIDI libraries.
- `core/os/src/midi.rs` (new, `#[cfg(feature = "midi")] pub mod midi;`): `listen(on) -> Listening`. A thread
  `m2-midi` reads the list of inputs every second, opens each new one (its own `MidiInput`), closes gone ones, and
  tells `Plugged(name)`, `Unplugged(name)` and `Bytes(name, &bytes)`. "Midi Through" ports are skipped. System
  messages (clock, sysex, active sensing) are ignored. A port that won't open is tried again after 5 s. Names read
  plainly (`pretty`: ALSA's "Keystation 49:Keystation 49 MIDI 1 24:0" is "Keystation 49"), and two keyboards with one
  name get "… 2". Dropping `Listening` stops the thread and closes every connection; nothing is heard after.
- `core/os/src/{linux,mac}/mod.rs`: `midi_allowed()`. On Linux, `access("/dev/snd/seq")` first, because alsa-lib
  writes a line to stderr each time the sequencer won't open (here: `EACCES`), and the listener tries every 5 s. On a
  Mac, always true.

**`Cargo.lock`** gained only new entries (`git diff Cargo.lock`, 173 lines in, 2 changed):

| New | Why |
|---|---|
| `midir 0.11.0` | the crate |
| `coremidi 0.9.2`, `coremidi-sys 3.2.1` | its macOS side |
| `android-build 0.1.4`, `java-locator 0.1.9`, `jni-min-helper 0.3.4`, `dlopen2 0.9.0`, `process_path 0.1.4` | its Android side (never built) |
| `windows-sys 0.52.0`, `windows-targets 0.52.6`, `windows_{aarch64_gnullvm, aarch64_msvc, i686_gnu, i686_gnullvm, i686_msvc, x86_64_gnu, x86_64_gnullvm, x86_64_msvc} 0.52.6` | its Windows side (never built) |

The two changed lines only add a version to a name that now has two versions (`"dlopen2"` → `"dlopen2 0.8.2"` in
`tao`, `"windows_i686_gnullvm"` → `"… 0.53.1"` in `windows-targets 0.53.5`). No existing entry's version moved. It
reuses `alsa 0.11.0`, `alsa-sys`, `libc`, `log`, `bitflags 2`, `objc2`, `block2`, `core-foundation`.

**Tests** (`core/os`, `--features midi`): `listening_never_errors` (here MIDI won't open: no keyboards, no stderr
line; drop stops the thread in under 2 s; nothing heard after), `through_ports_are_skipped`, `names_read_plainly`.
All 41 `m2-os` tests pass with the feature; clippy with and without it; the Mac lint with it (it built
`coremidi-sys` with the stub C compiler).

**Red first:** none; a new module (its tests are guards).

**For gaps.md:** none yet (B3).

**Contract:** PLAT-2's *Today*: `m2_os::midi` (midir; one file, no OS folder; `midi_allowed` per OS).

## Lane C

Settings and safety.

## Lane D

Effects and small fixes.

## Merge

## End
