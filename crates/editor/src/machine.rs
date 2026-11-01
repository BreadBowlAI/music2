//! Make it a kit (KIT-22, KIT-23; the design is `docs/designs/drums/design.md`): a track that plays
//! the drum machine becomes a kit when the musician asks, never by itself. The track becomes the
//! kit's bus, so everything around the drum machine stays: its key, name, colour and channel (fader,
//! pan, sends, output), the code around the call, and what reads its exports. Its notes go onto Drum
//! tracks, one for each sound they use (`drums::split`), each going to the bus, the drum machine's
//! knobs carried by the conversions the single drums got (FMT-25) and each group's level on its
//! tracks' faders. One Tx, one undo step, all or nothing: what it can't carry exactly is named by
//! file and line, and nothing is written.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use m2_ir::Kind;
use m2_script::component::Spelled;
use m2_script::sites::{Arg, Site};
use m2_session::{Revision, Session};
use m2_song::tx::{Declared, Op, TxPath};
use m2_song::{Key, LaneAddr, LaneData, NoteClip};

use crate::drums::{GM, SILENT, split};
use crate::mix::{self, Channel, Mixing};
use crate::sites::{bound_names, fn_params, imported_names, properties, references};
use crate::tracks::{self, Becomes, Goes, Made, NewTrack, Play, quoted};
use crate::upgrade::{CLOSED, Knob, OPEN, convert_lane, exact, literal, string};
use crate::{EditError, ready};

/// What Make it a kit keeps, in its words wherever it is offered and done (KIT-22).
pub const KEEPS: &str = "Same drums, pitches, levels and lengths; each hit's noise comes out a little different.";

/// The drum machine, by its ComponentId.
pub const MACHINE: &str = "@m2/drums:drum-kit";

/// The Drum, which a kit's tracks play.
const DRUM: &str = "@m2/drums:drum";

/// The voices each Drum gets when the drum machine's call says none: the drum machine's own, so no
/// drum runs out of voices where it didn't (KIT-23).
const VOICES: f64 = 16.0;

/// A hat of the Hats track, each with its own length on the drum machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Hat {
    Closed,
    Pedal,
    Open,
}

/// How one of the drum machine's knobs reaches the Drums made of it (KIT-23).
#[derive(Clone, Copy)]
enum Carry {
    /// A knob of these sounds' Drums: the Drum's knob, and how a value reads there.
    Knob(&'static [&'static str], &'static str, Knob),
    /// The closed or the open hats' length: the Hats' one `decay`.
    Hat(Hat),
    /// The tom's tune: the Toms' `tune`, and the `decay` that keeps its ring.
    TomTune,
    /// Every Drum's knob of the same name and unit.
    Every(&'static str),
    /// A level: the faders of these sounds' tracks.
    Level(&'static [&'static str]),
}

const KICKS: &[&str] = &["kick", "kick 2"];
const SNARES: &[&str] = &["snare", "snare 2"];

/// The drum machine's knobs, as `@m2/drums` declares them, and how each is carried. The ranges are
/// those its kernel clamps each value to.
const KNOBS: [(&str, Carry); 15] = [
    ("kickTune", Carry::Knob(KICKS, "tune", Knob::Tune { own: 48.0, lo: 20.0, hi: 200.0 })),
    ("kickDecay", Carry::Knob(KICKS, "decay", Knob::Decay { own: 0.45, lo: 0.05, hi: 5.0 })),
    ("punch", Carry::Knob(KICKS, "punch", Knob::Punch(0.5))),
    ("snareTune", Carry::Knob(SNARES, "tune", Knob::Tune { own: 190.0, lo: 60.0, hi: 1000.0 })),
    ("snap", Carry::Knob(SNARES, "punch", Knob::Punch(0.6))),
    ("hatDecay", Carry::Hat(Hat::Closed)),
    ("openDecay", Carry::Hat(Hat::Open)),
    ("hatTone", Carry::Knob(&["hats"], "tune", Knob::Tune { own: 8000.0, lo: 2000.0, hi: 16000.0 })),
    ("tomTune", Carry::TomTune),
    ("drive", Carry::Every("drive")),
    ("kickLevel", Carry::Level(KICKS)),
    ("snareLevel", Carry::Level(&["snare", "snare 2", "clap", "rim"])),
    ("hatLevel", Carry::Level(&["hats"])),
    ("tomLevel", Carry::Level(&["tom"])),
    ("percLevel", Carry::Level(&["claves", "cowbell", "cymbal", "cymbal 2", "ride", "ride 2", "shaker", "shaker 2", "tambourine"])),
];

