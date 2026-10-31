//! What the player's integration tests share: projects in temp folders, songs, and a device
//! driven by hand.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use m2_ir::{Flat, Ir};
use m2_play::{Driver, Player};
use m2_plugins::Owner;
use m2_runtime::audio::Cache;
use m2_runtime::{Basis, Continues, Keep, PackageSet, PlanInfo, Renderer};
use m2_song::{Arrangement, DataSet, LaneAddr, LaneData, LaneKind, Project};
use serde_json::{Value, json};

pub const SR: u32 = 48_000;

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn std_packages() -> PackageSet {
    // Plugins run their main-thread calls on the tests' dedicated thread (HOST-2, C1).
    m2_plugins::test_main_thread();
    m2_plugins::probe::install();
    let mut p = PackageSet::default().with_store(m2_song::pack::test_store());
    p.load(&repo().join("std")).expect("std loads");
    p
}

/// A fresh, empty temp folder.
pub fn temp(what: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("m2-play-{what}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().unwrap().is_dir() { copy(&src, &dst) } else { std::fs::copy(&src, &dst).map(drop).unwrap() }
    }
}

/// A compiled project: its graph, data, packages and rate.
pub struct Song {
    pub flat: Flat,
    pub data: DataSet,
    pub packages: PackageSet,
    pub rate: u32,
    pub dir: PathBuf,
}

impl Song {
    pub fn renderer(&self) -> Renderer {
        let mut r = Renderer::new(&self.flat, &self.data, &self.packages, self.rate).unwrap();
        r.set_strict(false);
        r
    }

    /// A plan of this song prepared against `old`'s, playing as `info` describes it, with what
    /// continues given (RENDER-25): what the live engine is sent for an edit.
    pub fn against(&self, old: &Song, info: &PlanInfo, cont: &Continues) -> Renderer {
        let restarts = std::collections::HashSet::new();
        let keep = Keep { old: &old.flat, old_data: &old.data, info, cont, restarts: &restarts };
        let mut r = Renderer::prepare(
            &self.flat,
            &self.data,
            &self.packages,
            self.rate,
            &mut Cache::default(),
            &Owner::discard(),
            Basis::Keep(keep),
        )
        .unwrap();
        r.set_strict(false);
        r
    }

    /// [`Song::against`] with the nodes matched by name (graphs written as IR).
    pub fn after(&self, old: &Song, info: &PlanInfo) -> Renderer {
        self.against(old, info, &Continues::named(&old.flat, &self.flat))
    }

    /// Its graph and rate, with `data` and packages of their own (its folder stays with this one).
    pub fn with_data(&self, data: DataSet) -> Song {
        Song { flat: self.flat.clone(), data, packages: std_packages(), rate: self.rate, dir: temp("parts") }
    }

    /// The whole song and a second past its last clip, offline, as it plays live.
    pub fn seconds(&self) -> f64 {
        self.data.tempo().seconds_at(self.data.end()) + 1.0
    }
}

