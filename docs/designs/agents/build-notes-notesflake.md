# Undo and drags on a slow machine: build notes for branch `notesflake`

**In short**
- The browser suite `notes` (the run `notes-proj`) failed on GitHub's runners and here under load, with a 409 or with "one write". There were three causes, two in the app and one in the suite.
- **Undo** chose its step from what the window showed. The server makes an edit's undo step before the edit's answer reaches the window, so a ⌘Z pressed in between named the step below and was refused (409). If the refusal came back first, nothing was undone. Now Undo pressed while the window's edits are on their way chooses its step at the press, from the steps those edits make, and waits at most 2 s for them.
- **A timeline drag** made right after a note was added in the roll named the clip by the hash drawn before the note (409, "changed in another window"), or was dropped when let go before the state came. Now a drag let go waits for the state. Only in a lane the roll's own write left as it is now does it name its clips as drawn then.
- **The suite** read the server's revision once, as soon as the note was on disk. The server writes the file before it publishes the revision. The check now waits for the revision to move, as its drag check did. The bound is the same.
- Proof: under load (the suite pinned to 2 cores with 3 busy loops), `main` failed 13 of 26 runs; the branch passed 30 of 30. Six new checks hold the server's answers back the way a slow machine does. Each one fails on the code before its fix and passes with it.

## What failed, and why

The CI logs are of pull request #17 (suites 2/2): run 1 `FAIL one write`, run 2 `FAIL no console errors … 409 (Conflict)`. Here, with every request and answer logged:

| Seen | Cause | Shown by |
|---|---|---|
| 409 on `/graph/undo`, then the undo sent again; or refused for good, with "isn't the newest change now" | ⌘Z sent `step: 0` (or the step below) while the delete's answer was on its way: the server had made the delete's step. When the 409 came back before the delete's answer, `madeHere` didn't know the step yet, so nothing was undone. | Every 409 in 26 runs under load was this one |
| 409 on `/graph/clips`, "changed in another window or by an agent", for a clip moved right after a note was added to it | The drag took the clip's hash at the press, from a timeline that didn't show the note yet. By the drop the state had come, so `ready()` let the stale hash go. Let go before the state, the drag was dropped silently. | 3 of 56 runs under load |
| `FAIL one write` after the double-click | The suite read `/api/graph` once. The lane's file is written in the edit, and the revision is published after the settle. | A probe of lane writes: once the file had changed, the revision read then was the old one in 40 of 40 writes |

## What changed

- `web/src/lib/history.ts`: `editing()` marks an edit of the window as on its way until it ends, and keeps the undo steps its answers made (`recorded(…, edit)`). Every route that records a step uses one. A lane write waiting in the roll's queue, a lease's commit and a drag let go each count from the moment they start.
- `undoTop` (`web/src/components/GraphCanvas.tsx`): with nothing on its way, as before. With edits on their way, it takes the step at the press. That is the newest step those edits make once they are answered, or, if none makes one, the step shown at the press. Undo waits at most 2 s. Then it takes back nothing and says so. Another ⌘Z while it waits does nothing.
- `SongTimeline.tsx`'s `drop`: a drag let go waits until the timeline can take a gesture (`readySoon`), and is an edit on its way meanwhile. If the press was on an older picture, `retaken` names a clip as drawn at the drop only when the roll's last write left its lane as it is now (`wroteHere` in `web/src/lib/graph.ts`) and the clip has an id of its own. Every other clip keeps the hash drawn at the press.
- `e2e/notes.mjs`: the "one write" read waits for the revision to move. A `holder` pauses chosen answers through the browser's DevTools protocol and lets them through later. With it, six checks cover the ⌘Z and drag cases above and the review's N1 to N4 (69 checks in all).

## Choices

1. **Wait in the window, not change the server.** The server is right to refuse an undo of a step that isn't the newest (AGENT-2). The window knows which of its edits are on its way, so it waits for them.
2. **The step is chosen at the press, from the waited edits' own steps.** Reading the top when the wait ends could take back an agent's step that landed meanwhile (the review's N2). Choosing from the edits' steps keeps AGENT-2: the musician never takes back a step they didn't see named. Sent that way, an undo that isn't the newest is refused and names the newer step. It is not resent for a later step of this window's, because the press didn't mean one.
3. **2 s, then nothing.** An edit's answer takes tens of milliseconds here, even under load. Two seconds is far past that and still short enough to feel like an answer. After that, guessing a step would be wrong more often than right. So the toast says to try again, and a later ⌘Z chooses from what the window shows by then. Presses during a wait are dropped, not queued: a queue would run all at once when the answer came, taking back several steps for what the musician saw as one press.
4. **A drag names a clip as drawn at the drop only in a lane the roll just wrote.** Elsewhere the hash from the press stays, so another writer's change is still refused (WRITE-12). The roll's write is checked by content, so another writer's change after it falls back to the press's hash too. A clip with no id of its own is never looked up again: it can only be found in the revision it was read from.

## Answers to the review

- **N1** (a waiting drag isn't an edit on its way): fixed. `drop` takes `editing()` as it is let go, and `readySoon` never waits for itself. Case "⌘Z pressed while a drag let go waits": failed before (the note undone, the clip moved), passes now.
- **N2** (the step chosen when the wait ends could be an agent's): fixed. The step is chosen at the press from the waited edits' own steps, sent with no resend. Case "an agent's step landing while the window's delete is on its way": failed before (the agent's step undone), now refused, naming it.
- **N3** (no limit, no sign, a pile of ⌘Z): fixed. The wait is at most 2 s, then a toast, nothing undone. A ⌘Z during the wait does nothing. Case "⌘Z waits at most 2 s": failed before (both presses ran when the answer came), passes now.
- **N4** (an older picture lost the hash guard): fixed. A clip is named as drawn at the drop only where `wroteHere` says the roll's own write made its lane what it is. Case "a drag pressed on an older picture is refused when an agent changed the clip of another lane": failed before (the agent's change moved silently), passes now.

## Checked

- `e2e/notes.mjs`: 69 of 69. Under load, 30 of 30 at `82110ac`.
- The PR tier (`ci/check.sh --pr`), after the merge of `main` (`414bf8e`), at `7ba3f84`: PASS, every row (quick, clippy, rust, suites, renders, repo unchanged).
