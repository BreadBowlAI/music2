//! Previews (LIB-3): an item playing for 4.5 seconds, made by an offline render with music2's own
//! engine (RENDER-1) of a small song written for it, so a preview can't oversell: it is the item
//! playing.
//!
//! [`song`] writes the song: a phrase chosen by the item's category (a beat for drums, a line for a
//! bass or a lead, four chords for the rest), played by the item with a sound's preset values; an
//! effect plays a pluck and a drum kit, the first bar dry and the second through it. [`render`]
//! renders it in a folder of the cache, never a project, on a thread of its own at background
//! priority, and writes the preview: 16-bit stereo at 48 kHz, brought to one loudness ([`level`],
//! measured by [`loudness`] as ITU-R BS.1770 does). [`fingerprint`] is the hash of what it renders
//! and of what renders it, by which the server keeps it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use m2_runtime::PackageSet;
use m2_script::Catalog;
use m2_script::component::{Descriptor, SettingKind};
use m2_song::Project;

use super::{Item, Kind};
use crate::render::{Length, Render, Threads};

/// How long a preview is: two bars at 120 BPM, and half a second for the tail.
pub const SECONDS: f64 = 4.5;
/// Its last quarter second fades out, so a held note never ends in a click.
const FADE: f64 = 0.25;
/// Its sample rate.
pub const RATE: u32 = 48_000;
/// The loudness every preview is brought to, as [`loudness`] measures it (EBU R 128's)…
pub const LUFS: f64 = -23.0;
/// …unless a peak would go above this, in dBFS.
pub const PEAK_DB: f64 = -1.0;

/// A preview's song: its files, by path in the song's folder, but its manifest, which [`render`]
/// writes for the packages it loads.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub files: BTreeMap<String, String>,
}

/// What an item plays in its preview.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phrase {
    /// A beat on these notes: kick, snare, hat.
    Beat([u8; 3]),
    Bass,
    Lead,
    Chords,
}

/// A note in a lane: pitch, start and length in beats, velocity.
type Note = (u8, f64, f64, f64);

