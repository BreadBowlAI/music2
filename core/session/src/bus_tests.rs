//! std 0.4.0's mixer and buses (RENDER-34): a channel's sends (after the fader, or before it with
//! `pre: true`) and `to` (a group bus instead of the master) feed its bus channels, sample for
//! sample, and a bus that would feed itself is refused in plain words.

use std::path::{Path, PathBuf};

use m2_ir::Diagnostic;
use m2_runtime::{PackageSet, Renderer};
use m2_song::Project;

use crate::{Compiled, compile};

fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    p
}

/// A project of one `song.ts` in a temp folder, which is removed when dropped.
struct Song(PathBuf);

impl Drop for Song {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `body`, what the song's function returns, with a saw `vox` and a sine `keys`, and the buses
/// `verb` and `delay` it names, which play what they are fed as it is (`delay` times 1.0, which
/// changes no bit). A bus nobody feeds is a build error, so only those named are declared.
fn song(body: &str) -> Song {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("m2-bus-test-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("music2.toml"), crate::tests::MANIFEST).unwrap();
    let verb = if body.contains("verb") { "const verb = bus(\"verb\", ({ input }) => input);\n" } else { "" };
    let delay = if body.contains("delay") { "const delay = bus(\"delay\", ({ input }) => scale(input, 1));\n" } else { "" };
    let code = format!(
        "import {{ bus, db, feed, song, track }} from \"@m2/core\";\n\
         import {{ gain, osc }} from \"std/dsp\";\n\
         import {{ scale }} from \"std/math\";\n\
         import {{ mixer }} from \"std/mix\";\n\
         const vox = track(\"vox\", () => osc.saw(220));\n\
         const keys = track(\"keys\", () => osc.sine(330));\n\
         {verb}{delay}export default song(() => {{\n{body}\n}});\n"
    );
    std::fs::write(dir.join("song.ts"), code).unwrap();
    Song(dir)
}

fn build(s: &Song) -> Result<Compiled, Vec<Diagnostic>> {
    compile(&Project::open(&s.0).unwrap(), &mut std_packages())
}

/// Half a second of the song, offline.
fn render(body: &str) -> Vec<[f32; 2]> {
    let s = song(body);
    let c = build(&s).unwrap_or_else(|e| panic!("{body}: {e:?}"));
    Renderer::new(&c.flat, &c.data, &std_packages(), 48_000).unwrap().render(0.5).unwrap()
}

/// Whether two renders are the same bits, and something plays.
fn same(a: &[[f32; 2]], b: &[[f32; 2]]) -> bool {
    a.iter().any(|f| f[0] != 0.0) && a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.map(f32::to_bits) == y.map(f32::to_bits))
}

/// The one error a song fails to build with.
fn error(body: &str) -> Diagnostic {
    let Err(mut e) = build(&song(body)) else { panic!("{body}: expected an error") };
    assert_eq!(e.len(), 1, "{e:?}");
    e.remove(0)
}

// RENDER-34: a send after the fader is the channel's strip (after its pan and fader) at the send's
// level, so it follows the fader. Sent at 0 dB to a bus that plays its input, the song is the
// strip twice, sample for sample: the same bits as two channels with that fader and pan. The
// verb channel is silent with nothing sent to it.
#[test]
fn a_send_after_the_fader_follows_it() {
    for fader in ["db(0)", "db(-20)"] {
        let sent = render(&format!(
            "return mixer({{ vox: {{ in: vox.out, db: {fader}, pan: 0.4, sends: {{ verb: db(0) }} }}, keys: keys.out, verb: verb.out }});"
        ));
        let twice = render(&format!(
            "return mixer({{ vox: {{ in: vox.out, db: {fader}, pan: 0.4 }}, keys: keys.out, wet: {{ in: vox.out, db: {fader}, pan: 0.4 }} }});"
        ));
        assert!(same(&sent, &twice), "fader {fader}: the send is the strip");
    }
    let dry = render("return mixer({ vox: { in: vox.out, db: db(-6) }, keys: keys.out });");
    let unfed = render("return mixer({ vox: { in: vox.out, db: db(-6) }, keys: keys.out, verb: verb.out });");
    assert!(same(&unfed, &dry), "a bus channel nothing is sent to is silence");
}

// RENDER-34: a send before the fader (`pre: true`) is the channel's input at the send's level: it
// ignores the channel's pan and fader.
#[test]
fn a_send_before_the_fader_ignores_it() {
    let sent =
        render("return mixer({ vox: { in: vox.out, db: db(-20), pan: 0.4, sends: { verb: { db: db(0), pre: true } } }, verb: verb.out });");
    let apart = render("return mixer({ vox: { in: vox.out, db: db(-20), pan: 0.4 }, wet: vox.out });");
    assert!(same(&sent, &apart), "the send is the input");
}

// RENDER-34: a channel with `to` goes to that bus instead of the master: the group plays its
// tracks summed, and they aren't in the master twice. A bus may go to another bus, and send.
#[test]
fn a_track_sent_to_a_group_leaves_the_master() {
    let grouped =
        render("return mixer({ vox: { in: vox.out, db: db(-6), to: \"verb\" }, keys: { in: keys.out, to: \"verb\" }, verb: verb.out });");
    let plain = render("return mixer({ vox: { in: vox.out, db: db(-6) }, keys: keys.out });");
    assert!(same(&grouped, &plain), "the group is its tracks, once");
    let chained = render(
        "return mixer({ vox: { in: vox.out, db: db(-6), to: \"verb\" }, keys: { in: keys.out, to: \"delay\" }, verb: { in: verb.out, to: \"delay\" }, delay: delay.out });",
    );
    assert!(same(&chained, &plain), "a group into a group");
}

// RENDER-34: a bus that feeds itself is refused: by std's mixer, naming the loop by channel keys,
// and by the checker when code feeds a bus its own output, naming the modules.
#[test]
fn a_bus_that_feeds_itself_is_refused_in_plain_words() {
    let e = error(
        "return mixer({ vox: { in: vox.out, sends: { verb: db(-12) } }, verb: { in: verb.out, sends: { delay: db(-6) } }, delay: { in: delay.out, to: \"verb\" } });",
    );
    assert_eq!(e.code, "code.build", "{e}");
    assert!(
        e.message.contains("verb feeds itself: verb → delay → verb. A bus can't feed itself. Take away one of these sends or outputs."),
        "{e}"
    );
    let e = error("return mixer({ vox: vox.out, verb: { in: verb.out, to: \"verb\" } });");
    assert!(e.message.contains("verb feeds itself: verb → verb."), "{e}");
    let e = error("return mixer({ vox: { in: vox.out, sends: { verbb: db(-12) } }, verb: verb.out });");
    assert!(e.message.contains("vox sends to \"verbb\", which isn't a bus channel of this mixer"), "{e}");
    let e = error("return mixer({ vox: { in: vox.out, to: \"keys\" }, keys: keys.out });");
    assert!(e.message.contains("vox goes to \"keys\", which isn't a bus channel of this mixer"), "{e}");
    // Through code: the checker's loop, naming the modules it goes through.
    let e = error("feed(delay, gain(delay.out, db(-6)));\nreturn delay.out;");
    assert_eq!(e.code, "code.check", "{e}");
    assert!(e.message.contains("form a loop") && e.message.contains("It goes through song → delay → song."), "{e}");
}
