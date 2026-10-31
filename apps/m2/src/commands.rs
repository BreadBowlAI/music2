//! Every `m2` command, declared once (AGENT-12): `m2`'s usage and `m2 docs commands` (the guide's
//! `docs/guide/commands.md`) are both made from [`COMMANDS`]. A command `m2` doesn't know is
//! refused, exit status 2; so is a word a command doesn't take, since each `usage` is also the
//! grammar its words are read against (AGENT-13, `words.rs`).
//!
//! Each entry's `words` is what `m2` prints; `more` is what the guide adds. Both are Markdown: the
//! usage strips the backticks.

/// One command.
pub struct Command {
    /// The word after `m2`.
    pub name: &'static str,
    /// Other words that run it (`build` for `check`).
    pub also: &'static [&'static str],
    /// How it is called: one form a line, each starting `m2 `, and the grammar its words are read
    /// against (AGENT-13): its place words, then its flags, `[…]` around what can be left out.
    pub usage: &'static str,
    /// What it does, in a few lines.
    pub words: &'static str,
    /// What the guide's page adds to the words. Can be empty.
    pub more: &'static str,
}

/// The first line of the usage.
pub const TITLE: &str = "m2: music2's songs from the terminal: build, check, render, listen and edit them";

/// After the table, in the usage and on the guide's page.
pub const NOTES: &str = "\
Every command that reads a project prints what needs attention in it, such as data no lane reads, \
with the commands that fix it.

One program writes a project at a time (its lock is `.music2/lock`). While the app has the song open, \
a command that writes it hands its edit to the app, which makes it one undo step, named by \
`--as <name>` (or `M2_AS`) and `--why <words>`, which go last, after the command's own arguments: the \
musician sees it there, and `m2 undo` takes it back. While the musician drags what it changes, \
records, or has unsaved code in its file, the app refuses it: `m2` waits up to 30 s, then exits 3, \
changing nothing. Don't write the song's files directly while the app has it open: the app can't \
refuse that, and it ends a drag on what it changes. With the app closed, `m2` writes the files \
itself, and undo isn't kept once it exits. Reading, rendering and `bounce` without `--place` work all \
the same. What is the musician's own (play, stop, seek, the loop, record, mute and solo while \
listening, the view, plugin windows, redo, exports, settings, opening and closing songs) no agent can \
do: `m2 call` refuses it, and `m2 point` shows them where instead.

std, the templates and the examples are found at `$M2_STD`, `$M2_TEMPLATES` and `$M2_EXAMPLES`, \
in the app or package `m2` came in (`music2.app/Contents/Resources`, or `lib/music2` beside the \
`bin` that holds `m2` on Linux, as in `/usr/lib/music2`), or else in the source tree it was built \
from.

`m2` alone, `m2 help` or `m2 --help` prints this, and `m2 --version` (or `-V`) which `m2` this is. \
`m2 <command> --help` prints that command's part. A command `m2` doesn't know is refused, with exit status 2. So is a word a command \
doesn't take, before anything is read or written, and `m2` says which: a flag it hasn't got, a flag without its value, \
a word too many or too few. A word that starts with `-` is a flag, unless it is a number (`-6`), has a space in it, \
or is `-` alone: standard input, where the usage says `| -`. A flag's value is the next word, unless that is a flag; \
a `<text>` is the next word whatever it is. Flags can go anywhere after the command, but `--as` and `--why` go last. \
`m2 docs` prints the guide, built into this `m2`.";

