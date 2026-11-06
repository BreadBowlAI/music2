//! Kernel signatures: the typed ports a package declares for each kernel.

use crate::kind::{PortKind, Unit};

#[derive(Clone, Debug, PartialEq)]
pub struct PortSig {
    pub name: String,
    pub kind: PortKind,
    /// Used when the input is not connected. Controls only.
    pub default: Option<f64>,
    /// The values the kernel makes use of, `lo..hi`, when it says (it clamps the rest).
    /// Controls only; an editor's knob spans this.
    pub range: Option<(f64, f64)>,
}

impl PortSig {
    /// Parse `"name: kind [= default] [in lo..hi]"`.
    pub fn parse(spec: &str) -> Result<PortSig, String> {
        // A choice's options may hold spaces, `=` and ` in ` (CARD-12): what follows the kind is
        // read after its last `>`.
        let after = spec.rfind('>').map_or(0, |i| i + 1);
        let (spec_, range) = match spec[after..].rsplit_once(" in ").map(|(rest, r)| (&spec[..after + rest.len()], r)) {
            Some((rest, r)) => {
                let bad = || format!("port `{spec}`: a range is written `in lo..hi`");
                let (lo, hi) = r.trim().split_once("..").ok_or_else(bad)?;
                let (lo, hi) = (lo.trim().parse::<f64>().map_err(|_| bad())?, hi.trim().parse::<f64>().map_err(|_| bad())?);
                if !(lo.is_finite() && hi.is_finite() && lo < hi) {
                    return Err(format!("port `{spec}`: the range must go up"));
                }
                (rest, Some((lo, hi)))
            }
            None => (spec, None),
        };
        let mut port = PortSig::parse_plain(spec_).map_err(|e| e.replace(spec_, spec))?;
        if range.is_some() && !matches!(port.kind, PortKind::Control(u) if u.is_none_or(Unit::is_number)) {
            return Err(format!("port `{spec}`: only controls of a number can have a range"));
        }
        port.range = range;
        Ok(port)
    }

    /// `"name: kind [= default]"`.
    fn parse_plain(spec: &str) -> Result<PortSig, String> {
        let (name, rest) = spec.split_once(':').ok_or_else(|| format!("port `{spec}`: expected `name: kind`"))?;
        let after = rest.rfind('>').map_or(0, |i| i + 1);
        let (kind, default) = match rest[after..].split_once('=') {
            Some((k, d)) => {
                (&rest[..after + k.len()], Some(d.trim().parse::<f64>().map_err(|_| format!("port `{spec}`: default must be a number"))?))
            }
            None => (rest, None),
        };
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("port `{spec}`: bad name"));
        }
        let kind: PortKind = kind.parse().map_err(|e| format!("port `{spec}`: {e}"))?;
        // An audio input may default to silence, so a kernel may be called with it left out (ABI-12).
        match (kind, default) {
            (PortKind::Audio(_), Some(d)) if d != 0.0 => return Err(format!("port `{spec}`: an audio input's default is silence, 0")),
            (PortKind::Control(_) | PortKind::Any | PortKind::Audio(_), _) | (_, None) => {}
            _ => return Err(format!("port `{spec}`: only controls, and audio as silence, have defaults")),
        }
        // An unconnected choice or switch plays its default, which must be one of its places (CARD-12).
        if let (PortKind::Control(Some(u)), Some(d)) = (kind, default)
            && let Err(holds) = u.holds(d)
        {
            return Err(format!("port `{spec}`: its default must be {holds}, not {d}"));
        }
        Ok(PortSig { name: name.to_string(), kind, default, range: None })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KernelSig {
    /// `package:kernel`, e.g. `std:svf`.
    pub id: String,
    pub description: String,
    pub inputs: Vec<PortSig>,
    pub outputs: Vec<PortSig>,
    /// Its meters (ABI-13): numbers for the window to show, after its outputs in its node's
    /// buffers and its port table, which no wire may read.
    pub meters: Vec<PortSig>,
    /// Samples of delay the kernel adds.
    pub latency: u32,
    /// Seconds of delay it adds, its manifest's `latency_s`, counted in frames at the song's rate by
    /// `m2_kernel::latency_frames` (VOX-24). A kernel gives one of the two; the other is 0.
    pub latency_s: f64,
}

impl KernelSig {
    /// Whether it adds latency, in frames or in seconds.
    pub fn late(&self) -> bool {
        self.latency > 0 || self.latency_s > 0.0
    }

    /// How late it is, in words: "64 samples", "15 ms".
    pub fn lateness(&self) -> String {
        match self.latency_s > 0.0 {
            true => format!("{} ms", (self.latency_s * 1e4).round() / 10.0),
            false => format!("{} samples", self.latency),
        }
    }

