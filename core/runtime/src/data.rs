//! What a song's IR declares that reads data: its lanes, by module key and lane key, and its
//! plugins with saved state. The project's data is read against these ([`m2_song::Project::data`]),
//! and a renderer plays the result.

use m2_ir::{Ir, Kind, Op};
use m2_song::{Decls, Key, LaneAddr, LaneKind, StateDecl};

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

/// How a lane of `kind` reads its file. A control lane records its unit by its name.
pub fn lane_kind(kind: Kind) -> LaneKind {
    match kind {
        Kind::Events(_) => LaneKind::Notes,
        Kind::Control(unit) => LaneKind::Control { unit: unit.map(|u| u.name().to_string()) },
        Kind::Audio(_) => LaneKind::Audio,
    }
}