/// The guide's page only: commands to try, on a copy of an example.
pub const EXAMPLES: &str = "\
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
```";

pub const COMMANDS: &[Command] = &[
    Command {
        name: "new",
        also: &[],
        usage: "m2 new <folder> [--template ID | --example ID]",
        words: "make a new song in exactly `<folder>` from a template (`starter`, the default, or `empty`) or a copy \
                of an example (`--example first-song`). A folder that is there, in any capitals, is refused",
        more: "The song is named after its folder. It gets `AGENTS.md`, for agents working on it, which names this \
               `m2` by its full path, and `CLAUDE.md`, which imports it for Claude Code. Try the commands below on \
               such a copy, never on the examples themselves.",
    },
    Command {
        name: "check",
        also: &["build"],
        usage: "m2 check <project> [--json]",
        words: "run the TypeScript, check the graph and the data, and print a summary; `--json` prints the compiled \
                graph instead. `m2 build` is the same",
        more: "A problem is printed as the file, line and column, then what is wrong; one in the sound graph also \
               names the module's key and the node in square brackets ([How a song is written](code.md#when-somethings-wrong)).",
    },
    Command {
        name: "render",
        also: &[],
        usage: "m2 render <project> <out.wav> [--from S | --from-bar N] [--seconds N] [--preroll P] [--threads N]",
        words: "render to a 24-bit WAV, as the app's Export Audio does: to the last clip's end and on until it falls \
                silent (10 s more at most), at the project's `sample_rate`. `--from` starts S seconds in, or \
                `--from-bar` at bar N (counted from 1), replaying the song up to there; `--seconds` renders exactly \
                N seconds. `--preroll` starts only P seconds earlier instead of replaying: fast for listening late in \
                a long song, but approximate (notes held from before then, and the tails of what played earlier, are \
                missing), so it is never written into `assets/`. `--threads` renders on N threads (1 to 64, or \
                `M2_THREADS`), the same file on any number",
        more: "It is the same file the app's Export (Song) writes, which falls silent when the sound has been silent \
               for half a second. 3 hours at most. If a part breaks while it renders, it fails and names it.",
    },
    Command {
        name: "listen",
        also: &[],
        usage: "m2 listen <project> [--bars 9-16] [--tracks kick,bass] [--as-heard] [--with <tx.json | ->] [--png out.png] [--json]",
        words: "measure the song as an agent can't hear it: loudness (LUFS), true peak, crest, ten octave bands, \
                stereo and masking, for the mix and each track, its own sound and as it reaches the mix. The bars \
                count from 1 (else the loop when one is on, else the song), with 2 bars of preroll; `--tracks` \
                measures those alone; `--as-heard` with the musician's listening mute and solo; `--with` a Tx (from a file, or \
                standard input) checked and never written, before and after; `--png` draws the bands over time. Its own process, at \
                background priority, one at a time; it writes nothing into the song",
        more: "It works with the app open or closed. [Listening](listening.md) says what each number means.",
    },
    Command { name: "kernels", also: &[], usage: "m2 kernels [--json]", words: "list every kernel and its typed ports", more: "" },
    Command {
        name: "bounce",
        also: &[],
        usage: "m2 bounce <project> <asset-name> [--track KEY] [--export out] [--from S] [--seconds N] [--place track.lane@beat] [--threads N]",
        words: "render a track, named by its key (or the whole song), to a new `assets/<asset-name>.wav` (never over \
                one that exists), optionally placing it as a clip on an audio lane (both by key)",
        more: "`--export` names which of the track's outputs to render (`out` unless you say). `--place keys.lane@beat` \
               puts the new file on an audio lane, as one edit.",
    },
    Command {
        name: "graph",
        also: &[],
        usage: "m2 graph <project> [--json]",
        words: "the graph the canvas shows: calls, their knobs, connections",
        more: "",
    },
    Command {
        name: "controls",
        also: &[],
        usage: "m2 controls <project>",
        words: "every number written in the project that the graph can edit, with its id, and every declared control \
                a call leaves out (inherited: its default)",
        more: "An id names a file, a call in it and an argument: `song.ts#2/0.hats.pan` is, in `song.ts`, call number \
               2 (the mixer), its first argument (`0`), and `hats.pan` inside that. A control the call leaves out is \
               named by its call and `@` with the control's name: `tracks/hats.ts#1/@decay`.",
    },
    Command {
        name: "set",
        also: &[],
        usage: "m2 set <project> <control> <value> [--expect V]",
        words: "change one, verified by the compiler (values in the control's units); `--expect` refuses unless it \
                is V now",
        more: "",
    },
    Command {
        name: "automate",
        also: &[],
        usage: "m2 automate <project> <control>",
        words: "make a control's value the fallback of a new, empty control lane, read where the value was, verified \
                to sound the same",
        more: "It is what the app's Automate does: the knob gets an automation lane of its own.",
    },
    Command {
        name: "label",
        also: &[],
        usage: "m2 label <project> <track> <name>",
        words: "show a track, named by its key, as `<name>`: its `track()` call's name, verified to change nothing \
                that plays (its key as `<name>` takes the name out)",
        more: "",
    },
    Command {
        name: "add-track",
        also: &[],
        usage: "m2 add-track <project> <instrument> [--name N] [--key ID] [--after ID]",
        words: "add a track playing an instrument (`std:synth-pluck`, or `synth.pluck`): its file in `tracks/`, its \
                import and mixer channel in `song.ts`, an empty notes lane, and its place and colour in \
                `layout.json`, verified",
        more: "It is what + Track does. `--name \"Lead Vox\"` names it, `--key` gives its key, `--after kick` puts it \
               below that track.",
    },
    Command {
        name: "import",
        also: &[],
        usage: "m2 import <project> <file.wav|file.flac>… [--track ID] [--at BAR]",
        words: "bring WAV or FLAC files into `assets/` (a copy, or the same bytes already there; a taken name numbered; \
                a FLAC as the WAV of its samples, with a note of what it was made from) and \
                place their clips from bar BAR (1 if not given): on a track's audio lane one after another, or \
                without `--track` each on a new audio track named after it, at the end; one edit that undo takes back",
        more: "With the song open in the app, the app brings them in, as one undo step, and nothing is copied while \
               the musician holds the song (a take, V's Compare): it waits, as every write does.",
    },
    Command {
        name: "add-sample",
        also: &[],
        usage: "m2 add-sample <project> <sample id> [--track ID] [--at BAR]",
        words: "add a sample from the library (its id, from `m2 search --kind sample`): its file copied into \
                `assets/` with a note beside it of where it came from (a pack's name, version, author and \
                licence; or that it is from your own folder), and its clip placed from bar BAR (1 if not given), \
                on a track's audio lane or on a new audio track; a loop at another tempo follows the song's. One \
                edit that undo takes back",
        more: "With the song open in the app, the app adds it, as one undo step. The note is `assets/<file>.json`; \
               the file and its note stay when the clip is undone. [Samples](samples.md) says more.",
    },
    Command {
        name: "duplicate-track",
        also: &[],
        usage: "m2 duplicate-track <project> <track>",
        words: "copy a track, named by its key: its code, its clips (with new ids), its plugin states and its mixer \
                channel, verified",
        more: "",
    },
    Command {
        name: "delete-track",
        also: &[],
        usage: "m2 delete-track <project> <track> [--cascade]",
        words: "take a track out: its file, its imports, its channel, its data and its layout; `--cascade` also takes \
                it out of effects that read it (a duck's sidechain), which are otherwise listed and nothing is changed",
        more: "",
    },
    Command {
        name: "layout",
        also: &[],
        usage: "m2 layout <project> order <IDs…>\nm2 layout <project> colour|color <ID> <colour>\nm2 layout <project> reset",
        words: "reorder or recolour tracks in `layout.json` (no revision), or write the default layout over one that \
                doesn't read; the colours are rose, teal, amber, indigo, green, plum, olive, blue, coral, cyan, violet \
                and lime",
        more: "No revision: the layout never changes the sound.",
    },
    Command {
        name: "tempo",
        also: &[],
        usage: "m2 tempo <project> <bpm> [--at <bar>]",
        words: "set the song's tempo (20 to 999 BPM) in `data/arrangement.json`, or with `--at` a tempo change from \
                bar N (counted from 1), verified",
        more: "",
    },
    Command {
        name: "meter",
        also: &[],
        usage: "m2 meter <project> <n/d> [--at <bar>]",
        words: "set the song's time signature (`7/8`: 1 to 64 beats of a 1, 2, 4, 8, 16 or 32 note), or with `--at` a \
                change from bar N, verified",
        more: "",
    },
    Command {
        name: "title",
        also: &[],
        usage: "m2 title <project> <name>",
        words: "call the song `<name>`: its name in `music2.toml` (the folder keeps its own)",
        more: "",
    },
    Command {
        name: "cables",
        also: &[],
        usage: "m2 cables <project>",
        words: "named signals a cable can start from, and the inputs cables reach",
        more: "",
    },
    Command {
        name: "connect",
        also: &[],
        usage: "m2 connect <project> <signal> <control>",
        words: "write a signal's name in place of a control's value, verified",
        more: "",
    },
    Command {
        name: "disconnect",
        also: &[],
        usage: "m2 disconnect <project> <input>",
        words: "write the input's declared default in place of the name, verified",
        more: "",
    },
    Command {
        name: "effects",
        also: &[],
        usage: "m2 effects <project>",
        words: "the effects packages offer, by id (`std:delay`), and the calls they can go after",
        more: "",
    },
    Command {
        name: "add-effect",
        also: &[],
        usage: "m2 add-effect <project> <call> <effect>",
        words: "wrap a call's audio in an effect, named by its component id (`std:delay`) or its export \
                (`std/fx.delay`), verified",
        more: "",
    },
    Command {
        name: "remove-effect",
        also: &[],
        usage: "m2 remove-effect <project> <call>",
        words: "take an effect out, leaving what it was applied to, verified",
        more: "",
    },
    Command {
        name: "relink",
        also: &[],
        usage: "m2 relink <project> <from> <to>",
        words: "move data no lane or plugin reads (left behind when a key was renamed in code) to where it is read \
                now, verified: a lane file (`data/kick/hits.json data/kicks/hits.json`), a track's folder \
                (`data/kick data/kicks`) or a plugin state",
        more: "",
    },
    Command {
        name: "delete-data",
        also: &[],
        usage: "m2 delete-data <project> <path>",
        words: "delete data no lane or plugin reads: a lane file, a folder, a state",
        more: "",
    },
    Command {
        name: "fix",
        also: &[],
        usage: "m2 fix <project> <path> <fix>",
        words: "apply a fix a problem offers (`assign-clip-ids`, `repin-asset`, …)",
        more: "`finish-tx` and `discard-tx` finish or put back an edit that was cut off (its path is \
               `.music2/tx/<id>.json`). Every command prints the problems it finds with the commands that fix them.",
    },
    Command {
        name: "tx",
        also: &[],
        usage: "m2 tx <project> <tx.json | -> [--check] [--seen HASH]",
        words: "a verified edit as Tx JSON: `--check` prints its delta, the delta's hash and the diagnostics, writing \
                nothing; without it, it is committed (with `--seen`, only while its delta has that hash) and the \
                outcome printed as JSON; exit 1 when it isn't committed",
        more: "[The local API](api.md#edits) says what a Tx holds.",
    },
    Command {
        name: "plugins",
        also: &[],
        usage: "m2 plugins [clap:<id>]",
        words: "list installed CLAP plugins, or one plugin's ports",
        more: "",
    },
    Command {
        name: "pack",
        also: &[],
        usage: "m2 pack new <folder> --name @owner/name\nm2 pack import-sfz <file.sfz> <folder> --name @owner/name [--label L] \
                [--category C] [--license L]\nm2 pack check <folder>\nm2 pack build <folder>\nm2 pack install <folder>\n\
                m2 pack get <@owner/name>[@<version>]\nm2 pack get [<project>]\nm2 pack list\nm2 pack remove @owner/name <version>\n\
                m2 pack lock <project>\nm2 pack update <project> [@owner/name]",
        words: "make, check and install packs: instruments, effects, sounds and recordings in a folder. `new` makes one with an \
                example instrument; `import-sfz` makes one from an SFZ instrument (its recordings, as an instrument written as \
                data), saying what it couldn't keep; `check` says in words what the registry would refuse (exit 1); `build` writes its \
                `m2pack.json`; `install` checks, builds and copies it into your store, read-only; `get` gets one from the library \
                (the registry), the latest version unless one is named, or every pack a song's `music2.lock` names that isn't \
                installed; `list` and `remove` show and take out what is installed; `lock` writes the song's `music2.lock` from its `music2.toml`, moving only what must \
                move; `update` moves the song to the newest installed version each line allows (or that pack's only)",
        more: "Your store is `~/Music/music2/Packs`, one folder a version (`Packs/@alice/felt-piano/1.2.0/`). A version is never \
               changed: installing one that is there with other files is refused, and installing it again when its copy was \
               changed puts it back. A song keeps the version it was made with in `music2.lock`; adding a pack's instrument or \
               effect in the app writes it. To move a song to another version, change its line in `music2.toml`, then run \
               `m2 pack lock`; both lock and update work while the song doesn't build. `get` reads the registry \
               `M2_REGISTRY` names, else `registry` in `~/.music2/settings.json`; each file is checked by its hash, and a pack \
               is installed whole or not at all. See [Packs](packs.md), [Getting packs from the library](library.md) and \
               [Making a pack](making-a-pack.md).",
    },
    Command {
        name: "registry",
        also: &[],
        usage: "m2 registry check <registry> [--base <registry>] [--author <github-login>]\nm2 registry add <registry> <pack-folder> \
                --source <url>\nm2 registry pending <registry> --out <bucket> [--from <bucket>]\nm2 registry build <registry> \
                --sources <folder> --out <bucket> [--from <bucket>] [--made <time>] [--to-delete <file>]",
        words: "the pack registry's robot, and listing your pack in it. `check` reads every pack file strictly and says what a \
                change broke: a listed version changed or a take-down taken back (`--base`: the registry before), a file \
                changed by someone not its scope's (`--author`); `add` checks your pack, writes its `m2pack.json` and lists \
                its version in `packs/@owner/name.toml`; `pending` prints the versions the bucket hasn't got yet, each with \
                its hash and where to get it; `build` refuses to change anything the bucket publishes, then builds each \
                pending version on its own from `<sources>/<its hash>/`, writing its files, previews and facts into \
                `--out` by hash, then the catalog. A version that fails is left out and stays pending (exit 1)",
        more: "The registry is a folder: one file a pack, `packs/@owner/name.toml`, `tags.txt`, the words a pack's tags \
               may use, and `removed.toml`, the versions taken down, which only music2's people list. `--from` is the \
               bucket as it is: what it has isn't built again, and what it publishes never changes: `https://…` (the \
               bucket's own address, never a cache), a folder or `file:///…`. `--made` is the catalog's time, `2026-10-30T12:00:00Z` (now, when it is left out), so \
               one registry and one time always make the same bytes. `--to-delete` writes the bucket paths of taken-down \
               versions' files that no other version uses, one a line. See [Making a pack](making-a-pack.md#listing-it-in-the-registry).",
    },
    Command {
        name: "plugin-state",
        also: &[],
        usage: "m2 plugin-state <project> <track>/<key> clap:<id>",
        words: "save a plugin's default state to `data/<track>/<key>.state`, with the `.state.json` sidecar that says \
                which plugin wrote it, as one edit (replacing a state that is there, which undo puts back)",
        more: "It saves the plugin's default settings, not the values your code gives it ([CLAP plugins](plugins.md)).",
    },
    Command {
        name: "mute-in-song",
        also: &[],
        usage: "m2 mute-in-song <project> <track>… on|off",
        words: "mute tracks, named by their keys, in the song (every render and export), or unmute them: their \
                `track()` calls' `muted` option, as one edit that undo takes back",
        more: "",
    },
    Command {
        name: "clips",
        also: &[],
        usage: "m2 clips <project> create <track>/<lane> --at <place> [--length B] [--beats]\n\
                m2 clips <project> move <clip>… [--by B] [--to <track>/<lane>] [--copy] [--beats]\n\
                m2 clips <project> resize <clip> --start <place> --length B [--beats]\n\
                m2 clips <project> split <clip>… --at <place> [--beats]\n\
                m2 clips <project> join <clip>… [--beats]\n\
                m2 clips <project> duplicate <clip>… [--grid B] [--beats]\n\
                m2 clips <project> quantize <clip>… --grid B [--ends] [--beats]\n\
                m2 clips <project> delete <clip>… [--beats]\n\
                m2 clips <project> json <gesture.json | ->",
        words: "one clip gesture, as one edit that undo takes back. A clip is `track/lane:id` (`bass/notes:k3f9q2zt`), \
                or `track/lane@place` for the clip that starts there. Places are bar.beat.sixteenth (`9.3.1`), or \
                beats with `--beats`; `--by`, `--length` and `--grid` are beats. `json` takes any gesture, as the app \
                sends it",
        more: "Besides the gestures above, `json` takes the others `POST /api/graph/clips` takes: `paste`, `notes`, \
               `points`, `audio` and `set`.",
    },
    Command {
        name: "unautomate",
        also: &[],
        usage: "m2 unautomate <project> <track>/<lane> [--at <place>] [--beats]",
        words: "take a knob's automation out: what the lane plays at the place (else its fallback) is written back \
                where the lane is read, and the lane file goes, as one edit that undo takes back",
        more: "",
    },
    Command {
        name: "edit",
        also: &[],
        usage: "m2 edit <project> <file> --old <text> --new <text> [--all]",
        words: "replace text found once in a file of the song (every time, with `--all`), as one edit: how an agent \
                changes a file while the app has the song open. While the song's code doesn't build, it fixes a code \
                file: written only if the song then builds",
        more: "Found no times, or more than once without `--all`, it changes nothing and says how many times. If the \
               file changed after it was read, it reads it again and tries once more.",
    },
    Command {
        name: "write",
        also: &[],
        usage: "m2 write <project> <file> [<from> | -]",
        words: "write a whole text file of the song (from a file, or standard input), as one edit; while the code \
                doesn't build, as `m2 edit`",
        more: "It writes text only: `m2 import` brings audio in, `m2 plugin-state` saves a plugin's state.",
    },
    Command {
        name: "undo",
        also: &[],
        usage: "m2 undo <project>",
        words: "take back your own newest change in the open app (by `--as`): never the musician's, nor another \
                agent's",
        more: "With the app closed there is nothing to undo: undo lives in the open app.",
    },
    Command {
        name: "focus",
        also: &[],
        usage: "m2 focus <project>",
        words: "what the musician looks at in the open app, as JSON: the view, the selected track, clips and notes, \
                the knob last touched, the bars shown, the code's file and line, where playback is",
        more: "In full: the dock's view, the selected track by its key, the clips picked (`{track, lane, id}`) and a \
               marquee's bars (`range`), the roll's selected notes by `pitch`, `start` and `length`, the knob last \
               touched, the beats the timeline shows (`seen`), the code view's file and line; and from the app, \
               `playing`, `beat`, `loop`, the files with unsaved code, what V compares (`comparing`) and the MIDI \
               keyboard's target (`midi`). Fields the window hasn't said yet are null. It writes nothing.",
    },
    Command {
        name: "watch",
        also: &[],
        usage: "m2 watch <project> [--meters]",
        words: "follow the open app: each change, undo, toast, focus, pointer and transport change as one JSON object \
                a line, `t` first, until the app quits (then `closed`); meters too with `--meters`",
        more: "First it prints `{\"t\":\"watch\",\"session\":…}`, then each event the window hears, one JSON object a \
               line with `t` first and the song's `session`: a change landed, an undo or a redo, a toast, the focus, \
               a pointer, the Compare pill ([the local API](api.md#events) lists them), and `transport` when it plays \
               or stops, the loop changes or the playhead jumps (read every quarter second). The meters only with \
               `--meters`. When the app quits it prints `{\"t\":\"closed\",\"why\":\"app\"}` and exits 0; when the app \
               opens another song or closes this one, `{\"t\":\"closed\",\"why\":\"song\"}`, and it prints nothing of \
               the other song. While whoever reads it doesn't, it keeps 1,024 lines and drops the rest, then says \
               `{\"t\":\"lagged\",\"missed\":n,\"by\":\"m2\"}`; when they go away (`m2 watch … | head -1`), it ends \
               quietly. It writes nothing, and it never takes a message from the musician's window.",
    },
    Command {
        name: "point",
        also: &[],
        usage: "m2 point <project> <target> [<words>]\nm2 point <project> --clear",
        words: "show a note in the open app at `track:bass`, `clip:bass/notes:<id>`, `lane:bass/cutoff`, `knob:<id>` \
                or `bars:9-16` (by `--as`); it never moves the musician's view: they press play. `--clear` takes it \
                away; so does their OK",
        more: "`bars:9` is one bar. A pointer is how an agent says \"listen to bars 9 to 16\": the musician presses \
               play. It never selects anything. A target the song hasn't got is refused, \
               saying so, and so are bars past its end, saying how many bars it has.",
    },
    Command {
        name: "routes",
        also: &[],
        usage: "m2 routes [--json]",
        words: "every route of the app's API, from its one table: the ones for agents first, each with what it does, \
                whether it writes and an example body; the musician's own (play, record, the view…) last",
        more: "No app needs to run. `--json` prints the table: `[{method, path, limit, writes, agents, words, \
               example}]`, `limit` being the largest body the route takes (2 MiB, more for imports and Tx). \
               [API routes](routes.md) is the same table.",
    },
    Command {
        name: "call",
        also: &[],
        usage: "m2 call <project> <path> [<json> | -] [--method M]",
        words: "send one route's request to the open app, as an agent (by `--as`): song changes with no command of \
                their own (an effect's bypass, markers, the song's key) and every read. The song's session is filled \
                in; a GET's keys go in its query; it prints the answer and exits 1 when the app refuses. The \
                musician's controls are refused before anything is sent",
        more: "The song changes it reaches include a track's instrument or ID, and a bus. `--method` picks one \
               where a path takes two (`/api/settings`). Refused before anything is sent, too: a body bigger than \
               the route takes. A write the musician holds waits, as every `m2` write does (exit 3). On standard \
               error it says the undo step a write made, or what an undo took back.",
    },
    Command {
        name: "search",
        also: &[],
        usage: "m2 search [<words…>] [--kind <kind>] [--json]",
        words: "find what can be added to a song: instruments, sounds (presets), effects, samples and plugins, best \
                first, with the search ⌘F asks in the app. One line each: its name, its kind, what it is and where \
                it is from, and its id, which adding it names",
        more: "With the app open it asks the app, whose list has the open song's own instruments and the plugins it \
               has listed; with the app closed it searches music2's sounds, the packs you installed, your presets and \
               your sample folders itself. Every word must \
               match. `--kind` is `instrument`, `sound`, `effect`, `sample` or `plugin`; the same words typed in \
               the search filter too, and so do `loop`, `one-shot`, a tempo (`90 bpm`) and a key (`Am`, `F#`), \
               which only samples carry (a tempo within 2 bpm, a key or its relative). `--json` prints each item with all it knows: its `kind`, `id`, `label`, \
               `category`, `description`, `tags`, `package`, `component` (what `m2 add-effect` or a new track \
               takes), and a sound's `preset`. After what you have come the library's items of packs you haven't got \
               (when a registry is set), each marked `library`, its id `library:<id>`, with `library`: its pack, \
               version, licence and the bytes `m2 pack get` fetches; the search runs on this computer and sends \
               nothing. [Finding sounds](finding-sounds.md) says more.",
    },
    Command {
        name: "docs",
        also: &[],
        usage: "m2 docs [<page> | all]",
        words: "the guide, built into this `m2`: its pages, or one page as Markdown, or all of them. It needs no \
                song and no app",
        more: "A link to another page, such as `window.md`, is `m2 docs window`. `m2 docs commands` and \
               `m2 docs routes` are made from the code when asked. `m2 docs agents` is the `AGENTS.md` a new song \
               gets from this `m2`, naming it: `m2 docs agents > AGENTS.md` gives a song made before them its own.",
    },
];

