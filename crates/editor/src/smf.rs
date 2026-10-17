//! Standard MIDI Files (SMF 1.0), read and written: what music2 swaps with other music apps
//! (studio lane C, Q31, D10, D37). No crate: the format is small, and what music2 keeps of it is
//! smaller.
//!
//! Read: formats 0 and 1, time in ticks per quarter note. Kept: notes, program changes, tempo
//! (`FF 51`), time signatures (`FF 58`), each track's first name (`FF 03`) and where it ends.
//! Skipped: sysex, every other meta event, controllers, pitch bend and unknown chunks. A Note On
//! with velocity 0 is a Note Off, as every player reads it. Running status carries across meta
//! and sysex events, as players do. Refused in words, naming the byte where it went wrong: a file
//! that isn't one, SMPTE time, format 2, a division of 0, a length over 4 bytes, a chunk or an
//! event past the file's end, data with no status before it, a file over [`MAX_BYTES`] or with
//! over [`MAX_NOTES`] notes. Reading never panics, whatever the bytes.
//!
//! Written: format 0 or 1 as [`Smf`] says, full status bytes (no running status), each track
//! named first, and at one tick tempo and meter, then programs, then Offs, then Ons (D37).

use std::fmt;

/// The largest file read: what the import route's body limit takes (D10).
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
/// The most notes a file may hold (D10).
pub const MAX_NOTES: usize = 200_000;

/// A MIDI file: its format (0 or 1), its ticks per quarter note, and its tracks.
#[derive(Clone, Debug, PartialEq)]
pub struct Smf {
    pub format: u16,
    pub ppq: u16,
    pub tracks: Vec<SmfTrack>,
}

/// One track: its name (the first `FF 03`), its events by tick from its start, in file order,
/// and the tick where it ends (its End of Track, or its last event when it has none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SmfTrack {
    pub name: Option<String>,
    pub events: Vec<(u64, Event)>,
    pub end: u64,
}

/// What music2 keeps of a track's events. Channels are 0 to 15 (channel 10 is 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A Note On with a velocity of 1 to 127.
    NoteOn { channel: u8, key: u8, velocity: u8 },
    /// A Note Off, or a Note On with velocity 0.
    NoteOff { channel: u8, key: u8 },
    Program { channel: u8, program: u8 },
    /// Microseconds per quarter note.
    Tempo(u32),
    /// A time signature: `beats` over 2 to the power `unit_pow` (as written: maybe nonsense).
    Meter { beats: u8, unit_pow: u8 },
}

/// A note found in a track: its channel, key and velocity, and its start and end in ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmfNote {
    pub channel: u8,
    pub key: u8,
    pub velocity: u8,
    pub start: u64,
    pub end: u64,
}

/// Why a file doesn't read: what follows its name, in words ("isn't a MIDI file: it doesn't
/// start with MThd.").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmfError(pub String);

impl fmt::Display for SmfError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SmfError {}

fn damaged(why: String) -> SmfError {
    SmfError(format!("doesn't read as a MIDI file: {why}."))
}

/// The bytes, read from the front.
struct Bytes<'a> {
    all: &'a [u8],
    at: usize,
    /// Where the current chunk ends.
    end: usize,
}

impl Bytes<'_> {
    fn cut_short(&self) -> SmfError {
        damaged(format!("it ends early, at byte {}", self.at))
    }

    fn byte(&mut self) -> Result<u8, SmfError> {
        if self.at >= self.end {
            return Err(self.cut_short());
        }
        self.at += 1;
        Ok(self.all[self.at - 1])
    }

    fn take(&mut self, n: usize) -> Result<&[u8], SmfError> {
        if n > self.end - self.at {
            self.at = self.end;
            return Err(self.cut_short());
        }
        self.at += n;
        Ok(&self.all[self.at - n..self.at])
    }

    fn u16(&mut self) -> Result<u16, SmfError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, SmfError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A variable-length quantity: at most 4 bytes, 28 bits.
    fn vlq(&mut self) -> Result<u32, SmfError> {
        let from = self.at;
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.byte()?;
            v = (v << 7) | (b & 0x7f) as u32;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(damaged(format!("a length at byte {from} is longer than 4 bytes")))
    }
}

