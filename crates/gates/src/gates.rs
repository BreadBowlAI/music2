//! Step 3's gates on a rig (contract, Acceptance gates; step 3a's design, §15). Each plays a song,
//! does what the gate names (seek, loop, edit, stop, add an effect), and checks what was heard.
//!
//! Latencies are counted as step 3a's design counts them: the wall time spent preparing (after
//! the compile, which each budget excludes), then the frames until the engine applied the change,
//! then one 512-frame device buffer.

use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_runtime::Renderer;
use m2_session::reference::Reference;
use m2_song::{LaneAddr, NoteClip};

use crate::report::Outcome;
use crate::rig::{Change, Cmd, Deck, Kind, NoteOn, PERIOD, Sent};
use crate::signal::{self, Around, Rng, Summary, db, rms, span};
use crate::songs::{self, Fx, LOOP, PAD_NOTE, Pad};
use crate::{Lab, Song};

/// How long and how big a run is.
#[derive(Clone, Debug)]
pub struct Opts {
    /// The reference project the timed gates play: p16 for a recorded run, a 3-track one in CI.
    pub reference: Reference,
    pub seed: u64,
    /// Device buffer, in frames.
    pub period: usize,
    /// How long the loop gate loops.
    pub loop_minutes: f64,
    pub seeks: usize,
    pub activations: usize,
    /// Edits G3 and G5 time through the server.
    pub edits: usize,
}

impl Opts {
    /// The contract's recorded runs: p16, 10 minutes of loop, 50 seeks.
    pub fn recorded() -> Opts {
        Opts { reference: Reference::P16, seed: 3, period: PERIOD, loop_minutes: 10.0, seeks: 50, activations: 50, edits: 10 }
    }

    /// Small and quick enough for CI.
    pub fn quick() -> Opts {
        Opts {
            reference: Reference { tracks: 3, ..Reference::P16 },
            loop_minutes: 1.0,
            seeks: 8,
            activations: 12,
            edits: 2,
            ..Opts::recorded()
        }
    }
}

/// The rule ids each gate checks.
pub const GATES: [(&str, &str); 9] = [
    ("seek", "RENDER-8, 9, 10, 11"),
    ("loop", "RENDER-9, 9a, 10"),
    ("note-edit", "RENDER-26, PLANE-1"),
    ("stuck", "RENDER-13, 26a, 9, 6a"),
    ("add-effect", "RENDER-25, 25a, 8"),
    ("step", "RENDER-25a, 27"),
    ("tail-stop", "RENDER-6a, 10"),
    ("range", "RENDER-4, 5"),
    ("plugins", "RENDER-6a, 8, 9, 9a, 10, 11, 25, 26, 26a; HOST-6"),
];

pub fn run(name: &str, lab: &mut Lab, kind: Kind, o: &Opts) -> Result<Outcome> {
    match name {
        "seek" => seek(lab, kind, o),
        "loop" => looping(lab, kind, o),
        "note-edit" => note_edit(lab, kind, o),
        "stuck" => stuck(lab, kind, o),
        "add-effect" => add_effect(lab, kind, o),
        "step" => step(lab, kind, o),
        "tail-stop" => tail_stop(lab, kind, o),
        "range" => range(lab, o),
        "plugins" => crate::plugins::with_plugins(lab, kind, o),
        other => bail!("no gate `{other}`; the gates are {}", GATES.map(|g| g.0).join(", ")),
    }
}

fn rules(gate: &str) -> &'static str {
    GATES.iter().find(|g| g.0 == gate).map_or("", |g| g.1)
}

/// A rig with `song` open at `beat`, playing, `settle` seconds in.
pub(crate) fn playing(lab: &Lab, kind: Kind, o: &Opts, song: &Song, beat: f64, settle: f64) -> Result<(Deck, Sent)> {
    let mut deck = Deck::new(kind.make(lab)?, o.period);
    deck.rig.open(song)?;
    deck.rig.send(Cmd::Seek(beat))?;
    let play = deck.rig.send(Cmd::Play)?;
    deck.run_secs(settle)?;
    Ok((deck, play))
}

/// What the probe says, for counters that must not move: None when the rig doesn't keep it.
pub(crate) fn unchanged(out: &mut Outcome, what: &str, before: Option<u64>, after: Option<u64>) {
    match (before, after) {
        (Some(a), Some(b)) => {
            out.check(what, a == b, format!("{a} before, {b} after"));
        }
        _ => out.skip(what, "this rig doesn't count it"),
    }
}

/// `a - b`, frame by frame.
pub(crate) fn difference(a: &[[f32; 2]], b: &[[f32; 2]]) -> Vec<[f32; 2]> {
    a.iter().zip(b).map(|(x, y)| [x[0] - y[0], x[1] - y[1]]).collect()
}

pub(crate) fn offline(lab: &Lab, song: &Song, frames: u64) -> Result<Vec<[f32; 2]>> {
    let mut r = Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?;
    r.set_strict(false);
    let mut out = vec![[0.0; 2]; frames as usize];
    r.process(&mut out)?;
    Ok(out)
}

// ---------------------------------------------------------------- G1

/// G1: a seek is audible within 50 ms, lands where it was asked, and resets nothing (RENDER-8,
/// RENDER-10); from the trace when kept, every note the song starts from the target on is heard at
/// its frame and nothing else is, so nothing is chased (RENDER-11), and the first of them sounds.
pub fn seek(lab: &mut Lab, kind: Kind, o: &Opts) -> Result<Outcome> {
    let song = lab.reference(&o.reference).and_then(|d| lab.compile(&d))?;
    let one_minute = song.beat(song.rate as u64 * 60);
    let (mut deck, _) = playing(lab, kind, o, &song, one_minute, 1.0)?;
    let mut out = Outcome::new("G1 seek", rules("seek"), &format!("{} on {}", deck.rig.name(), o.reference.name()));
    let before = deck.rig.probe();
    let mut rng = Rng::new(o.seed);
    let (mut late, mut misplaced) = (vec![], vec![]);
    let mut notes = SeekNotes::new(&song);
    for _ in 0..o.seeks {
        let target = rng.range(0.0, song.data.end());
        deck.rig.ons();
        let sent = deck.rig.send(Cmd::Seek(target))?;
        deck.run_secs(rng.range(0.3, 1.0))?;
        late.push(deck.latency_ms(&sent).context("a seek was never applied")?);
        let at = deck.rig.applied(&sent).expect("applied");
        if let Some(t) = deck.rig.probe().transport {
            let want = song.frame(target) + (deck.now() - at);
            if t != want {
                misplaced.push(format!("beat {target:.2}: transport {t}, expected {want}"));
            }
        }
        let ons = deck.rig.ons();
        notes.add(&deck, &song, ons, target, at);
    }
    let s = Summary::of(&late);
    out.check("audible within 50 ms", s.max <= 50.0, s);
    out.check(
        "lands on its target",
        misplaced.is_empty(),
        misplaced.first().map_or("every seek".into(), |m| format!("{} wrong; {m}", misplaced.len())),
    );
    let after = deck.rig.probe();
    unchanged(&mut out, "no kernel reset (RENDER-10)", before.resets, after.resets);
    unchanged(&mut out, "no new renderer (RENDER-8)", before.generation, after.generation);
    notes.report(&mut out, "");
    Ok(out)
}

