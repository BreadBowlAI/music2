# Editing fixtures

`first-song` is the example project as of commit `283af19`, before its controls
were changed while exercising the canvas. Editing tests depend on its original
written numbers, comments, units, note pitches, and tempo. Each test copies this
fixture to its own temporary directory before making changes.

Keep these inputs stable when adjusting the example song. The complete project,
including its timeline, is stored here so demo edits cannot silently change the
test's starting state. Update the fixture only when intentionally changing the
behavior covered by its assertions.
