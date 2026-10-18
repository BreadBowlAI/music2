//! The bus edits (RENDER-34, the bus plan's slice 2): what each writes, byte for byte, that each is
//! one undo step, that a send's level is a knob like a fader, and that a loop is refused before
//! anything is written, in words that name the tracks.

use m2_session::Session;

use super::*;
use crate::bus;
use crate::testing::fixture;
use crate::{Deleted, Gestures, MixView, Strip};

const BUSES: &str = "core/session/tests/fixtures/buses";

fn render(s: &Session, seconds: f64) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(seconds).unwrap()
}

/// The largest difference between two renders, sample for sample.
fn apart(a: &[[f32; 2]], b: &[[f32; 2]]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).flat_map(|(x, y)| [(x[0] - y[0]).abs(), (x[1] - y[1]).abs()]).fold(0.0, f32::max)
}

/// Which track reaches which bus, as the checker found it.
fn routes(s: &Session) -> Vec<(String, String)> {
    s.accepted().compiled.flat.routes.clone()
}

fn route_of(m: &str, b: &str) -> (String, String) {
    (m.to_string(), b.to_string())
}

fn strip(v: &MixView, key: &str) -> Strip {
    v.strips.iter().find(|x| x.channel.as_deref() == Some(key)).unwrap_or_else(|| panic!("no strip for {key}: {v:?}")).clone()
}

/// The knob id of channel `channel`'s send to `bus`, as the Mix view names it.
fn send_knob(s: &Session, channel: &str, bus: &str) -> String {
    let x = strip(&s.mix(), channel);
    let send = x.sends.iter().find(|x| x.bus == bus).unwrap_or_else(|| panic!("{channel} sends to no {bus}: {x:?}"));
    send.knob.clone().unwrap_or_else(|| panic!("{channel}'s send to {bus} has no knob"))
}

/// The cells of `a` and `b` are laid out alike, and only these differ: each `(index, new value)`.
fn cell_writes(a: &m2_ir::Flat, b: &m2_ir::Flat) -> Vec<(usize, f64)> {
    let (x, y) = (m2_runtime::cells::cells(a), m2_runtime::cells::cells(b));
    assert_eq!(x.len(), y.len(), "the same cells");
    assert!(x.iter().zip(&y).all(|(p, q)| p.at == q.at), "at the same places");
    x.iter().zip(&y).enumerate().filter(|(_, (p, q))| p.value.to_bits() != q.value.to_bits()).map(|(i, (_, q))| (i, q.value)).collect()
}

// Add a bus: a Reverb on first-song writes `tracks/reverb.ts` with `bus(…)`, its import and its
// channel in the song, and builds a bus module whose strip comes last, marked as a bus. One undo
// step puts every byte back and takes the file away. A Delay and a Group build too.
#[test]
fn add_bus_writes_a_bus_file_and_a_channel() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let song = f.read("song.ts");
    let made = bus::add_bus(&mut s, BusKind::Reverb, None, None).unwrap();
    assert_eq!((made.module.as_str(), made.label.as_str()), ("reverb", "Reverb"));
    assert_eq!(
        f.read("tracks/reverb.ts"),
        "import { bus } from \"@m2/core\";\nimport { reverb } from \"std/fx\";\n\n\
         export default bus(\"reverb\", { name: \"Reverb\" }, ({ input }) => reverb(input, { mix: 1 }));\n"
    );
    let now = f.read("song.ts");
    assert!(now.contains("import reverb from \"./tracks/reverb\";\n"), "{now}");
    assert!(now.contains("      reverb: reverb.out,\n    }),"), "{now}");
    let m = s.accepted().compiled.ir.modules.iter().find(|m| m.key == "reverb").unwrap();
    assert!(m.bus && m.track);
    let v = s.mix();
    let last = v.strips.last().unwrap();
    assert_eq!((last.channel.as_deref(), last.label.as_str(), last.bus), (Some("reverb"), "Reverb", true));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), song);
    assert!(!f.0.join("tracks/reverb.ts").exists());

    let delay = bus::add_bus(&mut s, BusKind::Delay, None, None).unwrap();
    assert!(f.read("tracks/delay.ts").contains("bus(\"delay\", { name: \"Delay\" }, ({ input }) => delay(input, { mix: 1 }))"));
    let group = bus::add_bus(&mut s, BusKind::Group, Some("Drums"), Some("kick")).unwrap();
    assert_eq!((delay.module.as_str(), group.module.as_str(), group.label.as_str()), ("delay", "drums", "Drums"));
    assert!(f.read("tracks/drums.ts").contains("bus(\"drums\", { name: \"Drums\" }, ({ input }) => gain(input))"));
    assert!(s.accepted().compiled.ir.modules.iter().filter(|m| m.bus).count() == 2);
}

