# Live feeds: what track `feeds` built

**In short**
- Part 1 of the [live feeds note](design.md) is built: an agent writes a track's notes ahead of the playhead, and music2 plays them, says how full the buffer is, and refuses what comes too late. Rules FEED-1 to FEED-8 in the [contract](../../architecture.md#feeds), and the AGENT-11 change: an agent may open a feed.
- The engine change is the one the note asked for: a notes lane's scheduler has a second source, its feed, swapped at a block line as `LaneSwap::Feed`.
- A few words and limits differ from the note, each for a reason below. Parts 2 to 4 (the web) are not started.

## What was built

| Part | Where | What |
|---|---|---|
| Engine | `core/runtime/src/sched.rs` | `NoteScheduler`'s feed half: `swap_feed`, `NotesData::feed` (notes in song beats, keyed by pitch and start, `ClipKey::Feed`), its own cursor, reconciled only when it was swapped, sought at every jump, its room fixed (`FEED`: 128 sounding, 64 starting in 128 frames) beside the clips'. A grown scheduler takes the feed with it (`take_state`). `Renderer::swap_lane` takes `LaneSwap::Feed` |
| Server | `crates/server/src/graph/feeds.rs` | each feed's notes and written stretches on the control plane; put, open, close, keep; the lock; what played; the events; each settle sends a feed to a new plan or a new tempo (`settled`) |
| Thread | `crates/server/src/lib.rs` | `music2-feeds`, every 50 ms: keeps what played, tells `low`, `empty`, `jump` and the quarter-second state |
| Routes | `crates/server/src/routes_feed.rs` | `POST /api/feed/open`, `put`, `keep`, `close`, `GET /api/feed`, open to agents; only Keep writes |
| `m2` | `apps/m2/src/feed.rs` | `m2 feed open`, `put`, `status`, `wait`, `keep`, `close` |
| Page | `web/src/components/Live.tsx`, `web/src/lib/feeds.ts`, `Tracks.tsx` | the track menu's **Let an agent play this track**; the header's Live mark (Live · Claude) with Keep what played and Close |
| Docs | `docs/guide/live.md`, `templates/song-agents.md` | the guide's Playing live page; one row in every new song's `AGENTS.md` |

Tests (each names its rule): `sched.rs`'s `feed_tests`, `a_feed_crosses_an_activation_with_its_scheduler` (`tests/live/keep.rs`), the feed swaps in `no_alloc.rs`'s run, `crates/server/src/graph/feeds_tests.rs` (13), `the_feed_routes_are_open_to_agents`, Keep in `every_change_answers_its_revision_step_and_moved`, `apps/m2/tests/feed.rs` (3) and the browser suite `live` (23 checks, PR tier).

"Sample-accurate" is checked on the live engine's own trace (`Renderer::trace`, as the lane leases' tests read it): a driven player, the test's thread its audio thread, each On's transport frame equal to its beat's frame.

## Departures from the note

| The note | Built | Why |
|---|---|---|
| `m2 feed keep <track> [--as clip\|version]`, the route's `"as"` | `--into KIND`, the route's `"into"` | `--as` and a body's `as` name the agent ([AGENT-10](../../architecture.md#agents)); `--as clip` would name an agent "clip" |
| `wait … [--for low\|empty\|jump] [--timeout <seconds>]` | `[--for EVENT] --timeout <seconds>` | the usage is the grammar (AGENT-13), which reads `a\|b` as place words; and the time limit is required, so no wait waits for ever |
| `wait` waits for the next event | it answers at once when the feed is already low (or empty); `--for low` takes `empty` too | `low` is said once until the buffer is above the mark again: an agent whose put didn't lift it there would wait out its time limit |
| events say `low`, `empty`, `jump` | also `open` and `closed`, and each carries `lock`, `notes`, `late`, `writer`, `playing`, `heard` | the page and an agent learn a feed opened or closed the same way; `heard` is false when the feed's lane plays nowhere |
| the buffer is written to minus the playhead | inside a loop that is on it goes round the loop, up to a lap, and the low mark is at most the loop's length | a feed over a loop is a live loop: a 2-bar loop all written must not say `low` every time round |
| notes more than 16 bars behind are dropped | not inside a loop that is on | a 32-bar loop would lose its first bars every time round |
| a put replaces what the range held past the lock | and keeps a note sounding inside a loop, with the put's note at the same pitch and start counted late | inside a loop the beats before the playhead are written for the next time round, so the lock alone doesn't keep a sounding note |
| what played: each note from the lock on | each note the playhead passed; a beat played again (a loop) keeps what played there last | a note between the lock and the playhead can still be closed away; a loop rewritten each time round would otherwise layer every pass |
| Keep into a new clip | and those notes leave the feed, and what played | the clip plays them now: left in the feed they would sound twice |
| the musician opens a feed for "Claude" | "Live" with no writer until an agent writes | the page doesn't know which agent will |
| a put needing more room is not described | refused in words (128 at once, 64 starting in 128 samples) | the feed's room is fixed when the scheduler is made, so the audio thread never grows it |