impl Carry {
    /// The unit a lane of it is in, and the unit constructors of `@m2/core` a value of it may be
    /// written with, with their scale.
    fn unit(self) -> (&'static str, &'static [(&'static str, f64)]) {
        match self {
            Carry::Knob(_, _, k) => (k.unit().0, k.spellings()),
            Carry::Hat(_) => ("seconds", CLOSED.spellings()),
            Carry::TomTune => ("hz", &[("hz", 1.0), ("khz", 1000.0)]),
            Carry::Every(_) | Carry::Level(_) => ("db", &[("db", 1.0)]),
        }
    }

    /// The sounds of `used` whose Drums it moves.
    fn moves<'a>(self, used: &[&'a str]) -> Vec<&'a str> {
        used.iter()
            .copied()
            .filter(|s| match self {
                Carry::Knob(of, ..) | Carry::Level(of) => of.contains(s),
                Carry::Hat(_) => *s == "hats",
                Carry::TomTune => *s == "tom",
                Carry::Every(_) => true,
            })
            .collect()
    }
}

/// A knob's value as the call writes it.
#[derive(Clone, Debug)]
enum Value {
    /// A number, in the knob's base unit.
    Number(f64),
    /// A lane drives it: `lane.control("<key>", "<unit>", <fallback>)`, the fallback in base units.
    Lane { key: String, unit: String, fallback: Option<f64> },
}

/// What Make it a kit made: the bus (the track it was), the Drum tracks, and what changed that the
/// musician should know, in words.
#[derive(Clone, Debug)]
pub struct MadeKit {
    pub bus: String,
    pub tracks: Vec<Made>,
    pub said: Vec<String>,
}

/// Make track `module`, which plays the drum machine, a kit (KIT-22): one Tx, one undo step. Refused
/// in words for a track it can't make one of; what it can't carry exactly is named by file and line,
/// and nothing is written (KIT-23).
pub fn make_kit(s: &mut Session, module: &str) -> Result<MadeKit, EditError> {
    ready(s)?;
    let plan = plan(s.accepted(), module)?;
    let tracks = tracks::add_tracks_as(s, &plan.tracks, Some(module), None, Some(&plan.becomes))?;
    if let Some(first) = tracks.first() {
        s.name_step(first.edited.revision, &plan.becomes.shown, true);
    }
    Ok(MadeKit { bus: module.to_string(), tracks, said: plan.said })
}

/// The tracks that play the drum machine and that Make it a kit can make a kit of, by key, in the
/// song's order: each one's menu and card offer it (KIT-24).
pub fn drum_machines(rev: &Revision) -> Vec<String> {
    rev.compiled
        .ir
        .modules
        .iter()
        .filter(|m| m.track && !m.bus && m.frames.iter().any(|f| f.component.as_deref() == Some(MACHINE)))
        .filter(|m| plan(rev, &m.key).is_ok())
        .map(|m| m.key.clone())
        .collect()
}

/// Why Make it a kit can't make a kit of track `module` now, in its words: what to do by hand first
/// (KIT-24); None when it can.
pub fn refusal(rev: &Revision, module: &str) -> Option<String> {
    match plan(rev, module) {
        Ok(_) => None,
        Err(EditError::Rejected(why)) => Some(why),
        Err(e) => Some(e.to_string()),
    }
}

/// Everything Make it a kit writes, and what it says.
struct Plan {
    tracks: Vec<NewTrack>,
    becomes: Becomes,
    said: Vec<String>,
}

