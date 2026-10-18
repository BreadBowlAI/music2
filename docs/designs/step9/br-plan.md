# Track br: add an installed plugin, and presets

Gaps #32 and #26. Branch `br`, from `main` at `e7518f4`. Worktree `~/m2-runs/wt/br`.

## In short

- Three slices, in this order. Each test is shown red first, then green.

  | # | Slice | What you get |
  |---|---|---|
  | 1 | Plugins in the browser and + Track (#32) | Installed CLAP plugins are listed. Drag one onto a track, or double-click it, and it goes after the sound. Pick a plugin instrument in + Track and you get a new track. One Undo each |
  | 2 | Presets for std sounds (#26, first half) | Each instrument and effect card has **Presets**. Save the knobs under a name. Load one back: one Undo step. A few presets ship with std |
  | 3 | Presets for plugins (#26, second half) | The same **Presets** list for a plugin: your saved ones, and the plugin's own (when it offers them through CLAP) |

- **The app never loads plugin code.** The list comes from the plugin host process's scan (`PluginHost::scan`, HOST-3).
- **Every add and every load is one Tx built in `crates/editor`**, as std effects and instruments are added today.
- **One preset format, one folder.** A preset is one JSON file. Yours are in `~/Music/music2/Presets`. std's are in `std/presets`.
- **No sound changes.** `e2e/renders.sha256` must still match. No format bump, no SDK bump, no new crate (`clap-sys` 0.5 already has CLAP's preset-discovery and preset-load).

## Words used here

| Word | Meaning |
|---|---|
| scan | the host process loads every `.clap` bundle and says what plugins it found (`Found`: id, name, vendor, version, features) |
| preset | a sound's settings under a name: a std sound's knob values, or a plugin's state |
| factory preset | one that ships with std, or one a plugin offers itself. You can load it, not change it |
| user preset | one you saved. It lives in your Presets folder |
| scratch instance | a plugin made for a moment in the host process, to turn a plugin's own preset into a state, then thrown away |

## Slice 1: plugins in the browser and + Track (#32)

**Goal.** The side browser and + Track list the installed plugins. Adding one writes it into the track's code in one undoable Tx.

**What the code becomes.**

| Gesture | Code before | Code after |
|---|---|---|
| Drop an effect plugin on Kick | `kick(lane.notes("hits"))` | `plugin("clap:m2.probe", { in: kick(lane.notes("hits")), key: "m2-probe" })`, and `plugin` imported from `@m2/core` |
| + Track ▸ an instrument plugin | (no track) | `tracks/surge-xt.ts`: `track("surge-xt", { name: "Surge XT" }, ({ lane }) => plugin("clap:org.surge-synth-team.surge-xt", { notes: lane.notes("notes"), key: "surge-xt" }))`, its mixer channel and its colour |

The `key` is always written, so the plugin's settings can be saved (HOME-6 needs a state path). An effect's key is the plugin's name as a key, numbered when its frame has it (`m2-probe-2`). A new track's plugin takes the track's key, as `examples/with-plugins` does.

**Steps and files.**

| Step | What | Files |
|---|---|---|
| 1a | The plugin list: scanned once per app run, off the async threads, on the first ask; kept until **Look again**. Each plugin is an instrument (`instrument` in its features) or an effect (`audio-effect`), or both | `crates/server/src/routes_plugins.rs` (new): `GET /api/plugins` → `{plugins: [{id: "clap:…", name, vendor, version, instrument, effect}], errors}`, `POST /api/plugins/scan`; the cache in `AppState` (`crates/server/src/lib.rs`) |
| 1b | Adding an effect plugin. `add_effect` sees a `clap:` id and wraps the call in `plugin(…)`: two insertions around it (R5, WRITE-8), the import, one `Confined` Tx. The planner's check: exactly one new plugin node reads what the call made, and the readers now read it. Refused in words when the plugin is missing, has no audio input, or is an instrument only (its features). *As built:* the plugin is described through the host before planning, so a refusal writes nothing; the planner also checks the candidate has no new `plugin.missing`. The wrap is shared with std's effects (`effect::wrap_call`) | `crates/editor/src/plugins.rs` (new), `effect.rs` (the dispatch) |
| 1c | Adding a plugin track. `Play::Plugin(id)` beside `Play::Instrument`; `tracks::add` takes a `clap:` id. Refused in words when the plugin is missing, or takes no notes | `crates/editor/src/tracks.rs`, `plugins.rs` |
| 1d | Routes: no new write route. `POST /api/graph/effect/add` and `/api/graph/track/add` take `clap:<id>` as their `component` | `crates/server/src/routes.rs` (docs and errors only) |
| 1e | The app: the side browser gets a **Plugins** group under the effects, and one under the instruments; + Track gets a **Plugins** group last. Each opens with "Looking for plugins…" and has **Look again**. Drag, double-click and Enter work as for std. With none installed: "No plugins are installed. music2 looks in ~/Library/Audio/Plug-Ins/CLAP." (the folder from the server, so Linux says its own). *As built:* the side browser's Plugins groups stay folded until opened, since opening one is what scans; the Device view's + lists effect plugins last too | `web/src/components/Browser.tsx`, `EffectBrowser.tsx`, `Tracks.tsx`, `SongTimeline.tsx` (the drop), `PluginsFoot.tsx` (new: a group's foot), `web/src/lib/plugins.ts` (new) |
| 1f | The contract: HOST-3's "Finding plugins" row says the app's lists come from the scan. gaps.md: #32 done; its leftovers listed | `docs/architecture.md`, `docs/gaps.md`, `README.md` |

**Words the app says.**

| When | Words |
|---|---|
| Added | "Added m2 probe after Kick" with **Undo** |
| Not installed (removed since the scan) | "m2.probe isn't installed on this computer, so it can't be added. Install it, then choose Look again." *As built:* by its id, since the editor can't learn a missing plugin's name (a gap) |
| An instrument dropped as an effect | "Surge XT plays notes; it can't go after a sound. Add it with + Track." |

**Tests, red first.**

| Test | Where | Red by |
|---|---|---|
| `the_plugin_list_comes_from_the_host_scan` (the probe, as both kinds, through `Remote`; then no `.clap` mapped in the test process) | `crates/server/tests/plugin_list.rs` (*as built in slice 3:* a process of its own, as the unit tests' in-process plugins map `.clap` files) | the route missing (404) |
| `adding_a_plugin_effect_wraps_the_call_in_one_tx` (the text above; one Undo gives the old bytes back) | `crates/editor/src/plugins_tests.rs` | `add_effect` refusing `clap:` ids, as today |
| `a_second_plugin_in_one_frame_gets_a_free_key` | same | the key not numbered (`key.duplicate`) |
| `a_plugin_instrument_track_is_one_tx` (file, channel, colour; one Undo takes all away) | `crates/editor/src/tracks_tests.rs` | `tracks::add` refusing `clap:` ids |
| `a_plugin_that_isnt_installed_is_refused_in_words` (nothing written) | `plugins_tests.rs` | the check left out: the Tx commits with `plugin.missing` |
| `planned_call_forms_are_current` gains the two new forms | existing test | — |
| `e2e/br-plugins.mjs`: the Plugins groups list m2 probe; a drag onto Kick adds it, the card shows, Undo takes it away; + Track adds a probe track | `e2e/` (with `M2_PROBE=1`) | no Plugins group |

**Done when** the tests above pass, clippy and the Mac lint are clean, the renders match, the suites `br-plugins`, `effects`, `s8-tracks` and `graph` pass, and the app on Xvfb `:95` shows the Plugins group (a picture in `~/m2-runs/app-shots/br-1/`).

## Slice 2: presets for std sounds (#26, first half)

**Goal.** Save an instrument's or an effect's knobs by name, and load them back as one undoable Tx. Build the one preset format, its folders and the card's preset list.

**The format (FMT-22, new).** *As built:* FMT-22, since FMT-21 is taken (Conversion). One JSON file, `<name>.json`, two-space, keys in this order:

| Key | Holds |
|---|---|
| `m2preset` | `1`, the format's version. A newer one is refused in words |
| `name` | its name: one line, 1 to 40 characters, no slash, backslash, colon, `*`, `?`, `"`, `<`, `>` or bar, not starting with `.` |
| `sound` | what it is for: a ComponentId (`std:synth-pluck`) or a plugin id (`clap:org.surge-synth-team.surge-xt`). This is its identity, never the folder (P1) |
| `values` | numbers by name: a std control's slot in base units (`{"cutoff": 1200}`); a plugin's parameter by its CLAP id (`{"4092842705": 0.6}`, FMT-18) |
| `plugin_version`, `state` | a plugin only (slice 3): its version, and its state as base64 |

**The folders.**

| Kind | Where |
|---|---|
| Yours | `m2_os::dirs::presets()`: `~/Music/music2/Presets/<package>/<name in the id>/` on every OS, under `$HOME` (PLAT-7). It doesn't follow the "new songs go" setting. Windows: the profile's Music folder, designed |
| std's | `std/presets/<component>/` (`std/presets/synth-pluck/Soft.json`), found by `resources::std_dir()` |
| A plugin's own | from the plugin (slice 3), never a file music2 writes |

A folder name is the id with every character outside `A–Z a–z 0–9 . _ -` turned to `_`. A file whose `sound` isn't the call's is left out of its list.

**Steps and files.**

| Step | What | Files |
|---|---|---|
| 2a | `dirs::presets()` | `core/os/src/dirs.rs` |
| 2b | The format: read, write (to a temporary file, then a rename), list a sound's presets (std's, then yours, each by name), the name rules. A name taken in other capitals is taken, as for songs. *As built:* a name also has no space at either end and no dot at the end (a file name on every OS) | `crates/editor/src/presets.rs` (new) |
| 2c | Save (std): every declared control of the call, each by its slot, from `controls()`. Not a Tx: it writes outside the song | `presets.rs` |
| 2d | Load (std): one `Values` Tx with an anchor for each control. *As built:* the joined insertion is `lens::together`; each `Control` knows the calls whose slots it feeds (`Control::calls`), so a value other calls share is kept and named, as is one written in other units; a value the call plays already is not written, and a load that changes nothing makes no Undo step. A written number is rewritten through its lens, keeping its spelling (`khz(0.9)`); a control the call leaves out is written in (PKG-3's inherited slot, as `value::insert`). Several left-out controls go into the call's options as one insertion. A control a lane or a cable drives is left alone and named. A preset for another sound is refused | `crates/editor/src/presets.rs`, `value.rs` (share the lens and insertion parts) |
| 2e | *As built:* the Graph side is `crates/server/src/graph/presets.rs` (new now, not in slice 3), files read and written off the actor's thread; the list takes `revision` too; save answers `{taken}` (not an error) when the name is there, so the app asks. Routes: `GET /api/graph/presets?session&node` → `{std: [...], yours: [...], plugin: [...]}`; `POST /api/graph/preset/save {session, node, name, replace}`; `POST /api/graph/preset/load {session, at, node, from: "std"\|"yours", name}` (a write route: the lock applies) | `crates/server/src/routes_presets.rs` (new), `routes.rs` (the write list) |
| 2f | The card: a **Presets** button in the header of every instrument and effect card. A popover: a search field; groups "Yours" and "music2"; a row loads on click; **Save as preset…** asks for a name ("A preset called Soft is there. Replace it?"). *As built:* an instrument is a component's call that takes notes; an effect is one in the effects list; a plugin's card and the Song's mixer have none yet. The name field takes the list's place while it is open, so the popover stays on screen. The popover is its own file | `web/src/components/NodeGraph.tsx` (the button), `web/src/components/PresetPopover.tsx` (new), `web/src/lib/presets.ts` (new) |
| 2g | Factory presets: two each for Pluck, Pad, Bass, Reverb and Delay (10 files). The rest go to gaps.md. *As built:* std/synth's Synth pluck, Synth pad and Synth bass (the ones + Track adds). The desktop bundle carries `std/presets` (`tauri.conf.json`) | `std/presets/` |
| 2h | The contract: FMT-22 (the format, the folders, load is one Tx, save writes no song file), PLAT-7 names the Presets folder, the Homes table's "the code is the preset" says how a preset gets there. gaps.md: #26 partly | `docs/architecture.md`, `docs/gaps.md`, `README.md` |

**Words the app says.**

| When | Words |
|---|---|
| Loaded | "Loaded Soft into Pluck" with **Undo** (*as built:* the card's title, "Loaded Soft into Synth pluck") |
| Nothing to change | *As built:* "Synth pluck has Soft's settings already." (no Undo) |
| A knob kept | "Loaded Soft into Pluck. Cutoff follows its lane, so it kept the lane." |
| Saved | "Saved Pluck's settings as My Pluck" |
| Another sound's | "This preset is for Pad, not Pluck." |

**Tests, red first.**

| Test | Where | Red by |
|---|---|---|
| `what_music2_writes_is_under_home` gains `presets()` | `core/os/src/dirs.rs` | `presets()` outside `$HOME` (a stub) |
| `a_preset_file_round_trips`, `a_preset_name_is_one_line_of_1_to_40` | `crates/editor/src/presets_tests.rs` | the reader and the name check stubbed |
| `saving_a_std_preset_writes_every_control_of_the_call` | same | save writes only the written numbers |
| `loading_a_std_preset_is_one_values_tx` (a written number rewritten in its units, two left-out ones written in; one Undo gives the bytes back; the render equals the same values written by hand) | same | load as one Tx per control (two Undo steps) |
| `a_control_a_lane_drives_keeps_its_lane_and_is_named` | same | the lane's call overwritten |
| `a_preset_for_another_sound_is_refused` | same | the `sound` check left out |
| `every_factory_preset_loads` (each file parses, names a std sound, each value a declared slot; each loads into a song that calls it) | same | a factory file with a wrong slot |
| `e2e/br-presets.mjs` (std part): Presets on Pluck's card; load Soft, the knob moves, Undo puts it back; save My Pluck, it is listed under Yours | `e2e/` | no Presets button |

**Done when** as slice 1, with the suites `br-presets`, `s5-knobs`, `nx-undo` and `taste` (its lint of words), and a picture of the popover on Xvfb `:95`.

## Slice 3: presets for plugins (#26, second half)

**Goal.** Save and load a plugin's settings as presets, in the same list and format. List and load the plugin's own presets, where it offers them through CLAP.

**How it works.**

| Act | What happens |
|---|---|
| Save | The session asks the host for `save_state` (as autosave does, HOME-6), takes the values of the parameters the code writes as numbers, and writes one preset file: `state` (base64), `plugin_version`, `values` |
| Load yours | One `Confined` Tx: the state blob and its sidecar (FMT-17, as `plugin_state` writes them), and each parameter the code writes as a number rewritten to the preset's value. Then the usual state load reaches the live instance (RENDER-26b). A parameter a lane drives keeps its lane, and is named |
| The plugin's own list | A new trait method, `PluginHost::presets(&Found)`: in the host process, CLAP's preset-discovery factory: each provider's plugin-internal locations, and its file locations (folders walked for its file types, 2,000 files at most). Kept per plugin for the app run |
| Load the plugin's own | A scratch instance in the host process: `create`, `MainCall::LoadPreset { kind, location, key }` (CLAP's `preset-load`), `save_state`, read the code-owned parameters' values, `destroy`. Then the same one Tx as "Load yours". The live instance is never touched before the Tx commits |

**Steps and files.**

| Step | What | Files |
|---|---|---|
| 3a | `presets` on the trait; `MainCall::LoadPreset`, `Reply::Loaded`; in-process (discovery and preset-load on the plugin main thread, HOST-2) and through the host process (`Req::Presets`, the wire) | `core/plugins/src/host.rs`, `inprocess.rs`, `remote/mod.rs`, `remote/wire.rs`, `remote/child.rs` |
| 3b | The probe offers a discovery factory with two presets, "Bright" and "Dark" (plugin-internal), and `preset-load`, which sets its parameters and tag | `core/plugins/src/probe.rs` |
| 3c | Save and load for plugins; refused in words with no key ("Give this plugin a key in its code to save its settings"), when missing, or for another plugin | `crates/editor/src/presets.rs`, `crates/server/src/graph/presets.rs` (new: the host calls, off the actor's thread), `routes_presets.rs` (`from: "plugin"`) |
| 3d | The card: the same popover on a plugin's card, with a third group named for the plugin ("Surge XT's") | `NodeGraph.tsx`, `lib/presets.ts` |
| 3e | The contract: HOST-1's trait sketch, a new HOST-8 (plugin presets reach the plugin only through `PluginHost`; the plugin's own are loaded on a scratch instance in the host process, never in the app). gaps.md: #26 done; its leftovers | `docs/architecture.md`, `docs/gaps.md`, `README.md` |

**Tests, red first.**

| Test | Where | Red by |
|---|---|---|
| `the_probe_offers_its_presets_on_both_hosts` | `core/plugins/src/tests.rs`, `core/plugins/tests/remote.rs` | `presets` returning nothing |
| `a_plugin_preset_loads_on_a_scratch_instance` (the saved blob holds Dark's tag; afterwards as many instances as before) | same | `LoadPreset` ignored |
| `saving_a_plugin_preset_keeps_its_state_and_code_values` | `crates/server/src/graph/presets_tests.rs` | `state` left out |
| `loading_a_plugin_preset_is_one_undo_step` (blob, sidecar and a literal; the live probe loads it; no autosave Tx after, with the probe's `dirty-at`; one Undo loads the old blob back) | same | blob and literal as two Tx |
| `a_preset_for_another_plugin_is_refused_in_words` | same | the `sound` check left out |
| `e2e/br-presets.mjs` (plugin part): the probe's card lists "m2 probe's" Bright and Dark; Dark loads, with Undo | `e2e/` | no plugin group |

**Done when** as slice 2, and `m2-gates plugins` passes, and `ci/check.sh` passes once at the track's end (through `~/m2-runs/tw`). Surge XT on Xvfb `:95` is tried by hand: its own presets listed if it offers discovery, a saved preset loaded back (information only, a picture).

### Slice 3 as built

| Change from the plan | Why |
|---|---|
| The host's types are `Offered { name, location, key }` and `PresetAt::Plugin` or `PresetAt::File(path)`; `MainCall::LoadPreset { location, key }` | CLAP's location kind and path are one value: a file location always has a path |
| The host tests are both in `core/plugins/tests/remote.rs` | That file runs each test on both hosts |
| A plugin's card shows Yours, then the plugin's own ("m2 probe's"), and no music2 group | std has no presets for plugins. The plugin's group shows only when it offers some, or says why it couldn't be read |
| A parameter the code doesn't write isn't written in by a plugin load | The state holds it (HOME-4) |
| A sidecar written by a load names the preset's `plugin_version` | The sidecar says which plugin wrote the blob (FMT-17) |
| No "dirty within three calls after a load is ignored" | Not needed: the probe's save after a load equals the file, so autosave writes nothing (`loading_a_plugin_preset_is_one_undo_step`). Listed in the contract as the condition |
| The plugin card is found by the call's one plugin node, whatever frames are around it | A helper with two plugins in one call is refused ("isn't a sound whose settings can be saved") |
| `data::plugin_state`'s writes are shared (`data::state_writes`) | The load writes the same two files |
| A plugin's own presets are searched on the calling thread, not the main thread (set by the review) | CLAP's discovery factory is thread-safe; a long search on the main thread froze plugin windows and held up autosave. Loading one stays on the main thread |
| A new server test, `the_plugins_own_preset_loads_on_a_scratch_instance` | Covers the list, the scratch instance (made, then destroyed, by the probe's records) and the Tx |

Surge XT on Xvfb `:95`, by hand: it offers 1,974 presets through discovery, listed in about 0.5 s; one of them (Attacky) loads in about 0.1 s, and its saved state names it; a preset of yours (67 KB) saves and loads back. Picture and logs: `~/m2-runs/app-shots/br-3/`.

## Decisions

| What | Chosen | Why |
|---|---|---|
| Where the plugin list lives | Per app run, in the server, not in each revision's graph state | A scan loads every bundle (up to 2 minutes). A song with no plugins still starts no host process until a list is opened |
| How a plugin travels in the add routes | `clap:<id>` as the `component` of the existing routes | It is what the code writes. No second route for the same gesture |
| A plugin call always gets a `key` | Yes | Without one its settings can't be saved (HOME-6), and presets can't load |
| An instrument plugin dropped on a track | Refused in words, pointing at + Track | Changing a track's instrument to a plugin is #1's leftover |
| One file per preset, the state as base64 | Yes | One file to copy or share. Most states are small (Surge XT's is 52 KB) |
| User presets in `~/Music/music2/Presets` | Yes, fixed | Easy to find and back up; under `$HOME` (PLAT-7); not hidden like `~/.music2` |
| A preset's identity | Its `sound` and its name; the folder is a hint | P1: a path is never identity |
| A std preset holds every control | Yes; numbers only | Load gives the sound as saved. Settings that aren't numbers (`voices`, `mode`) wait for #35 |
| Controls a preset doesn't name | Keep their value | A preset from an older std still loads |
| A plugin's own preset | Loaded on a scratch instance, then one Tx | The live sound changes only when the Tx commits, so Undo and a refused Tx stay exact |
| The card shows which preset is loaded | No | Nothing on disk says it (P2). A gap |
| std's version | Not bumped | Presets are files beside std; no component, port or sound changes |
| Delete and rename your presets | Not now | A gap; the Presets folder can be tidied by hand |

## Risks

| Risk | What we do |
|---|---|
| Several left-out controls written into one call: two insertions may collide | One insertion writes them all; `loading_a_std_preset_is_one_values_tx` covers a call with no options at all |
| The effect wrap's planner check assumes a component's frames; a plugin call has none | A plugin-specific check: one new node, made at the new call's site, reading the old output |
| A plugin marks itself dirty after a load, so autosave makes a second Undo step | The test catches it; then a dirty within the three calls after a load is ignored, as HOME-5a's set-back already is |
| Preset discovery is slow, or crashes the host process | It runs off the actor's thread with `remote::LIMIT`; a crash is HOST-3's `plugin.crashed` path; the list says "Couldn't read Surge XT's presets" |
| Many plugins don't offer CLAP preset discovery | Their group isn't shown; saving and loading your own still works |
| A large state (up to 50 MB) as base64 | Read and written off the actor's thread; noted in FMT-22. *As built (slice 2):* a list reads every file of its folder, so slice 3 should list without decoding states |
| Other tracks change `NodeGraph.tsx`, `tracks.rs` or `routes.rs` at the same time | Small, separate edits; new code in new files; merge, never rebase |
| Disk: this worktree's `target/` | Built in place, cleaned at the track's end (scratch-build-disk note) |

## What goes to gaps.md instead

One line each, under #32 or #26, or in Loose ends:

- Remove, turn off and move a plugin effect from its card (code only; Undo works right after adding).
- Change a track's instrument to a plugin (#1).
- A plugin installed while the app runs shows only after **Look again**.
- A plugin call with no `key` can't save or load presets; offer to add one.
- Rename and delete your presets, and Show in Finder.
- The card doesn't say which preset a sound came from, or that it changed since.
- Presets for whole tracks, chains and the mixer; settings that aren't numbers (#35).
- Factory presets for every std sound (10 ship now).
- A plugin's own preset tags, favourites and collections (discovery's metadata beyond the name).
- `m2 preset` commands (save, load, list) for agents (#37).
