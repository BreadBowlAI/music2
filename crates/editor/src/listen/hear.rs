//! What an agent's listen hears (AGENT-7): which buffers of the song its copy taps read, the range
//! render that copies them, and what it says, as JSON and in words.
//!
//! - **A track's own sound**: the overlay site where its audio enters the song's own module, where
//!   std's mixer is (else its first site, MON-1): before its channel, as a stem is.
//! - **As it reaches the mix**: its channel's fader ([`crate::mix::fader`]), after its pan, when the
//!   channel is the track's alone. A track in no channel, in two, in one with another track, or in
//!   a song without one std mixer, has none, and says why: its masking isn't measured.
//! - **The render**: a range render (RENDER-4), from `preroll` before the range, its copy taps from
//!   the range's first frame on. Each tap's samples are read where they carry the range's song time
//!   (RENDER-19), into a [`Meter`] of their own: the mix with its bands and stereo, each track's own
//!   sound with its loudness and peaks, and each track as heard with its bands and stereo too.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use m2_runtime::{BLOCK, Copies, PackageSet, Renderer, TapAt};
use m2_session::Revision;
use m2_song::ROOT;
use serde_json::{Map, Value, json};

use super::{Masked, Measures, Meter, OCTAVES, Picture, Wants, masking};
use crate::faults::{Namer, Song};
use crate::mix::{self, Mixing};
use crate::render::{Threads, pure, threads_for};

/// Frames rendered between reads of the copies.
const CHUNK: usize = 4096;

/// Where one track is read.
#[derive(Clone, Debug, PartialEq)]
pub struct Tapped {
    pub key: String,
    pub label: String,
    /// The overlay site where its own sound enters the mixer's module: None when nothing in the
    /// song reads it.
    pub own: Option<usize>,
    /// Its channel, and its fader's node, when the channel is its own.
    pub strip: Option<(String, usize)>,
    /// Why it has no strip of its own, when it hasn't.
    pub why: Option<String>,
}

/// Where each track of `rev` is read, in the song's order.
pub fn tapped(rev: &Revision) -> Vec<Tapped> {
    let flat = &rev.compiled.flat;
    let mixing = mix::mixer(rev);
    let modules = rev.compiled.ir.modules.iter().filter(|m| m.track);
    modules
        .map(|m| {
            let key = m.key.clone();
            let sites: Vec<usize> = (0..flat.overlays.len()).filter(|&k| flat.overlays[k].module == key).collect();
            let own = sites.iter().copied().find(|&k| flat.overlays[k].into == ROOT).or(sites.first().copied());
            let (strip, why) = match &mixing {
                _ if own.is_none() => (None, Some("nothing in the song reads it".to_string())),
                Mixing::One(mx) => {
                    let ins: Vec<&mix::Channel> = mx.channels.iter().filter(|c| c.tracks.contains(&key)).collect();
                    match ins[..] {
                        [] => (None, Some("it isn't in the mixer".to_string())),
                        [c] if c.tracks.len() > 1 => {
                            let others: Vec<&str> = c.tracks.iter().filter(|t| **t != key).map(String::as_str).collect();
                            (None, Some(format!("it shares channel {} with {}", c.key, others.join(", "))))
                        }
                        [c] => match mix::fader(rev, mx, &c.key) {
                            Some(n) => (Some((c.key.clone(), n)), None),
                            None => (None, Some(format!("channel {}'s fader wasn't found", c.key))),
                        },
                        _ => (None, Some(format!("it is in {} channels", ins.len()))),
                    }
                }
                Mixing::None => (None, Some("the song doesn't use std's mixer".to_string())),
                Mixing::Several(n) => (None, Some(format!("the song has {n} mixers"))),
                Mixing::Unwritten => (None, Some("the song's mixer isn't one object written in its code".to_string())),
            };
            Tapped { key, label: m.label().to_string(), own, strip, why }
        })
        .collect()
}

/// What to listen to.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    /// The range, in seconds of the song.
    pub from: f64,
    pub to: f64,
    /// Seconds rendered before `from` and dropped (RENDER-4): approximate unless it reaches 0.
    pub preroll: f64,
    /// The tracks to measure, by key; every one when None.
    pub tracks: Option<Vec<String>>,
    /// Each overlay site's value (`--as-heard`, MON-3); None: as the song sets them.
    pub overlays: Option<Vec<f32>>,
    /// The picture's columns: the mix's, and with `tracks`, each one's as heard (else its own).
    pub picture: bool,
    pub threads: Threads,
}

