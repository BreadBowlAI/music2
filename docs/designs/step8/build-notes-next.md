# Next: build notes

What each part of the next track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/s8n/plan.md`. Each part writes only under its own heading.

## P0

The lead's seams.

## Lane A

Hear it while you drag.

## Lane B

Play in: the metronome, the keyboard piano, the MIDI keyboard.

## Lane C

Settings and safety.

## Lane D

Effects and small fixes. Branch `next-d`, worktree `wt/nx-d`. Built in the order X29 sets: D4e, D1, D2, the rest of
D4, then D3. All five built; nothing of the lane is left unbuilt.

**For 8a's merge:** none. Lane D touches nothing in `core/runtime`, `core/play`, `core/os`, `crates/gates` or
`apps/`, and `Cargo.lock` is unchanged. In shared files it adds only: `crates/server/src/lib.rs` `mod routes_fx;`
(after `mod routes_view;`), `routes.rs` one `.merge(crate::routes_fx::router(…))` (after `routes_view`'s),
`graph.rs` `mod fx;` (after `mod watcher;`); `e2e/run.mjs` rows `nx-effects` (after `effects`) and `nx-nits` (after
`pl-arrange`), and their `e2e/README.md` rows.

### D4e. A first options object on its own line (Q31)

**In short**

| What | Before | Now |
|---|---|---|
| Moving the master fader in a song written over lines | `}), { db: -2.5 },` on the mixer's last line | `{ db: -2.5 },` on a line of its own, lined up with the mixer |

**What changed**

- `crates/editor/src/lens.rs`, `Insertion::plan`, the arm that writes a new options object after the last argument:
  when the call's first argument starts on a new line after its `(`, the object goes on a line of its own at the last
  argument's indent. After the last argument's comma when it has one (`\n    { db: -2.5 },`), else with a comma
  before it (`,\n    { db: -2.5 }`). A line comment after that comma stays after the new object. A call on one
  line, or one whose first argument starts on the `(`'s line (`master(mixer({\n …\n }))`), is written as before.
- The empty call written over lines (plan D4e) is not built: no std component takes only options, so nothing reaches
  it.
- History's words don't change: `fold` drops the line breaks and the comma before `)`, so it still says
  `master(…, { db: -2.5 })`.

**Tests**

- `crates/editor`: `a_first_options_object_goes_on_its_own_line` (five calls: over lines with and without a trailing
  comma, a tab indent with a line comment, one line, first argument on the `(`'s line; each undone back to the
  bytes). `the_master_fader_writes_master` (`mix_tests.rs`) now expects the object's own line. 282 of 282
  `m2-editor` tests, clippy clean.
- `s8-mix` reads the code it writes: the master's `{ db: … },` must be on its own line, between the mixer's `}),`
  and `),`. 80 of 80. `taste` 39 of 39.

**Red first**

- The test without the `lens.rs` change:
  `left: "…master(\n    mixer({ a: osc.sine(220) }), { ceiling: -3 },\n  ),…"` against
  `right: "…master(\n    mixer({ a: osc.sine(220) }),\n    { ceiling: -3 },\n  ),…"`.
- `s8-mix` on the server from before (`bin/d-70f9b61`):
  `FAIL  the master's fader writes master(…, { db: … }) on its own line: "…    }), { db: -4.8 },\n  ),\n);\n"`.

**Left:** the empty call written over lines (nothing reaches it).

**For gaps.md:** none (a fix).

**Contract:** an options object the app writes into a call written over lines gets a line of its own.

### D1. Turn an effect off and on (Q24, Q25, X10, X11)

**In short**

| What | Now |
|---|---|
| An effect card | Has a ⋯ button (on hover). Its menu says **Turn off** or **Turn on** |
| Turn off | The sound goes on without the effect at once. The card stays where it was, dimmed, and says "Reverb · Off". Its knobs stay, to set |
| In the code | `reverb(x, { size: 0.6, bypass: true })`. Turn on takes `bypass: true` out again |
| Undo | One step: "Undo turning Reverb off" |

