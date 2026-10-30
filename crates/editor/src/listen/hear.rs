//! What an agent's listen hears (AGENT-7): which buffers of the song its copy taps read, the range
//! render that copies them, and what it says, as JSON and in words.
//!
//! - **A track's own sound**: the overlay site where its audio enters the song's own module, where
//!   std's mixer is (else its first site, MON-1): before its channel, as a stem is. A bus that
//!   passes its input on has no site: its own sound is what enters its channel's strip.
//! - **As it reaches the mix**: its channel's fader ([`crate::mix::fader`]), after its pan, when the
//!   channel is the track's alone. A channel that goes `to` a bus channel reaches the mix inside
//!   that bus: it is read at its own fader, before the bus's, and the bus is measured as a strip of
//!   its own. Masking is among what the master sums: the strips that go to it. A track in no
//!   channel, in two, in one with another track, or in a song without one std mixer, has none, and
//!   says why: its masking isn't measured.
//! - **The render**: a range render (RENDER-4), from `preroll` before the range, copying its taps
//!   ([`Copying`]) from a second before the range. Each tap's samples are read where they carry the
//!   range's song time (RENDER-19), into a [`Meter`] of their own, warmed with that second: the mix
//!   with its bands and stereo, each track's own sound with its loudness and peaks, and each track
//!   as heard with its bands, stereo and moments too.
//! - **Tracks by key**: the listening mute and solo are given to each revision by its tracks' keys
//!   ([`Overlaid`]), and two revisions' tracks are matched by key ([`difference`]).

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use m2_runtime::{BLOCK, Copies, Copying, PackageSet, Renderer, TapAt};
use m2_session::Revision;
use m2_song::ROOT;
use serde_json::{Map, Value, json};

use super::{Masked, Measures, Meter, Moment, OCTAVES, Picture, Wants, masking};
use crate::faults::{Namer, Song};
use crate::mix::{self, Mixing};
use crate::render::{Threads, pure, threads_for};

/// Frames rendered between reads of the copies.
const CHUNK: usize = 4096;
/// What warms the meters before the range, in seconds, when the render has it: their filters
/// settle, and the true peak has the samples before the range's first (RENDER-4's preroll).
const WARM: f64 = 1.0;

/// Where one track is read.
#[derive(Clone, Debug, PartialEq)]
pub struct Tapped {
    pub key: String,
    pub label: String,
    /// Where its own sound is read: None when nothing in the song reads it.
    pub own: Option<TapAt>,
    /// Its channel, and its fader's node, when the channel is its own.
    pub strip: Option<(String, usize)>,
    /// The bus channel its channel goes to instead of the master.
    pub to: Option<String>,
    /// Why its masking isn't measured, when it isn't: a sentence.
    pub why: Option<String>,
}