/// One track, heard.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackHeard {
    pub key: String,
    pub label: String,
    /// Its channel, when it is its own.
    pub channel: Option<String>,
    /// Why it has no strip of its own.
    pub why: Option<String>,
    /// Its own sound; None when nothing in the song reads it.
    pub own: Option<Measures>,
    /// As it reaches the mix: after its channel's pan and fader.
    pub heard: Option<Measures>,
}

/// What a listen heard.
#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    /// The range, in seconds.
    pub from: f64,
    pub to: f64,
    /// It rendered from the song's start (RENDER-5).
    pub exact: bool,
    /// The same files render the same bits again: no plugin (RENDER-2).
    pub reproducible: bool,
    /// Samples silenced as not numbers.
    pub non_finite: u64,
    pub threads: usize,
    pub mix: Measures,
    pub tracks: Vec<TrackHeard>,
    pub masking: Vec<Masked>,
    /// The picture's blocks, by name: "mix" first.
    pub pictures: Vec<(String, Picture)>,
}

/// Listen to `ask`'s range of `rev`: render it with copy taps, and measure them.
pub fn hear(rev: Arc<Revision>, packages: &PackageSet, ask: &Ask) -> Result<Heard> {
    let all = tapped(&rev);
    let tracks: Vec<Tapped> = match &ask.tracks {
        None => all,
        Some(keys) => keys
            .iter()
            .map(|k| {
                let names = || all.iter().map(|t| t.key.as_str()).collect::<Vec<_>>().join(", ");
                all.iter().find(|t| &t.key == k).cloned().with_context(|| format!("there's no track `{k}`: the tracks are {}", names()))
            })
            .collect::<Result<_>>()?,
    };
    let rate = rev.project.sample_rate();
    let song: Arc<dyn Song> = rev.clone();
    let namer = Namer::new(0, song.clone(), packages);
    let (flat, data) = (song.flat(), song.data());
    let frame = |s: f64| (s.max(0.0) * rate as f64).round() as u64;
    let (first, end) = (frame(ask.from), frame(ask.to));
    if end <= first {
        bail!("there's nothing to listen to: the range is empty");
    }
    let t0 = frame(ask.from - ask.preroll);
    let made = match t0 {
        0 => Renderer::new(flat, data, packages, rate),
        t => Renderer::starting_at(flat, data, packages, rate, t),
    };
    let mut r = made.map_err(|e| namer.faulted(e))?;
    r.set_strict(false);
    if let Some(v) = &ask.overlays {
        r.set_overlays(v);
    }
    // A tap and a meter for the mix, then for each track its own sound and its strip.
    let pictured = ask.picture && ask.tracks.is_some();
    let (mut at, mut meters) = (vec![TapAt::Out], vec![Meter::new(rate, Wants { bands: true, stereo: true, picture: ask.picture })]);
    let mut slots = vec![];
    for t in &tracks {
        let mut tap = |a: TapAt, wants: Wants| {
            at.push(a);
            meters.push(Meter::new(rate, wants));
            meters.len() - 1
        };
        let own = t.own.map(|k| tap(TapAt::Site(k), Wants { picture: pictured && t.strip.is_none(), ..Wants::default() }));
        let heard = t.strip.as_ref().map(|&(_, n)| tap(TapAt::Node(n), Wants { bands: true, stereo: true, picture: pictured }));
        slots.push((own, heard));
    }
    r.copy_taps(&at, first)?;
    let (mut runner, _pool) = threads_for(&r, ask.threads);
    let late: Vec<u64> = {
        let c = r.copies().context("copy taps")?;
        (0..c.taps()).map(|t| c.late(t) as u64).collect()
    };
    // Rendered until each tap has the range's last frame: what it hands out is past the song's
    // latency, which the copies count in their own.
    let deliver = (end + late.iter().copied().max().unwrap_or(0)).saturating_sub(t0);
    let mut scratch = vec![[0.0f32; 2]; CHUNK];
    let mut done = 0;
    while done < deliver {
        let n = (deliver - done).min(CHUNK as u64) as usize;
        r.process_on(&mut scratch[..n], &mut runner).map_err(|e| namer.faulted(e))?;
        done += n as u64;
        let c = r.copies().context("copy taps")?;
        feed(c, &late, (first, end), &mut meters);
        c.clear();
    }
    runner.rest();
    let measured = |i: Option<usize>| i.map(|i| meters[i].measures());
    let tracks: Vec<TrackHeard> = tracks
        .iter()
        .zip(&slots)
        .map(|(t, &(own, heard))| TrackHeard {
            key: t.key.clone(),
            label: t.label.clone(),
            channel: t.strip.as_ref().map(|s| s.0.clone()),
            why: t.why.clone(),
            own: measured(own),
            heard: measured(heard),
        })
        .collect();
    let bands: Vec<(&str, [Option<f64>; 10])> =
        tracks.iter().filter_map(|t| Some((t.key.as_str(), t.heard.as_ref()?.bands?))).collect();
    let mut pictures: Vec<(String, Picture)> = meters[0].picture().map(|p| ("mix".to_string(), p)).into_iter().collect();
    for (t, &(own, heard)) in tracks.iter().zip(&slots) {
        if let Some(p) = heard.or(own).and_then(|i| meters[i].picture()) {
            pictures.push((t.key.clone(), p));
        }
    }
    Ok(Heard {
        from: ask.from,
        to: ask.to,
        exact: t0 == 0,
        reproducible: pure(flat),
        non_finite: r.non_finite(),
        threads: runner.threads(),
        mix: meters[0].measures(),
        masking: masking(&bands),
        tracks,
        pictures,
    })
}