/// A lane's file with one clip of `beats`, holding `notes`.
fn lane(notes: &[Note], beats: f64) -> String {
    let notes: Vec<String> = notes
        .iter()
        .map(|(p, start, length, velocity)| format!(r#"{{"pitch": {p}, "start": {start}, "length": {length}, "velocity": {velocity}}}"#))
        .collect();
    format!("{{\"clips\": [{{\"start\": 0, \"length\": {beats}, \"notes\": [\n  {}\n]}}]}}\n", notes.join(",\n  "))
}

/// The phrase `d` plays: a beat for drums (on the notes its `editor.notes` names for kick, snare
/// and closed hat, else 36, 38 and 42), a line for a bass (`bass` or `sub` in its name) or a lead,
/// four chords for everything else.
fn phrase(d: &Descriptor) -> Phrase {
    let category = d.editor.category.clone().unwrap_or_default();
    if category == "Drums" {
        let find = |name: &str, or: u8| d.editor.notes.iter().find(|(_, n)| n.to_lowercase().contains(name)).map_or(or, |(k, _)| *k);
        return Phrase::Beat([find("kick", 36), find("snare", 38), find("closed", 42)]);
    }
    let named = format!("{} {} {category}", d.label, d.id).to_lowercase();
    let words: Vec<&str> = named.split(|c: char| !c.is_alphanumeric()).collect();
    if words.iter().any(|w| matches!(*w, "bass" | "sub")) {
        Phrase::Bass
    } else if words.contains(&"lead") {
        Phrase::Lead
    } else {
        Phrase::Chords
    }
}

/// The notes of `p` over two bars (eight beats).
fn notes(p: Phrase) -> Vec<Note> {
    match p {
        Phrase::Beat([kick, snare, hat]) => {
            let mut v = vec![];
            for bar in [0.0, 4.0] {
                v.extend([(kick, bar, 0.25, 0.95), (kick, bar + 2.0, 0.25, 0.9), (kick, bar + 2.5, 0.25, 0.7)]);
                v.extend([(snare, bar + 1.0, 0.25, 0.85), (snare, bar + 3.0, 0.25, 0.85)]);
                v.extend((0..8).map(|i| (hat, bar + i as f64 / 2.0, 0.2, if i % 2 == 0 { 0.6 } else { 0.4 })));
            }
            v.sort_by(|a, b| (a.1, a.0).partial_cmp(&(b.1, b.0)).expect("no NaN"));
            v
        }
        Phrase::Bass => [
            (33, 0.0, 0.75),
            (33, 1.0, 0.4),
            (45, 1.5, 0.4),
            (36, 2.0, 0.75),
            (38, 3.0, 0.75),
            (29, 4.0, 0.75),
            (29, 5.0, 0.4),
            (41, 5.5, 0.4),
            (31, 6.0, 0.75),
            (31, 7.0, 0.9),
        ]
        .map(|(p, s, l)| (p, s, l, 0.85))
        .to_vec(),
        Phrase::Lead => [
            (69, 0.0, 0.45),
            (72, 0.5, 0.45),
            (76, 1.0, 0.9),
            (74, 2.0, 0.45),
            (72, 2.5, 0.45),
            (69, 3.0, 0.9),
            (67, 4.0, 0.9),
            (69, 5.0, 2.5),
        ]
        .map(|(p, s, l)| (p, s, l, 0.8))
        .to_vec(),
        // Am, F, C, G: two beats each.
        Phrase::Chords => [[57, 60, 64], [57, 60, 65], [55, 60, 64], [55, 59, 62]]
            .iter()
            .enumerate()
            .flat_map(|(i, chord)| chord.map(|p| (p, i as f64 * 2.0, 1.9, 0.75)))
            .collect(),
    }
}

/// How code imports `id`, as `alias`: the import line, and what calls it. `synth.pluck` from
/// `std/synth` is `import { synth as alias } from "std/synth";`, called as `alias.pluck`.
fn imported(catalog: &Catalog, id: &str, alias: &str) -> Result<(String, String), String> {
    let cant = || format!("{id} can't be imported, so it has no preview.");
    let exports = catalog.exports.get(id).ok_or_else(cant)?;
    let e = exports.iter().find(|e| !e.name.contains('.')).or_else(|| exports.first()).ok_or_else(cant)?;
    let module = serde_json::to_string(&e.module).expect("a string is JSON");
    Ok(match e.name.split_once('.') {
        Some((object, member)) => (format!("import {{ {object} as {alias} }} from {module};"), format!("{alias}.{member}")),
        None => (format!("import {{ {} as {alias} }} from {module};", e.name), alias.to_string()),
    })
}

/// The options a call of `d` is written with: `values` (a preset's, in base units) over what adding
/// it writes (`editor.insert`), each only where `d` declares that control (a dictionary's left
/// out). Empty when there are none.
fn options(d: &Descriptor, values: &BTreeMap<String, f64>) -> String {
    let mut all: BTreeMap<&str, f64> = d.editor.insert.iter().map(|i| (i.slot.as_str(), i.value.value)).collect();
    all.extend(values.iter().map(|(k, v)| (k.as_str(), *v)));
    let written: Vec<String> = all
        .into_iter()
        .filter(|(k, v)| v.is_finite() && d.controls.iter().any(|c| c.name == *k && c.each.is_none()))
        .map(|(k, v)| format!("{}: {v}", serde_json::to_string(k).expect("a string is JSON")))
        .collect();
    if written.is_empty() { String::new() } else { format!(", {{ {} }}", written.join(", ")) }
}

const ARRANGEMENT: &str = "{\n  \"tempo\": [{\"beat\": 0, \"bpm\": 120}],\n  \"meter\": []\n}\n";
const IMPORTS: &str = "import { song, track } from \"@m2/core\";\nimport { mixer } from \"std/mix\";\n";

/// Why component `d` has no preview, in words, if it has none: the song's own code (a preview renders
/// outside the song), or an instrument that plays a file you give it (an asset setting, as the
/// Sampler's), which a preview has none to give, so it would be silence.
pub fn none_for(d: &Descriptor) -> Option<String> {
    if !d.id.contains(':') {
        return Some(format!("{} is the song's own, so it has no preview here.", d.label));
    }
    if d.settings.iter().any(|s| matches!(s.kind, SettingKind::Asset | SettingKind::Keymap)) {
        return Some(format!("{} plays a file you give it, so it has no preview.", d.label));
    }
    None
}

/// The song `item`'s preview renders, with `values` (a sound's preset) written into its call; or
/// why it has none, in words: a plugin, a plugin's preset, the song's own code and an instrument
/// that plays a file you give it have none.
pub fn song(item: &Item, catalog: &Catalog, values: &BTreeMap<String, f64>) -> Result<Song, String> {
    if item.kind == Kind::Plugin {
        return Err("Plugins have no preview yet.".into());
    }
    if item.component.starts_with("clap:") {
        return Err("A plugin's sounds have no preview yet.".into());
    }
    let d = catalog.components.get(&item.component);
    if let Some(why) = d.and_then(|d| none_for(d)) {
        return Err(why);
    }
    if !item.preview {
        return Err(format!("{} is the song's own, so it has no preview here.", item.label));
    }
    let d = d.ok_or_else(|| format!("{} isn't there any more.", item.label))?;
    let (import, call) = imported(catalog, &d.id, "item")?;
    let opts = options(d, values);
    let mut files = BTreeMap::new();
    files.insert("data/arrangement.json".to_string(), ARRANGEMENT.to_string());
    let text = if d.is_effect() {
        let (keys_import, keys) = imported(catalog, "std:synth-pluck", "keys")?;
        let (kit_import, kit) = imported(catalog, "std:drum-kit", "kit")?;
        let both = format!("mixer({{ keys: {keys}(lane.notes(\"keys\")), drums: {kit}(lane.notes(\"hits\")) }})");
        // The loop: Am then F, plucked, over a kick, a snare and hats. Bar 1 is dry, bar 2 goes
        // through the effect.
        let stabs: Vec<Note> = [
            (0.0, [57, 60, 64]),
            (0.75, [57, 60, 64]),
            (1.5, [57, 60, 64]),
            (2.0, [53, 57, 60]),
            (2.75, [53, 57, 60]),
            (3.5, [53, 57, 60]),
        ]
        .iter()
        .flat_map(|(at, chord)| chord.map(|p| (p, *at, 0.4, 0.8)))
        .collect();
        let beat: Vec<Note> = notes(Phrase::Beat([36, 38, 42])).into_iter().filter(|n| n.1 < 4.0).collect();
        let later = |v: &[Note]| v.iter().map(|&(p, s, l, vel)| (p, s + 4.0, l, vel)).collect::<Vec<Note>>();
        files.insert("data/dry/keys.json".into(), lane(&stabs, 8.0));
        files.insert("data/dry/hits.json".into(), lane(&beat, 8.0));
        files.insert("data/wet/keys.json".into(), lane(&later(&stabs), 8.0));
        files.insert("data/wet/hits.json".into(), lane(&later(&beat), 8.0));
        format!(
            "{IMPORTS}{keys_import}\n{kit_import}\n{import}\n\n\
             const dry = track(\"dry\", {{ name: \"Dry\" }}, ({{ lane }}) => {both});\n\
             const wet = track(\"wet\", {{ name: \"Wet\" }}, ({{ lane }}) => {call}({both}{opts}));\n\n\
             export default song(() => mixer({{ dry: dry.out, wet: wet.out }}));\n"
        )
    } else if d.is_instrument() {
        files.insert("data/play/notes.json".into(), lane(&notes(phrase(d)), 8.0));
        format!(
            "{IMPORTS}{import}\n\n\
             const play = track(\"play\", {{ name: \"Play\" }}, ({{ lane }}) => {call}(lane.notes(\"notes\"){opts}));\n\n\
             export default song(() => mixer({{ play: play.out }}));\n"
        )
    } else {
        return Err(format!("{} is neither an instrument nor an effect, so it has no preview.", item.label));
    };
    files.insert("song.ts".into(), text);
    Ok(Song { files })
}

/// The files of package folder `dir` a render reads: its manifest, its kernels and its code.
fn package_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![dir.join("m2pkg.toml")];
    let mut stack = vec![dir.join("src"), dir.join("kernels")];
    while let Some(d) = stack.pop() {
        for p in std::fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.path()) {
            if p.is_dir() { stack.push(p) } else { out.push(p) }
        }
    }
    out.sort();
    out
}