/// What a run of seeks played, held against the song (RENDER-11): after each seek, the note-ons
/// the engine's trace shows, each placed at the transport frame its engine frame gives, against
/// the note-ons the song's lane files start from the target until the next seek. A seek that goes
/// silent misses notes; one that chases plays notes no note starts there. Neither can hide behind
/// the engine's own stamps.
pub(crate) struct SeekNotes {
    song: Vec<(u64, u32)>,
    seeks: usize,
    expected: usize,
    missing: Vec<String>,
    extra: Vec<String>,
    /// Seeks whose first note didn't sound.
    quiet: Vec<String>,
    /// Seeks with a note to sound in what was heard.
    sounded: usize,
    traced: bool,
}

impl SeekNotes {
    pub(crate) fn new(song: &Song) -> SeekNotes {
        SeekNotes { song: song.note_ons(), seeks: 0, expected: 0, missing: vec![], extra: vec![], quiet: vec![], sounded: 0, traced: true }
    }

    /// The seek to `target` in `song`, applied at engine frame `at` and played until now, with
    /// what the trace kept since just before it (None: this rig keeps no trace).
    pub(crate) fn add(&mut self, deck: &Deck, song: &Song, ons: Option<Vec<NoteOn>>, target: f64, at: u64) {
        let Some(ons) = ons else {
            self.traced = false;
            return;
        };
        let from = song.frame(target);
        let p = played(&self.song, &ons, at, deck.now(), from);
        let beat = |t: u64| format!("transport frame {t} (beat {:.3})", song.beat(t));
        self.seeks += 1;
        self.expected += p.expected.len();
        if let Some(m) = p.missing.first() {
            self.missing.push(format!(
                "seek to beat {target:.2}: {} of {} not heard, first at {}",
                p.missing.len(),
                p.expected.len(),
                beat(m.0)
            ));
        }
        if let Some(x) = p.extra.first() {
            let chased = p.extra.iter().filter(|x| x.0 == from).count();
            self.extra.push(format!(
                "seek to beat {target:.2}: {} heard that no note starts there ({chased} at the target itself), first at {}",
                p.extra.len(),
                beat(x.0)
            ));
        }
        // The first note the song starts after the target, over its first 100 ms.
        if let Some(first) = p.expected.first() {
            let on = at + (first.0 - from);
            let to = on + song.rate as u64 / 10;
            if to <= deck.now() {
                self.sounded += 1;
                let level = db(rms(span(&deck.tape, on, to)));
                if level <= -60.0 {
                    self.quiet.push(format!("seek to beat {target:.2}: {level:.1} dBFS over its first note's 100 ms"));
                }
            }
        }
    }

    pub(crate) fn report(&self, out: &mut Outcome, prefix: &str) {
        let (heard, chased, sounds) = (
            format!("{prefix}every note from the target on is heard (RENDER-11)"),
            format!("{prefix}nothing chased (RENDER-11)"),
            format!("{prefix}the first note after each seek sounds"),
        );
        if !self.traced {
            for name in [heard, chased, sounds] {
                out.skip(name, "this rig keeps no note trace");
            }
            return;
        }
        out.check(
            heard,
            self.expected > 0 && self.missing.is_empty(),
            match self.missing.first() {
                Some(m) => format!("{} of {} seeks missed notes; {m}", self.missing.len(), self.seeks),
                None => format!("{} note-ons the song starts after {} seeks, each heard at its frame", self.expected, self.seeks),
            },
        );
        out.check(
            chased,
            self.extra.is_empty(),
            self.extra
                .first()
                .map_or("no note-on the song doesn't start there".into(), |x| format!("{} of {} seeks; {x}", self.extra.len(), self.seeks)),
        );
        out.check(
            sounds,
            self.sounded > 0 && self.quiet.is_empty(),
            self.quiet
                .first()
                .map_or(format!("over -60 dBFS after each of {} seeks", self.sounded), |q| format!("{} quiet; {q}", self.quiet.len())),
        );
    }
}

/// What a jump played, against what the song has from where it landed (RENDER-11).
pub(crate) struct Played {
    /// The song's note-ons in the transport frames played: `(frame, pitch bits)`.
    pub expected: Vec<(u64, u32)>,
    /// Of those, the ones not heard at their frame.
    pub missing: Vec<(u64, u32)>,
    /// Heard, where no note of that pitch starts: a chased note, or one played out of place.
    pub extra: Vec<(u64, u32)>,
}

/// The note-ons `ons` heard in engine frames `[at, until)`, each placed at the transport frame its
/// engine frame gives (`from` at `at`, no wrap meanwhile), against the song's note-ons `song` (in
/// order) in transport frames `[from, from + until - at)`. Both are multisets: the same note on two
/// tracks is two note-ons.
pub(crate) fn played(song: &[(u64, u32)], ons: &[NoteOn], at: u64, until: u64, from: u64) -> Played {
    let to = from + until.saturating_sub(at);
    let expected = song[song.partition_point(|o| o.0 < from)..song.partition_point(|o| o.0 < to)].to_vec();
    let mut heard: Vec<(u64, u32)> =
        ons.iter().filter(|n| (at..until).contains(&n.engine)).map(|n| (from + (n.engine - at), n.pitch.to_bits())).collect();
    heard.sort_unstable();
    let (mut missing, mut extra) = (vec![], vec![]);
    let (mut e, mut h) = (expected.iter().peekable(), heard.iter().peekable());
    loop {
        match (e.peek(), h.peek()) {
            (Some(x), Some(y)) if x == y => {
                e.next();
                h.next();
            }
            (Some(x), Some(y)) if x < y => missing.extend(e.next()),
            (Some(_), Some(_)) | (None, Some(_)) => extra.extend(h.next()),
            (Some(_), None) => missing.extend(e.next()),
            (None, None) => break,
        }
    }
    Played { expected, missing, extra }
}

// ---------------------------------------------------------------- G2

/// Where a loop run's wraps fall on the engine clock: the first when the transport first reaches
/// the loop's end, then one loop length apart, every length the same integer (RENDER-9a).
struct Laps {
    /// Engine frame of the first wrap.
    first: u64,
    len: u64,
    start: u64,
    /// Engine frame playback started at, and the transport frame it started from.
    played: u64,
    from: u64,
}

impl Laps {
    fn wraps(&self, until: u64) -> impl Iterator<Item = u64> + '_ {
        (0..).map(|k| self.first + k * self.len).take_while(move |w| *w < until)
    }

    /// The engine frame transport frame `t` plays at in lap `k` (lap 0 from where play started).
    fn engine(&self, k: u64, t: u64) -> u64 {
        if k == 0 { self.played + (t - self.from) } else { self.first + (k - 1) * self.len + (t - self.start) }
    }
}

