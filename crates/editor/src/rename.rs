//! Rename a track's label (ID-10): what the app shows for it, written as the `name` its
//! `track()` call is given. A label is never an address, and graphs are compared without labels
//! (WRITE-6), so the Tx is `Identical`: a rename that would change anything else is refused.
//!
//! The key stays: it files the track's data and is how everything names it (ID-1), so renaming a
//! track in the app never moves a file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m2_ir::Module;
use m2_script::sites::{Arg, Site};
use m2_session::Session;
use m2_song::ROOT;
use m2_song::tx::Expect;

use crate::plan::{self, path};
use crate::sites::properties;
use crate::{EditError, Edited, ready};

/// The `track()` call that made `m` (`bus()` for a bus, RENDER-34): a call of the project's code
/// named so, in the file the module was built from, whose first argument is the module's key
/// written as a string.
pub(crate) fn track_call<'a>(sites: &'a [Site], files: &BTreeMap<PathBuf, std::sync::Arc<str>>, m: &Module) -> Option<&'a Site> {
    // `src` is `file:line:column` of the module's function.
    let file = m.src.as_deref().and_then(|s| s.rsplitn(3, ':').nth(2))?;
    let text = files.get(Path::new(file))?;
    let written = |a: &Arg| {
        let (s, e) = a.span();
        let t = text.get(s as usize..e as usize).unwrap_or("");
        [format!("\"{}\"", m.key), format!("'{}'", m.key), format!("`{}`", m.key)].iter().any(|k| k == t)
    };
    let mut calls = sites.iter().filter(|x| {
        x.in_project && x.file == file && x.callee == if m.bus { "bus" } else { "track" } && x.args.first().is_some_and(written)
    });
    let call = calls.next()?;
    calls.next().is_none().then_some(call)
}

/// The edit that shows the track whose `track()` call is `site` (in `text`, keyed `key`) as
/// `label`: its `name` written, replaced, or, when the label is its key, taken out (the label
/// defaults to the key). `Some(None)`: nothing to change; None: the call isn't written so that its
/// name can be.
pub(crate) fn name_edit(text: &str, site: &Site, key: &str, label: &str) -> Option<Option<(u32, u32, String)>> {
    // What `name` is written as: a JSON string is a TypeScript string.
    let name = serde_json::to_string(label).expect("a string serialises");
    let at = site.args.first()?.span();
    Some(Some(match &site.args[..] {
        // `track("kick", fn)`: options after the key, so the call's first and last characters stay
        // and every node keeps its name (5.2).
        [_, _] if label == key => return Some(None),
        [_, _] => (at.1, at.1, format!(", {{ name: {name} }}")),
        [_, Arg::Object { start, end, .. }, _] => {
            let props = properties(text, (*start, *end))?;
            match props.iter().position(|p| p.key == "name") {
                Some(i) if !props[i].string => return None,
                // Back to the key: the property goes, and the object too when it held nothing else.
                Some(_) if label == key && props.len() == 1 => (at.1, *end, String::new()),
                Some(i) if label == key => match (i.checked_sub(1).map(|j| &props[j]), props.get(i + 1)) {
                    (Some(prev), _) => (prev.end, props[i].end, String::new()),
                    (None, Some(next)) => (props[i].start, next.start, String::new()),
                    (None, None) => unreachable!("more than one property"),
                },
                Some(i) => (props[i].value.0, props[i].value.1, name),
                None if label == key => return Some(None),
                None => match props.first() {
                    Some(first) => (first.start, first.start, format!("name: {name}, ")),
                    None => (*start, *end, format!("{{ name: {name} }}")),
                },
            }
        }
        _ => return None,
    }))
}

/// Show `module` as `label`: the `name` of its `track()` call written, replaced, or, when the
/// label is its key, taken out (the label defaults to the key). One `Identical` Tx; undo puts the
/// old name back.
pub fn rename_label(s: &mut Session, module: &str, label: &str) -> Result<Edited, EditError> {
    ready(s)?;
    let label = label.trim();
    if label.is_empty() {
        return Err(EditError::Rejected("a name can't be empty; to show the key, name it with the key".into()));
    }
    if label.chars().any(char::is_control) {
        return Err(EditError::Rejected("a name is one line of text".into()));
    }
    if module == ROOT {
        return Err(EditError::Rejected("the song itself has no name to change".into()));
    }
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == module).ok_or_else(|| EditError::NotFound(module.to_string()))?;
    let (before, key_of) = (m.label().to_string(), m.key.clone());
    if !m.track {
        return Err(EditError::Rejected(format!("{module} isn't a track, so it has no name to change")));
    }
    let in_code = || {
        EditError::Rejected(format!("{before}'s track() call isn't written with its key and options in the code; change its name there"))
    };
    let site = track_call(&rev.compiled.sites, &rev.snapshot.files, m).cloned().ok_or_else(in_code)?;
    let text = rev.snapshot.files[Path::new(&site.file)].clone();
    let unchanged = Edited { revision: rev.number, file: site.file.clone(), before: before.clone(), after: before.clone() };
    if label == before {
        return Ok(unchanged);
    }
    let edit = name_edit(&text, &site, &key_of, label).ok_or_else(in_code)?;
    let Some(edit) = edit else { return Ok(unchanged) };
    let file = path(&site.file)?;
    let op = plan::text_edit(&file, &text, &[edit]);
    let label_now = label.to_string();
    let tx = plan::planned(s, (before.clone(), label_now.clone()), vec![op], Expect::Identical);
    let said = |why: &str| EditError::Rejected(format!("renaming {before} would change more than its name ({why})"));
    let done = plan::run_as(s, tx, said, |_, checked| {
        // The planner's assertion: the module shows the new name.
        let shown = checked.candidate().compiled.ir.modules.iter().find(|m| m.key == module).map(|m| m.label().to_string());
        match shown.as_deref() == Some(label_now.as_str()) {
            true => Ok(()),
            false => Err(EditError::Rejected(format!("the edit wouldn't show {module} as {label_now}"))),
        }
    })?;
    Ok(Edited { revision: done.number, file: site.file.clone(), before, after: label.to_string() })
}

#[cfg(test)]
#[path = "rename_tests.rs"]
mod tests;
