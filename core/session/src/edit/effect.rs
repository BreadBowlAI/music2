//! Effects: put one of the packages' effects after a call's audio, or take it out.
//!
//! An effect is a component whose descriptor has an editor category (PKG-2, PKG-7); the
//! packages' catalog lists them (PKG-11). Adding wraps the call where it is written:
//! `pluck(notes)` becomes `delay(pluck(notes))`, importing `delay` if the file doesn't. That works
//! wherever the call is (an argument, a return, an arrow's body) and needs no new names. The
//! descriptor's `insert` is written after the audio, `lowpass(pluck(notes), 1000)`, so its
//! settings are knobs from the start (and every other control is a knob too, inherited).
//! Removing is the inverse, `delay(x, …)` back to `x`: the effect's own options go with it.
//!
//! A call is an effect by the ComponentId its frame records (PKG-9), never by how its callee is
//! spelled: `echo(x)` after `import { delay as echo }` is `std:delay`.
//!
//! Both are verified the same way, against the graph. Take the effect call's own nodes (the
//! numbers written in its arguments and the controls it leaves out included: they are made in
//! its frame) out of the graph that has it, and make whatever read its output read its input
//! instead: that must be exactly the graph without it.

use std::collections::BTreeSet;

use m2_ir::split_ref;
use m2_script::component::Spelled;
use m2_script::sites::{import_insertion, imported_names};
use serde::Serialize;

use super::cable::{canonical, excise, refs};
use super::*;

/// An effect the packages offer, as the effect browser lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EffectInfo {
    /// The ComponentId (`std:delay`): what adding it names.
    pub component: String,
    /// What it is called (`Delay`).
    pub label: String,
    /// Where it is imported from and as what: `delay` from `std/fx`.
    pub name: String,
    pub module: String,
    pub description: String,
    /// A free string the browser groups by (PKG-7).
    pub category: String,
    /// What adding it writes after the audio, from its descriptor's insert: `, 1000`, or
    /// `, { time: 0.25, feedback: 0.3, mix: 0.3 }`; empty when it inserts nothing.
    pub insert: String,
}

/// The component frames of `m` whose chain holds `n`, outermost first: the sites of the
/// component calls it was made inside (ID-15). `key()` frames aren't calls of a component.
pub(crate) fn calls_of<'a>(m: &'a Module, n: &Node) -> impl Iterator<Item = u32> + 'a {
    m.chain(n.frame).into_iter().filter(|f| f.key.is_none()).map(|f| f.site)
}

/// The nodes call `site` made, and the module they are in.
fn made_by(compiled: &Compiled, site: u32) -> Option<(String, BTreeSet<String>)> {
    let output = compiled.outputs.iter().find(|o| o.site == site)?;
    let mut ids = BTreeSet::new();
    let m = compiled.ir.modules.iter().find(|m| m.key == output.module)?;
    walk(&m.nodes, &mut |n| {
        if calls_of(m, n).any(|s| s == site) || n.origin.as_ref().is_some_and(|o| o.site == site) {
            ids.insert(n.id.clone());
        }
    });
    Some((output.module.clone(), ids))
}

/// Whether `with` is `without` plus effect call `site`: removing the call's nodes and
/// reading what they read in place of their output gives exactly `without`.
fn wraps(with: &Compiled, without: &Ir, site: u32) -> bool {
    let mut outputs = with.outputs.iter().filter(|o| o.site == site);
    let (Some(output), None) = (outputs.next(), outputs.next()) else { return false };
    let Some((module, ids)) = made_by(with, site) else { return false };
    let ids: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    if !ids.contains(split_ref(&output.port).0) {
        return false;
    }
    // What the effect reads from outside itself: one of these is its input. Its nodes may be
    // in a voice (an effect inside `poly`), so every node list is searched, voices included.
    let inputs = std::cell::RefCell::new(BTreeSet::new());
    let m = with.ir.modules.iter().find(|m| m.key == module).expect("the output's module");
    walk(&m.nodes, &mut |n| {
        if !ids.contains(n.id.as_str()) {
            return;
        }
        // The node's own references: a voice's nodes are walked in their turn.
        let mut node = vec![n.clone()];
        if let Op::Poly { body, .. } = &mut node[0].op {
            body.nodes.clear();
        }
        refs(&mut node, &mut BTreeMap::new(), &|_| {}, &|r: &mut String| {
            if !ids.contains(split_ref(r).0) {
                inputs.borrow_mut().insert(r.clone());
            }
        });
    });
    let inputs = inputs.into_inner();
    let without = canonical(without);
    inputs.iter().any(|input| {
        excise(&with.ir, &module, &ids, &|r| r == output.port, input).is_some_and(|ir| canonical(&ir) == without)
    })
}

