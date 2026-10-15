//! Step 3's gates on `examples/with-plugins` (contract, Acceptance gates: "on p16 at 1:00 and on
//! `examples/with-plugins`"): Surge XT plays the lead's notes into Airwindows Consolidated, beside
//! a std kick, at 120 bpm for 4 bars. The rig builds its renderers with the plugins the project
//! names, found on the CLAP search path (`CLAP_PATH`, else the OS's CLAP folders);
//! without them the gate is skipped, saying why.
//!
//! One plugin instance isn't bit-exact with the next, so where the std gates compare bits these
//! check notes (the engine's trace), the notes each plugin holds, levels, and the engine's counters.
//! The checks that listen for the lead's tails play a copy whose kick doesn't play.
//!
//! Plugin instances persist across plans (HOST-5, RENDER-25): the engine rig prepares each plan as
//! the server does, against the plan last sent, and these check that seeks, loops and swaps keep
//! every instance, that an edit keeps the ones its code still names with the notes sounding into
//! them, and what must not be kept. `--no-arena` is the negative reference.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::Gestures;
use m2_song::{LaneAddr, NoteClip};

use crate::gates::{Opts, SeekNotes, difference, offline, playing, unchanged};
use crate::report::Outcome;
use crate::rig::{Change, Cmd, Deck, Kind, Plugins};
use crate::signal::{self, Around, Rng, Summary, db, rms, span};
use crate::{Lab, Song, fixture, load_std, songs};

/// The fixture's tempo: a beat is half a second.
const BPM: f64 = 120.0;
/// Where the lead's first chord sounds (A3 C4 E4).
const CHORD: (f64, f64) = (0.0, 3.5);

pub fn with_plugins(lab: &mut Lab, kind: Kind, o: &Opts) -> Result<Outcome> {
    let mut out = Outcome::new("with-plugins", "RENDER-6a, 8, 9, 9a, 10, 11, 25, 26, 26a; HOST-2, 5, 6", kind.name());
    let dir = lab.copy(&fixture("examples/with-plugins"), "with-plugins")?;
    // Compiled with a set of its own that binds the plugins: the rig builds with that set.
    let mut packages = load_std()?;
    let t = Instant::now();
    let (project, snapshot) = crate::open(&dir)?;
    let c = m2_session::compile(&project, &mut packages).map_err(|d| anyhow!("{} doesn't compile: {d:?}", dir.display()))?;
    if let Some(d) = c.problems.iter().find(|d| d.code == "plugin.missing") {
        out.skip("every check", format!("{d} (CLAP_PATH says where Surge XT and Airwindows are)"));
        return Ok(out);
    }
    if project.arrangement.tempo.first().map(|t| t.bpm) != Some(BPM) {
        bail!("examples/with-plugins no longer plays at {BPM} bpm, which these checks count in");
    }
    let song = Song::of(&dir, c, snapshot, project.sample_rate(), t.elapsed());
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
    add_effect(lab, kind, o, song, out)?;
    kept(lab, kind, o, &lead, out)?;
    removed(lab, kind, o, &lead, out)?;
    inside(lab, kind, o, &lead, out)?;
    new_state(lab, kind, o, &lead, out)?;
    no_leak(lab, kind, o, &lead, out)
}

/// The rig's plugin instances must be the same ones, and none created (HOST-5, RENDER-10).
fn same_instances(out: &mut Outcome, what: &str, before: &Option<Plugins>, after: &Option<Plugins>) {
    match (before, after) {
        (Some(a), Some(b)) => {
            let handles = |p: &Plugins| p.nodes.iter().map(|n| n.handle).collect::<Vec<_>>();
            out.check(
                what,
                handles(a) == handles(b) && a.created == b.created,
                format!(
                    "{} instances, the same: {}; {} created in between",
                    a.nodes.len(),
                    handles(a) == handles(b),
                    b.created - a.created
                ),
            );
        }
        _ => out.skip(what, "this rig doesn't say"),
    }
}