/// One knob the drum machine's call writes: its key, how it is carried, its value, and where.
type Given = (&'static str, Carry, Value, u32);

fn plan(rev: &Revision, module: &str) -> Result<Plan, EditError> {
    let refused = |why: String| Err(EditError::Rejected(why));
    let Some(m) = rev.compiled.ir.modules.iter().find(|m| m.key == module && m.track) else {
        return Err(EditError::NotFound(module.to_string()));
    };
    let label = m.label().to_string();
    if m.bus {
        return refused(format!("{label} is a bus: it plays no drum machine."));
    }
    // Its one instrument, the drum machine, called in its own file.
    let players = crate::swap::players(rev, m);
    let machine = |site: &Site| m.frames.iter().any(|f| f.site == site.id && f.component.as_deref() == Some(MACHINE));
    let call = match (players.calls.as_slice(), players.plugins) {
        ([call], 0) if machine(call) => *call,
        (calls, _) if calls.iter().filter(|c| machine(c)).count() > 1 => {
            return refused(format!("{label} plays two drum machines: put each on a track of its own first."));
        }
        (calls, _) if calls.iter().any(|c| machine(c)) => {
            return refused(format!(
                "{label} plays the drum machine and another instrument: put the drum machine on a track of its own first."
            ));
        }
        _ => return refused(format!("{label} plays no drum machine.")),
    };
    if !call.in_project || Some(call.file.as_str()) != crate::swap::file_of(m) {
        return refused(format!("{label}'s drum machine is made in other code. Change it in code."));
    }
    let file = call.file.clone();
    let text = rev.snapshot.files.get(Path::new(&file)).cloned().ok_or_else(|| EditError::NotFound(file.clone()))?;
    let slice = |(a, b): (u32, u32)| &text[a as usize..b as usize];
    let line_at = |at: u32| text[..at as usize].matches('\n').count() + 1;
    let sites: Vec<&Site> = rev.compiled.sites.iter().filter(|x| x.file == file).collect();
    let site_at = |span: (u32, u32)| sites.iter().copied().find(|x| (x.start, x.end) == span);

    // Its notes: a lane of its own, `lane.notes("<key>")`, and no other lane of notes or audio.
    let notes = call
        .args
        .first()
        .and_then(|a| site_at(a.span()))
        .filter(|n| n.callee.trim().ends_with(".notes"))
        .and_then(|n| string(slice(n.args.first()?.span())))
        .filter(|k| m.lanes.iter().any(|l| l.key == *k && matches!(l.kind, Kind::Events(_))));
    let Some(notes) = notes else {
        return refused(format!("{label}'s drum machine plays notes that aren't a lane of its own. Change it in code."));
    };
    if let Some(other) = m.lanes.iter().find(|l| l.key != notes && !matches!(l.kind, Kind::Control(_))) {
        return refused(format!(
            "{label} has another lane of notes or audio, {}, and a kit's bus has none: move it to a track of its own first.",
            other.key
        ));
    }
    // Its channel: one of the song's mixer, reading its `out` as it is: the kit's bus's then.
    let Mixing::One(mixer) = mix::mixer(rev) else {
        return refused(format!("{label} isn't in the song's one mixer, so its kit would have no bus channel."));
    };
    let channel = match mixer.channels.iter().find(|c| c.tracks.iter().any(|t| t == module)) {
        Some(c) if c.tracks.len() == 1 && reads_out(rev, &mixer.file, c) => c.key.clone(),
        Some(c) => {
            return refused(format!(
                "{label}'s channel, {}, plays it through other code: write it as `{}: {module}.out`, with its effects in {label}'s own code, first.",
                c.key, c.key
            ));
        }
        None => return refused(format!("{label} has no channel in the mixer, so its kit would have no bus channel.")),
    };

    // The drum machine's options: each knob's value as written, and its voices. What it can't read
    // is named.
    let mut left: Vec<String> = vec![];
    let leave = |left: &mut Vec<String>, at: u32, what: String| {
        let said = format!("{file}:{}: {what}", line_at(at));
        if !left.contains(&said) {
            left.push(said);
        }
    };
    let mut given: Vec<Given> = vec![];
    let mut voices = VOICES;
    match call.args.get(1) {
        None => {}
        Some(Arg::Object { start, end, .. }) => match properties(&text, (*start, *end)) {
            Some(props) => {
                for p in props {
                    let written = slice(p.value).to_string();
                    let Some((key, carry)) = KNOBS.iter().find(|(k, _)| *k == p.key).copied() else {
                        match (p.key.as_str(), literal(&written)) {
                            ("voices", Some(n)) => voices = n,
                            ("voices", None) => leave(&mut left, p.start, format!("its `voices` is worked out by code (`{written}`)")),
                            (k, _) => leave(&mut left, p.start, format!("the drum machine has no `{k}`")),
                        }
                        continue;
                    };
                    match value(&text, &sites, p.value, carry) {
                        Some(v) => given.push((key, carry, v, p.start)),
                        None => leave(&mut left, p.start, format!("its `{key}` is worked out by code (`{written}`)")),
                    }
                }
            }
            None => leave(&mut left, *start, "its options are worked out by code".into()),
        },
        Some(a) => leave(&mut left, a.span().0, format!("its options are worked out by code (`{}`)", slice(a.span()))),
    }

    // Its notes, clip by clip, split by sound: the toms on the keys that play their tuning, and the
    // notes the drum machine is silent at gone.
    let clips: Vec<NoteClip> = match rev.compiled.data.lane(module, &notes).map(|d| (**d).clone()) {
        Some(LaneData::Notes(clips)) => clips,
        _ => vec![],
    };
    let mut by_sound: BTreeMap<usize, Vec<serde_json::Value>> = BTreeMap::new();
    // The notes that go, by key, and how many.
    let (mut gone, mut gone_keys, mut higher) = (0, BTreeSet::new(), false);
    let key_of = |n: &m2_song::Note| n.pitch.round().clamp(0.0, 127.0) as u8;
    for c in &clips {
        let parts = split(&c.notes);
        gone += parts.left.len();
        gone_keys.extend(parts.left.iter().map(key_of));
        for (sound, mut mine) in parts.lanes {
            let silent = |n: &m2_song::Note| SILENT.contains(&key_of(n));
            gone += mine.iter().filter(|n| silent(n)).count();
            gone_keys.extend(mine.iter().filter(|n| silent(n)).map(key_of));
            mine.retain(|n| !silent(n));
            if sound == "tom" {
                for n in &mut mine {
                    let key = n.pitch.round();
                    higher |= key == 48.0 || key == 50.0;
                    n.pitch = crate::drums::tom_key(key as u8).map_or(key, f64::from);
                }
            }
            if mine.is_empty() {
                continue;
            }
            let clip = NoteClip { id: None, other_id: None, start: c.start, length: c.length, notes: mine };
            let place = GM.iter().position(|g| g.sound == sound).expect("a sound of the table");
            by_sound.entry(place).or_default().push(serde_json::to_value(&clip).expect("a clip serialises"));
        }
    }
    let used: Vec<&str> = by_sound.keys().map(|p| GM[*p].sound).collect();
    // The notes it goes without, by name: "E3 (52) and C4 (60)".
    let named: Vec<String> =
        gone_keys.iter().map(|k| format!("{} ({k})", m2_song::lane::pitch_name(f64::from(*k)).unwrap_or_default())).collect();
    // A drum machine with nothing a Drum can play is no kit.
    if used.is_empty() {
        return refused(match gone {
            0 => format!("{label}'s drum machine plays no notes: delete it, or add a kit with + Track ▸ Drum kit."),
            _ => {
                format!("{label}'s drum machine plays only notes it has no drum for, {}: delete them, or delete it.", crate::listed(&named))
            }
        });
    }
    let hats: BTreeSet<Hat> = by_sound
        .get(&GM.iter().position(|g| g.sound == "hats").expect("the hats"))
        .into_iter()
        .flatten()
        .flat_map(|c| c["notes"].as_array().cloned().unwrap_or_default())
        .map(|n| match n["pitch"].as_f64().map(f64::round) {
            Some(44.0) => Hat::Pedal,
            Some(46.0) => Hat::Open,
            _ => Hat::Closed,
        })
        .collect();

    // Each knob's lane: the one Drum it moves, or named; a lane two knobs read, named.
    let mut read: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, v, _) in &given {
        if let Value::Lane { key, .. } = v {
            *read.entry(key.as_str()).or_default() += 1;
        }
    }
    for (key, carry, v, at) in &given {
        let Value::Lane { unit, key: lane, .. } = v else { continue };
        let moves = carry.moves(&used);
        let why = match carry {
            _ if read[lane.as_str()] > 1 => Some(format!("its lane {lane} moves two knobs: give each its own lane")),
            _ if unit != carry.unit().0 => Some(format!("its `{key}` is on a lane in {unit}, not {}", carry.unit().0)),
            Carry::Level(_) => Some(format!(
                "its `{key}` is on a lane: the drum machine sets a hit's level as it starts, and a fader would move it while it rings"
            )),
            Carry::TomTune => Some(format!(
                "its `{key}` is on a lane: the Drum rings a tom longer the lower it is tuned, where the drum machine's tom rang the same"
            )),
            _ if moves.is_empty() => Some(format!("its `{key}` is on a lane, and no drum it moves plays here")),
            _ if moves.len() > 1 => Some(format!("its `{key}` is on a lane, and it moves {} Drums", moves.len())),
            _ => None,
        };
        if let Some(why) = why {
            leave(&mut left, *at, format!("{why}: take the lane out first"));
        }
    }

    // Each Drum: its sound, the knobs it gets, its fader, its lanes.
    let mut new = vec![];
    let mut moved: Vec<String> = vec![];
    for (place, clips) in by_sound {
        let g = &GM[place];
        let mut code: Vec<(String, String)> = vec![("drum".into(), quoted(g.sound))];
        let mut lanes: Vec<(String, String)> = vec![];
        let mut db: Option<f64> = None;
        let mut hats_said = false;
        let mut take = |code: &mut Vec<(String, String)>,
                        to: &str,
                        lane: &str,
                        got: Result<(String, Option<String>), String>,
                        at: u32,
                        left: &mut Vec<String>| match got {
            Ok((call, file)) => {
                code.push((to.to_string(), call));
                lanes.extend(file.map(|f| (lane.to_string(), f)));
                moved.push(lane.to_string());
            }
            Err(why) => leave(left, at, format!("its lane {lane}: {why}")),
        };
        for (key, carry, v, at) in &given {
            if carry.moves(&[g.sound]).is_empty() {
                continue;
            }
            match (carry, v) {
                (Carry::Knob(_, to, knob), Value::Number(x)) => match knob.convert(*x) {
                    Some(c) => code.push((to.to_string(), exact(c))),
                    None => leave(&mut left, *at, format!("its `{key}` of {} is past what the Drum plays", number(*x))),
                },
                (Carry::Knob(_, to, knob), Value::Lane { key: lane, fallback, .. }) => {
                    take(&mut code, to, lane, lane_moved(rev, module, lane, *fallback, Some(*knob)), *at, &mut left);
                }
                (Carry::Hat(_), _) if !hats_said => {
                    hats_said = true;
                    match hats_decay(&given, &hats) {
                        Ok(None) => {}
                        Ok(Some(HatDecay::Factor(f))) => code.push(("decay".into(), exact(f))),
                        Ok(Some(HatDecay::Lane(lane, fallback, knob))) => {
                            take(&mut code, "decay", &lane, lane_moved(rev, module, &lane, fallback, Some(knob)), *at, &mut left);
                        }
                        Err(why) => leave(&mut left, *at, why),
                    }
                }
                (Carry::TomTune, Value::Number(t)) => {
                    let t = t.clamp(30.0, 600.0);
                    code.push(("tune".into(), exact(12.0 * (t / 130.0).log2())));
                    if t != 130.0 {
                        code.push(("decay".into(), exact((t / 130.0).sqrt())));
                    }
                }
                (Carry::Every(to), Value::Number(x)) => code.push((to.to_string(), number(*x))),
                (Carry::Every(to), Value::Lane { key: lane, fallback, .. }) => {
                    take(&mut code, to, lane, lane_moved(rev, module, lane, *fallback, None), *at, &mut left);
                }
                (Carry::Level(_), Value::Number(x)) => db = Some(x.clamp(-60.0, 12.0)),
                // The other hat's length, said with the first; a lane a fader or the tom can't take,
                // named above.
                (Carry::Hat(_) | Carry::TomTune | Carry::Level(_), _) => {}
            }
        }
        code.push(("voices".into(), number(voices)));
        new.push(NewTrack {
            play: Play::Instrument(DRUM.into()),
            label: Some(g.name.to_string()),
            clips,
            code,
            lanes,
            to: Some(Goes::Bus(channel.clone())),
            db: db.map(|x| Spelled { value: x, spelled: None, written: x }),
            ..NewTrack::default()
        });
    }
    if !left.is_empty() {
        return Err(EditError::Rejected(format!(
            "Make it a kit can't carry all of {label}'s drum machine, so nothing was written:\n{}",
            left.join("\n")
        )));
    }

    // The track as the kit's bus.
    let becomes = bus_edits(rev, m, &text, call, &channel, &notes, &moved)?;
    let mut said = vec![];
    if gone > 0 {
        let (s, them, they) = if gone == 1 { ("", "it", "It was") } else { ("s", "them", "They were") };
        said.push(format!("{gone} note{s} went: the drum machine played nothing for {them}. {they} on {}.", crate::listed(&named)));
    }
    // A pair of one of the drum machine's choke groups, each on a track of its own now (KIT-21).
    for (a, b) in [("kick", "kick 2"), ("snare", "snare 2"), ("cymbal", "cymbal 2")] {
        if used.contains(&a) && used.contains(&b) {
            said.push(format!("the {a} and {b} no longer cut each other off: each is on a track of its own"));
        }
    }
    if used.contains(&"tom") {
        said.push(match higher {
            true => "its toms moved to the keys that play their pitch on the Drum's tom; those on C3 and D3 (48, 50) play half a semitone higher".into(),
            false => "its toms moved to the keys that play their pitch on the Drum's tom".into(),
        });
    }
    Ok(Plan { tracks: new, becomes: Becomes { shown: format!("Make {label} a kit"), ..becomes }, said })
}

