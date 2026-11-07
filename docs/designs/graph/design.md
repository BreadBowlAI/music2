# The graph: nodes you open

| | |
|---|---|
| **Status** | Proposed, for the owner's yes (AGENTS.md rule 11). Nothing here is built. |
| **Written** | 2026-11-07, from `main` at `841ea608`, after the owner asked for it: "when I say card I mean each node in the graph", "rename it graph from patch", "show the collapsed card in the graph, only when the user clicks expand will it expand", "just have collapsed and expand", "the design and architecture needs to be simple yet elegant". |
| **Replaces** | Device's Chain view and its three card sizes: [Cards you play with](../cards2/design.md) §3.3, and [taste.md](../step8/taste.md)'s DV-11 to DV-16 and DV-18 to DV-19 |
| **Mockup** | [a picture](images/mockup.png): Keys, with Pluck and Lowpass collapsed and Reverb open. The clickable page it was taken from is kept outside the repo, on the maintainer's server (`$SCRATCH/graph-note/mockup.html`, [paths](../README.md#paths-in-these-notes)). |

**In short**
- **One view, the Graph.** Device's Chain · Patch switch goes, and the dock's Device tab is renamed Graph. Every box in it is a **node**: one call in the song's code.
- **A node has two sizes.**
  - It starts **collapsed**: its main picture, live, and up to six values as bars.
  - **⤢ opens it**: every group side by side, each with its own pictures, every value as a knob, its settings and its code line.
  - Which nodes are open belongs to the window, never to the song.
