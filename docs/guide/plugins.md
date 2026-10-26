# CLAP plugins

**In short**
- music2 plays installed CLAP plugins, instruments and effects, as boxes in the song's graph.
- A plugin runs in a second program: if it crashes, its sound goes quiet and comes back, and the song plays on.

music2 can play installed CLAP plugins, such as [Surge XT](https://surge-synthesizer.github.io) and [Airwindows Consolidated](https://www.airwindows.com), as boxes in the graph:

```ts
const synth = plugin("clap:org.surge-synth-team.surge-xt", { notes: lane.notes("notes"), key: "lead", a_filter_1_cutoff_4092842705: 0.6 });
```

- The app lists the installed plugins: the browser's **Plugins** groups (one under the instruments, one under the effects), the Device view's **+**, and **+ Track**. Drag an effect plugin onto a track, or double-click it, and it goes after the track's sound. Pick an instrument plugin in + Track, and you get a new track playing it. Each is one undo step, and writes the `plugin(…)` call with a `key`, so its settings are saved. A plugin installed while the app is open shows after **Look again**.
- `m2 plugins` lists installed plugins; `m2 plugins clap:<id>` shows one plugin's inputs and knobs.
- A parameter is written as its name, then its CLAP id: `a_filter_1_cutoff_4092842705`. Only the id says which parameter it is, so a plugin update that renames its parameters changes nothing (music2 notes the new name). Written without its id (`a_filter_1_cutoff`), it is an error that names the full address.
- A song whose plugin isn't installed still plays: that plugin's node is silent (an instrument) or passes its audio through (an effect), and the song says which plugin is missing.
- `m2 plugin-state <song> <track>/<key> clap:<id>` saves the plugin's default settings (not the values your code gives it) to `data/<track>/<key>.state`, which the plugin keyed `<key>` on that track loads. Beside it, `<key>.state.json` says which plugin wrote it; a state without it, or written by another plugin, isn't loaded.
- Plugins run in a second program, beside the app. If a plugin crashes or hangs, its sound goes quiet, the app says so once, and it comes back by itself; the song plays on. A plugin stuck for 30 seconds is ended the same way.
- **Open window** on a plugin's box in Device opens the plugin's own window. A knob turned there writes its number into the code (one undo step); a knob a lane drives stays where you put it ("Latched"; **Re-enable** gives it back to the lane); any other change saves the plugin's settings a second later (one undo step).
- A plugin that plays late (a look-ahead limiter, say) is made up for: every other track waits for it, live and in exports.
- Plugins can't go inside `poly` (an instrument plugin plays all the notes itself).
- See [`examples/with-plugins`](https://github.com/BreadBowlAI/music2/tree/main/examples/with-plugins), which needs both plugins installed.
