//! Bouncing: a track, or the whole song, rendered to a new file in the song's `assets/`, with its
//! provenance beside it: `m2 bounce`, and the app's Bounce to audio (RENDER-40), one code path.
//!
//! [`Bounce`] plans it on one revision, [`Bounce::render`] makes its renderer (cheap from the
//! song's start, so the app makes it on its actor) and [`Bounce::write`] renders it into the file,
//! anywhere. A track's bounce is its `out` export alone, before its mixer channel: what a stem is
//! (C3), the same bits (RENDER-2).
//!
//! [`in_place`] then puts a track's bounce in the song, as one Tx: an audio track right after it,
//! playing the file from beat 0 in a copy of its channel, and the track muted in the song, so the
//! song sounds the same.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use m2_ir::{FOp, Root};
use m2_runtime::PackageSet;
use m2_session::{Compiled, Revision, Session};
use m2_song::{AssetName, DataPath, ROOT, Sha};
use serde_json::json;

use crate::mix::{Mixing, mixer};
use crate::render::{Length, Parts, Render, Rendered, Threads, bounce_seconds, rooted};
use crate::tracks::{Made, NewTrack, Play};
use crate::{EditError, Edited};

/// One bounce, planned on one revision: what it plays, from where, for how long, and what its
/// provenance records.
pub struct Bounce {
    song: Arc<Parts>,
    /// The module and export it plays: the song's own root, or a track's export.
    pub bounced: Root,
    pub from: f64,
    pub seconds: f64,
    pub sample_rate: u32,
    /// The revision it was planned on, when a session's: a change after it refuses
    /// [`in_place`].
    revision: Option<u64>,
    /// Every package file the build and the runtime read, by package (RENDER-2).
    packages: serde_json::Value,
}

/// What [`Bounce::write`] wrote.
#[derive(Clone, Debug)]
pub struct Bounced {
    pub asset: AssetName,
    /// The file, and its provenance beside it.
    pub path: PathBuf,
    pub record: PathBuf,
    /// The file's hash, as a clip pins it.
    pub hash: Sha,
    pub seconds: f64,
    pub rendered: Rendered,
    /// The channels of the sound bounced: a mono one is written as two equal channels.
    pub channels: u8,
    revision: Option<u64>,
}

