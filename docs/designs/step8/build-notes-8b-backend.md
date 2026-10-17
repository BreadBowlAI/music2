# 8b back ends: build notes

The `8b-backend` track builds the back ends of 8b.4 (mute, solo, mute in song), 8b.5 (clips), 8b.6 (audition) and
8b.8 (meters): engine, IR, session, Tx builders, routes, CLI and contract. No UI. The plan is
`~/m2-runs/s8b/refresh.md` (its Attack and Decisions win) on top of `design-8b.md`.

## In short

| Slice | What | State |
|---|---|---|
| B1 | The IR's `muted`, and where overlays go (`Flat.overlays`, `Flat.muted`) | built |

---

## B1. Where overlays go

**In short:** the checker now says where mute and solo act. Nothing plays it yet.

| Piece | What it does | Where |
|---|---|---|
| `Module.muted` | Mute in song, in the IR. Written only when true, so old IRs keep their bytes and hash. | `core/ir/src/lib.rs` |
| `Flat.muted` | The muted tracks, by key, sorted. Refused on `song`, and on a module that isn't a track. | `core/ir/src/check.rs` |
| `Flat.overlays` | Each place a track's audio enters another module: an input, a voice's capture, a feedback, or the song's output. | `core/ir/src/check.rs` |

**What isn't a site:**
- a control export, such as `kick.env`;
- a sidechain input: an input made inside a frame that recorded a sidechain, which reads the signal that frame got;
- a plugin's audio inputs other than `in` (PKG-13);
- a voice's capture that a sidechain input reads.

**Choices made here:**
- A site also carries `into` (the module that reads it) and `src` (what it reads). The server's solo rule needs
  `into`; the runtime needs `src`. Both come from the graph, so the list stays "same code, same list".
- `Consumer` has four cases, as the runtime's edges do: `Input`, `Capture`, `Feedback`, `Out` (A5).
- `Flat.side_inputs` isn't a field (S3). The checker works out sidechain inputs inside `check()`.
- No SDK option yet (G2): `muted: true` would be legal and do nothing until the op is built.

**Tests:** `core/ir/src/overlay_tests.rs` (new file, G4): the IR hash, the song's output, every input, the
sidechain input (with and without its record), a control export, a capture and a feedback, a track read by a
track, and `muted` refused on `song` and on a non-track. Written with the code, not before it.

**Checks:** `m2-ir`, `m2-script`, `m2-session` tests.

**Contract:** ID-11 *Today*, PKG-13 *Today*.