/// How an effect's insert value is written (D11): a number in a spelling of scale 1 (all of
/// std's) as itself, so the call reads as the descriptor wrote it; in another spelling with its
/// constructor (`ms(45)`), never a base value with float noise. The constructors the text needs
/// imported come back with it.
fn insert_text(v: &Spelled, units: &[m2_script::UnitCtor]) -> (String, Option<String>) {
    let number = |x: f64| if x == 0.0 { "0".to_string() } else { x.to_string() };
    match v.spelled.as_deref().and_then(|s| units.iter().find(|u| u.name == s)) {
        Some(u) if u.scale != 1.0 => (format!("{}({})", u.name, number(v.written)), Some(u.name.clone())),
        Some(_) => (number(v.written), None),
        None => (number(v.value), None),
    }
}

/// The arguments adding effect `d` writes after the audio (D11): positional when its insert's
/// slots are its first positional ones in order, else an options object in the insert's order;
/// and the unit constructors that text needs imported.
fn insert_args(d: &Descriptor, units: &[m2_script::UnitCtor]) -> (String, Vec<String>) {
    let values: Vec<(&str, String, Option<String>)> = d
        .editor
        .insert
        .iter()
        .map(|i| {
            let (v, ctor) = insert_text(&i.value, units);
            (i.slot.as_str(), v, ctor)
        })
        .collect();
    let positional = values.len() <= d.positional.len() && values.iter().zip(&d.positional).all(|(v, p)| v.0 == p);
    let text = match () {
        _ if values.is_empty() => String::new(),
        _ if positional => values.iter().map(|v| format!(", {}", v.1)).collect(),
        _ => format!(", {{ {} }}", values.iter().map(|v| format!("{}: {}", v.0, v.1)).collect::<Vec<_>>().join(", ")),
    };
    (text, values.into_iter().filter_map(|v| v.2).collect())
}

impl Session {
    /// The effects the packages offer: every catalogued component with an editor category, and
    /// where to import it from; in the order their packages declare them (by file, then line).
    pub fn effects(&self) -> Vec<EffectInfo> {
        let mut found: Vec<(&Descriptor, EffectInfo)> = self
            .catalog
            .effects()
            .filter_map(|d| {
                let export = self.catalog.exports.get(&d.id)?.iter().find(|e| !e.name.contains('.'))?;
                let info = EffectInfo {
                    component: d.id.clone(),
                    label: d.label.clone(),
                    name: export.name.clone(),
                    module: export.module.clone(),
                    description: d.editor.description.clone(),
                    category: d.editor.category.clone().unwrap_or_default(),
                    insert: insert_args(d, &self.accepted.compiled.units).0,
                };
                Some((&**d, info))
            })
            .collect();
        // `at` is `file:line:col`.
        let line = |d: &Descriptor| d.at.rsplit(':').nth(1).and_then(|l| l.parse::<u32>().ok()).unwrap_or(0);
        found.sort_by(|(a, _), (b, _)| (&a.file, line(a)).cmp(&(&b.file, line(b))));
        found.into_iter().map(|(_, e)| e).collect()
    }

    /// The effect that call `site` of the accepted revision is, if it is a call in the project of
    /// a component with an editor category: by the ComponentId its frames record (PKG-9).
    pub(crate) fn effect_at(&self, site: &Site) -> Option<Arc<Descriptor>> {
        if !site.in_project {
            return None;
        }
        let compiled = &self.accepted.compiled;
        let mut ids = compiled.ir.modules.iter().flat_map(|m| &m.frames).filter(|f| f.site == site.id).map(|f| f.component.as_deref());
        // Every evaluation of the call is one component (a call takes only its own site, D17).
        let id = ids.next()??;
        if ids.any(|x| x != Some(id)) {
            return None;
        }
        compiled.components.get(id).filter(|d| d.is_effect()).cloned()
    }

