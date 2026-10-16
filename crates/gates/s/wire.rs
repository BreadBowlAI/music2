//! What m2-gates and S's runner say to each other (G7f, RENDER-32). S is HEAD `8aa301e`, the commit
//! before step 7, played in a process of its own by the runner in this folder. m2-gates sends one
//! command a line on the runner's stdin and waits for its one-line answer on its stdout, so the two
//! never run at once: they take turns, as players in one process do.
//!
//! Both sides compile this same file: m2-gates as `baseline::wire`, and the runner, built at S, as
//! its own module. So it uses std alone, and both know whether they were built from the same code
//! ([`Reply::Ready`]).

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// A command, one line.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    /// Compile the project in `dir` and play it from `from` seconds on a player whose callback the
    /// runner runs, `frames` frames a callback.
    Player {
        from: f64,
        frames: usize,
        dir: PathBuf,
    },
    /// Compile it and build a renderer of it, timed, its transport at `from` seconds: one block a step.
    Renderer {
        from: f64,
        dir: PathBuf,
    },
    /// `n` callbacks or blocks, not timed.
    Warm(usize),
    /// `lead` more, then `n` more, each timed; only the `n` are heard. The lead is in the same
    /// command so that nothing runs between it and the `n`.
    Turn {
        lead: usize,
        n: usize,
    },
    /// The hash of what the timed ones played.
    Hash,
    /// Drop the player or renderer, timed.
    Drop,
    Quit,
}

/// An answer, one line.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// The runner's first line: the hash of its code ([`code`]) and the commit it plays.
    Ready {
        code: u64,
        s: String,
    },
    /// Done, with what was measured: `build_ms`, `count`, `drop_ms`.
    Ok(Vec<(String, f64)>),
    /// Each timed callback or block, in µs: a turn's lead first.
    Times(Vec<f64>),
    Heard(Heard),
    Error(String),
}

/// What the timed callbacks or blocks played: an FNV-1a hash of every sample's bits, and how many
/// samples. Two sides that played the same music have the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heard {
    pub hash: u64,
    pub samples: u64,
}

impl Default for Heard {
    fn default() -> Heard {
        Heard { hash: 0xcbf2_9ce4_8422_2325, samples: 0 }
    }
}

impl Heard {
    pub fn add(&mut self, samples: &[f32]) {
        for s in samples {
            for b in s.to_bits().to_le_bytes() {
                self.hash = (self.hash ^ b as u64).wrapping_mul(0x0100_0000_01b3);
            }
        }
        self.samples += samples.len() as u64;
    }

    /// A block of stereo frames.
    pub fn add_frames(&mut self, frames: &[[f32; 2]]) {
        for f in frames {
            self.add(f);
        }
    }
}