impl Drop for Song {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Compile the project in `dir`, which the song then owns.
pub fn compile(dir: PathBuf) -> Song {
    compile_on(dir, None)
}

/// [`compile`], its plugins on `host` when given (the host process, HOST-3).
pub fn compile_on(dir: PathBuf, host: Option<std::sync::Arc<dyn m2_plugins::PluginHost>>) -> Song {
    let project = Project::open(&dir).unwrap_or_else(|d| panic!("{d:?}"));
    let mut packages = std_packages();
    if let Some(h) = host {
        packages = packages.with_host(h);
    }
    let c = m2_session::compile(&project, &mut packages).unwrap_or_else(|d| panic!("{d:?}"));
    Song { flat: c.flat.clone(), data: c.data, packages, rate: project.sample_rate(), dir }
}

/// A project in a temp folder: a manifest at 48 kHz that asks for std, and `files` (path, text),
/// compiled.
pub fn project(what: &str, files: &[(&str, &str)]) -> Song {
    project_on(what, files, None)
}

/// [`project`], its plugins on `host` when given.
pub fn project_on(what: &str, files: &[(&str, &str)], host: Option<std::sync::Arc<dyn m2_plugins::PluginHost>>) -> Song {
    let dir = temp(what);
    let manifest = format!(
        "[project]\nname = \"{what}\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.6\"\n\"@m2/drums\" = \"1\"\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"1\"\n"
    );
    for (rel, text) in [("music2.toml", manifest.as_str())].into_iter().chain(files.iter().copied()) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    compile_on(dir, host)
}

/// What `new` continues of `old`, as the session names it: both compiled from their folders.
pub fn continues(old: &Song, new: &Song) -> Continues {
    let read = |s: &Song| {
        let snapshot = m2_song::Snapshot::read(&s.dir).unwrap();
        let project = Project::from_snapshot(&snapshot).unwrap_or_else(|d| panic!("{d:?}"));
        (m2_session::compile(&project, &mut std_packages()).unwrap_or_else(|d| panic!("{d:?}")), snapshot)
    };
    let ((a, sa), (b, sb)) = (read(old), read(new));
    m2_session::tx::continues::between((&a, &sa), (&b, &sb))
}

/// first-song edited while it plays (8a's live half): the bass's duck becomes a drive (the duck
/// plays out the window as a ghost, the bass fader's input fades to the drive), and the keys'
/// channel plays the hats through a new delay (a fade-in), so the keys track plays out the window
/// as ghosts, which the song reads only through a fade's old side.
pub fn first_song_edited() -> Song {
    let mut song = std::fs::read_to_string(repo().join("examples/first-song/song.ts")).unwrap();
    for (from, to) in [
        ("import { duck } from \"std/fx\";", "import { delay, drive } from \"@m2/effects\";"),
        ("bass: duck(bass.out, kick.env, { depth: db(-9) }),", "bass: drive(bass.out, db(6)),"),
        ("keys: { in: keys.out,", "keys: { in: delay(hats.out),"),
    ] {
        assert!(song.contains(from), "first-song's song.ts has `{from}`");
        song = song.replace(from, to);
    }
    // It plays @m2/effects now: the song lists it, as adding one of its effects would (PACK-10).
    let manifest = std::fs::read_to_string(repo().join("examples/first-song/music2.toml")).unwrap() + "\"@m2/effects\" = \"1\"\n";
    fixture_with("examples/first-song", &[("song.ts", &song), ("music2.toml", &manifest)])
}

/// A copy of the repo's fixture `rel`, compiled.
pub fn fixture(rel: &str) -> Song {
    fixture_with(rel, &[])
}

/// A copy of the repo's fixture `rel` with `files` (path, text) written over it, compiled.
pub fn fixture_with(rel: &str, files: &[(&str, &str)]) -> Song {
    let dir = temp("fixture");
    copy(&repo().join(rel), &dir);
    for (path, text) in files {
        std::fs::create_dir_all(dir.join(path).parent().unwrap()).unwrap();
        std::fs::write(dir.join(path), text).unwrap();
    }
    compile(dir)
}

/// A song held in memory: one module's nodes and lanes, and its lanes' data.
pub fn song(nodes: Value, out: &str, lanes: Value, data: DataSet) -> Song {
    let ir: Ir = serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
        "modules": [{"key": "song", "lanes": lanes, "nodes": nodes, "exports": {"out": out}}]}))
    .unwrap();
    let packages = std_packages();
    let flat = m2_ir::check(&ir, &packages).unwrap_or_else(|e| panic!("{e:?}"));
    Song { flat, data, packages, rate: SR, dir: temp("song") }
}

pub fn notes(v: Value) -> LaneData {
    LaneData::read(&v.to_string(), &LaneKind::Notes).unwrap()
}

pub fn at_bpm(bpm: f64) -> Arrangement {
    serde_json::from_value(json!({"tempo": [{"beat": 0, "bpm": bpm}]})).unwrap()
}

pub fn addr(lane: &str) -> LaneAddr {
    LaneAddr::new("song", lane).unwrap()
}