/// Where each track of `rev` is read, in the song's order: its buses too.
pub fn tapped(rev: &Revision) -> Vec<Tapped> {
    let flat = &rev.compiled.flat;
    let mixing = mix::mixer(rev);
    let routes = match &mixing {
        Mixing::One(mx) => crate::bus::routing(rev, mx),
        _ => Default::default(),
    };
    let modules = rev.compiled.ir.modules.iter().filter(|m| m.track);
    modules
        .map(|m| {
            let key = m.key.clone();
            let sites: Vec<usize> = (0..flat.overlays.len()).filter(|&k| flat.overlays[k].module == key).collect();
            let mut own = sites.iter().copied().find(|&k| flat.overlays[k].into == ROOT).or(sites.first().copied()).map(TapAt::Site);
            let not = |why: String| (None, None, Some(format!("{why}, so its masking isn't measured")));
            let (strip, to, why) = match &mixing {
                Mixing::One(mx) => {
                    let ins: Vec<&mix::Channel> = mx.channels.iter().filter(|c| c.tracks.contains(&key)).collect();
                    match ins[..] {
                        [] if own.is_none() => not("nothing in the song reads it".into()),
                        [] => not("it isn't in the mixer".into()),
                        [c] if c.tracks.len() > 1 => {
                            let others: Vec<&str> = c.tracks.iter().filter(|t| **t != key).map(String::as_str).collect();
                            not(format!("it shares channel {} with {}", c.key, others.join(", ")))
                        }
                        [c] => match mix::fader(rev, mx, &c.key) {
                            Some(n) => {
                                // A bus that passes its input on has no site: what enters its strip.
                                if own.is_none() && m.bus {
                                    own = mix::strip_input(rev, mx, &c.key).map(TapAt::Node);
                                }
                                let to = routes.get(&c.key).and_then(|r| r.to.clone());
                                let why = to.as_ref().map(|b| {
                                    format!(
                                        "channel {} goes to bus channel {b}: it is read at its own fader, before the bus's, and its \
                                         masking is measured on {b}, as that reaches the mix",
                                        c.key
                                    )
                                });
                                (Some((c.key.clone(), n)), to, why)
                            }
                            None => not(format!("channel {}'s fader wasn't found", c.key)),
                        },
                        _ => not(format!("it is in {} channels", ins.len())),
                    }
                }
                _ if own.is_none() => not("nothing in the song reads it".into()),
                Mixing::None => not("the song doesn't use std's mixer".into()),
                Mixing::Several(n) => not(format!("the song has {n} mixers")),
                Mixing::Unwritten => not("the song's mixer isn't one object written in its code".into()),
            };
            Tapped { key, label: m.label().to_string(), own, strip, to, why }
        })
        .collect()
}

/// Each overlay site's value for a revision (MON-3), found from its own tracks by their keys:
/// `--as-heard`'s mute and solo. Given the revision it renders, so never another's values.
pub type Overlaid = Arc<dyn Fn(&Revision) -> Vec<f32> + Send + Sync>;

/// What to listen to.
#[derive(Clone)]
pub struct Ask {
    /// The range, in seconds of the song.
    pub from: f64,
    pub to: f64,
    /// Seconds rendered before `from` and dropped (RENDER-4): approximate unless it reaches 0.
    pub preroll: f64,
    /// The tracks to measure, by key, each once; every one when None.
    pub tracks: Option<Vec<String>>,
    /// The listening mute and solo (`--as-heard`); None: as the song sets them.
    pub overlays: Option<Overlaid>,
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
    /// The bus channel its channel goes to instead of the master.
    pub to: Option<String>,
    /// Why its masking isn't measured, when it isn't.
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
    /// The song's output samples silenced as not numbers.
    pub non_finite: u64,
    pub threads: usize,
    pub mix: Measures,
    pub tracks: Vec<TrackHeard>,
    pub masking: Vec<Masked>,
    /// The picture's blocks, by name: "mix" first.
    pub pictures: Vec<(String, Picture)>,
}