    pub fn parse(id: &str, description: &str, inputs: &[String], outputs: &[String], latency: u32) -> Result<KernelSig, String> {
        let parse_all = |v: &[String]| v.iter().map(|s| PortSig::parse(s)).collect::<Result<Vec<_>, _>>();
        let sig = KernelSig {
            id: id.to_string(),
            description: description.to_string(),
            inputs: parse_all(inputs).map_err(|e| format!("kernel `{id}`: {e}"))?,
            outputs: parse_all(outputs).map_err(|e| format!("kernel `{id}`: {e}"))?,
            meters: vec![],
            latency,
            latency_s: 0.0,
        };
        if sig.outputs.is_empty() {
            return Err(format!("kernel `{id}`: needs at least one output"));
        }
        let mut names: Vec<&str> = sig.inputs.iter().chain(&sig.outputs).map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        if let Some(w) = names.windows(2).find(|w| w[0] == w[1]) {
            return Err(format!("kernel `{id}`: port `{}` declared twice", w[0]));
        }
        for o in &sig.outputs {
            // An events output's shape is reserved in kernel ABI v1, not built (ABI-8).
            if o.default.is_some() || matches!(o.kind, PortKind::Events(_)) {
                return Err(format!("kernel `{id}`: output `{}` must be audio or control, without a default", o.name));
            }
            let var_in = |f: fn(&PortKind) -> bool| sig.inputs.iter().any(|i| f(&i.kind));
            let bound = match o.kind {
                PortKind::Any => var_in(|k| matches!(k, PortKind::Any)),
                PortKind::Audio(None) => var_in(|k| matches!(k, PortKind::Audio(None) | PortKind::Any)),
                PortKind::Control(None) => var_in(|k| matches!(k, PortKind::Control(None))),
                _ => true,
            };
            if !bound {
                return Err(format!("kernel `{id}`: output `{}` is `{}` but no input decides what that is", o.name, o.kind));
            }
        }
        Ok(sig)
    }

    pub fn output_index(&self, name: &str) -> Option<usize> {
        self.outputs.iter().position(|p| p.name == name)
    }

    /// The kernel with `meters` (ABI-13), each `"name: control<unit>"`: a control of a unit, with
    /// no default and no range, named apart from its ports.
    pub fn with_meters(mut self, meters: &[String]) -> Result<KernelSig, String> {
        let id = &self.id;
        for spec in meters {
            let m = PortSig::parse(spec).map_err(|e| format!("kernel `{id}`: meter {e}"))?;
            if !matches!(m.kind, PortKind::Control(Some(_))) {
                return Err(format!("kernel `{id}`: meter `{spec}`: a meter is a control of a unit, such as `control<db>`"));
            }
            if m.default.is_some() || m.range.is_some() {
                return Err(format!("kernel `{id}`: meter `{spec}`: a meter has no default and no range: the kernel says it"));
            }
            if self.inputs.iter().chain(&self.outputs).chain(&self.meters).any(|p| p.name == m.name) {
                return Err(format!("kernel `{id}`: port `{}` declared twice", m.name));
            }
            self.meters.push(m);
        }
        Ok(self)
    }

    /// Meter `name`'s place among its meters (ABI-13): its node's buffer is the one at
    /// `outputs.len()` plus this.
    pub fn meter_index(&self, name: &str) -> Option<usize> {
        self.meters.iter().position(|p| p.name == name)
    }
}

/// Where the checker looks up kernels. Implemented by the runtime's package set.
pub trait Kernels {
    fn kernel(&self, id: &str) -> Option<&KernelSig>;
    /// Whether `id` is an offline kernel (kernel ABI v1, flags bit 0): one the host runs off the
    /// audio thread to make a clip's render, which song code can't call (`kernel.offline`).
    fn offline(&self, _id: &str) -> bool {
        false
    }
    /// The signature of an installed plugin, as ports: `in`, `notes`, parameters, `out`.
    fn plugin(&self, _id: &str) -> Option<&KernelSig> {
        None
    }
}