/// Read a MIDI file (see the module's doc).
pub fn read(bytes: &[u8]) -> Result<Smf, SmfError> {
    if bytes.len() > MAX_BYTES {
        return Err(SmfError("is over 16 MB: music2 brings in MIDI files up to 16 MB.".into()));
    }
    if !bytes.starts_with(b"MThd") {
        return Err(SmfError("isn't a MIDI file: it doesn't start with MThd.".into()));
    }
    let mut b = Bytes { all: bytes, at: 4, end: bytes.len() };
    let length = b.u32()? as usize;
    if length < 6 {
        return Err(damaged(format!("its header is {length} bytes long, not 6")));
    }
    let header_end = b.at.saturating_add(length);
    let format = b.u16()?;
    let _count = b.u16()?;
    let division = b.u16()?;
    if division & 0x8000 != 0 {
        return Err(SmfError(
            "counts time in SMPTE frames, which music2 doesn't read. Save it with beats (ticks per quarter note) instead.".into(),
        ));
    }
    if division == 0 {
        return Err(damaged("its header says a beat has 0 ticks".into()));
    }
    match format {
        0 | 1 => {}
        2 => {
            return Err(SmfError("is a format 2 MIDI file (a set of patterns), which music2 doesn't read. Save it as format 1.".into()));
        }
        f => return Err(damaged(format!("its header says format {f}, which isn't one"))),
    }
    if header_end > bytes.len() {
        b.at = bytes.len();
        return Err(b.cut_short());
    }
    b.at = header_end;
    let mut tracks = vec![];
    let mut notes = 0usize;
    while b.at < bytes.len() {
        let id = b.take(4)?;
        let is_track = id == b"MTrk";
        let length = b.u32()? as usize;
        if length > bytes.len() - b.at {
            return Err(damaged(format!("a chunk at byte {} runs past the file's end", b.at - 8)));
        }
        let end = b.at + length;
        if is_track {
            let mut chunk = Bytes { all: bytes, at: b.at, end };
            tracks.push(track(&mut chunk, &mut notes)?);
        }
        // Unknown chunks are skipped (SMF 1.0 says so).
        b.at = end;
    }
    Ok(Smf { format, ppq: division, tracks })
}

/// One `MTrk` chunk's events, up to its End of Track or its end.
fn track(b: &mut Bytes, notes: &mut usize) -> Result<SmfTrack, SmfError> {
    let mut t = SmfTrack::default();
    let mut tick = 0u64;
    let mut running: Option<u8> = None;
    while b.at < b.end {
        tick += b.vlq()? as u64;
        let at = b.at;
        let first = b.byte()?;
        let (status, first_data) = match first {
            s if s >= 0x80 => (s, None),
            d => match running {
                Some(s) => (s, Some(d)),
                None => return Err(damaged(format!("the data at byte {at} has no event before it"))),
            },
        };
        match status {
            0x80..=0xef => {
                running = Some(status);
                let channel = status & 0x0f;
                let a = match first_data {
                    Some(d) => d,
                    None => b.byte()?,
                } & 0x7f;
                let two = !matches!(status & 0xf0, 0xc0 | 0xd0);
                let c = if two { b.byte()? & 0x7f } else { 0 };
                let event = match status & 0xf0 {
                    0x90 if c > 0 => Some(Event::NoteOn { channel, key: a, velocity: c }),
                    0x80 | 0x90 => Some(Event::NoteOff { channel, key: a }),
                    0xc0 => Some(Event::Program { channel, program: a }),
                    _ => None,
                };
                if let Some(e) = event {
                    if matches!(e, Event::NoteOn { .. }) {
                        *notes += 1;
                        if *notes > MAX_NOTES {
                            return Err(SmfError("has over 200,000 notes: music2 brings in 200,000 at most.".into()));
                        }
                    }
                    t.events.push((tick, e));
                }
            }
            // Sysex: its bytes are skipped. Running status carries on past it.
            0xf0 | 0xf7 => {
                let n = b.vlq()? as usize;
                b.take(n)?;
            }
            0xff => {
                let kind = b.byte()?;
                let n = b.vlq()? as usize;
                let data = b.take(n)?;
                match (kind, data) {
                    (0x2f, _) => {
                        t.end = tick;
                        return Ok(t);
                    }
                    (0x51, [x, y, z, ..]) => t.events.push((tick, Event::Tempo(u32::from_be_bytes([0, *x, *y, *z])))),
                    (0x58, [beats, unit_pow, ..]) => t.events.push((tick, Event::Meter { beats: *beats, unit_pow: *unit_pow })),
                    (0x03, name) if t.name.is_none() => t.name = Some(text(name)),
                    _ => {}
                }
            }
            // System messages a file shouldn't hold: their data bytes are skipped.
            0xf2 => {
                b.take(2)?;
            }
            0xf1 | 0xf3 => {
                b.byte()?;
            }
            _ => {}
        }
        t.end = tick;
    }
    Ok(t)
}

/// A name's bytes as text: UTF-8 when they are, else Latin-1 (what older files use). Control
/// characters go, and spaces at either end.
fn text(bytes: &[u8]) -> String {
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&c| c as char).collect(),
    };
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

