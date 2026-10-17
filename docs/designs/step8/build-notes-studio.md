# Studio: build notes

What each part of the studio track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/sst/plan.md`. Each part writes only under its own heading.

## P0

The lead's ground: written by the lead.

## Lane A

Audio in the app: written by lane A.

## Lane B

Record what you play: the count-in, the recorder, the Record button, and which MIDI keyboard plays. Branch
`studio-b`.

### B1. The count-in, in the engine

**In short**

| What | Now |
|---|---|
| A count-in | The engine can click one bar, then play the song. The first click is higher and louder |
| When the song starts | Exactly one bar after the first click, on the next block line |
| Space, Stop, a jump or an export during the count | The count stops. Nothing more clicks |
| The metronome off | The count still clicks. The song then plays with no click |
| Exports | Never count in. Nothing they make changes |

Nothing in the app uses it yet: that is B2 and B3.

**What changed**

- `core/runtime/src/click.rs`: `Click` gains a count (plain fields: beats, frames per beat, frames done, its lead-in,
  its length, and an end flag). `count_in(beats, frames_per_beat)` lays the bar so it ends on a block line: click
  `k` at `round(k × frames_per_beat)` after the first, the bar `round(beats × frames_per_beat)` frames, the first
  click `ceil(bar / BLOCK) × BLOCK − bar` frames into the first block (D27). `mix` makes the count's blips from
  its own frames, the first accented; `counting()`, `cancel_count()`, and `count_ended()`, true once, for the block
  the bar ends with.
- `core/runtime/src/engine.rs`: `Msg::CountIn { beats, frames_per_beat }`, applied at once as a transport command
  is. A Play, a Stop or a seek read while counting cancels it (D6); so does a hold, and a count read while held does
  nothing. The engine never idles while counting. After the click's mix, a count that ended starts the transport
  (`Cmd::Play`), so the song's first frame is the block after the bar.
- `core/play/src/lib.rs`: `Player::count_in(beats, frames_per_beat) -> Result<u64>` (its seq: `applied` says the
  block it started at). The `CountIn` arm where nothing plays.

**Tests**

- `core/runtime/src/click/tests.rs`: `a_count_in_clicks_one_bar_then_plays` (120 bpm 4/4 stopped at beat 8, the
  metronome on: clicks at 0, 24,000, 48,000, 72,000; at frame 96,000 the transport plays from beat 8's frame, and
  its downbeat clicks there, accented), `a_count_in_follows_the_meter` (3/4: the first click 64 frames in, the bar
  ending on the line at 72,064; 6/8: six clicks 12,000 apart; a step of 20,000.4 frames rounds each click),
  `the_count_accents_its_first_click`, `stop_cancels_a_count_in` (a Stop, a seek and a hold),
  `play_while_counting_cancels_the_count` (D6: no second Play at the bar's end), `a_count_in_wakes_an_idle_engine`.
- `core/play/tests/no_alloc_count.rs`: `the_count_in_never_allocates` (the count, the song after it, and a second
  count stopped half way, under the trap).

**Red first** (`Click::count_in` a stub that does nothing):

```
---- click::tests::a_count_in_clicks_one_bar_then_plays stdout ----
panicked at core/runtime/src/click/tests.rs:228:5: playing once the bar is over
---- click::tests::a_count_in_follows_the_meter stdout ----
assertion `left == right` failed  left: []  right: [64, 24064, 48064]
---- click::tests::play_while_counting_cancels_the_count stdout ----
assertion `left == right` failed: no click after the Play  left: []  right: [0, 24000]
```

**Guards:** `no_count_in_changes_no_bit` (no count, or one cancelled in its first block before its first click: the
export's bits). `the_count_in_never_allocates` is a guard too: the code it watches never allocated.

**Checks:** `cargo test -p m2-runtime -p m2-play`, clippy of both; the renders: 10 of 10.

**What's left:** nothing of B1.

**For 8a's merge**

| File | Block |
|---|---|
| `core/runtime/src/click.rs` | The count's six fields at `Click`'s end; `count_in`, `counting`, `cancel_count`, `count_ended` after `is_on`; in `mix`, `&& !self.counting()` in the first test and the one `if self.counting()` branch before `if self.on` |
| `core/runtime/src/click/tests.rs` | Seven tests at the file's end |
| `core/runtime/src/engine.rs` | `Msg::CountIn` at the enum's end and in `structural()`; in `apply_now`, the cancel in the `Transport` arm and the two `CountIn` arms; the hold's arm in `try_apply` (a hold cancels the count) and `CountIn` in its last arm; in `render`, `&& !self.click.counting()` on the idle check and on the idle-after-silence check, and the three lines after `self.click.mix(…)`. Nothing in the `Err(e)` arm |
| `core/play/src/lib.rs` | `Player::count_in` after `Player::click`; the `CountIn` arm in `take_held`'s inner match |
| `core/play/tests/no_alloc_count.rs` | New |

**For gaps.md:** #19's count-in: built in the engine (B1), used by Record (B3).

**Contract:** RENDER-28: the count-in is plain fields in `Click`, no allocation; it never reaches an export
(RENDER-1, RENDER-3), as the metronome doesn't.

## Lane C

MIDI files and stems: written by lane C.

## Lane D

Plain-language help: written by lane D.

## Merge

Written by the lead.

## End

Written by the lead.
