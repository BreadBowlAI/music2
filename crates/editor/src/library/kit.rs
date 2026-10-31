//! std's kit (LIB-8): a few one-shots of music2's own drum machine (`@m2/drums`, PACK-15),
//! rendered by music2 itself, so a beginner has sounds to drop in before getting any pack, under
//! std's licence.
//!
//! [`make`] renders one drum: a small song in a scratch folder in which `@m2/drums`' kit plays the
//! drum's note once at beat 0, an offline render (RENDER-1, so the same bits every time), cut where
//! it falls 80 dB under its peak, brought to a peak of −1 dBFS and written as a 16-bit WAV at
//! 48 kHz, mono as the drum is. `std/samples/make.sh` writes them into `std/samples/kit/`; the test
//! below checks, whenever it isn't asked to write, that they are made again to the same bytes.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use m2_runtime::PackageSet;
use m2_song::Project;

use super::preview::{ARRANGEMENT, lane, wanted};
use crate::render::{Length, Render, Threads};

/// std's kit: each drum's file name, and the note std's drum kit plays it on (General MIDI).
pub const KIT: [(&str, u8); 14] = [
    ("kick", 36),
    ("snare", 38),
    ("clap", 39),
    ("hat-closed", 42),
    ("hat-open", 46),
    ("tom-low", 41),
    ("tom-mid", 45),
    ("tom-high", 48),
    ("rim", 37),
    ("cowbell", 56),
    ("crash", 49),
    ("ride", 51),
    ("shaker", 69),
    ("tambourine", 54),
];

/// Where they are in std.
pub const FOLDER: &str = "samples/kit";
/// How long each is rendered before it is cut: longer than the longest, the crash.
const SECONDS: f64 = 6.0;
const RATE: u32 = 48_000;
/// The peak each is brought to, −1 dBFS: 10^(−1/20), written out, so no host maths makes it
/// (PLAT-4).
const PEAK: f64 = 0.891_250_938_133_745_6;
/// Where its tail is cut: 80 dB under its peak.
const FLOOR: f64 = 1e-4;

/// The song that plays `note` once, on music2's drum kit.
const SONG: &str = "import { song, track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\n\
                    const kit = track(\"kit\", { name: \"Kit\" }, ({ lane }) => drums.kit(lane.notes(\"notes\")));\n\n\
                    export default song(() => kit.out);\n";

/// The bytes of the kit's file for `note`: the drum kit of the pack `@m2/drums` in `drums`, on std
/// in `std`, plays it, rendered in the scratch folder `work` (made here, and removed after).
pub fn make(std: &Path, drums: &Path, note: u8, work: &Path) -> Result<Vec<u8>> {
    let _ = std::fs::remove_dir_all(work);
    let made = make_in(std, drums, note, work);
    let _ = std::fs::remove_dir_all(work);
    made
}

fn make_in(std: &Path, drums: &Path, note: u8, work: &Path) -> Result<Vec<u8>> {
    let mut set = PackageSet::default();
    let version = set.load(std).context("loading std")?.version.clone();
    // The pack from its folder (PACK-7), by its whole path: the song is in `work`.
    let drums = drums.canonicalize().with_context(|| format!("finding @m2/drums at {}", drums.display()))?;
    let manifest = format!(
        "[project]\nname = \"Kit\"\nformat = 1\nsample_rate = {RATE}\n\n[dependencies]\n\"@m2/core\" = \"{}\"\nstd = \"{}\"\n\"@m2/drums\" = {{ path = {} }}\n",
        wanted(m2_script::SDK_VERSION),
        wanted(&version),
        serde_json::to_string(&drums.to_string_lossy()).expect("a string")
    );
    let files = [
        ("music2.toml", manifest),
        ("song.ts", SONG.to_string()),
        ("data/arrangement.json", ARRANGEMENT.to_string()),
        ("data/kit/notes.json", lane(&[(note, 0.0, 0.25, 0.8)], 4.0)),
    ];
    for (path, text) in files {
        let file = work.join(path);
        std::fs::create_dir_all(file.parent().expect("a file in the song"))?;
        std::fs::write(&file, text)?;
    }
    let project = Project::open(work).map_err(|d| anyhow!("{}", d.first().map_or("the kit's song can't be read", |d| &d.message)))?;
    let compiled =
        m2_session::compile(&project, &mut set).map_err(|d| anyhow!("{}", d.first().map_or("it doesn't build", |d| &d.message)))?;
    let wav = work.join("render.wav");
    Render::new(Arc::new(compiled), &set, RATE, 0.0, Length::Seconds(SECONDS), Threads::Exactly(1))?.write_float(&wav, |_| {})?;
    let mut reader = hound::WavReader::open(&wav)?;
    let channels = reader.spec().channels as usize;
    let all: Vec<f32> = reader.samples::<f32>().collect::<Result<_, _>>()?;
    // Mono as the drum is: both sides alike.
    let mono = all.chunks(channels).all(|f| f.iter().all(|s| *s == f[0]));
    let out_channels = if mono { 1 } else { channels };
    let samples: Vec<f32> = if mono { all.chunks(channels).map(|f| f[0]).collect() } else { all };
    let peak = samples.iter().fold(0f32, |p, s| p.max(s.abs())) as f64;
    if peak == 0.0 {
        bail!("note {note} made no sound");
    }
    let last = samples.iter().rposition(|s| s.abs() as f64 >= peak * FLOOR).expect("the peak is above the floor");
    let frames = last / out_channels + 1;
    let gain = PEAK / peak;
    let spec =
        hound::WavSpec { channels: out_channels as u16, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut bytes = Cursor::new(vec![]);
    let mut w = hound::WavWriter::new(&mut bytes, spec)?;
    for s in &samples[..frames * out_channels] {
        w.write_sample((*s as f64 * gain * 32767.0).round().clamp(-32768.0, 32767.0) as i16)?;
    }
    w.finalize()?;
    Ok(bytes.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    // LIB-8: std's kit is made again to the same bytes as the files std ships. With `M2_WRITE_KIT`
    // set (`std/samples/make.sh`) it writes them instead.
    #[test]
    fn std_kit_is_made_again_to_the_same_bytes() {
        let std = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
        let drums = m2_song::pack::source_packs().join("@m2/drums");
        let folder = std.join(FOLDER);
        let write = std::env::var_os("M2_WRITE_KIT").is_some();
        for (name, note) in KIT {
            let work = std::env::temp_dir().join(format!("m2-kit-{name}-{}", std::process::id()));
            let made = make(&std, &drums, note, &work).unwrap_or_else(|e| panic!("{name}: {e:#}"));
            let file = folder.join(format!("{name}.wav"));
            if write {
                std::fs::create_dir_all(&folder).unwrap();
                std::fs::write(&file, &made).unwrap();
                continue;
            }
            let shipped = std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}; make it with std/samples/make.sh", file.display()));
            assert!(made == shipped, "{name}: made again, it isn't the file std ships; make them with std/samples/make.sh");
        }
    }
}
