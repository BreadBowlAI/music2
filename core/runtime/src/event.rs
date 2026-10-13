//! Events (RENDER-12, RENDER-14): what reaches instruments, per block, stamped with the frame
//! in the block. The shape is reserved whole; only note on and note off are produced so far.

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

    /// At one frame, offs come first, so a key can be struck again at the frame it was let go.
    fn rank(&self) -> u8 {
        if matches!(self, Event::Off { .. }) { 0 } else { 1 }
    }
}

/// One block's events, in time order. Its capacity is fixed when it is made: pushing never
/// allocates (RENDER-28). A full buffer refuses an On and counts it; Offs always have room,
/// because a scheduler sizes the buffer for one Off per note it can hold.
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
            debug_assert!(matches!(e, Event::On { .. }), "an Off must always have room");
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