/// Sine voices at their notes' pitches, through an envelope releasing over `release` seconds,
/// then `after` (reading the poly as `v`), out of `out`.
pub fn synth(release: f64, after: Value, out: &str, notes_json: Value, arr: Arrangement) -> Song {
    let body = json!([
        {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
        {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
        {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
        {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g", "sustain": "s", "release": "r"}},
        {"id": "s", "op": "const", "value": 1},
        {"id": "r", "op": "const", "value": release, "unit": "seconds"},
        {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "e"}}]);
    let mut nodes = vec![
        json!({"id": "n", "op": "lane", "lane": "notes"}),
        json!({"id": "v", "op": "poly", "events": "n", "voices": 8, "body": {"out": "amp", "nodes": body}}),
    ];
    nodes.extend(after.as_array().unwrap().iter().cloned());
    let data = DataSet::with_lanes(arr, [(addr("notes"), notes(notes_json))]);
    song(Value::Array(nodes), out, json!([{"key": "notes", "kind": "events<note>"}]), data)
}

/// Device buffer sizes, in turn: odd ones, and ones bigger than a block.
pub const SIZES: [usize; 7] = [1, 7, 128, 300, 64, 1000, 4096];

/// Fill `frames` frames through the driver in buffers of [`SIZES`], stereo, calling `between`
/// before each buffer with the frames done so far.
pub fn hear(d: &mut Driver, frames: usize, between: &mut dyn FnMut(&mut Driver, usize)) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(frames);
    let mut data = vec![0.0f32; 2 * 4096];
    for n in SIZES.iter().cycle() {
        let n = (*n).min(frames - out.len());
        if n == 0 {
            break;
        }
        between(d, out.len());
        d.fill(&mut data[..2 * n], 2);
        out.extend(data[..2 * n].chunks(2).map(|f| [f[0], f[1]]));
        drop(d.drain_retired());
    }
    out
}

/// A player driven by hand, playing `r` from 0: opened and started before the first fill.
pub fn playing(r: Renderer, device_rate: u32) -> (Player, Driver) {
    let (player, d) = Player::driven(device_rate);
    player.open(r).unwrap();
    player.play();
    (player, d)
}

pub fn peak(x: &[[f32; 2]]) -> f32 {
    x.iter().fold(0.0, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

pub fn rms(x: &[[f32; 2]]) -> f32 {
    (x.iter().map(|f| 0.5 * (f[0] * f[0] + f[1] * f[1])).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// The largest step from one sample to the next.
pub fn max_step(x: &[[f32; 2]]) -> f32 {
    x.windows(2).map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs())).fold(0.0, f32::max)
}

pub fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

/// The first frame where `a` and `b` differ, comparing bits.
pub fn first_diff(a: &[[f32; 2]], b: &[[f32; 2]]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| x[0].to_bits() != y[0].to_bits() || x[1].to_bits() != y[1].to_bits())
}

/// A small deterministic generator, for "random" choices that reproduce.
pub fn lcg(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed >> 33
}

/// A stereo 16-bit WAV of `seconds` at 48 kHz, written fast: frame `i` holds `i` (mod 65536) on
/// the left and its negative on the right, so where a sample came from can be told (RENDER-30).
pub fn long_wav(path: &Path, seconds: f64) {
    let frames = (seconds * SR as f64) as usize;
    let data = (frames * 4) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    for part in [&b"RIFF"[..], &(36 + data).to_le_bytes(), b"WAVEfmt ", &16u32.to_le_bytes(), &1u16.to_le_bytes(), &2u16.to_le_bytes()] {
        b.extend_from_slice(part);
    }
    for part in [&SR.to_le_bytes()[..], &(SR * 4).to_le_bytes(), &4u16.to_le_bytes(), &16u16.to_le_bytes(), b"data", &data.to_le_bytes()] {
        b.extend_from_slice(part);
    }
    for i in 0..frames {
        let x = (i % 65536) as u16 as i16;
        b.extend_from_slice(&x.to_le_bytes());
        b.extend_from_slice(&x.wrapping_neg().to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

/// A song at `bpm` that plays one clip of a [`long_wav`] of `seconds` from beat 0, on a stereo audio
/// lane `clips`: a lane that streams (RENDER-30).
pub fn long_audio(seconds: f64, bpm: f64) -> Song {
    long_audio_clip(seconds, bpm, json!({"start": 0, "length": seconds * bpm / 60.0, "asset": "long.wav"}))
}

/// [`long_audio`]'s file, in a song at `bpm` whose one clip is `clip`.
pub fn long_audio_clip(seconds: f64, bpm: f64, clip: Value) -> Song {
    let dir = temp("long-audio");
    let files = [
        ("music2.toml", "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\n".to_string()),
        ("song.ts", String::new()),
        ("data/arrangement.json", json!({"tempo": [{"beat": 0, "bpm": bpm}]}).to_string()),
        ("data/song/clips.json", json!({"clips": [clip]}).to_string()),
    ];
    for (name, text) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    long_wav(&dir.join("assets/long.wav"), seconds);
    let ir: Ir = serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
        "modules": [{"key": "song", "nodes": [{"id": "out", "op": "lane", "lane": "clips"}], "exports": {"out": "out"},
            "lanes": [{"key": "clips", "kind": "audio<2>"}]}]}))
    .unwrap();
    let packages = std_packages();
    let flat = m2_ir::check(&ir, &packages).unwrap_or_else(|e| panic!("{e:?}"));
    let (data, diags): (DataSet, _) = Project::open(&dir).unwrap().data(&m2_runtime::decls(&ir), None);
    assert!(diags.iter().all(|d| !d.code.starts_with("lane.") && d.code != "asset.missing"), "{diags:?}");
    Song { flat, data, packages, rate: SR, dir }
}
