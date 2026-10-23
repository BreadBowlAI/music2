//! Forwarding (AGENT-1, track 2 of the agents design): a read-only session hands its commits to
//! the lock's holder, each checked here first under its own expectation and sent with the hash of
//! that check; once one landed, the session reads what the holder wrote; undo and redo are the
//! holder's. And the retract a busy settle takes an agent's change back with (AGENT-3).

use std::sync::{Arc, Mutex};

use m2_song::Sha;
use m2_song::tx::{Expect, Op, Tx, TxPath};

use super::tests::{Dir, based, p, pan, std_packages};
use crate::forward::{Forward, Landed, Sent as Sending};
use crate::{Refusal, Session, Source, StepError, Uncommitted};

/// What the holder was sent: each Tx, with its `seen` and its assets.
type Sent = Arc<Mutex<Vec<(Tx, Sha, Vec<TxPath>)>>>;

/// The app, as the lock's holder: a writer session of the same project, which commits a forwarded
/// Tx only while its delta has the hash it was sent with, as the app's job does. It records each
/// step as an agent's.
struct Holder {
    s: Arc<Mutex<Session>>,
    sent: Sent,
}

impl Forward for Holder {
    fn commit(&mut self, s: &Sending) -> Result<Landed, Refusal> {
        let (tx, seen) = (s.tx, s.seen);
        self.sent.lock().unwrap().push((tx.clone(), seen, s.assets.clone()));
        let mut s = self.s.lock().unwrap();
        s.act_as(Source::Agent("Claude".into()), Some("brighter hats".into()));
        let done = s.commit_with(tx.clone(), |_, c| if c.hash == seen { Ok(()) } else { Err(c.hash) });
        s.act_as(Source::Page, None);
        match done {
            Ok(c) => Ok(Landed { step: s.last_step(), number: c.number, code_rev: c.code_rev, data_rev: c.data_rev }),
            Err(Uncommitted::Refused(r)) => Err(r),
            Err(Uncommitted::Asserted(h)) => Err(Refusal::Io { message: format!("the delta is {h}, not what was seen") }),
        }
    }

    fn save(&mut self, file: &TxPath, base: &m2_song::tx::Base, text: &str) -> Result<Landed, Refusal> {
        let mut s = self.s.lock().unwrap();
        s.act_as(Source::Agent("Claude".into()), Some("fixed".into()));
        let saved = s.save_built(&file.path(), base.clone(), text);
        s.act_as(Source::Page, None);
        saved?;
        let a = s.accepted();
        Ok(Landed { step: s.last_step(), number: a.number, code_rev: a.code_rev, data_rev: a.data_rev })
    }
}

/// A read-only session of `d` whose commits go to a writer session of it, and what that writer
/// was sent.
fn forwarding(d: &Dir) -> (Session, Arc<Mutex<Session>>, Sent) {
    let holder = Arc::new(Mutex::new(d.open()));
    let mut s = Session::open_read_only(&d.0, std_packages()).unwrap();
    let sent = Sent::default();
    s.forward_to(Box::new(Holder { s: holder.clone(), sent: sent.clone() }));
    (s, holder, sent)
}

// AGENT-1: a commit in a forwarding session is checked here, then sent with the hash of that
// check; the holder commits it, and the numbers are the holder's. Nothing is written here.
#[test]
fn a_forwarding_session_sends_its_checked_tx_with_seen() {
    let d = Dir::first_song();
    let (mut s, holder, sent) = forwarding(&d);
    let tx = pan(&d, 0.4);
    let checked = s.check(&tx).unwrap();
    let done = s.commit(tx).unwrap();
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 1, "sent once");
    assert_eq!(sent[0].1, checked.hash, "with the hash of its check");
    assert_eq!(done.hash, checked.hash);
    let h = holder.lock().unwrap();
    assert_eq!((done.number, done.code_rev), (h.accepted().number, h.accepted().code_rev), "the holder's numbers");
    assert!(done.wrote.is_empty(), "nothing was written here");
    assert!(d.read("song.ts").contains("pan: 0.4"), "the holder wrote it");
    assert_eq!(s.landed().map(|l| l.step), Some(h.last_step()), "where it landed: the holder's step");
    // A Tx that doesn't check here is never sent.
    drop((sent, h));
    let mut stale = pan(&d, 0.5);
    stale.base.insert(p("song.ts"), m2_song::tx::Base::text("no such text"));
    assert!(s.commit(stale).is_err());
    assert_eq!(holder.lock().unwrap().history.len(), 1, "a refused Tx isn't sent");
}