/// Hand each tap's copied blocks to its meter: the samples that carry song frames `first` to `end`.
fn feed(c: &Copies, late: &[u64], (first, end): (u64, u64), meters: &mut [Meter]) {
    for (k, &at) in c.blocks().iter().enumerate() {
        for (t, m) in meters.iter_mut().enumerate() {
            // The song frame the block's first sample carries.
            let song = at as i64 - late[t] as i64;
            let (lo, hi) = ((first as i64).max(song), (end as i64).min(song + BLOCK as i64));
            if lo >= hi {
                continue;
            }
            let (a, b) = ((lo - song) as usize, (hi - song) as usize);
            let x = c.block(t, k);
            let left = &x[a..b];
            let right = if c.channels(t) == 2 { &x[BLOCK + a..BLOCK + b] } else { left };
            m.push(left, right);
        }
    }
}

/// `x` to one decimal: dB.
fn tenth(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// `x` to two decimals: a correlation.
fn hundredth(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// An octave band's name: its centre in Hz, as ISO 266 names it (`31.5`, `63`, `16000`).
fn band_name(hz: f64) -> String {
    m2_song::lane::number(hz)
}

impl Measures {
    /// As JSON: levels in dB to a tenth, null where there is none (a silent track's).
    pub fn json(&self) -> Value {
        let db = |x: Option<f64>| x.map(tenth);
        let mut v = json!({
            "lufs": db(self.lufs),
            "lufs_short_max": db(self.lufs_short_max),
            "true_peak": db(self.true_peak),
            "peak": db(self.peak),
            "rms": db(self.rms),
            "crest": db(self.crest),
            "clipped": self.clipped,
            "silent": self.silent,
        });
        if let Some(b) = &self.bands {
            let bands: Map<String, Value> = OCTAVES.iter().zip(b).map(|(&hz, &l)| (band_name(hz), json!(db(l)))).collect();
            v["bands"] = Value::Object(bands);
        }
        if let Some(s) = &self.stereo {
            v["stereo"] = json!({"correlation": s.correlation.map(hundredth), "width": db(s.width), "lows_mono": s.lows_mono});
        }
        v
    }

    /// In words: its loudness, true peak and crest; a silent one says so.
    fn said(&self) -> String {
        if self.silent {
            return "silent".into();
        }
        let n = |x: Option<f64>| x.map_or_else(|| "—".into(), |x| format!("{:.1}", tenth(x)));
        format!("{} LUFS, true peak {} dBTP, crest {} dB", n(self.lufs), n(self.true_peak), n(self.crest))
    }
}

impl Heard {
    /// As JSON: the range, the mix, each track by key (its own sound and as heard), masking and
    /// the silent tracks.
    pub fn json(&self) -> Value {
        let tracks: Map<String, Value> = self
            .tracks
            .iter()
            .map(|t| {
                let mut v = json!({
                    "label": t.label,
                    "channel": t.channel,
                    "own": t.own.as_ref().map(Measures::json),
                    "heard": t.heard.as_ref().map(Measures::json),
                });
                if let Some(why) = &t.why {
                    v["why"] = json!(format!("{why}, so its masking isn't measured"));
                }
                (t.key.clone(), v)
            })
            .collect();
        let masking: Vec<Value> = self
            .masking
            .iter()
            .map(|m| json!({"hz": m.hz, "tracks": m.tracks, "db": m.db.iter().map(|&d| tenth(d)).collect::<Vec<_>>()}))
            .collect();
        json!({
            "seconds": [(self.from * 1000.0).round() / 1000.0, (self.to * 1000.0).round() / 1000.0],
            "exact": self.exact,
            "reproducible": self.reproducible,
            "mix": self.mix.json(),
            "tracks": tracks,
            "masking": masking,
            "silent": self.silent(),
            "non_finite": self.non_finite,
        })
    }

    /// The tracks silent in the range: their own sound, else as heard.
    pub fn silent(&self) -> Vec<&str> {
        self.tracks.iter().filter(|t| t.own.as_ref().or(t.heard.as_ref()).is_some_and(|m| m.silent)).map(|t| t.key.as_str()).collect()
    }

    /// In words, a line each: the mix, its bands, each track its own sound and as heard, masking,
    /// and the silent tracks.
    pub fn words(&self) -> String {
        let mut out = vec![];
        let m = &self.mix;
        let n = |x: Option<f64>| x.map_or_else(|| "—".into(), |x| format!("{:.1}", tenth(x)));
        let mut mix = match m.silent {
            true => "silent".to_string(),
            false => format!(
                "{} LUFS, short-term max {}; true peak {} dBTP; crest {} dB",
                n(m.lufs),
                n(m.lufs_short_max),
                n(m.true_peak),
                n(m.crest)
            ),
        };
        if let Some(s) = &m.stereo {
            let c = s.correlation.map_or_else(|| "—".into(), |c| format!("{:.2}", hundredth(c)));
            let w = s.width.map_or_else(|| "—".into(), |w| format!("{:.1}", tenth(w)));
            let lows = if s.lows_mono { "lows mono" } else { "lows not mono" };
            mix.push_str(&format!("; correlation {c}, width {w} dB, {lows}"));
        }
        if m.clipped > 0 {
            mix.push_str(&format!("; {} samples over 0 dBFS", m.clipped));
        }
        out.push(format!("{:<12} {mix}", "mix"));
        if let Some(b) = &m.bands {
            let each: Vec<String> = OCTAVES
                .iter()
                .zip(b)
                .map(|(&hz, l)| format!("{} {}", band_name(hz), l.map_or_else(|| "—".into(), |l| format!("{:.0}", l))))
                .collect();
            out.push(format!("{:<12} {} (Hz dBFS)", "  bands", each.join(" · ")));
        }
        for t in &self.tracks {
            let own = t.own.as_ref().map_or_else(|| "not in the mix".into(), Measures::said);
            let heard = match (&t.heard, &t.why) {
                (Some(h), _) => format!("as heard {}", h.said()),
                (None, Some(why)) => format!("not measured as heard: {why}"),
                (None, None) => "not measured as heard".into(),
            };
            out.push(format!("{:<12} own {own} · {heard}", t.key));
        }
        let masking: Vec<String> = self
            .masking
            .iter()
            .map(|m| {
                let db: Vec<String> = m.db.iter().map(|d| format!("{:.1}", tenth(*d))).collect();
                format!("{} Hz: {} ({} dBFS)", band_name(m.hz), m.tracks.join(", "), db.join(", "))
            })
            .collect();
        out.push(format!("{:<12} {}", "masking", if masking.is_empty() { "none".into() } else { masking.join("; ") }));
        let silent = self.silent();
        if !silent.is_empty() {
            out.push(format!("{:<12} {}", "silent", silent.join(", ")));
        }
        out.join("\n")
    }
}

/// What a Tx changes in what a listen measures: `after` less `before`, for every number the mix
/// and each track have in both (to a tenth of a dB; a correlation to a hundredth) that changed, and
/// the masking that began and ended. What didn't change isn't there.
pub fn difference(before: &Heard, after: &Heard) -> Value {
    let (b, a) = (before.json(), after.json());
    let mut tracks = Map::new();
    if let (Some(bt), Some(at)) = (b["tracks"].as_object(), a["tracks"].as_object()) {
        for (k, t) in at {
            if let Some(was) = bt.get(k)
                && let d @ Value::Object(_) = less(t, was, "")
            {
                tracks.insert(k.clone(), d);
            }
        }
    }
    // Band by band, the tracks that began masking there and those that ended.
    let masking = |of: &Heard, hz: f64| -> Vec<String> {
        of.masking.iter().filter(|m| m.hz == hz).flat_map(|m| m.tracks.clone()).collect()
    };
    let (mut began, mut ended) = (vec![], vec![]);
    for hz in OCTAVES {
        let (was, now) = (masking(before, hz), masking(after, hz));
        let new: Vec<&String> = now.iter().filter(|t| !was.contains(t)).collect();
        let gone: Vec<&String> = was.iter().filter(|t| !now.contains(t)).collect();
        if !new.is_empty() {
            began.push(json!({"hz": hz, "tracks": new}));
        }
        if !gone.is_empty() {
            ended.push(json!({"hz": hz, "tracks": gone}));
        }
    }
    json!({"mix": less(&a["mix"], &b["mix"], ""), "tracks": tracks, "masking": {"began": began, "ended": ended}})
}

/// [`difference`] in words, a line for the mix and each track that changed: its loudness and true
/// peak, and each band that moved a dB or more apart from its loudness; then the masking that
/// began and ended.
pub fn difference_words(d: &Value) -> String {
    let said = |who: &str, m: &Value| -> Option<String> {
        let mut parts = vec![];
        let signed = |x: f64| format!("{x:+.1}");
        if let Some(x) = m["lufs"].as_f64() {
            parts.push(format!("{} LUFS", signed(x)));
        }
        if let Some(x) = m["true_peak"].as_f64() {
            parts.push(format!("true peak {} dB", signed(x)));
        }
        let louder = m["lufs"].as_f64().unwrap_or(0.0);
        let bands: Vec<String> = OCTAVES
            .iter()
            .filter_map(|&hz| {
                let x = m["bands"][band_name(hz)].as_f64().unwrap_or(0.0);
                ((x - louder).abs() >= 1.0).then(|| format!("{} Hz {}", band_name(hz), signed(x)))
            })
            .collect();
        if !bands.is_empty() {
            parts.push(format!("bands {} dB", bands.join(", ")));
        }
        (!parts.is_empty()).then(|| format!("{who} {}", parts.join("; ")))
    };
    let mut lines: Vec<String> = said("mix", &d["mix"]).into_iter().collect();
    for (k, t) in d["tracks"].as_object().into_iter().flatten() {
        lines.extend(said(&format!("{k}'s own sound"), &t["own"]));
        lines.extend(said(&format!("{k} as heard"), &t["heard"]));
    }
    if lines.is_empty() {
        lines.push("nothing measured changed".into());
    }
    let pairs = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .map(|m| {
                let tracks: Vec<&str> = m["tracks"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                format!("{} Hz: {}", m["hz"], tracks.join(", "))
            })
            .collect()
    };
    let (began, ended) = (pairs(&d["masking"]["began"]), pairs(&d["masking"]["ended"]));
    if !began.is_empty() {
        lines.push(format!("masking began: {}", began.join("; ")));
    }
    if !ended.is_empty() {
        lines.push(format!("masking ended: {}", ended.join("; ")));
    }
    let mut out = vec![];
    for (i, l) in lines.into_iter().enumerate() {
        out.push(format!("{:<12} {l}", if i == 0 { "change" } else { "" }));
    }
    out.join("\n")
}

/// `a - b` for each number both have at the same place; objects inside, by key.
fn less(a: &Value, b: &Value, key: &str) -> Value {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let d = x.as_f64().unwrap_or(0.0) - y.as_f64().unwrap_or(0.0);
            let d = if key == "correlation" { hundredth(d) } else { tenth(d) };
            if d == 0.0 { Value::Null } else { json!(d) }
        }
        (Value::Object(x), Value::Object(y)) => {
            let m: Map<String, Value> = x
                .iter()
                .filter_map(|(k, v)| {
                    let d = less(v, y.get(k)?, k);
                    (!d.is_null()).then(|| (k.clone(), d))
                })
                .collect();
            if m.is_empty() { Value::Null } else { Value::Object(m) }
        }
        _ => Value::Null,
    }
}
