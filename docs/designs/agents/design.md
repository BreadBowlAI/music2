# Agents beside the musician: design

## 1. In short

- An agent (Claude Code, Codex, a script) helps while the song is open in the app. It uses one door: `m2`.
  The commands are the same whether the app is open or closed.
- While the app has the song open, `m2` no longer refuses to write. It builds the edit, checks it, and hands
  it to the app. The app is still the only writer. This reopens owner decision 10.
- Every change is one undo step with a source: you, the agent by name, or "outside the app". ⌘Z takes back
  the newest step, whoever made it. `m2 undo` takes back only the agent's own (D1).
- You win every clash with a change that comes through `m2`. If you are dragging a knob, a note or the
  tempo, recording, comparing, or have unsaved code in a file, the agent's change is refused. If you start a
  drag while its change is landing, the app takes the change back. Nothing you hold ends.
- So with the app open, agents change files with `m2 edit` and `m2 write`. A file an agent writes itself is
  still an outside write, as today: one undo step, but the app can't refuse it, so it can end your drag.
- The app tells you and an agent apart by key: the page has its own, and agents use the one in
  `session.json`. Agents can't play, record, solo, move your view or open plugin windows (D6).
- You see each change as it lands: a quiet notice with Undo, and a glow on what changed. Press V to flip
  the newest change off and on, to hear before and after (D2). While you compare, agents wait.
- The agent can see what you look at (a focus record), follow what happens (`m2 watch`), point at things
  to teach (`m2 point`), and measure the sound (`m2 listen`).
- Listening is an offline render in its own low-priority process. A new gate counts dropouts while it runs.
- The app gets no AI. It only shares focus, shows changes and takes actions.
- Five tracks, smallest first. Three recommendations wait for your yes before track 1 (section 9).

| | Today | After |
|---|---|---|
| `m2` write while the app has the song open | refused (WRITE-18) | done in the app, one undo step |
| A file saved outside the app | taken in, not undoable | taken in, one undo step |
| An agent's change in the app | no sign, unless it failed | a notice, a glow, a name in Undo |
| An agent's change (HTTP or `m2`) to a knob you are dragging | your drag ends | the agent is refused; your drag goes on |
| An agent writing that file directly | your drag ends | your drag still ends: so agents use `m2 edit` |
| The page and an agent | one key: the app can't tell them apart | two keys |
| What you are looking at | hidden from agents | `m2 focus` |
| The sound | a WAV and its peak | loudness, peaks, bands, masking, stereo |
| Actions with no `m2` command | 59 (HTTP only) | song changes: `m2 call`; your controls stay yours |

