//! Signal kinds and units.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A value's unit: what the checker holds every connection to (PKG-10). A closed set, which no
/// package adds to: nine units of number, and the switch and the choice (CARD-12), which fit only
/// themselves. Written by name (`hz`, `switch`), a choice with its options (`choice<saw|square>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    /// On or off: 1 or 0 (CARD-11).
    Switch,
    /// One of a list of named options, as its place in the list, 0 to n − 1 (CARD-10). Its unit is
    /// the list, so it fits only a port of exactly those options, in their order (CARD-12).
    Choice(Names),
}

impl Unit {
    /// The units of number: each has spellings in the SDK's table (PKG-11).
    pub const ALL: [Unit; 9] =
        [Unit::Hz, Unit::Db, Unit::Gain, Unit::Semitones, Unit::Seconds, Unit::Beats, Unit::Unit01, Unit::Bipolar, Unit::Scalar];

    /// Its name, as a lane file records it (FMT-10, CARD-15): a choice's is `choice`, whatever its
    /// options; [`Unit`]'s `Display` writes them too.
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
            Unit::Switch => "switch",
            Unit::Choice(_) => "choice",
        }
    }

    /// A choice's options.
    pub fn options(self) -> Option<Names> {
        match self {
            Unit::Choice(o) => Some(o),
            _ => None,
        }
    }

    /// Whether it is a number's unit: not a switch or a choice, which no math reads (CARD-12).
    pub fn is_number(self) -> bool {
        !matches!(self, Unit::Switch | Unit::Choice(_))
    }

    /// Whether `v` is a value of this unit, and if not, what the unit holds (CARD-12): a choice
    /// holds a place, a whole number from 0 to n - 1, and a switch 0 or 1. A number's unit holds
    /// any number.
    pub fn holds(self, v: f64) -> Result<(), String> {
        let ok = match self {
            Unit::Switch => v == 0.0 || v == 1.0,
            Unit::Choice(n) => v.fract() == 0.0 && v >= 0.0 && v < n.len() as f64,
            _ => return Ok(()),
        };
        match self {
            _ if ok => Ok(()),
            Unit::Switch => Err("0 or 1".into()),
            Unit::Choice(n) => Err(format!("a whole number from 0 to {}", n.len() - 1)),
            _ => unreachable!("a number's unit holds any number"),
        }
    }

    /// Whether a value in `self` may flow into a port expecting `into`.
    /// 0..1 is a valid linear gain; every unitless range is a scalar. A switch and a choice fit
    /// only themselves (CARD-12).
    pub fn fits(self, into: Unit) -> bool {
        self == into
            || matches!(
                (self, into),
                (Unit::Unit01, Unit::Gain) | (Unit::Unit01, Unit::Scalar) | (Unit::Bipolar, Unit::Scalar) | (Unit::Gain, Unit::Scalar)
            )
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Unit::Choice(o) => write!(f, "choice<{o}>"),
            u => f.write_str(u.name()),
        }
    }
}

impl FromStr for Unit {
    type Err = String;
    fn from_str(s: &str) -> Result<Unit, String> {
        if s == "switch" {
            return Ok(Unit::Switch);
        }
        if let Some(names) = s.strip_prefix("choice<").and_then(|r| r.strip_suffix('>')) {
            return Names::new(names.split('|')).map(Unit::Choice).map_err(|e| format!("`{s}`: {e}"));
        }
        Unit::ALL.into_iter().find(|u| u.name() == s).ok_or_else(|| format!("unknown unit `{s}`"))
    }
}

impl Serialize for Unit {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Unit {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Unit, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

/// The most options a choice has (CARD-10).
pub const MOST_OPTIONS: usize = 64;
/// The longest an option's name is, in characters (CARD-10).
pub const LONGEST_OPTION: usize = 24;

/// A choice's options: their names, in place order (CARD-10). Each list is kept once for the life of
/// the process, so a unit stays a plain value (`Copy`) that compares by its names.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Names(&'static [Box<str>]);

/// Every list of options made so far, each once.
static KEPT: LazyLock<Mutex<HashSet<&'static [Box<str>]>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

impl Names {
    /// The options `names`, in order, or what is wrong with them: 1 to 64 names, each named once,
    /// each one line of 1 to 24 characters with no space at either end and none of `|`, `<`, `>`.
    pub fn new<S: AsRef<str>>(names: impl IntoIterator<Item = S>) -> Result<Names, String> {
        let names: Vec<Box<str>> = names.into_iter().map(|n| Box::from(n.as_ref())).collect();
        if names.is_empty() || names.len() > MOST_OPTIONS {
            return Err(format!("a choice has 1 to {MOST_OPTIONS} options, not {}", names.len()));
        }
        for (i, n) in names.iter().enumerate() {
            if let Some(why) = name_problem(n) {
                return Err(format!("option {}, {:?}: {why}", i + 1, &**n));
            }
            if names[..i].contains(n) {
                return Err(format!("the option {:?} is named twice", &**n));
            }
        }
        let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(o) = kept.get(&names[..]) {
            return Ok(Names(o));
        }
        let o: &'static [Box<str>] = Box::leak(names.into_boxed_slice());
        kept.insert(o);
        Ok(Names(o))
    }