/// A rig playing `song` from 1:00 with [`LOOP`] set.
fn looped(lab: &Lab, kind: Kind, o: &Opts, song: &Song) -> Result<(Deck, Laps)> {
    let from_beat = song.beat(song.rate as u64 * 60);
    let mut deck = Deck::new(kind.make(lab)?, o.period);
    deck.rig.open(song)?;
    deck.rig.send(Cmd::Seek(from_beat))?;
    deck.rig.send(Cmd::Loop(Some(LOOP)))?;
    let play = deck.rig.send(Cmd::Play)?;
    deck.run(deck.period as u64)?;
    let played = deck.rig.applied(&play).context("play was never applied")?;
    let (start, end, from) = (song.frame(LOOP.0), song.frame(LOOP.1), song.frame(from_beat));
    Ok((deck, Laps { first: played + (end - from), len: end - start, start, played, from }))
}

/// G2: a 4-bar loop at 127 bpm runs `loop_minutes` with no tail cut and no drift against the grid.
pub fn looping(lab: &mut Lab, kind: Kind, o: &Opts) -> Result<Outcome> {
    let mut out = Outcome::new("G2 loop", rules("loop"), &format!("{} for {} min", kind.name(), o.loop_minutes));
    let window = 2_400; // 50 ms at 48 kHz

    // Tails: one note mid-loop into a long reverb, then a long delay. Whatever sounds just after a
    // wrap is their tail, which must ring through.
    for fx in [Fx::LongReverb, Fx::LongDelay] {
        let song = lab.project("tail", &songs::tail(fx)).and_then(|d| lab.compile(&d))?;
        let (mut deck, laps) = looped(lab, kind, o, &song)?;
        out.rig = deck.rig.name();
        let before = deck.rig.probe();
        deck.run_to(laps.played + deck.frames(o.loop_minutes * 60.0))?;
        let around: Vec<Around> = laps.wraps(deck.now() - window).map(|w| Around::of(&deck.tape, w, window)).collect();
        let quiet = around.iter().filter(|a| db(a.before) <= -60.0).count();
        let worst = around.iter().min_by(|a, b| a.ratio().total_cmp(&b.ratio())).copied();
        let name = format!("{fx:?}");
        out.check(
            format!("{name}: a tail sounds before every wrap (precondition)"),
            quiet == 0 && !around.is_empty(),
            format!("{} wraps, {quiet} quieter than -60 dBFS", around.len()),
        );
        if let Some(w) = worst {
            out.check(
                format!("{name}: no tail cut at a wrap"),
                w.ratio() >= 0.5,
                format!("worst after/before {:.3} at engine frame {} ({:.1} → {:.1} dBFS)", w.ratio(), w.at, db(w.before), db(w.after)),
            );
        }
        let after = deck.rig.probe();
        unchanged(&mut out, &format!("{name}: no kernel reset"), before.resets, after.resets);
        unchanged(&mut out, &format!("{name}: no new renderer"), before.generation, after.generation);
        if let Some(t) = after.transport {
            let last = laps.wraps(deck.now()).last().unwrap_or(0);
            let want = if last == 0 { laps.from + deck.now() - laps.played } else { laps.start + deck.now() - last };
            out.check(format!("{name}: the transport is where the wraps put it"), t == want, format!("{t}, expected {want}"));
        }
    }

    // The detector's negative control: a panic before the buffer of each wrap cuts the tail, and
    // the detector must say so. A detector that passes this measures nothing.
    let song = lab.project("tail", &songs::tail(Fx::LongReverb)).and_then(|d| lab.compile(&d))?;
    let (mut deck, laps) = looped(lab, kind, o, &song)?;
    let wraps: Vec<u64> = laps.wraps(laps.first + 4 * laps.len).collect();
    for w in &wraps {
        deck.run_to(w - w % deck.period as u64)?;
        deck.rig.send(Cmd::Panic)?;
    }
    deck.run_to(wraps.last().expect("wraps") + window + 1)?;
    let ratios: Vec<f64> = wraps.iter().map(|w| Around::of(&deck.tape, *w, window).ratio()).collect();
    out.check(
        "negative control: a panic at each wrap is seen as a cut",
        ratios.iter().all(|r| *r < 0.5),
        format!("after/before {:.3?}", ratios),
    );

    // Drift: a click on every beat, found to the sample in the output. Each must land where the
    // grid puts it, lap after lap; the offline render says how far into a click the detector fires.
    let song = lab.project("clicks", &songs::clicks()).and_then(|d| lab.compile(&d))?;
    let reference = offline(lab, &song, song.frame(24.0))?;
    let found = signal::onsets(&reference, 0.01, window as usize);
    let offsets: Vec<i64> = found.iter().enumerate().map(|(b, f)| *f as i64 - song.frame(b as f64) as i64).collect();
    let lag = offsets.first().copied().unwrap_or(0);
    if !offsets.iter().all(|x| *x == lag) || offsets.len() < 20 {
        out.check("drift: the click detector is exact offline", false, format!("offsets {offsets:?}"));
        return Ok(out);
    }
    let (mut deck, laps) = looped(lab, kind, o, &song)?;
    deck.run_to(laps.played + deck.frames(o.loop_minutes * 60.0))?;
    let heard = signal::onsets(&deck.tape, 0.01, window as usize);
    let mut expected = vec![];
    // The clicks' transport frames in `[lo, hi)`: lap 0 from where play started, then whole laps.
    let clicks = |lo: u64, hi: u64| (0..160).map(|b| song.frame(b as f64)).filter(move |t| (lo..hi).contains(t));
    let (start, end) = (song.frame(LOOP.0), song.frame(LOOP.1));
    expected.extend(clicks(laps.from, end).map(|t| laps.engine(0, t)));
    for k in 1.. {
        let lap: Vec<u64> = clicks(start, end).map(|t| laps.engine(k, t)).collect();
        if lap.first().is_none_or(|f| *f >= deck.now()) {
            break;
        }
        expected.extend(lap);
    }
    let expected: Vec<u64> = expected.into_iter().map(|e| e + lag as u64).filter(|e| *e + window < deck.now()).collect();
    let heard: Vec<u64> = heard.into_iter().filter(|h| *h + window < deck.now()).collect();
    let wrong = expected.iter().zip(&heard).find(|(e, h)| e != h);
    let laps_heard = (deck.now() - laps.first) / laps.len;
    out.check(
        "drift: every click on the grid, lap after lap",
        heard.len() == expected.len() && wrong.is_none(),
        match wrong {
            Some((e, h)) => format!("click at engine frame {h}, the grid says {e}"),
            None => format!("{} clicks over {laps_heard} laps, heard {}; loop {} frames", expected.len(), heard.len(), laps.len),
        },
    );
    match deck.rig.ons() {
        None => out.skip("drift: each lap's note-ons equal the first lap's", "this rig keeps no note trace"),
        Some(ons) => {
            let lap = |k: u64| -> Vec<(u64, u32)> {
                let (lo, hi) = (laps.first + (k - 1) * laps.len, laps.first + k * laps.len);
                ons.iter().filter(|n| (lo..hi).contains(&n.engine)).map(|n| (n.engine - lo, n.pitch.to_bits())).collect()
            };
            let first = lap(1);
            let bad = (2..=laps_heard).find(|k| lap(*k) != first);
            out.check(
                "drift: each lap's note-ons equal the first lap's",
                bad.is_none(),
                bad.map_or("every lap".into(), |k| format!("lap {k} differs")),
            );
        }
    }
    reference_loop(lab, kind, o, &mut out)?;
    Ok(out)
}

