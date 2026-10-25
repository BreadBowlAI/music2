# How your changes reach the song

**In short**
- With the app open, `m2` hands each write to the app: one undo step, named by `--as` and `--why`.
- When the musician holds what a change touches, `m2` waits, then exits 3 (busy). Never write the files directly then.
- With the app closed, `m2` writes the files itself.

## With the app open

While the app has a song open, the commands that write it (`set`, `automate`, `unautomate`, `label`, `add-track`, `import`, `mute-in-song`, `clips`, `duplicate-track`, `delete-track`, `layout`, `tempo`, `meter`, `title`, `connect`, `disconnect`, `add-effect`, `remove-effect`, `relink`, `delete-data`, `fix`, `tx`, `plugin-state`, `edit`, `write` and `bounce --place`) hand their edit to the app. `m2` builds and checks it as always, then the app writes it as one undo step, the agent's, and `m2` prints what it would have printed, then "in the open app: one undo step, Claude: brighter hats" (on stderr for `tx`, whose answer stays JSON alone). `--as <name>` (else `M2_AS`) says who you are, and `--why "<words>"` names the step; both go last, after the command's own arguments, so text you give `--old` or `--new` is never taken for them. Both are shown as plain text, at most 80 characters, without invisible characters; a name that reads as the app's own words ("Changed outside the app") is shown as `agent`. Without a name the step is `agent`'s, and `m2` says so once. Two agents at once should use two names.

## When the musician is busy

When the musician holds what the edit changes (a drag, a take, V's Compare pill, a plugin window not yet saved), or the edit would end what they hold (a change to `song.ts` that takes out the track whose knob they drag), `m2` waits, saying "waiting: you're dragging Bass cutoff in the app", and tries again every quarter second for up to 30 s; unsaved code in the app's code editor is refused at once. Then it gives up, changing nothing, with exit status 3 (busy: try later). Don't write the file directly instead: the app can't refuse that, and it would end what the musician holds. If the app closes while it writes, `m2` reads the files to say what happened: saved (but its undo step went with the app), cut off (with the `m2 fix` commands that finish or put it back), never started, or that it can't tell. A song the app has open that `m2` can't reach (no `~/.music2/session.json`, or one naming another program) changes nothing, and says so. While the song's code doesn't build, `m2` can't plan an edit: `m2 edit` and `m2 write` fix the code (above), and the other commands wait for it to build. An edit is sent whole, every file it changes as it was and as it will be, and the app takes up to 144 MiB at once; `m2` says so for a bigger one and sends nothing (with the app closed it writes it itself). With the app closed, `m2` writes the files itself; its edit has no undo once `m2` exits.

## Try it on a copy

Commands that change a song write into its folder, so try them on a copy, not on the examples:

```bash
m2 new /tmp/m2-scratch --example first-song                    # a copy of the example
m2 check /tmp/m2-scratch
m2 render /tmp/m2-scratch out.wav
m2 controls /tmp/m2-scratch                                     # ids look like song.ts#2/0.hats.pan
m2 set /tmp/m2-scratch 'song.ts#2/0.hats.pan' 0.45
m2 set /tmp/m2-scratch 'tracks/hats.ts#1/@decay' 0.08            # a control the call leaves out: writes { decay: 0.08 }
m2 add-effect /tmp/m2-scratch 'tracks/hats.ts#1' std:delay

m2 new /tmp/m2-resample --example resample
m2 bounce /tmp/m2-resample keys-again --track keys              # render the keys track to assets/keys-again.wav
```

## How a knob is named

An id names a file, a call in it and an argument. `song.ts#2/0.hats.pan` means: in `song.ts`, call number 2 (here the mixer), its first argument (`0`), and `hats.pan` inside that. A control the call leaves out is named by its call and `@` with the control's name: `tracks/hats.ts#1/@decay`.

## Editing the files directly

Or just edit the files, while the app is closed. While it has the song open, use `m2 edit` and `m2 write` instead, for `layout.json` too: the app can refuse those while the musician holds the file, names them, and `m2 undo` takes them back. A file you write directly is an outside write: the app takes it in once it has settled (no file changed for 150 ms), usually within a quarter of a second, as one undo step ("Changed outside the app: …", which ⌘Z takes back), but it can't be refused, so it ends a drag on what it changes; a code editor with unsaved text in that file asks which to keep; and `m2 undo` can't take it back. Some outside changes can't be undone in the app at all: a changed or removed audio file or plugin state (its old bytes are gone), a file that is a link or renamed only in capitals, and whatever git writes (git can take it back). `layout.json` alone makes no undo step either: the app just shows it.