/// The hash of what a preview renders and of what renders it: how it renders, music2's version and
/// its SDK's ([`made_by`]), its song's files, and each file of the packages it loads (their
/// manifests, kernels and code). A changed preset, package or music2 gives another.
pub fn fingerprint(song: &Song, packages: &[(String, PathBuf)]) -> m2_song::Sha {
    fingerprint_by(song, packages, &made_by())
}

/// What renders a preview: music2's version, and the version of the SDK (`@m2/core`) its song is
/// built with: `music2 0.1.0, @m2/core 1.0.0`.
pub fn made_by() -> String {
    format!("music2 {}, @m2/core {}", env!("CARGO_PKG_VERSION"), m2_script::SDK_VERSION)
}

/// [`fingerprint`], with `by` for what renders it.
pub fn fingerprint_by(song: &Song, packages: &[(String, PathBuf)], by: &str) -> m2_song::Sha {
    let mut all: Vec<u8> =
        format!("m2 preview by {by}: {SECONDS} s at {RATE} Hz, {LUFS} LUFS, {PEAK_DB} dB peak, {FADE} s fade\n").into_bytes();
    for (path, text) in &song.files {
        all.extend(format!("{path}\0{}\0", text.len()).bytes());
        all.extend(text.bytes());
    }
    for (name, dir) in packages {
        for file in package_files(dir) {
            let bytes = std::fs::read(&file).unwrap_or_default();
            let rel = file.strip_prefix(dir).unwrap_or(&file).to_string_lossy().replace('\\', "/");
            all.extend(format!("{name}/{rel}\0{}\0", bytes.len()).bytes());
            all.extend(bytes);
        }
    }
    m2_song::Sha::of(&all)
}