// A send, added and taken out: into a bare entry (wrapped), into an object with sends; taken out of
// several sends, and with the `sends` object when it was the last. One undo step each. A send that
// is there already, one that isn't, and a send to a track that isn't a bus are refused.
#[test]
fn add_send_and_remove_it() {
    let f = fixture(BUSES);
    let mut s = f.open();
    let song = f.read("song.ts");
    for (edit, from, to) in [
        (
            ("add", "bass", "verb"),
            "bass: duck(bass.out, kick.env, { depth: db(-9) }),",
            "bass: { in: duck(bass.out, kick.env, { depth: db(-9) }), sends: { verb: db(-12) } },",
        ),
        (("add", "hats", "echo"), "sends: { verb: db(-14) } }", "sends: { verb: db(-14), echo: db(-12) } }"),
        (("remove", "keys", "verb"), "sends: { verb: db(-10), echo:", "sends: { echo:"),
        (("remove", "hats", "verb"), "to: \"drums\", sends: { verb: db(-14) } }", "to: \"drums\" }"),
        (("remove", "echo", "verb"), "echo: { in: echo.out, db: db(-6), sends: { verb: db(-12) } }", "echo: { in: echo.out, db: db(-6) }"),
    ] {
        let (what, channel, b) = edit;
        match what {
            "add" => drop(bus::add_send(&mut s, channel, b, false).unwrap()),
            _ => drop(bus::remove_send(&mut s, channel, b).unwrap()),
        }
        assert_eq!(f.read("song.ts"), song.replacen(from, to, 1), "{edit:?}");
        assert_eq!(routes(&s).contains(&route_of(channel, b)), what == "add", "{edit:?}");
        s.undo().unwrap();
        assert_eq!(f.read("song.ts"), song, "{edit:?}: one undo step");
    }
    let rev = s.accepted().number;
    for (refused, said) in [
        (bus::add_send(&mut s, "hats", "verb", false), "Hats already sends to Reverb."),
        (bus::remove_send(&mut s, "bass", "verb"), "Bass doesn't send to Reverb."),
        (bus::add_send(&mut s, "hats", "bass", false), "Bass isn't a bus, so nothing can be sent to it."),
    ] {
        assert_eq!(refused.unwrap_err(), EditError::Rejected(said.into()));
    }
    assert_eq!((f.read("song.ts"), s.accepted().number), (song, rev));
}

// A send before the fader: added so, set so (the bare level wrapped with `pre: true`) and set back
// (the level bare again), each one undo step. Setting what is already so writes nothing.
#[test]
fn a_send_can_go_before_the_fader() {
    let f = fixture(BUSES);
    let mut s = f.open();
    let song = f.read("song.ts");
    bus::add_send(&mut s, "bass", "echo", true).unwrap();
    assert!(f.read("song.ts").contains("bass: { in: duck(bass.out, kick.env, { depth: db(-9) }), sends: { echo: { db: db(-12), pre: true } } },"));
    s.undo().unwrap();
    bus::set_pre(&mut s, "hats", "verb", true).unwrap();
    assert_eq!(f.read("song.ts"), song.replacen("sends: { verb: db(-14) }", "sends: { verb: { db: db(-14), pre: true } }", 1));
    bus::set_pre(&mut s, "hats", "verb", false).unwrap();
    assert_eq!(f.read("song.ts"), song, "after the fader again: the level bare");
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), song);
    bus::set_pre(&mut s, "keys", "echo", false).unwrap();
    assert_eq!(f.read("song.ts"), song.replacen("echo: { db: db(-12), pre: true }", "echo: db(-12)", 1));
    s.undo().unwrap();
    let rev = s.accepted().number;
    bus::set_pre(&mut s, "keys", "verb", false).unwrap();
    assert_eq!((f.read("song.ts"), s.accepted().number), (song, rev), "already after the fader");
}