impl SmfTrack {
    /// The track's notes, by start: each On paired with the next Off of its channel and key. A
    /// note still sounding when another of its pitch starts ends there; one with no Off ends at
    /// the track's end; an Off with no On is ignored.
    pub fn notes(&self) -> Vec<SmfNote> {
        /// End the note in `slot`, if one sounds there, at `end`. One cut to no length by another
        /// On at its own tick is a repeat, and goes; one an Off ends at once is a tap, and stays.
        fn close(out: &mut Vec<SmfNote>, slot: &mut Option<(u64, u8)>, (channel, key): (u8, u8), end: u64, by_off: bool) {
            if let Some((start, velocity)) = slot.take()
                && (end > start || by_off)
            {
                out.push(SmfNote { channel, key, velocity, start, end });
            }
        }
        let mut open: Vec<Option<(u64, u8)>> = vec![None; 16 * 128];
        let mut out = vec![];
        for &(tick, e) in &self.events {
            match e {
                Event::NoteOn { channel, key, velocity } => {
                    let slot = &mut open[channel as usize * 128 + key as usize];
                    close(&mut out, slot, (channel, key), tick, false);
                    *slot = Some((tick, velocity));
                }
                Event::NoteOff { channel, key } => close(&mut out, &mut open[channel as usize * 128 + key as usize], (channel, key), tick, true),
                _ => {}
            }
        }
        let end = self.end.max(self.events.last().map_or(0, |e| e.0));
        for (i, slot) in open.iter_mut().enumerate() {
            close(&mut out, slot, ((i / 128) as u8, (i % 128) as u8), end, true);
        }
        out.sort_by_key(|n| (n.start, n.channel, n.key, n.end));
        out
    }
}

/// Where an event goes among others at its tick (D37): tempo and meter, programs, Offs, Ons.
fn rank(e: &Event) -> u8 {
    match e {
        Event::Tempo(_) | Event::Meter { .. } => 0,
        Event::Program { .. } => 1,
        Event::NoteOff { .. } => 2,
        Event::NoteOn { .. } => 3,
    }
}

fn vlq(out: &mut Vec<u8>, v: u64) {
    let v = v.min(0x0fff_ffff) as u32;
    let mut bytes = [(v & 0x7f) as u8, 0, 0, 0];
    let mut n = 1;
    let mut rest = v >> 7;
    while rest > 0 {
        bytes[n] = (rest & 0x7f) as u8 | 0x80;
        rest >>= 7;
        n += 1;
    }
    out.extend(bytes[..n].iter().rev());
}

/// Write a MIDI file (see the module's doc). Each track's events are put in time order first;
/// a track ends at its `end` or its last event, whichever is later.
pub fn write(smf: &Smf) -> Vec<u8> {
    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(smf.format.to_be_bytes());
    out.extend((smf.tracks.len() as u16).to_be_bytes());
    out.extend(smf.ppq.to_be_bytes());
    for t in &smf.tracks {
        let mut body = vec![];
        if let Some(name) = &t.name {
            body.extend([0, 0xff, 0x03]);
            vlq(&mut body, name.len() as u64);
            body.extend(name.as_bytes());
        }
        let mut events = t.events.clone();
        events.sort_by_key(|(tick, e)| (*tick, rank(e)));
        let mut now = 0;
        for (tick, e) in &events {
            vlq(&mut body, tick - now);
            now = *tick;
            match *e {
                Event::NoteOn { channel, key, velocity } => body.extend([0x90 | channel & 0x0f, key & 0x7f, velocity.clamp(1, 127)]),
                Event::NoteOff { channel, key } => body.extend([0x80 | channel & 0x0f, key & 0x7f, 0x40]),
                Event::Program { channel, program } => body.extend([0xc0 | channel & 0x0f, program & 0x7f]),
                Event::Tempo(us) => {
                    let b = us.min(0xff_ffff).to_be_bytes();
                    body.extend([0xff, 0x51, 0x03, b[1], b[2], b[3]]);
                }
                // 24 clocks a click, 8 thirty-seconds a quarter: what every file says.
                Event::Meter { beats, unit_pow } => body.extend([0xff, 0x58, 0x04, beats, unit_pow, 24, 8]),
            }
        }
        vlq(&mut body, t.end.saturating_sub(now));
        body.extend([0xff, 0x2f, 0x00]);
        out.extend(b"MTrk");
        out.extend((body.len() as u32).to_be_bytes());
        out.extend(body);
    }
    out
}

#[cfg(test)]
#[path = "smf_tests.rs"]
mod tests;
