//! Where a scope reads (TAP-8): a box of the graph view, by its id, as a point of a plan. The id is
//! the view's for a call (`tracks/bass.ts#3`: its file and its place among the file's calls). An
//! edit may move a call to another place, so the server follows the call itself into each new
//! revision (ID-13's continuity) and finds the id the new view gives it ([`id_of`], TAP-9).

use m2_ir::{FOp, Kernels, Kind, Op, split_ref};
use m2_runtime::TapAt;
use m2_session::Revision;

/// Where a scope on box `node` of module `module` reads in `rev`: the call's own output, at the top
/// of its track, as sound; and its channels. Refused in words when it isn't one. `kernels` names
/// each kernel's and plugin's outputs, as the revision was built with them.
pub fn point(rev: &Revision, kernels: &dyn Kernels, module: &str, node: &str) -> Result<(TapAt, u8), String> {
    let gone = || format!("There's no box {node} in {module} now, so there's nothing to show.");
    let (file, ordinal) = node.rsplit_once('#').ok_or_else(gone)?;
    let ordinal: u32 = ordinal.parse().map_err(|_| gone())?;
    let compiled = &rev.compiled;
    let site = compiled.sites.iter().find(|s| s.file == file && s.ordinal == ordinal).ok_or_else(gone)?;
    let outputs: Vec<_> = compiled.outputs.iter().filter(|o| o.site == site.id).collect();
    if outputs.is_empty() {
        return Err(format!("{} gives no sound to show.", site.callee));
    }
    let outputs: Vec<_> = outputs.into_iter().filter(|o| o.module == module).collect();
    let output = match outputs[..] {
        [] => return Err(gone()),
        [one] => one,
        _ => return Err(format!("{} is called more than once, so a scope can't tell which one to show.", site.callee)),
    };
    if !output.top {
        return Err(format!("{} plays inside an instrument's voices, one for each note: put the scope on the instrument.", site.callee));
    }
    let (at, channels, index) = resolve(rev, kernels, &output.module, &output.port).ok_or_else(gone)?;
    match (index, at) {
        (0, Some(at)) => Ok((at, channels)),
        (_, None) => Err(format!("{} gives a control or notes, not sound: a scope shows sound.", site.callee)),
        (i, Some(_)) => Err(format!("{} gives its sound on its output {}: a scope shows a box's first output.", site.callee, i + 1)),
    }
}

/// The id the graph view gives the call of `module` whose own output is flat node `node` of `rev`,
/// at the top of its track: where a scope on that call is found after an edit moved it (TAP-9).
pub fn id_of(rev: &Revision, kernels: &dyn Kernels, module: &str, node: usize) -> Option<String> {
    let compiled = &rev.compiled;
    let found = compiled
        .outputs
        .iter()
        .filter(|o| o.module == module && o.top)
        .filter(|o| matches!(resolve(rev, kernels, &o.module, &o.port), Some((Some(TapAt::Node(i)), _, 0)) if i == node));
    // The view draws the song's own calls, not those inside a package's components; of two that
    // give the same node (one wrapping the other), the outer one.
    let sites = found.filter_map(|o| compiled.sites.iter().find(|s| s.id == o.site && s.in_project));
    let outer = sites.max_by_key(|s| s.end - s.start)?;
    Some(format!("{}#{}", outer.file, outer.ordinal))
}

/// What output reference `r` of `module` is in `rev`'s flat graph: the node, when it is sound, its
/// channels, and the output's place among its node's. None when it isn't found.
fn resolve(rev: &Revision, kernels: &dyn Kernels, module: &str, r: &str) -> Option<(Option<TapAt>, u8, usize)> {
    let compiled = &rev.compiled;
    // An import is the export it names, in its own module.
    let (mut module, mut r) = (module, r);
    for _ in 0..32 {
        let (id, port) = split_ref(r);
        let m = compiled.ir.modules.iter().find(|m| m.key == module)?;
        let n = m.nodes.iter().find(|n| n.id == id)?;
        let index = match &n.op {
            Op::Import { module: target, export } => {
                r = compiled.ir.modules.iter().find(|m| &m.key == target).and_then(|m| m.exports.get(export))?;
                module = target;
                continue;
            }
            Op::Kernel { kernel, .. } => kernels.kernel(kernel).and_then(|k| k.output_index(port))?,
            Op::Plugin { plugin, .. } => kernels.plugin(plugin).and_then(|k| k.output_index(port))?,
            _ => 0,
        };
        let name = format!("{module}/{id}");
        let at = compiled.flat.nodes.iter().position(|n| n.name == name)?;
        let flat = &compiled.flat.nodes[at];
        return Some(match flat.outputs.get(index) {
            Some(&Kind::Audio(c)) if !matches!(flat.op, FOp::Const(_)) => (Some(TapAt::Node(at)), c, index),
            _ => (None, 0, index),
        });
    }
    None
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