// A send's level is a knob, as a fader is: turning it is a `Values` edit whose graph differs only
// in that constant's cell, so what plays takes it with no new plan. A level the send leaves out
// (`{ pre: true }`) is a knob too, at its default; its first turn writes it in, also one cell.
#[test]
fn a_send_level_is_a_cell_write() {
    let f = fixture(BUSES);
    let mut s = f.open();
    let song = f.read("song.ts");
    let knob = send_knob(&s, "hats", "verb");
    let before = s.accepted().compiled.flat.clone();
    s.set(s.accepted().number, &knob, -6.0, None).unwrap();
    assert_eq!(f.read("song.ts"), song.replacen("sends: { verb: db(-14) }", "sends: { verb: db(-6) }", 1));
    assert_eq!(cell_writes(&before, &s.accepted().compiled.flat).iter().map(|c| c.1).collect::<Vec<_>>(), [-6.0]);

    let left_out = song.replacen("echo: { db: db(-12), pre: true }", "echo: { pre: true }", 1);
    f.write("song.ts", &left_out);
    assert!(s.refresh());
    let knob = send_knob(&s, "keys", "echo");
    let c = s.controls().into_iter().find(|c| c.id == knob).unwrap();
    assert_eq!((c.written, c.value), (false, -12.0));
    let before = s.accepted().compiled.flat.clone();
    s.set(s.accepted().number, &knob, -6.0, None).unwrap();
    assert_eq!(f.read("song.ts"), left_out.replacen("echo: { pre: true }", "echo: { pre: true, db: -6 }", 1));
    assert_eq!(cell_writes(&before, &s.accepted().compiled.flat).iter().map(|c| c.1).collect::<Vec<_>>(), [-6.0]);
}

// A send's level is automated as a fader is: its lane is named for the track and the bus, and the
// strip's send then names the lane instead of a knob.
#[test]
fn a_send_level_is_automated_like_a_fader() {
    let f = fixture(BUSES);
    let mut s = f.open();
    let knob = send_knob(&s, "hats", "verb");
    let done = s.automate(s.accepted().number, &knob).unwrap();
    assert_eq!((done.name.as_str(), done.lane.lane.as_str()), ("Hats send to Reverb", "hats-send-to-reverb"));
    let x = strip(&s.mix(), "hats");
    let send = x.sends.iter().find(|x| x.bus == "verb").unwrap();
    assert_eq!(send.knob, None);
    assert_eq!(send.lane.as_ref().map(|l| l.lane.as_str()), Some("hats-send-to-reverb"));
}

// Route a track to a group and back: `to: "drums"` written into its entry (wrapped), and taken out
// again (the entry bare again, the same bytes). In the group, the kick is heard once, through it: a
// group at 0 dB sounds as the song did. Muting the group in the song silences the kick with it.
#[test]
fn route_takes_a_track_to_a_group_and_back() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let plain = render(&s, 4.0);
    bus::add_bus(&mut s, BusKind::Group, Some("Drums"), None).unwrap();
    let song = f.read("song.ts");
    bus::route(&mut s, "kick", Some("drums")).unwrap();
    assert_eq!(f.read("song.ts"), song.replacen("kick: kick.out", "kick: { in: kick.out, to: \"drums\" }", 1));
    assert!(routes(&s).contains(&route_of("kick", "drums")));
    assert_eq!(strip(&s.mix(), "kick").output.as_deref(), Some("drums"));
    assert!(apart(&plain, &render(&s, 4.0)) < 1e-5, "the kick is heard once, through the group");

    crate::mute::mute_in_song(&mut s, &["drums".to_string()], true).unwrap();
    let group_muted = render(&s, 4.0);
    s.undo().unwrap();
    crate::mute::mute_in_song(&mut s, &["kick".to_string()], true).unwrap();
    assert!(apart(&group_muted, &render(&s, 4.0)) < 1e-5, "muting the group silences the kick");
    s.undo().unwrap();

    bus::route(&mut s, "kick", None).unwrap();
    assert_eq!(f.read("song.ts"), song, "back to the master: the entry bare again");
    assert_eq!(strip(&s.mix(), "kick").output, None);
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), song);
}