/// The Hats' one `decay`, from the drum machine's closed and open hats' lengths: a factor when every
/// hat the Hats track plays needs the same one, or the one lane that says it.
enum HatDecay {
    Factor(f64),
    Lane(String, Option<f64>, Knob),
}

/// [`HatDecay`] for the hats `played`: none when neither length is written; why, when no one
/// `decay` keeps them.
fn hats_decay(given: &[Given], played: &BTreeSet<Hat>) -> Result<Option<HatDecay>, String> {
    let of = |hat: Hat| given.iter().find(|(_, c, ..)| matches!(c, Carry::Hat(h) if *h == hat)).map(|(_, _, v, _)| v);
    if of(Hat::Closed).is_none() && of(Hat::Open).is_none() {
        return Ok(None);
    }
    let mut factors: Vec<f64> = vec![];
    let mut lanes: Vec<(String, Option<f64>, Knob)> = vec![];
    for hat in played {
        let (v, knob) = match hat {
            Hat::Pedal => (None, CLOSED),
            Hat::Closed => (of(Hat::Closed), CLOSED),
            Hat::Open => (of(Hat::Open), OPEN),
        };
        match v {
            None => factors.push(1.0),
            Some(Value::Number(x)) => {
                factors.push(knob.convert(*x).ok_or_else(|| format!("its hats' length of {} s is past what the Drum plays", number(*x)))?)
            }
            Some(Value::Lane { key, fallback, .. }) => lanes.push((key.clone(), *fallback, knob)),
        }
    }
    let same = factors.windows(2).all(|w| (w[0] - w[1]).abs() <= 1e-9);
    match (lanes.as_slice(), factors.first()) {
        ([], None) => Ok(None),
        ([], Some(f)) if same => Ok(Some(HatDecay::Factor(*f))),
        ([(key, fallback, knob)], None) => Ok(Some(HatDecay::Lane(key.clone(), *fallback, *knob))),
        _ => Err(hats_said(given, played)),
    }
}

