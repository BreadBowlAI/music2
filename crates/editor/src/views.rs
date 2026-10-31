//! A card's groups and views (CARD-25, CARD-26), resolved for its call: what the window draws over
//! the card's rows, and what an agent reads of it.
//!
//! - **Groups** put a card's values and views in its author's order (CARD-17). A row is an input of
//!   the call that is one of its declared controls (a knob, a cable, a lane, a value the code works
//!   out), a setting, or a view; what no group names goes in a last group, More. An input that is a
//!   connection (notes, audio) is in no group: the card draws it above them.
//! - **Views** are pictures over values with homes (HOME-1). An envelope's and a filter's parts each
//!   name their control's slot and its knob, when the card can move it; a keys view what std's
//!   Sampler plays; a scope its own call, which the window asks the server for (TAP-8).

use m2_ir::Module;
use m2_script::Descriptor;
use m2_script::component::ViewOf;
use serde::Serialize;

use crate::lens;
pub use crate::sampler::KeyRegion;
use crate::view::{InputPort, LaneRef, ViewNode};

/// A group of a card (CARD-25): its name, and its rows in the order the group names them.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CardGroup {
    pub name: String,
    pub rows: Vec<Row>,
}

/// A row of a card: an input of its call (by `InputPort.id`), a setting (by its id) or a view (by
/// its name).
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Row {
    Input(String),
    Setting(String),
    View(String),
}

/// A view of a card (CARD-26), resolved for its call.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CardView {
    /// Its name in the descriptor (`amp`).
    pub name: String,
    #[serde(flatten)]
    pub of: Drawn,
}

/// What a view draws.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Drawn {
    /// An envelope's shape; its corners move the attack, the decay and sustain, and the release.
    Envelope { attack: Part, decay: Part, sustain: Part, release: Part },
    /// A low-pass curve from cutoff and q; its peak moves them.
    Filter { cutoff: Part, q: Part },
    /// Which recording plays on which keys and velocities, from what std's Sampler plays.
    Keys {
        /// The `regions` or `keymap` setting it draws, by its id.
        setting: String,
        regions: Vec<KeyRegion>,
        /// The notes lane that plays the call, which a clicked key plays on (CARD-28).
        #[serde(skip_serializing_if = "Option::is_none")]
        lane: Option<LaneRef>,
        /// Why no keys are drawn, when they can't be.
        #[serde(skip_serializing_if = "Option::is_none")]
        why: Option<String>,
    },
    /// The call's sound as it plays (TAP-8).
    Scope {},
}

/// A value a view draws and moves: its control's slot, its knob when the card can move it, and
/// why not when it can't.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Part {
    pub slot: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knob: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// The declared control at argument `path` of a call of `desc` whose options are argument
/// `options` (`Frame.options`): a positional one by its place, one in the options object by its
/// name, a dictionary entry's by its whole path (`channels.hats.db`). None for an input port, or
/// what the descriptor doesn't declare.
pub(crate) fn slot_at(desc: &Descriptor, options: Option<usize>, path: &[String]) -> Option<String> {
    let (first, rest) = path.split_first()?;
    let i: usize = first.parse().ok()?;
    let slot = match options == Some(i) {
        true => rest.join("."),
        false => {
            let name = *lens::params(desc).get(i)?;
            std::iter::once(name).chain(rest.iter().map(String::as_str)).collect::<Vec<_>>().join(".")
        }
    };
    desc.control(&slot).map(|_| slot)
}

/// Card `node`'s groups (CARD-25), of a call of `desc`: empty when it declares none.
pub(crate) fn groups(desc: &Descriptor, node: &ViewNode) -> Vec<CardGroup> {
    if desc.editor.groups.is_empty() {
        return vec![];
    }
    let rows_of = |name: &str| -> Vec<Row> {
        let mut rows: Vec<Row> = node.inputs.iter().filter(|p| p.slot.as_deref() == Some(name)).map(|p| Row::Input(p.id.clone())).collect();
        rows.extend(node.settings.iter().filter(|x| x.slot == name).map(|x| Row::Setting(x.id.clone())));
        if desc.editor.views.iter().any(|v| v.name == name) {
            rows.push(Row::View(name.to_string()));
        }
        rows
    };
    let mut out: Vec<CardGroup> = desc
        .editor
        .groups
        .iter()
        .map(|g| CardGroup { name: g.name.clone(), rows: g.items.iter().flat_map(|i| rows_of(i)).collect() })
        .collect();
    // More: the values and views no group names, in the card's order.
    let more: Vec<Row> = node
        .inputs
        .iter()
        .filter(|p| p.slot.is_some())
        .map(|p| Row::Input(p.id.clone()))
        .chain(node.settings.iter().map(|x| Row::Setting(x.id.clone())))
        .chain(desc.editor.views.iter().map(|v| Row::View(v.name.clone())))
        .filter(|r| !out.iter().any(|g| g.rows.contains(r)))
        .collect();
    if !more.is_empty() {
        out.push(CardGroup { name: "More".into(), rows: more });
    }
    out
}

/// Card `node`'s views (CARD-26), of call `site` of `desc`, made in module `m`; `notes` is the
/// notes lane that plays the call, when one does.
pub(crate) fn views(m: &Module, site: u32, desc: &Descriptor, node: &ViewNode, notes: Option<&LaneRef>) -> Vec<CardView> {
    let part = |slot: &str| -> Part {
        let knob = node.knobs.iter().find(|k| k.slot.as_deref() == Some(slot)).map(|k| k.id.clone());
        let why = match knob {
            Some(_) => None,
            None => Some(why_not(m, node.inputs.iter().find(|p| p.slot.as_deref() == Some(slot)))),
        };
        Part { slot: slot.to_string(), knob, why }
    };
    desc.editor
        .views
        .iter()
        .map(|v| {
            let of = match &v.of {
                ViewOf::Envelope { attack, decay, sustain, release } => {
                    Drawn::Envelope { attack: part(attack), decay: part(decay), sustain: part(sustain), release: part(release) }
                }
                ViewOf::Filter { cutoff, q } => Drawn::Filter { cutoff: part(cutoff), q: part(q) },
                ViewOf::Keys { regions } => {
                    let setting = node.settings.iter().find(|x| &x.slot == regions).map(|x| x.id.clone()).unwrap_or_default();
                    let (regions, why) = match crate::sampler::key_regions(m, site) {
                        Some(r) => (r, None),
                        None => (vec![], Some("its recordings don't play through std's Sampler, so their keys can't be drawn".to_string())),
                    };
                    Drawn::Keys { setting, regions, lane: notes.cloned(), why }
                }
                ViewOf::Scope => Drawn::Scope {},
            };
            CardView { name: v.name.clone(), of }
        })
        .collect()
}

/// Why the card can't move the value at input `p`, in words.
fn why_not(m: &Module, p: Option<&InputPort>) -> String {
    match p {
        Some(InputPort { lane: Some(l), .. }) => {
            let label = m.lanes.iter().find(|x| x.key == l.lane).map_or(l.lane.as_str(), |x| x.label());
            format!("it follows the {label} lane")
        }
        Some(InputPort { inlet: Some(_), .. }) => "a cable drives it".to_string(),
        Some(InputPort { readonly: Some(_), .. }) => "it is worked out by its code".to_string(),
        _ => "it can be changed only in its code".to_string(),
    }
}

#[cfg(test)]
#[path = "views_tests.rs"]
mod tests;