/// G1: a seek is heard within 50 ms, lands where it was asked and resets nothing; every note from
/// the target on is heard at its frame, nothing is chased, and the first note sounds.
fn seek(lab: &mut Lab, kind: Kind, o: &Opts, song: &Song, out: &mut Outcome) -> Result<()> {
    let (mut deck, _) = playing(lab, kind, o, song, 2.0, 1.0)?;
    let before = deck.rig.probe();
    let plugins_before = deck.rig.plugins();
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
    same_instances(out, "G1: every plugin instance kept through the seeks", &plugins_before, &deck.rig.plugins());
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
    let plugins_before = deck.rig.plugins();
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
    same_instances(out, "G2: every plugin instance kept through the wraps", &plugins_before, &deck.rig.plugins());
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
    let plugins_before = a.rig.plugins();
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
    same_instances(out, "G3: every plugin instance kept through the swap", &plugins_before, &a.rig.plugins());
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
    // RENDER-6a: an engine may idle only once every plugin says it sleeps. Surge XT and Airwindows
    // never do (step 3b §1.2), so this one never idles: recorded, not assumed.
    if let Some(p) = deck.rig.plugins() {
        let says: Vec<String> = p.nodes.iter().map(|n| format!("{:?}", n.status)).collect();
        out.info("G7: the engine idled after stop", format!("{} (the plugins last said {})", p.idled, says.join(", ")));
    }
    Ok(())
}