**What changed**

- `core/script/sdk/core.ts`: a call of a component whose first argument is an audio input and whose output is audio
  (and that declares no `bypass` of its own) takes `bypass: true` in its options. The body runs as always, the frame
  records `bypass`, `produced` keeps the call's own output (the editor finds the call by it), and the call returns
  its audio input. Nothing reads the effect's nodes, so they are not in `Flat.order` and the engine never makes them
  (`core/ir/src/check.rs` keeps only what the output reaches; no warning). **The engine doesn't change.** A call of
  anything else refuses `bypass` in words ("can't be bypassed: only a call that takes audio first and gives audio
  can be"); `bypass: "yes"` is refused too. Types (X10a): `CallOptions` gains `bypass?: boolean` for such a component,
  and its call type always has the options form (`NeedsOptions`), so the code editor marks nothing.
- `core/ir/src/lib.rs`: `Frame.bypass: bool`, written only when true (ID-11): no IR of today moves.
- `crates/editor/src/effect.rs`: `bypass(s, at, call, on)`, `is_bypassed`, `bypass_refused`. On: `bypass: true` after
  the options' last property, into empty braces, `false` made `true`, or a new options object by `Insertion::plan`
  (D4e's rule for a call over lines). Off: the property out with its comma, and the object when it was alone. Words
  when it can't: the call spreads its arguments, its options are not written in the call, a comment beside it, "It has
  its own Bypass knob." The planner asserts: with the effect's own nodes taken out of the graph where it is off, that
  graph is exactly the one where it is on with the effect removed (`wraps`, as remove asserts), and the frame says so.
  Its expectation is `Seen` with a probe first (R26), as rekey's is: the call around the effect reads the effect's
  input while the effect's nodes stay unchanged, which no `Confined` range can say. One `TextEdit`, inside the call.
- `crates/editor/src/view.rs`: a node carries `bypassed` and `menu` (`EffectMenu { bypass }`: why the row can't run).
  For an effect turned off, what the call around it reads (the reader whose call holds the effect's, or the module's
  export) is drawn from the effect's card with `PortEdge.bypassed`, so the Device view keeps the card in the chain
  (X10c). `controls.rs`: `options_of` is `pub(crate)`.
- Server: `POST /api/graph/effect/bypass {session, revision, node, on}` (`routes_fx.rs`, its own `WRITES_FX` and 423
  layer; `graph/fx.rs`). Lines in shared files: `lib.rs` `mod routes_fx;`, `routes.rs` one `.merge`, `graph.rs`
  `mod fx;`.
- Web: `web/src/lib/effects.ts` (new; its types added to graph.ts's by `declare module`), `NodeGraph.tsx`: the card's
  ⋯ (`Ellipsis`, shown on hover and focus) opens a `Popover` of `MenuRow`s (X11; no Power button); a row that can't
  run is greyed with the words in its tooltip. An off card: title "Reverb · Off" in `fg-muted`, body at 50%; the
  line out of it dashed in `line-strong`. No toast (a direct edit, ST-5); Undo names it.

**Tests**

- `core/script`: `bypass_returns_the_audio_input` (the song reads the saw; the frame says `bypass`; the call's own
  output is still the delay's; `bypass: false` is on; no `bypass` in an IR that has none on), `bypass_keeps_the_frame_
  and_its_knobs` (the same constants and frames, three call forms), `bypass_is_refused_on_an_instrument` (follow,
  noise, square, mix, a notes-in instrument; a string; a component's own `bypass` stays its knob). 59 of 59.
- SDK types (`web`'s typecheck, `core/script/sdk/check`): `calls.ts` and `std.ts` gain the forms (`gain(x, -3,
  { bypass: true })`, `duck(x, l, { depth, bypass })`, `master(x, { bypass: true })`, …) and four refusals.
- `crates/editor`: `bypass_writes_bypass_true_into_the_options`, `bypass_into_a_call_with_no_options` (five forms),
  `bypass_off_takes_it_out_and_its_braces` (five), `bypass_then_unbypass_is_the_same_code` (six layouts),
  `a_bypassed_effect_runs_nothing` (to the bit as removed), `bypass_refuses_a_call_that_spreads_its_arguments` (and on
  when on, and its own Bypass knob, in the view's menu too), `a_bypassed_effect_keeps_its_place_in_the_chain`. 289 of
  289. Clippy clean on `m2-ir`, `m2-script`, `m2-editor`, `m2-server`; `m2-server` 234 of 234.
- Suite `nx-effects` (new, `fix`, 25): the menu, its tooltip and lint; Turn off writes only `bypass: true`; the card
  is off, "Reverb · Off" in `fg-muted`, body 0.5, in the same place, the line out dashed; no toast; "Undo turning
  Reverb off"; lint of the off card; the menu says Turn on; the code editor marks nothing; ⌘Z; Turn on restores the
  options; a PC page's menu. Moves: `effects` 12, `s6-builders` 56, `s5-knobs` 120, `graph` 19, `taste` 39: all pass.
- The renders: 10 of 10.

**Red first**

- `core/script` without the SDK change: `delay(): no option "bypass" (options: time, feedback, mix)`; `assertion
  failed: refused(…follow(…, { bypass: true })…).contains("follow(): can't be bypassed")`.
- SDK types without it: `calls.ts(129,14): error TS2353: Object literal may only specify known properties, and
  'bypass' does not exist in type '{ time?: …; feedback?: …; mix?: … }'`; `calls.ts(134,15): error TS2554: Expected
  1-2 arguments, but got 3.`
- `crates/editor` with `bypass` a stub: 6 of 7 fail (`called Result::unwrap() on an Err value: Rejected("stub")`).
- `nx-effects` on the page and server from before: `FAIL  the Reverb card's ⋯ menu: `, `FAIL  Turn off writes
  bypass: true into the call, and nothing else`, `FAIL  the card is off`.

**Left:** a turned-off effect bound to a name read elsewhere (`const wet = reverb(x, { bypass: true })` read later)
draws its card as a branch, not in the chain. `m2` has no bypass command (Q35). Delete and Edit in code are not in
the ⋯ menu (the card's own buttons stay).

**For gaps.md:** #21: an effect can be turned off and on (bypass), kept in its code.

**Contract:** `bypass: true` in any audio-to-audio component call's options gives its audio input on; its frame
records `bypass` (IR, written only when true); the engine never runs it. Route `POST /api/graph/effect/bypass`.

### D2. Move an effect before or after another (Q26, X11)

**In short**

| What | Now |
|---|---|
| The ⋯ menu | Also has **Move left** and **Move right** |
| Move left on Delay, after Reverb | Delay goes before Reverb. Each keeps its own settings. The cards swap |
| When it can't | The row is grey. Its tooltip says why: "Reverb and Delay aren't next to each other in the code, so they can't swap here. Change their order in code.", or "Delay is the last effect here." |
| Undo | One step: "Undo moving Delay before Reverb" |

**What changed**

- `crates/editor/src/effect.rs`: `reorder(s, at, call, earlier)`. Only two effects written one directly inside the
  other: `outer(inner(x, I), O)` becomes `inner(outer(x, O), I)`. Two edits: the heads (`outer(` and `inner(` swapped,
  the gaps after each `(` kept where they were) and the tails (`, O)` and `, I)` swapped). The sound they work on (`x`)
  isn't touched, so its nodes keep their names. Confined to the outer call. The planner asserts that the outer call
  reads the inner one's output in each graph (or the inner one is turned off), and that with both effects taken out
  the two graphs are the same. `wraps` is now built on `reads_of` and `unwrapped` (the graph without a call, one per
  input it could stand for), which the assertion uses twice.
- Words (Q26): effects next to each other in the sound but not in the code; "Delay is the first effect here." and
  "… the last effect here." for a row with no effect on that side; calls that spread their arguments.
- `view.rs`: `EffectMenu.left`, `.right`: `MoveRow { with, refused }` (the effect it swaps with, or why not).
- Server: `POST /api/graph/effect/move {session, revision, node, earlier}` (`routes_fx.rs`, in `WRITES_FX`;
  `graph/fx.rs` `move_effect`).
- Web: `moveEffect` in `effects.ts`; the menu's two rows (`ArrowLeft`, `ArrowRight`), greyed with the words in the
  tooltip, "Move Delay before Reverb" when they run. No toast: the cards swap on screen. A card kept across an edit
  that now shows another call closes its menu.

**Tests**

- `crates/editor`: `two_nested_effects_swap` (the code, the label `delay(reverb(…))` → `reverb(delay(…))`, the order
  the engine runs them, undo, and Move right on the inner one), `the_options_go_with_their_effect` (positional, none, a
  turned-off one, over lines), `an_effect_not_next_to_another_is_refused_in_words` (and the menu's rows, and the
  first and last), `moving_twice_is_the_same_code`. 293 of 293; clippy clean on `m2-editor`, `m2-server`.
- `nx-effects` gains 14 (39): Delay added after Reverb; its rows and Reverb's, greyed with why; lint of the menu;
  Move left writes the swap; the cards swap; no toast; "Undo moving Delay before Reverb"; ⌘Z; after an outside save
  that binds Reverb to a name, Delay's Move left is greyed with Q26's words. Moves: `effects`, `s6-builders`,
  `graph`, `taste`: all pass.

**Red first**

- `crates/editor` with `reorder` a stub: 4 of 4 fail (`Rejected("stub")`; `assertion failed: words(moved(&mut s,
  "delay", true)).contains(apart)`).
- `nx-effects` on D1's server (`bin/d-d1a`) with this page: `FAIL  Delay's Move left: Move left (off): Move Delay
  before undefined`, `FAIL  Move left: Delay goes before Reverb, each with its own settings`, `FAIL  the cards swap
  places`.

**Left:** a call written over lines keeps its gaps where they were, so its lines read oddly after a swap (it is still
right). Effects not written one inside the other, or in two files, are refused (Q26).

**For gaps.md:** #21: an effect moves before or after the effect written next to it.

**Contract:** Route `POST /api/graph/effect/move`; one Tx that swaps two nested effect calls, each keeping its text.

### D4. Small fixes: a, b, c, f, g (Q28 to Q33, X12)

**In short**

| Fix | Before | Now |
|---|---|---|
| a. The ruler | A marker hid its bar's number. A tempo change touched its bar's number | The ruler is a little taller. Markers on top, bar numbers under them. A change sits on the numbers' line, after its bar's number |
| b. The ruler's end | The last bar number could hide under Follow and zoom | It scrolls clear of them |
| c. A drum lane | Its note menu said "Up a semitone" and offered octaves; black keys behind the drum names | "Up a row", "Down a row", no octaves; every row alike |
| f. Shift + wheel over a fader | On Linux it moved the fader | It scrolls the strips, on every OS |
| g. `s8-mute`'s lint | Its own weaker copy | The shared one (`e2e/lintlib.mjs`) |

**What changed**

- a (X12): `layout.ts`: `RULER_H` 28 → 36, the timeline's; `ROLL_RULER_H = 28` serves NoteView's three uses (its
  import line and the three uses only). `index.css` `--spacing-ruler: 36px`. `dockMax` follows. `SongTimeline.tsx`
  `Ruler`: the markers keep their 14 px band at the top; the bar numbers sit at 17 px (`leading-none`), each in a
  row with the change at its bar line, 6 px after the number; a change off a bar line sits 3 px after its beat on
  the same line. Beat ticks and the loop brace stay at the foot. `Markers.tsx`: its header comment only.
- b (Q29): the timeline's width is the song's room plus `TOOLS_W`; and no bar number is drawn where it would start
  under the tools at the ruler's very end.
- c (Q30): `NoteMenu.tsx` takes `named`: "Up a row" ↑, "Down a row" ↓, no octave rows. `NoteView.tsx`: `Keys` draws
  every row of a named lane alike, a line under each, no black keys; the kit's grid shades no rows (`shaded` is
  empty; `Lanes` reads an empty list as none). `pl-sounds` followed: the kit's rows are unshaded, by a key or as
  black keys.
- f (Q32): `Mix.tsx`, the fader's wheel: with Shift it never steps; deltaY with Shift scrolls the strips
  (`[data-testid=mix-strips]`) itself; deltaX was the browser's already.
- g (Q33): `s8-mute` lints with `lintlib.mjs`; its count stays 152.
- Suites that followed the ruler: `pl-zoom` (the tools are 36 px tall), `s8-songbar` (a bar number's place is its
  row's `left`).

**Tests**

- Suite `nx-nits` (new, `fix`, 26): a marker at bar 5 and its number apart, the point at the number's middle is the
  ruler and no marker; the ruler 36 px, the markers' band 14 px at its top; a tempo change at bar 9 says ♩140, 4 px
  after "9" (boxes; its text 6 px), on one line; one off a bar line 3 px after its beat; lint of the ruler; zoomed out
  to 64 px bars and scrolled to the end, the last number (18) ends left of the tools; a Drum kit's note menu, its
  keys 60 and 61 alike, its grid unshaded, both linted; on a PC page Shift+wheel up and down over Hats' fader leaves
  it and scrolls the strips; a Mac page's sideways the same; lint of the Mix view; the wheel wrote nothing.
- Moves, all pass: `pl-zoom` 62, `pl-arrange` 57, `s8-clips` 94, `s8-automation` 45, `s8-songbar` 82, `pl-notes`
  102, `pl-mix` 47, `s8-mix` 80, `s8-mute` 152, `taste` 39, `fx-fixes` 75, `pl-sounds` 45.
- The narrow strips: Shift+wheel is checked at 380 px wide (as `s8-mix`'s sideways check), where first-song's strips
  overflow; at 1100 px its five strips fit and nothing can scroll.

**Red first**

- `nx-nits` on the page and server from before: 13 of 26 fail, among them `FAIL  nothing of the marker is at the
  middle of "5": marker Chorus`, `FAIL  "9" and "♩140" are at least 4 px apart: -12`, `FAIL  scrolled to the end,
  the last bar number (19) ends left of Follow and zoom: 1412 < 1355`, `FAIL  its note menu moves a row up or down:
  up:Up a semitone↑ | …`, `FAIL  row 61's key looks as row 60's: {… "61":{"black":true,"hit":"black",…}}`, `FAIL  a
  PC page: Shift+wheel up and down leaves Hats' fader: -6 → -6.3`.
- g: the old copy and lintlib on one page with a 14 px text field and a `::before` gradient: `old inline lint: []`,
  `lintlib: ["font-size 14px at <input>","gradient ::before…"]`.

**Left:** nothing of D4 (d is in a).

**For gaps.md:** none (fixes). taste.md §3.2 at the End: the ruler is 36 px, its rows (markers 14 px band; numbers
and changes at 17 px; ticks and loop at the foot).

**Contract:** `RULER_H` 36 (timeline), `ROLL_RULER_H` 28 (roll).

### D3. Pick what Duck listens to (Q27, X28)

**In short**

| What | Now |
|---|---|
| The Duck card | Its side row says **Listens to: Kick ▾** |
| Click it | A list of the song's other tracks, by name. Pick one: the duck now dips when that track plays |
| In the code | `duck(bass.out, kick.env, …)` for a track that has an `env`; `duck(bass.out, follow(hats.out), …)` for any other, and `follow` is imported |
| Undo | One step: "Undo Bass's duck listens to Hats" |
| A Duck in a track's own file | Greyed. Its tooltip: "Bass's Duck is in tracks/bass.ts, which can't hear other tracks. Change it in code." |

**What changed**

- `crates/editor/src/effect.rs`: `sidechain(s, at, call, track)`. The Duck (`std:duck`; the effect browser doesn't
  list it, so it is found by its frame's ComponentId, not `effect_at`) has its side argument (`args[1]`) replaced
  with `<handle>.env` when the track exports `env` (X28), else `follow(<handle>.out)`, importing `follow` from
  `std/dsp` when the file doesn't. `<handle>` is the name the Duck's file default-imports the track by
  (`sites::imports_of`). Refused in words: a Duck in a track's file, a track its file doesn't import, the track it
  listens to already. `Confined` to the argument and the import line (the new `follow` or import node is inside
  them, so the Duck's rewired side is allowed). The planner asserts the Duck's side now comes from that track (an
  import of its export, or a `follow` of one).
- `listens(s, site)` and `Listens { port, ducks, track, choices, refused }`: what the row shows; the choices are
  the tracks the file imports, but the one it ducks, sorted by name. `view.rs`: `ViewNode.listens`, its `port`
  filled with the side input's row.
- Server: `POST /api/graph/effect/sidechain {session, revision, node, track}` (`routes_fx.rs`, in `WRITES_FX`;
  `graph/fx.rs` `listen_to`).
- Web: `listenTo` in `effects.ts`; `NodeGraph.tsx` `ListensRow` in place of the side input's row: "Listens to:",
  the track's name and `ChevronDown` (a 20 px button), its list a `Popover` of `MenuRow`s with a check on the
  current one. No toast (the row says it); Undo names it.

**Tests**

- `crates/editor`: `a_track_with_no_env_gets_follow_of_its_out_and_the_import` (the starter's Bass duck: its row
  says Kick and offers Clap, not Bass; `kick.env` → `follow(clap.out)` and the import; undo puts both back),
  `the_picker_writes_the_tracks_env` (Chords: Hats, then Kick writes `kick.env`; Kick again is refused),
  `a_duck_in_a_track_file_is_refused_in_words` (and its row says so). 296 of 296; clippy clean.
- `nx-effects` gains 8 (47): the row says "Listens to:Kick", its list "Hats,Keys,Kick", lint of the row and list,
  picking Hats writes `follow(hats.out)` and the import and nothing else, the row says Hats, "Undo Bass's duck listens
  to Hats", ⌘Z. Moves, all pass: `effects` 12, `s6-builders` 56, `s5-knobs` 120, `graph` 19, `ports` 46,
  `fixes-graph` 42, `taste` 39, `s8-mix` 80, `pl-ids` 39, `s8-tracks` 75, `pl-tracks` 20. `m2-server` 234 of 234.

**Red first**

- `crates/editor` with `sidechain` a stub: 3 of 3 fail (`Rejected("stub")`).
- `nx-effects` on D2's server with this page: `FAIL  the Duck's side row says what it listens to: null`, `FAIL  picking
  Hats writes follow(hats.out), and imports follow`, `FAIL  Undo: Undo (⌘Z)`.

**Left:** a `follow` import left behind when the Duck goes back to a track's `env` (harmless; unused). A track that
exports another control signal than `env` (an LFO) is still heard through `follow(<its>.out)` (X28). `m2` has no
command for it (Q35).

**For gaps.md:** #21: a Duck's sidechain is picked from a list of the song's tracks.

**Contract:** Route `POST /api/graph/effect/sidechain`; the view's `listens` on a Duck card.

## Merge

## End
