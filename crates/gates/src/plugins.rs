//! Step 3's gates on `examples/with-plugins` (contract, Acceptance gates: "on p16 at 1:00 and on
//! `examples/with-plugins`"): Surge XT plays the lead's notes into Airwindows Consolidated, beside
//! a std kick, at 120 bpm for 4 bars. The rig builds its renderers with the plugins the project
//! names, found on the CLAP search path (`CLAP_PATH`, else `~/Library/Audio/Plug-Ins/CLAP`);
//! without them the gate is skipped, saying why.
//!
//! One plugin instance isn't bit-exact with the next, so where the std gates compare bits these
//! check notes (the engine's trace), the notes each plugin holds, levels, and the engine's counters.
//! The checks that listen for the lead's tails play a copy whose kick doesn't play.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_song::{LaneAddr, NoteClip, Project};

use crate::gates::{Opts, SeekNotes, difference, offline, playing, unchanged};
use crate::report::Outcome;
use crate::rig::{Change, Cmd, Deck, Kind};
use crate::signal::{self, Around, Rng, Summary, db, rms, span};
use crate::{Lab, Song, fixture, load_std, songs};

/// The fixture's tempo: a beat is half a second.
const BPM: f64 = 120.0;
/// Where the lead's first chord sounds (A3 C4 E4).
const CHORD: (f64, f64) = (0.0, 3.5);

pub fn with_plugins(lab: &mut Lab, kind: Kind, o: &Opts) -> Result<Outcome> {
    let mut out = Outcome::new("with-plugins", "RENDER-6a, 8, 9, 9a, 10, 11, 25, 26, 26a; HOST-6", kind.name());
    let dir = lab.copy(&fixture("examples/with-plugins"), "with-plugins")?;
    // Compiled with a set of its own that binds the plugins: the rig builds with that set.
    let mut packages = load_std()?;
    let t = Instant::now();
    let project = Project::open(&dir).map_err(|d| anyhow!("{} doesn't open: {d:?}", dir.display()))?;
    let c = m2_session::compile(&project, &mut packages).map_err(|d| anyhow!("{} doesn't compile: {d:?}", dir.display()))?;
    if let Some(d) = c.problems.iter().find(|d| d.code == "plugin.missing") {
        out.skip("every check", format!("{d} (CLAP_PATH says where Surge XT and Airwindows are)"));
        return Ok(out);
    }
    if project.arrangement.tempo.first().map(|t| t.bpm) != Some(BPM) {
        bail!("examples/with-plugins no longer plays at {BPM} bpm, which these checks count in");
    }
    let song = Song { dir: dir.clone(), flat: c.flat, data: c.data, rate: project.sample_rate(), compile: t.elapsed() };
    let saved = std::mem::replace(&mut lab.packages, Arc::new(packages));
    let done = checks(lab, kind, o, &song, &mut out);
    lab.packages = saved;
    done.map(|()| out)
}

fn checks(lab: &mut Lab, kind: Kind, o: &Opts, song: &Song, out: &mut Outcome) -> Result<()> {
    out.rig = format!("{} on with-plugins", kind.make(lab)?.name());
    seek(lab, kind, o, song, out)?;
    looped(lab, kind, o, song, out)?;
    note_edit(lab, kind, o, song, out)?;
    // The lead alone: its tails are what these listen for.
    let lead = lab.copy(&song.dir, "lead-alone")?;
    songs::edit_notes(&lead, "kick", "hits", |c| c.clear())?;
    let lead = lab.compile(&lead)?;
    tail_cut(lab, kind, o, &lead, out)?;
    stuck(lab, kind, o, &lead, out)?;
    tail_after_stop(lab, kind, o, &lead, out)?;
    add_effect(lab, kind, o, song, out)
}

/// G1: a seek is heard within 50 ms, lands where it was asked and resets nothing; every note from
/// the target on is heard at its frame, nothing is chased, and the first note sounds.
fn seek(lab: &mut Lab, kind: Kind, o: &Opts, song: &Song, out: &mut Outcome) -> Result<()> {
    let (mut deck, _) = playing(lab, kind, o, song, 2.0, 1.0)?;
    let before = deck.rig.probe();
    let mut rng = Rng::new(o.seed);
    let (mut late, mut misplaced) = (vec![], vec![]);
    let mut notes = SeekNotes::new(song);
    for _ in 0..o.seeks.min(20) {
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
        notes.add(&deck, song, ons, target, at);
    }
    let s = Summary::of(&late);
    out.check("G1: a seek is audible within 50 ms", s.max <= 50.0, s);
    out.check("G1: it lands on its target", misplaced.is_empty(), misplaced.first().cloned().unwrap_or_else(|| "every seek".into()));
    let after = deck.rig.probe();
    unchanged(out, "G1: no reset", before.resets, after.resets);
    unchanged(out, "G1: no new renderer", before.generation, after.generation);
    notes.report(out, "G1: ");
    Ok(())
}

