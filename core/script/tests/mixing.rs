//! `@m2/mixing`'s descriptors (PACK-26): each tool is an effect in its category, and what a call
//! that leaves a value out gets is its kernel's port default (PKG-4), so the kernels' own tests,
//! which play their manifest's defaults, test what songs play. The gate's key only listens, and may
//! be left out; the EQ's card has a group for each band and no picture.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m2_script::describe;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The port defaults of the pack's kernels, by `(kernel id, port)`, read from its `m2pkg.toml`.
fn port_defaults() -> BTreeMap<(String, String), f64> {
    let manifest = std::fs::read_to_string(repo().join("packs/@m2/mixing/m2pkg.toml")).unwrap();
    let (mut kernel, mut inputs, mut out) = (String::new(), false, BTreeMap::new());
    for line in manifest.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("name = \"").and_then(|s| s.strip_suffix('"')) {
            kernel = format!("@m2/mixing:{name}");
        }
        inputs |= line.starts_with("inputs = [");
        if inputs {
            for spec in line.split('"').skip(1).step_by(2) {
                let Some((port, kind)) = spec.split_once(':') else { continue };
                if let Some((_, default)) = kind.rsplit_once(" = ") {
                    out.insert((kernel.clone(), port.trim().to_string()), default.split_whitespace().next().unwrap().parse().unwrap());
                }
            }
        }
        inputs &= !line.ends_with(']');
    }
    out
}

/// A control's name as its kernel's port: `lowMid` is `low_mid`.
fn snake(name: &str) -> String {
    name.chars().flat_map(|c| if c.is_ascii_uppercase() { vec!['_', c.to_ascii_lowercase()] } else { vec![c] }).collect()
}

// PACK-26, PKG-4.
#[test]
fn the_mixing_tools_are_effects_that_start_where_their_kernels_do() {
    let catalog = describe(&[("std".to_string(), repo().join("std")), ("@m2/mixing".to_string(), repo().join("packs/@m2/mixing"))]);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let tools = [
        ("eq", "EQ", 40),
        ("gate", "Dynamics", 5),
        ("transient", "Dynamics", 2),
        ("deesser", "Dynamics", 3),
        ("multiband", "Dynamics", 17),
    ];
    let ids: Vec<&str> = catalog.components.keys().filter_map(|k| k.strip_prefix("@m2/mixing:")).collect();
    let mut want: Vec<&str> = tools.iter().map(|t| t.0).collect();
    want.sort();
    assert_eq!(ids, want, "the pack's tools");
    let ports = port_defaults();
    for (id, category, controls) in tools {
        let d = &catalog.components[&format!("@m2/mixing:{id}")];
        assert_eq!((d.editor.category.as_deref(), d.is_effect()), (Some(category), true), "{id}: an effect in {category}");
        assert_eq!(d.controls.len(), controls, "{id}'s controls");
        for c in &d.controls {
            let port = (d.id.clone(), snake(&c.name));
            let kernel = ports.get(&port).unwrap_or_else(|| panic!("{id}.{}: its kernel has no port {}", c.name, port.1));
            assert_eq!(c.default.as_ref().map(|s| s.value), Some(*kernel), "{id}.{}: its default is its kernel's port's", c.name);
        }
    }
    let gate = &catalog.components["@m2/mixing:gate"];
    let key = gate.inputs.iter().find(|i| i.name == "key").expect("the gate's key");
    // `many` only because core has no input that may be left out yet (gaps.md); one at most.
    assert!(key.sidechain && key.many, "the gate's key only listens, and may be left out: {key:?}");
    let eq = &catalog.components["@m2/mixing:eq"];
    let groups: Vec<&str> = eq.editor.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(groups, (1..=8).map(|b| format!("Band {b}")).collect::<Vec<_>>(), "the EQ's groups");
    // No picture until a view of an EQ's whole curve exists (review Z3, gaps.md): the filter view
    // draws a low-pass, which no band is at the EQ's start.
    let views: Vec<&str> = eq.editor.views.iter().map(|v| v.name.as_str()).collect();
    assert!(views.is_empty(), "the EQ draws {views:?}");
}