// A send or an output that would make a loop is refused before anything is written, in words that
// name the tracks: directly, through another bus, and a bus to itself.
#[test]
fn a_send_or_route_that_makes_a_loop_is_refused_before_writing() {
    let f = fixture(BUSES);
    let mut s = f.open();
    let song = f.read("song.ts");
    let rev = s.accepted().number;
    for (refused, said) in [
        (bus::add_send(&mut s, "verb", "echo", false), "Echo already feeds Reverb, so Reverb can't send to Echo."),
        (bus::route(&mut s, "verb", Some("echo")), "Echo already feeds Reverb, so Reverb can't go to Echo."),
        (bus::add_send(&mut s, "echo", "echo", true), "Echo can't send to itself."),
        (bus::route(&mut s, "drums", Some("drums")), "Drums can't go to itself."),
    ] {
        assert_eq!(refused.unwrap_err(), EditError::Rejected(said.into()));
    }
    assert_eq!((f.read("song.ts"), s.accepted().number), (song, rev));
    bus::add_send(&mut s, "drums", "verb", false).unwrap();
    let song = f.read("song.ts");
    let refused = bus::route(&mut s, "verb", Some("drums")).unwrap_err();
    assert_eq!(refused, EditError::Rejected("Drums already feeds Reverb, so Reverb can't go to Drums.".into()));
    assert_eq!(f.read("song.ts"), song);
}

// Deleting a bus asks first, listing the sends and outputs that go with it; then takes them out with
// it: the sends to it (the `sends` object with its last), and the tracks that went to it back to
// the master. The song builds; one undo step puts every byte back.
#[test]
fn deleting_a_bus_takes_its_sends_and_routes_with_it() {
    let f = fixture(BUSES);
    let mut s = f.open();
    let song = f.read("song.ts");
    let Deleted::Asks(deps) = s.delete_track("verb", false).unwrap() else { panic!("it asks first") };
    let rows: Vec<(&str, &str, bool)> = deps.iter().map(|d| (d.module.as_str(), d.component.as_str(), d.removable)).collect();
    assert_eq!(rows, [("hats", "Send", true), ("keys", "Send", true), ("echo", "Send", true)]);
    let Deleted::Done { .. } = s.delete_track("verb", true).unwrap() else { panic!("deleted") };
    let without = song
        .replacen("import verb from \"./tracks/verb\";\n", "", 1)
        .replacen(", sends: { verb: db(-14) } }", " }", 1)
        .replacen("sends: { verb: db(-10), echo:", "sends: { echo:", 1)
        .replacen("      verb: { in: verb.out, db: db(-3) },\n", "", 1)
        .replacen(", sends: { verb: db(-12) } }", " }", 1);
    assert_eq!(f.read("song.ts"), without);
    assert!(s.draft_errors.is_empty() && !s.accepted().compiled.ir.modules.iter().any(|m| m.key == "verb"));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), song);

    let Deleted::Asks(deps) = s.delete_track("drums", false).unwrap() else { panic!("it asks first") };
    let rows: Vec<(&str, &str)> = deps.iter().map(|d| (d.module.as_str(), d.component.as_str())).collect();
    assert_eq!(rows, [("kick", "Output"), ("hats", "Output")]);
    s.delete_track("drums", true).unwrap();
    let text = f.read("song.ts");
    assert!(text.contains("      kick: kick.out,\n") && text.contains("hats: { in: hats.out, db: db(-6), pan: 0.3, sends: { verb: db(-14) } },"), "{text}");
    assert!(!routes(&s).iter().any(|r| r.1 == "drums"));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), song);
}