/// The reference project looping (step 3a's design, G2b): every lap plays the same note-ons as the
/// first, each at the engine frame its lap and transport frame put it, and nothing resets.
fn reference_loop(lab: &mut Lab, kind: Kind, o: &Opts, out: &mut Outcome) -> Result<()> {
    let name = o.reference.name();
    let song = lab.reference(&o.reference).and_then(|d| lab.compile(&d))?;
    let (mut deck, laps) = looped(lab, kind, o, &song)?;
    if deck.rig.ons().is_none() {
        // Without a trace there is nothing to compare, and a replaying engine would replay a
        // minute of the song at every wrap.
        out.skip(format!("{name}: every lap plays the first lap's notes on the grid"), "this rig keeps no note trace");
        return Ok(());
    }
    let before = deck.rig.probe();
    let until = laps.played + deck.frames(o.loop_minutes * 60.0);
    let mut ons = vec![];
    // A few seconds at a time, so the trace's ring never laps.
    while deck.now() < until {
        deck.run_to((deck.now() + deck.frames(4.0)).min(until))?;
        ons.extend(deck.rig.ons().unwrap_or_default());
    }
    let after = deck.rig.probe();
    unchanged(out, &format!("{name}: no kernel reset"), before.resets, after.resets);
    unchanged(out, &format!("{name}: no new renderer"), before.generation, after.generation);
    let laps_heard = (deck.now() - laps.first) / laps.len;
    let lap = |k: u64| -> Vec<(u64, u32)> {
        let (lo, hi) = (laps.first + (k - 1) * laps.len, laps.first + k * laps.len);
        ons.iter().filter(|n| (lo..hi).contains(&n.engine)).map(|n| (n.transport, n.pitch.to_bits())).collect()
    };
    let first = lap(1);
    let differs = (2..=laps_heard).find(|k| lap(*k) != first);
    let off_grid =
        ons.iter().find(|n| n.engine >= laps.first && n.engine != laps.engine(1 + (n.engine - laps.first) / laps.len, n.transport));
    out.check(
        format!("{name}: every lap plays the first lap's notes on the grid"),
        !first.is_empty() && differs.is_none() && off_grid.is_none(),
        match (differs, off_grid) {
            (Some(k), _) => format!("lap {k} differs from the first"),
            (_, Some(n)) => format!("a note-on at engine frame {} for transport frame {} is off the grid", n.engine, n.transport),
            _ => format!("{} note-ons a lap over {laps_heard} laps", first.len()),
        },
    );
    Ok(())
}

// ---------------------------------------------------------------- G3

/// Add a note `ahead` beats past `beat` to the notes lane `module/notes` of the project in `dir`,
/// inside the clip that holds that beat. Returns the note's absolute start.
fn add_note(dir: &std::path::Path, module: &str, beat: f64, pitch: f64) -> Result<f64> {
    let mut at = None;
    songs::edit_notes(dir, module, "notes", |clips: &mut Vec<NoteClip>| {
        if let Some(c) = clips.iter_mut().find(|c| c.start <= beat && beat + 0.5 < c.start + c.length) {
            c.notes.push(songs::note(pitch, beat - c.start, 0.5));
            at = Some(beat);
        }
    })?;
    at.ok_or_else(|| anyhow!("no clip of {module} holds beat {beat}"))
}

/// G3: a note edit is audible within compile time + 20 ms, swaps the lane rather than rebuild,
/// and cuts no tail: up to the new note, the output is the same bits as without the edit.
pub fn note_edit(lab: &mut Lab, kind: Kind, o: &Opts) -> Result<Outcome> {
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let one_minute = song.beat(song.rate as u64 * 60);
    let (mut a, _) = playing(lab, kind, o, &song, one_minute, 1.0)?;
    let (mut b, _) = playing(lab, kind, o, &song, one_minute, 1.0)?;
    let mut out = Outcome::new("G3 note edit", rules("note-edit"), &format!("{} on {}", a.rig.name(), o.reference.name()));
    let before = a.rig.probe();
    let (e0, t0) = (a.now(), before.transport.context("the rig has no transport position")?);
    let beat = add_note(&dir, "t01", song.beat(t0) + 0.5, 72.0)?;
    let edited = lab.compile(&dir)?;
    let lane = LaneAddr::new("t01", "notes").map_err(anyhow::Error::msg)?;
    let sent = a.change(&edited, Change::Data(vec![lane]))?;
    // Where the note is written to sound, had nothing jumped.
    let on = e0 + (song.frame(beat) - t0);
    a.run_to(on.max(a.now()) + a.frames(1.0))?;
    b.run_to(a.now())?;
    let latency = a.latency_ms(&sent).context("the edit was never applied")?;
    out.info("compile (not counted)", format!("{:.1} ms", edited.compile.as_secs_f64() * 1e3));
    out.check("audible within compile + 20 ms", latency <= 20.0, format!("{latency:.1} ms after the compile"));
    out.check("took SwapLane, not a new renderer (RENDER-26)", sent.swap, if sent.swap { "swapped" } else { "rebuilt" });
    let after = a.rig.probe();
    unchanged(&mut out, "no kernel reset", before.resets, after.resets);
    unchanged(&mut out, "no new renderer", before.generation, after.generation);
    let same = a.tape[..on as usize] == b.tape[..on as usize];
    let first = a.tape.iter().zip(&b.tape).position(|(x, y)| x != y);
    out.check(
        "no tail cut: the same bits as unedited until the new note",
        same,
        format!("first difference at engine frame {first:?}, note at {on}"),
    );
    let diff: Vec<[f32; 2]> =
        span(&a.tape, on, on + 2_400).iter().zip(span(&b.tape, on, on + 2_400)).map(|(x, y)| [x[0] - y[0], x[1] - y[1]]).collect();
    out.check(
        "the new note is heard where it was written",
        db(rms(&diff)) > -60.0,
        format!("{:.1} dBFS of difference in its first 50 ms", db(rms(&diff))),
    );
    Ok(out)
}

// ---------------------------------------------------------------- G4

/// What happens to the pad note while it sounds.
#[derive(Clone, Copy, Debug)]
enum Act {
    Delete,
    ShortenBehind,
    ShortenAhead,
    Lengthen,
    Stop,
    Wrap,
    Seek,
    MoveClip,
    /// One of two copies (duplicate notes, layered clips, anonymous clips).
    DeleteOne,
    ShortenOne,
    /// The same data written again: the output must not change by a bit.
    Same,
}