    /// Put effect `component` (a ComponentId, `std:delay`) after call `call` (`file#ordinal` at
    /// revision `at`).
    pub fn add_effect(&mut self, at: u64, call: &str, component: &str) -> Result<Edited, EditError> {
        self.ready()?;
        let effect = self.effects().into_iter().find(|e| e.component == component).ok_or_else(|| EditError::NotFound(component.to_string()))?;
        let desc = self.catalog.components[component].clone();
        let site = self.current_site(at, call)?.clone();
        if !site.in_project {
            return Err(EditError::NotFound(call.to_string()));
        }
        let file = Path::new(&site.file);
        let text = self.accepted.snapshot.files.get(file).ok_or_else(|| EditError::NotFound(site.file.clone()))?.clone();
        let written = &text[site.start as usize..site.end as usize];
        let (args, ctors) = insert_args(&desc, &self.accepted.compiled.units);
        let wrapped = format!("{}({written}{args})", effect.name);
        let mut edits = vec![(site.start, site.end, wrapped.clone())];
        let mut imports = vec![];
        if let Some(i) = import_insertion(&text, &effect.module, &effect.name) {
            imports.push(i);
        }
        for ctor in &ctors {
            if !imported_names(&text, "@m2/core").contains(ctor) && !imports.iter().any(|(_, line)| line.contains(ctor.as_str())) {
                imports.extend(import_insertion(&text, "@m2/core", ctor));
            }
        }
        // Joined where they go: two at one place become one edit.
        imports.sort_by_key(|(at, _)| *at);
        let mut joined: Vec<(u32, String)> = vec![];
        for (at, line) in imports {
            match joined.last_mut() {
                Some((a, l)) if *a == at => l.push_str(&line),
                _ => joined.push((at, line)),
            }
        }
        let shift: u32 = joined.iter().filter(|(at, _)| *at <= site.start).map(|(_, line)| line.len() as u32).sum();
        edits.extend(joined.into_iter().map(|(at, line)| (at, at, line)));
        let candidate = self.candidate_edits(file, &edits)?;
        // The effect's call, where it ends up in the candidate.
        let new = candidate.compiled.sites.iter().find(|s| s.file == site.file && s.start == site.start + shift && s.callee == effect.name);
        let refused = || EditError::Rejected(format!("adding {} here would change more than putting it after this sound", effect.label));
        let new = new.ok_or_else(refused)?.id;
        if !wraps(&candidate.compiled, &self.accepted.compiled.ir, new) {
            return Err(refused());
        }
        self.write_shown(candidate, written, &wrapped)
    }

    /// Take out effect call `call`, leaving what it was applied to.
    pub fn remove_effect(&mut self, at: u64, call: &str) -> Result<Edited, EditError> {
        self.ready()?;
        let site = self.current_site(at, call)?.clone();
        let effect = self.effect_at(&site).ok_or_else(|| EditError::Rejected(format!("`{}` is not an effect that can be removed here", site.callee)))?;
        let refused = || EditError::Rejected(format!("removing {} would change more than taking it out", effect.label));
        let input = site.args.first().ok_or_else(refused)?.span();
        let text = &self.accepted.snapshot.files[Path::new(&site.file)];
        let (written, inner) = (text[site.start as usize..site.end as usize].to_string(), text[input.0 as usize..input.1 as usize].to_string());
        let candidate = self.candidate(Path::new(&site.file), site.start, site.end, &inner)?;
        if !wraps(&self.accepted.compiled, &candidate.compiled.ir, site.id) {
            return Err(refused());
        }
        self.write_shown(candidate, &written, &inner)
    }

    /// Write a structural edit, describing it as `before` → `after`.
    fn write_shown(&mut self, candidate: Candidate, before: &str, after: &str) -> Result<Edited, EditError> {
        let file = candidate.file.display().to_string();
        let undo = self.exact(&candidate.file);
        let revision = self.commit(candidate, Some(undo))?;
        Ok(Edited { revision, file, before: before.to_string(), after: after.to_string() })
    }
}

#[cfg(test)]
#[path = "effect_tests.rs"]
mod tests;