/// G2: the fixture's 4 bars looped for `loop_minutes`: every lap plays the first lap's notes, each
/// on the grid, with nothing reset and the plugins never rebuilt (RENDER-9a, RENDER-10).
fn looped(lab: &mut Lab, kind: Kind, o: &Opts, song: &Song, out: &mut Outcome) -> Result<()> {
    let mut deck = Deck::new(kind.make(lab)?, o.period);
    deck.rig.open(song)?;
    deck.rig.send(Cmd::Loop(Some((0.0, 16.0))))?;
    let play = deck.rig.send(Cmd::Play)?;
    deck.run(deck.period as u64)?;
    let played = deck.rig.applied(&play).context("play was never applied")?;
    let len = song.frame(16.0);
    let before = deck.rig.probe();
    let until = played + deck.frames(o.loop_minutes * 60.0);
    // What the trace kept so far, the first lap's opening notes among them.
    let mut ons = match deck.rig.ons() {
        Some(ons) => ons,
        None => {
            out.skip("G2: every lap plays the first lap's notes on the grid", "this rig keeps no note trace");
            vec![]
        }
    };
    while deck.now() < until {
        deck.run_to((deck.now() + deck.frames(4.0)).min(until))?;
        ons.extend(deck.rig.ons().unwrap_or_default());
    }
    let after = deck.rig.probe();
    unchanged(out, "G2: no reset in the loop", before.resets, after.resets);
    unchanged(out, "G2: no new renderer in the loop", before.generation, after.generation);
    if let Some(t) = after.transport {
        let want = (deck.now() - played) % len;
        out.check("G2: the transport is where the wraps put it", t == want, format!("{t}, expected {want}"));
    }
    if !ons.is_empty() {
        let laps = (deck.now() - played) / len;
        let lap = |k: u64| -> Vec<(u64, u64, u32)> {
            let lo = played + k * len;
            ons.iter().filter(|n| (lo..lo + len).contains(&n.engine)).map(|n| (n.engine - lo, n.transport, n.pitch.to_bits())).collect()
        };
        let first = lap(0);
        let differs = (1..laps).find(|k| lap(*k) != first);
        let off_grid = first.iter().find(|(e, t, _)| e != t);
        out.check(
            "G2: every lap plays the first lap's notes on the grid",
            !first.is_empty() && differs.is_none() && off_grid.is_none(),
            match (differs, off_grid) {
                (Some(k), _) => format!("lap {k} differs from the first"),
                (_, Some((e, t, _))) => format!("a note-on {e} frames into its lap for transport frame {t}"),
                _ => format!("{} note-ons a lap over {laps} laps of {:.1} min", first.len(), o.loop_minutes),
            },
        );
    }
    Ok(())
}