/// Flags that are gone: never shown, but still known, so that the command says why, in words that
/// need the song read first (`--sr`: it renders at the song's own rate, which it names). One
/// written with a value (`--sr N`) takes the next word when it isn't a flag; any may be given
/// more than once: whatever is given, the command says it is gone.
pub const GONE: &[(&str, &str)] = &[("render", "--sr N"), ("bounce", "--sr N"), ("plugin-state", "--force")];

/// The command `word` runs, by its name or another word for it.
pub fn find(word: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == word || c.also.contains(&word))
}

/// `m2`'s usage: the title, each command's forms and words, then the notes, wrapped to 100 columns.
pub fn usage() -> String {
    let mut out = format!("{TITLE}\n\nUSAGE:\n");
    for c in COMMANDS {
        out += &entry(c);
    }
    format!("{out}\n{}\n", wrap(&plain(NOTES), WIDTH).join("\n"))
}

/// One command's part of the usage: its forms, and its words beside or below them. `m2 <command>
/// --help` prints it (AGENT-13).
pub fn entry(c: &Command) -> String {
    let mut out = String::new();
    let forms: Vec<String> = c.usage.lines().flat_map(|f| wrap_form(f, WIDTH - 2)).collect();
    // One short form: the words start beside it; else below the forms.
    let beside = forms.len() == 1 && forms[0].chars().count() + 3 <= WORDS;
    if !beside {
        for f in &forms {
            out += &format!("  {f}\n");
        }
    }
    for (i, w) in wrap(&plain(c.words), WIDTH - WORDS).iter().enumerate() {
        if i == 0 && beside {
            out += &format!("  {:<width$}{w}\n", forms[0], width = WORDS - 2);
        } else {
            out += &format!("{}{w}\n", " ".repeat(WORDS));
        }
    }
    out
}