/// Listen to `ask`'s range of `rev`: render it with copy taps, and measure them. It runs the
/// kernels of the packages `rev` was built with (PACK-9), whatever `packages` shows: a candidate
/// (`m2 listen --with`) with its own.
pub fn hear(rev: Arc<Revision>, packages: &PackageSet, ask: &Ask) -> Result<Heard> {
    let all = tapped(&rev);
    let tracks: Vec<Tapped> = match &ask.tracks {
        None => all,
        Some(keys) => {
            let names = || all.iter().map(|t| t.key.as_str()).collect::<Vec<_>>().join(", ");
            let mut out: Vec<Tapped> = vec![];
            for k in keys {
                let t = all.iter().find(|t| &t.key == k).with_context(|| format!("there's no track `{k}`: the tracks are {}", names()))?;
                if !out.iter().any(|o| o.key == t.key) {
                    out.push(t.clone());
                }
            }
            out
        }
    };
    let rate = rev.project.sample_rate();
    let song: Arc<dyn Song> = rev.clone();
    let seen = crate::render::seen(&*song, packages);
    let packages = &*seen;
    let namer = Namer::new(0, song.clone(), packages);
    let (flat, data) = (song.flat(), song.data());
    let frame = |s: f64| (s.max(0.0) * rate as f64).round() as u64;
    let (first, end) = (frame(ask.from), frame(ask.to));
    if end <= first {
        bail!("there's nothing to listen to: the range is empty");
    }
    let t0 = frame(ask.from - ask.preroll);
    // The meters are warmed with what the render has before the range, a second at most.
    let warm = first - first.min(t0.max(first.saturating_sub(frame(WARM))));
    let made = match t0 {
        0 => Renderer::new(flat, data, packages, rate),
        t => Renderer::starting_at(flat, data, packages, rate, t),
    };
    let mut r = made.map_err(|e| namer.faulted(e))?;
    r.set_strict(false);
    if let Some(values) = &ask.overlays {
        r.set_overlays(&values(&rev));
    }
    // A tap and a meter for the mix, then for each track its own sound and its strip.
    let pictured = ask.picture && ask.tracks.is_some();
    let wants = |picture: bool| Wants { picture, ..Wants::default() };
    let (mut at, mut meters) = (vec![TapAt::Out], vec![Meter::new(rate, Wants { bands: true, stereo: true, ..wants(ask.picture) })]);
    let mut slots = vec![];
    for t in &tracks {
        let mut tap = |a: TapAt, wants: Wants| {
            at.push(a);
            meters.push(Meter::new(rate, wants));
            meters.len() - 1
        };
        let own = t.own.map(|a| tap(a, wants(pictured && t.strip.is_none())));
        let heard =
            t.strip.as_ref().map(|&(_, n)| tap(TapAt::Node(n), Wants { bands: true, stereo: true, moments: true, picture: pictured }));
        slots.push((own, heard));
    }
    let (mut runner, _pool) = threads_for(&r, ask.threads);
    let mut c = Copying::new(r, &at, first - warm)?;
    let late: Vec<u64> = {
        let c = c.copies();
        (0..c.taps()).map(|t| c.late(t) as u64).collect()
    };
    // Rendered until each tap has the range's last frame: what it hands out is past the song's
    // latency, which the copies count in their own.
    let deliver = (end + late.iter().copied().max().unwrap_or(0)).saturating_sub(t0);
    let mut scratch = vec![[0.0f32; 2]; CHUNK];
    let mut done = 0;
    while done < deliver {
        let n = (deliver - done).min(CHUNK as u64) as usize;
        c.process_on(&mut scratch[..n], &mut runner).map_err(|e| namer.faulted(e))?;
        done += n as u64;
        feed(c.copies(), &late, (first - warm, first, end), &mut meters);
        c.copies().clear();
    }
    runner.rest();
    let measured = |i: Option<usize>| i.map(|i| meters[i].measures());
    let heard: Vec<TrackHeard> = tracks
        .iter()
        .zip(&slots)
        .map(|(t, &(own, heard))| TrackHeard {
            key: t.key.clone(),
            label: t.label.clone(),
            channel: t.strip.as_ref().map(|s| s.0.clone()),
            to: t.to.clone(),
            why: t.why.clone(),
            own: measured(own),
            heard: measured(heard),
        })
        .collect();
    // Masking is among what the master sums: the strips that go to it.
    let moments: Vec<(&str, &[Moment])> = tracks
        .iter()
        .zip(&slots)
        .filter(|(t, _)| t.to.is_none())
        .filter_map(|(t, &(_, heard))| Some((t.key.as_str(), meters[heard?].moments()?)))
        .collect();
    let masking = masking(&moments);
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
        non_finite: c.renderer().non_finite(),
        threads: runner.threads(),
        mix: meters[0].measures(),
        masking,
        tracks: heard,
        pictures,
    })
}

