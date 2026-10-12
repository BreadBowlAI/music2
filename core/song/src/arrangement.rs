//! `data/arrangement.json`: the tempo and the meter (FMT-13). A beat is a quarter note.
//!
//! ```json
//! {
//!   "tempo": [
//!     {"beat": 0, "bpm": 124}
//!   ],
//!   "meter": [
//!     {"beat": 0, "beats": 4, "unit": 4}
//!   ]
//! }
//! ```
//!
//! Every clip's timing depends on it, so an invalid arrangement blocks acceptance (FMT-9).

use serde::{Deserialize, Serialize};

use crate::diag::Diagnostic;
use crate::lane::number;
use crate::path::DataPath;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tempo {
    pub beat: f64,
    pub bpm: f64,
}

/// A time signature from `beat` on: `beats`/`unit`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Meter {
    pub beat: f64,
    pub beats: u32,
    pub unit: u32,
}

impl Meter {
    pub const COMMON: Meter = Meter { beat: 0.0, beats: 4, unit: 4 };

    /// How long a bar is, in quarter notes: `beats × 4 / unit`.
    pub fn bar(&self) -> f64 {
        self.beats as f64 * 4.0 / self.unit as f64
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Arrangement {
    pub tempo: Vec<Tempo>,
    /// Empty means 4/4 from beat 0.
    #[serde(default)]
    pub meter: Vec<Meter>,
}

/// 120 bpm in 4/4: a project without `data/arrangement.json`.
impl Default for Arrangement {
    fn default() -> Arrangement {
        Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: 120.0 }], meter: vec![] }
    }
}

impl Arrangement {
    /// Parse and check an arrangement. Anything that breaks FMT-13 is `arrangement.invalid`,
    /// naming the entry.
    pub fn read(text: &str) -> Result<Arrangement, Diagnostic> {
        let file = DataPath::Arrangement;
        let invalid = |m: String| Diagnostic::error("arrangement.invalid", format!("{file}: {m}")).file(&file);
        let a: Arrangement = serde_json::from_str(text).map_err(|e| invalid(e.to_string()))?;
        a.check().map_err(invalid)?;
        Ok(a)
    }

    fn check(&self) -> Result<(), String> {
        let Some(first) = self.tempo.first() else { return Err("`tempo` needs an entry at beat 0".into()) };
        if first.beat != 0.0 {
            return Err(format!("tempo entry 1 is at beat {}, but the first must be at beat 0", number(first.beat)));
        }
        for (i, t) in self.tempo.iter().enumerate() {
            let at = format!("tempo entry {} (beat {})", i + 1, number(t.beat));
            if !(20.0..=999.0).contains(&t.bpm) {
                return Err(format!("{at}: {} bpm is outside 20 to 999", number(t.bpm)));
            }
            if i > 0 && t.beat <= self.tempo[i - 1].beat {
                return Err(format!(
                    "{at}: tempo beats must strictly increase, but it comes after beat {}",
                    number(self.tempo[i - 1].beat)
                ));
            }
        }
        if let Some(first) = self.meter.first()
            && first.beat != 0.0
        {
            return Err(format!("meter entry 1 is at beat {}, but the first must be at beat 0", number(first.beat)));
        }
        for (i, m) in self.meter.iter().enumerate() {
            let at = format!("meter entry {} (beat {})", i + 1, number(m.beat));
            if !(1..=64).contains(&m.beats) {
                return Err(format!("{at}: `beats` is {}, but must be 1 to 64", m.beats));
            }
            if ![1, 2, 4, 8, 16, 32].contains(&m.unit) {
                return Err(format!("{at}: `unit` is {}, but must be 1, 2, 4, 8, 16 or 32", m.unit));
            }
            if i > 0 {
                let prev = self.meter[i - 1];
                if m.beat <= prev.beat {
                    return Err(format!("{at}: meter beats must strictly increase, but it comes after beat {}", number(prev.beat)));
                }
                let bar = prev.bar();
                // Bars of `n/2^k` are exact binary fractions, so this division is exact.
                if ((m.beat - prev.beat) / bar).fract() != 0.0 {
                    return Err(format!(
                        "{at}: a meter change must fall on a bar line of the {}/{} before it, every {} beats from beat {}",
                        prev.beats,
                        prev.unit,
                        number(bar),
                        number(prev.beat)
                    ));
                }
            }
        }
        Ok(())
    }

    /// The tempo map. The arrangement must have been read (checked).
    pub fn tempo_map(&self) -> TempoMap {
        TempoMap::new(&self.tempo)
    }

    /// The meter in effect at `beat`.
    pub fn meter_at(&self, beat: f64) -> Meter {
        let i = self.meter.partition_point(|m| m.beat <= beat);
        i.checked_sub(1).map_or(Meter::COMMON, |i| self.meter[i])
    }