/// Why no one `decay` keeps the lengths of the hats `played`: each played kind's length, as the
/// drum machine rings it ("its closed hats ring 90 ms and its pedal hats 25 ms").
fn hats_said(given: &[Given], played: &BTreeSet<Hat>) -> String {
    let ms = |hat: Hat, own: f64, knob: Knob| match given.iter().find(|(_, c, ..)| matches!(c, Carry::Hat(h) if *h == hat)).map(|g| &g.2) {
        Some(Value::Number(x)) => match knob {
            Knob::Decay { lo, hi, .. } => format!("{} ms", exact(x.clamp(lo, hi) * 1000.0)),
            _ => format!("{} ms", exact(x * 1000.0)),
        },
        Some(Value::Lane { key, .. }) => format!("as its lane {key} says"),
        None => format!("{} ms", exact(own * 1000.0)),
    };
    let rings: Vec<String> = played
        .iter()
        .map(|h| match h {
            Hat::Closed => format!("its closed hats ring {}", ms(Hat::Closed, 0.045, CLOSED)),
            Hat::Pedal => "its pedal hats 25 ms".to_string(),
            Hat::Open => format!("its open hats {}", ms(Hat::Open, 0.45, OPEN)),
        })
        .collect();
    format!(
        "{}, and the Drum's one `decay` sets every hat's length from its own 45, 25 and 450 ms: give them lengths in that proportion first",
        crate::listed(&rings)
    )
}

