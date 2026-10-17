# Polish: build notes

What each part of the polish track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/s8c/plan.md`. Each part writes only under its own heading.

## P0

The lead's seams.

## Lane A

Mix and sounds.

## Lane B

Timeline: zoom, follow the playhead, track heights, markers, repeat a clip. Branch `polish-b`,
worktree `wt/pl-b`.

There was no P0 when this lane started. Lane B built the seams it needs itself: the view store
(P0b-2), its keys and menu rows (P0a's, for its own ids only), in its own files where it could and
in small blocks of the shared ones.

### View settings kept per song (P0b-2's view store)

**In short.** The app can now keep how you look at a song: the zoom, the track heights and so on.
They are kept in the song's own `.music2/view.json`, on this computer only. They come back when the
song opens again.

| What | How |
|---|---|
| Where | `.music2/view.json`, beside the loop it already kept. Never shared, never an undo step |
| Which settings | `zoom`, `follow`, `heights`, `hidden`, `grid`, `audition`. Any other name is refused |
| Read-only window | The settings work, but only in memory: the file is the writer's |
| Two writes at once | One lock in the server, and each write has its own temporary file: no write loses another's |

**What changed**

- `crates/server/src/graph/view_file.rs`: `set(dir, changes)` merges a map in (null takes a key
  out), under one lock with a temporary name per write; `write` (the loop) goes through it.
  `local(dir)` reads the page's keys. `Graph::set_view(session, changes)` lives here.
- `graph.rs`, `actor.rs`: the published project holds `local`, read from `view.json` at the open;
  the state carries it as `local`.
- `crates/server/src/routes_view.rs` (new): `POST /api/graph/view {session, set}`. Not a write.
- `web/src/lib/local.ts` (new): `useLocal(key, fallback)`, `getLocal`, `setLocal(key, value)`,
  `flushLocal()`. Seeded from the state's `local` when the song changes. Sent 300 ms after the last
  change; sent at once before Close Song and on `pagehide` (X37).

**Tests.** Rust, `crates/server/src/graph/view_file_tests.rs`:
`view_settings_merge_and_keep_the_loop` (a set, a null, a refused `loop` and unknown key, another
session refused, kept over a reopen) and `a_read_only_window_keeps_view_settings_in_memory`.
`cargo test -p m2-server --lib view_file`: 2 passed. Clippy of `m2-server`: clean.

**Red first.** With `set_view` a stub that refuses:
`panicked at crates/server/src/graph/view_file_tests.rs:43:95` and `:75:87` (the set is refused).
The old server answers the route 404.

**Left.** Lane C's grid and audition, and lane D's hidden lanes, can use `useLocal` and `setLocal`
with their keys. A song opened from the desktop's Open while a setting waits its 300 ms loses it.

**For gaps.md.** View settings are kept per song (Q16): zoom, follow and heights so far.

**Contract.** FMT-20 and the HOME table: `view.json` also keeps `zoom`, `follow`, `heights`,
`hidden`, `grid`, `audition`, set through `POST /api/graph/view`.

## Lane C

Songs and tracks.

## Lane D

Notes and automation.

## Merge

The lead's merges of the four lanes.

## End

The lead's last check, docs and pictures.