Written 2026-10-21 on branch `ag`, from `main` at `381e02d`, against `docs/architecture.md` (normative).
Revision 2: the first draft revised after a skeptic's review (section 10). It builds on the design panel's
combined architecture, "one song, one writer, many hands", and on the code map (every UI action against its
route and its `m2` command, and what an agent can and can't see). Nothing here is built yet. The build
starts after track os lands on `main`.

## 2. What each side needs

| The musician needs | The agent needs |
|---|---|
| To keep playing and working while the agent works, with no glitch | One door, the same with the app open or closed |
| To see what changed, who changed it and why, with nothing in the way | To write safely: checked, refused when stale, refused in plain words |
| To take any change back with the keys they know | To take back its own last step |
| To hear before and after with one key | To measure the sound before and after, without touching playback |
| To win every clash, and to keep their hands on play, record and the view | To know what "this" means: what the musician selected and sees |
| To learn: the agent shows where things are | To notice the musician's edits, so its plan doesn't go stale |
| No AI inside the app | To reach every song change the page can make, and to read the list of them |

## 3. The layers

```
 song folder (the truth) ── files ──▶ the watcher ──┐
                                                    ▼
 the page ─ page key ──▶ the writer: the app's server ──▶ core/session: Tx, bases, delta, undo
 m2 ─────── tools key ─▶   (while the song is open)
 m2 ──── direct ──────────────────────────────────▶ core/session (while it is closed)
```

| Layer | What it owns | What changes here |
|---|---|---|
| Song folder | The truth: code, data, layout | Nothing |
| `core/session`, `crates/editor` | Tx, bases, the delta, undo's inverses, the lock | A read-only session can hand its commits to the lock's holder (5.2). A step has an id and a source. A watcher batch that builds becomes a step (5.4). The write hook may refuse; a step can be retracted (5.5). |
| The writer: the app's server while the song is open, `m2` while it is closed | The revision, leases, previews, the one undo history, refusals, Compare, the focus record (memory only), the route table | Two keys (5.1); refusals and the landing mark (5.5); undo by step id (5.7); `step`, `undo`, `focus` and `point` events; one route table (5.8) |
| `m2` | The agent's one door | Forwards writes; adds `edit`, `write`, `undo`, `call`, `routes`, `focus`, `watch`, `point`, `listen` |
| The page | Showing and taking actions. No model. | Its own key; posts focus; shows the notice, the glow, Compare and pointers |

### 3.1 What the code says, where the panel or the first draft assumed otherwise

| It was assumed | The code says | So this design |
|---|---|---|
| `/api/tx/commit` already shows "An agent's edit" | That text appears only when an agent's commit failed after it was heard (`crates/server/src/graph/actor.rs:1797-1800`, `preview.rs:82-88`). A commit that lands shows nothing. | Adds the notice (5.6) |
| `m2 <write> --by Claude` | `--by` is taken: `m2 clips move --by <beats>` (`apps/m2/src/edits.rs:93`) | Uses `--as Claude` |
| `m2` can build the Tx and send it | Gestures plan and commit in one call: `plan::run_as` calls `Session::commit_with` (`crates/editor/src/plan.rs:79-107`). `m2 tx` and recovery fixes call `Session::commit` (`apps/m2/src/main.rs:1099`, `crates/editor/src/data.rs:52`). | Adds one seam in core: a session that forwards its commit (5.2) |
| The server knows when an agent asks | One token, `s.token`, checked by `auth` (`crates/server/src/routes.rs:115-126`). The page gets it from the window's start script (`apps/music2-desktop/src/main.rs:86-89`, `web/src/lib/native.ts:15`); tools get the same one from `session.json` (`main.rs:66`). | Two keys (5.1) |
| A check when the agent's Tx arrives protects a drag | A drag begins on the control plane without waiting for the actor (`crates/server/src/graph.rs:1147-1160`; `graph/lanes.rs:4-5`), so one can begin while the agent's change compiles | Checks again just before the write, with a landing mark (5.5) |
| `take_in_only` is the watcher's door | The app's own import and Sampler drop call it too (`crates/editor/src/import.rs:322-330`, `crates/editor/src/sampler.rs:107`) | Records outside steps only in the watcher's job (5.4) |
| A step is known by the revision it made | A layout-only Tx keeps the revision number (`core/session/src/tx/mod.rs:1505-1508`), so two steps can share it (`session.rs:529-533`) | Gives each step an id (5.7) |
| The route table is declared once | 14 router functions (`routes.rs:43-111` merges 13) and 11 write lists kept by hand (`routes.rs:129-157`, `WRITES_8B` and 9 more) | Makes the one table (5.8) |
| `--as-heard` applies the listening mute and solo | MON-4: "Overlays MUST NOT change files, revisions, exports or agent renders" (`docs/architecture.md:1224`) | Amends MON-4 (open decision D4) |
| Per-track numbers come from the render | Taps exist only in live plans, and hold peaks only (`core/runtime/src/taps.rs:1-9`, `core/runtime/src/lib.rs:2806-2810`) | Adds offline copy taps (5.11) |
| Outside edits can all be undone | A snapshot keeps binaries by hash only (`core/song/src/lib.rs:77-82`), so a replaced asset's old bytes are gone | Only text changes become steps (5.4) |
| The musician wins today | A lease ends when the code under it changes (`crates/server/src/graph/lease.rs:10-13`), so an agent's edit would end your drag | Refuses the agent instead (5.5) |

## 4. The invariants, as contract rules

New rules, in a new section "Agents" of `docs/architecture.md`, after "Writes". The prefix `AGENT` is unused
there today. Rules marked *pending* wait for the owner's answer in section 9.

**AGENT-1.** While a music2 app holds a project's lock ([WRITE-17](#the-project-lock)), an `m2` command that
writes the project MUST hand its Tx to that app instead of failing. It builds the Tx on a read-only session
from the files on disk and checks it there, under the gesture's own expectation and its planner's assertion.
It sends it to the app's `POST /api/tx/commit` with `Seen(hash)` of that check and the new assets it names,
using the url and the tools key in `~/.music2/session.json` ([AGENT-10](#agents)). A Tx given to `m2 tx`, and
a recovery fix, go as they are. It forwards only when the lock's holder is the program `music2` with the pid
that file names, and the app says this folder is the song it has open. Otherwise it fails before changing
anything, naming the holder ([WRITE-18](#the-project-lock)). `m2 edit` and `m2 write` make a change to a
song file a Tx with a base, so an agent changes files through the app while it is open. A forwarded write
prints what a local one prints, and says it is one undo step in the app.
- *Why:* while the song is open there must be one writer and one history, and only a change that comes
  through the writer can be refused when the musician holds what it touches.

**AGENT-2.** Every change the app accepts while it has the song open (a Tx, a save, or a watcher batch that
builds, [WRITE-19](#outside-writers)), other than an undo or a redo, MUST be exactly one undo step, with an
id that is never reused in that history, and a source: the page, an agent by its name, or outside the app.
An agent's step is named by the words it gives. Only the watcher's job makes outside steps: files the app
takes in for its own Tx (an import's) make none. An outside step's inverse puts back each text file the
batch changed or removed, guarded by the hash the batch left, as a save's inverse is
([WRITE-21](#undo)), and removes each text file it made. A batch that only adds files under `assets/` makes
no step and no notice. A batch that changes or removes a binary file (an asset, a plugin state) makes no
step, and says it can't be undone. Undo and redo name the step they mean by its id, and are refused when it
isn't the newest (for redo, the newest undone). Undo's name says the source.
- *Why:* whoever made a change, the musician can take it back with ⌘Z, sees who made it, and never takes
  back a step they didn't see named.

**AGENT-3.** Agents MUST NOT hold leases. The app MUST refuse an agent's write with status 423 and a `busy`
answer that names what the musician holds, when the write touches: a file in which a knob lease lives; a
lane file a lane lease holds; `data/arrangement.json` while a tempo lease lives; the lane file a take is
recording into; a file the code editor has unsaved text in; or anything, while the Compare pill (D2) is
open. It checks when the write arrives, and again in the session's last step before anything is written,
under the control plane's lock. That second check puts up a landing mark naming the files, until the settle that
follows reaches its first phase. A drag, or unsaved text, that starts on a marked file meanwhile is never
kept waiting: the app takes the agent's change back before any of it is sent to the engine or shown, and
answers busy. Nothing the musician holds ends, and they never hear a refused change. A file an agent writes
itself is an outside write ([WRITE-19](#outside-writers)): this rule can't refuse it, and it can end a drag.
- *Why:* the musician wins every clash. Only a write that comes through the app can be refused, so agents
  are told to write through it.

**AGENT-4.** An undo sent with the tools key (`m2 undo`) MUST take back the newest step only when its source
is an agent of the same name, and otherwise refuse, naming the newest step and its source. *Pending D1:*
the page's Undo takes the newest step, whatever its source.
- *Why:* one history in one line; an agent may only take back its own work.

**AGENT-5.** Every step not made by the page MUST be shown as it lands: a notice that never takes the
keyboard's focus and never blocks, with Undo while it is still the newest step, and a glow on the tracks,
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
([WRITE-17](#the-project-lock)), AGENT-11's 403 and `m2 routes --json` are all made from that one table.
- *Why:* a list kept by hand drifts. Agents learn the API from it, not from the source.

**AGENT-9.** The app MUST contain no model and call none. It only shares focus, shows what changed and takes
actions. *Pending D3:* there is no "agents may change the song" switch; when an agent changes things is
decided by what the musician asks it.
- *Why:* owner rule: no AI in the app, simple yet elegant.

**AGENT-10.** The app MUST give its page a key of its own, handed only to its window, and publish a different
key, the tools key, in `~/.music2/session.json`. A request with the tools key is an agent's. Its name is the
one it gives (`as` in the body, or the `x-m2-as` header), else `agent`. A step's source, AGENT-3, AGENT-4
and AGENT-11 go by the key, never by what a request says about itself.
- *Why:* with one key, a script that leaves out its name passes as the musician and skips every refusal.

**AGENT-11.** *Pending D6.* The transport (play, stop, seek, the loop, Stop All Sound), recording, listening
mute and solo, the view, plugin windows and their latches, auditions and the MIDI keyboard's target, naming
steps, settings, and opening, closing, making or renaming songs are the musician's. The app MUST answer them
403 with plain words when the tools key asks. Every read stays open to agents.
- *Why:* none of these is a song change, so none has a step, a notice or an undo; and a jump ends a take
  (`crates/server/src/graph/record.rs:16`).

Each track adds the rules it builds, and changes the existing text they touch, in its own first commit (the
contract's rule: a change that breaks a rule changes the page first). Track 1: AGENT-2, AGENT-5, WRITE-19's
and WRITE-21's text. Track 2: AGENT-1, AGENT-3, AGENT-4, AGENT-9, AGENT-10, owner decision 10, WRITE-18,
WRITE-22's text and Deferred. Track 3: AGENT-6. Track 4: AGENT-8 and AGENT-11. Track 5: AGENT-7 and MON-4.
The changes:
- **Owner decision 10** (`docs/architecture.md:41`): forwarding is adopted (2026-10-21), as AGENT-1.
- **WRITE-18** (`:913-930`): "Forwarding writes to the holder is deferred" becomes "A writer that finds the
  app holding the lock forwards ([AGENT-1](#agents)); a lock held by any other program fails as above." Its
  Why stays true: the safe writer is no longer blocked, only asked to wait while the musician holds a file.
- **Deferred** (`:1424`): the forwarding row goes.
- **WRITE-19** (`:934`): "accepts it as one revision" gains "and, in the app, one undo step ([AGENT-2](#agents))".
- **WRITE-21** (`:949`): "One Tx is one undo step" gains "and so is a watcher batch that builds
  ([AGENT-2](#agents))".
- **WRITE-22** (`:959-974`): its Today gains the landing mark: a drag begun while an agent's change lands
  takes the change back ([AGENT-3](#agents)).
- **MON-4** (`:1224`), pending D4: "…exports, or an agent render that didn't ask for them (`m2 listen
  --as-heard`, which says so)."
- **Acceptance gates**: a row "agents" with each track's gate (section 7).

## 5. Each piece against today's code

Words used below:

| Word | Meaning |
|---|---|
| step | One entry in undo's history: a stored inverse Tx (`TxStep`, `core/session/src/tx/mod.rs:280-298`), now with an id |
| source | Who made a step: `page`, `agent` (with a name) or `outside` |
| page key, tools key | The two keys of AGENT-10 |
| forwarding session | A read-only `Session` whose commits go to the lock's holder |
| busy | The refusal of AGENT-3 |
| landing mark | AGENT-3's note of the files an agent's change is writing, from its last check to its settle |

### 5.1 Who is asking: two keys

**Today.** `auth` lets in any request with `s.token` (`routes.rs:115-126`). The shell hands that token to
the page in `window.__M2__` (`apps/music2-desktop/src/main.rs:86-89`; `web/src/lib/native.ts:15`) and to
every local tool in `session.json` (`main.rs:66`, `:176-190`). So a script that sends no name counts as the
musician.

**Design.**
- `ServeOptions` (`crates/server/src/lib.rs:210-217`) gains `page_token`; `token` becomes the tools key. The
  shell makes both (`main.rs:192`), puts only the page key in the window's start script, and only the
  tools key in `session.json`.
- `auth` accepts either and puts the caller on the request: `Caller::Page`, or `Caller::Agent(name)` with
  the name from `x-m2-as` (or the body's `as`), else `agent`. `blocking` (`routes.rs:39-41`) and
  `Graph::call` (`graph.rs:909`) carry it to the actor's job, which gives it to every step it records.
- The headless server (`crates/server/examples/headless.rs:35-56`) has no window, so it writes both keys
  into the session file it is told to write. The browser suites read `page_token` from it (`e2e/fixlib.mjs:43-48`
  and 29 other suites).
- Names are what agents call themselves. Two agents at once should use two names (`M2_AS="Claude 2"`); `m2`
  says once, on stderr, when it writes with no name.

### 5.2 Forwarding

**Today.** Every `m2` write opens its session with `locked()` (`apps/m2/src/main.rs:266-272`). When the app
holds the lock, `Session::open_as` gives a read-only session, and `locked()` stops with WRITE-18's message
(`core/session/src/lock.rs:89-104`). Forwarding is "deferred (owner decision 10)" (`lock.rs:6-7`). The
app's `POST /api/tx/commit` exists (`routes.rs:80`, `:781-786`); it previews, commits and records the step
like any edit (`actor.rs:1774-1823`; `tx/mod.rs:759-765`). The UI never calls it. `~/.music2/session.json`
holds `{url, token, pid}`, mode 0600, written at each launch and never removed (`main.rs:176-190`). The
lock's file names its holder `{pid, program, …}` (`lock.rs:24-37`), and the app takes it as `"music2"`
(`actor.rs:1253`).

**The seam, in core.** One small trait, used at the two places a session commits:

```rust
/// The lock's holder, which takes a read-only session's commits (AGENT-1).
pub trait Forward: Send {
    fn commit(&mut self, tx: &Tx, assets: &[TxPath]) -> Result<Landed, Refusal>;
}
pub struct Landed { pub step: u64, pub number: u64, pub code_rev: u64, pub data_rev: u64 }
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
- `undo` and `redo` in a forwarding session are refused. `m2 undo` is its own route (5.7).
- The app commits a `Seen` Tx as it does today's `m2 tx --seen`, so its undo step is a raw one, guarded by
  its bases (`tx/mod.rs:282-283`).

**New assets.** `import` and `bounce --place` write their files under `assets/` create-only, then commit a
Tx that names them. A forwarded commit lists those paths in `assets`. The app's job takes them in before it
commits, as the app's own import does (`import::take_in`, `import.rs:322-330`), so the clip plays from its
first block with no `asset.missing`, and the watcher's later batch for them finds nothing new (5.4).

**In `m2`.** A new module, `apps/m2/src/app.rs`, "the open app":
1. `locked()` gets `ReadOnly::Held(Some(holder))`. If `holder.program == "music2"`, it calls `app::reach`.
2. `reach` reads `session.json`, requires its pid to be the holder's, and asks `GET /api/graph`. It requires
   `dir` to be this folder (both canonical) and keeps `session`, the open song's token, which the app checks
   on every commit (`actor.rs:456-461`).
3. `locked()` then calls `s.forward_to(app)`. Every write command forwards with no change of its own: `set`,
   `automate`, `unautomate`, `label`, `add-track`, `import`, `mute-in-song`, `clips`, `duplicate-track`,
   `delete-track`, `layout`, `tempo`, `meter`, `title`, `connect`, `disconnect`, `add-effect`,
   `remove-effect`, `relink`, `delete-data`, `fix`, `plugin-state`, `tx`, `bounce --place`, and the new
   `edit` and `write` (5.3).
4. Requests are plain HTTP/1.1 over `TcpStream` to 127.0.0.1, as the server's tests send them
   (`crates/server/src/routes_8b_tests.rs:14-29`), with the tools key. No new crate.

**Commands that aren't a Tx today.** Every `m2` write command builds a Tx and reaches one of the two commit
calls above (WRITE-1). The only other writes `m2` makes are the new assets above. Actions that aren't a
song change at all (transport, listening mute and solo, the loop, view settings, recording, plugin windows,
settings) have no `m2` command, and stay the musician's (AGENT-11, 5.8).

**`--as` and `--why`.** The author is `--as <name>`, else `M2_AS`, else `agent`. The words are `--why
"<words>"`. `m2` takes both off the command line before it runs the command, because `m2 clips` refuses
flags it doesn't know (`edits.rs:191-196`). They travel in the commit's body: `TxReq` (`routes.rs:763-769`)
gains `as`, `why` and `assets`. The actor names the new step with `why` (`Session::name_step`,
`core/session/src/session.rs:538`) and gives it the source `agent` with that name. Without `--why`, the step
keeps today's plain name (`crates/server/src/graph/steps.rs:31`).

**What `m2` says, case by case.**

| Case | `m2` says | Exit | Song changed |
|---|---|---|---|
| The app holds the lock, has this song open, answers | what a local write prints, then "in the open app: one undo step, Claude: brighter hats" | 0 | yes |
| The lock is free | writes the files itself, as today (no undo once it exits) | 0 | yes |
| Another `m2` holds the lock | WRITE-18's message, as today | 1 | no |
| `music2` holds it, but `session.json` is missing or names another pid | "this song is open in music2 (pid 4121), which m2 can't reach. Make the edit there, or close it there and run this again." | 1 | no |
| The app answers another folder (it is opening or closing songs) | "the app is opening or closing this song; run this again" | 1 | no |
| The app's window is read-only (423 from the gate, `routes.rs:161-169`) | the app's words | 1 | no |
| Busy: a drag, a take, or Compare (AGENT-3) | waits, retrying every 250 ms for up to 30 s, saying "waiting: you're dragging Bass cutoff"; then gives up with those words | 3 | no |
| Busy: unsaved text in the code editor | at once: "tracks/bass.ts has unsaved changes in the app's code editor" | 3 | no |
| Taken back: the musician started a drag as it landed | "you started dragging Bass cutoff as this landed, so it was taken back" | 3 | no |
| Stale: a base changed (409) | the refusal JSON, as `m2 tx` prints it today, and "read again and retry" | 1 | no |
| The song changed between check and commit (`Seen` refused, 422) | "the song changed while this was planned; nothing was changed; run it again" | 1 | no |
| The app is slow (a long compile) | waits while the app's pid lives, saying "waiting for the app…" every 5 s | — | — |

Every busy answer ends with: "Don't write the file directly instead: the app can't refuse that, and it would
end what the musician holds." Exit 3 means "busy, nothing changed, try later", so an agent can tell it from a
mistake.

**If the app quits during the call.** `m2` knows each target's base hash and, from its own check, the hash it
should have after (the candidate's snapshot). When the connection drops, it reads the targets:

| The targets are | It means | `m2` says | Exit |
|---|---|---|---|
| all at their bases | the app quit before it took the edit | "nothing was changed: the app closed first. Run this again; it will write the files itself." | 1 |
| all at the candidate's hashes | it landed, then the app quit | "it is saved, but its undo step went with the app" | 0 |
| some of each | the app was killed mid-commit; its journal is in `.music2/tx/` (WRITE-16) | the `m2 fix … finish-tx` and `discard-tx` commands, as every command prints them (`main.rs:295-316`) | 1 |

Killing `m2` while it waits doesn't take the edit back: it may still land. `m2 watch` shows it if it does.

### 5.3 Writing files through `m2`: `m2 edit` and `m2 write`

An agent's normal path is to edit files (owner decision 5). A direct write reaches the app only through the
watcher, after it is on disk, so there is nothing left to refuse. These two commands make the same edit a
Tx, which the app can refuse, name, and let the agent undo.

```
m2 edit  <song> <file> --old <text> --new <text> [--all] [--as <name>] [--why <words>]
m2 write <song> <file> [<from> | -]                      [--as <name>] [--why <words>]
```

- **`m2 edit`** reads the file, finds `--old` exactly once (or every time, with `--all`), and makes one
  `TextEdit` (`core/song/src/tx.rs:77-80`) whose edits each guard their place with `expected`, the old text
  (`tx.rs:103-109`), on the base of the text it read (`Base::WithText`, `tx.rs:52`). It is what an
  agent's own Edit tool does, as a Tx. Found no times, or more than once without `--all`: refused, saying
  how many. On a stale base (409) it reads again and tries once more, since "replace this text" still means
  the same.
- **`m2 write`** writes a whole text file (a new track's file, a rewrite) as one `Write` op (`tx.rs:82-85`)
  on the base it read (`Absent` for a new file). It overwrites what was there when it read, as a direct
  write would, but it is named, the agent can undo it, and it is refused while the musician holds the file.
  It refuses binary files: `m2 import` brings in audio, `m2 plugin-state` writes a plugin's state.
- Both are checked as `m2 tx --check` checks (WRITE-11) and committed `Seen`: in the app (5.2), or by `m2`
  itself when the app is closed.
- **What agents are told.** The README's "Or just edit the files" (`README.md:748`) becomes: with the app
  closed, edit freely. With it open, use `m2 edit` and `m2 write`. A direct write is an outside write: it is
  one undo step, but it can't be refused, it ends a drag on what it changes, a code editor with unsaved
  text in that file asks Theirs or Mine, and `m2 undo` can't take it back. The same words are in
  `m2 --help` and in every busy answer.

### 5.4 Outside file edits as undo steps

**Today.** The watcher takes a settled batch in whole, as one revision (`actor.rs:1688` calls
`take_in_only`, `take.rs:129`). If its code doesn't build, the batch waits and the last good version plays
(`take.rs:12-17`). `take_in` records no step (`take.rs:103-121`). Only a save (`take.rs:221-232`) and a
Tx commit (`tx/mod.rs:750-752`, `:759-765`) record one. `take_in_only` is also how the app takes in its own
new files: the import (`import.rs:322-330`, called by `place` at `:339`) and the Sampler drop
(`sampler.rs:107`).

**Design.** The step is recorded only in the actor's watcher job (`Actor::settled`, `actor.rs:1668-1700`),
through a new core call, `Session::take_in_batch(snapshot, paths)`: `take_in_only`, then, when a revision
was accepted, one step, source `outside`, raw (taken in like an outside write when undone, as a save's step
is), shown by its first changed file. `take_in_only` itself still records nothing, so an import makes only
its own Tx's step. Its inverse, for each path that changed between the revision before and the one after:

| The batch | Inverse op | Its base |
|---|---|---|
| changed a text file | `Write` of the text before | the hash after |
| made a text file | `Remove` | the hash after |
| removed a text file | `Write` of the text before | `Absent` |
| made a binary file, beside text changes | none: it stays, and an asset no lane names plays nothing | — |
| only made binary files (new assets) | none: the batch makes no step and no notice | — |
| changed or removed a binary file | none: the batch makes no step, and its notice says "can't be undone" | — |

- **The before-image.** The revision before the batch holds each text file's text (`snapshot.files`, shared
  `Arc<str>`, not copied). The step keeps those, counted against undo's 64 MiB bound
  (`session.rs:133-137`). Binaries are kept by hash only (`core/song/src/lib.rs:77-82`), which is why a
  changed binary can't be put back.
- **Only revisions that build.** A batch that waits makes no step. When it is taken in, there is one step from
  the last accepted revision to the new one. A change to `layout.json` alone makes no revision (PLANE-3), so
  no step. A code-editor save that makes waiting outside changes build keeps only its own step; those
  changes ride along in its revision, as today.
- **Never twice.** A code-editor save takes in through `take_in` (`take.rs:221`) and keeps its own step. The
  session's own commits, the retract (5.5) included, make no batch (WRITE-19). A forwarded import took its
  assets in already (5.2), so the watcher's batch for them is equal to the draft and is dropped
  (`take.rs:104-106`).
- **Undo after a later edit.** History is one line, so later steps are undone first. If the file changed since
  in a way that made no step (a binary batch, a batch still waiting), the base no longer matches. Undo is
  then a `Conflict` naming the file, and nothing changes (WRITE-21).
- **Its name.** The plain name of its file (`steps.rs:31`), shown as "Changed outside the app:
  tracks/bass.ts", or "…: 3 files". The watcher can't know who wrote a file, so it is never an agent's.

### 5.5 Refusals: what the musician holds

AGENT-3 compares the Tx's touched paths (`Tx::touched`) with what the musician holds:

| The musician is | The server knows it from | Refused when the Tx touches | Words |
|---|---|---|---|
| dragging a knob | `Graph::leases` (`graph.rs:1351-1354`), each with its `file` (`lease.rs:197-217`) | that file | "you're dragging Bass cutoff in the app" |
| dragging a clip, a note or automation | `Graph::lane_leases` (`graph/lanes.rs:531`) | that lane's file | "you're dragging a clip on Bass" |
| dragging the tempo | `Graph::tempo_leases` (`graph/tempo_lease.rs:365`) | `data/arrangement.json` | "you're dragging the tempo" |
| recording | the record state (`graph/record.rs:921`) | the take's lane file | "you're recording on Bass" |
| typing code not yet saved | `Graph::unsaved` (`graph.rs:1950-1954`) | that file | "tracks/bass.ts has unsaved changes in the code editor" |
| comparing (the pill is open, 5.12) | `Graph::comparing` (new) | anything | "you're comparing Claude: brighter hats" |

**When it is checked.**
1. **On arrival.** In the actor's job, before the Tx is checked (`Actor::tx`, `actor.rs:1774`, and every
   job an agent's write route runs, 5.8). This is the quick answer: no compile is spent on a refused change.
2. **Just before the write.** A drag begins on the control plane without waiting for the actor
   (`graph.rs:1147-1160`; `lanes.rs:4-5`), so one can begin while the agent's change compiles. So the check
   runs again in the session's write hook (`BeforeWrite`, `tx/mod.rs:112`). The hook runs after the targets
   were read again and the candidate was built, just before the journal (`tx/mod.rs:1584`), on every
   commit path (`commit`, `commit_with`, the gestures). It may already fail the write there, when the lock
   was let go (`:1588-1592`). It gains a refusal of its own: it returns `Result`, and an `Err` is a new
   `Refusal::Busy { words }`, answered 423. For an agent's write the actor installs this hook in place of
   the PLANE-5 preview hook (`actor.rs:1794`, `:1825`). So nothing of an agent's change is heard before it
   is written; it is heard at its settle, a few milliseconds later.
3. **The landing mark.** When that check passes, under the same control-plane lock, it puts up a mark
   naming the touched files. The mark comes down at the start of the settle's first phase
   (`actor.rs:888-890`), under that lock, before any lease is carried (`:898-919` for knobs, `:1087` for
   lanes) or anything is sent to the engine. Meanwhile a knob, lane or tempo `begin` (`graph.rs:1147`), or
   `POST /api/graph/unsaved` (`graph.rs:1940`), on a marked file goes on at once, as RENDER-29 requires,
   and notes itself on the mark. A write that fails after the hook takes the mark down with it.
4. **Taken back.** If the mark was noted, the settle sends nothing and carries nothing. It drops the
   renderer it prepared (the disk thread holds its lanes weakly, `core/play/src/disk.rs:45`, so nothing is
   left behind), takes the agent's step back, and settles again. `Session::retract(id)` is new in core: it
   commits the newest step's inverse at once and keeps it in neither undo nor redo. Nothing has written
   since, so its bases match (only an outside write in those milliseconds could stop it, and then the drag
   meets that write, as today). The drag is carried to a revision whose files read as before, so it goes
   on. The agent gets 423 busy with `"taken_back": true`. No `step` event and no notice are sent.

The answer is 423 with `{"busy": {"what": "knob", "file": "tracks/bass.ts", "words": "…", "taken_back":
false}}`. A knob lease is matched by file, not by the call it holds: simpler, and a drag lasts seconds.
Agents never hold leases: the lease routes are the musician's (AGENT-11). A file written directly can't be
refused: it is an outside step (5.4) and ends a drag on what it changes, as WRITE-22 says today (5.3).

### 5.6 The notice and the glow

**Today.** Toasts go to every window (`crates/server/src/lib.rs:157-170`). A toast's Undo works only while its
edit is the newest, judged by the page's own revision (`web/src/components/GraphCanvas.tsx:327-336`), which
can be stale. Undo's name comes from the top step (`steps.rs:21-27`), shown in the state as `undo_name`
(`actor.rs:660-664`).

**Design.**
- After each job that records a step, whatever its source, the server sends a `step` event:
  `{"t": "step", "step": 57, "made": 42, "source": "agent", "by": "Claude", "name": "brighter hats",
  "files": [...], "tracks": [...], "lanes": [...], "knobs": [...]}`. `tracks`, `lanes` and `knobs` come
  from the commit's delta: modules and node names by module key, lanes changed, and changed constants
  turned into the view's knob ids. A new `crates/editor/src/changed.rs` does this, since knob ids are the
  editor's.
- `undo_name` and `redo_name` gain `step`, `source` and `by`. The Undo tooltip and the Edit menu say "Undo
  Claude: brighter hats", "Undo outside change: tracks/bass.ts", or, for your own, "Undo Bass notes" as today.
- The page shows a notice for each step whose source isn't `page`: "Claude: brighter hats · Undo". Its
  Undo sends that step's id (5.7), so it can never take back another step. One notice at a time: a newer
  step replaces it ("Claude: 3 changes · newest: brighter hats"). It never takes the keyboard's focus.
- The glow: an outline on those track headers, clips and knobs, until your next pointer or key action in the
  song, or the next step.

### 5.7 Undo by step, and `m2 undo`

- **Step ids.** The session gives each step an id from a counter that never goes back, kept on `TxStep`
  (`tx/mod.rs:280-298`) beside `made` and `name`, with its source.
- **Undo and redo name their step.** `POST /api/graph/undo` and `/redo` (`routes.rs:811-828`; today
  `{session}` only) take `{session, step}`. When the step isn't the newest (for redo, the newest undone),
  the answer is 409, "the newest change is now Claude: warmer bass", and nothing changes. The top bar's
  Undo, ⌘Z, the notice's Undo and V all send the id they show. The page's own revision check
  (`GraphCanvas.tsx:327-336`) goes.
- **`m2 undo <song> [--as Claude]`.** It sends `POST /api/graph/undo` with `{session}` and the tools key. The
  app takes back the newest step only if its source is `agent` with that name. Otherwise 409: "nothing was
  undone: the newest change is yours (Bass notes), not Claude's". With the app closed: "nothing to undo:
  undo lives in the open app" (history is in memory, `session.rs:150-154`). There is no `m2 redo`: putting
  back is the musician's choice.

### 5.8 `m2 call`, `m2 routes --json`, and the one route table

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
gate becomes one middleware that reads `writes` from it; the 11 hand lists go. A second middleware answers
403 when the tools key asks for a route not marked `agents` (AGENT-11). `m2` links `m2_server` already
(`apps/m2/Cargo.toml`), so `m2 routes --json` prints the table with no app running: method, path, writes,
agents, words, example.

Not for agents, so not marked `agents`: the knob, lane and tempo lease routes and `/api/graph/preview`
(AGENT-3); play and stop, seek, the loop and Stop All Sound (`routes.rs:77`, `:84-86`); recording
(`routes_record.rs:29-31`); listening mute and solo (`routes_8b.rs:30`); the view (`routes_view.rs:29`);
plugin windows and latches (`routes_8b.rs:36-37`); auditions and the inject routes (`routes_8b.rs:34-35`,
`routes_record.rs:32`); naming steps (`routes_settings.rs:25`); settings (`routes_settings.rs:23`); the code
editor's save and unsaved list (`routes.rs:88`, `:90`: agents use `m2 edit`, which is a Tx); posting focus
and closing Compare (5.9, 5.12), and `compare: true` on undo and redo; opening, closing, making, Save As
and renaming songs; and Retry (`routes.rs:78`). Every GET is for agents, and so
is the WebSocket, for reading (`m2 watch`): what a socket with the tools key sends (an audition, the MIDI
target, `routes_8b.rs:225-263`) is ignored.

`m2 call <path> [json | -] [--as] [--why]` looks up the method in the table, sends the body (a GET's keys go
in the query), prints the answer, and exits 1 on a status of 400 or more. It refuses a route not marked
`agents` before sending, with the same words the app would give. `--as` and `--why` go as headers `x-m2-as`
and `x-m2-why`, which `auth` reads (5.1) and the actor's job uses to name every step it records. Then every
write route can be named, not only `/api/tx/commit`, and every write route runs AGENT-3's checks for an
agent.

### 5.9 The focus record

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
`unsaved` (`graph.rs:1950-1954`), `comparing` (5.12) and the MIDI target.

- **Who posts it, and when:** the page, with `POST /api/focus` and the page key, 200 ms after the last change
  to any field, and once when its socket connects. It is not a write, so it works read-only. The tools key
  can read it, not post it.
- **Memory only:** kept beside `unsaved` in the server (`graph.rs:417`), dropped when the song closes or
  another opens. The newest post wins.
- **Event:** `{"t": "focus", "focus": {...}}` to every socket on each post. `GET /api/focus` and `m2 focus`
  return it.

### 5.10 `m2 watch`

`m2 watch [--meters]` opens `/api/ws?token=…` (`routes.rs:185-236`) and prints one JSON object a line, `t`
first. The WebSocket client is `tungstenite` 0.29.0, already in `Cargo.lock`.

| `t` | Made by | Says |
|---|---|---|
| `step` | new (5.6) | a change landed: its id, who, why, files, tracks, lanes, knobs |
| `undo`, `redo` | new, from the undo and redo routes | which step went or came back, its source, and `compare` when V did it |
| `compare` | new (5.12) | the Compare pill closed |
| `graph` | today (`lib.rs:181-188`) | the revision, problems, diagnostics, leases |
| `focus` | new (5.9) | the musician's focus |
| `point` | new (5.13) | a pointer shown or cleared |
| `toast`, `export`, `take`, `midi` | today | as the page hears them |
| `transport` | `m2` itself | `playing`, `beat`, `loop`, when playing or the loop changes, or the playhead jumps |
| `meters` | today, only with `--meters` | about 30 a second |
| `closed` | `m2` itself | the app went away; then `m2 watch` exits 0 |

Transport changes send no event today (`routes.rs:850-890`); the page polls status every 200 ms
(`GraphCanvas.tsx:59-63`). `m2 watch` polls `/api/graph/status` every 250 ms the same way, so the server
needs no change for it.

### 5.11 `m2 listen`

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

### 5.12 The Compare key (pending D2)

V, for "versus". The page's single keys are taken by c, a, e, x, b, m, s, z, k and r
(`web/src/App.tsx:69`); V is free.
- When the newest step isn't yours (an agent's or an outside one), V sends `POST /api/graph/undo` with
  `{session, step, compare: true}`. The server undoes it and keeps `comparing = step`, in memory, beside the
  focus record. The pill says "Before: Claude: brighter hats". V again sends redo with the same step: "After".
- While `comparing` is set, every agent write is busy (AGENT-3), so no agent step can clear redo and take
  "After" away.
- Esc, or any other action, closes the pill and keeps what plays: the page sends `POST /api/graph/compare`
  `{session, open: false}`, and any other write with the page key, or the page's socket closing, ends it too. If it
  closes on Before, the change stays undone, and Redo still has it until the next change.
- A file written directly can still land while you compare (5.5). Its outside step clears redo, so the
  server ends `comparing`, and the pill says "After is gone: a file changed outside the app".
- Each flip is an undo or redo Tx. A data change is heard within 20 ms (PLANE-5); a code change after its
  compile, through an activation that keeps every instance (RENDER-25). Neither glitches.

### 5.13 `m2 point`

`m2 point <song> <target> "<words>"`, or `--clear`. A target is `track:bass`, `clip:bass/notes:c3`,
`lane:bass/cutoff`, `knob:<id>` or `bars:9-16`. It sends `POST /api/point`, which isn't a write. The page
shows a small note at the target, with OK to dismiss it. It never scrolls, zooms or selects. When the target
is off screen, the note waits at the edge with "Show", and only that click moves the view. One pointer at a
time, memory only, and the `point` event tells `m2 watch`. This is how an agent says "listen to bars 9-16":
it points, and you press play (AGENT-11).

## 6. Out of scope

- Any model, chat or AI in the app.
- An unwritten candidate with Keep and Back (the panel's `m2 try`): a second world in every view.
- Taking back an older step on its own (selective undo), or one undo history per author.
- Undo that survives closing the song.
- Refusing or naming a file an agent writes directly: the watcher can't know who wrote it (5.5).
- Agents driving playback, recording, the view or plugin windows, holding leases, or previewing live (AGENT-11).
- Streaming live audio, screenshots, or live analysis to the agent.
- MCP and `m2 describe`: later, as thin wrappers over these commands.
- Telling apart two agents that give the same name; several agents at once beyond their names and the refusals.
- Reaching the app from another computer. The server stays on 127.0.0.1.

## 7. Build order

Before track 1, the owner answers D1 to D3 (section 9). Each track is one branch, merged to `main`, with one
full check (`ci/check.sh`, `music2 check: PASS` on Linux) at its end. Every test below is written first and
shown failing before its code lands. "Proves" says what the full check then shows.

| Track | What | Waits for | Size |
|---|---|---|---|
| 1 `ag-steps` | Step ids and sources; undo and redo by step id; outside batches become steps in the watcher's job; the `step` event; the notice and the glow; Undo's name; the README fixes | D1 | small |
| 2 `ag-fwd` | Two keys; forwarding with assets, `--as`, `--why`; `m2 edit`, `m2 write`; busy refusals, the landing mark and `retract`; `m2 undo`; the contract's changes | D1, D3 | medium |
| 3 `ag-see` | The focus record, `m2 focus`, `m2 watch`, `m2 point`; the Compare key and `comparing` | D2 | medium |
| 4 `ag-routes` | The one route table, the 403 for the musician's controls, `m2 routes --json`, `m2 call`, `x-m2-as` on every write | D6 | medium, mostly mechanical |
| 5 `ag-listen` | `m2 listen`, copy taps, `--with`, `--as-heard`, `--png`; G13 | D4 | large |

**Track 1, `ag-steps`.** Tests:
- `core/session`: `an_outside_edit_that_builds_is_one_undo_step`, `an_outside_batch_that_waits_is_one_step_once_it_builds`,
  `undoing_an_outside_edit_puts_back_its_text`, `undoing_an_outside_edit_after_a_change_without_a_step_conflicts`,
  `an_outside_binary_change_makes_no_step`, `a_batch_that_only_adds_assets_makes_no_step`,
  `a_code_editor_save_is_still_one_step`, `the_sessions_own_commit_is_not_an_outside_step`,
  `a_layout_step_has_its_own_id`.
- `crates/server`: `an_import_in_the_app_makes_one_step_and_no_outside_step`, `undo_says_the_source_of_the_newest_step`,
  `an_undo_for_a_step_that_is_no_longer_newest_changes_nothing`, `a_step_event_names_what_its_delta_changed`.
- `crates/editor`: `changed_names_tracks_lanes_and_knobs` (`changed.rs`).
- Browser suite, `e2e/ag-notice.mjs`: an outside save shows one notice; its Undo works while it is newest and
  says why once it isn't; a WAV dropped on a lane makes one step and no notice; the track glows until a click.
- Proves: the Rust tests, clippy and the browser suite pass; the offline renders still equal
  `e2e/renders.sha256` (no render changed); undo's step counts in the existing tests keep their meaning.

**Track 2, `ag-fwd`.** Tests:
- `core/session`: `a_forwarding_session_sends_its_checked_tx_as_seen`, `a_recovery_fix_is_forwarded_as_it_is`,
  `after_forwarding_the_session_reads_what_landed`, `a_forwarding_session_refuses_undo`,
  `a_write_hook_that_refuses_writes_nothing`, `retract_leaves_no_redo`.
- `crates/server`, with test hooks at the two checks (as `between_phases`, `actor.rs:70-72`):
  `the_tools_key_makes_a_request_an_agents`, `a_write_with_the_page_key_is_the_musicians`,
  `a_drag_begun_while_an_agents_change_compiles_makes_it_busy`,
  `a_drag_begun_while_an_agents_change_lands_goes_on_and_the_change_is_taken_back`,
  `unsaved_text_said_while_an_agents_change_lands_takes_it_back`, `an_agents_change_is_not_heard_before_it_is_written`,
  `a_forwarded_import_plays_its_asset_from_the_start_and_makes_one_step`.
- `apps/m2/tests/forward.rs`, against a headless server (`crates/server/examples/headless.rs`) with `HOME` set
  to a temp folder: `a_write_while_the_app_holds_the_lock_lands_in_the_app`, `every_write_command_forwards`
  (one row per command in 5.2), `the_step_is_named_by_as_and_why`,
  `a_song_held_by_an_app_m2_cant_reach_fails_naming_it`, `a_write_on_a_file_with_a_knob_drag_is_busy_and_the_drag_goes_on`,
  `a_write_to_a_file_with_unsaved_editor_text_is_busy`, `a_write_to_the_recording_lane_is_busy`,
  `busy_exits_3_and_says_not_to_write_the_file`, `the_app_quitting_before_the_commit_changes_nothing`,
  `the_app_killed_mid_commit_names_its_fixes`, `the_app_quitting_after_the_commit_says_it_landed`,
  `m2_undo_takes_back_only_its_own_step`, `m2_undo_with_no_name_never_takes_back_the_musicians_step`,
  `m2_undo_with_the_app_closed_says_undo_lives_in_the_app`.
- `apps/m2/tests/edit.rs`: `m2_edit_replaces_text_found_once_as_one_named_step`, `m2_edit_refuses_text_found_twice`,
  `m2_edit_on_a_stale_file_reads_again_once`, `m2_write_makes_a_new_file_as_one_step`,
  `m2_write_refuses_a_binary_file`, `m2_edit_works_with_the_app_closed`.
- `apps/m2/tests/lock.rs:52`: `a_write_while_another_process_holds_the_lock_fails_and_changes_nothing` keeps
  its meaning. Its holder is a lock taken as `music2` with no app behind it (`lock.rs:59`), so `m2` can't
  reach it and fails, naming it; only the words change. Every `m2` test sets `HOME` to a temp folder, so a
  real app's `session.json` is never read.
- Proves: every `m2` write command works with the app open and lands as one named step; no lease or unsaved
  text of the musician's ends because of an agent's write, in either window; track 2's contract changes
  (section 4) are in.

**Track 3, `ag-see`.** Tests:
- `crates/server`: `focus_is_memory_only_and_goes_with_the_song`, `a_focus_for_another_song_is_refused`,
  `a_point_is_not_a_write`, `while_comparing_an_agents_write_is_busy`, `an_outside_step_ends_compare`.
- `apps/m2/tests/watch.rs`: `m2_watch_prints_one_json_line_an_event`, `m2_watch_ends_when_the_app_quits`,
  `m2_focus_reads_what_the_page_posted`.
- Browser suite, `e2e/ag-focus.mjs`: selecting a track and two notes is what `m2 focus` says, by key and by
  note; focus changes no file; a pointer never moves the view or the selection; off screen it waits at the
  edge; V flips the newest outside step while the loop plays, and the pill says Before and After; an agent's
  write while the pill is open is refused, and After stays.
- Proves: an agent can see the musician's focus and follow every change, nothing of it reaches the song,
  and Compare can't be broken by an agent's write.

**Track 4, `ag-routes`.** Tests:
- `crates/server`: `every_route_is_declared_once` (the router's paths are the table's, and no hand list is
  left), `every_example_body_reads` (each example, posted to a fixture's server, is never refused for its
  shape), `a_read_only_window_answers_every_write_423` (every route the table marks `writes`),
  `the_tools_key_gets_403_on_every_route_not_for_agents` (transport, seek, loop, record, overlay, view,
  plugin windows, settings among them).
- `apps/m2/tests/call.rs`: `m2_routes_lists_every_route`, `m2_call_reaches_an_action_with_no_command` (an
  effect's bypass), `m2_call_refuses_the_musicians_controls`, `m2_call_names_its_step`.
- Proves: every song change the page can make is reachable from `m2`, the musician's controls aren't, and
  the API describes itself.

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
| 5 | `README.md:748` | "Or just edit the files" | doesn't say those edits can't be undone. After track 1 they can, if they are text; after track 2 it says to use `m2 edit` while the app is open (5.3). |
| 6 | `crates/server/src/routes_view.rs:55-56` | the view route takes 6 keys | it takes 8: `metronome` and `guide` too (`crates/server/src/graph/view_file.rs:25`) |

Two more, found while checking: `README.md:746` says the WebSocket has "three events" (it has more: `meters`,
`midi`, `take`, `graph`'s fields); and `README.md:230` says `.music2/` holds undo (it doesn't). Both are
fixed in track 1 too.

## 9. Decisions for the owner

D1 to D3 are recommended and wait for your yes before the build starts. D4 to D6 are new questions; each
waits only for the track that needs it (section 7).

| # | The question | Recommended | Why |
|---|---|---|---|
| D1 | One shared undo line? Your ⌘Z can take back Claude's newest change. Claude can take back only its own. | Yes | One history, nothing to keep in step. The notice and Undo's name say whose step is next. |
| D2 | How do you hear before and after? Claude makes the change, and you press V to flip it. Or a second "trial" song with Keep and Back. | Write, then V | One song: what you see is what you hear, and every state is saved. |
| D3 | A switch in the app, "agents may change the song"? | No switch | When Claude changes things is decided by what you ask it. The lock and the keys already keep other programs and web pages out. |
| D4 | May `m2 listen --as-heard` apply your listening mute and solo? This changes MON-4. | Yes | You ask "why does this sound muddy?" about what you hear. It is never the default, and it says so. |
| D5 | A file saved outside the app that replaces an audio file or a plugin's state can't be undone (its old bytes are gone). Keep a copy of every such file to make it undoable? | No copy | Agents change text, and with the app open they use `m2 edit`. Copying every changed asset costs disk and time for a rare case. |
| D6 | May an agent press play, stop, seek, loop, record, solo, or change your view? | No | These are your hands. None is an undo step, a seek ends a take, and Claude can point at bars 9-16 for you to play instead. |

## 10. Review

A skeptic checked the first draft against the code. Its six major findings changed the design:

| Finding | What changed |
|---|---|
| M1: direct file writes skip AGENT-3, end drags, and are named "outside" | `m2 edit` and `m2 write` (5.3); AGENT-1 and AGENT-3 say plainly that a direct write is an outside write; the busy answer says not to fall back to one; In short says it too |
| M2: the busy check came before the compile, but drags begin without waiting for the actor | A second check in the write hook, and the landing mark with `retract` (5.5) |
| M3: an agent's step could clear Compare's redo; the notice's Undo could take back another step | Agents are busy while comparing (5.12); undo and redo name their step by id (5.7) |
| M4: an import made outside steps; new assets made empty steps | Outside steps only in the watcher's job; asset-only batches make no step; forwarded assets are taken in by the commit's job (5.2, 5.4) |
| M5: the page and agents shared one key | Two keys, AGENT-10 (5.1) |
| M6: `m2 call` reached the musician's controls with no rule | AGENT-11 and the 403 from the route table (5.8), pending D6 |

Taken in part, with the reason:
- **M2's fix said to check in `commit_with`'s assertion.** The app's Tx route calls `commit`, not
  `commit_with` (`actor.rs:1794`), and a drag can still begin after that point, during the settle's renderer
  build. So the check is in the write hook, which every commit path runs, and the landing mark covers the
  rest of the way to the settle.
- **M5's fix said to add the `m2` process id to agent names.** Rejected: each `m2` call is a new process, so
  `m2 undo` would never match the step its own earlier call made. Names stay what agents call themselves,
  and two agents use two names (5.1).

The review's text reached this revision cut off partway through M6 (the hand-off keeps its first 9,000
characters). Findings after M6, if any, were not seen, so none is answered or rejected here.