## After the review

A fresh reviewer read the branch; its findings, and what was done:

| Finding | Done |
|---|---|
| `wait --for low` answered at once, for ever, on a loop shorter than the low mark | the state says `low_mark` (capped by the loop), and `wait` judges by it; the guide says to write inside a loop (`wait_on_a_short_loop_all_written_waits`, `a_short_loop_all_written_is_never_low`) |
| the limits didn't bound a feed, and heavy work ran under the control plane's lock | at most 4,096 notes a feed (refused in words) and 65,536 kept as what played; what played is kept by start and by arrival, so a pass replaces a range and the oldest go first, never a scan of all; notes behind leave in 4-bar steps, so a look sends nothing most of the time. Under the lock a put sorts and builds at most 4,096 notes |
| a lane's readers were sized for its clips alone | `LanePlan::room`: the clips' room and the feed's, for every reader; a kernel's `cap` is 1,196 at its smallest (`a_full_lane_and_a_dense_feed_lose_nothing_through_a_plan`) |
| the compile test was void here | `a_put_never_waits_for_the_session` holds the actor busy, no clock; the old test, with its 20 ms bound, is in the nightly tier unchanged |
| a second writer didn't reach the page | the put route tells the page whenever the writer changes (`a_new_writer_is_told_to_the_page`) |
| a huge `--timeout` panicked | at most a day, refused in words |
| a feed was timed in the accepted tempo | in the tempo of what plays, `PlaysView::timing` (`a_feed_is_timed_in_the_tempo_that_plays`, with Compare's Before) |
| a resend never checked the room | one too dense for the tempo is sent empty and says `silent` (`a_feed_too_dense_for_a_new_tempo_goes_silent_and_says_so`) |
| Keep could take out a note written since | only notes exactly as kept leave the feed, and what played by its own ids (`keep_leaves_a_note_written_since_at_the_same_beat`) |
| state changed before a send that failed | open, put and close change the feed only after the send went (`a_put_that_cant_be_sent_changes_nothing`) |
| `m2_listen_never_hears_a_feed` couldn't fail | it now also keeps the notes and sees `m2 listen` measure them: a feed that reached a file would be heard |
| a feed could open on a module with no header | only a track the timeline shows with a header takes one (`only_a_track_with_a_header_takes_a_feed`) |
| route work under the lock on async workers | `GET /api/feed` and a put's writer check run in `blocking` |

## What is left

- Parts 2 to 4, the web (gap #64).
- The feed's notes don't show on the timeline or in the roll: heard, not seen, until kept.
- One feed a track; a track with two notes lanes needs `lane` named on the route (`m2 feed open` and the track menu can't name it).
- A feed is keyed by its track: renaming the track's key leaves it playing nowhere (`heard: false`) until it is closed and opened again.
- The events come from a 50 ms look, so `low` and `empty` are said up to about 50 ms after they happen.
- The design's protocol `feed/1`, written up for other players, waits for Part 4.
