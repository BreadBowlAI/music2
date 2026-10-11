//! Signal kinds and units.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Hz,
    Db,
    Gain,
    Semitones,
    Seconds,
    Beats,
    Unit01,
    Bipolar,
    Scalar,
}

impl Unit {
    pub const ALL: [Unit; 9] = [
        Unit::Hz,
        Unit::Db,
        Unit::Gain,
        Unit::Semitones,
        Unit::Seconds,
        Unit::Beats,
        Unit::Unit01,
        Unit::Bipolar,
        Unit::Scalar,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Unit::Hz => "hz",
            Unit::Db => "db",
            Unit::Gain => "gain",
            Unit::Semitones => "semitones",
            Unit::Seconds => "seconds",
            Unit::Beats => "beats",
            Unit::Unit01 => "unit01",
            Unit::Bipolar => "bipolar",
            Unit::Scalar => "scalar",
        }
    }

    /// Whether a value in `self` may flow into a port expecting `into`.
    /// 0..1 is a valid linear gain; every unitless range is a scalar.
    pub fn fits(self, into: Unit) -> bool {
        self == into
            || matches!(
                (self, into),
                (Unit::Unit01, Unit::Gain) | (Unit::Unit01, Unit::Scalar) | (Unit::Bipolar, Unit::Scalar) | (Unit::Gain, Unit::Scalar)
            )
    }
}

impl FromStr for Unit {
    type Err = String;
    fn from_str(s: &str) -> Result<Unit, String> {
        Unit::ALL.into_iter().find(|u| u.name() == s).ok_or_else(|| format!("unknown unit `{s}`"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventType {
    Note,
}

/// The kind of a concrete signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Sound with 1 or 2 channels.
    Audio(u8),
    /// A continuous value. `None` is an untyped literal, which fits any unit.
    Control(Option<Unit>),
    Events(EventType),
}

impl Kind {
    pub fn channels(self) -> u8 {
        match self {
            Kind::Audio(c) => c,
            _ => 1,
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Kind::Audio(c) => write!(f, "audio<{c}>"),
            Kind::Control(Some(u)) => write!(f, "control<{}>", u.name()),
            Kind::Control(None) => write!(f, "number"),
            Kind::Events(EventType::Note) => write!(f, "events<note>"),
        }
    }
}

fn generic(s: &str) -> Option<(&str, &str)> {
    let (head, rest) = s.trim().split_once('<')?;
    Some((head.trim(), rest.strip_suffix('>')?.trim()))
}

impl FromStr for Kind {
    type Err = String;
    fn from_str(s: &str) -> Result<Kind, String> {
        if s.trim() == "number" {
            return Ok(Kind::Control(None));
        }
        match generic(s) {
            Some(("audio", "1")) => Ok(Kind::Audio(1)),
            Some(("audio", "2")) => Ok(Kind::Audio(2)),
            Some(("control", u)) => Ok(Kind::Control(Some(u.parse()?))),
            Some(("events", "note")) => Ok(Kind::Events(EventType::Note)),
            _ => Err(format!("unknown kind `{s}` (expected audio<1|2>, control<unit>, events<note>)")),
        }
    }
}

impl Serialize for Kind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Kind, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

/// The kind of a kernel port, which may contain type variables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortKind {
    /// `*`: any kind; all `*` ports of a kernel share one kind.
    Any,
    /// `audio<n>`, or `audio<*>` (`None`): all `audio<*>` ports share one channel count.
    Audio(Option<u8>),
    /// `control<unit>`, or `control<*>` (`None`): all `control<*>` ports share one unit.
    Control(Option<Unit>),
    Events(EventType),
}

impl fmt::Display for PortKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PortKind::Any => write!(f, "*"),
            PortKind::Audio(None) => write!(f, "audio<*>"),
            PortKind::Audio(Some(c)) => write!(f, "audio<{c}>"),
            PortKind::Control(None) => write!(f, "control<*>"),
            PortKind::Control(Some(u)) => write!(f, "control<{}>", u.name()),
            PortKind::Events(EventType::Note) => write!(f, "events<note>"),
        }
    }
}

impl FromStr for PortKind {
    type Err = String;
    fn from_str(s: &str) -> Result<PortKind, String> {
        match s.trim() {
            "*" => return Ok(PortKind::Any),
            "audio<*>" => return Ok(PortKind::Audio(None)),
            "control<*>" => return Ok(PortKind::Control(None)),
            _ => {}
        }
        Ok(match s.parse::<Kind>()? {
            Kind::Audio(c) => PortKind::Audio(Some(c)),
            Kind::Control(u) => PortKind::Control(u),
            Kind::Events(e) => PortKind::Events(e),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip() {
        for s in ["audio<1>", "audio<2>", "control<hz>", "control<unit01>", "events<note>", "number"] {
            assert_eq!(s.parse::<Kind>().unwrap().to_string(), s);
        }
        for s in ["*", "audio<*>", "control<*>", "audio<2>", "control<db>"] {
            assert_eq!(s.parse::<PortKind>().unwrap().to_string(), s);
        }
        assert!("audio<3>".parse::<Kind>().is_err());
        assert!("control<furlongs>".parse::<Kind>().is_err());
    }

    #[test]
    fn unit_subtyping() {
        assert!(Unit::Unit01.fits(Unit::Gain));
        assert!(!Unit::Gain.fits(Unit::Unit01));
        assert!(!Unit::Hz.fits(Unit::Db));
    }
}