/// G5: an effect added while playing is heard within compile + 100 ms, with no jump back. The
/// contract's code-plane budget leaves plugin loads out; step 3b carries the instances, so the
/// activation builds none: what building them takes is printed beside the latency, for the record.
fn add_effect(lab: &mut Lab, kind: Kind, o: &Opts, song: &Song, out: &mut Outcome) -> Result<()> {
    let dir = lab.copy(&song.dir, "add-effect")?;
    let song = lab.compile(&dir)?;
    let (mut a, _) = playing(lab, kind, o, &song, 1.0, 1.0)?;
    let mut session = lab.session(&dir)?;
    let target = kick_call(&session, &dir)?;
    let t = Instant::now();
    session.add_effect(session.accepted().number, &target, "std:delay").map_err(|e| anyhow!("{e}"))?;
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
    session.add_effect(session.accepted().number, &target, "std:delay").map_err(|e| anyhow!("{e}"))?;
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

// ---------------------------------------------------------------- instances kept (HOST-5)

/// Wait up to a second for the plugin main thread to destroy what was dropped.
fn settled(f: impl Fn() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed().as_secs_f64() < 1.0 {
        if f() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    f()
}

/// The lead playing from 0 on a copy of `lead` (its kick silent), at the engine frame transport 0
/// played, its plugins, and the copy's folder.
fn lead_deck(lab: &mut Lab, kind: Kind, o: &Opts, lead: &Song, name: &str) -> Result<(Deck, u64, Song)> {
    let dir = lab.copy(&lead.dir, name)?;
    let song = lab.compile(&dir)?;
    let (mut deck, play) = playing(lab, kind, o, &song, 0.0, 0.0)?;
    deck.run(deck.period as u64)?;
    let played = deck.rig.applied(&play).context("play was never applied")?;
    Ok((deck, played, song))
}

/// Surge's node: the one the chord sounds into.
fn surge(p: &Plugins) -> Option<&m2_runtime::PluginView> {
    p.nodes.iter().find(|n| !n.notes.is_empty())
}

/// Step 3b, G5 on the plugins (HOST-5, RENDER-25, C7): an effect added on the kick at beat 1.5, the
/// lead's first chord held (beats 0 to 3.5), keeps both plugin instances. None is created; Surge's
/// node holds the same notes with the same ids after the activation; no NoteOff reaches it there,
/// and the chord's Offs arrive at beat 3.5; the lead's level over the 200 ms after the activation is
/// at least 0.9 times the 200 ms before, relative to the same windows offline; and no step across
/// the crossfade is larger than twice the chord's own (G6's rule). With `--no-arena` the level check
/// must fail: a fresh Surge chases nothing, and the old one fades out.
fn kept(lab: &mut Lab, kind: Kind, o: &Opts, lead: &Song, out: &mut Outcome) -> Result<()> {
    let (mut deck, played, song) = lead_deck(lab, kind, o, lead, "kept")?;
    let Some(_) = deck.rig.plugins() else {
        out.skip("G5 (3b): plugin instances kept", "this rig doesn't say which instances it plays");
        return Ok(());
    };
    let mut session = lab.session(&song.dir)?;
    let target = kick_call(&session, &song.dir)?;
    let t = Instant::now();
    session.add_effect(session.accepted().number, &target, "std:delay").map_err(|e| anyhow!("{e}"))?;
    let edited = crate::accepted(&session, t.elapsed());
    deck.run_to(played + song.frame(1.5))?;
    let pre = deck.rig.plugins().expect("checked");
    let sent = deck.change(&edited, Change::Structure)?;
    deck.run(deck.period as u64)?;
    let at = deck.rig.applied(&sent).context("the activation was never applied")?;
    let first = deck.rig.plugins().expect("checked");
    out.info("G5 (3b): preparing the activation, plugins kept", format!("{:.1} ms", sent.prepare.as_secs_f64() * 1e3));
    same_instances(out, "G5 (3b): the edit's plan kept every plugin instance, creating none", &Some(pre.clone()), &Some(first.clone()));
    deck.run_to(played + song.frame(3.75))?;
    // The lead's level over 200 ms each side of the activation (its chorus and unison move it by
    // several dB from one 5 ms to the next), against the same windows offline, in one instance.
    let w = deck.frames(0.2);
    let ratio = |tape: &[[f32; 2]], at: u64| rms(span(tape, at, at + w)) / rms(span(tape, at - w, at));
    let natural = ratio(&offline(lab, &song, song.frame(3.5))?, at - played);
    let r = ratio(&deck.tape, at);
    out.check(
        "G5 (3b): the lead plays on at its level through the activation",
        r >= 0.9 * natural,
        format!("after/before over 200 ms: {r:.3}; the same song offline, {natural:.3} (at least 0.9 times that)"),
    );
    // G6's rule on the plugin path (RENDER-25a): across the activation's 5 ms crossfade, no
    // sample-to-sample step larger than twice the largest of the held chord just before it.
    let before = signal::max_step(span(&deck.tape, at - w, at)).0;
    let across = signal::max_step(span(&deck.tape, at, at + deck.frames(0.02))).0;
    out.check(
        "G5 (3b): no step across the activation larger than twice the chord's own before it (G6's rule)",
        across > 0.0 && across <= 2.0 * before,
        format!("largest step {across:.4} across, {before:.4} over the 200 ms before (bound {:.4})", 2.0 * before),
    );
    let s1 = surge(&pre).and_then(|s0| first.nodes.iter().find(|n| n.handle == s0.handle).map(|s1| (s0, s1)));
    let Some((s0, s1)) = s1 else {
        out.check(
            "G5 (3b): the chord stays held through the activation",
            false,
            "Surge's node holds no chord before the edit, or is gone after it",
        );
        return Ok(());
    };
    out.check(
        "G5 (3b): the chord stays held through the activation, with the same note ids",
        s0.notes == s1.notes && s1.offs == 0,
        format!("{:?} before, {:?} after; {} NoteOffs sent to Surge at the activation", s0.notes, s1.notes, s1.offs),
    );
    let end = deck.rig.plugins().expect("checked");
    let s2 = end.nodes.iter().find(|n| n.handle == s1.handle);
    let want = played + song.frame(3.5);
    out.check(
        "G5 (3b): the chord's NoteOffs arrive at its end (beat 3.5)",
        s2.is_some_and(|s| s.offs == s0.notes.len() as u64 && s.last_off == Some(want)),
        format!("{:?} NoteOffs, the last at {:?}; the chord ends at engine frame {want}", s2.map(|s| s.offs), s2.map(|s| s.last_off)),
    );
    Ok(())
}

/// Step 3b (RENDER-26a, C7): one revision changes the lead's code and drops the chord's top note.
/// The instances are kept; the dropped note's NoteOff reaches Surge at the activation's first
/// frame, the other two go on to beat 3.5, and the next chord's notes take ids above every one Surge
/// had (CLAP matches voices by id).
fn removed(lab: &mut Lab, kind: Kind, o: &Opts, lead: &Song, out: &mut Outcome) -> Result<()> {
    let (mut deck, played, song) = lead_deck(lab, kind, o, lead, "removed")?;
    deck.run_to(played + song.frame(1.5))?;
    let Some(pre) = deck.rig.plugins() else {
        out.skip("removed note (3b)", "this rig doesn't say which instances it plays");
        return Ok(());
    };
    let Some(s0) = surge(&pre).cloned() else { bail!("Surge holds no chord at beat 1.5") };
    let top = *s0.notes.iter().max_by_key(|n| n.1).expect("a chord");
    let lead_ts = song.dir.join("tracks/lead.ts");
    let code = std::fs::read_to_string(&lead_ts)?;
    std::fs::write(&lead_ts, format!("{code}// edited with its notes\n"))?;
    songs::edit_notes(&song.dir, "lead", "notes", |c| {
        c[0].notes.retain(|n| !(n.start == CHORD.0 && n.pitch.round() as i16 == top.1));
    })?;
    let edited = lab.compile(&song.dir)?;
    let sent = deck.change(&edited, Change::Structure)?;
    deck.run(deck.period as u64)?;
    let at = deck.rig.applied(&sent).context("the activation was never applied")?;
    let first = deck.rig.plugins().expect("checked");
    same_instances(out, "removed note (3b): the instances are kept", &Some(pre.clone()), &Some(first.clone()));
    let s1 = first.nodes.iter().find(|n| n.handle == s0.handle).cloned().unwrap_or_else(|| s0.clone());
    let kept: Vec<(u32, i16)> = s0.notes.iter().copied().filter(|n| *n != top).collect();
    out.check(
        "removed note (3b): its NoteOff reaches Surge at the activation, the others go on",
        s1.offs == 1 && s1.last_off == Some(at) && s1.notes == kept,
        format!(
            "{} NoteOff(s), the last at {:?} (activation at {at}); Surge holds {:?}, expected {kept:?}",
            s1.offs, s1.last_off, s1.notes
        ),
    );
    deck.run_to(played + song.frame(4.5))?;
    let next = deck.rig.plugins().expect("checked");
    let had = s0.notes.iter().map(|n| n.0).max().unwrap_or(0);
    let ids: Vec<u32> =
        next.nodes.iter().find(|n| n.handle == s0.handle).map(|n| n.notes.iter().map(|x| x.0).collect()).unwrap_or_default();
    out.check(
        "removed note (3b): ids never repeat into the kept instance",
        !ids.is_empty() && ids.iter().all(|i| *i > had),
        format!("the next chord's ids {ids:?}, every one Surge had up to {had}"),
    );
    Ok(())
}

/// Step 3b (HOST-5, HOME-4): a code edit inside the lead (Airwindows' dry/wet, 0.35 to 0.9) keeps
/// both instances, and the new value reaches the kept instance at its first call.
fn inside(lab: &mut Lab, kind: Kind, o: &Opts, lead: &Song, out: &mut Outcome) -> Result<()> {
    let (mut deck, played, song) = lead_deck(lab, kind, o, lead, "inside")?;
    deck.run_to(played + song.frame(1.0))?;
    let Some(pre) = deck.rig.plugins() else {
        out.skip("code edit inside the lead (3b)", "this rig doesn't say which instances it plays");
        return Ok(());
    };
    let lead_ts = song.dir.join("tracks/lead.ts");
    let code = std::fs::read_to_string(&lead_ts)?;
    if !code.contains("dry_wet_2944917344: 0.35") {
        bail!("tracks/lead.ts no longer sets dry_wet_2944917344 to 0.35");
    }
    std::fs::write(&lead_ts, code.replace("dry_wet_2944917344: 0.35", "dry_wet_2944917344: 0.9"))?;
    let edited = lab.compile(&song.dir)?;
    deck.change(&edited, Change::Structure)?;
    deck.run_secs(0.1)?;
    let after = deck.rig.plugins().expect("checked");
    same_instances(out, "code edit inside the lead (3b): both instances kept", &Some(pre.clone()), &Some(after.clone()));
    let air = after.nodes.iter().find(|n| n.notes.is_empty() && Some(n.handle) != surge(&pre).map(|s| s.handle));
    let value = air.and_then(|n| match m2_plugins::host().main_call(n.handle, m2_plugins::MainCall::Value(2_944_917_344)) {
        Ok(m2_plugins::Reply::Value(v)) => v,
        _ => None,
    });
    out.check(
        "code edit inside the lead (3b): the new value reached the kept instance",
        value.is_some_and(|v| (v - 0.9).abs() < 1e-6),
        format!("Airwindows' dry/wet reads {value:?}"),
    );
    Ok(())
}

/// Surge XT's filter 1 type, which `tracks/lead.ts` sets to 0.05 ("LP 24 dB").
const FILTER_TYPE: u32 = 1_537_728_286;

/// One of a plugin's parameters, as it reads now (on the plugin main thread).
fn param(h: m2_plugins::Handle, id: u32) -> Option<f64> {
    match m2_plugins::host().main_call(h, m2_plugins::MainCall::Value(id)) {
        Ok(m2_plugins::Reply::Value(v)) => v,
        _ => None,
    }
}

/// Step 7 (RENDER-26b, HOME-6, HOST-5): a changed state blob (another Surge patch, its volume turned
/// down) is loaded into the Surge that plays as the plan is prepared, as the server does: the same
/// instance and Airwindows kept, none created, exactly one state load, and Surge's volume then
/// reads the new patch's. Then Surge's own state, saved and committed (as an autosave will), loads
/// nothing: the host skips the state the instance holds (HOME-6).
fn new_state(lab: &mut Lab, kind: Kind, o: &Opts, lead: &Song, out: &mut Outcome) -> Result<()> {
    let (mut deck, played, song) = lead_deck(lab, kind, o, lead, "new-state")?;
    if deck.rig.plugins().is_none() || deck.rig.arena().is_none() {
        out.skip("changed state", "this rig doesn't say which instances it plays");
        return Ok(());
    }
    // Another patch: a fresh Surge with its volume turned down, saved through the host (a fresh
    // Surge alone saves the bytes the fixture holds), and destroyed before anything is counted.
    let host = m2_plugins::host();
    let found = m2_plugins::find("org.surge-synth-team.surge-xt")?;
    let gone = m2_plugins::InProcess::counts().1;
    let (h, info) = host.create(&found, None, &m2_plugins::Owner::discard())?;
    let volume = info.params.iter().find(|p| !p.stepped && p.name.contains("Volume")).context("Surge XT has a volume parameter")?;
    let want = volume.min + (volume.max - volume.min) * 0.3;
    let blob = host.activate(h, song.rate, 128).and_then(|_| {
        let (mut l, mut r, silence) = (vec![0.0f32; 128], vec![0.0f32; 128], vec![0.0f32; 128]);
        let mut io = m2_plugins::SharedBuffers { inputs: [&silence, &silence], n_in: 2, outputs: [&mut l, &mut r], n_out: 2, frames: 128 };
        let mut ev = m2_plugins::Events::with_capacity(1);
        ev.push(m2_plugins::Event::Param { time: 0, id: volume.id, value: want });
        host.process(h, &mut io, &ev, &mut m2_plugins::Events::with_capacity(16), &m2_plugins::Transport::default());
        host.save_state(h).recv()?.map_err(|e| anyhow!("{e}"))
    });
    host.destroy(h);
    if !settled(|| m2_plugins::InProcess::counts().1 > gone) {
        bail!("the patch's Surge was never destroyed");
    }
    std::fs::write(song.dir.join("data/lead/lead.state"), blob?.bytes)?;
    let edited = lab.compile(&song.dir)?;
    let hash = |s: &Song| s.data.state_hash("lead", &["lead"]).map(|h| h.hex()[..8].to_string());
    out.info("changed state: the blob's hash", format!("{:?} before, {:?} after", hash(&song), hash(&edited)));
    deck.run_to(played + song.frame(1.0))?;
    let pre = deck.rig.plugins().expect("checked");
    let surge0 = surge(&pre).map(|s| s.handle).context("Surge holds no chord at beat 1")?;
    let (was, code_type) = (param(surge0, volume.id), param(surge0, FILTER_TYPE));
    deck.change(&edited, Change::Structure)?;
    deck.run_secs(0.1)?;
    let after = deck.rig.plugins().expect("checked");
    let stats = deck.rig.arena().expect("checked").stats;
    // The Surge that plays now, at the node the first played at: the same one, unless an engine
    // with no arena made it anew.
    let at = pre.nodes.iter().position(|n| n.handle == surge0).expect("found above");
    let surge1 = after.nodes.get(at).map_or(surge0, |n| n.handle);
    same_instances(out, "changed state: the same Surge, Airwindows kept, none created", &Some(pre.clone()), &Some(after.clone()));
    out.check(
        "changed state: exactly one state load, into the kept Surge",
        stats.loads == 1 && stats.kept.plugins == 2 && stats.fresh.plugins == 0 && stats.unloaded.is_empty(),
        format!("{} loaded, {} kept, {} new, {} refused", stats.loads, stats.kept.plugins, stats.fresh.plugins, stats.unloaded.len()),
    );
    let now = param(surge1, volume.id);
    out.check(
        "changed state: Surge's volume reads the new patch's",
        now.is_some_and(|v| (v - want).abs() < 1e-4),
        format!("{now:?}, the patch's {want:.4}; {was:?} before"),
    );
    // HOME-4: the patch has the filter off; the code sets it ("LP 24 dB"), and wins. Surge applies a
    // loaded state at its next call, over the values that call brings: the call after sends them again.
    // A stepped parameter, it reads as Surge stores the code's 0.05: as it read before the load.
    let ftype = param(surge1, FILTER_TYPE);
    out.check(
        "changed state (HOME-4): Surge's filter type, which the code sets, reads as before the load",
        ftype.is_some() && ftype == code_type,
        format!("{ftype:?}; {code_type:?} before, from the code's 0.05"),
    );
    // HOME-6: Surge's own state, committed as an autosave would: the instance holds it already.
    let own = match host.save_state(surge1).recv().map_err(|e| anyhow!("{e}")).and_then(|r| r.map_err(|e| anyhow!("{e}"))) {
        Ok(b) => b,
        Err(e) => {
            out.check("changed state (HOME-6): its own saved state committed loads nothing", false, format!("Surge didn't save: {e:#}"));
            return Ok(());
        }
    };
    std::fs::write(song.dir.join("data/lead/lead.state"), own.bytes)?;
    let autosaved = lab.compile(&song.dir)?;
    deck.change(&autosaved, Change::Structure)?;
    deck.run_secs(0.1)?;
    let skipped = deck.rig.arena().expect("checked").stats;
    let last = deck.rig.plugins().expect("checked");
    same_instances(out, "changed state (HOME-6): its own saved state committed keeps both instances", &Some(after), &Some(last));
    // Its save holds the parameters code set since, so its bytes are the patch's only by chance:
    // then the plan's own compare skips it before the host is asked.
    let by = if hash(&autosaved) == hash(&edited) { "the patch's own bytes: the plan's compare" } else { "new bytes: the host" };
    out.check(
        "changed state (HOME-6): its own saved state committed loads nothing",
        skipped.loads == 0 && skipped.unloaded.is_empty(),
        format!("{} loaded; the blob {:?} after {:?} ({by} skipped it)", skipped.loads, hash(&autosaved), hash(&edited)),
    );
    Ok(())
}

/// Step 3b: 20 activations (the lead's dry/wet edited back and forth) create no instance and leak
/// none: the rig's live instances are those of the playing renderer, and once the rig is dropped
/// the plugin main thread destroys every one.
fn no_leak(lab: &mut Lab, kind: Kind, o: &Opts, lead: &Song, out: &mut Outcome) -> Result<()> {
    let (mut deck, _, song) = lead_deck(lab, kind, o, lead, "no-leak")?;
    let Some(pre) = deck.rig.plugins() else {
        out.skip("no leak (3b)", "this rig doesn't say which instances it plays");
        return Ok(());
    };
    let lead_ts = song.dir.join("tracks/lead.ts");
    let code = std::fs::read_to_string(&lead_ts)?;
    for k in 0..20 {
        let v = if k % 2 == 0 { "0.4" } else { "0.35" };
        std::fs::write(&lead_ts, code.replace("dry_wet_2944917344: 0.35", &format!("dry_wet_2944917344: {v}")))?;
        let edited = lab.compile(&song.dir)?;
        deck.change(&edited, Change::Structure)?;
        deck.run_secs(0.05)?;
    }
    deck.run_secs(0.1)?;
    let after = deck.rig.plugins().expect("checked");
    let owner = after.owner.clone().context("the rig names its owner")?;
    let live = settled(|| owner.live() == after.nodes.len() as u64);
    out.check(
        "no leak (3b): 20 activations create no instance, and only the playing ones live",
        after.created == pre.created && live,
        format!("{} created; {} live for {} playing", after.created - pre.created, owner.live(), after.nodes.len()),
    );
    drop(deck);
    let gone = settled(|| owner.live() == 0);
    out.check("no leak (3b): every instance destroyed once the engine is gone", gone, format!("{} left", owner.live()));
    Ok(())
}