/// Bring interleaved stereo `samples` at [`RATE`] to one loudness: [`LUFS`], or less where a peak
/// would pass [`PEAK_DB`] (a sound of short, sharp hits); then fade the last [`FADE`] seconds out to
/// silence. Silence, or a sound too quiet to measure, is no preview: said in words.
pub fn level(samples: &mut [f32]) -> Result<()> {
    let Some(loud) = loudness(samples) else { bail!("it made no sound, so it has no preview") };
    let peak = samples.iter().fold(0f64, |m, &s| m.max((s as f64).abs()));
    let db = |d: f64| 10f64.powf(d / 20.0);
    let gain = db(LUFS - loud).min(db(PEAK_DB) / peak);
    for s in samples.iter_mut() {
        *s = (*s as f64 * gain) as f32;
    }
    let frames = samples.len() / 2;
    let fade = ((FADE * RATE as f64) as usize).min(frames);
    for i in 0..fade {
        // From 1 down to 0 at the last frame.
        let g = (fade - 1 - i) as f32 / (fade - 1).max(1) as f32;
        let at = (frames - fade + i) * 2;
        samples[at] *= g;
        samples[at + 1] *= g;
    }
    Ok(())
}

/// The integrated loudness of interleaved stereo `samples` at [`RATE`], in LUFS, as ITU-R BS.1770-4
/// measures it: K-weighted, in blocks of 400 ms that overlap by 300, gated at −70 LUFS and then at
/// 10 LU under the loudness of the blocks left. None when every block is under −70: silence.
pub fn loudness(samples: &[f32]) -> Option<f64> {
    // K-weighting at 48 kHz: a high shelf, then a high-pass (BS.1770-4, table 1 and table 2).
    const SHELF: ([f64; 3], [f64; 2]) = ([1.53512485958697, -2.69169618940638, 1.19839281085285], [-1.69065929318241, 0.73248077421585]);
    const HIGH: ([f64; 3], [f64; 2]) = ([1.0, -2.0, 1.0], [-1.99004745483398, 0.99007225036621]);
    let frames = samples.len() / 2;
    let weighted = |channel: usize| -> Vec<f64> {
        let mut out: Vec<f64> = (0..frames).map(|i| samples[i * 2 + channel] as f64).collect();
        for (b, a) in [SHELF, HIGH] {
            let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
            for v in out.iter_mut() {
                let y = b[0] * *v + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
                (x2, x1, y2, y1) = (x1, *v, y1, y);
                *v = y;
            }
        }
        out
    };
    let (left, right) = (weighted(0), weighted(1));
    let (block, hop) = (RATE as usize * 4 / 10, RATE as usize / 10);
    let powers: Vec<f64> = (0..)
        .map(|n| n * hop)
        .take_while(|at| at + block <= frames)
        .map(|at| (at..at + block).map(|i| left[i] * left[i] + right[i] * right[i]).sum::<f64>() / block as f64)
        .collect();
    let lufs = |power: f64| -0.691 + 10.0 * power.log10();
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let heard: Vec<f64> = powers.into_iter().filter(|&p| p > 0.0 && lufs(p) > -70.0).collect();
    if heard.is_empty() {
        return None;
    }
    let gate = lufs(mean(&heard)) - 10.0;
    let kept: Vec<f64> = heard.into_iter().filter(|&p| lufs(p) > gate).collect();
    Some(lufs(mean(&kept)))
}

