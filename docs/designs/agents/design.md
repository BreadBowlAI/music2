# Agents beside the musician: design

## 1. In short

- An agent (Claude Code, Codex, a script) helps while the song is open in the app. It uses one door: `m2`.
  The commands are the same whether the app is open or closed.
- While the app has the song open, `m2` no longer refuses to write. It builds the edit, checks it, and hands
  it to the app. The app is still the only writer. This reopens owner decision 10.
- Every change is one undo step with a source: you, the agent by name, or "outside the app" (a file someone
  saved). ⌘Z takes back the newest step, whoever made it. `m2 undo` takes back only the agent's own.
- You win every clash. If you are dragging a knob, a note or the tempo, recording, or typing unsaved code in
  a file, the agent's edit is refused and nothing you hold is touched.
- You see each change as it lands: a quiet notice with Undo, and a glow on what changed. Press V to flip
  the newest change off and on, to hear before and after (D2).
- The agent can see what you look at (a focus record), follow what happens (`m2 watch`), point at things
  to teach (`m2 point`), and measure the sound (`m2 listen`).
- Listening is an offline render in its own low-priority process. A new gate counts dropouts while it runs.
- The app gets no AI. It only shares focus, shows changes and takes actions.
- Five tracks, smallest first. Three choices wait for you before track 1 (section 9).

| | Today | After |
|---|---|---|
| `m2` write while the app has the song open | refused (WRITE-18) | done in the app, one undo step |
| A file saved outside the app | taken in, not undoable | taken in, one undo step |
| An agent's change in the app | no sign, unless it failed | a notice, a glow, a name in Undo |
| Agent edit on a knob you are dragging | your drag ends | the agent is refused |
| What you are looking at | hidden from agents | `m2 focus` |
| The sound | a WAV and its peak | loudness, peaks, bands, masking, stereo |
| Actions with no `m2` command | 59 (HTTP only) | all reachable with `m2 call` |

