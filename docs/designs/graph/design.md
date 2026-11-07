# The graph: nodes you expand

| | |
|---|---|
| **Status** | Proposed, for the owner's yes (AGENTS.md rule 11). Revised after the skeptic's review (G1 to G22, answered in §14). Nothing here is built. |
| **Written** | 2026-11-07, from `main` at `841ea608`. The owner asked for it in these words: "when I say card I mean each node in the graph", "rename it graph from patch", "show the collapsed card in the graph, only when the user clicks expand will it expand", "just have collapsed and expand", and "the design and architecture needs to be simple yet elegant". |
| **Replaces** | Device's Chain view and its three card sizes: [Cards you play with](../cards2/design.md) §3 and its D1, D4 and D10, and [taste.md](../step8/taste.md) §3.5 and DV-1, DV-11 to DV-16 and DV-18. §9 maps every rule. |
| **Mockup** | [a picture](images/mockup.png): Keys, in the dock's real height, with Pluck and Lowpass collapsed and Reverb expanded. The clickable page it was taken from is kept outside the repo, on the maintainer's server (`$SCRATCH/graph-note/mockup.html`, [paths](../README.md#paths-in-these-notes)). |

**In short**
- **One view, the Graph.** Device's Chain · Patch switch goes, and the dock's Device tab is renamed Graph. Every box in it is a **node**.
- **A node that is a call has two sizes.**
  - It starts **collapsed**: its inputs, its main picture (live), and up to four values.
  - **⤢ expands it**: its pictures across the top, then every group of values side by side, then its code line.
  - Which nodes are expanded belongs to the window, and an edit never moves it to another node.
- **Five primitives:**
  - a **node** (a call);
  - its **inputs** and **outputs**;
  - a **wire** (one call reading another);
  - a **value** (a number, a choice or a switch written in the call);
  - a **picture** (the pack's drawing).
- **One value control at both sizes: the bar.** It shows the label, the number and how far along its range the value is. It drags up and down, as today's knobs do.
- **The edits are today's, plus one.**
  - Today's: set a value; connect and disconnect a value (today's cables); remove, move and bypass an effect.
  - New: **+ on a wire** puts an effect or a note effect there, and offers only what fits that wire. It replaces + Effect.
- **The core gains nothing.**
  - The editor's add route gains one case, "on a wire".
  - The view gains a key for each node that edits keep.
  - Packs don't change: their faces and groups already say what each size shows.
- **Not in this note:**
  - wires that show their level (that needs a new tap, so it gets its own note);
  - pulling a new node out of an input.

## 1. What is wrong today

- **Two views of one thing, drawn twice.**
  - Patch is the graph, with every value a row with its port. It can fold a card to its first group (CARD-25), but every card starts unfolded, so its nodes are long. A Polysynth is a column of about twenty rows, and a track's graph is mostly scrolling.
  - Chain is cards in a row, offered only when the sound is a line (DV-11).
  - The two use different code: `NodeGraph.tsx`'s card, and `Chain.tsx` with `frame.ts`. Between them come three sizes, tabs, a switch, and the rules for when Chain may show.
- **Chain hides the structure.** A modulator has no card, a sidechain is a menu row, and a sound that isn't a line greys Chain out. The owner put it this way: "a chain doesn't capture it, because it's a one-to-one, whereas a patch is like a graph."
- **Two layouts breed their own flaws.** The cards in 0.2.0 show axis labels over pictures, a Polysynth card half empty, and a label cut to "S".

## 2. The primitives

| Primitive | In the code | Drawn |
|---|---|---|
| **Node** | A call, `reverb(pluck, { size: 0.6 })`, or one of the places a sound comes from or goes to | See the kinds below. |
| **Input, output** | A call's argument that is a sound, notes or a control; the call's result | A shape on the node's edge: inputs on the left, outputs on the right. Shapes as taste.md §2.8 has them, with no colour: audio is a circle, control a diamond, notes a rounded square. Filled means connected, hollow means not. |
| **Wire** | An argument that reads another node's output | A curve, as §2.8's cables: audio 2 px, control 1.5 px, notes 1.5 px dashed. A **+** shows on it on hover, and on the selected node's wires (§3). |
| **Value** | An argument written in the call: a number, a choice or a switch ([CARD-10](../../architecture.md#kinds-of-values)) | A **bar** (§4). A value that a wire, a lane or code drives is instead a row that names its source. |
| **Picture** | One of the pack's views ([CARD-60](../../architecture.md#pictures-on-the-card)): a drawing, a scope, a keys view | 48 px tall collapsed and 132 px expanded (DV-19's heights), with its live layers and dots as today. |

**The kinds of node**

| Kind | Sizes |
|---|---|
| A call: an instrument, an effect, a note effect, a modulator, a building block | Collapsed and expanded |
| Where a sound or notes come from: a notes lane, the time, another track's sound, a voice's notes | One: a pill with its name and its outputs |
| Where it goes: the module's export (Out) | One: a pill |
| The Song's mixer | One: today's compact card with Open Mix ([MX-6](../step8/taste.md)) |

**The window's own state:** which nodes are expanded, which one is selected, and the graph's scroll and zoom. None of it is saved or sent.

**Why these primitives:** they are the program's own parts, so nothing drawn is missing from the code. A musician and an agent see the same thing, and a beginner learns how a sound is made by looking at it.

## 3. The edits

Each edit is one undo step and one change to the code.

| Edit | Gesture | The code | Today? |
|---|---|---|---|
| Set a value | Drag a bar up or down, type into it, press ⌫ to reset it, or drag a picture's dot | The written value changes | Yes |
| Connect a value | Drag from a control output onto a bar. On a collapsed node, drop onto the node: a menu lists its values that fit, and dims the rest, saying why (DV-6). | The value becomes the name of a `const`, as today's cables: from the same file, declared above the value; else "name it in code first" | Yes |
| Disconnect | ✕ at the wire's end | The value goes back to its input's declared default, as today | Yes |
| Insert on a wire | **+** on the wire, then pick from a list of what fits it: effects on audio, note effects on notes (today's EffectBrowser, filtered) | See below | New |
| Remove | ⋯ ▸ Remove, or Delete on the selected node | Unwraps it, `b(x(a))` to `b(a)`. Never deletes anything upstream. | Yes (effects, and note effects on notes-sdk) |
| Move | ⋯ ▸ Move left, Move right | Today's reorder | Yes |
| Bypass | ⏻ in the header | `bypass: true` ([PKG-3](../../architecture.md#component-descriptors)) | Yes |
| Make it a kit | ⋯ on the drum machine | [KIT-24](../../architecture.md#drum-kits) | Yes |

**Where they are offered:**
- **⏻** shows only where bypass works: a component call whose sound in and sound out are both audio, or both notes, and that has no bypass of its own.
- **Remove** is offered on an effect or a note effect: a component call with one input of its output's kind (a sidechain aside), whose other arguments are values. On anything else, ⋯ shows Remove greyed, with why in words. For example: "Sum mixes two sounds: change it in code." "An instrument is changed with Change instrument."

**Insert, exactly.** `POST /api/graph/effect/add` keeps its one request and gains a second form:
- `{call, item}`, as today: put the item **after a call**, so every reader hears it. `m2 add-effect` keeps this form.
- `{reader, input, item}`, new: put the item **on one wire**, wrapping the argument `input` of the call `reader`. So `reverb(p)` becomes `reverb(x(p))`, while another reader of `p` is unchanged.

The **+** on a wire uses one of the two forms:
- the second, when the wire ends at an argument of a call;
- the first, when the wire ends at an export or a mixer channel and its source call is read once.

On any other wire the + is greyed, with why. That covers a helper function's parameter, a spread, an array made by `.map`, a source read more than once, and CARD-36's refusals. A + inside a voice puts the effect in each voice.

## 4. A node, collapsed (the default)

From top to bottom:

1. **Its header**, 24 px tall:
   - ⏻ where bypass works, then its label;
   - **+N** when N values are hidden;
   - ⋯ (Remove, Move, Show in code, Copy its id for `m2 add-effect`), and ⤢;
   - an instrument has its track's colour along its top;
   - a node that plays inside an instrument's voices says **per note**;
   - the pack is the label's tooltip.
2. **Its lines**, under the header, where they let you do something:
   - the voices line ("Plays up to 12 notes at once");
   - the key line with Set the key ([VOX-26](../../architecture.md#vocals));
   - a plugin's row: Open window, Latched, Re-enable ([HOST-7](../../architecture.md#pluginhost)).
3. **Its inputs**, one row each, with the input shape on the left edge:
   - every sound or notes input;
   - every value a wire drives;
   - an unwired Listens to or Cut by, with its **None ▾** ([CARD-35](../../architecture.md#listening-to-another-track)). Picking a track draws it as a pill, wired in.
4. **Its outputs**:
   - output *i* is on the right of input row *i*, so a line of nodes draws straight, as today;
   - outputs beyond the rows get rows of their own;
   - a node with no input rows puts its first output on its header.
5. **Its face's first view**, of any kind, 48 px tall and live.
6. **Up to four of its face's values that you can set**, as bars, two to a row. A label that doesn't fit is shortened, and its tooltip says it whole.

**What +N stands for.** +N stands for every value it hides (DV-16's rule, kept):
- it lights when an agent changes one ([AGENT-5](../../architecture.md#agents));
- an agent's pointer at one shows on it;
- Show expands the node and scrolls to that value ([AGENT-6](../../architecture.md#agents));
- clicking it expands the node.

**Size.** A collapsed node is 216 px wide, as Patch's node is, and 60 to 190 px tall. In the dock's first height (236 px at 100%), a line of nodes fits. A sound where two branches meet fits at Fit's 75% (measured on the mockup's four examples).

**The face** is the descriptor's face ([CARD-41](../../architecture.md#the-cards-frame)), else its first group, else every value: DV-12's rule, kept.

## 5. A node, expanded

1. **Its header**: as collapsed, with its pack and Presets.
2. **Its lines, inputs and outputs**: the same rows at the same places. So expanding a node never moves the end of a wire on it.
3. **Its face's views across the whole width**, 132 px tall. For example, the EQ's curve, over its spectrum, as wide as the node.
4. **Every group as a column**, 176 px wide, side by side, wrapping after four columns.
   - Each value is drawn once, as a bar. A choice of five or fewer is a segmented control (DV-14). A group's own pictures sit at the top of its column.
   - Face values that are in no group go first in the first column. Values in no group go in a last column, More. Settings go in Setup, after the values ([DV-8](../step8/taste.md)).
   - Each value's tooltip gives its id for `m2 set`, as All did.
5. **Its code line** ([CARD-41](../../architecture.md#the-cards-frame)), wrapped to the node's width.

An expanded node has no tabs, so it hides nothing. A dot whose `opens` names a group lights that group's column and scrolls the graph to it. That was DV-18's tab. So an expanded EQ shows its curve across the top, and a click on band 8's dot brings band 8's column into view.

## 6. The layout

- **It doesn't change in kind.** `layout(module, folded)` in `NodeGraph.tsx` is already a pure function of the graph and each node's size: columns by depth, each source just before what reads it, a line drawn straight, and loops drawn round below.
- **Expanding a node only changes its size.**
  - The layout runs again, and the other nodes move aside over 280 ms.
  - The zoom stays as it is, so Fit isn't worked out again. The graph scrolls the node's top into view.
- **The expanded set is kept by a node's key.**
  - The view gives each node a key that edits keep ([ID-13](../../architecture.md#derived-identity)), as [TAP-9](../../architecture.md#taps)'s scopes follow a call.
  - Today a node's id is its place among its file's calls, so an insert would move "expanded" to the next node.
  - A node added by + starts collapsed.

## 7. Keyboard and selection

These keys rewrite DV-15.
- **Click a node's header** to select it. The selected node has a ring, and its wires show their +.
- **Tab** goes through the nodes in layout order. Inside a node, it goes through the header's buttons and then the values.
- **Enter** on a header expands or collapses its node.
- **Delete** removes the selected node, where Remove is offered.
- **Esc** clears the selection.
- **A value keeps DV-15's keys:** the arrows, PageUp and PageDown, Home and End, and ⌫ to its default. It also keeps its focus across its own write.
- **Collapse all** sits beside Fit in the graph's zoom bar.

## 8. Words

| Today | New |
|---|---|
| Device (the dock's tab), Patch, Chain | Graph |
| card | node |
| cable | wire |
| Fold, Normal, Expand | Collapse, Expand |

- **`m2 focus`**: the focus record's `view` says Graph, not Device (`crates/server/src/graph/focus.rs`). Agents read this.
- **The guide:** eleven pages name Device, Chain, Patch or "+ on a box": `window.md`, `sounds.md`, `making-a-pack.md`, `plugins.md`, `packs.md`, `music-basics.md`, `first-song.md`, `what-you-can-do.md`, `shortcuts.md`, `samples.md` and `audio.md`.
  - The first song's step 5 becomes "press + on the wire after the sound".
- **Rule ids** keep their numbers.

## 9. The rules: kept, rewritten or removed

The contract changes first, in track 1's first commit.

| Rule | Becomes |
|---|---|
| taste.md §3.5, DV-1 (the chain is Device's default), DV-11 (when Chain shows) | **Removed:** one view |
| DV-5 (port shapes, cables, Fit 75–100%), DV-6 (a drag offers only what fits), DV-9 (keys view), DV-10 (scope), DV-17 (pictures in music2's style), §2.8 | **Kept.** DV-6 also covers a collapsed node's value menu. |
| DV-7 (+ after a node; delete) | **Rewritten:** + on wires (§3); Remove in ⋯ and with Delete |
| DV-8 (settings after the values) | **Kept**, in an expanded node's Setup column |
| DV-12 (a card's frame), DV-13 (three sizes) | **Rewritten:** a node at two sizes (§4, §5) |
| DV-14 (a card's values as knobs in a grid) | **Rewritten:** values are bars; a choice of five or fewer is segmented, expanded |
| DV-15 (the keyboard) | **Rewritten:** §7 |
| DV-16 (what a hidden knob stands for) | **Rewritten:** +N (§4) |
| DV-18 (a dot shows its group's tab) | **Rewritten:** it lights and scrolls to its column (§5) |
| DV-19 (pictures 48, 104 and 132 px) | **Kept** at 48 collapsed and 132 expanded; Normal's 104 goes |
| CARD-25 (a folded patch card) | **Rewritten:** collapsed, the default |
| CARD-35 (Listens to) | **Kept**, as an input row with None ▾ |
| CARD-36 (+ on a track's sound in the mix) | **Rewritten:** + on the wire into the mixer |
| CARD-41 (the face, the code line), CARD-60 (pictures) | **Kept** |
| CARD-63 (a Folded card's sentence) | **Removed:** a picture's words stay for agents and tooltips |
| PKG-19 ("so + Effect adds them") | **Words:** "so + on a wire adds them" |
| VOX-26, KIT-24, HOST-7, MX-6 | **Kept**, in the places §3 and §4 give them |
| AGENT-5, AGENT-6 | **Rewritten:** +N stands for hidden values; Show expands the node |
| TAP-10's words ("Fold a card or scroll one away") | **Words:** "Collapse a node or scroll one away" |
| cards2 D1 (a face and tabs) | **Kept:** the face; the tabs go, since expanded shows every group |
| cards2 D4 (groups as tabs, with All) | **Rewritten:** groups as columns; All goes, and each value's tooltip names its id |
| cards2 D10 (Device without ports; cables only in Patch) | **Reversed:** one view has both |

## 10. What goes, and the suites

**What goes from `web/`:**
- `Chain.tsx` (559 lines);
- most of `lib/frame.ts` (141), and `useCards` in `lib/folded.ts`;
- the switch, the tabs, the third size, the knobs in the graph, and + Effect in the headers.

**Checks that go with their features** (AGENTS.md rule 3: with a contract change, made first):
- `frame`'s chain checks, most of its 81: the chain shown, its widths, Fold and its sentence, the tab bar's fade and arrows, All;
- the four chain runs: nx-effects (46), sidechain (18), card-views (41) and tune (30), 135 checks in all.

Their behaviours that stay move to a new `nodes` suite:
- two sizes and +N;
- connecting to a hidden value;
- insert on a wire, and its refusals;
- Remove's refusals;
- the keyboard;
- the expanded set kept across an insert.

**Suites that change:**
- **`ports`:** its port shapes and ✕ stay, as §2.8 stays. It gains the collapsed node's rows.
- **The 32 suites that use a knob in the graph:** bars keep `data-knob`, and they drag up and down as knobs do. A suite whose value is on the collapsed face runs as it is. The others expand the node first, with one helper.
- **The 35 suites that set `m2.dock.device`:** they set `m2.dock.graph`, one line each.

**The estimate:**

| Part | Lines |
|---|---|
| `web/` | about −900 and +700 |
| The editor (the insert form and the refusals) | about +150 |
| The server (the node key, the focus word) | about +40 |
| `e2e/` | about 50 suites touched, mostly by one line; four runs gone; one new suite |
| The guide | eleven pages |

## 11. What the core gains

| Part | Change |
|---|---|
| The kernel ABI, the IR, the runtime, the taps, the file formats | Nothing |
| The editor | The add route's second form, and Remove's and +'s refusals in words |
| The server | A node key that edits keep, and the focus record's word |
| The packs | Nothing |

## 12. Build order

Each track has a builder, a skeptic and the builder again, and each changes the contract first.

| # | Track | Builds | Size |
|---|---|---|---|
| 1 | `graph-node` | The contract (§9). The node at two sizes, the bar, +N, the lines and rows, selection and the keyboard, the Graph tab, the node key, Chain removed, and the suites moved. | large |
| 2 | `graph-insert` | + on wires, the add route's second form, Remove's and +'s refusals, and the add buttons gone. | medium |

The events note's track 4 (`notes-chain`) folds into these, since a note effect is a node on the notes wire. Its card is the `@m2/notes` pack's own (track 3).

## 13. Decisions for the owner

| # | Question | Recommended |
|---|---|---|
| D1 | Chain goes. The dock shows one view, the Graph, and its tab is called Graph. All right? | **Yes.** It is the owner's own ask. |
| D2 | Two sizes, collapsed (the default) and expanded, by ⤢. All right? | **Yes.** It is the owner's own ask. |
| D3 | Expanding a node pushes the others aside, and the zoom stays as it is. All right? | **Yes.** The wires stay true, and nothing jumps in size. |
| D4 | + on a wire replaces + Effect. It offers only what fits the wire, and it is greyed, saying why, where it can't work. All right? | **Yes.** It is one gesture, and it is honest. |
| D5 | One value control in the graph, the bar, in place of knobs. It shows the label, the number and how far along the value is, and drags up and down. All right? | **Yes.** It is one control, it takes half the room, and it shows the exact number. A knob shows none of that at a glance. |
| D6 | The words become Graph, node, input, output, wire, Expand and Collapse. All right? | **Yes.** They are the program's own words, and Expand is the owner's. |

## 14. The review, answered

| Id | Finding | Answer |
|---|---|---|
| G1 | The glow can't run on meter slots; rings are 31 and shared; many wires can't be looked at | Agreed. The glow is cut from this note. A wire's level needs a new tap (a sound port's block peak in a meter slot), so it gets a note of its own. |
| G2 | No way to wire to a hidden value | Drop onto a collapsed node: a menu of its values that fit (§3). |
| G3 | Connect and Disconnect aren't today's cables for sound and notes | Agreed. Connect and Disconnect are for values only, as today, and Disconnect writes the declared default (§3). Rewiring a sound is for code. |
| G4 | Remove and ⏻ offered where they can't work; the mockup's Remove deletes upstream | Each is offered only where it works, and is greyed elsewhere with why. Remove never deletes upstream, and the mockup now does that too (§3). |
| G5 | The key line, Make it a kit, the plugin row, the mixer, Listens to: None, Move, Automate have no place | Each has its place (§3, §4): lines, ⋯, the mixer's one size, the input row with None ▾. The bar keeps the knob's menu. |
| G6 | The suite claim is not honest | Counted again (§10): what goes, what changes, and what moves. |
| G7 | One request shape can't say "after a call"; some wires have no argument | The route keeps "after a call" and gains "on a wire". The + chooses one, or is greyed, with the cases named (§3). |
| G8 | The expanded set follows a node's number | It is kept by a key that edits keep (§6). |
| G9 | Collapsed nodes too tall for the dock; Fit re-zooms | Collapsed now has up to four values, a 24 px header, and no "← source" text. Fit isn't worked out again. The mockup uses the dock's real height (§4, §6). |
| G10 | The expanded EQ's curve shrinks | The face's views span the whole width at 132 px. A dot scrolls to its column. The mockup has an EQ (§5). |
| G11 | Port shapes against §2.8 | §2.8 is kept as it is (§2). |
| G12 | The rule mapping is incomplete | §9 maps every rule. |
| G13 | Hidden values and agents | +N stands for them (§4). |
| G14 | Gestures collide; Delete has no selection; no keyboard rule | One gesture each: Disconnect is the ✕, and pulling into space is cut. Selection and the keyboard are in §7. Values' ids are in their tooltips. |
| G15 | `poly` voices drawn flat | A node in a voice says "per note"; a + there puts the effect in each voice. A frame round a voice is left for later. |
| G16 | Pulling into empty space nobody asked for | Cut. |
| G17 | Not every node is a call; outputs after the first | The kinds of node table (§2); outputs (§4). |
| G18 | Claims about the code | Corrected: the voices line, the ids, + Note effect not built, EffectBrowser's list, the agents' focus word, Patch's fold, 48 px. |
| G19 | Words | "Expand", the owner's word; no "glow"; inputs and outputs, not ports. |
| G20 | Two widgets, "← Pluck", a + on every wire, Collapse all | One widget, the bar. No "← source". + shows on hover and on the selected node's wires. Collapse all is in §7. |
| G21 | Other views on a face; values driven by lanes or code | The face's first view of any kind; driven values are rows naming their source (§2, §4). |
| G22 | The guide pages and the estimate | Eleven pages, and the estimate counts everything it changes (§8, §10). |

## 15. The mockup

[The picture](images/mockup.png) is the Keys example in the dock's real height: a Chords lane, Pluck collapsed, Reverb expanded with its tail across the top, and Lowpass collapsed.

The clickable page has five examples:
- **Keys:** a line of nodes;
- **Wobble:** an LFO wired into a VCA's Gain;
- **Sidechain:** a compressor listening to the Kick track;
- **Arpeggiator:** a note effect on a notes wire;
- **EQ:** an expanded EQ whose dots scroll to their bands.

In the page:
- ⤢ expands a node, and values drag up and down;
- + on a hovered wire inserts a node, and ⋯ ▸ Remove unwraps one;
- Play moves the pictures' live layers.

The sound and the numbers are made up.