/// A knob's value written at `span`: a number, a unit constructor of it (`hz(50)`, `ms(30)`), or a
/// lane (`lane.control("boom", "seconds", 0.45)`); None when code works it out.
fn value(text: &str, sites: &[&Site], span: (u32, u32), carry: Carry) -> Option<Value> {
    let written = &text[span.0 as usize..span.1 as usize];
    if let Some(x) = literal(written) {
        return Some(Value::Number(x));
    }
    let call = sites.iter().find(|s| (s.start, s.end) == span)?;
    let callee = call.callee.trim();
    if let Some((_, scale)) = carry.unit().1.iter().find(|(n, _)| *n == callee) {
        return match &call.args[..] {
            [Arg::Number { value, .. }] => Some(Value::Number(value * scale)),
            _ => None,
        };
    }
    if callee.ends_with(".control")
        && let [key, unit, rest @ ..] = &call.args[..]
        && rest.len() <= 1
    {
        let at = |a: &Arg| &text[a.span().0 as usize..a.span().1 as usize];
        let fallback = match rest.first() {
            None => None,
            Some(f) => match value(text, sites, f.span(), carry)? {
                Value::Number(x) => Some(x),
                Value::Lane { .. } => return None,
            },
        };
        return Some(Value::Lane { key: string(at(key))?, unit: string(at(unit))?, fallback });
    }
    None
}

