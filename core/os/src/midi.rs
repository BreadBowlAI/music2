//! MIDI keyboards (8b.10): every MIDI input on this machine as keyboards come and go, and the
//! messages they send. midir hides CoreMIDI (macOS) and ALSA (Linux), so this module has no OS
//! folder (the platform design's §8 row).
//!
//! Which track a key plays, the sustain pedal and what a message means are the caller's (PLAT-3).
//! Built only with the `midi` feature, which only the server turns on.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use midir::{Ignore, MidiInput, MidiInputConnection};

/// What the listener heard. A keyboard's name is the one a person reads ("Keystation 49"), the same
/// in each of its events, and two keyboards plugged in at once never share one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// A keyboard is there: plugged in now, or already when listening began.
    Plugged(&'a str),
    /// A keyboard went. No `Bytes` from it follow.
    Unplugged(&'a str),
    /// One message from a keyboard: its status byte, then its data bytes. System messages (clock,
    /// active sensing, sysex) never come.
    Bytes(&'a str, &'a [u8]),
}

/// What hears the events: on the listener's thread (`Plugged`, `Unplugged`) or on the OS's MIDI
/// thread for that keyboard (`Bytes`), never once [`Listening`] has dropped.
pub type Hear = Arc<dyn Fn(Event<'_>) + Send + Sync>;

/// How often the list of inputs is read: a keyboard plugged in is heard within this.
const POLL: Duration = Duration::from_secs(1);
/// How long after a failed open (no MIDI here, or no right to it) the next try waits.
const RETRY: Duration = Duration::from_secs(5);

/// Listening to every MIDI input, for as long as this lives. Dropping it stops the listener's
/// thread and closes every keyboard's connection.
pub struct Listening {
    stop: Arc<Stop>,
    thread: Option<JoinHandle<()>>,
}

struct Stop {
    stopped: Mutex<bool>,
    wake: Condvar,
}

impl Stop {
    /// Waits up to `d`; true once the listener should stop.
    fn wait(&self, d: Duration) -> bool {
        let g = self.stopped.lock().unwrap_or_else(|e| e.into_inner());
        let (g, _) = self.wake.wait_timeout_while(g, d, |s| !*s).unwrap_or_else(|e| e.into_inner());
        *g
    }
}

/// Listen to every MIDI input on this machine, but the system's "Midi Through" ports, on every
/// channel: those plugged in now, and each one plugged in later (within a second). A machine with
/// no MIDI, or where opening it isn't allowed, has no keyboards: never an error and never a message.
pub fn listen(on: Hear) -> Listening {
    let stop = Arc::new(Stop { stopped: Mutex::new(false), wake: Condvar::new() });
    let s = stop.clone();
    let thread = std::thread::Builder::new().name("m2-midi".into()).spawn(move || run(&on, &s)).ok();
    Listening { stop, thread }
}

impl Drop for Listening {
    fn drop(&mut self) {
        *self.stop.stopped.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.stop.wake.notify_all();
        // Dropped by what it called (the last owner let go inside an event): the thread ends on its
        // own once this returns, and can't wait for itself.
        if let Some(t) = self.thread.take()
            && t.thread().id() != std::thread::current().id()
        {
            let _ = t.join();
        }
    }
}

/// One keyboard the listener has open.
struct Open {
    id: String,
    name: String,
    _connection: MidiInputConnection<()>,
}

fn run(on: &Hear, stop: &Stop) {
    let mut lister: Option<MidiInput> = None;
    let mut tried: Option<Instant> = None;
    let mut open: Vec<Open> = Vec::new();
    // Ports that wouldn't open, and when: tried again after `RETRY`.
    let mut refused: Vec<(String, Instant)> = Vec::new();
    loop {
        if lister.is_none() && tried.is_none_or(|t| t.elapsed() >= RETRY) {
            tried = Some(Instant::now());
            lister = new_input();
        }
        let ports = lister.as_ref().map(|m| m.ports()).unwrap_or_default();
        let here: Vec<(midir::MidiInputPort, String)> = match &lister {
            Some(m) => ports.into_iter().filter_map(|p| m.port_name(&p).ok().map(|n| (p, n))).collect(),
            None => Vec::new(),
        };
        // The gone first, their connection closed before they are told of, so no message follows.
        let mut i = 0;
        while i < open.len() {
            if here.iter().any(|(p, _)| p.id() == open[i].id) {
                i += 1;
            } else {
                let gone = open.remove(i);
                let name = gone.name;
                drop(gone._connection);
                on(Event::Unplugged(&name));
            }
        }
        refused.retain(|(_, t)| t.elapsed() < RETRY);
        for (port, raw) in &here {
            let id = port.id();
            if skipped(raw) || open.iter().any(|o| o.id == id) || refused.iter().any(|(r, _)| *r == id) {
                continue;
            }
            let name = unique(&pretty(raw), &open);
            let Some(input) = new_input() else {
                refused.push((id, Instant::now()));
                continue;
            };
            let hear = on.clone();
            let n = name.clone();
            match input.connect(port, "music2 in", move |_, bytes, _| hear(Event::Bytes(&n, bytes)), ()) {
                Ok(c) => {
                    on(Event::Plugged(&name));
                    open.push(Open { id, name, _connection: c });
                }
                Err(_) => refused.push((id, Instant::now())),
            }
        }
        if stop.wait(POLL) {
            return;
        }
    }
}

fn new_input() -> Option<MidiInput> {
    if !crate::os::midi_allowed() {
        return None;
    }
    let mut m = MidiInput::new("music2").ok()?;
    m.ignore(Ignore::All);
    Some(m)
}

/// The OS's loopback ports, which echo what other apps send: never a keyboard (Q3).
pub fn skipped(raw: &str) -> bool {
    raw.starts_with("Midi Through")
}

/// A port's name as a person reads it. ALSA names a port "client:port client-id:port-id"
/// ("Keystation 49:Keystation 49 MIDI 1 24:0"): the numbers go, and a port named after its
/// client is just the client ("Keystation 49"). CoreMIDI's names are already plain.
pub fn pretty(raw: &str) -> String {
    let mut s = raw.trim();
    if let Some((head, tail)) = s.rsplit_once(' ')
        && let Some((a, b)) = tail.split_once(':')
        && !a.is_empty()
        && !b.is_empty()
        && a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit())
    {
        s = head.trim_end();
    }
    if let Some((client, port)) = s.split_once(':')
        && port.starts_with(client)
        && !client.is_empty()
    {
        s = client;
    }
    s.to_string()
}

/// `name`, or "name 2", "name 3"… when a keyboard open now already has it.
fn unique(name: &str, open: &[Open]) -> String {
    let taken = |n: &str| open.iter().any(|o| o.name == n);
    if !taken(name) {
        return name.to_string();
    }
    (2..).map(|i| format!("{name} {i}")).find(|n| !taken(n)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn listening_never_errors() {
        // Here opening MIDI is refused (`/dev/snd/seq`: EACCES), and a Mac in CI has no keyboard:
        // both are "no keyboards". Whatever is plugged in, nothing is heard once drop returns.
        let after = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let (a, d) = (after.clone(), dropped.clone());
        let l = listen(Arc::new(move |_| {
            if d.load(Ordering::SeqCst) {
                a.store(true, Ordering::SeqCst);
            }
        }));
        std::thread::sleep(Duration::from_millis(300));
        let t = Instant::now();
        drop(l);
        dropped.store(true, Ordering::SeqCst);
        assert!(t.elapsed() < Duration::from_secs(2), "stopping took {:?}", t.elapsed());
        std::thread::sleep(Duration::from_millis(100));
        assert!(!after.load(Ordering::SeqCst), "an event came after drop");
    }

    #[test]
    fn through_ports_are_skipped() {
        assert!(skipped("Midi Through:Midi Through Port-0 14:0"));
        assert!(!skipped("Keystation 49:Keystation 49 MIDI 1 24:0"));
        assert!(!skipped("Keystation 49"));
    }

    #[test]
    fn names_read_plainly() {
        assert_eq!(pretty("Keystation 49:Keystation 49 MIDI 1 24:0"), "Keystation 49");
        assert_eq!(pretty("USB Midi:Port A 20:1"), "USB Midi:Port A");
        assert_eq!(pretty("Keystation 49"), "Keystation 49");
        assert_eq!(pretty("Launchkey Mini MK3 MIDI Port"), "Launchkey Mini MK3 MIDI Port");
    }
}
