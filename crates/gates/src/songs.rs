//! The small songs each gate plays, written as ordinary projects (TypeScript and lane files) so
//! they compile the way a user's song does. All run at 127 bpm, the contract's loop tempo.

use std::path::Path;

use anyhow::{Context, Result, bail};
use m2_song::lane::{self, LaneData, LaneKind};
use m2_song::{Arrangement, Note, NoteClip, Tempo};

pub const BPM: f64 = 127.0;

/// The loop the contract measures: 4 bars at 127 bpm, from beat 128 (about 1:00 in).
pub const LOOP: (f64, f64) = (128.0, 144.0);

/// A project's files, by path.
pub type Files = Vec<(String, String)>;

fn project(name: &str, song: &str, lanes: &[(&str, Vec<NoteClip>)]) -> Files {
    let mut files = vec![
        (
            "music2.toml".into(),
            format!(
                "[project]\nname = \"{name}\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n"
            ),
        ),
        ("data/arrangement.json".into(), Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: BPM }], meter: vec![] }.canonical()),
        ("song.ts".into(), song.to_string()),
    ];
    for (path, clips) in lanes {
        files.push((format!("data/{path}.json"), lane::canonical(&LaneData::Notes(clips.clone()))));
    }
    files
}

pub fn note(pitch: f64, start: f64, length: f64) -> Note {
    Note { pitch, start, length, velocity: 0.8 }
}

pub fn clip(id: Option<&str>, start: f64, length: f64, notes: Vec<Note>) -> NoteClip {
    NoteClip { id: id.map(str::to_string), other_id: None, start, length, notes }
}

/// What rings after the tone in [`tail`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fx {
    /// A reverb that rings for many seconds (size 0.98, no damping, all wet): the loop's
    /// tail-cut probe.
    LongReverb,
    /// Echoes every 250 ms that barely fade (feedback 0.9, all wet).
    LongDelay,
    /// An ordinary reverb, half wet.
    Reverb,
    /// Echoes every 250 ms at feedback 0.6, half wet.
    Delay,
    /// A 500 ms delay inside each voice: silent gaps between echoes that an engine idling on
    /// silence would swallow.
    VoiceDelay,
}

/// One saw note from beat 132, the middle of [`LOOP`], into `fx`: 4 beats long, or 0.4 for the
/// voice delay so its echoes have silence between them. Nothing starts in the loop's first 4
/// beats or its last 8, so whatever sounds right after a wrap is a tail.
pub fn tail(fx: Fx) -> Files {
    let env = "adsr(n.gate, { attack: ms(5), decay: secs(0.2), sustain: 0.7, release: ms(60) })";
    let voice = format!("vca(osc.saw(n.pitch), scale({env}, scale(n.velocity, 0.5)))");
    let notes = "lane.notes(\"notes\")";
    let body = match fx {
        Fx::LongReverb => format!("reverb(poly({notes}, (n) => {voice}, {{ voices: 4 }}), {{ size: 0.98, damping: 0, mix: 1 }})"),
        Fx::LongDelay => format!("delay(poly({notes}, (n) => {voice}, {{ voices: 4 }}), {{ time: 0.25, feedback: 0.9, mix: 1 }})"),
        Fx::Reverb => format!("reverb(poly({notes}, (n) => {voice}, {{ voices: 4 }}), {{ size: 0.7, damping: 0.3, mix: 0.5 }})"),
        Fx::Delay => format!("delay(poly({notes}, (n) => {voice}, {{ voices: 4 }}), {{ time: 0.25, feedback: 0.6, mix: 0.5 }})"),
        Fx::VoiceDelay => format!("poly({notes}, (n) => delay({voice}, {{ time: 0.5, feedback: 0.6, mix: 0.5 }}), {{ voices: 4 }})"),
    };
    let song = format!(
        "import {{ ms, poly, secs, song, track }} from \"@m2/core\";\nimport {{ adsr, osc, vca }} from \"std/dsp\";\n\
         import {{ delay, reverb }} from \"std/fx\";\nimport {{ scale }} from \"std/math\";\n\n\
         const tone = track(\"tone\", ({{ lane }}) => {body});\n\nexport default song(() => tone.out);\n"
    );
    let length = if fx == Fx::VoiceDelay { 0.4 } else { 4.0 };
    project("tail", &song, &[("tone/notes", vec![clip(Some("tail"), 128.0, 16.0, vec![note(48.0, 4.0, length)])])])
}

/// A short 1 kHz click on every beat from 0 to 160, dry: onsets a detector finds to the sample,
/// for drift against the grid.
pub fn clicks() -> Files {
    let song = "import { hz, ms, poly, song, track } from \"@m2/core\";\nimport { adsr, osc, vca } from \"std/dsp\";\n\
                import { scale } from \"std/math\";\n\n\
                const click = track(\"click\", ({ lane }) =>\n  poly(lane.notes(\"notes\"), (n) => vca(osc.sine(hz(1000)), \
                scale(adsr(n.gate, { attack: ms(0.5), decay: ms(25), sustain: 0, release: ms(5) }), n.velocity)), { voices: 4 }),\n);\n\n\
                export default song(() => click.out);\n";
    let notes = (0..160).map(|b| note(84.0, b as f64, 0.1)).collect();
    project("clicks", song, &[("click/notes", vec![clip(Some("clicks"), 0.0, 160.0, notes)])])
}

