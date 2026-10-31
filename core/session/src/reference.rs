//! The reference projects p16 and p50, which step 3's timing gates are measured on (contract,
//! Acceptance gates): `tracks` tracks cycling std `bass` with `delay`, `pluck`, and `pad` with
//! `reverb`, each playing `examples/first-song`'s notes repeated `repeats` times, at `bpm`.
//! With `stems`, that many one-minute stereo WAVs also play on audio lanes, every second one
//! warped: the audio-heavy variant.
//!
//! Tests and gate runs generate them into folders of their own; they are never checked in.
//! `cargo run -p m2-server --example reference -- <dir> --tracks 16` writes one by hand.

use std::path::Path;

use anyhow::{Context, Result, bail};
use m2_song::lane::{self, LaneData, LaneKind};
use m2_song::{Arrangement, AudioClip, ClipId, LaneAddr, NoteClip, Sha, Tempo};

/// first-song's bass line and chords: 32 beats each.
const BASS: &str = include_str!("../../../examples/first-song/data/bass/notes.json");
const CHORDS: &str = include_str!("../../../examples/first-song/data/keys/chords.json");

/// A stem's length, and where its clips start: every 128 beats (32 bars).
const STEM_SECONDS: u32 = 60;
const STEM_EVERY: f64 = 128.0;
/// A stem clip's length in beats: under a minute at 127 bpm, so an unwarped clip never runs past
/// its file.
const STEM_BEATS: f64 = 120.0;
const STEM_RATE: u32 = 48_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reference {
    pub tracks: usize,
    pub bpm: f64,
    /// How many times first-song's 32-beat clips repeat: 16 is 512 beats, 4:02 at 127 bpm.
    pub repeats: u32,
    /// One-minute WAVs on audio lanes of their own (0 for none).
    pub stems: usize,
}

impl Reference {
    pub const P16: Reference = Reference { tracks: 16, bpm: 127.0, repeats: 16, stems: 0 };
    pub const P50: Reference = Reference { tracks: 50, bpm: 127.0, repeats: 16, stems: 0 };

    /// `p16`, or `p16-stems4` for the audio-heavy variant.
    pub fn name(&self) -> String {
        match self.stems {
            0 => format!("p{}", self.tracks),
            n => format!("p{}-stems{n}", self.tracks),
        }
    }

    /// The beat the last clip ends on.
    pub fn end(&self) -> f64 {
        32.0 * self.repeats as f64
    }
}

/// Write the project `r` into `dir`, which must be missing or empty: it never replaces files.
pub fn write(dir: &Path, r: &Reference) -> Result<()> {
    if !(1..=99).contains(&r.tracks) || r.stems > 99 || r.repeats == 0 {
        bail!("a reference project has 1 to 99 tracks, at most 99 stems and at least one repeat");
    }
    let arrangement = Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: r.bpm }], meter: vec![] };
    Arrangement::read(&arrangement.canonical()).map_err(|d| anyhow::anyhow!("{d}"))?;
    if dir.read_dir().is_ok_and(|mut d| d.next().is_some()) {
        bail!("{} isn't empty: a reference project is written into a new folder", dir.display());
    }
    let put = |rel: &str, bytes: &[u8]| -> Result<()> {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("inside dir")).with_context(|| format!("creating {}", p.display()))?;
        std::fs::write(&p, bytes).with_context(|| format!("writing {}", p.display()))
    };
    let name = r.name();
    put(
        "music2.toml",
        format!("[project]\nname = \"{name}\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.7\"\n\"@m2/drums\" = \"1\"\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"1\"\n")
            .as_bytes(),
    )?;
    put(".gitignore", b".music2/\n")?;
    put("data/arrangement.json", arrangement.canonical().as_bytes())?;

    let (bass, chords) = (notes(BASS)?, notes(CHORDS)?);
    let mut imports = String::new();
    let mut channels = vec![];
    for i in 0..r.tracks {
        let key = format!("t{:02}", i + 1);
        let (body, src) = match i % 3 {
            0 => ("delay(bass(lane.notes(\"notes\")))", &bass),
            1 => ("pluck(lane.notes(\"notes\"))", &chords),
            _ => ("reverb(pad(lane.notes(\"notes\")))", &chords),
        };
        put(
            &format!("tracks/{key}.ts"),
            format!(
                "import {{ track }} from \"@m2/core\";\nimport {{ delay, reverb }} from \"@m2/effects\";\nimport {{ bass, pad, pluck }} from \"@m2/synths\";\n\n\
                 export default track(\"{key}\", ({{ lane }}) => {body});\n"
            )
            .as_bytes(),
        )?;
        imports.push_str(&format!("import {key} from \"./tracks/{key}\";\n"));
        channels.push(format!("      {key}: {{ in: {key}.out, db: db(-12) }},"));
        let addr = LaneAddr::new(&key, "notes").map_err(anyhow::Error::msg)?;
        put(&format!("data/{key}/notes.json"), lane::canonical(&LaneData::Notes(repeated(src, r.repeats, &addr))).as_bytes())?;
    }
    for i in 0..r.stems {
        let key = format!("s{:02}", i + 1);
        let asset = format!("stem-{:02}.wav", i + 1);
        let wav = stem(i as u32)?;
        let pin = Sha::of(&wav).pin();
        put(&format!("assets/{asset}"), &wav)?;
        put(
            &format!("tracks/{key}.ts"),
            format!("import {{ track }} from \"@m2/core\";\n\nexport default track(\"{key}\", ({{ lane }}) => lane.audio(\"clips\"));\n")
                .as_bytes(),
        )?;
        imports.push_str(&format!("import {key} from \"./tracks/{key}\";\n"));
        channels.push(format!("      {key}: {{ in: {key}.out, db: db(-18) }},"));
        let addr = LaneAddr::new(&key, "clips").map_err(anyhow::Error::msg)?;
        let clips = (0..)
            .map(|n| n as f64 * STEM_EVERY)
            .take_while(|start| *start < r.end())
            .enumerate()
            .map(|(n, start)| AudioClip {
                id: Some(ClipId::derive("reference", &addr, n as u32)),
                other_id: None,
                start,
                length: STEM_BEATS,
                asset: asset.clone(),
                hash: Some(pin.clone()),
                offset: 0.0,
                gain_db: 0.0,
                fade_in: 0.01,
                fade_out: 0.01,
                reverse: false,
                pitch: 0.0,
                // Every second stem is stretched a little, so opening it warps (RENDER-27).
                warp: if i % 2 == 1 { vec![(0.0, 0.0), (54.0, STEM_BEATS)] } else { vec![] },
            })
            .collect();
        put(&format!("data/{key}/clips.json"), lane::canonical(&LaneData::Audio(clips)).as_bytes())?;
    }
    put(
        "song.ts",
        format!(
            "import {{ db, song }} from \"@m2/core\";\nimport {{ master, mixer }} from \"std/mix\";\n{imports}\n\
             export default song(() =>\n  master(\n    mixer({{\n{}\n    }}),\n  ),\n);\n",
            channels.join("\n")
        )
        .as_bytes(),
    )?;
    Ok(())
}