/// Hand each tap's copied blocks to its meter: the samples that carry song frames `warm` to
/// `first` warm it, and those from `first` to `end` are the range.
fn feed(c: &Copies, late: &[u64], (warm, first, end): (u64, u64, u64), meters: &mut [Meter]) {
    for (k, &at) in c.blocks().iter().enumerate() {
        for (t, m) in meters.iter_mut().enumerate() {
            // The song frame the block's first sample carries.
            let song = at as i64 - late[t] as i64;
            let x = c.block(t, k);
            let side = |a: usize, b: usize| -> (&[f32], &[f32]) {
                let left = &x[a..b];
                (left, if c.channels(t) == 2 { &x[BLOCK + a..BLOCK + b] } else { left })
            };
            for (lo, hi, count) in [(warm, first, false), (first, end, true)] {
                let (lo, hi) = ((lo as i64).max(song), (hi as i64).min(song + BLOCK as i64));
                if lo >= hi {
                    continue;
                }
                let (left, right) = side((lo - song) as usize, (hi - song) as usize);
                if count { m.push(left, right) } else { m.warm(left, right) }
            }
        }
    }
}

/// `x` to one decimal: dB.
fn tenth(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// `x` to two decimals: a correlation, a share.
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
            "non_finite": self.non_finite,
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

    /// In words: its loudness, true peak and crest; a silent one says so; samples that weren't
    /// numbers, when there were.
    fn said(&self) -> String {
        let bad = match self.non_finite {
            0 => String::new(),
            n => format!(" ({n} samples weren't numbers, NaN or infinity, and were read as 0)"),
        };
        if self.silent {
            return format!("silent{bad}");
        }
        let n = |x: Option<f64>| x.map_or_else(|| "—".into(), |x| format!("{:.1}", tenth(x)));
        format!("{} LUFS, true peak {} dBTP, crest {} dB{bad}", n(self.lufs), n(self.true_peak), n(self.crest))
    }
}

/// Masking as JSON: each pair, its band, its levels there and how much of the range it holds.
fn masking_json(m: &Masked) -> Value {
    json!({"hz": m.hz, "tracks": m.tracks, "db": m.db.iter().map(|&d| tenth(d)).collect::<Vec<_>>(), "share": hundredth(m.share)})
}

/// The pairs that mask the most of the range said in words; this many at most.
const SAID: usize = 12;

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
                if let Some(to) = &t.to {
                    v["to"] = json!(to);
                }
                if let Some(why) = &t.why {
                    v["why"] = json!(why);
                }
                (t.key.clone(), v)
            })
            .collect();
        json!({
            "seconds": [(self.from * 1000.0).round() / 1000.0, (self.to * 1000.0).round() / 1000.0],
            "exact": self.exact,
            "reproducible": self.reproducible,
            "mix": self.mix.json(),
            "tracks": tracks,
            "masking": self.masking.iter().map(masking_json).collect::<Vec<_>>(),
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
        if self.non_finite > 0 {
            mix.push_str(&format!("; {} samples weren't numbers and were silenced", self.non_finite));
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
            let heard = match (&t.heard, &t.to) {
                (Some(h), Some(to)) => format!("into bus {to} {}", h.said()),
                (Some(h), None) => format!("as heard {}", h.said()),
                (None, _) => "not measured as heard".into(),
            };
            let why = t.why.as_ref().map(|w| format!(" ({w})")).unwrap_or_default();
            out.push(format!("{:<12} own {own} · {heard}{why}", t.key));
        }
        // The pairs that mask the most of the range first.
        let mut most: Vec<&Masked> = self.masking.iter().collect();
        most.sort_by(|a, b| b.share.total_cmp(&a.share));
        let said: Vec<String> = most
            .iter()
            .take(SAID)
            .map(|m| {
                let db: Vec<String> = m.db.iter().map(|d| format!("{:.1}", tenth(*d))).collect();
                format!("{} Hz: {} {:.0}% of the time ({} dBFS)", band_name(m.hz), m.tracks.join(", "), 100.0 * m.share, db.join(", "))
            })
            .collect();
        let mut masking = if said.is_empty() { "none".into() } else { said.join("; ") };
        if most.len() > SAID {
            masking.push_str(&format!("; and {} more (--json)", most.len() - SAID));
        }
        out.push(format!("{:<12} {masking}", "masking"));
        let silent = self.silent();
        if !silent.is_empty() {
            out.push(format!("{:<12} {}", "silent", silent.join(", ")));
        }
        out.join("\n")
    }
}

