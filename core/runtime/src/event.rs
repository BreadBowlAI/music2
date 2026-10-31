//! Events (RENDER-12, RENDER-14): what reaches instruments, per block, stamped with the frame
//! in the block. The shape is reserved whole; note on and note off are produced, and `Midi` for a
//! lane's pitch wheel and mod wheel from live input (D8 of track pm).

/// What a note expression changes (reserved: MPE and per-note pitch come later).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expression {
    Pitch,
    Pressure,
    Timbre,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// `from_pitch`: glide and legato (later).
    On {
        frame: u32,
        id: u32,
        pitch: f32,
        velocity: f32,
        from_pitch: Option<f32>,
    },
    /// `velocity`: release velocity.
    Off {
        frame: u32,
        id: u32,
        velocity: f32,
    },
    NoteExpression {
        frame: u32,
        id: u32,
        kind: Expression,
        value: f32,
    },
    Param {
        frame: u32,
        id: u32,
        value: f32,
    },
    Midi {
        frame: u32,
        bytes: [u8; 3],
    },
}

impl Event {
    pub fn frame(&self) -> u32 {
        match *self {
            Event::On { frame, .. }
            | Event::Off { frame, .. }
            | Event::NoteExpression { frame, .. }
            | Event::Param { frame, .. }
            | Event::Midi { frame, .. } => frame,
        }
    }

    /// The event as a kernel reads it (ABI-8, `docs/kernel-abi.md`): six 4-byte words, the kind
    /// (0 On, 1 Off, 2 note expression, 3 param, 4 MIDI), the frame in the block, the note's id, and
    /// three values: an On's pitch, velocity and `from_pitch` (NaN for none); an Off's release
    /// velocity; a note expression's kind (0 pitch, 1 pressure, 2 timbre) and value; a param's
    /// value; a MIDI message's three bytes in the low bytes of the first. Unused words are 0.
    pub(crate) fn words(&self) -> [u32; 6] {
        match *self {
            Event::On { frame, id, pitch, velocity, from_pitch } => {
                [0, frame, id, pitch.to_bits(), velocity.to_bits(), from_pitch.unwrap_or(f32::NAN).to_bits()]
            }
            Event::Off { frame, id, velocity } => [1, frame, id, velocity.to_bits(), 0, 0],
            Event::NoteExpression { frame, id, kind, value } => {
                let kind = match kind {
                    Expression::Pitch => 0,
                    Expression::Pressure => 1,
                    Expression::Timbre => 2,
                };
                [2, frame, id, kind, value.to_bits(), 0]
            }
            Event::Param { frame, id, value } => [3, frame, id, value.to_bits(), 0, 0],
            Event::Midi { frame, bytes: [a, b, c] } => [4, frame, 0, u32::from_le_bytes([a, b, c, 0]), 0, 0],
        }
    }

    /// At one frame, the wheels come first (RENDER-14: before the notes), then offs, so a key can
    /// be struck again at the frame it was let go.
    fn rank(&self) -> u8 {
        match self {
            Event::Midi { .. } => 0,
            Event::Off { .. } => 1,
            _ => 2,
        }
    }
}

/// One block's events, in time order. Its capacity is fixed when it is made: pushing never
/// allocates (RENDER-28). A full buffer refuses an On and counts it; Offs and the wheels always
/// have room, because a scheduler sizes the buffer for one Off per note it can hold, and one event
/// per wheel.
#[derive(Debug)]
pub struct Events {
    buf: Vec<Event>,
    dropped_ons: u32,
}

impl Events {
    pub fn with_capacity(n: usize) -> Events {
        Events { buf: Vec::with_capacity(n), dropped_ons: 0 }
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Whether `n` more events fit.
    pub fn room(&self, n: usize) -> bool {
        self.buf.len() + n <= self.buf.capacity()
    }

    /// Add `e`; false when there is no room (an On is then counted as dropped).
    pub fn push(&mut self, e: Event) -> bool {
        if !self.room(1) {
            debug_assert!(matches!(e, Event::On { .. }), "an Off or a wheel must always have room");
            self.drop_on();
            return false;
        }
        self.buf.push(e);
        true
    }

    /// Count an On that was dropped for want of room.
    pub fn drop_on(&mut self) {
        self.dropped_ons += 1;
    }

    /// Ons dropped since the buffer was made.
    pub fn dropped_ons(&self) -> u32 {
        self.dropped_ons
    }

    pub fn as_slice(&self) -> &[Event] {
        &self.buf
    }

    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Put the events in time order, offs first at each frame, keeping the order they were
    /// pushed in otherwise. In place: an insertion sort, as the buffer is nearly sorted and a
    /// stable library sort may allocate.
    pub fn sort(&mut self) {
        let b = &mut self.buf;
        for i in 1..b.len() {
            let mut j = i;
            let key = (b[i].frame(), b[i].rank());
            while j > 0 && (b[j - 1].frame(), b[j - 1].rank()) > key {
                b.swap(j - 1, j);
                j -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(frame: u32, id: u32) -> Event {
        Event::On { frame, id, pitch: 60.0, velocity: 1.0, from_pitch: None }
    }

    fn off(frame: u32, id: u32) -> Event {
        Event::Off { frame, id, velocity: 0.0 }
    }

    // RENDER-14: events are in time order, offs first at a frame, stable otherwise; a full
    // buffer drops Ons, counted, and never reallocates.
    #[test]
    fn sorted_stable_and_fixed() {
        let mut e = Events::with_capacity(6);
        for x in [off(9, 1), on(3, 2), on(9, 3), off(9, 4), on(3, 5), off(0, 6)] {
            assert!(e.push(x));
        }
        e.sort();
        assert_eq!(e.as_slice(), [off(0, 6), on(3, 2), on(3, 5), off(9, 1), off(9, 4), on(9, 3)]);
        assert!(!e.push(on(10, 7)));
        assert_eq!((e.dropped_ons(), e.capacity()), (1, 6));
    }
}