    /// Which bar `beat` falls in (0 is the first) and how far into it, in quarter notes.
    pub fn bar_at(&self, beat: f64) -> (u64, f64) {
        let meters = if self.meter.is_empty() { &[Meter::COMMON][..] } else { &self.meter[..] };
        let mut bars = 0u64;
        for (i, m) in meters.iter().enumerate() {
            let bar = m.bar();
            match meters.get(i + 1) {
                Some(next) if beat >= next.beat => bars += ((next.beat - m.beat) / bar).round() as u64,
                _ => {
                    let k = ((beat - m.beat) / bar).floor().max(0.0);
                    return (bars + k as u64, beat - m.beat - k * bar);
                }
            }
        }
        unreachable!("the last meter runs forever")
    }

    /// The canonical form (FMT-8a): fixed key order, shortest numbers, one entry per line.
    pub fn canonical(&self) -> String {
        let tempo: Vec<String> =
            self.tempo.iter().map(|t| format!("{{\"beat\": {}, \"bpm\": {}}}", number(t.beat), number(t.bpm))).collect();
        let meter: Vec<String> =
            self.meter.iter().map(|m| format!("{{\"beat\": {}, \"beats\": {}, \"unit\": {}}}", number(m.beat), m.beats, m.unit)).collect();
        let list = |items: Vec<String>| if items.is_empty() { "[]".to_string() } else { format!("[\n    {}\n  ]", items.join(",\n    ")) };
        format!("{{\n  \"tempo\": {},\n  \"meter\": {}\n}}\n", list(tempo), list(meter))
    }
}

/// Piecewise-constant tempo: song time in beats ↔ seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct TempoMap {
    /// Strictly increasing, the first at beat 0, each with the seconds at its start.
    segs: Vec<(Tempo, f64)>,
}

impl TempoMap {
    /// From checked tempo entries ([`Arrangement::read`]).
    fn new(tempo: &[Tempo]) -> TempoMap {
        let mut segs: Vec<(Tempo, f64)> = Vec::with_capacity(tempo.len());
        for t in tempo {
            let secs = segs.last().map_or(0.0, |(prev, at)| at + (t.beat - prev.beat) * 60.0 / prev.bpm);
            segs.push((*t, secs));
        }
        TempoMap { segs }
    }

    pub fn constant(bpm: f64) -> TempoMap {
        TempoMap::new(&[Tempo { beat: 0.0, bpm }])
    }

    pub fn seconds_at(&self, beat: f64) -> f64 {
        let i = self.segs.partition_point(|(s, _)| s.beat <= beat).saturating_sub(1);
        let (s, at) = self.segs[i];
        at + (beat - s.beat) * 60.0 / s.bpm
    }

    pub fn beat_at(&self, seconds: f64) -> f64 {
        let i = self.segs.partition_point(|(_, at)| *at <= seconds).saturating_sub(1);
        let (s, at) = self.segs[i];
        s.beat + (seconds - at) * s.bpm / 60.0
    }

    /// Tempo at a beat.
    pub fn bpm_at(&self, beat: f64) -> f64 {
        let i = self.segs.partition_point(|(s, _)| s.beat <= beat).saturating_sub(1);
        self.segs[i].0.bpm
    }

