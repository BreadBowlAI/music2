//! Effects: put one of the packages' effects after a call's audio, or take it out.
//!
//! Adding wraps the call where it is written: `pluck(notes)` becomes
//! `delay(pluck(notes))`, importing `delay` if the file doesn't. That works wherever the
//! call is (an argument, a return, an arrow's body) and needs no new names. Removing
//! is the inverse, `delay(x, …)` back to `x`: the effect's own options go with it.
//!
//! Both are verified the same way, against the graph. Take the effect call's own nodes
//! out of the graph that has it, and make whatever read its output read its input
//! instead: that must be exactly the graph without it.

use std::collections::BTreeSet;

use m2_ir::split_ref;
use m2_runtime::Effect;
use m2_script::sites::{import_insertion, imported_names};

use super::cable::{canonical, excise, refs};
use super::*;

/// The nodes call `site` made, and the module they are in.
fn made_by(compiled: &Compiled, site: u32) -> Option<(String, BTreeSet<String>)> {
    let output = compiled.outputs.iter().find(|o| o.site == site)?;
    let mut ids = BTreeSet::new();
    let m = compiled.ir.modules.iter().find(|m| m.name == output.module)?;
    walk(&m.nodes, &mut |n| {
        if n.within.contains(&site) || n.origin.as_ref().is_some_and(|o| o.site == site) {
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
    // What the effect reads from outside itself: one of these is its input.
    let mut inputs = BTreeSet::new();
    let m = with.ir.modules.iter().find(|m| m.name == module).expect("the output's module");
    let mut nodes = m.nodes.clone();
    let mut exports = BTreeMap::new();
    let inside = |ns: &mut Vec<Node>| ns.retain(|n| ids.contains(n.id.as_str()));
    let collect = std::cell::RefCell::new(&mut inputs);
    refs(&mut nodes, &mut exports, &inside, &|r: &mut String| {
        if !ids.contains(split_ref(r).0) {
            collect.borrow_mut().insert(r.clone());
        }
    });
    let without = canonical(without);
    inputs.iter().any(|input| {
        excise(&with.ir, &module, &ids, &|r| r == output.port, input).is_some_and(|ir| canonical(&ir) == without)
    })
}

impl Session {
    /// The effects the loaded packages offer.
    pub fn effects(&self) -> Vec<Effect> {
        self.packages.effects().cloned().collect()
    }

    /// The effect that call `site` of the accepted revision is, if it is a call of one
    /// by the name its file imports it under.
    pub(crate) fn effect_at(&self, site: &Site) -> Option<Effect> {
        let text = self.accepted.snapshot.files.get(Path::new(&site.file))?;
        self.packages
            .effects()
            .find(|e| e.name == site.callee && site.in_project && imported_names(text, &e.module).contains(&e.name))
            .cloned()
    }

    /// Put `effect` after call `call` (`file#ordinal` at revision `at`).
    pub fn add_effect(&mut self, at: u64, call: &str, module: &str, name: &str) -> Result<Edited, EditError> {
        self.ready()?;
        let effect = self
            .packages
            .effects()
            .find(|e| e.module == module && e.name == name)
            .cloned()
            .ok_or_else(|| EditError::NotFound(format!("{module}.{name}")))?;
        let site = self.current_site(at, call)?.clone();
        if !site.in_project {
            return Err(EditError::NotFound(call.to_string()));
        }
        let file = Path::new(&site.file);
        let text = self.accepted.snapshot.files.get(file).ok_or_else(|| EditError::NotFound(site.file.clone()))?.clone();
        let written = &text[site.start as usize..site.end as usize];
        let wrapped = format!("{}({written})", effect.name);
        let mut edits = vec![(site.start, site.end, wrapped.clone())];
        let import = import_insertion(&text, &effect.module, &effect.name);
        if let Some((at, line)) = &import {
            edits.push((*at, *at, line.clone()));
        }
        let candidate = self.candidate_edits(file, &edits)?;
        // The effect's call, where it ends up in the candidate.
        let shift = import.as_ref().filter(|(at, _)| *at <= site.start).map_or(0, |(_, line)| line.len() as u32);
        let new = candidate.compiled.sites.iter().find(|s| s.file == site.file && s.start == site.start + shift && s.callee == effect.name);
        let refused = || EditError::Rejected(format!("adding {} here would change more than putting it after this sound", effect.name));
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
        let refused = || EditError::Rejected(format!("removing {} would change more than taking it out", effect.name));
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
        let base = self.accepted.number;
        let revision = self.commit(candidate, Some(Undo::Exact { base }))?;
        Ok(Edited { revision, file, before: before.to_string(), after: after.to_string() })
    }
}

#[cfg(test)]
#[path = "effect_tests.rs"]
mod tests;