/// What a Tx changes in what a listen measures: `after` less `before`, tracks matched by key. For
/// every number the mix and each track have in both that changed (to a tenth of a dB; a
/// correlation to a hundredth), the change; where one has a measure and the other has none (a
/// track that goes silent has no loudness), or a word or a yes-or-no changed (`silent`), what it
/// was and is now; a track only one has, `added` or `removed`; and the masking that began, ended or
/// changed how much of the range it holds. What didn't change isn't there.
pub fn difference(before: &Heard, after: &Heard) -> Value {
    let (b, a) = (before.json(), after.json());
    let mut tracks = Map::new();
    let (bt, at) = (b["tracks"].as_object().cloned().unwrap_or_default(), a["tracks"].as_object().cloned().unwrap_or_default());
    for (k, t) in &at {
        match bt.get(k) {
            Some(was) => {
                if let d @ Value::Object(_) = less(t, was, "") {
                    tracks.insert(k.clone(), d);
                }
            }
            None => drop(tracks.insert(k.clone(), json!({"added": true}))),
        }
    }
    for k in bt.keys().filter(|k| !at.contains_key(*k)) {
        tracks.insert(k.clone(), json!({"removed": true}));
    }
    let pair = |m: &Masked| (m.hz.to_bits(), m.tracks.iter().min().cloned(), m.tracks.iter().max().cloned());
    let find = |of: &Heard, m: &Masked| of.masking.iter().find(|x| pair(x) == pair(m)).map(|x| hundredth(x.share));
    let said = |m: &Masked| json!({"hz": m.hz, "tracks": m.tracks, "share": hundredth(m.share)});
    let began: Vec<Value> = after.masking.iter().filter(|m| find(before, m).is_none()).map(said).collect();
    let ended: Vec<Value> = before.masking.iter().filter(|m| find(after, m).is_none()).map(said).collect();
    let changed: Vec<Value> = after
        .masking
        .iter()
        .filter_map(|m| {
            let was = find(before, m)?;
            (was != hundredth(m.share)).then(|| json!({"hz": m.hz, "tracks": m.tracks, "was": was, "now": hundredth(m.share)}))
        })
        .collect();
    json!({"mix": less(&a["mix"], &b["mix"], ""), "tracks": tracks, "masking": {"began": began, "ended": ended, "changed": changed}})
}