/// G2's tail detector on the plugins: 4 beats from inside the first chord to inside the third,
/// so each wrap ends a chord and lands where none starts (none is chased). The 50 ms after each
/// wrap is the release and the reverb of what the wrap ended, which must ring through.
fn tail_cut(lab: &mut Lab, kind: Kind, o: &Opts, lead: &Song, out: &mut Outcome) -> Result<()> {
    let window = 2_400;
    let (start, end) = (2.0, 10.0);
    let len = lead.frame(end) - lead.frame(start);
    let run = |lab: &mut Lab, panic: bool| -> Result<Vec<Around>> {
        let mut deck = Deck::new(kind.make(lab)?, o.period);
        deck.rig.open(lead)?;
        deck.rig.send(Cmd::Seek(start))?;
        deck.rig.send(Cmd::Loop(Some((start, end))))?;
        let play = deck.rig.send(Cmd::Play)?;
        deck.run(deck.period as u64)?;
        let played = deck.rig.applied(&play).context("play was never applied")?;
        let wraps: Vec<u64> = (1..=8).map(|k| played + k * len).collect();
        for w in &wraps {
            if panic {
                deck.run_to(w - w % deck.period as u64)?;
                deck.rig.send(Cmd::Panic)?;
            }
            deck.run_to(w + window + 1)?;
        }
        Ok(wraps.iter().map(|w| Around::of(&deck.tape, *w, window)).collect())
    };
    // How the same chord's release falls over 50 ms when it ends as written (beat 11.5, offline):
    // Surge's release is short, so the bound is half of that rather than half of what played.
    let reference = offline(lab, lead, lead.frame(12.0))?;
    let natural = Around::of(&reference, lead.frame(11.5), window).ratio();
    out.info("G2: a chord's 50 ms after/before when it ends as written (offline)", format!("{natural:.3}"));
    let around = run(lab, false)?;
    let quiet = around.iter().filter(|a| db(a.before) <= -60.0).count();
    out.check(
        "G2: a tail sounds before every wrap (precondition)",
        quiet == 0,
        format!("{} wraps, {quiet} quieter than -60 dBFS", around.len()),
    );
    let worst = around.iter().min_by(|a, b| a.ratio().total_cmp(&b.ratio())).expect("wraps");
    out.check(
        "G2: no tail cut at a wrap",
        worst.ratio() >= 0.5 * natural,
        format!(
            "worst after/before {:.3} ({:.1} → {:.1} dBFS), bound {:.3}",
            worst.ratio(),
            db(worst.before),
            db(worst.after),
            0.5 * natural
        ),
    );
    // A CLAP reset doesn't cut these plugins' tails (Surge XT and Airwindows ring on through it),
    // so a panic isn't the cut it is for std kernels: printed, not judged. The detector's negative
    // control runs on the std probe songs (G2).
    let cut: Vec<f64> = run(lab, true)?.iter().map(Around::ratio).collect();
    out.info("G2: after/before with a panic at each wrap", format!("{cut:.3?}"));
    Ok(())
}

/// G3: a lead note written half a beat ahead is heard within compile + 20 ms, by `SwapLane`.
fn note_edit(lab: &mut Lab, kind: Kind, o: &Opts, song: &Song, out: &mut Outcome) -> Result<()> {
    let dir = lab.copy(&song.dir, "note-edit")?;
    let song = lab.compile(&dir)?;
    let (mut a, _) = playing(lab, kind, o, &song, 1.0, 1.0)?;
    let (mut b, _) = playing(lab, kind, o, &song, 1.0, 1.0)?;
    let before = a.rig.probe();
    let (e0, t0) = (a.now(), before.transport.context("the rig has no transport position")?);
    let beat = song.beat(t0) + 0.5;
    songs::edit_notes(&dir, "lead", "notes", |c: &mut Vec<NoteClip>| {
        let c = c.iter_mut().find(|c| c.start <= beat && beat + 0.5 < c.start + c.length).expect("the lead's clip holds the song");
        c.notes.push(songs::note(57.0, beat - c.start, 0.5));
    })?;
    let edited = lab.compile(&dir)?;
    let lane = LaneAddr::new("lead", "notes").map_err(anyhow::Error::msg)?;
    let sent = a.change(&edited, Change::Data(vec![lane]))?;
    let on = e0 + (song.frame(beat) - t0);
    a.run_to(on.max(a.now()) + a.frames(1.0))?;
    b.run_to(a.now())?;
    let latency = a.latency_ms(&sent).context("the edit was never applied")?;
    out.check("G3: a note edit is audible within compile + 20 ms", latency <= 20.0, format!("{latency:.1} ms after the compile"));
    out.check("G3: it took SwapLane, not a new renderer", sent.swap, if sent.swap { "swapped" } else { "rebuilt" });
    let after = a.rig.probe();
    unchanged(out, "G3: no reset", before.resets, after.resets);
    unchanged(out, "G3: no new renderer", before.generation, after.generation);
    // Up to the note, the edited engine plays what the unedited twin does (no tail cut); from it,
    // the note makes a difference as loud as a note of the lead's: its first chord, offline.
    let diff = |from: u64, to: u64| -> f64 {
        let d: Vec<[f32; 2]> =
            span(&a.tape, from, to).iter().zip(span(&b.tape, from, to)).map(|(x, y)| [x[0] - y[0], x[1] - y[1]]).collect();
        db(rms(&d))
    };
    let (before_note, at_note) = (diff(e0, on), diff(on, on + 4_800));
    let chord = db(rms(span(&offline(lab, &song, song.frame(1.0))?, song.frame(0.5), song.frame(1.0))));
    out.check(
        "G3: no tail cut: the same output as unedited until the new note",
        before_note < at_note - 40.0,
        format!("{before_note:.1} dBFS of difference before it, {at_note:.1} from it"),
    );
    out.check(
        "G3: the new note is heard where it was written",
        at_note > chord - 20.0,
        format!("{at_note:.1} dBFS of difference in its first 100 ms; the lead's first chord plays at {chord:.1} dBFS"),
    );
    Ok(())
}