/// G4: deleting, shortening or lengthening a sounding note, stopping, wrapping or seeking through
/// it, or moving its clip, leaves nothing stuck: after its release the pad is silent, and the
/// schedulers hold no note (RENDER-26a).
pub fn stuck(lab: &mut Lab, kind: Kind, o: &Opts) -> Result<Outcome> {
    let mut out = Outcome::new("G4 stuck notes", rules("stuck"), kind.name());
    // How long the pad takes to fall under -80 dBFS once its note ends, offline.
    let one = lab.project("pad", &songs::pad(Pad::One)).and_then(|d| lab.compile(&d))?;
    let end = one.frame(PAD_NOTE.1);
    let reference = offline(lab, &one, end + one.rate as u64 * 5)?;
    let release = signal::settles(span(&reference, end, u64::MAX), 480, -80.0).context("the pad never falls silent offline")?;
    out.info("pad release to -80 dBFS (offline)", format!("{:.0} ms", release as f64 * 1e3 / one.rate as f64));
    let cases = [
        (Pad::One, Act::Delete),
        (Pad::One, Act::ShortenBehind),
        (Pad::One, Act::ShortenAhead),
        (Pad::One, Act::Lengthen),
        (Pad::One, Act::Stop),
        (Pad::One, Act::Wrap),
        (Pad::One, Act::Seek),
        (Pad::One, Act::MoveClip),
        (Pad::Twice, Act::DeleteOne),
        (Pad::Twice, Act::ShortenOne),
        (Pad::Twice, Act::Same),
        (Pad::Layered, Act::DeleteOne),
        (Pad::Layered, Act::ShortenOne),
        (Pad::Layered, Act::Same),
        (Pad::Anonymous, Act::DeleteOne),
        (Pad::Anonymous, Act::ShortenOne),
        (Pad::Anonymous, Act::Same),
    ];
    for (how, act) in cases {
        let name = format!("{how:?} {act:?}");
        match stuck_case(lab, kind, o, how, act, release) {
            Ok((pass, detail)) => out.check(name, pass, detail),
            Err(e) => out.check(name, false, format!("{e:#}")),
        };
        out.rig = kind.make(lab)?.name();
    }
    Ok(out)
}

fn stuck_case(lab: &mut Lab, kind: Kind, o: &Opts, how: Pad, act: Act, release: u64) -> Result<(bool, String)> {
    let dir = lab.project("pad", &songs::pad(how))?;
    let song = lab.compile(&dir)?;
    let lane = LaneAddr::new("pad", "notes").map_err(anyhow::Error::msg)?;
    // Played from transport 0 at engine frame 0, so until a jump, a beat's engine frame is its
    // transport frame.
    let (mut deck, _) = playing(lab, kind, o, &song, 0.0, 0.0)?;
    let mut twin = if matches!(act, Act::Same) { Some(playing(lab, kind, o, &song, 0.0, 0.0)?.0) } else { None };
    for d in std::iter::once(&mut deck).chain(twin.as_mut()) {
        d.run_to(song.frame(8.0))?;
    }
    let sounding = db(rms(span(&deck.tape, deck.now() - 4_800, deck.now())));
    if sounding < -40.0 {
        bail!("the pad isn't sounding at beat 8 ({sounding:.1} dBFS)");
    }
    let mut edit = |deck: &mut Deck, change: &dyn Fn(&mut Vec<NoteClip>)| -> Result<Sent> {
        songs::edit_notes(&dir, "pad", "notes", |c| change(c))?;
        let edited = lab.compile(&dir)?;
        deck.change(&edited, Change::Data(vec![lane.clone()]))
    };
    let set_len = |c: &mut Vec<NoteClip>, clip: usize, note: usize, beats: f64| c[clip].notes[note].length = beats;
    let sent = match (act, how) {
        (Act::Delete, _) => edit(&mut deck, &|c| c[0].notes.clear())?,
        (Act::ShortenBehind, _) => edit(&mut deck, &|c| set_len(c, 0, 0, 2.0))?,
        (Act::ShortenAhead, _) => edit(&mut deck, &|c| set_len(c, 0, 0, 8.0))?,
        (Act::Lengthen, _) => edit(&mut deck, &|c| set_len(c, 0, 0, 40.0))?,
        (Act::MoveClip, _) => edit(&mut deck, &|c| c[0].start += 64.0)?,
        (Act::Stop, _) => deck.rig.send(Cmd::Stop)?,
        (Act::Wrap, _) => deck.rig.send(Cmd::Loop(Some((0.0, 16.0))))?,
        (Act::Seek, _) => deck.rig.send(Cmd::Seek(40.0))?,
        (Act::DeleteOne, Pad::Twice) => edit(&mut deck, &|c| c[0].notes.truncate(1))?,
        (Act::DeleteOne, _) => edit(&mut deck, &|c| c.truncate(1))?,
        (Act::ShortenOne, Pad::Twice) => edit(&mut deck, &|c| set_len(c, 0, 1, 8.0))?,
        (Act::ShortenOne, _) => edit(&mut deck, &|c| set_len(c, 1, 0, 8.0))?,
        (Act::Same, _) => edit(&mut deck, &|_| {})?,
    };
    deck.run(deck.period as u64)?;
    let applied = deck.rig.applied(&sent).context("never applied")?;
    // From here, the engine frame a beat plays at, by where the transport is now (an engine that
    // jumped back plays it later).
    let (e1, t1) = (deck.now(), deck.rig.probe().transport.context("the rig has no transport position")?);
    let f = |beat: f64| (e1 + song.frame(beat)).saturating_sub(t1);
    let margin = release + deck.frames(0.1);
    let level = |deck: &Deck, from: u64, to: u64| db(rms(span(&deck.tape, from, to)));
    let mut notes = vec![];
    let mut pass = true;
    // From where the pad must fall silent (its note over and released), and where it must still
    // sound before that.
    let (over, still) = match act {
        Act::Delete | Act::ShortenBehind | Act::Stop | Act::Seek | Act::MoveClip => (applied, None),
        Act::ShortenAhead => (f(12.0), Some(f(11.0))),
        Act::DeleteOne | Act::ShortenOne | Act::Same => (f(PAD_NOTE.1), Some(f(20.0))),
        Act::Wrap => {
            // The wrap at 16 ends it, and it starts again at beat 4 of the next lap.
            let (wrap, again) = (f(16.0), f(16.0) + song.frame(PAD_NOTE.0));
            deck.run_to(again)?;
            let q = level(&deck, wrap + margin, again);
            notes.push(format!("{q:.1} dBFS between the wrap's release and the next lap's note"));
            pass &= q < -80.0;
            deck.rig.send(Cmd::Loop(None))?;
            deck.rig.send(Cmd::Stop)?;
            (deck.now(), None)
        }
        Act::Lengthen => {
            deck.run_to(f(39.0))?;
            let s = level(&deck, f(37.0), f(39.0));
            notes.push(format!("{s:.1} dBFS past its old end"));
            pass &= s > -40.0;
            deck.rig.send(Cmd::Stop)?;
            (deck.now(), None)
        }
    };
    deck.run_to(over + margin + deck.frames(0.3))?;
    if let Some(t) = still {
        let s = level(&deck, t - 2_400, t);
        notes.push(format!("{s:.1} dBFS while it should still sound"));
        pass &= s > -40.0;
    }
    let q = level(&deck, over + margin, over + margin + deck.frames(0.2));
    notes.push(format!("{q:.1} dBFS after its release"));
    pass &= q < -80.0;
    if let Some(t) = &mut twin {
        t.run_to(deck.now())?;
        let same = t.tape == deck.tape;
        notes.push(format!("{} the unedited twin", if same { "bit-identical to" } else { "differs from" }));
        pass &= same;
    }
    deck.rig.send(Cmd::Stop)?;
    deck.run(margin)?;
    match deck.rig.probe().held_notes {
        Some(n) => {
            notes.push(format!("{n} notes held after stop"));
            pass &= n == 0;
        }
        None => notes.push("held notes not counted by this rig".into()),
    }
    Ok((pass, notes.join("; ")))
}