impl Bounced {
    /// Take the files back: nothing names them (a Tx that would have was refused). An asset is
    /// never replaced (FMT-14), but one this bounce made and nothing reads is no one's yet.
    pub fn remove(&self) {
        let _ = std::fs::remove_file(&self.record);
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Bounce {
    /// Bounce song `c` with `packages`: export `track.1` of track `track.0` alone (a track by its
    /// key, ID-11), else the whole song; from `from` seconds (rendered from 0, RENDER-1) for
    /// `seconds`, else to 2 s past the song's last clip.
    pub fn plan(
        c: &Compiled,
        packages: &PackageSet,
        track: Option<(&str, &str)>,
        from: f64,
        seconds: Option<f64>,
        sample_rate: u32,
    ) -> Result<Bounce> {
        let mut ir = c.ir.clone();
        let mut bounced = ir.root.clone();
        if let Some((track, export)) = track {
            if !ir.modules.iter().any(|m| m.key == track) {
                bail!("--track takes a track's key, and there's no track `{track}`: the tracks are {}", tracks(&ir));
            }
            bounced = Root { module: track.to_string(), export: export.to_string() };
            // Played through an export of the root module, which stays `song` (ID-3): as a stem is.
            ir = rooted(&ir, track, export)?;
        }
        let song = Arc::new(Parts::of(c, ir, packages).map_err(|d| {
            let first = d.first().map_or_else(String::new, |d| format!(": {}", d.message));
            anyhow!("the bounce doesn't build{first}")
        })?);
        let seconds = seconds.unwrap_or_else(|| bounce_seconds(&c.data) - from);
        Ok(Bounce { song, bounced, from, seconds, sample_rate, revision: None, packages: provenance(c, packages) })
    }

    /// The bounce Bounce to audio makes of track `track` of `rev` (RENDER-40): its `out`, from 0,
    /// once [`check`] says it can go in place. Refused in words before anything is rendered.
    pub fn in_place_of(rev: &Revision, packages: &PackageSet, track: &str) -> Result<Bounce, EditError> {
        check(rev, track)?;
        let mut b = Bounce::plan(&rev.compiled, packages, Some((track, "out")), 0.0, None, rev.project.sample_rate())
            .map_err(|e| EditError::Rejected(format!("{e:#}")))?;
        let label = label(rev, track);
        let plugins = plugins_of(&b.song.flat, packages);
        if !plugins.is_empty() {
            return Err(EditError::Rejected(format!(
                "{label} plays {}: bouncing a track that plays a plugin isn't built yet. Export its stem from another music app, or \
                 use the plugin's own freeze.",
                crate::listed(&plugins)
            )));
        }
        b.revision = Some(rev.number);
        Ok(b)
    }

    /// Where `assets/<asset>` and its provenance go in the project at `project`; refused when either
    /// is there, so a caller asks before it renders. An asset is never overwritten (FMT-14): a new
    /// version gets a new name. Nor is a provenance, even one whose asset is gone.
    pub fn free(project: &Path, asset: &AssetName) -> Result<(PathBuf, PathBuf)> {
        let record = AssetName::new(&format!("{asset}.json")).map_err(|e| anyhow!("{e}"))?;
        let path = project.join(DataPath::Asset(asset.clone()).path());
        let record = project.join(DataPath::Asset(record).path());
        for p in [&path, &record] {
            if std::fs::symlink_metadata(p).is_ok() {
                bail!("{} exists, and assets are never replaced: bounce to a new name", p.display());
            }
        }
        Ok((path, record))
    }

    /// Whether the same files render the same bits again: false when it plays a plugin (RENDER-2).
    pub fn reproducible(&self) -> bool {
        crate::render::pure(&self.song.flat)
    }

    /// Its renderer, on `threads`, at its start: exact (RENDER-1), with the packages it was built
    /// with (PACK-9). From the song's start it renders nothing yet; from later it replays up to there.
    pub fn render(&self, packages: &PackageSet, threads: Threads) -> Result<Render> {
        Render::new(self.song.clone(), packages, self.sample_rate, self.from, Length::Seconds(self.seconds), threads)
    }

    /// Render it with `render` (made by [`Bounce::render`], stopped when its stop is set) into the
    /// project at `project`'s new `assets/<asset>`, and its provenance beside it as
    /// `<asset>.json`: 32-bit float, every sample as rendered (D15). Neither is ever written over a
    /// file there (FMT-14), and both are refused before anything is rendered when one is. When the
    /// provenance can't be written the file goes too, so the name stays free for a retry.
    /// `progress` hears how far it is (0..1).
    pub fn write(&self, render: Render, project: &Path, asset: &AssetName, progress: impl FnMut(f64)) -> Result<Bounced> {
        let (path, record) = Bounce::free(project, asset)?;
        let folder = path.parent().expect("inside assets/").to_path_buf();
        // `assets/`, and `vox/` in it for `vox/keys`, each synced in the one above as it is made.
        m2_os::fs::make_dirs(&folder)?;
        let rendered = render.write_float(&path, progress)?;
        let provenance = json!({
            "bounced_from": {"module": self.bounced.module, "export": self.bounced.export},
            "from_seconds": self.from, "seconds": self.seconds, "sample_rate": self.sample_rate,
            "reproducible": self.reproducible(),
            "packages": self.packages,
        });
        // Created only if it isn't there, as the asset is. Its bytes are pushed, and its folder
        // synced, before a Tx names the asset (WRITE-1).
        let written = serde_json::to_string_pretty(&provenance).map_err(anyhow::Error::from).and_then(|text| {
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&record)?;
            std::io::Write::write_all(&mut f, (text + "\n").as_bytes())
                .and_then(|()| m2_os::fs::push(&f))
                .and_then(|()| m2_os::fs::sync_dir(&folder))
                .inspect_err(|_| drop(std::fs::remove_file(&record)))?;
            Ok(())
        });
        if let Err(e) = written {
            let _ = std::fs::remove_file(&path);
            return Err(e.context(format!("writing {}", record.display())));
        }
        let hash = Sha::of_file(&path)?;
        let channels = self.song.flat.out_kind.channels();
        Ok(Bounced { asset: asset.clone(), path, record, hash, seconds: self.seconds, rendered, channels, revision: self.revision })
    }
}

/// Every package file the build and the runtime read, by content (RENDER-2): its code, its
/// manifest and wasm, the files its kernels read (a pack's recordings, PACK-5), and a pack's
/// version hash (PACK-6), with each package's version.
fn provenance(c: &Compiled, pk: &PackageSet) -> serde_json::Value {
    let read: Vec<(String, String)> = c
        .data
        .kernel_assets()
        .iter()
        .filter_map(|(name, sha)| m2_song::pack::split_id(name).map(|(pkg, path)| (format!("{pkg}/{path}"), sha.hex())))
        .collect();
    let packages: std::collections::BTreeMap<&str, serde_json::Value> = pk
        .packages()
        .map(|p| {
            // A scoped name has a `/` of its own (PACK-1): the file is the package's when its path starts with it.
            let ours = |path: &str| path.strip_prefix(p.name.as_str()).is_some_and(|rest| rest.starts_with('/'));
            let files: std::collections::BTreeMap<&str, &str> = c
                .package_files
                .iter()
                .filter(|f| ours(&f.path))
                .map(|f| (f.path.as_str(), f.sha256.as_str()))
                .chain(p.files.iter().map(|(path, sha)| (path.as_str(), sha.as_str())))
                .chain(read.iter().filter(|(path, _)| ours(path)).map(|(path, sha)| (path.as_str(), sha.as_str())))
                .collect();
            let mut entry = json!({"version": p.version, "files": files});
            if let Some(hash) = c.packs.iter().find(|r| r.name.as_str() == p.name).and_then(|r| r.hash) {
                entry["hash"] = json!(hash.prefixed());
            }
            (p.name.as_str(), entry)
        })
        .collect();
    json!(packages)
}

/// The tracks, by key, with their labels where they differ: `kick, chops (Chops)`.
pub fn tracks(ir: &m2_ir::Ir) -> String {
    let named = |m: &m2_ir::Module| if m.label() == m.key { m.key.clone() } else { format!("{} ({})", m.key, m.label()) };
    ir.modules.iter().filter(|m| m.track).map(named).collect::<Vec<_>>().join(", ")
}

/// The names of the plugins `flat` plays.
fn plugins_of(flat: &m2_ir::Flat, packages: &PackageSet) -> Vec<String> {
    let mut names: Vec<String> = vec![];
    for n in &flat.nodes {
        if let FOp::Plugin { plugin, .. } = &n.op {
            let id = || plugin.strip_prefix("clap:").unwrap_or(plugin).to_string();
            let name = packages.plugin_info(plugin).map_or_else(id, |i| i.found.name.clone());
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// A module's label, or its key when there is no such module.
fn label(rev: &Revision, key: &str) -> String {
    rev.compiled.ir.modules.iter().find(|m| m.key == key).map_or_else(|| key.to_string(), |m| m.label().to_string())
}

/// Whether track `track` of `rev` can be bounced in place (RENDER-40), in words when not: a track,
/// not muted in the song, in a mixer channel of its own that reads its `out` by name (`keys.out`),
/// and its sound read nowhere else, so muting it silences only what the bounce plays.
pub fn check(rev: &Revision, track: &str) -> Result<(), EditError> {
    let no = |why: String| Err(EditError::Rejected(why));
    let Some(m) = rev.compiled.ir.modules.iter().find(|m| m.key == track) else {
        return Err(EditError::Missing(format!("There's no track `{track}`.")));
    };
    let label = m.label();
    if !m.track {
        return no(format!("{track} isn't a track, so it can't be bounced to audio."));
    }
    if m.muted {
        return no(format!("{label} is muted in the song, so its bounce would be silent. Unmute it in the song first."));
    }
    if !m.exports.contains_key("out") {
        return no(format!("{label} has no sound out to bounce."));
    }
    let Mixing::One(mixer) = mixer(rev) else {
        return no("The song has no one mixer to give the bounce a channel in.".into());
    };
    let mine: Vec<&crate::mix::Channel> = mixer.channels.iter().filter(|c| c.tracks.iter().any(|t| t == track)).collect();
    let c = match mine[..] {
        [] => return no(format!("{label} doesn't go into the mixer, so there's no channel for its bounce to copy.")),
        [c] => c,
        _ => return no(format!("{label} goes into the mixer in more than one channel, so its bounce can't replace it.")),
    };
    if c.tracks.len() > 1 {
        let others: Vec<String> = c.tracks.iter().filter(|t| *t != track).map(|t| label_of(rev, t)).collect();
        return no(format!("{label} shares its mixer channel with {}, so its bounce can't have a copy of it.", crate::listed(&others)));
    }
    let reads: Vec<&crate::mix::ExportRead> = c.reads.iter().filter(|r| r.module == track && !r.sidechain).collect();
    if reads.is_empty() {
        return no(format!("{label}'s mixer channel doesn't read it by name ({track}.out), so music2 can't copy it for the bounce."));
    }
    if let Some(r) = reads.iter().find(|r| r.export != "out") {
        return no(format!("{label}'s mixer channel reads its {} as well, which the bounce doesn't hold.", r.export));
    }
    // Muting it silences every place its sound goes; the bounce replaces only its channel. A
    // sidechain only listens (MON-2), and a mute never silences that.
    if let Some(o) = mixer.channels.iter().find(|o| o.key != c.key && o.reads.iter().any(|r| r.module == track && !r.sidechain && r.audio))
    {
        return no(format!("{label}'s sound goes into the {} channel too, which muting it would silence.", o.key));
    }
    let plays_it = |o: &m2_ir::Module| {
        o.nodes.iter().any(|n| matches!(&n.op, m2_ir::Op::Import { module, export } if module == track && export == "out"))
    };
    if let Some(o) = rev.compiled.ir.modules.iter().find(|o| o.key != ROOT && o.key != track && plays_it(o)) {
        return no(format!("{} plays {label}'s sound, which muting it would silence.", o.label()));
    }
    Ok(())
}

/// A module's label, or its key when there is no such module.
fn label_of(rev: &Revision, key: &str) -> String {
    label(rev, key)
}

/// What [`in_place`] did: the track it added, and the step.
#[derive(Clone, Debug)]
pub struct InPlace {
    pub made: Made,
    pub edited: Edited,
}

/// Put `bounced`, the bounce of track `track` ([`Bounce::in_place_of`]), in the song (RENDER-40):
/// one Tx, one undo step, that adds "Keys audio" right after the track, playing the file from beat
/// 0 at its own length in a copy of the track's channel, and mutes the track in the song. Refused
/// when the song changed since the bounce was planned (it may sound different now), and as
/// [`check`] refuses. The file stays either way: the caller removes it when nothing names it.
pub fn in_place(s: &mut Session, track: &str, bounced: &Bounced) -> Result<InPlace, EditError> {
    crate::ready(s)?;
    if bounced.revision.is_some_and(|r| r != s.accepted().number) {
        return Err(EditError::Rejected("The song changed while it bounced, so nothing was added. Bounce it again.".into()));
    }
    check(s.accepted(), track)?;
    crate::import::take_in_assets(s, std::iter::once(&bounced.asset))?;
    let rev = s.accepted();
    let tempo = rev.compiled.data.tempo();
    let length = tempo.beat_at(tempo.seconds_at(0.0) + bounced.seconds);
    let clip = m2_song::AudioClip {
        id: None,
        other_id: None,
        start: 0.0,
        length: (length * 1e6).round() / 1e6,
        asset: bounced.asset.to_string(),
        hash: Some(bounced.hash.pin()),
        offset: 0.0,
        gain_db: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
        reverse: false,
        pitch: 0.0,
        warp: vec![],
    };
    let clip = serde_json::to_value(&clip).map_err(|e| EditError::Io(e.to_string()))?;
    let new = NewTrack {
        play: Play::Audio,
        label: Some(format!("{} audio", label(rev, track))),
        clips: vec![clip],
        like: Some(track.to_string()),
        mono: bounced.channels == 1,
        ..NewTrack::default()
    };
    let made = crate::tracks::add_tracks_as(s, &[new], Some(track), None, None, &[track.to_string()])?.remove(0);
    Ok(InPlace { edited: made.edited.clone(), made })
}

#[cfg(test)]
#[path = "bounce_tests.rs"]
mod tests;