Written 2026-10-21 on branch `ag`, from `main` at `381e02d`, against `docs/architecture.md` (normative). It
builds on the design panel's combined architecture, "one song, one writer, many hands", and on the code map
(every UI action against its route and its `m2` command, and what an agent can and can't see). Nothing here
is built yet. The build starts after track os lands on `main`.

## 2. What each side needs

| The musician needs | The agent needs |
|---|---|
| To keep playing and working while the agent works, with no glitch | One door, the same with the app open or closed |
| To see what changed, who changed it and why, with nothing in the way | To write safely: checked, refused when stale, refused in plain words |
| To take any change back with the keys they know | To take back its own last step |
| To hear before and after with one key | To measure the sound before and after, without touching playback |
| To win every clash with the agent | To know what "this" means: what the musician selected and sees |
| To learn: the agent shows where things are | To notice the musician's edits, so its plan doesn't go stale |
| No AI inside the app | To reach every action the page has, and to read the list of them |

## 3. The layers

```
 song folder (the truth) ── files ──▶ the watcher ──┐
                                                    ▼
 the page ─ HTTP/WS ─▶ the writer: the app's server ──▶ core/session: Tx, bases, delta, undo
 m2 ──── HTTP ───────▶   (while the song is open)
 m2 ──── direct ─────────────────────────────────▶ core/session (while it is closed)
```

| Layer | What it owns | What changes here |
|---|---|---|
| Song folder | The truth: code, data, layout | Nothing |
| `core/session`, `crates/editor` | Tx, bases, the delta, undo's inverses, the lock | A read-only session can hand its commits to the lock's holder (5.1). A step has a source. A settled outside batch becomes a step (5.2). |
| The writer: the app's server while the song is open, `m2` while it is closed | The revision, leases, previews, the one undo history, refusals, the focus record (memory only), the route table | Refusals for what the musician holds (5.3); `step`, `undo`, `focus` and `point` events; one route table (5.6) |
| `m2` | The agent's one door | Forwards writes; adds `undo`, `call`, `routes`, `focus`, `watch`, `point`, `listen` |
| The page | Showing and taking actions. No model. | Posts focus; shows the notice, the glow, Compare and pointers |

### 3.1 What the code says, where the panel assumed otherwise

| The panel assumed | The code says | So this design |
|---|---|---|
| `/api/tx/commit` already shows "An agent's edit" | That text appears only when an agent's commit failed after it was heard (`crates/server/src/graph/actor.rs:1797-1800`, `preview.rs:82-88`). A commit that lands shows nothing. | Adds the notice (5.4) |
| `m2 <write> --by Claude` | `--by` is taken: `m2 clips move --by <beats>` (`apps/m2/src/edits.rs:93`) | Uses `--as Claude` |
| `m2` can build the Tx and send it | Gestures plan and commit in one call: `plan::run_as` calls `Session::commit_with` (`crates/editor/src/plan.rs:79-107`). `m2 tx` and recovery fixes call `Session::commit` (`apps/m2/src/main.rs:1099`, `crates/editor/src/data.rs:52`). | Adds one seam in core: a session that forwards its commit (5.1) |
| The route table is declared once | 14 router functions (`crates/server/src/routes.rs:43-111` merges 13) and 11 write lists kept by hand (`routes.rs:129-157`, `WRITES_8B` and 9 more) | Makes the one table (5.6) |
| `--as-heard` applies the listening mute and solo | MON-4: "Overlays MUST NOT change files, revisions, exports or agent renders" (`docs/architecture.md:1224`) | Amends MON-4 (open decision D4) |
| Per-track numbers come from the render | Taps exist only in live plans, and hold peaks only (`core/runtime/src/taps.rs:1-9`, `core/runtime/src/lib.rs:2806-2810`) | Adds offline copy taps (5.9) |
| Outside edits can all be undone | A snapshot keeps binaries by hash only (`core/song/src/lib.rs:77-82`), so a replaced asset's old bytes are gone | Only text changes become steps (5.2) |
| The musician wins today | A lease ends when the code under it changes (`crates/server/src/graph/lease.rs:10-13`), so an agent's edit would end your drag | Refuses the agent instead (5.3) |

## 4. The invariants, as contract rules

New rules, in a new section "Agents" of `docs/architecture.md`, after "Writes". The prefix `AGENT` is unused
there today. Rules marked *pending* wait for the owner's answer in section 9.

**AGENT-1.** While a music2 app holds a project's lock ([WRITE-17](#the-project-lock)), an `m2` command that
writes the project MUST hand its Tx to that app instead of failing. It builds the Tx on a read-only session
from the files on disk and checks it there, under the gesture's own expectation and its planner's assertion.
It sends it to the app's `POST /api/tx/commit` with `Seen(hash)` of that check, using the url and token in
`~/.music2/session.json`. A Tx given to `m2 tx`, and a recovery fix, go as they are. It forwards only when
the lock's holder is the program `music2` with the pid that file names, and the app says this folder is the
song it has open. Otherwise it fails before changing anything, naming the holder ([WRITE-18](#the-project-lock)).
A forwarded write prints what a local one prints, and says it is one undo step in the app.
- *Why:* while the song is open there must be one writer and one history. The gesture's meaning is checked
  where the gesture is built, and core in the app commits exactly that delta, or nothing.

**AGENT-2.** Every change the app accepts while it has the song open (a Tx, a save, or a settled outside
batch, [WRITE-19](#outside-writers)), other than an undo or a redo, MUST be exactly one undo step, with a
source: the app (its page), an agent by the name it gives, or outside the app. An agent's step is named by
the words it gives. An outside batch's inverse puts back each text file the batch changed or removed, guarded
by the hash the batch left, as a save's inverse is ([WRITE-21](#undo)), and removes each text file it made.
The one exception: a batch that changes or removes a binary file (an asset, a plugin state) makes no step,
and says it can't be undone. Undo's name says the source.
- *Why:* whoever made a change, the musician can take it back with ⌘Z and can see who made it.

**AGENT-3.** Agents MUST NOT hold leases. The app MUST refuse an agent's write (a Tx sent to
`POST /api/tx/commit`, or any write route called with an agent's name) before its Tx is checked, with status
423 and a `busy` answer that names what the musician holds, when the Tx touches: a file
in which a knob lease lives; a lane file a lane lease holds; `data/arrangement.json` while a tempo lease lives;
the lane file a take is recording into; or a file the code editor has unsaved text in. Nothing changes, and
nothing the musician holds ends.
- *Why:* the musician wins every clash. Today an agent's edit would end the drag in their hand.

**AGENT-4.** `m2 undo` MUST take back the newest step only when its source is an agent of the same name, and
otherwise refuse, naming the newest step and its source. *Pending D1:* the musician's Undo takes the newest
step, whatever its source.
- *Why:* one history in one line; an agent may only take back its own work.

**AGENT-5.** Every step not made by the app's own page MUST be shown as it lands: a notice that never takes
the keyboard's focus and never blocks, with Undo while it is still the newest step, and a glow on the tracks,
lanes and knobs its delta changed, until the musician's next action.
- *Why:* trust comes from seeing every change, without being interrupted.

**AGENT-6.** The focus record and pointers are attention, not song. The page posts the focus record; `m2 point`
posts a pointer. The server keeps them in memory only and sends them as events. They MUST NOT be written to a
file, a revision or undo. A pointer MUST NOT move the musician's view or change their selection.
- *Why:* what someone looks at is not the song, and the agent must never take the musician's place.

**AGENT-7.** Agent listening MUST be an offline range render ([RENDER-4](#range-render)) in a process of its
own, on the cores playback doesn't need, at background priority, its plugin host process included, and one
at a time on a machine. It reads committed files, or a Tx it checked and never wrote. It MUST NOT touch the
live engine, the app's plugin instances or the audio thread. Gate G13 counts playback's dropouts while it runs.
- *Why:* real-time audio never glitches, whatever an agent does.

**AGENT-8.** Every HTTP route MUST be declared once, with its method, its path, whether it writes, whether
agents may call it, plain words and an example body. The router, the read-only window's 423
([WRITE-17](#the-project-lock)) and `m2 routes --json` are all made from that one table.
- *Why:* a list kept by hand drifts. Agents learn the API from it, not from the source.

**AGENT-9.** The app MUST contain no model and call none. It only shares focus, shows what changed and takes
actions. *Pending D3:* there is no "agents may change the song" switch; when an agent changes things is
decided by what the musician asks it.
- *Why:* owner rule: no AI in the app, simple yet elegant.

Each track adds the rules it builds, and changes the existing text they touch, in its own first commit (the
contract's rule: a change that breaks a rule changes the page first). Track 1: AGENT-2, AGENT-5 and WRITE-21.
Track 2: AGENT-1, AGENT-3, AGENT-4, AGENT-9, owner decision 10, WRITE-18 and Deferred. Track 3: AGENT-6.
Track 4: AGENT-8. Track 5: AGENT-7 and MON-4. The changes:
- **Owner decision 10** (`docs/architecture.md:41`): forwarding is adopted (2026-10-21), as AGENT-1.
- **WRITE-18** (`:913-930`): "Forwarding writes to the holder is deferred" becomes "A writer that finds the
  app holding the lock forwards ([AGENT-1](#agents)); a lock held by any other program fails as above."
- **Deferred** (`:1424`): the forwarding row goes.
- **WRITE-21** (`:949`): "One Tx is one undo step" gains "and so is a settled outside batch ([AGENT-2](#agents))".
- **MON-4** (`:1224`), pending D4: "…exports, or an agent render that didn't ask for them (`m2 listen
  --as-heard`, which says so)."
- **Acceptance gates**: a row "agents" with each track's gate (section 7).

## 5. Each piece against today's code

Words used below:

| Word | Meaning |
|---|---|
| step | One entry in undo's history: a stored inverse Tx (`TxStep`, `core/session/src/tx/mod.rs:280-298`) |
| source | Who made a step: `app`, `agent` (with a name) or `outside` |
| forwarding session | A read-only `Session` whose commits go to the lock's holder |
| busy | The refusal of AGENT-3 |

### 5.1 Forwarding

**Today.** Every `m2` write opens its session with `locked()` (`apps/m2/src/main.rs:266-272`). When the app
holds the lock, `Session::open_as` gives a read-only session, and `locked()` stops with WRITE-18's message
(`core/session/src/lock.rs:89-104`). Forwarding is "deferred (owner decision 10)" (`lock.rs:6-7`). The
app's `POST /api/tx/commit` exists (`crates/server/src/routes.rs:80`, `:781-786`); it previews, commits and
records the step like any edit (`actor.rs:1774-1823`; `core/session/src/tx/mod.rs:759-765`). The UI never
calls it. `~/.music2/session.json` holds `{url, token, pid}`, mode 0600, written at each launch and never
removed (`apps/music2-desktop/src/main.rs:176-190`). The lock's file names its holder `{pid, program, …}`
(`lock.rs:24-37`), and the app takes it as `"music2"` (`actor.rs:1253`).

**The seam, in core.** One small trait, used at the two places a session commits:

```rust
/// The lock's holder, which takes a read-only session's commits (AGENT-1).
pub trait Forward: Send {
    fn commit(&mut self, tx: &Tx) -> Result<Landed, Refusal>;
}
pub struct Landed { pub number: u64, pub code_rev: u64, pub data_rev: u64 }
impl Session { pub fn forward_to(&mut self, to: Box<dyn Forward>); }
```

- `commit_with` (`tx/mod.rs:715`) checks, judges and asserts as now. Where a read-only session refuses today
  (`tx/mod.rs:739`), a forwarding one sets `expect = Seen(checked.hash)` and sends the Tx. A `Journal`
  expectation (a recovery fix) is sent unchanged.
- `commit` (`tx/mod.rs:759`) sends its Tx as it is. That serves `m2 tx` (with or without `--seen`) and
  recovery fixes.
- The `Committed` it returns takes the delta, hash and diagnostics from the local check, and the revision
  numbers from the app. `wrote` is empty.
- Then the session takes in the touched paths from disk (`take_in_only`, `core/session/src/take.rs:129`). So
  a command that commits twice (import: new tracks, then clips) plans its second Tx on the first's result.
- `undo` and `redo` in a forwarding session are refused. `m2 undo` is its own route (5.5).
- The app commits a `Seen` Tx as it does today's `m2 tx --seen`, so its undo step is a raw one, guarded by
  its bases (`tx/mod.rs:282-283`).

**In `m2`.** A new module, `apps/m2/src/app.rs`, "the open app":
1. `locked()` gets `ReadOnly::Held(Some(holder))`. If `holder.program == "music2"`, it calls `app::reach`.
2. `reach` reads `session.json`, requires its pid to be the holder's, and asks `GET /api/graph`. It requires
   `dir` to be this folder (both canonical) and keeps `session`, the open song's token, which the app checks
   on every commit (`actor.rs:456-461`).
3. `locked()` then calls `s.forward_to(app)`. Every write command forwards with no change of its own: `set`,
   `automate`, `unautomate`, `label`, `add-track`, `import`, `mute-in-song`, `clips`, `duplicate-track`,
   `delete-track`, `layout`, `tempo`, `meter`, `title`, `connect`, `disconnect`, `add-effect`,
   `remove-effect`, `relink`, `delete-data`, `fix`, `plugin-state`, `tx` and `bounce --place`.
4. Requests are plain HTTP/1.1 over `TcpStream` to 127.0.0.1, as the server's tests send them
   (`crates/server/src/routes_8b_tests.rs:14-29`). No new crate.

**Commands that aren't a Tx today.** Every `m2` write command builds a Tx and reaches one of the two commit
calls above (WRITE-1). The only other writes `m2` makes are new assets, made create-only by `import` and
`bounce`; they need no lock and already work while the app is open (WRITE-18). Actions that aren't a Tx at
all (transport, listening mute and solo, the loop, view settings, recording, plugin windows, settings) have
no `m2` command today: 59 actions have a route and no command (the code map). `m2 call` reaches them (5.6).

**`--as` and `--why`.** The author is `--as <name>`, else `M2_AS`, else `m2`. The words are `--why "<words>"`.
`m2` takes both off the command line before it runs the command, because `m2 clips` refuses flags it doesn't
know (`edits.rs:191-196`). They travel in the commit's body: `TxReq` (`routes.rs:763-769`) gains `as` and
`why`. The actor names the new step with `why` (`Session::name_step`, `core/session/src/session.rs:538`) and
gives it the source `agent` with that name. Without `--why`, the step keeps today's plain name
(`crates/server/src/graph/steps.rs:31`).

**What `m2` says, case by case.**

| Case | `m2` says | Exit | Song changed |
|---|---|---|---|
| The app holds the lock, has this song open, answers | what a local write prints, then "in the open app: one undo step, Claude: brighter hats" | 0 | yes |
| The lock is free | writes the files itself, as today (no undo once it exits) | 0 | yes |
| Another `m2` holds the lock | WRITE-18's message, as today | 1 | no |
| `music2` holds it, but `session.json` is missing or names another pid | "this song is open in music2 (pid 4121), which m2 can't reach. Make the edit there, or close it there and run this again." | 1 | no |
| The app answers another folder (it is opening or closing songs) | "the app is opening or closing this song; run this again" | 1 | no |
| The app's window is read-only (423 from the gate, `routes.rs:161-169`) | the app's words | 1 | no |
| Busy (AGENT-3) | retries every 250 ms for 10 s, saying "waiting: you're dragging Bass cutoff"; then gives up with those words | 1 | no |
| Stale: a base changed (409) | the refusal JSON, as `m2 tx` prints it today, and "read again and retry" | 1 | no |
| The song changed between check and commit (`Seen` refused, 422) | "the song changed while this was planned; nothing was changed; run it again" | 1 | no |
| The app is slow (a long compile) | waits while the app's pid lives, saying "waiting for the app…" every 5 s | — | — |

**If the app quits during the call.** `m2` knows each target's base hash and, from its own check, the hash it
should have after (the candidate's snapshot). When the connection drops, it reads the targets:

| The targets are | It means | `m2` says | Exit |
|---|---|---|---|
| all at their bases | the app quit before it took the edit | "nothing was changed: the app closed first. Run this again; it will write the files itself." | 1 |
| all at the candidate's hashes | it landed, then the app quit | "it is saved, but its undo step went with the app" | 0 |
| some of each | the app was killed mid-commit; its journal is in `.music2/tx/` (WRITE-16) | the `m2 fix … finish-tx` and `discard-tx` commands, as every command prints them (`main.rs:295-316`) | 1 |

Killing `m2` while it waits doesn't take the edit back: it may still land. `m2 watch` shows it if it does.

### 5.2 Outside file edits as undo steps

**Today.** The watcher takes a settled batch in whole, as one revision (`actor.rs:1688` calls
`take_in_only`, `take.rs:129`). If its code doesn't build, the batch waits and the last good version plays
(`take.rs:12-17`). `take_in` records no step (`take.rs:103-121`). Only a save (`take.rs:221-232`) and a
Tx commit (`tx/mod.rs:750-752`, `:759-765`) record one.

**Design.** In `take_in_only`, the watcher's door, a batch that is accepted records one step: source
`outside`, raw (taken in like an outside write when undone, as a save's step is), shown by its first changed
file. Its inverse, for each path that changed between the revision before and the one after:

| The batch | Inverse op | Its base |
|---|---|---|
| changed a text file | `Write` of the text before | the hash after |
| made a text file | `Remove` | the hash after |
| removed a text file | `Write` of the text before | `Absent` |
| made a binary file (a new asset) | none: it stays, and an asset no lane names plays nothing | — |
| changed or removed a binary file | none: the batch makes no step, and its notice says "can't be undone" | — |

- **The before-image.** The revision before the batch holds each text file's text (`snapshot.files`, shared
  `Arc<str>`, not copied). The step keeps those, counted against undo's 64 MiB bound
  (`session.rs:133-137`). Binaries are kept by hash only (`core/song/src/lib.rs:77-82`), which is why a
  changed binary can't be put back.
- **Only revisions that build.** A batch that waits makes no step. When it is taken in, there is one step from
  the last accepted revision to the new one. A change to `layout.json` alone makes no revision (PLANE-3), so
  no step.
- **Never twice.** A code-editor save takes in through `take_in` (`take.rs:221`), not `take_in_only`, and
  keeps its own step. The session's own commits make no batch (WRITE-19).
- **Undo after a later edit.** History is one line, so later steps are undone first. If the file changed since
  in a way that made no step (a binary batch, a batch still waiting), the base no longer matches. Undo is
  then a `Conflict` naming the file, and nothing changes (WRITE-21).
- **Its name.** The plain name of its file (`steps.rs:31`), shown as "Changed outside the app:
  tracks/bass.ts", or "…: 3 files".

### 5.3 Refusals: what the musician holds

The check runs in `Actor::tx` (`actor.rs:1774`), before the commit and its preview. It compares the Tx's
touched paths (`Tx::touched`) with what the musician holds:

| The musician is | The server knows it from | The Tx is refused when it touches | Words |
|---|---|---|---|
| dragging a knob | `Graph::leases` (`graph.rs:1351-1354`), each with its `file` (`lease.rs:197-217`) | that file | "you're dragging Bass cutoff in the app" |
| dragging a clip, a note or automation | `Graph::lane_leases` (`graph/lanes.rs:531`) | that lane's file | "you're dragging a clip on Bass" |
| dragging the tempo | `Graph::tempo_leases` (`graph/tempo_lease.rs:365`) | `data/arrangement.json` | "you're dragging the tempo" |
| recording | the record state (`graph/record.rs:921`) | the take's lane file | "you're recording on Bass" |
| typing code not yet saved | `Graph::unsaved` (`graph.rs:1950-1954`) | that file | "tracks/bass.ts has unsaved changes in the code editor" |

From track 4, writes through other routes can carry an agent's name too (5.6); the actor then runs the same
check before each commit such a job makes. The answer is 423 with
`{"busy": {"what": "knob", "file": "tracks/bass.ts", "words": "…"}}`. A knob lease is
matched by file, not by the call it holds: simpler, and a drag lasts seconds. Agents never hold leases: `m2
call` refuses the lease routes (5.6).

### 5.4 The notice and the glow

**Today.** Toasts go to every window (`crates/server/src/lib.rs:157-170`). A toast's Undo works only while its
edit is the newest (`web/src/components/GraphCanvas.tsx:327-336`). Undo's name comes from the top step
(`steps.rs:21-27`), shown in the state as `undo_name` (`actor.rs:660-664`).

**Design.**
- After each job that records a step, whatever its source, the server sends a `step` event:
  `{"t": "step", "made": 42, "source": "agent", "by": "Claude", "name": "brighter hats", "files": [...],
  "tracks": [...], "lanes": [...], "knobs": [...]}`. `tracks`, `lanes` and `knobs` come from the commit's
  delta: modules and node names by module key, lanes changed, and changed constants turned into the view's
  knob ids. A new `crates/editor/src/changed.rs` does this, since knob ids are the editor's.
- `undo_name` gains `source` and `by`. The Undo tooltip and the Edit menu say "Undo Claude: brighter hats",
  "Undo outside change: tracks/bass.ts", or, for your own, "Undo Bass notes" as today.
- The page shows a notice for each step whose source isn't `app`: "Claude: brighter hats · Undo". Undo is
  `undoAction(session, made)`, which already refuses once a later change came. One notice at a time: a newer
  step replaces it ("Claude: 3 changes · newest: brighter hats"). It never takes the keyboard's focus.
- The glow: an outline on those track headers, clips and knobs, until your next pointer or key action in the
  song, or the next step.

### 5.5 `m2 undo`

`m2 undo <song> [--as Claude]`. With the app open, it sends `POST /api/graph/undo` with `{session, as}`
(`routes.rs:816-821`). The app takes back the newest step only if its source is `agent` with that name.
Otherwise 409: "nothing was undone: the newest change is yours (Bass notes), not Claude's". With the app
closed: "nothing to undo: undo lives in the open app" (history is in memory, `session.rs:150-154`). There is
no `m2 redo`: putting back is the musician's choice.

### 5.6 `m2 call`, `m2 routes --json`, and the one route table

**Today.** Routes are declared in 14 router functions; `routes.rs:43-111` merges the other 13. Which ones
write is kept by hand in 11 lists (`routes.rs:129-157` and `WRITES_8B`, `WRITES_AUDIO` and 8 more), and each
router repeats the 423 gate (`routes.rs:161-169`, `routes_8b.rs:41-49`, …). There is no schema (gap #38,
`docs/gaps.md:262`).

**Design.** A new `crates/server/src/api.rs` holds a `routes!` macro. Each `routes_*.rs` declares its routes
once with it:

```rust
routes! {
    POST "/api/graph/effect/bypass" => effect_bypass, writes, agents,
        "Turn an effect off or on",
        r#"{"session": 7, "id": "tracks/bass.ts#3", "on": false}"#;
}
```

The macro makes the file's router and its `ROUTES: &[Route]` table. `api::all()` joins the tables. The 423
gate becomes one middleware that reads `writes` from it; the 11 hand lists go. `m2` links `m2_server`
already (`apps/m2/Cargo.toml`), so `m2 routes --json` prints the table with no app running: method, path,
writes, agents, words, example.

`m2 call <path> [json | -] [--as] [--why]` looks up the method in the table, sends the body (a GET's keys go
in the query), prints the answer, and exits 1 on a status of 400 or more. It refuses routes marked not for
agents: the lease routes (AGENT-3), `/api/graph/preview` (a lease), the WebSocket (use `m2 watch`), opening,
closing and making songs, Save As, renaming the folder, settings, Retry, and the test-only inject routes.
`--as` and `--why` go as headers `x-m2-as` and `x-m2-why`. The `auth` middleware (`routes.rs:115-126`) reads
them, the `blocking` helper (`routes.rs:39-41`) carries them onto its thread, and `Graph::call`
(`graph.rs:909`) hands them to the actor's job, which names every step it records. Then every write route
can be named, not only `/api/tx/commit`.

### 5.7 The focus record

**Today.** The selection lives in the page (`web/src/lib/shell.ts:38-63`). Selected notes are indexes into the
clip (`NoteView.tsx:1152`), and notes have no ids. The only leak is the MIDI target (`graph.rs:768`).

**Fields**, posted by the page:

| Field | Type | Where the page has it |
|---|---|---|
| `session` | number | the open song's token: a focus for another song is refused |
| `revision` | number | the revision the page drew, so ids and clips are read against it |
| `view` | `"device"`, `"notes"`, `"mix"` or `"code"` | the dock's view (`shell.ts:40`) |
| `track` | key or null | the selected track (`shell.ts:47`) |
| `clips` | `[{lane, id}]` | the timeline's picked clips (`web/src/lib/clips.ts:156`) |
| `range` | `[start, end]` beats, or null | a marquee's bars (`clips.ts:158`) |
| `notes` | `{clip, notes: [{pitch, start, length}]}`, or null | the roll's selected notes, named by what they are, not by index |
| `knob` | knob id, or null | the knob last touched |
| `seen` | `[start, end]` beats | the bars the timeline shows |
| `code` | `{file, line}`, or null | the Code view's file and cursor line |

The server adds, when it serves the record: `playing`, `beat` and `loop` (its status, `routes.rs:291`),
`unsaved` (`graph.rs:1950-1954`) and the MIDI target.

- **Who posts it, and when:** the page, with `POST /api/focus`, 200 ms after the last change to any field, and
  once when its socket connects. It is not a write, so it works read-only.
- **Memory only:** kept beside `unsaved` in the server (`graph.rs:417`), dropped when the song closes or
  another opens. The newest post wins.
- **Event:** `{"t": "focus", "focus": {...}}` to every socket on each post. `GET /api/focus` and `m2 focus`
  return it.

### 5.8 `m2 watch`

`m2 watch [--meters]` opens `/api/ws?token=…` (`routes.rs:185-236`) and prints one JSON object a line, `t`
first. The WebSocket client is `tungstenite` 0.29.0, already in `Cargo.lock`.

| `t` | Made by | Says |
|---|---|---|
| `step` | new (5.4) | a change landed: who, why, files, tracks, lanes, knobs |
| `undo`, `redo` | new, from the undo and redo routes | which step went or came back, and its source |
| `graph` | today (`lib.rs:181-188`) | the revision, problems, diagnostics, leases |
| `focus` | new (5.7) | the musician's focus |
| `point` | new (5.11) | a pointer shown or cleared |
| `toast`, `export`, `take`, `midi` | today | as the page hears them |
| `transport` | `m2` itself | `playing`, `beat`, `loop`, when playing or the loop changes, or the playhead jumps |
| `meters` | today, only with `--meters` | about 30 a second |
| `closed` | `m2` itself | the app went away; then `m2 watch` exits 0 |

Transport changes send no event today (`routes.rs:850-890`); the page polls status every 200 ms
(`GraphCanvas.tsx:59-63`). `m2 watch` polls `/api/graph/status` every 250 ms the same way, so the server
needs no change for it.

### 5.9 `m2 listen`

```
m2 listen <song> [--bars 9-16] [--tracks kick,bass] [--as-heard] [--with tx.json] [--png out.png] [--json]
```

**Measurements**, for the mix and for each track (the track's own sound, before its mixer channel, as a stem
is):

| Measure | How |
|---|---|
| Loudness, integrated and short-term max | ITU-R BS.1770, gated as EBU R128 |
| True peak | 4× oversampled, BS.1770 |
| Crest factor | peak minus RMS, in dB |
| 10 octave bands, 31.5 Hz to 16 kHz | biquad filters: no FFT, no new crate |
| Masking | for each band, pairs of tracks within 6 dB of each other and of that band's loudest |
| Stereo | correlation; width (side against mid); whether the lows under 120 Hz are mono |
| Clipping and silence | samples over 0 dBFS; tracks silent in the range |

**How it renders.** A range render (`Render::range`, `crates/editor/src/render.rs:282`) from the bars asked
for (default: the loop if one is on, else the song), with 2 bars of preroll. It says it is approximate when
it is (RENDER-5). For the tracks, an offline plan gets copy taps at the places live meters tap today (each
track where its audio enters another module, `taps.rs:1-9`): after each block, each tapped buffer is copied
for the analysis. Taps only read buffers, so a test checks every pure fixture renders the same bits with and
without them.

**Flags.**
- `--with tx.json` checks the Tx on a read-only session and writes nothing. It renders the song as it is and
  the candidate (a `Revision` is a `Song`, `crates/editor/src/faults.rs:23`), and reports both and the
  difference.
- `--as-heard` applies the listening mute and solo, read from the app (or `.music2/overlays.json`), through
  the overlay cells the plan already has (MON-1). The result says `"as_heard": true`. Pending D4.
- `--png` writes a picture of the bands over time (third-octave rows, 50 ms columns), one block per track
  with `--tracks`, with the `png` crate already in `Cargo.lock` (0.17.16).

**Real-time safety.**
- Its own process. It never talks to the app's engine.
- Its render threads use `Threads::Beside`: the slow cores, at background priority (`render.rs:72-73`; nice 10
  on Linux, `core/os/src/linux/mod.rs:131-148`; QoS background on a Mac, `core/os/src/mac/mod.rs:161-168`).
  `m2 render` already does this when the app holds the lock (`apps/m2/src/main.rs:167-176`).
- New: its main thread and its plugin host process get background priority too. A song with plugins renders
  on its own instances in offline mode, never the app's (RENDER-1).
- One at a time on a machine: a second `m2 listen` waits on a lock file, `~/.music2/listen.lock`.
- **Gate G13**, `m2-gates realtime --beside listen`: G12's live run (`crates/gates/src/realtime/live.rs:1-18`)
  on p50, while `m2 listen` runs back to back on p50. Judged on the reference Mac: no dropout, callback p99 at
  most 70% of its period. On Linux the counts are recorded (PLAT-1).
- The agent reports numbers as numbers. It never says it heard anything, and it asks you to press V.

### 5.10 The Compare key (pending D2)

V, for "versus". The page's single keys are taken by c, a, e, x, b, m, s, z, k and r
(`web/src/App.tsx:69`); V is free. When the newest step isn't yours, V undoes it, and a pill says "Before:
Claude: brighter hats". V again redoes it: "After". Esc, or any other action, closes the pill and keeps what
plays. If it closes on Before, the change stays undone, and Redo still has it until your next edit. Each flip
is an undo or redo Tx. A data change is heard within 20 ms (PLANE-5); a code change after its compile,
through an activation that keeps every instance (RENDER-25). Neither glitches.

### 5.11 `m2 point`

`m2 point <song> <target> "<words>"`, or `--clear`. A target is `track:bass`, `clip:bass/notes:c3`,
`lane:bass/cutoff`, `knob:<id>` or `bars:9-16`. It sends `POST /api/point`, which isn't a write. The page
shows a small note at the target, with OK to dismiss it. It never scrolls, zooms or selects. When the target
is off screen, the note waits at the edge with "Show", and only that click moves the view. One pointer at a
time, memory only, and the `point` event tells `m2 watch`.

## 6. Out of scope

- Any model, chat or AI in the app.
- An unwritten candidate with Keep and Back (the panel's `m2 try`): a second world in every view.
- Taking back an older step on its own (selective undo), or one undo history per author.
- Undo that survives closing the song.
- Agents working plugin windows, holding leases, or previewing live.
- Streaming live audio, screenshots, or live analysis to the agent.
- MCP and `m2 describe`: later, as thin wrappers over these commands.
- Several agents at once beyond their names and the refusals.
- Reaching the app from another computer. The server stays on 127.0.0.1.

## 7. Build order

Before track 1, the owner answers D1 to D3 (section 9). Each track is one branch, merged to `main`, with one
full check (`ci/check.sh`, `music2 check: PASS` on Linux) at its end. Every test below is written first and
shown failing before its code lands. "Proves" says what the full check then shows.

| Track | What | Waits for | Size |
|---|---|---|---|
| 1 `ag-steps` | Outside edits become steps; a step's source; the `step` event; the notice and the glow; Undo's name; the README fixes | D1 | small |
| 2 `ag-fwd` | Forwarding, `--as`, `--why`; busy refusals; `m2 undo`; the contract's changes | D1, D3 | medium |
| 3 `ag-see` | The focus record, `m2 focus`, `m2 watch`, `m2 point`; the Compare key | D2 | medium |
| 4 `ag-routes` | The one route table, `m2 routes --json`, `m2 call`, `x-m2-as` on every write | — | medium, mostly mechanical |
| 5 `ag-listen` | `m2 listen`, copy taps, `--with`, `--as-heard`, `--png`; G13 | D4 | large |

**Track 1, `ag-steps`.** Tests:
- `core/session`: `an_outside_edit_that_builds_is_one_undo_step`, `an_outside_batch_that_waits_is_one_step_once_it_builds`,
  `undoing_an_outside_edit_puts_back_its_text`, `undoing_an_outside_edit_after_a_change_without_a_step_conflicts`,
  `an_outside_binary_change_makes_no_step`, `a_code_editor_save_is_still_one_step`,
  `the_sessions_own_commit_is_not_an_outside_step`.
- `crates/server`: `undo_says_the_source_of_the_newest_step`, `a_step_event_names_what_its_delta_changed`.
- `crates/editor`: `changed_names_tracks_lanes_and_knobs` (`changed.rs`).
- Browser suite, `e2e/ag-notice.mjs`: an outside save shows one notice; its Undo works while it is newest and
  says why once it isn't; the track glows until a click.
- Proves: the Rust tests, clippy and the browser suite pass; the offline renders still equal
  `e2e/renders.sha256` (no render changed); undo's step counts in the existing tests keep their meaning.

**Track 2, `ag-fwd`.** Tests:
- `core/session`: `a_forwarding_session_sends_its_checked_tx_as_seen`, `a_recovery_fix_is_forwarded_as_it_is`,
  `after_forwarding_the_session_reads_what_landed`, `a_forwarding_session_refuses_undo`.
- `apps/m2/tests/forward.rs`, against a headless server (`crates/server/examples/headless.rs`) with `HOME` set
  to a temp folder: `a_write_while_the_app_holds_the_lock_lands_in_the_app`, `every_write_command_forwards`
  (one row per command in 5.1), `the_step_is_named_by_as_and_why`,
  `a_song_held_by_an_app_m2_cant_reach_fails_naming_it`, `a_write_on_a_file_with_a_knob_drag_is_busy_and_the_drag_goes_on`,
  `a_write_to_a_file_with_unsaved_editor_text_is_busy`, `a_write_to_the_recording_lane_is_busy`,
  `the_app_quitting_before_the_commit_changes_nothing`, `the_app_killed_mid_commit_names_its_fixes`,
  `the_app_quitting_after_the_commit_says_it_landed`, `m2_undo_takes_back_only_its_own_step`,
  `m2_undo_with_the_app_closed_says_undo_lives_in_the_app`.
- `apps/m2/tests/lock.rs:52`: `a_write_while_another_process_holds_the_lock_fails_and_changes_nothing` keeps
  its meaning. Its holder is a lock taken as `music2` with no app behind it (`lock.rs:59`), so `m2` can't
  reach it and fails, naming it; only the words change. Every `m2` test sets `HOME` to a temp folder, so a
  real app's `session.json` is never read.
- Proves: every `m2` write command works with the app open and lands as one named step; the musician's
  leases and unsaved text are never touched; track 2's contract changes (section 4) are in.

**Track 3, `ag-see`.** Tests:
- `crates/server`: `focus_is_memory_only_and_goes_with_the_song`, `a_focus_for_another_song_is_refused`,
  `a_point_is_not_a_write`.
- `apps/m2/tests/watch.rs`: `m2_watch_prints_one_json_line_an_event`, `m2_watch_ends_when_the_app_quits`,
  `m2_focus_reads_what_the_page_posted`.
- Browser suite, `e2e/ag-focus.mjs`: selecting a track and two notes is what `m2 focus` says, by key and by
  note; focus changes no file; a pointer never moves the view or the selection; off screen it waits at the
  edge; V flips the newest outside step while the loop plays, and the pill says Before and After.
- Proves: an agent can see the musician's focus and follow every change, and nothing of it reaches the song.

**Track 4, `ag-routes`.** Tests:
- `crates/server`: `every_route_is_declared_once` (the router's paths are the table's, and no hand list is
  left), `every_example_body_reads` (each example, posted to a fixture's server, is never refused for its
  shape), `a_read_only_window_answers_every_write_423` (every route the table marks `writes`).
- `apps/m2/tests/call.rs`: `m2_routes_lists_every_route`, `m2_call_reaches_an_action_with_no_command` (an
  effect's bypass), `m2_call_refuses_the_lease_routes`, `m2_call_names_its_step`.
- Proves: every UI action that has a route is reachable from `m2`, and the API describes itself.

**Track 5, `ag-listen`.** Tests:
- `core/runtime`: `copy_taps_change_no_bit` (every pure fixture, with and without).
- `crates/editor` (a new `listen.rs`): `a_1_khz_sine_at_minus_23_dbfs_reads_minus_23_lufs` and the other EBU
  Tech 3341 cases, `true_peak_finds_the_peak_between_samples`, `pink_noise_has_flat_octave_bands`,
  `two_tracks_in_one_band_are_named_as_masking`, `mono_correlates_1_and_inverted_minus_1`.
- `apps/m2/tests/listen.rs`: `m2_listen_reports_every_track_and_the_mix`, `m2_listen_with_a_tx_writes_nothing`,
  `m2_listen_as_heard_applies_the_listening_mutes_and_says_so`, `m2_listen_png_writes_a_band_picture`,
  `m2_listen_runs_at_background_priority` (its threads and its plugin host process),
  `a_second_listen_waits_for_the_first`.
- Gate G13 (`m2-gates realtime --beside listen`, a line in `ci/gates.txt`). Shown failing first on the Mac
  with `M2_LISTEN_UNSAFE=1`, which makes `m2 listen` use every fast core at normal priority.
- Proves: the measurements match known signals; listening changes no file and no render; on the Mac, p50
  plays with no dropout while it runs.

## 8. The README mistakes to fix in track 1

| # | Where | It says | The code |
|---|---|---|---|
| 1 | `README.md:651` | `tx` stops while the app has the song open | `m2 tx --check` opens read-only and works (`apps/m2/src/main.rs:1082`) |
| 2 | `README.md:629` and `:723` | `m2 clips` takes 8 gestures; `/api/graph/clips` takes 9 | `m2 clips json` (`edits.rs:107-114`) and the route take every gesture: also notes, points, audio and set (`crates/editor/src/clips.rs:60-150`) |
| 3 | `README.md:693-744` | the route table, "the routes scripts use most" | leaves out effect bypass, move and sidechain, song key, markers, view, step, instrument, rekey, presets, plugins, plugin windows, recent, Save As, rename, leases, MIDI files, stems, sample, audio, record and settings. Track 4 replaces the table with a pointer to `m2 routes`. |
| 4 | `README.md:627` (and other rows) | a CLI edit is "one undo step" | undo lives in the app's memory (`session.rs:150-154`); `m2` has none once it exits |
| 5 | `README.md:748` | "Or just edit the files" | doesn't say those edits can't be undone. After track 1 they can, if they are text. |
| 6 | `crates/server/src/routes_view.rs:55-56` | the view route takes 6 keys | it takes 8: `metronome` and `guide` too (`crates/server/src/graph/view_file.rs:25`) |

Two more, found while checking: `README.md:746` says the WebSocket has "three events" (it has more: `meters`,
`midi`, `take`, `graph`'s fields); and `README.md:230` says `.music2/` holds undo (it doesn't). Both are
fixed in track 1 too.

## 9. Decisions for the owner

| # | The question | Recommended | Why |
|---|---|---|---|
| D1 | One shared undo line? Your ⌘Z can take back Claude's newest change. Claude can take back only its own. | Yes | One history, nothing to keep in step. The notice and Undo's name say whose step is next. |
| D2 | How do you hear before and after? Claude makes the change, and you press V to flip it. Or a second "trial" song with Keep and Back. | Write, then V | One song: what you see is what you hear, and every state is saved. |
| D3 | A switch in the app, "agents may change the song"? | No switch | When Claude changes things is decided by what you ask it. The lock and the token already keep other programs out. |
| D4 | May `m2 listen --as-heard` apply your listening mute and solo? This changes MON-4. | Yes | You ask "why does this sound muddy?" about what you hear. It is never the default, and it says so. |
| D5 | A file saved outside the app that replaces an audio file or a plugin's state can't be undone (its old bytes are gone). Keep a copy of every such file to make it undoable? | No copy | Agents edit text. Copying every changed asset costs disk and time for a rare case. |