- **Five primitives:** node, port, wire, value, picture.
- **Six edits:** set a value, connect, disconnect, insert on a wire, remove, bypass. Each edit is one undo step and one change to the code.
- **The graph is the code, drawn.** A node is a call, an input is an argument, and a wire is an argument that reads another call. A value is an argument that is written out: a number, a choice or a switch.
- **+ on every wire** puts a node there, offering only what fits the wire's type. It replaces + Effect and + Note effect.
- **Wires glow with their sound** while the song plays, using meters music2 already has ([TAP-5](../../architecture.md#taps), [TAP-10](../../architecture.md#taps)).
- **The core gains nothing.** The window changes, and the editor's two add routes become one "wrap this input" route. No pack has to change: its face and groups already say what each size shows.

## 1. What is wrong today

- **Two views of one thing, drawn twice.**
  - Patch is the graph, with every value a row with its port: long, busy nodes.
  - Chain is cards in a row, offered only when the sound is a line (DV-11).
  - They are two implementations: `NodeGraph.tsx`'s card, and `Chain.tsx` with `frame.ts`. Between them come three sizes, tabs, a switch, and the rules for when Chain may show.
- **Chain hides the structure.** A modulator has no card there, a sidechain is a menu row, and a sound that isn't a line greys Chain out. The owner put it this way: "a chain doesn't capture it, because it's a one-to-one, whereas a patch is like a graph."
- **Patch's nodes show everything at once.** A Polysynth node is a column of about twenty rows, so a track's graph is mostly scrolling.
- **The flaws seen on 0.2.0's cards are a symptom of two layouts:** axis labels over pictures, a Polysynth card half empty, a label cut to "S".

## 2. The primitives

| Primitive | What it is in the code | How it is drawn |
|---|---|---|
| **Node** | A call: `reverb(pluck, { size: 0.6 })` | A box, collapsed or open. |
| **Port** | One of a call's inputs, or its result, with a type: `audio`, `notes`, `control<unit>` | A dot on the edge: inputs on the left, outputs on the right. ● audio, ◆ notes, ■ control. |
| **Wire** | An argument that reads another call | A curve from an output to an input. While the song plays, it glows with its level, and it has a **+** at its middle. |
| **Value** | An argument written out: a number, a choice, a switch | Collapsed: a bar showing its label, its value and how far along its range it is. Open: a knob, a menu or a switch. When a wire, a lane or code drives it, it says where its value comes from: "← LFO", "lane", "code". |
| **Picture** | The pack's view ([CARD-60](../../architecture.md#pictures-on-the-card)) | 46 px tall collapsed, 98 px open, with its live layers and dots as today. |

| Edit | Gesture | What the code does |
|---|---|---|
| Set a value | Drag a bar, a knob or a dot, or type it | The written-out value changes, as today. |
| Connect | Drag from an output onto an input or a value | The argument becomes a reference to that output: today's cables. |
| Disconnect | Drag the wire's end away | The argument goes back to its written-out value: today's cables. |
| Insert on a wire | **+** on the wire, then pick a sound | The argument is wrapped: `b(a)` becomes `b(x(a))`. New: one route, §9. |
| Remove | ⋯ ▸ Remove, or Delete | The call is unwrapped: `b(x(a))` becomes `b(a)`. Today's remove effect, widened to any node whose output has its first input's type. |
| Bypass | ⏻ in the header | `bypass: true` ([PKG-3](../../architecture.md#component-descriptors)) |

**The window's own state:** which nodes are open, and the graph's scroll and zoom. These are never saved and never sent.

**Why these primitives:** they are the program's own parts. Nothing in the window exists that isn't in the code, so a musician and an agent see the same thing, and every gesture means one line of TypeScript. A beginner learns how a sound is made by looking at it. A programmer reads the graph as the dataflow of their code, because that is what it is.

## 3. A node, collapsed (the default)

From top to bottom:

1. **Its header**, 28 px tall:
   - ⏻ to bypass it, its label, ⋯ (Remove, Show in code, Copy its id for `m2 set`), and ⤢ to open it;
   - an instrument has its track's colour along its top;
   - its pack and its descriptor's sentence ("Plays up to 12 notes at once.") are the label's tooltip, not a row.
2. **Its ports**, as rows:
   - one row for each sound or notes input, and one for each value a wire drives ("Gain ← LFO");
   - each row has its port on the left edge;
   - its output is on the right of the first row, so a line of nodes draws as straight wires, as Patch does today;
   - a node whose first row isn't sound or notes, such as an LFO or an oscillator, gets an output row of its own, first.
3. **Its face's first picture**, 46 px tall, live.
4. **Its face's first six values that no wire drives**, as bars:
   - two to a row when every label and value fits, otherwise one;
   - drag a bar sideways to change it, use the arrow keys, or double-click it to type a value;
   - a choice opens its menu, and a switch flips.
5. **"N more"**, which opens the node, when anything is left out.

It is 216 px wide, as Patch's node is today, and usually 150 to 200 px tall. The face is the descriptor's face ([CARD-41](../../architecture.md#the-cards-frame)), else its first group, else every value: DV-12's rule, kept.

## 4. A node, open

1. **Its header**: as collapsed, plus its pack and Presets.
2. **Its ports**: the same rows as collapsed. So opening a node never moves the end of a wire on it, only the node's size.
3. **Every group as a column**, 176 px wide, side by side, wrapping after four columns.
   - A column holds its pictures (98 px), its numbers as knobs (three to a row), its choices and switches as bars, and each wired value as a chip ("Gain ← LFO").
   - Face items that are in no group go first in the first column. Values in no group go in a last column, More. Settings go in Setup.
   - Each value and each picture is drawn once.
4. **Its code line**, as CARD-41 says, wrapped to the node's width.

An open node has no tabs, so it hides nothing. A dot whose `opens` names a group lights that group's column; that was DV-18's tab.

## 5. Wires

- **Types** show in the ports' colour and shape: audio is grey ●, notes are green ◆, control is amber ■ with a dashed wire. While you drag a wire, the ports it fits light up and the rest dim. That is PKG-10's units, as today.
- **+ at the middle of every sound and notes wire.**
  - It lists only what fits: effects on an audio wire, note effects on a notes wire, from ⌘F's library ([LIB-1](../../architecture.md#library)).
  - Picking one wraps that argument.
  - A control wire has no +.
- **Pulling a wire from a port into empty space** opens the same list for that port, such as an LFO for a cutoff. Track 3.
- **The glow.**
  - An audio wire's width and strength follow its level, about 30 times a second.
  - Each glowing wire uses one meter slot at its source (TAP-5, of 256: TAP-10), and only while the wire is on screen.
  - Notes wires don't flash yet: no tap counts notes today.
- **Another track is a node:** a pill such as "Kick · track", wired into Listens to or Cut by ([CARD-35](../../architecture.md#listening-to-another-track)). Its ▾ picks another track.

## 6. The layout

It doesn't change in kind. `layout(module, folded)` in `NodeGraph.tsx` is already a pure function of the graph and each node's size:
- columns by depth, with each source just before what reads it;
- a line drawn straight;
- loops drawn round below.

Opening a node only changes its size. The layout runs again, and the other nodes move aside over 280 ms. No position is saved. When a node opens, the graph scrolls it into view.

## 7. Words

| Today | New |
|---|---|
| Device (the dock's tab) | Graph |
| Patch, Chain | Graph |
| card | node |
| cable | wire |
| Fold, Normal, Expand | Collapse, Open (⤢) |

- The guide's pages change with them: `window.md`, `sounds.md`, `making-a-pack.md`.
- Rule ids keep their numbers, as gap numbers do: CARD-n and DV-n stay.

## 8. What goes, counted

- **In `web/`:**
  - `Chain.tsx` (559 lines);
  - most of `lib/frame.ts` (141), and `useCards` in `lib/folded.ts`;
  - the Chain · Patch switch, the tabs, the third size;
  - + Effect and + Note effect in the headers.
- **The estimate:** about −900 and +600 lines in `web/`. The new parts are the node at two sizes, the bar, the open node's columns, + on wires, and the glow.
- **In the contract:** DV-11 (when Chain shows), DV-13's three sizes, DV-15 and DV-16's tab rules, and DV-19's patch size become three rules:
  - one view;
  - a node's two sizes;
  - what a collapsed node stands for.
- **The browser suites:** about 15 select chain cards or sizes (`frame`, `card-views`, `pictures`, `comp`, `layout`, `taste`, `taps2` and others). Each moves to nodes, and none loses a check.

## 9. What the core gains

- **The kernel ABI, the IR, the runtime, the agents' routes and the file formats:** nothing.
- **The editor (`crates/editor`):** one route, insert, `{call, input, item}`, which wraps that argument in a new call.
  - Today's add effect (after a call) becomes one case of it, and notes-sdk's note effect (before a call, around its notes) the other.
  - `m2 add-effect` keeps its words.
- **The taps:** nothing. The glow uses meter slots (TAP-5).
- **The packs:** nothing. Faces and groups already say what each size shows.

## 10. Build order

Each track has a builder, a skeptic and the builder again, and each changes the contract first.

| # | Track | Builds | Size |
|---|---|---|---|
| 1 | `graph-node` | Nodes and the Graph tab. Contract first: taste.md's DV rules, CARD-25's folding, AGENT-6 (Show opens a node), and TAP-10's words ("Collapse a node"). Then the node at two sizes, values as bars and knobs, the Graph tab, Chain removed, and the suites moved. | large |
| 2 | `graph-wire` | + on wires and the insert route; Remove widened; the add buttons gone. | medium |
| 3 | `graph-live` | Wires glow, and pulling a wire into empty space. | small |

- Track 1 starts once the owner says yes.
- The events note's track 4 (`notes-chain`) folds into these, since a note effect is just a node on the notes wire.
- The events note's track 3 (the `@m2/notes` pack) is unchanged.

## 11. Decisions for the owner

| # | Question | Recommended |
|---|---|---|
| D1 | Chain goes. The dock shows one view, the Graph, and its tab is called Graph. All right? | **Yes.** It is the owner's own ask. |
| D2 | Two sizes: collapsed (the default) and open. All right? | **Yes.** It is the owner's own ask. |
| D3 | An open node pushes the others aside (the graph lays out again), rather than floating over them. All right? | **Yes.** The wires stay true, and nothing is hidden. |
| D4 | + on every wire replaces + Effect and + Note effect. All right? | **Yes.** It is one gesture, and it offers only what fits. |
| D5 | Wires glow with their level while the song plays. All right? | **Yes.** You see where the sound is, and where it stops. It adds nothing on the audio thread beyond the meters music2 has. |
| D6 | The words become Graph, node, input, output and wire. All right? | **Yes.** They are the program's own words. |

## 12. The mockup

[The picture](images/mockup.png) shows Keys: a Chords lane, then Pluck collapsed, Reverb open (Room and Mix side by side, with its code line), and Lowpass collapsed. The clickable page has four examples:
- **Keys:** a line of nodes;
- **Wobble:** an LFO wired into a VCA's Gain;
- **Sidechain:** a compressor wired to listen to the Kick track;
- **Arpeggiator:** a note effect on a notes wire.

In the page, ⤢ opens a node, values drag, + on a wire inserts a node, ⋯ ▸ Remove unwraps one, and Play makes the wires glow. The sound and the numbers in it are made up.