// ---------------------------------------------------------------- G5

/// The call id (`file#ordinal`) of the first call to `callee` in `file`.
fn call(session: &m2_session::Session, file: &str, callee: &str) -> Result<String> {
    let site = session
        .accepted()
        .compiled
        .sites
        .iter()
        .find(|s| s.in_project && s.file == file && s.callee == callee)
        .with_context(|| format!("no call to {callee} in {file}"))?;
    Ok(format!("{}#{}", site.file, site.ordinal))
}

/// G5: an added effect is audible within compile time + 100 ms, entered at the audio thread's own
/// position, with no jump back (RENDER-25a, RENDER-8), and heard: against a twin that activates the
/// song as it was at the same frame, what differs is the effect alone (RENDER-25).
pub fn add_effect(lab: &mut Lab, kind: Kind, o: &Opts) -> Result<Outcome> {
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let one_minute = song.beat(song.rate as u64 * 60);
    let (mut a, _) = playing(lab, kind, o, &song, one_minute, 1.0)?;
    let (mut b, _) = playing(lab, kind, o, &song, one_minute, 1.0)?;
    let mut out = Outcome::new("G5 add effect", rules("add-effect"), &format!("{} on {}", a.rig.name(), o.reference.name()));
    let mut session = lab.session(&dir)?;
    let target = call(&session, "tracks/t02.ts", "pluck")?;
    let t = Instant::now();
    session.add_effect(session.accepted().number, &target, "std/fx", "delay").map_err(|e| anyhow!("{e}"))?;
    let edited = crate::accepted(&session, t.elapsed());
    out.info("compile (not counted)", format!("{:.1} ms", edited.compile.as_secs_f64() * 1e3));
    let (e0, t0) = (a.now(), a.rig.probe().transport.context("the rig has no transport position")?);
    let work = a.rig.audio_work();
    let sent = a.change(&edited, Change::Structure)?;
    // The twin activates a renderer of the song as it was, sent at the same engine frame: both
    // end their notes and enter alike, so only the effect tells them apart.
    let twin = b.change_after(&song, Change::Structure, a.now() - e0)?;
    // The audio-heavy variant: the new renderer reuses every clip render the open made (§6.3).
    match (o.reference.stems, work, a.rig.audio_work()) {
        (0, ..) => out.skip("no audio decoded or warped again", "no stems: run with --stems 4"),
        (_, Some(w0), Some(w1)) => {
            out.check(
                "no audio decoded or warped again",
                w0 == w1,
                format!("decodes and clip renders {w0:?} after the open, {w1:?} after the edit"),
            );
        }
        _ => out.skip("no audio decoded or warped again", "this rig doesn't count them"),
    }
    a.run_secs(1.0)?;
    b.run_to(a.now())?;
    // The transport must have moved on by exactly what was played, the frames played while the
    // renderer was built included.
    let moved = a.rig.probe().transport.expect("checked") as i64 - t0 as i64;
    let jump = moved - (a.now() - e0) as i64;
    let latency = a.latency_ms(&sent).context("the activation was never applied")?;
    out.check("audible within compile + 100 ms", latency <= 100.0, format!("{latency:.1} ms after the compile"));
    out.check(
        "no jump back: the transport moved on by what was played (RENDER-25a)",
        jump == 0,
        format!(
            "{jump} frames ({:.1} ms) against what was played, {:.1} ms of it while preparing",
            jump as f64 * 1e3 / a.rate as f64,
            sent.prepare.as_secs_f64() * 1e3
        ),
    );
    let applied = a.rig.applied(&sent).expect("applied");
    if b.rig.applied(&twin) != Some(applied) {
        bail!("the twin's activation took effect at engine frame {:?}, the edit's at {applied}", b.rig.applied(&twin));
    }
    let same = a.tape[..applied as usize] == b.tape[..applied as usize];
    out.check("nothing changes before the activation", same, format!("applied at engine frame {applied}, sent at {}", sent.at));
    let diff = db(rms(&difference(span(&a.tape, applied, a.now()), span(&b.tape, applied, a.now()))));
    out.check(
        "the added effect is heard (RENDER-25)",
        diff > -60.0,
        format!("{diff:.1} dBFS of difference in the second after, from a twin that activated the song as it was"),
    );
    Ok(out)
}

// ---------------------------------------------------------------- G6