/// The guide's page of every command, `docs/guide/commands.md`, as `m2 docs commands` prints it.
pub fn page() -> String {
    let mut out = String::from(
        "<!-- Made by `m2 docs commands` from apps/m2/src/commands.rs: change it there, then run ci/guide.sh gen. -->\n\
         # m2 commands\n\n\
         **In short**\n\
         - Every `m2` command: how to call it, and what it does. `m2` alone prints the same, shorter.\n\
         - `m2` works on song folders, with the app open or closed. A command that writes ends with \
         `--as <your name> --why \"<a few words>\"`.\n\
         - This page is made from `m2`'s own table, so `m2 docs commands` prints it as that `m2` has it.\n\n\
         From source, `cargo build --locked --release -p m2` builds `m2` as `target/release/m2`; in `music2.app` it is \
         `music2.app/Contents/MacOS/m2`, and from the Linux `.deb` `/usr/bin/m2`. In the examples below it is called `m2`. `<project>` is a song's folder.\n\n\
         ## How every command behaves\n\n",
    );
    out += NOTES;
    out += "\n";
    for c in COMMANDS {
        out += &format!("\n## m2 {}\n\n```\n{}\n```\n\n{}.", c.name, c.usage, sentence(c.words));
        if !c.more.is_empty() {
            out += &format!(" {}", c.more);
        }
        out += "\n";
    }
    out += &format!(
        "\n## Try them on a copy\n\nCommands that change a song write into its folder, so try them on a copy, not on the examples:\n\n{EXAMPLES}\n"
    );
    out
}