// A bus's `bus()` call is found as a track's is: it is renamed, muted in the song and duplicated
// (its channel copied with its sends).
#[test]
fn a_bus_is_renamed_and_muted_like_a_track() {
    let f = fixture(BUSES);
    let mut s = f.open();
    s.rename_label("verb", "Hall").unwrap();
    assert!(f.read("tracks/verb.ts").contains("bus(\"verb\", { name: \"Hall\" }, "));
    crate::mute::mute_in_song(&mut s, &["verb".to_string()], true).unwrap();
    assert!(f.read("tracks/verb.ts").contains("bus(\"verb\", { name: \"Hall\", muted: true }, "));
    assert!(s.accepted().compiled.flat.muted.contains(&"verb".to_string()));
    let made = s.duplicate_track("echo").unwrap();
    assert_eq!((made.module.as_str(), made.label.as_str()), ("echo-2", "Echo 2"));
    assert!(f.read("tracks/echo-2.ts").contains("bus(\"echo-2\", { name: \"Echo 2\" }, "));
    assert!(f.read("song.ts").contains("\"echo-2\": { in: echo2.out, db: db(-6), sends: { verb: db(-12) } },"), "{}", f.read("song.ts"));
}

// The Mix view: the tracks' strips, then the buses' (marked), each with its output, its sends (bus,
// knob, before the fader or not) and the buses it may send to or go to without a loop.
#[test]
fn the_mix_view_lists_buses_sends_and_outputs() {
    let f = fixture(BUSES);
    let s = f.open();
    let v = s.mix();
    let rows: Vec<(&str, bool, Option<&str>)> =
        v.strips.iter().map(|x| (x.channel.as_deref().unwrap(), x.bus, x.output.as_deref())).collect();
    assert_eq!(
        rows,
        [
            ("kick", false, Some("drums")),
            ("hats", false, Some("drums")),
            ("bass", false, None),
            ("keys", false, None),
            ("drums", true, None),
            ("verb", true, None),
            ("echo", true, None),
        ]
    );
    let keys = strip(&v, "keys");
    let sends: Vec<(&str, bool, f64)> = keys
        .sends
        .iter()
        .map(|x| (x.bus.as_str(), x.pre, s.controls().into_iter().find(|c| Some(&c.id) == x.knob.as_ref()).unwrap().value))
        .collect();
    assert_eq!(sends, [("verb", false, -10.0), ("echo", true, -12.0)]);
    assert!(strip(&v, "bass").sends.is_empty() && keys.sends.iter().all(|x| x.lane.is_none()));
    for (key, may) in [("kick", &["drums", "verb", "echo"][..]), ("drums", &["verb", "echo"]), ("verb", &["drums"]), ("echo", &["drums", "verb"])] {
        assert_eq!(strip(&v, key).may_use, may, "{key}");
    }
}

// The Mix view says why a bus is greyed on a strip, in the words a send or an output there would be
// refused with (its own bus too); and the timeline marks a bus's row, which has no lanes.
#[test]
fn the_mix_view_says_why_a_bus_would_make_a_loop() {
    let f = fixture(BUSES);
    let s = f.open();
    let v = s.mix();
    let loops = |key: &str| -> Vec<(String, String, String)> { strip(&v, key).loops.into_iter().map(|l| (l.bus, l.send, l.to)).collect() };
    let said = |b: &str, send: &str, to: &str| (b.to_string(), send.to_string(), to.to_string());
    assert_eq!(
        loops("verb"),
        [
            said("verb", "Reverb can't send to itself.", "Reverb can't go to itself."),
            said("echo", "Echo already feeds Reverb, so Reverb can't send to Echo.", "Echo already feeds Reverb, so Reverb can't go to Echo."),
        ]
    );
    assert_eq!(loops("drums"), [said("drums", "Drums can't send to itself.", "Drums can't go to itself.")]);
    assert!(loops("kick").is_empty() && loops("keys").is_empty());
    let rows: Vec<(String, bool, usize)> = crate::timeline::timeline(&s).tracks.into_iter().map(|t| (t.module, t.bus, t.lanes.len())).collect();
    let bus_rows: Vec<&(String, bool, usize)> = rows.iter().filter(|r| r.1).collect();
    assert_eq!(bus_rows.iter().map(|r| (r.0.as_str(), r.2)).collect::<Vec<_>>(), [("drums", 0), ("verb", 0), ("echo", 0)], "{rows:?}");
}
