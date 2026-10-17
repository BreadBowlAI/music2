# Editing fixtures

`first-song` is the example project as of commit `283af19`, before its controls
were changed while exercising the canvas, converted to format 1 (`data/`, track
keys) by the step 4 conversion script. Editing tests depend on its original
written numbers, comments, units, note pitches, and tempo. Each test copies this
fixture to its own temporary directory before making changes.

Keep these inputs stable when adjusting the example song. The complete project,
including its `data/` folder, is stored here so demo edits cannot silently change
the test's starting state. Update the fixture only when intentionally changing the
behavior covered by its assertions.

`sidechain/env` and `sidechain/follow` are gate 8's ducking fixtures (8b.4): a kick
on every beat, a bass drone ducked to it, and std's mixer. In `env` the duck reads
the kick's envelope, a control export the kick track makes (`kick.env`); in `follow`
it reads `follow(kick.out)`, written in `song.ts`, whose input is a sidechain input.
`core/play/tests/mute.rs` edits their `song.ts` by exact text: keep the
`track("kick", { name: "Kick" }`, `mixer({ kick: kick.out, bass:` and sidechain
expressions as written, or update the test with them.
