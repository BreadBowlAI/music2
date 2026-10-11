//! Kernel signatures: the typed ports a package declares for each kernel.

use crate::kind::PortKind;

#[derive(Clone, Debug, PartialEq)]
pub struct PortSig {
    pub name: String,
    pub kind: PortKind,
    /// Used when the input is not connected. Controls only.
    pub default: Option<f64>,
}

impl PortSig {
    /// Parse `"name: kind [= default]"`.
    pub fn parse(spec: &str) -> Result<PortSig, String> {
        let (name, rest) = spec.split_once(':').ok_or_else(|| format!("port `{spec}`: expected `name: kind`"))?;
        let (kind, default) = match rest.split_once('=') {
            Some((k, d)) => (k, Some(d.trim().parse::<f64>().map_err(|_| format!("port `{spec}`: default must be a number"))?)),
            None => (rest, None),
        };
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("port `{spec}`: bad name"));
        }
        let kind: PortKind = kind.parse().map_err(|e| format!("port `{spec}`: {e}"))?;
        if default.is_some() && !matches!(kind, PortKind::Control(_) | PortKind::Any) {
            return Err(format!("port `{spec}`: only controls can have defaults"));
        }
        Ok(PortSig { name: name.to_string(), kind, default })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KernelSig {
    /// `package:kernel`, e.g. `std:svf`.
    pub id: String,
    pub description: String,
    pub inputs: Vec<PortSig>,
    pub outputs: Vec<PortSig>,
    /// Samples of delay the kernel adds.
    pub latency: u32,
}

impl KernelSig {
    pub fn parse(id: &str, description: &str, inputs: &[String], outputs: &[String], latency: u32) -> Result<KernelSig, String> {
        let parse_all = |v: &[String]| v.iter().map(|s| PortSig::parse(s)).collect::<Result<Vec<_>, _>>();
        let sig = KernelSig {
            id: id.to_string(),
            description: description.to_string(),
            inputs: parse_all(inputs).map_err(|e| format!("kernel `{id}`: {e}"))?,
            outputs: parse_all(outputs).map_err(|e| format!("kernel `{id}`: {e}"))?,
            latency,
        };
        if let Some(p) = sig.inputs.iter().find(|p| matches!(p.kind, PortKind::Events(_))) {
            return Err(format!("kernel `{id}`: input `{}`: kernels can't take events yet (notes reach kernels through poly)", p.name));
        }
        if sig.outputs.is_empty() {
            return Err(format!("kernel `{id}`: needs at least one output"));
        }
        let mut names: Vec<&str> = sig.inputs.iter().chain(&sig.outputs).map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        if let Some(w) = names.windows(2).find(|w| w[0] == w[1]) {
            return Err(format!("kernel `{id}`: port `{}` declared twice", w[0]));
        }
        for o in &sig.outputs {
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
}

/// Where the checker looks up kernels. Implemented by the runtime's package set.
pub trait Kernels {
    fn kernel(&self, id: &str) -> Option<&KernelSig>;
}

impl Kernels for std::collections::HashMap<String, KernelSig> {
    fn kernel(&self, id: &str) -> Option<&KernelSig> {
        self.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::Unit;

    #[test]
    fn parse_ports() {
        let p = PortSig::parse("cutoff: control<hz> = 1000").unwrap();
        assert_eq!(p.name, "cutoff");
        assert_eq!(p.kind, PortKind::Control(Some(Unit::Hz)));
        assert_eq!(p.default, Some(1000.0));
        assert!(PortSig::parse("in: audio<1> = 3").is_err());
        assert!(PortSig::parse("no kind").is_err());
    }

    #[test]
    fn unbound_output_rejected() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(KernelSig::parse("p:k", "", &s(&["a: control<hz>"]), &s(&["out: *"]), 0).is_err());
        assert!(KernelSig::parse("p:k", "", &s(&["a: *"]), &s(&["out: *"]), 0).is_ok());
        assert!(KernelSig::parse("p:k", "", &s(&["n: events<note>"]), &s(&["out: audio<1>"]), 0).is_err());
    }
}