// AGENT-1: what is sent keeps its own expectation (a `Values` Tx stays one), so the holder's step
// is judged when undone, not raw; it is the agent's, named by its words.
#[test]
fn a_forwarded_tx_keeps_its_own_expectation() {
    let d = Dir::first_song();
    let (mut s, holder, sent) = forwarding(&d);
    s.commit(pan(&d, 0.4)).unwrap();
    assert!(matches!(sent.lock().unwrap()[0].0.expect, Expect::Values(_)), "sent as a Values Tx");
    let h = holder.lock().unwrap();
    let step = h.history.last().expect("one step");
    assert!(!step.raw, "judged when undone");
    let name = h.step_names().0.unwrap();
    assert_eq!(name.source, Source::Agent("Claude".into()));
    assert_eq!(name.name.as_deref(), Some("brighter hats"));
}

// AGENT-1: once it landed, the session reads what the holder wrote: the next edit plans on it,
// and a file the holder removed is gone here too.
#[test]
fn after_forwarding_the_session_reads_what_landed() {
    let d = Dir::first_song();
    d.write("data/hats/old.json", "{\"clips\": []}\n");
    let (mut s, _holder, _sent) = forwarding(&d);
    s.commit(pan(&d, 0.4)).unwrap();
    assert!(s.accepted().snapshot.files[std::path::Path::new("song.ts")].contains("pan: 0.4"), "read what landed");
    s.commit(pan(&d, 0.5)).expect("planned on what landed");
    assert!(d.read("song.ts").contains("pan: 0.5"));
    let remove = based(&d, vec![Op::Remove { file: p("data/hats/old.json") }], Expect::Identical);
    s.commit(remove).unwrap();
    assert!(!s.accepted().snapshot.files.contains_key(std::path::Path::new("data/hats/old.json")), "gone here too");
}

// AGENT-1: undo and redo are the holder's (`m2 undo`): a forwarding session refuses them and
// sends nothing.
#[test]
fn a_forwarding_session_refuses_undo() {
    let d = Dir::first_song();
    let (mut s, holder, sent) = forwarding(&d);
    s.commit(pan(&d, 0.4)).unwrap();
    for undo in [true, false] {
        match if undo { s.undo() } else { s.redo() } {
            Err(StepError::ReadOnly(why)) => assert!(why.contains("open app"), "{why}"),
            other => panic!("refused: {other:?}"),
        }
    }
    assert_eq!(sent.lock().unwrap().len(), 1, "nothing more was sent");
    assert!(d.read("song.ts").contains("pan: 0.4"), "nothing was undone");
    assert_eq!(holder.lock().unwrap().history.len(), 1);
}

// AGENT-3: retract takes back the last commit, undo or redo, and leaves undo and redo as they
// were before it: a commit's cleared redo comes back, an undone step is the newest again with its
// id, and nothing is left to retract.
#[test]
fn retract_leaves_undo_and_redo_as_they_were() {
    let d = Dir::first_song();
    let mut s = d.open();
    let ids = |s: &Session| (s.history.iter().map(|t| t.id).collect::<Vec<_>>(), s.redo.iter().map(|t| t.id).collect::<Vec<_>>());
    s.commit(pan(&d, 0.4)).unwrap();
    s.commit(pan(&d, 0.5)).unwrap();
    s.undo().unwrap();
    let before = ids(&s);
    assert_eq!((before.0.len(), before.1.len()), (1, 1));
    s.commit(pan(&d, 0.6)).unwrap();
    assert!(s.redo.is_empty(), "a commit ends redo");
    s.retract().expect("taken back");
    assert_eq!(ids(&s), before, "undo and redo as before the commit");
    assert!(d.read("song.ts").contains("pan: 0.4"), "its text as before");
    assert!(matches!(s.retract(), Err(StepError::Nothing(_))), "nothing more to take back");
    // An undo taken back: the step is the newest again, with its id; redo as it was.
    s.undo().unwrap();
    assert!(d.read("song.ts").contains("pan: 0.3"), "undone");
    s.retract().expect("taken back");
    assert_eq!(ids(&s), before);
    assert!(d.read("song.ts").contains("pan: 0.4"));
    // And a redo.
    s.redo().unwrap();
    assert!(d.read("song.ts").contains("pan: 0.5"));
    s.retract().expect("taken back");
    assert_eq!(ids(&s), before);
    assert!(d.read("song.ts").contains("pan: 0.4"));
}

