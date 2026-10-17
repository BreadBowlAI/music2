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
D4, then D3.

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

## Merge

## End