/// A version as a manifest asks for it: `1` of `1.4.0`, `0.4` of `0.4.0`.
fn wanted(version: &str) -> String {
    match version.split('.').collect::<Vec<_>>().as_slice() {
        ["0", minor, ..] => format!("0.{minor}"),
        [major, ..] => major.to_string(),
        [] => version.to_string(),
    }
}

/// Render `song` with `packages` loaded, in `work` (a folder of the cache, made here and removed
/// after), and write the preview to `out`: 16-bit stereo at [`RATE`], [`SECONDS`] long, through a
/// part file renamed into place, which goes if anything fails. A pure render, so the same song and
/// packages give the same bits. It builds and renders on a thread of its own at background
/// priority, as an export's workers do beside playback (AGENT-7), and waits for it.
pub fn render(song: &Song, packages: &[(String, PathBuf)], work: &Path, out: &Path) -> Result<()> {
    std::thread::scope(|s| {
        let made = std::thread::Builder::new().name("music2-preview".into()).spawn_scoped(s, || {
            // Never fatal: a thread the OS won't lower renders as it is.
            let _ = m2_os::thread::background();
            let _ = std::fs::remove_dir_all(work);
            let result = render_in(song, packages, work, out);
            let _ = std::fs::remove_dir_all(work);
            result
        })?;
        made.join().map_err(|_| anyhow!("the preview's render stopped"))?
    })
}

/// A file being written, removed when dropped unless kept: a write that fails leaves nothing.
struct Part<'a>(&'a Path, bool);

impl Drop for Part<'_> {
    fn drop(&mut self) {
        if !self.1 {
            let _ = std::fs::remove_file(self.0);
        }
    }
}

fn render_in(song: &Song, packages: &[(String, PathBuf)], work: &Path, out: &Path) -> Result<()> {
    let mut set = PackageSet::default();
    let mut deps = format!("\"@m2/core\" = \"{}\"\n", wanted(m2_script::SDK_VERSION));
    for (name, dir) in packages {
        let p = set.load(dir).with_context(|| format!("loading the {name} package"))?;
        deps += &format!("{} = \"{}\"\n", serde_json::to_string(name)?, wanted(&p.version));
    }
    let manifest = format!("[project]\nname = \"Preview\"\nformat = 1\nsample_rate = {RATE}\n\n[dependencies]\n{deps}");
    for (path, text) in std::iter::once((&"music2.toml".to_string(), &manifest)).chain(&song.files) {
        let file = work.join(path);
        std::fs::create_dir_all(file.parent().expect("a file in the song"))?;
        std::fs::write(&file, text)?;
    }
    let project = Project::open(work).map_err(|d| anyhow!("{}", d.first().map_or("the preview's song can't be read", |d| &d.message)))?;
    let compiled =
        m2_session::compile(&project, &mut set).map_err(|d| anyhow!("{}", d.first().map_or("it doesn't build", |d| &d.message)))?;
    let wav = work.join("render.wav");
    Render::new(Arc::new(compiled), &set, RATE, 0.0, Length::Seconds(SECONDS), Threads::Exactly(1))?.write(&wav, |_| {})?;
    let mut reader = hound::WavReader::open(&wav)?;
    let spec = reader.spec();
    if spec.channels != 2 {
        bail!("the preview rendered {} channels, not 2", spec.channels);
    }
    let full = (1i64 << (spec.bits_per_sample - 1)) as f32;
    let mut samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader.samples::<i32>().map(|s| s.map(|s| s as f32 / full)).collect::<Result<_, _>>()?,
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
    };
    level(&mut samples)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = out.with_extension(format!("wav.{}.part", std::process::id()));
    let mut guard = Part(&part, false);
    let spec = hound::WavSpec { channels: 2, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&part, spec)?;
    for s in samples {
        w.write_sample((s * 32767.0).round().clamp(-32768.0, 32767.0) as i16)?;
    }
    w.finalize()?;
    std::fs::rename(&part, out)?;
    guard.1 = true;
    Ok(())
}