    pub fn segments(&self) -> Vec<Tempo> {
        self.segs.iter().map(|(s, _)| *s).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Result<Arrangement, String> {
        Arrangement::read(text).map_err(|d| {
            assert_eq!((d.code, d.src.as_deref()), ("arrangement.invalid", Some("data/arrangement.json")));
            d.message
        })
    }

    #[test]
    fn tempo_map_round_trips() {
        let a = read(r#"{"tempo": [{"beat": 0, "bpm": 120}, {"beat": 8, "bpm": 60}], "meter": []}"#).unwrap();
        let t = a.tempo_map();
        assert_eq!(t.seconds_at(4.0), 2.0);
        assert_eq!(t.seconds_at(8.0), 4.0);
        assert_eq!(t.seconds_at(10.0), 6.0);
        for b in [0.0, 3.5, 8.0, 12.25] {
            assert!((t.beat_at(t.seconds_at(b)) - b).abs() < 1e-9);
        }
        assert_eq!(t.bpm_at(9.0), 60.0);
    }

    // FMT-13: tempo rules, each naming the entry.
    #[test]
    fn tempo_rules() {
        let e = |tempo: &str| read(&format!(r#"{{"tempo": {tempo}}}"#)).unwrap_err();
        assert!(e("[]").contains("beat 0"));
        assert!(e(r#"[{"beat": 1, "bpm": 120}]"#).contains("tempo entry 1"));
        assert!(e(r#"[{"beat": 0, "bpm": 120}, {"beat": 4, "bpm": 5}]"#).contains("tempo entry 2 (beat 4)"));
        assert!(e(r#"[{"beat": 0, "bpm": 1000}]"#).contains("20 to 999"));
        // Two entries at one beat would be decided by file order (FMT-8a), so they are refused.
        assert!(e(r#"[{"beat": 0, "bpm": 120}, {"beat": 0, "bpm": 60}]"#).contains("strictly increase"));
        assert!(e(r#"[{"beat": 0, "bpm": 120}, {"beat": 8, "bpm": 60}, {"beat": 4, "bpm": 90}]"#).contains("tempo entry 3"));
        assert!(read(r#"{"tempo": [{"beat": 0, "bpm": 20}, {"beat": 1, "bpm": 999}]}"#).is_ok());
    }

    // FMT-13: meter rules.
    #[test]
    fn meter_rules() {
        let m = |meter: &str| read(&format!(r#"{{"tempo": [{{"beat": 0, "bpm": 120}}], "meter": {meter}}}"#));
        assert!(m("[]").is_ok());
        assert!(m(r#"[{"beat": 0, "beats": 7, "unit": 8}, {"beat": 7, "beats": 4, "unit": 4}]"#).is_ok(), "two bars of 7/8");
        assert!(m(r#"[{"beat": 0, "beats": 3, "unit": 4}, {"beat": 6, "beats": 5, "unit": 32}]"#).is_ok());
        assert!(m(r#"[{"beat": 4, "beats": 4, "unit": 4}]"#).unwrap_err().contains("meter entry 1"));
        assert!(m(r#"[{"beat": 0, "beats": 0, "unit": 4}]"#).unwrap_err().contains("1 to 64"));
        assert!(m(r#"[{"beat": 0, "beats": 65, "unit": 4}]"#).unwrap_err().contains("1 to 64"));
        assert!(m(r#"[{"beat": 0, "beats": 4, "unit": 3}]"#).unwrap_err().contains("1, 2, 4, 8, 16 or 32"));
        assert!(
            m(r#"[{"beat": 0, "beats": 4, "unit": 4}, {"beat": 6, "beats": 3, "unit": 4}]"#)
                .unwrap_err()
                .contains("meter entry 2 (beat 6)")
        );
        assert!(m(r#"[{"beat": 0, "beats": 7, "unit": 8}, {"beat": 3.5, "beats": 4, "unit": 4}]"#).is_ok(), "one bar of 7/8");
        assert!(
            m(r#"[{"beat": 0, "beats": 4, "unit": 4}, {"beat": 0, "beats": 3, "unit": 4}]"#).unwrap_err().contains("strictly increase")
        );
        assert!(m(r#"[{"beat": 0, "beats": 4.5, "unit": 4}]"#).is_err());
    }

    // FMT-8: unknown fields are refused.
    #[test]
    fn strict() {
        assert!(read(r#"{"tempo": [{"beat": 0, "bpm": 120}], "beats_per_bar": 4}"#).unwrap_err().contains("beats_per_bar"));
        assert!(read(r#"{"tempo": [{"beat": 0, "bpm": 120, "curve": 1}]}"#).unwrap_err().contains("curve"));
        assert!(read(r#"{"tempo": [{"beat": 0, "bpm": 120}], "meter": [{"beat": 0, "beats": 4, "unit": 4, "x": 1}]}"#).is_err());
    }

    #[test]
    fn bars() {
        let a = read(
            r#"{"tempo": [{"beat": 0, "bpm": 120}], "meter": [{"beat": 0, "beats": 3, "unit": 4}, {"beat": 6, "beats": 7, "unit": 8}]}"#,
        )
        .unwrap();
        assert_eq!(a.bar_at(0.0), (0, 0.0));
        assert_eq!(a.bar_at(4.0), (1, 1.0));
        assert_eq!(a.bar_at(6.0), (2, 0.0));
        assert_eq!(a.bar_at(10.0), (3, 0.5));
        assert_eq!(a.meter_at(7.0), Meter { beat: 6.0, beats: 7, unit: 8 });
        assert_eq!(Arrangement::default().bar_at(9.0), (2, 1.0));
        assert_eq!(Arrangement::default().meter_at(9.0), Meter::COMMON);
    }

    // FMT-8a: one canonical form; reading and writing it gives the same bytes.
    #[test]
    fn canonical_round_trip() {
        let a =
            read(r#"{"meter": [{"unit": 8, "beats": 7, "beat": 0}], "tempo": [{"bpm": 124.0, "beat": -0.0}, {"beat": 32, "bpm": 90.5}]}"#)
                .unwrap();
        let text = a.canonical();
        assert_eq!(
            text,
            "{\n  \"tempo\": [\n    {\"beat\": 0, \"bpm\": 124},\n    {\"beat\": 32, \"bpm\": 90.5}\n  ],\n  \"meter\": [\n    {\"beat\": 0, \"beats\": 7, \"unit\": 8}\n  ]\n}\n"
        );
        assert_eq!(read(&text).unwrap().canonical(), text);
        assert_eq!(Arrangement::default().canonical(), "{\n  \"tempo\": [\n    {\"beat\": 0, \"bpm\": 120}\n  ],\n  \"meter\": []\n}\n");
    }
}
