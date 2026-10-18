//! What a song's IR declares that reads data: its lanes, by module key and lane key, its
//! plugins with saved state, and the assets its kernels read. The project's data is read against these ([`m2_song::Project::data`]),
//! and a renderer plays the result.

use m2_ir::{Ir, Kind, Node, Op};
use m2_song::{AssetDecl, Decls, Key, LaneAddr, LaneKind, StateDecl};

/// The declarations of `ir`. A lane or plugin whose keys break the key rule declares nothing:
/// the checker reports it as `key.invalid`.
pub fn decls(ir: &Ir) -> Decls {
    let mut d = Decls::default();
    for m in &ir.modules {
        for l in &m.lanes {
            if let Ok(addr) = LaneAddr::new(&m.key, &l.key) {
                d.lanes.push((addr, lane_kind(l.kind)));
            }
        }
        if let Ok(module) = Key::new(&m.key) {
            let mut kernels = vec![];
            walk(&m.nodes, &mut |n| kernels.push(n));
            for n in kernels {
                if let Op::Kernel { assets, .. } = &n.op {
                    d.assets.extend(assets.iter().map(|a| AssetDecl { module: module.clone(), name: a.clone() }));
                }
            }
        }
        for n in &m.nodes {
            let (Some(path), Op::Plugin { plugin, .. }) = (n.op.state_path(), &n.op) else { continue };
            let keys = std::iter::once(&m.key).chain(&path).map(|k| Key::new(k)).collect::<Result<Vec<_>, _>>();
            if let Ok(mut keys) = keys {
                let module = keys.remove(0);
                d.states.push(StateDecl { module, path: keys, plugin: plugin.clone() });
            }
        }
    }
    d
}

/// Every node of `nodes`, and of the voices of each `poly` among them.
fn walk<'a>(nodes: &'a [Node], f: &mut impl FnMut(&'a Node)) {
    for n in nodes {
        f(n);
        if let Op::Poly { body, .. } = &n.op {
            walk(&body.nodes, f);
        }
    }
}

/// How a lane of `kind` reads its file. A control lane records its unit by its name.
pub fn lane_kind(kind: Kind) -> LaneKind {
    match kind {
        Kind::Events(_) => LaneKind::Notes,
        Kind::Control(unit) => LaneKind::Control { unit: unit.map(|u| u.name().to_string()) },
        Kind::Audio(_) => LaneKind::Audio,
    }
}