/// A number as code writes it: its shortest form.
fn number(x: f64) -> String {
    m2_song::lane::number(x)
}

/// A knob's lane moved to the Drum it moves: the code its call writes for it, and the lane's file
/// there, converted by `knob` (as it is for `drive`, whose unit stays); no file when the drum
/// machine's track has none for it.
fn lane_moved(
    rev: &Revision,
    module: &str,
    lane: &str,
    fallback: Option<f64>,
    knob: Option<Knob>,
) -> Result<(String, Option<String>), String> {
    let (unit, now) = match knob {
        Some(k) => (k.unit().1, k.convert(fallback.unwrap_or(0.0)).ok_or("its fallback is past what the Drum plays")?),
        None => ("db", fallback.unwrap_or(0.0)),
    };
    let call = format!("lane.control({}, {}, {})", quoted(lane), quoted(unit), exact(now));
    let addr = LaneAddr { module: Key::new(module)?, lane: Key::new(lane)? };
    let Some(text) = rev.snapshot.files.get(Path::new(TxPath::lane(&addr).as_str())) else { return Ok((call, None)) };
    match knob.map(|k| convert_lane(text, k)).transpose()?.flatten() {
        Some((after, _)) => Ok((call, Some(after))),
        None => Ok((call, Some(text.to_string()))),
    }
}

/// The bus's words for itself (KIT-22).
const BUS_COMMENT: &str = "// The kit's bus: its Drum tracks play into it.";