/// A full-scale 440 Hz sine, with or without a delay after it: the contract's step test.
pub fn sine(with_delay: bool) -> Files {
    let out = if with_delay { "delay(osc.sine(hz(440)), { time: 0.25, feedback: 0.3, mix: 0.3 })" } else { "osc.sine(hz(440))" };
    let song = format!(
        "import {{ hz, song }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\nimport {{ delay }} from \"std/fx\";\n\n\
         export default song(() => {out});\n"
    );
    project("sine", &song, &[])
}

/// A sine voice playing A4 on every beat for 512 beats, each note 0.9 beats long, with or without
/// a delay after the `poly`: a note sounds through most of every beat, and a new one starts on the
/// next, so an activation inside a note switches sound (G6).
pub fn poly_sine(with_delay: bool) -> Files {
    let voices = "poly(lane.notes(\"notes\"), (n) => vca(osc.sine(n.pitch), adsr(n.gate, { attack: ms(5), decay: secs(0.1), sustain: 1, release: ms(50) })), { voices: 4 })";
    let body = if with_delay { format!("delay({voices}, {{ time: 0.25, feedback: 0.3, mix: 0.3 }})") } else { voices.to_string() };
    let song = format!(
        "import {{ ms, poly, secs, song, track }} from \"@m2/core\";\nimport {{ adsr, osc, vca }} from \"std/dsp\";\n\
         import {{ delay }} from \"std/fx\";\n\nconst v = track(\"voice\", ({{ lane }}) => {body});\n\nexport default song(() => v.out);\n"
    );
    let notes = (0..512).map(|b| note(69.0, b as f64, 0.9)).collect();
    project("poly-sine", &song, &[("voice/notes", vec![clip(Some("beats"), 0.0, 512.0, notes)])])
}

/// How the pad note of [`pad`] is written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pad {
    /// Once.
    One,
    /// Twice in one clip: duplicate notes (FMT-12 keeps both).
    Twice,
    /// In two clips with the same id, layered.
    Layered,
    /// In two clips with no id, at the same start.
    Anonymous,
}

/// Where the pad note sounds: beats 4 to 36.
pub const PAD_NOTE: (f64, f64) = (4.0, 36.0);

/// std's `pad` holding middle C over beats 4 to 36, written as `how` says.
pub fn pad(how: Pad) -> Files {
    let song = "import { song, track } from \"@m2/core\";\nimport { pad } from \"std/instruments\";\n\n\
                const p = track(\"pad\", ({ lane }) => pad(lane.notes(\"notes\")));\n\nexport default song(() => p.out);\n";
    let n = || note(60.0, PAD_NOTE.0, PAD_NOTE.1 - PAD_NOTE.0);
    let clips = match how {
        Pad::One => vec![clip(Some("padclip"), 0.0, 64.0, vec![n()])],
        Pad::Twice => vec![clip(Some("padclip"), 0.0, 64.0, vec![n(), n()])],
        Pad::Layered => vec![clip(Some("layer"), 0.0, 64.0, vec![n()]), clip(Some("layer"), 0.0, 64.0, vec![n()])],
        Pad::Anonymous => vec![clip(None, 0.0, 64.0, vec![n()]), clip(None, 0.0, 64.0, vec![n()])],
    };
    project("pad", song, &[("pad/notes", clips)])
}

/// `files` with the tempo `bpm` instead.
pub fn with_tempo(mut files: Files, bpm: f64) -> Files {
    for (path, text) in &mut files {
        if path == "data/arrangement.json" {
            *text = Arrangement { tempo: vec![Tempo { beat: 0.0, bpm }], meter: vec![] }.canonical();
        }
    }
    files
}

/// Write `files` into `dir`.
pub fn write(dir: &Path, files: &Files) -> Result<()> {
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().expect("inside dir"))?;
        std::fs::write(&p, text).with_context(|| format!("writing {}", p.display()))?;
    }
    Ok(())
}

/// Rewrite the notes lane `module/lane` of the project in `dir` through `edit`, in canonical form,
/// as the piano roll or an agent would.
pub fn edit_notes(dir: &Path, module: &str, lane: &str, edit: impl FnOnce(&mut Vec<NoteClip>)) -> Result<()> {
    let path = dir.join("data").join(module).join(format!("{lane}.json"));
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let LaneData::Notes(mut clips) = LaneData::read(&text, &LaneKind::Notes).map_err(|e| anyhow::anyhow!("{e:?}"))? else {
        bail!("{} isn't a notes lane", path.display())
    };
    edit(&mut clips);
    std::fs::write(&path, lane::canonical(&LaneData::Notes(clips)))?;
    Ok(())
}