/// G6: adding or removing an effect while a sine plays makes no sample-to-sample step larger than
/// twice the sine's own largest (RENDER-25a's 5 ms crossfade). Also a sine voice, and a renderer
/// built for another tempo activated mid-note, both slower and faster. The voices play a note on
/// every beat and each activation lands inside one, so every activation switches sound: a new
/// renderer doesn't chase the note the old one ends (RENDER-11), so one made between notes, or
/// after a note held for the whole song has ended, would switch silence for silence. Across each
/// activation the beat goes on (RENDER-25a, RENDER-27).
pub fn step(lab: &mut Lab, kind: Kind, o: &Opts) -> Result<Outcome> {
    let mut out = Outcome::new("G6 crossfade step", rules("step"), kind.name());
    let pure = lab.project("sine", &songs::sine(false)).and_then(|d| lab.compile(&d))?;
    let (own, _) = signal::max_step(&offline(lab, &pure, pure.rate as u64)?);
    out.info("the sine's own largest step", format!("{own:.4}"));
    let slow = songs::with_tempo(songs::poly_sine(false), 90.0);
    let cases = [
        ("sine ↔ delay(sine)", songs::sine(false), songs::sine(true), false),
        ("sine voice ↔ delay(voice)", songs::poly_sine(false), songs::poly_sine(true), true),
        ("sine voice at 127 ↔ 90 bpm", songs::poly_sine(false), slow, true),
    ];
    let mut rng = Rng::new(o.seed);
    for (name, a, b, voice) in cases {
        let a = lab.project("step-a", &a).and_then(|d| lab.compile(&d))?;
        let b = lab.project("step-b", &b).and_then(|d| lab.compile(&d))?;
        let (mut deck, _) = playing(lab, kind, o, &a, 0.0, 0.5)?;
        out.rig = deck.rig.name();
        let (mut quiet, mut jumped, mut slower, mut faster) = (vec![], vec![], 0, 0);
        let mut now = &a;
        for i in 0..o.activations {
            let next = if i % 2 == 0 { &b } else { &a };
            if voice {
                // 0.1 to 0.6 beats into the next beat's note, which lasts 0.9.
                let t = deck.rig.probe().transport.context("the rig has no transport position")?;
                let into = now.frame(now.beat(t).floor() + 1.0 + rng.range(0.1, 0.6));
                deck.run(into - t)?;
            } else {
                deck.run_secs(rng.range(0.05, 0.4))?;
            }
            // As `Deck::change`, reading the transport just before the message goes.
            let took = deck.rig.prepare(next, &Change::Structure)?;
            deck.run(deck.frames(took.as_secs_f64()))?;
            let (sent_at, t0) = (deck.now(), deck.rig.probe().transport);
            let sent = deck.rig.commit()?;
            deck.run(deck.period as u64)?;
            let applied = deck.rig.applied(&sent).context("an activation was never applied")?;
            let level = db(rms(span(&deck.tape, applied.saturating_sub(deck.frames(0.005)), applied)));
            if level < -40.0 {
                quiet.push(format!("activation {i} at engine frame {applied}: {level:.1} dBFS in the 5 ms before"));
            }
            // The old renderer played on to the block line; the new one entered at its beat.
            if let (Some(t0), Some(t1)) = (t0, deck.rig.probe().transport) {
                let beat = now.beat(t0 + (applied - sent_at));
                let want = next.frame(beat) + (deck.now() - applied);
                if t1 != want {
                    jumped.push(format!("activation {i} at beat {beat:.3}: transport {t1}, the beat puts it at {want}"));
                }
            }
            let (from, to) = (now.data.tempo().bpm_at(0.0), next.data.tempo().bpm_at(0.0));
            slower += (to < from) as usize;
            faster += (to > from) as usize;
            now = next;
        }
        deck.run_secs(0.5)?;
        out.check(
            format!("{name}: sound at every activation (precondition)"),
            quiet.is_empty(),
            quiet.first().map_or(format!("over -40 dBFS before each of {}", o.activations), |q| format!("{} quiet; {q}", quiet.len())),
        );
        let (worst, frame) = signal::max_step(&deck.tape);
        out.check(
            format!("{name}: {} activations, largest step", o.activations),
            worst <= 2.0 * own,
            format!("{worst:.4} at engine frame {frame} (bound {:.4})", 2.0 * own),
        );
        let tempo = if slower + faster > 0 { format!(", {slower} to a slower tempo and {faster} to a faster") } else { String::new() };
        out.check(
            format!("{name}: the beat goes on across every activation (RENDER-25a, RENDER-27)"),
            jumped.is_empty(),
            jumped.first().map_or(format!("{} activations{tempo}", o.activations), |j| format!("{} jumped; {j}", jumped.len())),
        );
    }
    Ok(out)
}

// ---------------------------------------------------------------- G7

/// G7: after stop, a tail decays instead of freezing or being cut: through a reverb, a delay with
/// feedback, and a delay inside the voice whose echoes have silence between them (an engine that
/// idles on silence would lose them). The engine keeps running while stopped (RENDER-6a).
pub fn tail_stop(lab: &mut Lab, kind: Kind, o: &Opts) -> Result<Outcome> {
    let mut out = Outcome::new("G7 tail after stop", rules("tail-stop"), kind.name());
    let window = 2_400;
    for fx in [Fx::Reverb, Fx::Delay, Fx::VoiceDelay] {
        let name = format!("{fx:?}");
        let song = lab.project("tail", &songs::tail(fx)).and_then(|d| lab.compile(&d))?;
        // Stop after the note, into its tail; for the voice delay, 90 ms into its fifth echo.
        let (stop_at, echo) = match fx {
            Fx::VoiceDelay => (song.frame(132.0) + song.rate as u64 * 259 / 100, Some(song.rate as u64 / 2)),
            Fx::Delay => (song.frame(137.0), Some(song.rate as u64 / 4)),
            _ => (song.frame(137.0), None),
        };
        let from = song.frame(130.0);
        let (mut deck, play) = playing(lab, kind, o, &song, 130.0, 0.0)?;
        out.rig = deck.rig.name();
        deck.run(deck.period as u64)?;
        let played = deck.rig.applied(&play).context("play was never applied")?;
        deck.run_to(played + stop_at - from)?;
        let stop = deck.rig.send(Cmd::Stop)?;
        deck.run(deck.period as u64)?;
        let at = deck.rig.applied(&stop).context("stop was never applied")?;
        let held = deck.rig.probe().transport;
        // Offline the same tail plays on undisturbed, from the transport frame the stop froze.
        let frozen = from + (at - played);
        let reference = offline(lab, &song, frozen + song.rate as u64 * 30)?;
        let settle = signal::settles(span(&reference, frozen, u64::MAX), 4_800, -80.0).context("the tail never ends offline")?;
        let drive = settle.max(song.rate as u64 * 3) + song.rate as u64 / 2;
        deck.run_to(at + drive)?;
        let after = deck.rig.probe();

        let a = Around::of(&deck.tape, at, window);
        out.check(
            format!("{name}: rings on through the stop"),
            db(a.before) > -60.0 && a.ratio() >= 0.5,
            format!("{:.1} → {:.1} dBFS over 50 ms each side", db(a.before), db(a.after)),
        );
        let tail = span(&deck.tape, at, deck.now());
        // A feedback delay's energy per echo period falls by its feedback whatever the phase; a
        // reverb's per 100 ms.
        let rise = signal::first_rise(&signal::levels(tail, echo.unwrap_or(4_800) as usize), 1.0, -90.0);
        out.check(
            format!("{name}: only decays"),
            rise.is_none(),
            rise.map_or("no window louder than one before".into(), |(i, by)| format!("rises {by:.1} dB at window {i}")),
        );
        let last = db(rms(span(&deck.tape, deck.now() - 4_800, deck.now())));
        out.check(
            format!("{name}: falls silent"),
            last < -80.0,
            format!("{last:.1} dBFS {:.1} s after stop", drive as f64 / song.rate as f64),
        );
        let repeat = signal::frozen(tail, 128);
        out.check(
            format!("{name}: never repeats a block"),
            repeat.is_none(),
            repeat.map_or("no block repeats".into(), |f| format!("block at {} repeats", at as usize + f)),
        );
        match (held, after.transport, after.playing) {
            (Some(x), Some(y), Some(p)) => {
                out.check(format!("{name}: the transport holds while stopped"), x == y && !p, format!("{x} then {y}, playing {p}"));
            }
            _ => out.skip(format!("{name}: the transport holds while stopped"), "this rig doesn't say"),
        };
        // Window by window, the tail offline plays: an engine that idled between echoes, froze or
        // reset would lose some of it.
        let live = signal::levels(tail, 4_800);
        let off = signal::levels(span(&reference, frozen, frozen + tail.len() as u64), 4_800);
        let worst = live.iter().zip(&off).filter(|(_, r)| db(**r) > -70.0).map(|(l, r)| (db(*l) - db(*r)).abs()).fold(0.0, f64::max);
        out.check(
            format!("{name}: the tail offline plays"),
            worst <= 1.0,
            format!("within {worst:.2} dB per 100 ms window above -70 dBFS"),
        );
    }
    Ok(out)
}

// ---------------------------------------------------------------- G10