// AGENT-3, WRITE-21 (the review's F5): a commit into a full history pushes its oldest step out;
// taking that commit back puts the step back, so undo is as it was, all of it. Once the commit is
// kept, the step is gone for good.
#[test]
fn retract_brings_back_what_a_full_history_pushed_out() {
    let d = Dir::first_song();
    let mut s = d.open();
    s.commit(pan(&d, 0.4)).unwrap();
    // A full history: 500 steps (copies of that one, each with an id of its own).
    let one = s.history[0].clone();
    for id in 2..=500 {
        let mut t = one.clone();
        t.id = id;
        s.history.push(t);
    }
    s.steps = 500;
    let ids = |s: &Session| s.history.iter().map(|t| t.id).collect::<Vec<_>>();
    let before = ids(&s);
    assert_eq!((before.len(), before[0]), (500, 1));
    s.act_as(Source::Agent("Claude".into()), None);
    s.commit(pan(&d, 0.5)).unwrap();
    s.act_as(Source::Page, None);
    assert_eq!((s.history.len(), s.history[0].id), (500, 2), "the oldest step was pushed out");
    s.retract().expect("taken back");
    assert_eq!(ids(&s), before, "undo as it was, its oldest step too");
    assert!(d.read("song.ts").contains("pan: 0.4"));
    // Kept, the commit's push is for good.
    s.commit(pan(&d, 0.5)).unwrap();
    s.keep_change();
    assert_eq!((s.history.len(), s.history[0].id), (500, 2));
}

// WRITE-2, AGENT-1 (the review's F12): an agent's save lands only when the song takes it in as
// written. While the code on disk doesn't build (the musician saved a mistake), one that leaves it
// broken is refused with its build's errors, and writes nothing; one that fixes it is one undo step.
// A forwarding session sends it to the holder, then reads what landed.
#[test]
fn an_agents_save_lands_only_as_written() {
    use m2_song::tx::Base;
    use std::path::Path;
    let d = Dir::first_song();
    let mut s = d.open();
    let kick = Path::new("tracks/kick.ts");
    let good = d.read("tracks/kick.ts");
    let broken = good.replace("ms(150)", "ms(150");
    assert!(!s.save(kick, Base::text(&good), &broken).unwrap().recorded, "the musician's mistake: no step");
    let steps = s.last_step();
    let worse = broken.replace("ms(150", "ms(120) +");
    match s.save_built(kick, Base::text(&broken), &worse) {
        Err(Refusal::Rejected { diagnostics, .. }) => assert!(diagnostics.iter().any(|d| d.code == "code.build"), "{diagnostics:?}"),
        other => panic!("refused: {:?}", other.map(|s| s.recorded)),
    }
    assert_eq!((d.read("tracks/kick.ts"), s.last_step()), (broken.clone(), steps), "nothing written, no step");
    // Through a forwarding session, to the holder.
    let holder = Arc::new(Mutex::new(s));
    let mut reader = Session::open_read_only(&d.0, std_packages()).unwrap();
    assert!(!reader.accepted().has_code(), "the code on disk doesn't build");
    reader.forward_to(Box::new(Holder { s: holder.clone(), sent: Sent::default() }));
    let fixed = broken.replace("ms(150", "ms(120)");
    assert!(reader.save_built(kick, Base::text(&broken), &fixed).unwrap().recorded);
    assert_eq!(d.read("tracks/kick.ts"), fixed);
    let s = holder.lock().unwrap();
    assert_eq!(s.last_step(), steps + 1, "one step");
    assert_eq!(s.step_names().0.unwrap().source, Source::Agent("Claude".into()));
    assert!(reader.accepted().has_code(), "the reader read what landed: it builds");
}