fn notes(text: &str) -> Result<Vec<NoteClip>> {
    match LaneData::read(text, &LaneKind::Notes) {
        Ok(LaneData::Notes(clips)) => Ok(clips),
        _ => bail!("first-song's notes don't read as a notes lane"),
    }
}

/// `src` laid end to end `repeats` times, each clip with an id of its own.
fn repeated(src: &[NoteClip], repeats: u32, addr: &LaneAddr) -> Vec<NoteClip> {
    let period = src.iter().map(|c| c.start + c.length).fold(0.0, f64::max);
    let mut out = vec![];
    for k in 0..repeats {
        for c in src {
            let mut c = c.clone();
            c.start += period * k as f64;
            c.id = Some(ClipId::derive("reference", addr, out.len() as u32));
            out.push(c);
        }
    }
    out
}

/// Stem `n`: a minute of 16-bit stereo, a saw a fifth apart per stem plus noise from a fixed
/// seed, so every generated project has the same bytes.
fn stem(n: u32) -> Result<Vec<u8>> {
    let spec = hound::WavSpec { channels: 2, sample_rate: STEM_RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut bytes = std::io::Cursor::new(vec![]);
    let mut w = hound::WavWriter::new(&mut bytes, spec)?;
    let hz = 55.0 * 1.5f64.powi(n as i32 % 4);
    let mut seed = 0x9e37_79b9u32.wrapping_mul(n + 1);
    let mut noise = || {
        // xorshift32: fixed, so the bytes never change.
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f64 / u32::MAX as f64 * 2.0 - 1.0
    };
    for i in 0..STEM_SECONDS * STEM_RATE {
        let saw = (i as f64 * hz / STEM_RATE as f64).fract() * 2.0 - 1.0;
        for _ in 0..2 {
            w.write_sample((((0.3 * saw + 0.1 * noise()) * 32_767.0).round()) as i16)?;
        }
    }
    w.finalize()?;
    Ok(bytes.into_inner())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use m2_runtime::PackageSet;
    use m2_song::Project;

    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new(tag: &str) -> Temp {
            Temp(std::env::temp_dir().join(format!("m2-reference-{tag}-{}", std::process::id())))
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn compile(dir: &Path) -> crate::Compiled {
        let mut pk = PackageSet::default().with_store(m2_song::pack::test_store());
        pk.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        crate::compile(&Project::open(dir).unwrap(), &mut pk).unwrap_or_else(|e| panic!("{e:?}"))
    }

    // Acceptance gates: p16 is 16 tracks of first-song's notes repeated to 4:02 at 127 bpm, and it
    // compiles with nothing to report.
    #[test]
    fn p16_compiles_cleanly_and_plays_four_minutes() {
        let t = Temp::new("p16");
        write(&t.0, &Reference::P16).unwrap();
        let c = compile(&t.0);
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.data.end(), 512.0);
        let seconds = c.data.tempo().seconds_at(c.data.end());
        assert!((seconds - 241.9).abs() < 0.1, "{seconds}");
        assert_eq!(c.data.notes("t02", "notes").len(), 16 * 72);
        assert!(c.data.lane("t16", "notes").is_some());
        // Never written over.
        assert!(write(&t.0, &Reference::P16).is_err());
    }

    #[test]
    fn the_stems_variant_pins_its_assets_and_writes_the_same_bytes_every_time() {
        let small = Reference { tracks: 3, bpm: 127.0, repeats: 8, stems: 2 };
        let (a, b) = (Temp::new("stems-a"), Temp::new("stems-b"));
        write(&a.0, &small).unwrap();
        write(&b.0, &small).unwrap();
        let c = compile(&a.0);
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        let clips = c.data.audio("s02", "clips");
        assert_eq!(clips.len(), 2);
        assert!(clips.iter().all(|c| !c.warp.is_empty()) && c.data.audio("s01", "clips").iter().all(|c| c.warp.is_empty()));
        for f in ["assets/stem-01.wav", "data/s02/clips.json", "data/t01/notes.json", "song.ts"] {
            assert_eq!(std::fs::read(a.0.join(f)).unwrap(), std::fs::read(b.0.join(f)).unwrap(), "{f}");
        }
    }
}