/// `words` as a sentence: a capital first, for the guide's page.
fn sentence(words: &str) -> String {
    let mut c = words.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

const WIDTH: usize = 100;
/// The column the words start at.
const WORDS: usize = 41;

/// Markdown's code spans as plain text.
fn plain(text: &str) -> String {
    text.replace('`', "")
}

/// `text` in lines of `width` at most, broken at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![];
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ').filter(|w| !w.is_empty()) {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line += word;
        }
        lines.push(line);
    }
    lines
}

/// One usage form in lines of `width` at most, broken only between its arguments (never inside
/// `[…]` or `<…>`), each line after the first indented by 10.
fn wrap_form(form: &str, width: usize) -> Vec<String> {
    let mut parts: Vec<String> = vec![];
    let (mut part, mut depth) = (String::new(), 0i32);
    for ch in form.chars() {
        match ch {
            '[' | '<' => depth += 1,
            ']' | '>' => depth -= 1,
            _ => {}
        }
        if ch == ' ' && depth == 0 {
            parts.push(std::mem::take(&mut part));
        } else {
            part.push(ch);
        }
    }
    parts.push(part);
    let mut lines = vec![];
    let mut line = String::new();
    for p in parts {
        if !line.is_empty() && line.chars().count() + 1 + p.chars().count() > width {
            lines.push(std::mem::take(&mut line));
            line = " ".repeat(10);
        } else if !line.trim().is_empty() {
            line.push(' ');
        }
        line += &p;
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn source(file: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file)).unwrap()
    }

    /// The quoted words of `text`: `"new"`, `"check" | "build"`.
    fn quoted(text: &str) -> Vec<String> {
        text.split('"').skip(1).step_by(2).map(String::from).collect()
    }

    /// The words `main.rs` runs a command for (its match arms `Some("new")`,
    /// `Some(command @ ("tempo" | "meter"))`), and those `edits::handles` takes.
    fn run() -> BTreeSet<String> {
        let mut words = BTreeSet::new();
        for line in source("main.rs").lines() {
            let Some(arm) = line.trim_start().strip_prefix("Some(") else { continue };
            if (arm.starts_with('"') || arm.starts_with("command @ (")) && arm.contains("=>") {
                words.extend(quoted(&arm[..arm.find("=>").unwrap()]));
            }
        }
        let edits = source("edits.rs");
        let handles = &edits[edits.find("matches!(cmd,").expect("edits::handles")..];
        words.extend(quoted(&handles[..handles.find(')').unwrap()]));
        words
    }

    // AGENT-12: every command m2 runs is declared once, in the one table, and the table declares
    // none it doesn't run.
    #[test]
    fn every_command_m2_runs_is_in_the_one_table() {
        let mut table = BTreeSet::new();
        for c in COMMANDS {
            for word in std::iter::once(c.name).chain(c.also.iter().copied()) {
                assert!(table.insert(word.to_string()), "`{word}` is declared twice in COMMANDS");
            }
        }
        let run = run();
        assert!(run.len() > 40, "main.rs's match arms weren't found: {run:?}");
        let missing: Vec<_> = run.difference(&table).collect();
        let extra: Vec<_> = table.difference(&run).collect();
        assert!(missing.is_empty(), "m2 runs {missing:?}, which apps/m2/src/commands.rs doesn't declare");
        assert!(extra.is_empty(), "apps/m2/src/commands.rs declares {extra:?}, which m2 doesn't run");
    }

    /// Flags only tests name, as ones m2 hasn't got: never shown.
    const TESTS: &[&str] = &[
        "--bras", // listen's test of a misspelt flag
        "--nope", // words.rs's test of a flag no command has
    ];

    // AGENT-12: every flag m2 reads is in a command's usage, or in the notes (--as, --why), but
    // those gone, which it knows only to refuse (GONE).
    #[test]
    fn every_flag_is_shown() {
        let shown: String = COMMANDS.iter().map(|c| c.usage).chain([NOTES]).collect::<Vec<_>>().join("\n");
        let shown: BTreeSet<&str> =
            shown.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).filter(|w| w.starts_with("--") && w.len() > 2).collect();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut read = BTreeSet::new();
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let text = std::fs::read_to_string(e.path()).unwrap();
            for q in quoted(&text) {
                if q.len() > 2 && q.starts_with("--") && q[2..].chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                    read.insert(q);
                }
            }
        }
        assert!(read.contains("--json") && read.contains("--as"), "no flags found in {}: {read:?}", dir.display());
        let gone: Vec<&str> = GONE.iter().map(|(_, f)| f.split(' ').next().unwrap()).chain(TESTS.iter().copied()).collect();
        let hidden: Vec<_> = read.iter().filter(|f| !shown.contains(f.as_str()) && !gone.contains(&f.as_str())).collect();
        assert!(hidden.is_empty(), "m2 reads {hidden:?}, which no usage line in apps/m2/src/commands.rs shows");
    }

    #[test]
    fn the_usage_fits_100_columns_and_names_every_command() {
        let u = usage();
        for line in u.lines() {
            assert!(line.chars().count() <= WIDTH, "{} columns: {line}", line.chars().count());
        }
        for c in COMMANDS {
            assert!(u.contains(&format!("  m2 {} ", c.name)) || u.contains(&format!("  m2 {}\n", c.name)), "no {}", c.name);
        }
        assert!(!u.contains('`'), "the usage is plain text");
    }
}