/// G10: a range render from bar 64 takes no longer than one of the same length from bar 1 (within
/// 10%), is the same bits every time, and says whether it is exact (RENDER-4, RENDER-5).
pub fn range(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    use m2_session::render::{Length, Render};
    let song = lab.reference(&o.reference).and_then(|d| lab.compile(&d))?;
    let mut out = Outcome::new("G10 range render", rules("range"), &o.reference.name());
    if !song.data.arrangement.meter.is_empty() {
        bail!("the range gate counts bars of 4 beats");
    }
    let file = lab.dir("range").join("out.wav");
    std::fs::create_dir_all(file.parent().expect("in a folder"))?;
    // Today's way to hear bar 64, replaying from 0 (RENDER-1), for the record.
    let t = Instant::now();
    let from = song.data.tempo().seconds_at(BAR_64);
    let length = song.data.tempo().seconds_at(BAR_64 + 32.0) - from;
    Render::new(&song.flat, &song.data, &lab.packages, song.rate, from, Length::Seconds(length))?.write(&file, |_| {})?;
    out.info("today's --from replay at bar 64, 8 bars", format!("{:.2} s", t.elapsed().as_secs_f64()));
    range_engine(lab, &song, &mut out)?;
    range_labels(lab, &song, &mut out, &file)?;
    Ok(out)
}

/// Bar 64's first beat, in 4/4.
const BAR_64: f64 = 63.0 * 4.0;

#[cfg(not(feature = "engine"))]
fn range_engine(_: &Lab, _: &Song, out: &mut Outcome) -> Result<()> {
    out.skip("range render timing", "built in step 3a: build with `--features engine`");
    Ok(())
}

/// The runtime's range render (`Renderer::starting_at`): `preroll` beats before `from`, then
/// `beats` of range, in frames, timed.
#[cfg(feature = "engine")]
fn range_engine(lab: &Lab, song: &Song, out: &mut Outcome) -> Result<()> {
    let render = |from: f64, preroll: f64, beats: f64| -> Result<(f64, Vec<[f32; 2]>)> {
        let t = Instant::now();
        let t0 = song.frame((from - preroll).max(0.0));
        let mut r = Renderer::starting_at(&song.flat, &song.data, &lab.packages, song.rate, t0)?;
        r.set_strict(false);
        let mut pre = vec![[0.0; 2]; (song.frame(from) - t0) as usize];
        r.process(&mut pre)?;
        let mut range = vec![[0.0; 2]; (song.frame(from + beats) - song.frame(from)) as usize];
        r.process(&mut range)?;
        Ok((t.elapsed().as_secs_f64(), range))
    };
    let (late, a) = render(BAR_64, 8.0, 32.0)?;
    let (early, _) = render(0.0, 8.0, 32.0)?;
    let (whole, _) = render(0.0, 0.0, 40.0)?;
    out.info("range from bar 64, 8 bars, 2 bars preroll", format!("{late:.2} s"));
    out.info("range from bar 1, 8 bars, 2 bars preroll", format!("{early:.2} s"));
    out.info("range from bar 1, 10 bars, no preroll", format!("{whole:.2} s"));
    out.check(
        "bar 64 takes no longer than the same rendered length from bar 1",
        late <= 1.1 * whole,
        format!("{late:.2} s against {whole:.2} s"),
    );
    let (_, b) = render(BAR_64, 8.0, 32.0)?;
    out.check("the same bits every time (RENDER-5)", a == b, "two range renders from bar 64 compared");
    let (_, exact) = render(0.0, 8.0, 32.0)?;
    let offline = offline(lab, song, song.frame(32.0))?;
    out.check("from bar 1 it is the offline render (RENDER-4)", exact == offline, "8 bars compared");
    Ok(())
}

#[cfg(not(feature = "range"))]
fn range_labels(_: &Lab, _: &Song, out: &mut Outcome, _: &std::path::Path) -> Result<()> {
    out.skip("what a range render says it is", "the session's `Render::range` isn't built: build with `--features range`");
    Ok(())
}

/// The session's range render, as `m2 render --from --preroll` makes it: what it says it is.
#[cfg(feature = "range")]
fn range_labels(lab: &Lab, song: &Song, out: &mut Outcome, file: &std::path::Path) -> Result<()> {
    use m2_session::render::{Length, Render};
    let seconds = |beat: f64| song.data.tempo().seconds_at(beat);
    let render = |from: f64| -> Result<m2_session::render::Rendered> {
        let r =
            Render::range(&song.flat, &song.data, &lab.packages, song.rate, seconds(from), seconds(8.0), Length::Seconds(seconds(32.0)))?;
        r.write(file, |_| {})
    };
    let (late, early) = (render(BAR_64)?, render(0.0)?);
    out.check("from bar 64 with preroll it says approximate (RENDER-5)", !late.exact, format!("exact: {}", late.exact));
    out.check("from bar 1 with preroll it says exact", early.exact, format!("exact: {}", early.exact));
    out.check("a pure song says reproducible", late.reproducible, format!("reproducible: {}", late.reproducible));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(engine: u64, pitch: f32) -> NoteOn {
        NoteOn { engine, transport: 0, pitch }
    }

    // RENDER-11: G1's note check holds what was heard after a seek against what the song starts
    // from the target, as multisets: a silent seek misses notes, a chased note is one no note
    // starts there, and the same note on two tracks counts twice.
    #[test]
    fn a_jump_is_held_against_the_notes_the_song_starts_there() {
        let p = |x: f32| x.to_bits();
        // The song: two tracks start A4 at 1000, C5 at 1500, and E5 at 5000 (past the window).
        let song = [(1000, p(69.0)), (1000, p(69.0)), (1500, p(72.0)), (5000, p(76.0))];
        // A seek to transport 900, applied at engine frame 100, heard until engine frame 1100.
        let all = played(&song, &[on(200, 69.0), on(200, 69.0), on(700, 72.0)], 100, 1100, 900);
        assert_eq!((all.expected.len(), all.missing.len(), all.extra.len()), (3, 0, 0));
        let silent = played(&song, &[], 100, 1100, 900);
        assert_eq!(silent.missing, [(1000, p(69.0)), (1000, p(69.0)), (1500, p(72.0))]);
        let one_track = played(&song, &[on(200, 69.0), on(700, 72.0)], 100, 1100, 900);
        assert_eq!((one_track.missing, one_track.extra), (vec![(1000, p(69.0))], vec![]));
        // A note sounding at the target turned on there: chased.
        let chased = played(&song, &[on(100, 60.0), on(200, 69.0), on(200, 69.0), on(700, 72.0)], 100, 1100, 900);
        assert_eq!((chased.missing, chased.extra), (vec![], vec![(900, p(60.0))]));
        // A note a frame late is both missed at its frame and heard where none starts.
        let late = played(&song, &[on(201, 69.0), on(200, 69.0), on(700, 72.0)], 100, 1100, 900);
        assert_eq!((late.missing, late.extra), (vec![(1000, p(69.0))], vec![(1001, p(69.0))]));
        // What was heard before the seek or after the window doesn't count.
        let outside = played(&song, &[on(99, 50.0), on(200, 69.0), on(200, 69.0), on(700, 72.0), on(1100, 50.0)], 100, 1100, 900);
        assert_eq!((outside.missing.len(), outside.extra.len()), (0, 0));
    }
}
