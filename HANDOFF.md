# Handoff

Written 2026-10-18, brought up to date 2026-11-08 (0.2.0 drafted; note effects, the small measuring steps, the mixing meters, the Voice changer, the Clipper, the Vocoder, the Flanger and Grains landed; the nightly's failures since 11-06 fixed, its speed tests scaled by the machine's speed, three still failing on GitHub's Mac; the graph note waits for the owner). Read this first, then [docs/architecture.md](docs/architecture.md) (the contract), then the
design for whatever you pick up. Older versions of this page, with each track's full story, are in git.

## In short

- **0.2.0 is made, as a draft** (#97, 2026-11-07). The tag `v0.2.0` and `release.yml`'s run (42391434245) built the
  Linux and Mac files and made the draft pre-release (five files). **Publishing it is the owner's.** Its notes are
  [docs/releases/0.2.0.md](docs/releases/0.2.0.md). The server has no `file` program, which linuxdeploy's AppImage
  tool needs: a build here puts an unpacked copy on `PATH` (`apt-get download file`); `release.yml` installs it, and
  `ci/release.sh build` says at once when it is missing.
- **The graph waits for the owner.** On 2026-11-07 the owner asked for the Device view's Chain and Patch to become one
  **Graph**, whose nodes are the cards: collapsed by default, expanded on a click. The note is
  `docs/designs/graph/design.md` on branch `graph-note` (80a5d073, worktree `~/m2-runs/wt-graph-note`, not pushed),
  with a mockup. It waits for the owner's D1 to D6. Build none of it before their yes (AGENTS.md rule 11).
- **New sounds in packs (2026-11-08)** (#104, #105, #106; gaps #50, #54 and #55), the core unchanged:
  - **Tune's Formant** knob (VOX-20) moves the voice's colour without its notes; at 0 Tune plays as before.
  - **The Voice changer** (`@m2/vocals`, VOX-28): Pitch and Formant, presets Deep and Chipmunk, Tune's own kernels,
    15 ms late as Tune is; its bits pinned by a hash (`CHANGED`, `core/play/tests/vocals.rs`).
  - **The Clipper** (`@m2/effects:clip`, PACK-34, CARD-127): cuts at 4 times the song's rate, twice, then no sample
    over the ceiling; 92 frames late, declared; the pure fixture `clip`.
  - **The Vocoder** (`@m2/vocals:vocoder`, VOX-29): a synth that talks with a voice, picked in its card's
    **Talks with**; Clarity, Smooth and Formant; 20 bands, no latency; the pure fixture `vocoder`, and one more
    RENDER-33 exception while Formant moves. The guide's "Make a synth talk" (`docs/guide/audio.md`).
  - **The Flanger** (`@m2/effects:flanger`, PACK-35, CARD-128, under Modulation): a copy swept from 0.1 to 10 ms,
    Feedback −0.9 to 0.9 (below 0 the hollow comb), no delay; no make-up gain, and its card says when high Feedback
    makes it 6 dB or more louder. Its picture: a cycle of where its notches are, a live line where the sweep is now.
  - **Grains** (`@m2/effects:grains`, PACK-36, CARD-129, under Delay & Reverb): short pieces of the last 2.5 s played
    back, at most 16 at once, reading only the past (no delay), seeded so the export plays the same bits; its level
    is measured from its grains. Its picture: its buffer, the band where grains start (a dot drags its far edge), a
    live line where the newest grain reads. Freeze came out for now (gap #55 says why).
  - Each has a pure fixture (`flanger`, `grains`) and both are in the browser suite `flanger-grains` and in
    `every_pack_picture_draws_inside_a_fifth_of_its_limit`.
  - `@m2/effects` 1.4.0 and `@m2/vocals` 1.2.0, recorded again. Left in gap #54: the Clipper's look-ahead stage and
    the Vocoder's row saying None; in gap #55, the convolution reverb and Grains' Freeze.
- **The nightly's failures since 11-06 (track ci-flakes, 2026-11-08):**
  - **GitHub's Mac** (five suite runs, every night): a meters read that heard no whole block sent zeros, read as
    silence, and now sends nothing (the taps count each block as it begins and ends); a window's ask again took its
    own read of a meter; `dress` read the EQ's spectrum before the song reached its hook; `instruments` counted reads
    before they came; `sidechain`'s steady tone is a 440 Hz sine, which every block holds at its peak. `dress2a`'s
    gate strip and `sidechain`'s dip need the 33 ms tick: each is a suite of its own on the sleeps list
    (`dress2a-strip`, `sidechain-dip`), and the rest of both runs there. Below, Known flakes and the Mac's paragraph.
  - **Linux:** `s8-meters`' hold tick fell between two hits of a steady kick, 0.0005 dB apart (a peak within 0.1 dB
    under the hold now holds it again, taste.md); `dress2a`'s closings differed by the test's own saw (its hits are
    now alike, sample for sample); the CPU-sharing draw test runs its draw at the lowest priority beside two busy
    threads (`m2_os::thread::lowest`).
  - **Round 2** (the lead's decisions):
    - **The six speed tests** of pictures, the EQ's drag, the measures and the spectra (CARD-75, CARD-85, TAP-13)
      judge CPU time, each bound scaled by the machine's speed: `m2_script::speed` times std's compress drawing its
      transfer curve (20 draws a sample, the median of 9) against the 4.5 ms this server takes, and says the ratio
      in the log. On GitHub's Linux they pass, and a draw of twice the work fails; on GitHub's Mac three still
      fail (Known flakes).
    - **`@m2/wavetable` 1.2.1:** its tables' pictures work out only the 31 harmonics a drawn frame shows (the vowels'
      first draw 2.4 to 2.0 ms of CPU, the same shapes).
    - **The silent player keeps the wall clock's pace** (RENDER-28a): after a late wake it renders the buffers due,
      up to 100 ms of them. On GitHub's Mac songs played at 40 to 60% of their pace.
    - `LIBC_FATAL_STDERR_=1` in `ci/check.sh` and `ci/pr.sh`: an abort says why in the log.
  - **Left:** three speed tests on GitHub's Mac; one SIGABRT as a record test ended, not seen again in 2,140 runs
    (Known flakes).
  - `ci/release.sh build` says at once when the `file` program is missing.
- **The events plan and the small measuring steps (2026-11-07 to 11-08)** (#96, #99 to #102). Both notes were accepted
  together (#96): [docs/designs/events/design.md](docs/designs/events/design.md) and
  [docs/designs/measure2/design.md](docs/designs/measure2/design.md).
  - **Note effects in the engine** (#99, ABI-15, RENDER-41, RENDER-42, PKG-21):
    - a pack's kernel can give notes (`out: events<note>`, and `fan` in its manifest);
    - a note effect plays in a lane pass before the song's sounds, so its values take no cable: the editor,
      `m2 connect` and the Patch view refuse one (`cable_refused`);
    - `@m2/core` 2.5.0;
    - `m2 add-effect` puts a note effect before an instrument.
  - **`@m2/notes` 0.1.0** (#101, PACK-29 to PACK-33, gap #62):
    - the Arpeggiator, Chord, Note echo and Lock to key, each a kernel of the pack's own, and `examples/arp`;
    - a key played live carries a bit (`LIVE_BIT`), so Hold latches only keys played live, never a clip's notes;
      plugins get the id without it;
    - a song saves the keys you played, never the arpeggio.
  - **The measuring steps** (#100, CARD-80, CARD-87, CARD-97, CARD-99):
    - `p.response({ level })`, so the multiband's split is measured under its thresholds;
    - std's drawn lowpass is gone, and importing it says what to use instead (`MOVED`'s `Went::Out`);
    - a picture reads the song's sample rate, `p.rate`, and each picture drawn from sums is checked at 44.1, 48 and
      96 kHz;
    - axis labels never cover a picture's line (taste.md DV-17).
  - **The mixing meters** (#102, PACK-26, VOX-22, VOX-27, CARD-73):
    - the gate, the de-esser and the multiband show how far they turn down, live, and the gate shows whether it is
      open;
    - Tune draws how far the voice is from its note, and how far Tune leaves it;
    - the window draws a meter in dB, or a switch, from the most it said since the last read (TAP-15's `most`);
    - a pure fixture, `core/session/tests/fixtures/meters`, pins the mixing tools' sound, and a hash in
      `core/play/tests/vocals.rs` pins Tune's.
  - **Versions since 0.2.0:** `@m2/core` 2.5.0, `@m2/effects` 1.4.0, `@m2/mixing` 1.3.0, `@m2/synths` 2.3.0,
    `@m2/vocals` 1.2.0, `@m2/wavetable` 1.2.1 and `@m2/notes` 0.1.0; std stays 0.10.0. None has shipped yet, so a
    change to one of these packs before the next release keeps its version, recorded again (PACK-18).
- **The cards plan (2026-11-06 to 11-07) gave every sound a card that draws its sound** (#85, #87, #90, #92, #93,
  #94, #95). Design: [docs/designs/cards2/design.md](docs/designs/cards2/design.md), accepted (#85).
  - **The frame** (#87, CARD-40 to CARD-47): the Device view's **Chain**, a card for each sound in the order it
    flows, each with a face, tabs (and **All**), and three sizes (Fold, Normal, Expand); **Patch** keeps the boxes
    and cables.
  - **Pictures** (#90, CARD-60 to CARD-75): a pack draws shapes in TypeScript (`view.picture`, `@m2/core` 2.4);
    music2 draws them in its style, owns the gestures (drag, scroll, ⌥-arrows, one undo step), and gives agents the
    same picture as numbers (`POST /api/graph/picture`). A draw has a budget: 20 ms of its own thread's CPU time,
    2 s by the clock at most (CARD-64).
  - **Measured curves** (#92, CARD-80 to CARD-86): `p.response` and `p.hit` measure a sound's curve or one hit
    from the sound itself, so the EQ's whole curve is what it plays.
  - **Dressed** (#93 std's cards, std 0.10.0, CARD-90 to CARD-105; #94 the packs' cards, CARD-110 to CARD-144):
    every card music2 ships has its picture: envelopes, filters, one note, the Wavetable's table, the FM partials,
    the Limit's and Master's ceiling, the EQ over its live spectrum. Pictures drawn from a kernel's sums are each
    checked against a render (CARD-99); gap #63 would measure them instead.
  - **Tidy** (#95): the window's card rules moved from the contract to `taste.md`, six rules said twice merged;
    gaps.md made honest (#59 built; #62 and #63 new); a flaky CPU-sharing draw test fixed (its busy threads
    added until the draw really waits).
- **The vocals plan (2026-11-06)** (#82 to #91). Design: [docs/designs/vocals/design.md](docs/designs/vocals/design.md),
  accepted (#82).
  - **Comping** (#84, VOX-1 to VOX-7): **Takes** on a track, a row per take, a swipe plays that take there.
  - **The song's key** in `data/arrangement.json`, read by sound as `time.key` (#86, VOX-10 to VOX-14); `m2 upgrade`
    moves an older song's key from `layout.json`.
  - **Tune** (#89, VOX-20 to VOX-26, gap #50): pitch correction in `@m2/vocals`, Glide and Amount, presets Natural
    and Robot. The whole song plays 15 ms later while a Tune is on.
  - **A voice's notes** (#91, VOX-40 to VOX-47, gap #51): Find notes, the notes over the waveform in Notes, drag
    pitch, timing and length, Straighten; only the edited phrases render through `@m2/vocals`' own kernel.
  - **Live looks** (#88, TAP-10 to TAP-16, ABI-13, ABI-14): 32 rings, meters from a kernel (a compressor's
    reduction), a spectrum.
- **Mix and sound tracks (2026-11-06)**: **Bounce to audio** (#80, RENDER-40, gap #53; `m2 bounce --in-place`);
  **sidechain by clicking** (#81, gap #21): **Listens to** on a Duck, a compressor or a gate, and Pump in
  `@m2/mixing`; **choke groups** (#83, gap #22): **Cut by** on a Drum, the Sampler's `off_by`.
- **CI and the Mac nightly**: the Mac nightly's failures fixed (#74: a kernel's time limit is clock time, a timeline
  drag goes from the revision it was pressed at, three tests); `ci/check.sh` waits for Xvfb's display, not 10 s
  (#79); gaps.md put in the owner's order, what a pop producer needs first (#78); Dependabot's #29 merged.
- **Drum kits, the FL way, and quick gestures (2026-11-01 to 11-02)** (#68 to #72; each track reviewed by an agent,
  every finding fixed with a test seen failing first, `ci/check.sh --pr` passed on each). Design:
  [docs/designs/drums/design.md](docs/designs/drums/design.md), the owner's "Yes" to D1 to D4.
  - **Drum** is the one building block: one drum sound on one track. Its menu has every sound the old drum machine
    had (17, grouped by family). A **drum kit** is just Drum tracks that play into one bus, worked out, never stored
    (KIT-1). + Track ▸ **Drum kit (several tracks)** makes one from a pack's preset (808, 909, Basic kit;
    `@m2/core` 2.2's `editor.kits`). MIDI drum files come in as a kit.
  - The **Beat** tab (⌥5): a kit's tracks as rows of steps, velocity, + Drum (KIT-10 to KIT-15).
  - The old one-track **Drum machine** is unlisted; **Make it a kit** (`m2 make-kit`) turns one into a kit, one undo
    step, saying that each hit's noise comes out a little different (a split lane can't keep its noise seeds).
    `m2 upgrade` stays exact and lists such tracks apart. Pads and the keyboard piano play a kit by sound; arming a
    Drum of a kit arms the kit (KIT-20 to KIT-26).
  - **Quick gestures** (AGENT-2): a gesture right after the window's own edit waits for it and lands, and never on a
    call, bar or clip it didn't press; every answer says `revision`, `step`, `moved` (files, by the kind of change),
    `beats` and `mixed`; every request names its window; a refusal names who changed the target. ⌘Z right after an
    import or a take works. It took four review rounds (O, P, Q, R) to get safe: worth reading AGENT-2 before
    touching `web/src/lib/history.ts` or `crates/server/src/graph/actor.rs`'s `Before::tell`.
  - Exporting a song whose code never compiled is refused in words (RENDER-3; it used to hit an index bug).
  - The guide: "Keep versions of your song with git" (Saving); gap #30 went (the owner chose git over snapshots).
- **The typed-sounds build (2026-10-31 to 11-01) made each sound's type its interface, and moved the sounds into
  packs** (#48 to #66; each track built by one agent, attacked by a reviewer agent, every finding fixed with a test
  seen failing first, `ci/check.sh --pr` passed on each; tracks were landed in batches to save CI rounds). Design:
  [docs/designs/cards/design.md](docs/designs/cards/design.md), with §7.1 for the owner's "most things should go
  inside packs".
  - **Cards drawn from types**: settings on a card as menus, switches, number boxes and waveforms (CARD-1 to CARD-7);
    choice and switch controls whose unit is their list of options, lanes of names, views and groups, all checked by
    TypeScript and again by the build (CARD-10 to CARD-17); the envelope, filter, keys and scope views, groups on the
    card, one undo step for a view's drag (CARD-20 to CARD-29); a setting has a label (CARD-34). `@m2/core` 2.1.0.
  - **std is a toolbox; the sounds are packs**: five bundled packs, installed into the store at start and recorded
    by hash in `packs/versions.toml`: `@m2/synths` (wave-shape menu, envelope and filter views), `@m2/drums` (the kit
    and **One drum**, one drum with a sound menu), `@m2/effects`, `@m2/mixing` (8-band EQ, gate, transient shaper,
    de-esser, multiband) and `@m2/wavetable` (a wavetable synth; drop a WAV as a table). Each first-party pack builds
    its own kernels from source in the repo (`std/build.sh`, ABI-10) and runs trusted like std (ABI-11). std 0.9.0
    keeps the generic blocks. **The pack format is frozen** as `@m2/core` 2 (PACK-22 to PACK-24); songs keep no
    backwards compatibility before 1.0 (FMT-3a, the owner's words).
  - **`m2 upgrade <song>`** (and **Upgrade it** in the status) moves an old song to today's std and packs in one
    undo step, keeping its sound bit for bit where it can and naming what it can't (FMT-25, WRITE-21a).
  - **The engine**: a time limit on a pack's own kernels (ABI-6, ABI-7; std's run unchecked: the checks cost 16 to
    24 %); event inputs for kernels and the sustain pedal (ABI-8, ABI-9, RENDER-35 to RENDER-39); taps for meters
    after the fader, loudness and a scope's stream (TAP-1 to TAP-9).
  - **Gaps closed**: recording (arm, punch, a take per loop pass, overdub, inputs 3–4; #24, #25), sections (#23),
    scales, lock to key, chord stamps and grooves (#6, #15, #16, #3), undo that survives closing and unsaved code
    through a crash (#29, #30; snapshots dropped for git, the owner's choice), meters after the fader (#20), `m2`
    refusing flags it doesn't know (#50), the Mac graph's flicker (a Fit/scrollbar loop, #60).
- **The registry day (2026-10-30) let music2 get packs from the internet** (#39 to #46; each track reviewed by a
  skeptic agent, every finding fixed with a test seen failing first, `ci/check.sh --pr` passed on each):
  - **FLAC** is read wherever WAV is (the `claxon` crate, FMT-24). A FLAC brought into a song lands as a WAV of the same
    bits, because clips stream from disk. A file decoded whole holds at most 2^27 samples, and a pack 2^28 in all,
    read from the headers before decoding (#42).
  - **Downloads**: `m2_editor::fetch` (NET-1 to NET-6): https only (loopback http and `file://` too), a size limit and
    a deadline on each, checked by hash, never a partial file. Built on reqwest + rustls with ring (#40).
  - **The registry's robot** (REG-1 to REG-10, #45): the list's format, the bucket's files (by hash), `m2 registry
    check|add|pending|build`, and `registry/`, the starting contents of the registry repo, with its workflows.
  - **The registry in the app** (REG-11 to REG-17, #46): ⌘F's From the library, previews, **Get**, `m2 pack get`, and
    Get for a song's missing locked packs. **No registry is set by default** until the owner picks the bucket's
    address (D10): `M2_REGISTRY=<url>` or `"registry"` in `~/.music2/settings.json` turns it on.
  - wasmtime 50.0.1 for RUSTSEC-2026-0424 (#44); Dependabot's #30 and #31; the npm lockfile check reads bundled
    packages (#39); the Mac nightly's two Rust failures (#43). Dependabot's #29 waits for the owner's review.
  - Design: [docs/designs/packs/design.md](docs/designs/packs/design.md) §19 and §20.
- **The packs night (2026-10-30) made music2 a library anyone can add sounds to** (#33 to #38; each track reviewed
  by a skeptic agent, every finding fixed with a test seen failing first, `ci/check.sh --pr` passed on each):
  - std's Sampler plays real recorded instruments: `regions` with velocity layers, round robin, release sounds and
    loops (PKG-15, PKG-16; std 0.5.0).
  - ⌘F searches instruments, sounds, effects, plugins and samples, with previews rendered by music2 (LIB-1 to LIB-8).
  - **Packs** install from a folder into `~/Music/music2/Packs`; each song pins its versions in `music2.lock`; every
    build plays its own view of the packages (PACK-1 to PACK-14; `m2 pack new|check|build|install|list|remove|lock|update`).
  - Samples come from std's new kit, from packs and from your own folders, and drop in as clips that keep time.
  - A pack's instrument can be written as data, and `m2 pack import-sfz` turns an SFZ instrument into a pack. The first
    real one, `@m2/grand-piano` (Salamander, CC-BY 3.0, 258 MB), is outside the repo, on the server at
    `~/m2-runs/packs-night/piano/grand-piano-0.1.0.tar.gz`.
  - No test writes your cache any more (PLAT-7, `ci/cache-left.sh`), and one wasmtime engine serves the process (#38).
  - Songs asking for std "0.4" need `std = "0.5"`; `@m2/core` is 1.7.0. **macOS: unverified since `e38c131`.**
  - Design: [docs/designs/packs/design.md](docs/designs/packs/design.md). Not built then: the registry (built the
    next day, above) and streaming (below).
- **Before the packs night (2026-10-18 to 10-26)**: every planned step (1 to 9) and the five tracks of the agents
  design were built on Linux (`m2` writes through the app as one undo step, `m2 watch`, `m2 focus`, `m2 point`,
  `m2 routes`, `m2 call`, `m2 listen`); buses, sends and groups; the GarageBand transport; plugins added from the app
  and presets; the PR, nightly and gates tiers, and the docs for contributors and agents; the guide (`docs/guide/`,
  also a website and `m2 docs`); and the first release, **0.1.0 (alpha)**, made by `.github/workflows/release.yml`
  from a pushed tag ([build notes](docs/designs/release/build-notes.md), PLAT-8). Each track's story is in its build
  notes under `docs/designs/` and in this page's history in git.
- What the app does: the guide, [docs/guide/](docs/guide/welcome.md) (`m2 docs` prints it). What is missing: [docs/gaps.md](docs/gaps.md). The checks by ear:
  [docs/mac-run.md](docs/mac-run.md).

## Mac runs owed

| # | Run | What it decides | Time |
|---|---|---|---|
| 1 | `ci/check.sh` on `main` | **Done at `e7518f4`** (2026-10-18), 11 of 13 rows. Two tests failed, fixed on `mf2`: a take's steps looked up by the folder's real path (`7482009`), and the plugin window test asks the host process that owns the window (`e505d0b`, the SIGBUS). G12 failed only its set-up (output at 44.1 kHz). `pl-arrange-fix`'s retry (on Linux too) was a real race, fixed on `mf2`: a marker's Delete or Rename now finds it by its bar. G7f passed one sandbox a box: a block −1,018 µs over S, a callback −2,748 µs, 76 MB against 554, build 28 ms against 205 | 71 min |
| 2 | (dropped) G7f store on the old gain | Not needed any more. It asked whether the Mac's +135 µs a block since step 7 was the 32 MiB store layout (O1). Track `ab` replaced that layout with one sandbox per box, so run 1's G7f rows now judge the new layout. The old std (ABI v0) can't load on `main` anyway. | — |
| 3 | 8a's Mac sweep, `ci/sweep.sh` | **Done at `e7518f4`.** It confirmed `live_count` 4 and `BETWEEN` = park (spin was far worse), so no code changed. Gate 8: 4 threads, 10 minutes, p99 1,308 µs (45%), 0 dropouts; provisional, at 44.1 kHz. Exports on 8 fast cores, 6.4× one thread | about 40 min |
| 4 | By ear, steps 7 to 141 | [docs/mac-run.md](docs/mac-run.md): step 7 (7–17), `sounds` (18–21), `taste-next` (22–31), `8b-ui` (32–38), `polish` (39–50), `next` (51–60), `studio` (61–72), `io` (73–82), `ab` (83–87), `ph` (88–95), `sx` (96–102), `pm` (103–110), `ex` (111–116), `br` (117–125), `bus` (126–132), `dn` (133), `docsite` (134–136), `release` (137–141) | about 2 hours, in parts |
| 5 | `ci/check.sh` on `main` (the same run as row 1's next one) | **Done at `a1e3c77`** (2026-10-25): `br`'s plugin preset search and preset load passed on a Mac (HOST-8: its preset tests, `br-presets` and `br-plugins`). The run's other failures are fixed on `mf3`; one more run, at 48 kHz, is owed | about 45 min |

**Next: one more `ci/check.sh` on the Mac, with the output at 48 kHz** (Audio MIDI Setup), on `main` (`mf3` is
merged). The run at `a1e3c77` (2026-10-25) covered `br`, `gb`, `bus` and the agents tracks, and passed 13 of 16 rows;
`mf3` fixed the rest but G12's rate, which is the Mac's setting. Everything since (packs, drums, vocals, cards) has
run on a Mac only on GitHub's Mac runner
([build-notes-mac.md](docs/designs/agents/build-notes-mac.md) says what each row should show next). The next run
judges G12 at its designed set-up, reads the plugin host's background state with the new macOS code, and shows how
long a listen of 2 bars takes on the efficiency cores. G13's first Mac numbers are recorded (callback p99 1,292 µs,
0 dropouts, at 44.1 kHz); once a run lands its listens, G13 is judged as G12 is
(`crates/gates/src/realtime/live.rs`, `G13_RECORDED`).

After a green run the lead writes in the contract's PLAT-6 and the gates table: "macOS: green at <commit>", with
G7f's numbers. If a row fails, fix it on Linux, check the mac lint, and ask for one more run. The Mac's pasted runs
are kept in `$SCRATCH/mac-runs/` on the server (`check-and-sweep-e7518f4.txt` is the last).

## The owner's Mac command

**In short:** next time only paste 1, the check, in Terminal on the Mac itself, with the output at 48 kHz. Then paste
back what it says. (Paste 2, the sweep, is done.)

**1. The check** (about 45 minutes, mostly waiting):

| # | Do this | Why |
|---|---|---|
| 1 | Plug the Mac in, keep the lid open, and quit other apps. | The speed tests need a quiet Mac. |
| 2 | Open Terminal on the Mac itself, not over ssh. | One test needs your login session. |
| 3 | Type `cd`, a space and the path of your music2 folder (the one you cloned), then `git fetch`, then `git switch main`, then `git pull`. | This gets everything on `main`. |
| 4 | Type `ci/check.sh` and press Return. | It runs every check and the speed tests. |
| 5 | If one line says `running` for more than 30 minutes, press Ctrl-C once. | It stops that check and still prints the summary. |
| 6 | Copy the last lines, from `music2 check:` to the `logs` line, and paste them back. | That is all the lead needs. |

Before it, set the output to the built-in speakers at 48 kHz (Audio MIDI Setup): at another rate or buffer, G12
says "not the designed set-up" and what to set.

**2. 8a's Mac sweep** (about 40 minutes; it plays nothing aloud). Same set-up as the check (plugged in, lid open,
other apps quit, built-in speakers at 48 kHz):

```sh
cd <your music2 folder> && git fetch && git switch main && git pull && ci/sweep.sh
```

Paste back what is in `~/m2-8a.txt`. Its steps: the pool's models (a FAIL: stop, fix on Linux first), `m2-os`'s
real-time tests (a FAIL: fix that code before anything live is judged), `sysctl`, the offline sweep, the live sweep
(the fewest threads whose p99 is within 5% of the best, with no dropout; spin if its p99 is at least 10% lower), and
gate 8 for 10 minutes. The full table: `docs/designs/step8/build-notes-8a.md`, "The Mac command".

**What must pass in the check:** the first line says `music2 check: PASS`; every row says PASS (the gates row too,
not PART); nothing "passed only on a retry". In the gates part: G11; G5 and G7a through the server, heard within
200 ms of the request; G5 and G7b on the engine rig, with G7a's click check; G7e (the export pauses, borrows both
plugins, comes back with no click, the held chord sounds again); G7f store (block within S + 80 µs by the mean,
footprint within S + 64 MB, build within 1.5 × S, the 25% rule, the same music); G7f arena (callback within S's +
320 µs by the mean, the same music, the slowest activation block under 5,333 µs); G12's minute.

**What may fail, and why:**
- G7f, near its bound: ask for one more run. G7f's own error is about ±150 µs a callback from run to run (the A/A
  table in `crates/gates/s/README.md`).
- PART, or an `info machines:` line: the Mac's name differs from the records written by hand. Copy what follows
  `machine: ` over the `machine=` value of the three record lines (`store-record` and `store-p1` in
  `crates/gates/store-S.txt`, `callback-record` in `callback-S.txt`), and ask for one more run.
- Anything else that fails is new.

**What the lead reads in the pasted lines:** the `machine:` line names `MacBookPro18,1, macOS …, Apple M1 Pro,
aarch64, 10 threads`; G7f's two "the same music" lines say ok; "S as recorded" within about 1% (90 µs) of 8,936 µs
a callback; the "starting a turn" lines above −100 µs a callback (−30 a block), else a pass near the bound is unsure.
G7f's S plays its own std, as S can't play the app's.

## Known flakes

Seen, not fixed. Linux timings are information only (PLAT-1): the gates are judged on the Mac. The sweep in #72 fixed the race behind several (`s8-clips` "8+28", `st-errors`, `nx-effects-fix`, `st-midi`'s ⌘Z after an import): a gesture or a read made before the page showed a change; `e2e/caughtup.mjs` is the wait for a change the page didn't make, and fixlib's `late` option holds the page's reads back to bring such races out.

| Test | What fails | Note |
|---|---|---|
| G7f arena's slowest activation block | over 5,333 µs under load | can fail the gates row here beside other agents |
| G3 (a note edit within compile + 20 ms) | p95 16–22 ms at n=10 | near its line under load |
| G11 tx-data, 5,000 files in assets/ | p95 6.8 ms past its 5 ms | once, at load 4–10 while other tracks built (track `br`); the rerun passed at 1.6 ms |
| G5 and G7a through the server | p95 past its bound | twice, at high load while other tracks built; alone they pass at about 111 ms |
| `taste` | the Bass header's click didn't select it within 30 s | once, three suites at once beside other agents |
| `live3b` | "its release writes db(-32) into song.ts" | once, at load 45–74, before step 7 |
| `s5-knobs` | page detached after a double-click on a knob at its default | from before the platform step |
| `pl-sounds-fix` | "one revision, and song.ts as it was" right after Return in Change instrument | once, 2026-10-30, track F's check; passed on the retry. The check reads the revision as soon as `tracks/hats.ts` is written, so the file may land before its revision is counted |
| `st-audio-fix` | "and adds nothing": the revision moved after an MP3 was refused | once, passed on its retry |
| `a_second_session_is_read_only` (`core/session`) | its `refresh` after the lock came back | once, under load; alone 3 of 3 pass |
| `taps_and_the_load_never_allocate` (`core/play`) | "the rings' frames were read": its reader thread wasn't scheduled within 40 callbacks | once each in three checks at load 50 to 70 (2026-10-31); moved to the nightly tier unchanged (`ci/nightly-tests.txt`). Once more on 2026-11-08 (ci-flakes, six crates' tests at once, load 4 to 12), "the meter slots were read", the same cause; passed on its retry |
| `pl-notes` | "the focus is back in the roll" after two Escapes | under heavy load, on main too (4 of 8 at load 60); cause not found |
| `pl-zoom-fix`, `ag-notice-fix`, `notes-proj`, `start-proj` (and `spotlight-fix`, `s8-songbar-fix`, `ports-proj`, each with one race site fixed in #72) | one check each, under load beside other agents' builds | each passed on its retry, 2026-10-31 |
| `the_samples_of_std_and_a_pack_are_found_and_fit_the_song` (`routes_library`) | a search right after a song opens found nothing | twice, passed on retry; more packs to load may widen it |
| `a_hung_host_process_is_ended_and_said` | the plugin host's timing | once, passed on retry |
| `m2-os`'s two `fake_sysfs` tests | they share one temp folder | run together they sometimes fail; alone they pass |
| `nx-piano-fix` | "the code editor gets its letters" | 2 of 2 on GitHub's Mac at `96d18e7` (run 40478186453); passes on Linux. Cause not found. Its check is now three (the editor's box, its text, no notes sent), each saying what it read. The next Mac nightly (`53382be`, run 40666853783) passed it with no retry. Back on GitHub's Mac on 11-06 and 11-08, each passed on its retry (the issue "Flaky tests"), and 1 of 2 in track ci-flakes' Mac probe (run 42481172658): the click hit the editor's lines, but after the keys the focus was in `body`, so they played notes. Not the Mac's late clocks |
| `sx-follow` | Chrome's page closed 2 s in ("Target closed", during a `$$eval`) | once on the Mac at `a1e3c77`; passed on its retry, 22 of 22 |
| `a_slow_disk_misses_but_never_waits` | failed once on the Mac at `e7518f4` | passed on its retry |
| `s8-mute-export` | failed once on the Mac at `e7518f4` | passed on its retry |
| `layout-proj`, `attention-attn` | failed once on GitHub's Mac at `53382be` (run 40666853783) | passed on their retry, alone; its logs weren't kept (a passing run keeps none) |
| The nightly's six speed tests: `every_pack_picture_draws_inside_a_fifth_of_its_limit`, `every_instrument_picture_draws_in_a_fifth_of_its_limit`, `an_eight_band_curve_draws_inside_its_budget`, `a_drag_frame_of_the_eq_stays_under_8_ms`, `thirty_one_spectra_fit_in_a_read_with_room`, `a_measure_follows_a_drag_inside_its_budget` | on GitHub's Mac, three: the pack pictures' (the chorus's motion, draws 2 and 4 at 44.1 kHz, 5.2 ms; the phaser's motion's first, 5.4 to 6.0 ms; the reverb's tail's first at 48 kHz, 6.5 ms; against 4.4 to 5.8 ms, the ratio 1.09 to 1.44), the drag's (p99 8.9 and 10.5 ms, its median 4.0 to 4.2, against 8.6) and the measure's (one measure past its budget, `Slow`) | Every nightly since they came (#90 to #94). Since ci-flakes' round 2 they judge CPU time, each bound scaled by the machine's speed (`m2_script::speed`, CARD-75): GitHub's Linux passes all six (run 42494489844, the ratio 1.50 to 1.56 beside the other tests; alone 0.90 to 0.93, where the measure's failed once, `Slow`), and a draw twice the work fails there. The Mac's reference work runs as fast as here (the ratio 1.07 to 1.44), but those draws don't: here the chorus's draws 2 and 4 take 1.3 ms, the phaser's first 2.9, the reverb's first 3.2 (the ratio 0.92), so 2 to 4 times longer there, and the drag's p99 is 2.2 to 2.6 times its median there against 1.1 here. Work that allocates (QuickJS's collector, a measure's copy) or a virtual machine's stolen time, likely: what the one reference doesn't do. The instruments' pictures keep the reference's ratio there (the Wavetable's vowels 2.33 ms at 1.17, 2.0 here). In ci-flakes' last nightly (run 42503673490) the Mac's own run passed all six, the drag on its retry, and a run of them after it failed the pack pictures' again: they come and go there. A draw twice the work (the phaser's motion worked out twice) failed on both runners and here |
| `an_armed_track_monitors_without_a_take` (`m2-server`, `record_tests.rs`) | passed, then its process aborted (SIGABRT, nothing on stderr) as it ended | once, in the lead's `ci/check.sh --pr` on 2026-11-08, under full load; passed on its retry. Not made to happen again: 2,140 runs of it alone (8, 16 and 24 at a time), 15 nextest stress rounds of the record tests and 6 of all `m2-server`'s lib tests, 16 at a time. An abort with nothing on stderr is `std::process::abort` with no panic (the pool's `AbortOnPanic` prints its panic first) or glibc's fatal message, which goes to the terminal, not stderr, when there is one (that check ran in tmux): `LIBC_FATAL_STDERR_=1` keeps it, set in `ci/check.sh` and `ci/pr.sh` since ci-flakes' round 2 |
| `dress2a-fix` | on GitHub's Mac (11-08), "the mid's dot dragged up raises its gain" wrote nothing, and the ⌘Z after it undid the suite's own setup, so Pump's checks after it failed | once; passed on its retry, and 2 of 2 in ci-flakes' Mac probe. Cause not found |
| `io-record-fix` | failed once on GitHub's Mac (11-08) | passed on its retry alone, and 2 of 2 in ci-flakes' Mac probe |
| `a_burst_past_the_hint_limit_is_walked_and_never_missed` (`m2-server`, `watch_tests.rs`) | "the walk was a rescan": 4,219 names, two walks, one scoped, no rescan | both tries on GitHub's Mac in run 42483302299; it passed on the 11-07 and 11-08 Mac nightlies. The cause: FSEvents' daemon can get a change a moment after its write, and a flush right after it may return first (`m2_os::watch`'s `flush` says so), so the first tick took part of the burst, under the 4,096 names, and the rest came after: no tick overflowed, and the watcher did as WRITE-19 says. Since ci-flakes' round 2 the test waits (at most 20 s) till the names overflow before its first tick, and says whether they had by the flush. On GitHub's Mac (run 42503673490) it passed 20 of 20, and in 4 of them the names overflowed only 20 to 37 ms after the flush: the runs the old test failed |
| `activations_keep_the_plugin_instances_and_closing_destroys_them` (`m2-server`, `graph/tests.rs`) | `Teardown { destroyed: 0, left: 0 }`: closing destroyed no plugin instance it counted | once, in ci-flakes' run of six crates' tests at once after merging #106 (2026-11-08, load 10 to 17); passed on its retry. Not looked into |
| `pl-mix-fix` (nightly tier) | "its strip meter still reads as the header's, after the fader": the reads before the fader caught a hat late in its fall (−34 to −41 dB, not −22), so the fall after looked short | GitHub's Linux in ci-flakes' run 42503673490 (no retry there); here, interleaved, 3 of 14 runs with the silent player keeping pace and 1 of 14 with the old one, the same check: its 10 reads before sample a meter that falls between hits. More runs may say whether the new player makes it likelier |
| `s8-tracks-fix` (nightly tier) | `TypeError: Cannot read properties of undefined (reading 'x')` at `s8-tracks.mjs:218` | GitHub's Linux in run 42503673490; 3 of 3 here. Its known flake (reads the new track's tab too soon), likely |
| `m2-server`'s registry tests, under `cargo test` | "Too many open files" | in a tmux shell, whose open-file limit is lower; they pass from a normal shell and in `ci/check.sh` |

**GitHub's Mac (macos-15), the first nightly (run 40296575023, `07cd88b`):** its sleeps last about four times what
they ask, as its logs show: the meters' 33 ms tick came 8 or 9 times a second, and the capture's 10.7 ms clock
played about 0.6 s of sound a second, in a suite run alone too. So what a sleep clocks there runs slow, every
time: `previews_never_wait_for_a_compile` ("update …'s stamp never came": the silent player's clock), and the
suites that listen to the capture or count meters: `nx-click`, `st-record`, `s8-meters`. On Linux, timers made
late by hand (a timer slack of 45 to 90 ms, `prctl(PR_SET_TIMERSLACK)`, on the headless server or the test) fail
them the same way, and they pass without. Not the code's: on a real Mac a device clocks the app. The owner chose
to leave these four out of that job alone (`M2_LATE_SLEEPS=1`: `ci/sleeps-tests.txt`, and the `sleeps` tags in
`e2e/run.mjs`); they still run in the PR tier, the Linux nightly and a real Mac's `ci/check.sh`.
Track ci-flakes measured those clocks there (2026-11-08, its Mac probe, run 42481172658): the silent player's 10 ms
sleeps took 0 to 88 ms, spread nearly evenly, and the meters' 33 ms tick came every 70 to 250 ms (median 121). Five
suite runs added since then failed there every night. The causes, now fixed: a meters read that held no block sent
zeros, which read as silence (now it sends nothing: `sidechain`); a window's ask every 4 s took its own read of a
meter (`dress2a`'s closings); `dress` read the EQ's spectrum before the song reached its hook, and `instruments`
counted reads before they came. A meters message there may also hold one 10 ms buffer, so `sidechain`'s steady tone
is a 440 Hz sine, every block of which holds its peak. `dress2a`'s gate strip counts openings read by read, which
needs the 33 ms tick: it is a suite of its own, `dress2a-strip`, on the sleeps list. The nightly on ci-flakes' branch
(run 42483302299) passed the other four there, on their first runs. Since ci-flakes' round 2 the silent player keeps
the wall clock's pace there (RENDER-28a: after a late wake it renders the buffers due, up to 100 ms; it caught up one,
so songs played at 40 to 60%). Then `sidechain`'s dip failed there (run 42494489844): a meters message held a kick
and the gap after it, so 3 reads with the kick down and a 0.7 dB dip; with 120 ms of timer slack here, the same,
and with the old player it passed. Those two checks are `sidechain-dip`'s, on the sleeps list. Its sleeps there ran
up to 150 ms late in that run, past the 100 ms the player catches up, so it may still drop a little sound there.
Of the two Rust tests of the sleeps list, run there 3 times each with the new player (run 42503673490),
`a_lane_written_in_two_halves_never_plays_empty` passed 3 of 3 and `previews_never_wait_for_a_compile` failed 3 of 3
("update …'s stamp never came"): both stay on the list until more runs say. The silent player's pace test passed
there 5 of 5 (2.04 to 2.13 s of song in as much by the clock). The capture's clock
(`crates/server/examples/headless.rs`, `nx-click` and `st-record`) still catches up one buffer.
`a_code_save_behind_a_previewed_write_plays_both` passed there on its retry. G10's timing failed there too ("bar 64
takes no longer than … from bar 1: 1.25 s against 1.05 s", in `the_live_engine_passes_every_gate`), timed by the
wall clock beside other tests, as macOS doesn't say how long a thread waited for a CPU. It now times the rendering
thread's CPU time (`1c1cc20`). That is unproven on a Mac: CPU time still counts a core's slowdown when something
shares it, and a virtual machine's stolen time. G10 stays on this watch list until a Mac nightly passes it.

A test or suite that passed only on its retry is listed by `ci/check.sh`; on GitHub's nightly, also in the open
issue "Flaky tests", with the nights each was named on, and both jobs keep their logs 14 days. The lead decides
whether it is one of these or a real failure, and fixes the real ones (most were a read a moment too soon).

## What comes next

1. **Publish 0.2.0** (the owner's): the draft is made.
2. **The graph** (the owner's D1 to D6 first): then the tracks `graph-node` (the node, collapsed and expanded, in
   place of Chain and Patch) and `graph-insert` (**+** on a wire, which also gives a note effect its way in from the
   window, gap #62).
3. **Small, left by the last tracks** (each in gaps.md):
   - the keyed compressor's dot rides its own input, not what it listens to (M2-21: it needs a look at the sidechain,
     a core change);
   - the reverb's tail at 44.1 kHz;
   - std/math's blocks take notes in the code editor with no type error;
   - the Arpeggiator's steps picture past three held notes;
   - the multiband's bars and Tune's middle line's name, with the graph's cards;
   - wires that show their level (a note of its own: it needs a tap on a wire);
4. **The owner's own items**, open:
   - **The registry, live**: the Cloudflare R2 bucket and its address (D10), then the repo
     `BreadBowlAI/music2-registry` from `registry/` (`registry/README.md`, "Setting it up"). Then a release with
     `DEFAULT_REGISTRY` set, `@m2/grand-piano` listed, then the `@m2/` packs.
   - **An Apple Developer ID** (gap #43): the repo's Apple secrets turn on signing and notarizing; the hardened
     runtime's entitlements have never been tried on a Mac.
   - **The Mac's one more `ci/check.sh`**, at 48 kHz (above). macOS is unverified by hand since `e38c131`.
   - **Tune and a voice's notes, by ear** (gaps #50, #51): the tests use a made-up singer; a real voice is owed.
   - **Their own songs**: `m2 upgrade <song>` (or Upgrade it) on each, e.g. "Beat Pack" (std "0.4"); then Make it a
     kit on any drum machine track, if wanted.
5. **Tiers**: `st-midi` and `s8-clips` can come back to the PR tier (their flakes' cause was fixed in #72), after a
   few clean nightlies.
   Three of the six speed tests still fail on GitHub's Mac, scaled (Known flakes): a reference closer to their work,
   or how they can hold there unweakened, is the lead's to decide, from track ci-flakes' report. The capture's clock
   could keep the wall clock's pace as the silent player does; then `nx-click` and `st-record` might leave the sleeps
   list.
6. **Then the top gaps** in [docs/gaps.md](docs/gaps.md)'s order (slicing #52, more effects #55, MIDI learn #56,
   macro knobs #57, the rest of #62: Bake to notes, note repeat in a kit, generators), each inside a pack where it
   can be; the beat grid's gaps (a sound when a step is clicked, copying a pattern, cut groups across a kit's tracks,
   D3).
7. **Windows, as its own step** ([PLAT-1](docs/architecture.md#platforms)): `core/os/src/windows/` and the build.
8. **Open owner decisions** (below).

The owner wants music2 to be a novel, type-safe DAW built from first principles, simple yet elegant, with std as
small as possible and most sounds in packs (2026-10-31).

## Open owner decisions (ask, with a recommendation)

| Question | What was built | Recommendation |
|---|---|---|
| The bucket's address (packs design D10). D1 is decided: Cloudflare R2, the list on GitHub (2026-10-30). | Everything but the bucket: `DEFAULT_REGISTRY` is empty, so the library shows only with `M2_REGISTRY` or the setting. | The bucket's own `r2.dev` address to start (the robot must read it, not a cache); a name of ours (`packs.<domain>`) in front for the app later. |
| A Settings field for the registry's address. | None: the environment or `settings.json`. | Not until there is a default; then a field for a mirror. |
| How big the cache of previews and got files may grow. | Never pruned. | A limit (2 GB), the oldest out first. |
| How loud ⌘F's previews are. | −23 LUFS, measured as BS.1770 does; hats and rims fall up to 2 dB short. | Keep it; −20 if previews feel quiet (hats, rims and Snare 2 then fall 2 to 4.5 dB short). |
| G7f: O2 named a p99 rule. | The mean of 5 pairs is judged; the p99's estimate is printed beside it. The p99 moved by hundreds of µs from pair to pair, the mean by about 12. | Keep the mean. |
| A plugin that keeps failing. | Started again once. If the new one fails within 10 s, it stays silent until the song is opened again, and you are told once. | Keep 10 s. |
| A plugin stuck for good. | After 30 s (2 minutes in a scan) music2 ends the plugin program and says so (track `ph`). | Keep 30 s; the Mac run may move it. |
| The output device changes while playing (headphones in or out). | Playback stops, and a message says "Press Play to go on". | Keep it: music shouldn't jump to the speakers on its own. |
| A songs folder reached through a link. | Settings and the start screen show it at its real place (`/Volumes/Big/Music/music2`). | Keep it. |
| Space, Return, Space: should the wobble and the drums' noise start again as in the export? | Not now: they carry on from where they stopped, as at any jump. Changing it changes no export. | Keep it. Ask once: "After you stop and go back to bar 1, the wobble carries on from where it was; should it start again as in the export?" |

Decided, and still in force: step 7's O2 to O5 (G7f's bounds, a note held across an export plays again, a new node's
inputs fade in); O1's 32 MiB layout was replaced in track `ab` by one sandbox a box (idea 3), which measured better
on every count ([RENDER-32](docs/architecture.md#engine-shape-step-7)); std's version moves with a render change,
one std bundled, the lockfile waits for its trigger ([FMT-3](docs/architecture.md#manifest-and-versions)); gate 8 on
the M1 Pro is provisional until a base M1 exists; no hosted Mac runners; the desktop app may open here only on an
invisible Xvfb screen, with pictures sent to the owner.

## Your own songs (for the owner)

**In short:** a song made with an older music2 opens with "Made for an older music2: Upgrade it" in its status. Press
it, or run `m2 upgrade <song>`. It moves the song to today's music2 in one step, which Undo takes back.

| | |
|---|---|
| Which songs | Any song asking for std "0.2" to "0.9" (a song from 0.1.0 asks for "0.4"), or using the old drums and synths from before the sounds moved into packs. |
| What it changes | The std line and the pack lines, the imports, the single drums (now the Drum, with their knob values converted), the song's key (from `layout.json` into `data/arrangement.json`), and the lock. Nothing else. |
| The sound | The same, bit for bit, where it can be; anything it can't convert exactly is named with its file and line, and then nothing is written. |
| Presets | Your saved presets for the old single drums aren't changed: the upgrade names them, so you can save them again. |
| First try | `m2 upgrade <song> --check` shows what it would change, and writes nothing. |
| A drum machine track | Upgrade it leaves it as it is, so the sound stays exactly the same. To give each drum its own track, use **Make it a kit** in the track's menu: same drums, pitches and levels; each hit's noise comes out a little different. |

## Where things are

| What | Where |
|---|---|
| The contract, and the order of work | `docs/architecture.md` |
| What each step and track built, decided and left | `docs/designs/cards2/`, `vocals/`, `drums/`, `cards/`, `packs/` (the designs since 0.1.0, each with its accepted decisions); `docs/designs/step9/` (`<track>-plan.md`, `build-notes-<track>.md` for `io`, `ab`, `ph`, `sx`, `pm`, `ex`, `br`, `bus`); `step8/` (taste, 8a, 8b, polish, next, macfix, studio); `agents/` (the agents design and its five tracks' build notes); `step7/`, `sounds/`, `platform/`, `kernel-layout/`, `step6/` |
| Kernel ABI v1 | `docs/kernel-abi.md` |
| The look | `docs/designs/step8/taste.md` |
| Each check's logs and summary | `$SCRATCH/check-<label>/` and `.tgz` (step 9's finals: `check-io`, `-ab`, `-ph`, `-sx`, `-pm`, `-ex`, `-br`, `-bus`) |
| Pictures for the owner | `$SCRATCH/app-shots/<track>-<commit>/` |
| Logs of long jobs | `$SCRATCH/logs/` |
| The Mac's pasted runs | `$SCRATCH/mac-runs/` |
| Branches | Everything is on `main`. A track's branch and worktree are deleted once it lands. On GitHub only Dependabot's branches are left beside `main` |

Engine and tooling items left by steps 7 and 8 that are not user gaps are in each build notes' "Left" or "For the
lead" sections. The ones a next session most needs:
- G12 isn't built with `--server`, `--export`, `--like-m1` or `--capture resample`.
- The recorded tests aren't in `ci/check.sh`: `cargo test -p m2-play --release --test bit_identity --test live
  --test churn -- --ignored --nocapture` (about 10 minutes).
- Every load of std compiles `std.wasm` (+70 ms a load); a compiled-module cache would win it back.
- K13: a note in the very block after a state change doesn't sound in Surge XT. K14: a plugin that applies a loaded
  state late can still win over the code's values.
- The speed work's cuts 3 and 4 are held (the owner's call, only if the Mac misses G7f).
- Platform design §3's OS-82, 83 and 84 still say "Designed"; they are built. (The contract's *Today* lines no
  longer cite old line numbers.)

## How the work is done

- **Writing to the owner:** they are dyslexic and new to music. "In short" first, short sentences, plain words,
  tables, no walls of text.
- **Pace** (since 2026-10-17): tracks in parallel, one full check per track, quality kept. For each track: plan,
  attack, build in green slices, review, fix (each with a test shown red first), `ci/check.sh --gates`, land.
- **Simple yet elegant**, a tasteful professional DAW. No built-in AI in the app. No backwards compatibility.
- **Worktrees:** each track works in its own, `$SCRATCH/wt/<name>` on branch `<name>`. Never touch
  the main checkout or another track's worktree. Never push from a track; the lead lands it.
- **Builds:** `--locked`, never `cargo update`. On a merge, take `main`'s `Cargo.lock` and let `cargo build` add the
  branch's edges; never take a side on `std.wasm`: rebuild it (`std/build.sh`). OS code outside `core/os` fails
  `core/os/tests/one_place.rs` (PLAT-2).
- **Commits:** plain words, ending with the Co-Authored-By and Claude-Session lines the session gives.

## The server

| What | How |
|---|---|
| `$SCRATCH` | The maintainer's scratch folder on the server, outside the repo: worktrees, logs, each check's folder, and the `tw` helper. Paths in this page and in `docs/designs/` use this name (set it in your own shell). |
| The machine | The maintainer's Linux server: Ubuntu 24.04, Ryzen 9 7950X3D (32 threads), 124 GB, ext4 on RAID. No sound card. Other agents' jobs often run beside yours: Linux timings are information only (PLAT-1). |
| Long jobs | Anything over 2 minutes: `$SCRATCH/tw <name> '<command>'` (a small helper kept outside the repo) runs it in a window of tmux session `m2-sh`, which the owner watches; the log goes to `$SCRATCH/logs/`. It waits 9.5 minutes (`TW_WAIT=<seconds>` for longer, `TW_WAIT=1` to wait), then prints `$SCRATCH/tw-wait <channel> <log>` to wait again. No double quotes or parentheses in the command. Never send keys to the tmux sessions `m2`, `1`, `2` or `ssh_tmux`. |
| Scratch `HOME` | Anything that runs music2 gets a scratch `HOME` under `$SCRATCH`, `M2_AUDIO=none` and `M2_MIDI=none`, and then `CARGO_HOME` and `RUSTUP_HOME` set to your real home's `.cargo` and `.rustup`. Never the real `~/.music2` or `~/Music`. `ci/check.sh` does this itself. |
| Disk | Scratch `target/` folders once filled 390 GB: share or clean them after each track. Never move a `target/` between worktrees uncleaned (it keeps the old paths). |
| Test plugins | `ci/plugins.sh` (once): Surge XT 1.3.4 and Airwindows Consolidated into `~/.local/share/m2-test-plugins/clap`, from the archives in `~/.local/share/m2-test-plugins/dl` (Airwindows' zip is gone upstream: keep that copy; the `test-plugins` release on GitHub holds one too). Never into `~/.clap` or `/usr/lib/clap`. Never delete anything under `$SCRATCH/plugins`. |
| The desktop app | Only on an invisible Xvfb screen, as the check's app row opens it. Never on a real screen. |
| Tools | Rust 1.99.0 (`rust-toolchain.toml`), Node 24, chrome-headless-shell 133.0.6943.141 (the check installs Rust and Chrome when missing), Xvfb. |
| MIDI | `/dev/snd/seq` gives `EACCES` here: tests inject notes and never open a device. |

## The tiers: what runs where

Every check runs from a script in `ci/`, so whatever CI runs, you run with the same command.

| Tier | Where and when | The command | What |
|---|---|---|---|
| Pull request | GitHub, on every pull request, merge-queue group and push to `main` (`.github/workflows/ci.yml`). One required check: `ci-ok`. | `ci/check.sh --pr` (all of it, one step after another); `ci/pr.sh <step>` (one job) | fmt, clippy, the Rust tests in 3 parts (cargo-nextest), the pool's models, the web build, std's kernels and `std.wasm`, the browser suites in 2 parts, the 12 renders, licences, actionlint and zizmor, the checks' own cases. RustSec's advisories only when a `Cargo.lock` or `deny.toml` changes. A docs-only change runs the docs job alone. |
| Nightly | GitHub, every day on `main`, only once the repo is public (`.github/workflows/nightly.yml`) | `ci/check.sh --nightly` | the full check without the timing gates (so every PR-tier check and the advisories too), the browser suites one at a time, the tests tagged nightly; and a Mac: the Rust tests, the pool's models, the browser suites, the renders. A failed or timed-out job opens or comments on one issue, "Nightly failed"; a test or suite that passed only on its retry is named in another, "Flaky tests", with the number of nights it was named on. |
| Timing gates | a quiet maintainer machine, every night, by cron | `ci/nightly.sh --report` | `ci/check.sh --gates` on a fresh clone of `origin/main`; posts the status `nightly/gates`. It runs as an OS user of its own, with a fine-grained token that can only write commit statuses (docs/maintainers.md, part 5). Not set up yet. |
| Full check | your machine; the server with `--gates=auto` | `ci/check.sh` | everything, below: every check of the other tiers. |

- **Which tier a test is in.** A Rust test that judges speed or is a known flake is listed in `ci/nightly-tests.txt`, with why; a browser suite gets `tier: "nightly"` and a `why` in `e2e/run.mjs`. Unchanged either way: it runs every night. One more list, apart from the tiers: a test whose clock is a sleep is in `ci/sleeps-tests.txt` (Rust) or tagged `sleeps` in `e2e/run.mjs`, with why. Only GitHub's Mac job leaves them out (`M2_LATE_SLEEPS=1` in `nightly.yml`: its sleeps run long), with a "late sleeps" row that names them; they run everywhere else, a real Mac's `ci/check.sh` too.
- **Times** (`ci/check.sh --pr` in a clean clone on 8 threads of the server, at `2141096` and `d4681ab`): about 10 minutes cold (607 s), 7.5 warm (447 and 451 s), with music2's own crates rebuilt. On GitHub's 4-core runner each job is one part of that; its real times are still to be measured (gaps.md).
- **What the first real runs found.** A test that hosts Surge XT or Airwindows in this process could pass, then crash as its process exited: a destroy is queued to the plugin main thread, and the exit ran the plugin library's destructors under it. nextest runs each test in a process of its own, so it showed at once (one test crashed 40 runs of 40 alone). Now `test_main_thread()` drains that thread when the test's thread ends (`d4681ab`).
- **Verdicts.** `--pr` says PASS only when CI would pass. A docs-only change needs no plugins. `M2_NO_PLUGINS=1` skips the plugin tests and says PART; so does `M2_LATE_SLEEPS=1`, for what it leaves out. A browser suite that was PART fails it, even when another run failed and then passed alone. "What failed" names a Rust test whose process crashed (nextest's `SEGV`). The cases are in `ci/pr-test.sh`, `ci/nightly-test.sh` and `ci/nightly-issue-test.sh`, which the lint step runs (in quick, and in the full check).

## The check: `ci/check.sh`

One script checks everything, on the server and on the Mac ([PLAT-6](docs/architecture.md#platforms)). Its first
line says `music2 check: PASS`, `FAIL` or `PART`. PART means no row failed but the gates couldn't judge G7f (the Mac
isn't the one S's records name). Only PASS is a pass.

Rows: gates rule (`--gates=auto`'s rule tried on its 51 cases), toolchain, lint (`ci/pr.sh lint`: fmt, actionlint, zizmor,
licences, the CI scripts' own cases), advisories (`ci/pr.sh advisories`: RustSec's), web build, build, app (Linux: the
desktop app on Xvfb with `first-song` and `with-plugins`, a picture of each), rust tests, nightly list and sleeps list (Linux: every
test `ci/nightly-tests.txt` and `ci/sleeps-tests.txt` name is there), clippy (a warning is an error), pool models, std kernels, `std.wasm` (rebuilt to the same bytes elsewhere), mac lint (Linux: the Mac-only code
type-checked), browser suites, renders (against `e2e/renders.sha256`, one record for every OS), old engine S (with
the gates), gates (the 8 runs `ci/gates.txt` names), repo unchanged.

- Without the test plugins it says FAIL. Edit no tracked file while it runs.

**When the gates run.** On a Mac: always (`--no-gates` leaves them out). On Linux: with `--gates`, always; with no
option, never; with `--gates=auto`, unless git proves that HEAD holds, in every file the gates read or build from,
exactly what a check on this machine already passed them on. The rule (`ci/gates-auto.sh`) is mechanical:

- The change is every file `git diff --no-renames` names between `git merge-base HEAD main` (or `--base <rev>`) and
  HEAD. Every file on it must be under `web/`, `docs/`, `README.md` or `HANDOFF.md`, which m2-gates never reads or
  builds from (checked by hand, in its build's dep-info files, and by tracing every file the S build and the 7 runs
  touch: the top of `ci/gates-auto.sh`). `e2e/` isn't one: m2-gates reads `e2e/renders.sha256`.
- The list isn't empty (not main itself), and HEAD holds no symlink.
- The tree is clean: nothing uncommitted or untracked, no change git hides (skip-worktree, assume-unchanged), and no
  ignored file where the gates read (build output in `target/`, `e2e/node_modules/` and std's kernels' `target/` is
  fine).
- **A recorded pass on the same files.** Every Linux check whose gates row passes, on a clean tree, adds a hash of
  every file the gates can read to `~/.cache/m2-check/gates-passed` (the gates row says "recorded in ..."). The skip
  needs HEAD's hash there. The merge base alone proves nothing: main fast-forwards through a track's every commit, so
  the base can be a mid-track commit whose own check failed, or a parallel track's commit that landed later with its
  fix.

Anything else runs them, and the check prints its decision at the start. A skip is the row `gates SKIP only web/,
docs/ changed since <base> (N files); the gates passed at <commit> (<date>) on the same files they read`, and the
first line says "without running the gates"; it is never a PASS. The rule doesn't see what the gates take from
outside the repo (`RUSTFLAGS`, a `.cargo/config.toml` above it): gaps.md.

**The browser suites** run several at a time on Linux (`e2e/run.mjs all --jobs N`, N a quarter of the threads, 1
to 4: 4 on the server; each suite still has its own server,
`HOME`, folder and browser; what they could share was checked, see `e2e/README.md`, "In parallel"), then each failed
run once more alone, as before. On a Mac they run one at a time, as before (`M2_CHECK_JOBS=4` tries 4 there).

Times on the server, warm `target/`:

| part | before | now |
|---|---|---|
| browser suites | 10.5 to 11 min, one at a time | 2.7 min, 4 at a time |
| timing gates, with the S build | 16 min | the same when they run; none when `--gates=auto` skips them |
| rust tests | 6 min | the same |
| the whole check, with the gates | 34 to 40 min | 26 to 27 min (`b5e3a0f`, `e4a6183`) |
| the whole check, gates skipped | (no such run) | 10 min (`2bd6456`, `--base e4a6183`: only `HANDOFF.md` changed; timed under the first rule, which kept no record) |

On the server, a track's check (with a warm `target/`):

```sh
TW_WAIT=3500 $SCRATCH/tw check-<label> 'M2_CHECK_OUT=$SCRATCH/check-<label> ci/check.sh --gates=auto'
```

`--gates` instead runs the gates whatever changed.

Run it from the worktree being checked. `M2_CHECK_OUT` must be a new folder. It gets `summary.txt`, every log, and
the app's two pictures; all logs are also packed in `<folder>.tgz`.