/// The edit that makes the `//` comment right above the line at `at` (a track's call) the bus's:
/// every line of it, as one, at its indentation; none where there is none.
fn comment_above(text: &str, at: u32) -> Option<(u32, u32, String)> {
    let line = text[..at as usize].rfind('\n').map_or(0, |i| i + 1);
    let mut start = line;
    while start > 0 {
        let above = text[..start - 1].rfind('\n').map_or(0, |i| i + 1);
        if !text[above..start - 1].trim_start().starts_with("//") {
            break;
        }
        start = above;
    }
    if start == line {
        return None;
    }
    let indent: String = text[start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    Some((start as u32, line as u32 - 1, format!("{indent}{BUS_COMMENT}")))
}

/// Whether channel `c` of the mixer in `file` reads its track's `out` as it is (`beat: beat.out`, or
/// `{ in: beat.out, … }`), as a bus channel must (RENDER-34).
fn reads_out(rev: &Revision, file: &str, c: &Channel) -> bool {
    let Some(text) = rev.snapshot.files.get(Path::new(file)) else { return false };
    let at = |(a, b): (u32, u32)| text[a as usize..b as usize].trim().to_string();
    let plain = |t: &str| {
        t.strip_suffix(".out").is_some_and(|b| !b.is_empty() && b.chars().all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$'))
    };
    plain(&at(c.property.value))
        || properties(text, c.property.value).is_some_and(|props| props.iter().any(|p| p.key == "in" && plain(&at(p.value))))
}

/// The edits that make track `m` the kit's bus (KIT-22): in its file `text`, `track(` is `bus(`, its
/// function takes `input`, and the drum machine's `call` is `input` (`gain(input)` where it was all
/// the function gave, as Add bus writes a Group), the imports following; its notes lane `notes` and
/// the knobs' lanes `moved` go.
fn bus_edits(
    rev: &Revision,
    m: &m2_ir::Module,
    text: &str,
    call: &Site,
    channel: &str,
    notes: &str,
    moved: &[String],
) -> Result<Becomes, EditError> {
    let label = m.label();
    let refused = |why: &str| {
        EditError::Rejected(format!("{label}'s code is written so Make it a kit can't make it a bus ({why}). Change it in code."))
    };
    let track = crate::rename::track_call(&rev.compiled.sites, &rev.snapshot.files, m)
        .ok_or_else(|| refused("its `track()` call can't be found"))?;
    let fun = track.args.last().map(Arg::span).ok_or_else(|| refused("its `track()` call has no function"))?;
    if !(fun.0 <= call.start && call.end <= fun.1) {
        return Err(refused("the drum machine isn't called in its function"));
    }
    let params = fn_params(text, fun).map_err(refused)?;
    let list = &text[params.start as usize..params.end as usize];
    // Whether the function reads `lane` anywhere but in the drum machine's call.
    let lane_read = references(text, "lane").into_iter().any(|at| fun.0 <= at && at < fun.1 && !(call.start <= at && at < call.end));
    let compact: String = list.chars().filter(|c| !c.is_whitespace()).collect();
    let inner = compact.strip_prefix("({").and_then(|c| c.strip_suffix("})"));
    let (input, param) = match (compact.as_str(), inner) {
        ("({lane})", _) if !lane_read => ("input".to_string(), Some((params.start, params.end, "({ input })".to_string()))),
        ("()", _) => ("input".to_string(), Some((params.start, params.end, "({ input })".to_string()))),
        (_, Some(props)) if props.split(',').any(|p| p == "input") => ("input".to_string(), None),
        (_, Some(props)) if props.split(',').all(|p| !p.is_empty() && p.chars().all(|ch| ch.is_alphanumeric() || ch == '_')) => {
            // The whole list rewritten, so the bus's `input` it now takes is inside the edit.
            let close = list.rfind('}').expect("a closing brace");
            let at = list[..close].trim_end().len();
            ("input".to_string(), Some((params.start, params.end, format!("{}, input{}", &list[..at], &list[at..]))))
        }
        (c, None)
            if c.len() > 2
                && c.starts_with('(')
                && c.ends_with(')')
                && c[1..c.len() - 1].chars().all(|ch| ch.is_alphanumeric() || ch == '_') =>
        {
            (format!("{}.input", &c[1..c.len() - 1]), None)
        }
        _ => return Err(refused("its function takes its context in a form it can't follow")),
    };
    if param.is_some() && bound_names(text).contains("input") {
        return Err(refused("its file has an `input` of its own"));
    }
    // The call alone is what the function gives: its sound passes through a gain at 0 dB, the bus's
    // own node (a Group, RENDER-34).
    let whole = text[params.end as usize..call.start as usize].trim() == "=>" && text[call.end as usize..fun.1 as usize].trim().is_empty();
    let core = imported_names(text, m2_song::SDK);
    if !core.iter().any(|n| n == "bus") && bound_names(text).contains("bus") {
        return Err(refused("its file has a `bus` of its own"));
    }
    let gain = whole && !imported_names(text, "std/dsp").iter().any(|n| n == "gain");
    if gain && bound_names(text).contains("gain") {
        return Err(refused("its file has a `gain` of its own"));
    }
    let callee = (track.start, track.start + track.callee.trim_end().len() as u32);
    let mut edits =
        vec![(callee.0, callee.1, "bus".to_string()), (call.start, call.end, if whole { format!("gain({input})") } else { input })];
    edits.extend(param);
    edits.extend(comment_above(text, track.start));
    let mut adds: Vec<(&'static str, &'static str)> = vec![(m2_song::SDK, "bus")];
    if gain {
        adds.push(("std/dsp", "gain"));
    }
    let gone = vec![callee, (call.start, call.end)];
    if let Some((_, b, _)) = crate::swap::import_edits(text, &adds, &gone)
        && b > track.start
    {
        return Err(refused("its imports aren't at its top"));
    }
    edits.sort_by_key(|e| (e.0, e.1));
    let (mut ops, mut ranges, mut lanes) = (vec![], BTreeMap::new(), vec![]);
    let key = Key::new(&m.key).map_err(EditError::Rejected)?;
    for lane in std::iter::once(notes).chain(moved.iter().map(String::as_str)) {
        let addr = LaneAddr { module: key.clone(), lane: Key::new(lane).map_err(EditError::Rejected)? };
        let at = TxPath::lane(&addr);
        if rev.snapshot.files.contains_key(Path::new(at.as_str())) {
            ops.push(Op::Remove { file: at.clone() });
            ranges.insert(at, Declared::Whole);
        }
        lanes.push(addr);
    }
    Ok(Becomes {
        module: m.key.clone(),
        channel: channel.to_string(),
        component: MACHINE.to_string(),
        file: call.file.clone(),
        edits,
        adds,
        gone,
        ops,
        ranges,
        lanes,
        shown: String::new(),
    })
}

#[cfg(test)]
#[path = "machine_tests.rs"]
mod tests;