/// [`difference`] in words, a line for the mix and each track that changed: its loudness and true
/// peak, each band that moved a dB or more apart from its loudness, and what it had and hasn't
/// now (or the other way); then the masking that began, ended or changed.
pub fn difference_words(d: &Value) -> String {
    let value = |v: &Value| match v {
        Value::Null => "none".to_string(),
        Value::Number(n) => format!("{:.1}", n.as_f64().unwrap_or(0.0)),
        Value::Object(_) => "measured".to_string(),
        v => v.to_string(),
    };
    let was_now = |v: &Value| v.get("was").is_some() || v.get("now").is_some();
    let said = |who: &str, m: &Value| -> Option<String> {
        if was_now(m) {
            return Some(format!("{who}: was {}, now {}", value(&m["was"]), value(&m["now"])));
        }
        let mut parts = vec![];
        let signed = |x: f64| format!("{x:+.1}");
        match m["silent"]["now"].as_bool() {
            Some(true) => parts.push("now silent".to_string()),
            Some(false) => parts.push("now sounds".to_string()),
            None => {}
        }
        for (k, unit) in [("lufs", "LUFS"), ("true_peak", "dB true peak")] {
            match &m[k] {
                Value::Number(x) => parts.push(format!("{} {unit}", signed(x.as_f64().unwrap_or(0.0)))),
                v if was_now(v) => parts.push(format!("{k} was {}, now {}", value(&v["was"]), value(&v["now"]))),
                _ => {}
            }
        }
        let louder = m["lufs"].as_f64().unwrap_or(0.0);
        let bands: Vec<String> = OCTAVES
            .iter()
            .filter_map(|&hz| {
                let b = &m["bands"][band_name(hz)];
                match b.as_f64() {
                    Some(x) => ((x - louder).abs() >= 1.0).then(|| format!("{} Hz {}", band_name(hz), signed(x))),
                    None if was_now(b) => Some(format!("{} Hz was {}, now {}", band_name(hz), value(&b["was"]), value(&b["now"]))),
                    None => None,
                }
            })
            .collect();
        if !bands.is_empty() {
            parts.push(format!("bands {} dB", bands.join(", ")));
        }
        if m["non_finite"].as_f64().is_some_and(|x| x > 0.0) {
            parts.push("more samples that weren't numbers".to_string());
        }
        (!parts.is_empty()).then(|| format!("{who} {}", parts.join("; ")))
    };
    let mut lines: Vec<String> = said("mix", &d["mix"]).into_iter().collect();
    for (k, t) in d["tracks"].as_object().into_iter().flatten() {
        if t["added"] == json!(true) {
            lines.push(format!("{k}: new with the change"));
            continue;
        }
        if t["removed"] == json!(true) {
            lines.push(format!("{k}: gone with the change"));
            continue;
        }
        lines.extend(said(&format!("{k}'s own sound"), &t["own"]));
        lines.extend(said(&format!("{k} as heard"), &t["heard"]));
        for (f, name) in [("channel", "channel"), ("to", "bus")] {
            if let Some(c) = t.get(f).filter(|c| was_now(c)) {
                lines.push(format!("{k}'s {name}: was {}, now {}", value(&c["was"]), value(&c["now"])));
            }
        }
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
                let share = |x: &Value| format!("{:.0}%", 100.0 * x.as_f64().unwrap_or(0.0));
                let how = match m.get("was") {
                    Some(was) => format!("{} of the time, from {}", share(&m["now"]), share(was)),
                    None => format!("{} of the time", share(&m["share"])),
                };
                format!("{} Hz: {} {how}", m["hz"], tracks.join(", "))
            })
            .collect()
    };
    for (k, said) in [("began", "masking began"), ("ended", "masking ended"), ("changed", "masking changed")] {
        let p = pairs(&d["masking"][k]);
        if !p.is_empty() {
            lines.push(format!("{said}: {}", p.join("; ")));
        }
    }
    let mut out = vec![];
    for (i, l) in lines.into_iter().enumerate() {
        out.push(format!("{:<12} {l}", if i == 0 { "change" } else { "" }));
    }
    out.join("\n")
}

/// `a - b` for each number both have at the same place; objects inside, by key. Where one has a
/// value and the other has none or another kind of value, or a word or a yes-or-no changed: what
/// it was (`b`) and is now (`a`).
fn less(a: &Value, b: &Value, key: &str) -> Value {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let d = x.as_f64().unwrap_or(0.0) - y.as_f64().unwrap_or(0.0);
            let d = if key == "correlation" { hundredth(d) } else { tenth(d) };
            if d == 0.0 { Value::Null } else { json!(d) }
        }
        (Value::Object(x), Value::Object(y)) => {
            let none = Value::Null;
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            let m: Map<String, Value> = keys
                .into_iter()
                .filter_map(|k| {
                    let d = less(x.get(k).unwrap_or(&none), y.get(k).unwrap_or(&none), k);
                    (!d.is_null()).then(|| (k.clone(), d))
                })
                .collect();
            if m.is_empty() { Value::Null } else { Value::Object(m) }
        }
        _ if a == b => Value::Null,
        _ => json!({"was": b, "now": a}),
    }
}