    /// The names, in place order.
    pub fn names(self) -> &'static [Box<str>] {
        self.0
    }

    /// The place of the option named `name`, if the choice has it.
    pub fn place(self, name: &str) -> Option<usize> {
        self.0.iter().position(|n| &**n == name)
    }

    /// The name of the option at `place`.
    pub fn name(self, place: usize) -> Option<&'static str> {
        self.0.get(place).map(|n| &**n)
    }

    pub fn len(self) -> usize {
        self.0.len()
    }

    pub fn is_empty(self) -> bool {
        self.0.is_empty()
    }
}

/// What is wrong with `n` as an option's name, if anything (CARD-10).
fn name_problem(n: &str) -> Option<&'static str> {
    let chars = n.chars().count();
    if chars == 0 || chars > LONGEST_OPTION {
        Some("a name is 1 to 24 characters")
    } else if n.trim() != n {
        Some("a name has no space at either end")
    } else if n.chars().any(|c| c.is_control() || matches!(c, '|' | '<' | '>')) {
        Some("a name is one line, without |, < or >")
    } else {
        None
    }
}

impl fmt::Display for Names {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, n) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("|")?;
            }
            f.write_str(n)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Names {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list().entries(self.0.iter()).finish()
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
            Kind::Control(Some(u)) => write!(f, "control<{u}>"),
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
            PortKind::Control(Some(u)) => write!(f, "control<{u}>"),
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

    // CARD-12: a choice's unit is its list of names, in order; a switch's is `switch`. Each fits
    // only itself, and a unit is a plain value that compares by its names.
    #[test]
    fn choices_and_switches_are_units() {
        for s in ["control<switch>", "control<choice<saw|square|sine>>", "control<choice<Low pass|x=1>>"] {
            assert_eq!(s.parse::<Kind>().unwrap().to_string(), s);
            assert_eq!(s.parse::<PortKind>().unwrap().to_string(), s);
        }
        let u = |s: &str| s.parse::<Unit>().unwrap();
        let saw = u("choice<saw|square>");
        assert_eq!(saw, u("choice<saw|square>"));
        assert_ne!(saw, u("choice<square|saw>"));
        assert!(saw.fits(u("choice<saw|square>")) && !saw.fits(u("choice<square|saw>")) && !saw.fits(Unit::Scalar));
        assert!(Unit::Switch.fits(Unit::Switch) && !Unit::Switch.fits(Unit::Unit01) && !Unit::Unit01.fits(Unit::Switch));
        assert!(!Unit::Scalar.fits(saw) && !Unit::Unit01.fits(Unit::Switch));
        assert_eq!((saw.name(), Unit::Switch.name()), ("choice", "switch"));
        assert_eq!(saw.options().map(|o| o.names().iter().map(|n| &**n).collect::<Vec<_>>()), Some(vec!["saw", "square"]));
        assert_eq!(saw.options().unwrap().place("square"), Some(1));
        assert_eq!(serde_json::to_string(&saw).unwrap(), "\"choice<saw|square>\"");
        assert_eq!(serde_json::from_str::<Unit>("\"switch\"").unwrap(), Unit::Switch);
        assert_eq!(serde_json::from_str::<Unit>("\"hz\"").unwrap(), Unit::Hz);
        // Names: 1 to 64, each once, each one line of 1 to 24 characters with no space at an end.
        for bad in
            ["choice<>", "choice<a|a>", "choice<a||b>", "choice< a|b>", "choice<a|b >", "choice<abcdefghijklmnopqrstuvwxy>", "choice"]
        {
            assert!(bad.parse::<Unit>().is_err(), "{bad}");
        }
        let many = (0..65).map(|i| i.to_string()).collect::<Vec<_>>().join("|");
        assert!(format!("choice<{many}>").parse::<Unit>().is_err());
        // Only number units have spellings in the SDK's table (PKG-10, PKG-11).
        assert!(!Unit::ALL.contains(&Unit::Switch));
    }

    #[test]
    fn unit_subtyping() {
        assert!(Unit::Unit01.fits(Unit::Gain));
        assert!(!Unit::Gain.fits(Unit::Unit01));
        assert!(!Unit::Hz.fits(Unit::Db));
    }
}
