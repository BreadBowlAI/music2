//! A notes clip's versions (VER-3): the acts of the one `versions` clip gesture, each on one clip.
//! One version plays, and its notes are the clip's; the others wait in the clip's `versions`
//! (VER-1). Nothing here plays anything: choosing a version writes the clip's notes.

use m2_song::lane::{Version, version_name};
use m2_song::{Note, NoteClip};
use serde::{Deserialize, Serialize};

use crate::EditError;

/// What a clip's playing notes are named when its first versions are added.
pub const ORIGINAL: &str = "original";

/// One act on a clip's versions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "act", rename_all = "lowercase")]
pub enum Act {
    /// Adds versions, which wait: what plays doesn't change. On a clip with none yet, what plays
    /// is first named `original`.
    Add {
        versions: Vec<NewVersion>,
    },
    /// Makes `name` the one that plays: its notes become the clip's, and the notes that played
    /// wait under their name.
    Play {
        name: String,
    },
    /// Sets `name`'s star, or clears it with `star` false.
    Star {
        name: String,
        #[serde(default = "yes")]
        star: bool,
    },
    Rename {
        name: String,
        to: String,
    },
    /// Removes `name`, which waits. The one that plays is refused.
    Remove {
        name: String,
    },
    /// Removes every version but the one that plays, and the field with them.
    Keep,
}

fn yes() -> bool {
    true
}

/// A version to add: its name, and its notes, times from the clip's start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewVersion {
    pub name: String,
    pub notes: Vec<Note>,
}

/// `` `a`, `b` and `c` ``.
fn listed(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
    match &quoted[..] {
        [] => String::new(),
        [one] => one.clone(),
        [most @ .., last] => format!("{} and {last}", most.join(", ")),
    }
}

/// Which of `c`'s versions `name` is.
fn find(c: &NoteClip, name: &str) -> Result<usize, EditError> {
    c.versions.iter().position(|v| v.name == name).ok_or_else(|| {
        let names: Vec<&str> = c.versions.iter().map(|v| v.name.as_str()).collect();
        EditError::Missing(match names.is_empty() {
            true => format!("The clip has no version `{name}`: it has no versions yet."),
            false => format!("The clip has no version `{name}`. It has {}.", listed(&names)),
        })
    })
}

/// `name` as a new name in `c`: one that fits (VER-1) and that no version of `c` has.
fn free(c: &NoteClip, name: &str) -> Result<(), EditError> {
    version_name(name).map_err(|why| EditError::Rejected(format!("`{name}` can't name a version: {why}.")))?;
    match c.versions.iter().any(|v| v.name == name) {
        true => Err(EditError::Rejected(format!("The clip has a version `{name}` already. Give it another name."))),
        false => Ok(()),
    }
}

/// `act` on `c`. `by` is who sends it, for the versions an `add` makes: an agent's name, none
/// for the musician. An act that changes nothing leaves `c` as it was.
pub(crate) fn apply(c: &mut NoteClip, act: &Act, by: Option<&str>) -> Result<(), EditError> {
    match act {
        Act::Add { versions } => {
            if versions.is_empty() {
                return Err(EditError::Rejected("Give a version to add: a name and its notes.".into()));
            }
            if c.versions.is_empty() {
                c.versions.push(Version { name: ORIGINAL.into(), star: false, by: None, notes: None });
            }
            for new in versions {
                free(c, &new.name)?;
                crate::clips::notes_fit(&new.notes)?;
                let by = by.map(str::trim).filter(|b| !b.is_empty()).map(str::to_string);
                c.versions.push(Version { name: new.name.clone(), star: false, by, notes: Some(new.notes.clone()) });
            }
        }
        Act::Play { name } => {
            let i = find(c, name)?;
            if let Some(notes) = c.versions[i].notes.take() {
                let played = std::mem::replace(&mut c.notes, notes);
                let was = c.versions.iter().position(|v| v.notes.is_none() && v.name != *name).expect("one version plays (VER-1)");
                c.versions[was].notes = Some(played);
            }
        }
        Act::Star { name, star } => {
            let i = find(c, name)?;
            c.versions[i].star = *star;
        }
        Act::Rename { name, to } => {
            let i = find(c, name)?;
            if to != name {
                free(c, to)?;
                c.versions[i].name = to.clone();
            }
        }
        Act::Remove { name } => {
            let i = find(c, name)?;
            if c.versions[i].notes.is_none() {
                return Err(EditError::Rejected(format!(
                    "`{name}` is the version that plays. Play another first, or keep it and remove the others."
                )));
            }
            c.versions.remove(i);
            // One version is no versions (VER-1).
            if c.versions.len() == 1 {
                c.versions.clear();
            }
        }
        Act::Keep => c.versions.clear(),
    }
    Ok(())
}

/// What changed between a lane's clips `before` and `after`, when it is a clip's versions: each
/// side in words, for the step's name. None when no clip's versions differ.
pub(crate) fn described(before: &[NoteClip], after: &[NoteClip]) -> Option<(String, String)> {
    // A clip whose span changed is named by its span, as any resize is.
    let same_span = |b: &NoteClip, c: &NoteClip| b.start == c.start && b.length == c.length;
    let by_id = |c: &&NoteClip| before.iter().find(|b| b.id.is_some() && b.id == c.id && same_span(b, c) && b.versions != c.versions);
    let (a, b) = after.iter().find_map(|c| by_id(&c).map(|b| (b, c)))?;
    let count = |c: &NoteClip| match c.versions.iter().find(|v| v.notes.is_none()) {
        Some(plays) => format!("{} versions, `{}` plays", c.versions.len(), plays.name),
        None => "no versions".to_string(),
    };
    if count(a) != count(b) {
        return Some((count(a), count(b)));
    }
    // As many, and the same one plays: the first that differs, by its name or its star.
    let (x, y) = a.versions.iter().zip(&b.versions).find(|(x, y)| x != y)?;
    let star = |v: &Version| format!("`{}`, {}", v.name, if v.star { "starred" } else { "no star" });
    Some(match x.name != y.name {
        true => (format!("version `{}`", x.name), format!("version `{}`", y.name)),
        false if x.star != y.star => (star(x), star(y)),
        false => (format!("version `{}`", x.name), format!("version `{}`, changed", y.name)),
    })
}