/// What happens to the lead's first chord while it sounds, at beat 1. The edits leave no note
/// after it, so the lead must then fall silent.
#[derive(Clone, Copy, Debug)]
enum Act {
    /// Every note deleted.
    Delete,
    /// The first chord shortened to half a beat, behind the playhead, and the rest deleted.
    ShortenBehind,
    Stop,
    Seek,
    Wrap,
    MoveClip,
    /// Every note off and every instance reset (RENDER-10): a plugin must let go of its notes too.
    Panic,
}

/// G4: deleting or shortening the sounding chord, stopping, seeking or wrapping through it, moving
/// its clip, or a panic leaves no note held in Surge XT, and the lead falls silent once its release
/// and reverb have run their course (measured offline when the last chord ends as written).
fn stuck(lab: &mut Lab, kind: Kind, o: &Opts, lead: &Song, out: &mut Outcome) -> Result<()> {
    let end = lead.frame(15.5);
    let reference = offline(lab, lead, lead.frame(40.0))?;
    let floor = -60.0;
    let Some(release) = signal::settles(span(&reference, end, u64::MAX), 480, floor) else {
        let l: Vec<String> = signal::levels(span(&reference, end, u64::MAX), 12_000).iter().map(|v| format!("{:.1}", db(*v))).collect();
        out.check(
            "G4: the last chord's release falls silent offline (precondition)",
            false,
            format!("not under {floor} dBFS in 12 s: {} dBFS per 250 ms", l.join(", ")),
        );
        return Ok(());
    };
    out.info("G4: the last chord's release to -60 dBFS (offline)", format!("{:.0} ms", release as f64 * 1e3 / lead.rate as f64));
    for act in [Act::Delete, Act::ShortenBehind, Act::Stop, Act::Seek, Act::Wrap, Act::MoveClip, Act::Panic] {
        let (pass, detail) = stuck_case(lab, kind, o, lead, act, release, floor)?;
        out.check(format!("G4: {act:?}"), pass, detail);
    }
    Ok(())
}

fn stuck_case(lab: &mut Lab, kind: Kind, o: &Opts, lead: &Song, act: Act, release: u64, floor: f64) -> Result<(bool, String)> {
    let dir = lab.copy(&lead.dir, "stuck")?;
    let song = lab.compile(&dir)?;
    let (mut deck, _) = playing(lab, kind, o, &song, 0.0, 0.0)?;
    deck.run_to(song.frame(1.0))?;
    let held = deck.rig.probe().held_notes;
    let lane = LaneAddr::new("lead", "notes").map_err(anyhow::Error::msg)?;
    let mut edit = |deck: &mut Deck, change: &dyn Fn(&mut Vec<NoteClip>)| -> Result<crate::rig::Sent> {
        songs::edit_notes(&dir, "lead", "notes", |c| change(c))?;
        let edited = lab.compile(&dir)?;
        deck.change(&edited, Change::Data(vec![lane.clone()]))
    };
    let sent = match act {
        Act::Delete => edit(&mut deck, &|c| c[0].notes.clear())?,
        Act::ShortenBehind => edit(&mut deck, &|c| {
            c[0].notes.retain(|n| n.start == CHORD.0);
            for n in &mut c[0].notes {
                n.length = 0.5;
            }
        })?,
        Act::Panic => deck.rig.send(Cmd::Panic)?,
        Act::MoveClip => edit(&mut deck, &|c| c[0].start += 64.0)?,
        Act::Stop => deck.rig.send(Cmd::Stop)?,
        // Past the last chord, where nothing plays.
        Act::Seek => deck.rig.send(Cmd::Seek(15.75))?,
        Act::Wrap => deck.rig.send(Cmd::Loop(Some((1.0, 3.0))))?,
    };
    // A wrap ends the chord at beat 3 and lands at 1, where none starts.
    let at = match act {
        Act::Wrap => {
            deck.run_to(song.frame(3.0))?;
            song.frame(3.0)
        }
        _ => {
            deck.run(deck.period as u64)?;
            deck.rig.applied(&sent).context("never applied")?
        }
    };
    deck.run(deck.period as u64)?;
    let after = deck.rig.probe().held_notes;
    // 200 ms once released. A panic lets the chord play on after it, from where it was: stop
    // first, as the app's panic button would leave it.
    if matches!(act, Act::Panic) {
        deck.rig.send(Cmd::Stop)?;
    }
    let (quiet_from, quiet_to) = (at + release + deck.frames(0.1), at + release + deck.frames(0.3));
    deck.run_to(quiet_to)?;
    let q = db(rms(span(&deck.tape, quiet_from, quiet_to)));
    let pass = held.is_some_and(|h| h > 0) && after == Some(0) && q < floor;
    let l: Vec<String> = signal::levels(span(&deck.tape, at, quiet_to), 4_800).iter().map(|v| format!("{:.0}", db(*v))).collect();
    Ok((pass, format!("Surge held {held:?} notes before, {after:?} after; {q:.1} dBFS once released ({} per 100 ms)", l.join(" "))))
}