impl Kernels for std::collections::HashMap<String, KernelSig> {
    fn kernel(&self, id: &str) -> Option<&KernelSig> {
        self.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ports() {
        let p = PortSig::parse("cutoff: control<hz> = 1000").unwrap();
        assert_eq!(p.name, "cutoff");
        assert_eq!(p.kind, PortKind::Control(Some(Unit::Hz)));
        assert_eq!(p.default, Some(1000.0));
        assert!(PortSig::parse("in: audio<1> = 3").is_err());
        assert!(PortSig::parse("no kind").is_err());
        assert_eq!(p.range, None);
    }

    // ABI-12: an audio input may default to silence, `= 0`, and only to it: left unconnected, it
    // reads 0. Notes and other audio defaults stay refused.
    #[test]
    fn an_audio_input_may_default_to_silence() {
        let p = PortSig::parse("cut: audio<1> = 0").unwrap();
        assert_eq!((p.kind, p.default), (PortKind::Audio(Some(1)), Some(0.0)));
        for (bad, says) in [
            ("in: audio<1> = 0.5", "an audio input's default is silence, 0"),
            ("n: events<note> = 0", "only controls, and audio as silence, have defaults"),
        ] {
            let e = PortSig::parse(bad).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    #[test]
    fn parse_ranges() {
        let p = PortSig::parse("attack: control<seconds> = 0.01 in 0.0001..1").unwrap();
        assert_eq!((p.default, p.range), (Some(0.01), Some((0.0001, 1.0))));
        assert_eq!(PortSig::parse("t: control<db> in -80..12").unwrap().range, Some((-80.0, 12.0)));
        for bad in ["in: audio<1> in 0..1", "x: control = 1 in 2..1", "x: control = 1 in 0-1", "x: control in a..b"] {
            assert!(PortSig::parse(bad).is_err(), "{bad}");
        }
    }

    // CARD-12: a choice's port names its options, which may hold spaces, `=` and ` in `; its
    // default is a place.
    #[test]
    fn parse_choice_and_switch_ports() {
        let p = PortSig::parse("shape: control<choice<sine|fade in|a=b>> = 1").unwrap();
        assert_eq!(p.kind.to_string(), "control<choice<sine|fade in|a=b>>");
        assert_eq!((p.default, p.range), (Some(1.0), None));
        let p = PortSig::parse("on: control<switch> = 0").unwrap();
        assert_eq!(p.kind, PortKind::Control(Some(Unit::Switch)));
        for bad in ["s: control<choice<>>", "s: control<choice<a|a>>", "s: control<choice<a> = 1", "s: control<choice<a|b>> = 0 in 0..1"] {
            assert!(PortSig::parse(bad).is_err(), "{bad}");
        }
    }

    // CARD-12 (review K7): a choice's or a switch's port defaults to one of its places, which an
    // unconnected port plays.
    #[test]
    fn a_choice_ports_default_is_one_of_its_places() {
        for ok in ["s: control<choice<a|b|c>> = 2", "s: control<choice<a|b|c>> = 0", "on: control<switch> = 1", "on: control<switch>"] {
            assert!(PortSig::parse(ok).is_ok(), "{ok}");
        }
        for (bad, says) in [
            ("s: control<choice<a|b|c>> = 7.5", "its default must be a whole number from 0 to 2, not 7.5"),
            ("s: control<choice<a|b|c>> = 3", "its default must be a whole number from 0 to 2, not 3"),
            ("on: control<switch> = 0.5", "its default must be 0 or 1, not 0.5"),
        ] {
            let e = PortSig::parse(bad).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    #[test]
    fn unbound_output_rejected() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(KernelSig::parse("p:k", "", &s(&["a: control<hz>"]), &s(&["out: *"]), 0).is_err());
        assert!(KernelSig::parse("p:k", "", &s(&["a: *"]), &s(&["out: *"]), 0).is_ok());
    }

    // ABI-13: a kernel's meters are controls of a unit, after its outputs, with no default or
    // range, named apart from its ports.
    #[test]
    fn a_kernel_gives_meters_of_a_unit() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let k = KernelSig::parse("p:k", "", &s(&["in: audio<*>"]), &s(&["out: audio<*>"]), 0).unwrap();
        assert!(k.meters.is_empty(), "none unless it says");
        let k = k.with_meters(&s(&["reduction: control<db>", "open: control<switch>"])).unwrap();
        assert_eq!(
            k.meters.iter().map(|m| (m.name.as_str(), m.kind)).collect::<Vec<_>>(),
            [("reduction", PortKind::Control(Some(Unit::Db))), ("open", PortKind::Control(Some(Unit::Switch)))]
        );
        assert_eq!(k.meter_index("open"), Some(1));
        assert_eq!(k.output_index("reduction"), None, "a meter is no output a wire can name");
        for (bad, says) in [
            ("reduction: audio<1>", "a meter is a control of a unit, such as `control<db>`"),
            ("reduction: control<*>", "a meter is a control of a unit, such as `control<db>`"),
            ("reduction: *", "a meter is a control of a unit, such as `control<db>`"),
            ("reduction: control<db> = 0", "a meter has no default"),
            ("reduction: control<db> in 0..24", "a meter has no default"),
            ("out: control<db>", "port `out` declared twice"),
        ] {
            let k = KernelSig::parse("p:k", "", &s(&["in: audio<*>"]), &s(&["out: audio<*>"]), 0).unwrap();
            let e = k.with_meters(&s(&[bad])).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    // ABI-8: a kernel takes notes through an `events<note>` input; an events output's shape is
    // reserved, not built.
    #[test]
    fn a_kernel_takes_events_but_never_gives_them() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let k = KernelSig::parse("p:k", "", &s(&["n: events<note>"]), &s(&["out: audio<1>"]), 0).unwrap();
        assert_eq!(k.inputs[0].kind, PortKind::Events(crate::EventType::Note));
        let e = KernelSig::parse("p:k", "", &s(&["n: events<note>"]), &s(&["out: events<note>"]), 0).unwrap_err();
        assert!(e.contains("output `out` must be audio or control"), "{e}");
    }
}