/// The hash of the runner's code, `main` and this file: m2-gates holds the same files, so a runner
/// built from other code says so at once.
pub fn code(main: &[u8], wire: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in main.iter().chain(&[0]).chain(wire) {
        h = (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl Cmd {
    pub fn line(&self) -> String {
        match self {
            Cmd::Player { from, frames, dir } => format!("player {from} {frames} {}", dir.display()),
            Cmd::Renderer { from, dir } => format!("renderer {from} {}", dir.display()),
            Cmd::Warm(n) => format!("warm {n}"),
            Cmd::Turn { lead, n } => format!("turn {lead} {n}"),
            Cmd::Hash => "hash".into(),
            Cmd::Drop => "drop".into(),
            Cmd::Quit => "quit".into(),
        }
    }

    pub fn parse(line: &str) -> Result<Cmd, String> {
        let bad = || format!("not a command: {line:?}");
        let line = line.trim_end_matches(['\r', '\n']);
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        let number = |w: &str| w.parse::<f64>().ok().filter(|x| x.is_finite());
        let count = |w: &str| w.parse::<usize>().ok();
        // A folder is last on the line, as it may hold spaces.
        let dir = |w: &str| (!w.is_empty()).then(|| PathBuf::from(w));
        Ok(match word {
            "player" => {
                let mut p = rest.splitn(3, ' ');
                let (from, frames, d) = (p.next().and_then(number), p.next().and_then(count), p.next().and_then(dir));
                Cmd::Player { from: from.ok_or_else(bad)?, frames: frames.filter(|f| *f > 0).ok_or_else(bad)?, dir: d.ok_or_else(bad)? }
            }
            "renderer" => {
                let (from, d) = rest.split_once(' ').ok_or_else(bad)?;
                Cmd::Renderer { from: number(from).ok_or_else(bad)?, dir: dir(d).ok_or_else(bad)? }
            }
            "warm" => Cmd::Warm(count(rest).ok_or_else(bad)?),
            "turn" => {
                let (lead, n) = rest.split_once(' ').ok_or_else(bad)?;
                Cmd::Turn { lead: count(lead).ok_or_else(bad)?, n: count(n).ok_or_else(bad)? }
            }
            "hash" if rest.is_empty() => Cmd::Hash,
            "drop" if rest.is_empty() => Cmd::Drop,
            "quit" if rest.is_empty() => Cmd::Quit,
            _ => return Err(bad()),
        })
    }
}

impl Reply {
    pub fn line(&self) -> String {
        match self {
            Reply::Ready { code, s } => format!("ready {code:016x} {s}"),
            Reply::Ok(values) => values.iter().fold("ok".to_string(), |l, (k, v)| format!("{l} {k}={v}")),
            Reply::Times(times) => times.iter().fold("times".to_string(), |l, t| format!("{l} {t:.3}")),
            Reply::Heard(h) => format!("heard {:016x} {}", h.hash, h.samples),
            // One line, whatever the message holds.
            Reply::Error(e) => format!("error {}", e.replace(['\r', '\n'], " ")),
        }
    }

    pub fn parse(line: &str) -> Result<Reply, String> {
        let bad = || format!("not an answer: {:?}", line.chars().take(200).collect::<String>());
        let line = line.trim_end_matches(['\r', '\n']);
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        let words = || rest.split(' ').filter(|w| !w.is_empty());
        Ok(match word {
            "ready" => {
                let (code, s) = rest.split_once(' ').ok_or_else(bad)?;
                Reply::Ready { code: u64::from_str_radix(code, 16).map_err(|_| bad())?, s: s.to_string() }
            }
            "ok" => Reply::Ok(
                words()
                    .map(|w| w.split_once('=').and_then(|(k, v)| Some((k.to_string(), v.parse().ok()?))))
                    .collect::<Option<_>>()
                    .ok_or_else(bad)?,
            ),
            "times" => Reply::Times(words().map(|w| w.parse().ok()).collect::<Option<_>>().ok_or_else(bad)?),
            "heard" => {
                let (hash, samples) = rest.split_once(' ').ok_or_else(bad)?;
                Reply::Heard(Heard {
                    hash: u64::from_str_radix(hash, 16).map_err(|_| bad())?,
                    samples: samples.parse().map_err(|_| bad())?,
                })
            }
            "error" => Reply::Error(rest.to_string()),
            _ => return Err(bad()),
        })
    }
}

/// What the runner plays: S's player or renderer, or a test's stand-in.
pub trait Deck {
    fn player(&mut self, dir: &Path, from: f64, frames: usize) -> Result<(), String>;
    /// The build's time (`build_ms`) and the kernel instances it made (`count`).
    fn renderer(&mut self, dir: &Path, from: f64) -> Result<Vec<(String, f64)>, String>;
    fn warm(&mut self, n: usize) -> Result<(), String>;
    /// `lead` callbacks or blocks, then `n`, each one's time pushed onto `times` in µs, and what
    /// each of the `n` played added to `heard` after its time was taken.
    fn turn(&mut self, lead: usize, n: usize, times: &mut Vec<f64>, heard: &mut Heard) -> Result<(), String>;
    /// Drop what plays: how long that took, in ms.
    fn drop_it(&mut self) -> f64;
}

/// The runner's side: say [`Reply::Ready`], then answer each command on `input` until `quit` or
/// its end. What the timed callbacks or blocks played is hashed anew for each player or renderer.
pub fn serve(ready: Reply, input: impl BufRead, mut output: impl Write, deck: &mut dyn Deck) -> io::Result<()> {
    writeln!(output, "{}", ready.line())?;
    output.flush()?;
    let mut heard = Heard::default();
    for line in input.lines() {
        let reply = match Cmd::parse(&line?) {
            Ok(Cmd::Quit) => break,
            Ok(Cmd::Player { from, frames, dir }) => {
                heard = Heard::default();
                deck.player(&dir, from, frames).map(|()| Reply::Ok(vec![]))
            }
            Ok(Cmd::Renderer { from, dir }) => {
                heard = Heard::default();
                deck.renderer(&dir, from).map(Reply::Ok)
            }
            Ok(Cmd::Warm(n)) => deck.warm(n).map(|()| Reply::Ok(vec![])),
            Ok(Cmd::Turn { lead, n }) => {
                let mut times = Vec::with_capacity(lead + n);
                deck.turn(lead, n, &mut times, &mut heard).map(|()| Reply::Times(times))
            }
            Ok(Cmd::Hash) => Ok(Reply::Heard(heard)),
            Ok(Cmd::Drop) => Ok(Reply::Ok(vec![("drop_ms".into(), deck.drop_it())])),
            Err(e) => Err(e),
        };
        writeln!(output, "{}", reply.unwrap_or_else(Reply::Error).line())?;
        output.flush()?;
    }
    Ok(())
}

/// m2-gates' side: commands out, answers in.
pub struct Client<R, W> {
    input: R,
    output: W,
}

impl<R: BufRead, W: Write> Client<R, W> {
    /// A client of the runner that answers on `input`: its first line, [`Reply::Ready`], read.
    pub fn new(input: R, output: W) -> Result<(Client<R, W>, u64, String), String> {
        let mut c = Client { input, output };
        match c.read()? {
            Reply::Ready { code, s } => Ok((c, code, s)),
            other => Err(format!("the runner said {:?} first, not ready", other.line())),
        }
    }

    /// Send `cmd` and wait for its answer; an error the runner sends back is an `Err`.
    pub fn ask(&mut self, cmd: &Cmd) -> Result<Reply, String> {
        writeln!(self.output, "{}", cmd.line()).and_then(|()| self.output.flush()).map_err(|e| format!("the runner is gone: {e}"))?;
        match self.read()? {
            Reply::Error(e) => Err(format!("S's runner: {e}")),
            r => Ok(r),
        }
    }

    /// Tell the runner to stop; it says nothing back.
    pub fn quit(&mut self) {
        let _ = writeln!(self.output, "{}", Cmd::Quit.line()).and_then(|()| self.output.flush());
    }

    fn read(&mut self) -> Result<Reply, String> {
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) => Err("the runner stopped without answering".into()),
            Ok(_) => Reply::parse(&line),
            Err(e) => Err(format!("reading the runner: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RENDER-32 (G7f): every command and answer reads back as itself, a folder with spaces too.
    #[test]
    fn every_line_reads_back() {
        let dir = PathBuf::from("/tmp/a lab/p50-0");
        for c in [
            Cmd::Player { from: 180.0, frames: 512, dir: dir.clone() },
            Cmd::Player { from: 0.25, frames: 1, dir: dir.clone() },
            Cmd::Renderer { from: 180.0, dir: dir.clone() },
            Cmd::Warm(94),
            Cmd::Turn { lead: 8, n: 375 },
            Cmd::Turn { lead: 0, n: 0 },
            Cmd::Hash,
            Cmd::Drop,
            Cmd::Quit,
        ] {
            assert_eq!(Cmd::parse(&c.line()), Ok(c.clone()), "{}", c.line());
        }
        for r in [
            Reply::Ready { code: 0x0123_4567_89ab_cdef, s: "8aa301e".into() },
            Reply::Ok(vec![]),
            Reply::Ok(vec![("build_ms".into(), 103.25), ("count".into(), 6058.0)]),
            Reply::Times(vec![8935.125, 12.5, 0.0]),
            Reply::Times(vec![]),
            Reply::Heard(Heard { hash: u64::MAX, samples: 2_880_000 }),
            Reply::Error("no such folder".into()),
        ] {
            assert_eq!(Reply::parse(&r.line()), Ok(r.clone()), "{}", r.line());
        }
        assert_eq!(Reply::parse(&Reply::Error("two\nlines".into()).line()), Ok(Reply::Error("two lines".into())));
    }

    // Anything else is refused, with what it was, never read as something close.
    #[test]
    fn a_wrong_line_is_refused() {
        for l in [
            "",
            "play 1 2 /x",
            "player 180 512",
            "player x 512 /x",
            "player 180 0 /x",
            "player inf 512 /x",
            "warm",
            "warm -1",
            "turn 1.5",
            "turn 2",
            "turn 2 x",
            "turn -1 94",
            "hash 1",
        ] {
            assert!(Cmd::parse(l).is_err(), "{l:?}");
        }
        for l in ["", "times 1 x", "ok build_ms", "heard 12", "ready", "ready xyz 8aa301e", "oops"] {
            assert!(Reply::parse(l).is_err(), "{l:?}");
        }
    }

    // The same samples hash the same, and one bit or one sample more doesn't.
    #[test]
    fn the_hash_hears_every_bit() {
        let (mut a, mut b) = (Heard::default(), Heard::default());
        a.add(&[0.5, -0.25]);
        b.add_frames(&[[0.5, -0.25]]);
        assert_eq!(a, b);
        assert_eq!(a.samples, 2);
        let mut c = Heard::default();
        c.add(&[0.5, f32::from_bits((-0.25f32).to_bits() ^ 1)]);
        assert_ne!(a.hash, c.hash);
        let mut d = a;
        d.add(&[0.0]);
        assert_ne!(a.hash, d.hash);
        // FNV-1a 64, by its published test value ("a" hashes to af63dc4c8601ec8c), over the two
        // files with a zero byte between them, so they are kept apart.
        assert_eq!(fnv(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(code(b"main", b"wire"), fnv(b"main\0wire"));
        assert_ne!(code(b"main", b"wire"), code(b"mai", b"nwire"));
        let mut one = Heard::default();
        one.add(&[f32::from_bits(u32::from_le_bytes(*b"abcd"))]);
        assert_eq!(one.hash, fnv(b"abcd"), "a sample's bits, little-endian");
    }

    fn fnv(bytes: &[u8]) -> u64 {
        bytes.iter().fold(Heard::default().hash, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
    }

    /// A stand-in for S: its "callbacks" play `step` and take no time.
    struct Fake {
        playing: bool,
        step: f32,
    }

    impl Deck for Fake {
        fn player(&mut self, dir: &Path, _: f64, _: usize) -> Result<(), String> {
            if !dir.exists() {
                return Err(format!("{} isn't there", dir.display()));
            }
            self.playing = true;
            Ok(())
        }
        fn renderer(&mut self, _: &Path, from: f64) -> Result<Vec<(String, f64)>, String> {
            self.playing = true;
            Ok(vec![("build_ms".into(), from), ("count".into(), 3.0)])
        }
        fn warm(&mut self, n: usize) -> Result<(), String> {
            self.step += n as f32;
            Ok(())
        }
        fn turn(&mut self, lead: usize, n: usize, times: &mut Vec<f64>, heard: &mut Heard) -> Result<(), String> {
            if !self.playing {
                return Err("nothing plays".into());
            }
            for _ in 0..lead {
                times.push(9.0);
                self.step += 1.0;
            }
            for _ in 0..n {
                times.push(1.0);
                heard.add(&[self.step]);
                self.step += 1.0;
            }
            Ok(())
        }
        fn drop_it(&mut self) -> f64 {
            self.playing = false;
            2.5
        }
    }

    // RENDER-32 (G7f): m2-gates and the runner over a pipe, as they run: each command answered in
    // order, the runner's errors returned as errors without ending it, and what the timed steps
    // played hashed from each new player on.
    #[test]
    fn a_client_and_a_runner_take_turns_over_a_pipe() {
        let (from_gates, to_runner) = io::pipe().unwrap();
        let (from_runner, to_gates) = io::pipe().unwrap();
        let runner = std::thread::spawn(move || {
            let mut deck = Fake { playing: false, step: 0.0 };
            serve(Reply::Ready { code: 7, s: "8aa301e".into() }, io::BufReader::new(from_gates), to_gates, &mut deck).unwrap();
            deck.step
        });
        let (mut c, code, s) = Client::new(io::BufReader::new(from_runner), to_runner).unwrap();
        assert_eq!((code, s.as_str()), (7, "8aa301e"));
        assert!(c.ask(&Cmd::Turn { lead: 0, n: 2 }).unwrap_err().contains("nothing plays"));
        let missing = Cmd::Player { from: 180.0, frames: 512, dir: "/no/such folder".into() };
        assert!(c.ask(&missing).unwrap_err().contains("/no/such folder isn't there"));
        let here = std::env::temp_dir();
        assert_eq!(c.ask(&Cmd::Player { from: 180.0, frames: 512, dir: here.clone() }), Ok(Reply::Ok(vec![])));
        assert_eq!(c.ask(&Cmd::Warm(10)), Ok(Reply::Ok(vec![])));
        assert_eq!(c.ask(&Cmd::Turn { lead: 2, n: 3 }), Ok(Reply::Times(vec![9.0, 9.0, 1.0, 1.0, 1.0])));
        let mut want = Heard::default();
        want.add(&[12.0, 13.0, 14.0]);
        assert_eq!(c.ask(&Cmd::Hash), Ok(Reply::Heard(want)));
        assert_eq!(c.ask(&Cmd::Drop), Ok(Reply::Ok(vec![("drop_ms".into(), 2.5)])));
        assert_eq!(c.ask(&Cmd::Renderer { from: 4.0, dir: here }), Ok(Reply::Ok(vec![("build_ms".into(), 4.0), ("count".into(), 3.0)])));
        assert_eq!(c.ask(&Cmd::Hash), Ok(Reply::Heard(Heard::default())), "a new renderer is heard anew");
        c.quit();
        assert_eq!(runner.join().unwrap(), 15.0);
    }
}