/// G7: after stop, the plugins' tail rings on and decays, nothing freezes, the transport holds,
/// and Surge holds no note.
fn tail_after_stop(lab: &mut Lab, kind: Kind, o: &Opts, lead: &Song, out: &mut Outcome) -> Result<()> {
    let window = 2_400;
    let (mut deck, play) = playing(lab, kind, o, lead, 0.0, 0.0)?;
    deck.run(deck.period as u64)?;
    let played = deck.rig.applied(&play).context("play was never applied")?;
    deck.run_to(played + lead.frame(2.0))?;
    let stop = deck.rig.send(Cmd::Stop)?;
    deck.run(deck.period as u64)?;
    let at = deck.rig.applied(&stop).context("stop was never applied")?;
    let held = deck.rig.probe();
    deck.run_to(at + deck.frames(3.0))?;
    let after = deck.rig.probe();
    let a = Around::of(&deck.tape, at, window);
    out.check(
        "G7: the plugins ring on through the stop",
        db(a.before) > -60.0 && a.ratio() >= 0.5,
        format!("{:.1} → {:.1} dBFS over 50 ms each side", db(a.before), db(a.after)),
    );
    let tail = span(&deck.tape, at, deck.now());
    // A plugin reverb's tail wobbles by a few dB from one 100 ms to the next, and by a different
    // amount in each instance (Airwindows modulates it), so it is judged over 500 ms windows with
    // the std gates' 1 dB: it only falls, and by at least 10 dB over the 3 s (it falls about 7 dB a
    // second; a frozen or held tail, not at all).
    let levels = signal::levels(tail, 24_000);
    let rise = signal::first_rise(&levels, 1.0, -90.0);
    out.check(
        "G7: the tail only decays (per 500 ms)",
        rise.is_none(),
        rise.map_or("no window louder than one before".into(), |(i, by)| format!("rises {by:.1} dB at window {i}")),
    );
    let fell = db(levels[0]) - db(*levels.last().expect("3 s of tail"));
    out.check(
        "G7: it falls at least 10 dB in 3 s",
        fell >= 10.0,
        format!("{fell:.1} dB, to {:.1} dBFS", db(*levels.last().expect("3 s of tail"))),
    );
    let repeat = signal::frozen(tail, 128);
    out.check(
        "G7: never repeats a block",
        repeat.is_none(),
        repeat.map_or("no block repeats".into(), |f| format!("block at {} repeats", at as usize + f)),
    );
    match (held.transport, after.transport, after.playing) {
        (Some(x), Some(y), Some(p)) => {
            out.check("G7: the transport holds while stopped", x == y && !p, format!("{x} then {y}, playing {p}"));
        }
        _ => out.skip("G7: the transport holds while stopped", "this rig doesn't say"),
    }
    if let Some(n) = after.held_notes {
        out.check("G7: Surge holds no note after stop", n == 0, format!("{n} held"));
    }
    Ok(())
}

/// G5: an effect added while playing is heard within compile + 100 ms, with no jump back. Step 3a
/// builds every plugin instance again with each new renderer (carrying them across is step 3b's
/// `PluginHost`), and the contract's code-plane budget leaves plugin loads out: what that took is
/// printed beside the latency.
fn add_effect(lab: &mut Lab, kind: Kind, o: &Opts, song: &Song, out: &mut Outcome) -> Result<()> {
    let dir = lab.copy(&song.dir, "add-effect")?;
    let song = lab.compile(&dir)?;
    let (mut a, _) = playing(lab, kind, o, &song, 1.0, 1.0)?;
    let mut session = lab.session(&dir)?;
    let target = kick_call(&session, &dir)?;
    let t = Instant::now();
    session.add_effect(session.accepted().number, &target, "std/fx", "delay").map_err(|e| anyhow!("{e}"))?;
    let edited = crate::accepted(&session, t.elapsed());
    let (e0, t0) = (a.now(), a.rig.probe().transport.context("the rig has no transport position")?);
    // What building the plugins alone takes: the same renderer, built again.
    let t = Instant::now();
    drop(m2_runtime::Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?);
    let plugins_build = t.elapsed();
    let sent = a.change(&edited, Change::Structure)?;
    a.run_secs(1.0)?;
    let moved = a.rig.probe().transport.expect("checked") as i64 - t0 as i64;
    let jump = moved - (a.now() - e0) as i64;
    let latency = a.latency_ms(&sent).context("the activation was never applied")?;
    out.info(
        "G5: building a renderer for the song as it was (its two plugins included)",
        format!("{:.1} ms", plugins_build.as_secs_f64() * 1e3),
    );
    out.check("G5: an added effect is audible within compile + 100 ms", latency <= 100.0, format!("{latency:.1} ms after the compile"));
    out.check("G5: no jump back", jump == 0, format!("{jump} frames against what was played"));
    let held = a.rig.probe().held_notes;
    out.info("G5: notes Surge holds after the activation", format!("{held:?} (the chord it cut isn't chased)"));
    effect_heard(lab, kind, o, &song.dir, out)
}

/// G5's effect is heard: the same edit on a copy whose lead doesn't play, beside two twins that
/// each activate it as it was at the same engine frame. The plugins are built and run all the
/// same; with the lead silent, what one instance differs from the next (Surge's random phases,
/// tens of dB above nothing while it plays) is no longer in the way. The edit differs from a twin
/// by the effect; the twins differ by nothing but that instance noise, the floor.
fn effect_heard(lab: &mut Lab, kind: Kind, o: &Opts, from: &Path, out: &mut Outcome) -> Result<()> {
    let dir = lab.copy(from, "kick-alone")?;
    songs::edit_notes(&dir, "lead", "notes", |c| c.clear())?;
    let song = lab.compile(&dir)?;
    let mut decks = vec![];
    for _ in 0..3 {
        decks.push(playing(lab, kind, o, &song, 1.0, 1.0)?.0);
    }
    let mut session = lab.session(&dir)?;
    let target = kick_call(&session, &dir)?;
    session.add_effect(session.accepted().number, &target, "std/fx", "delay").map_err(|e| anyhow!("{e}"))?;
    let edited = crate::accepted(&session, std::time::Duration::ZERO);
    let e0 = decks[0].now();
    let sent = decks[0].change(&edited, Change::Structure)?;
    let frames = decks[0].now() - e0;
    let twins = [decks[1].change_after(&song, Change::Structure, frames)?, decks[2].change_after(&song, Change::Structure, frames)?];
    decks[0].run_secs(1.0)?;
    let until = decks[0].now();
    for d in &mut decks[1..] {
        d.run_to(until)?;
    }
    let applied = decks[0].rig.applied(&sent).context("the activation was never applied")?;
    let at: Vec<Option<u64>> = decks[1..].iter_mut().zip(&twins).map(|(d, s)| d.rig.applied(s)).collect();
    if at.iter().any(|a| *a != Some(applied)) {
        bail!("the twins' activations took effect at engine frames {at:?}, the edit's at {applied}");
    }
    let diff = |x: &Deck, y: &Deck| db(rms(&difference(span(&x.tape, applied, until), span(&y.tape, applied, until))));
    let (heard, floor) = (diff(&decks[0], &decks[1]), diff(&decks[1], &decks[2]));
    out.check(
        "G5: the added effect is heard (RENDER-25)",
        heard > -60.0 && heard > floor + 20.0,
        format!(
            "with the lead silent, {heard:.1} dBFS of difference in the second after from a twin that activated the song as it was; \
             two such twins differ by {floor:.1}"
        ),
    );
    Ok(())
}

fn kick_call(session: &m2_session::Session, dir: &Path) -> Result<String> {
    let site = session
        .accepted()
        .compiled
        .sites
        .iter()
        .find(|s| s.in_project && s.file == "tracks/kick.ts" && s.callee == "kick")
        .with_context(|| format!("no call to kick in {}", dir.join("tracks/kick.ts").display()))?;
    Ok(format!("{}#{}", site.file, site.ordinal))
}
