//! Drum kits (KIT-1 to KIT-9; the design is `docs/designs/drums/design.md`): a kit is a Drums bus
//! and the Drum tracks that go to it, worked out from the song, never stored.
//!
//! - **The kits the packs name** (KIT-3, KIT-4): each instrument's `editor.kits`, as + Track's Drum
//!   kit lists them, and adding one (KIT-5): a Drums bus and the kit's tracks, each going to it, in
//!   one Tx, one undo step, through [`tracks::add_tracks`].

use m2_session::Session;
use serde::Serialize;

use crate::EditError;
use crate::bus::BusKind;
use crate::tracks::{self, Made, NewTrack, Play};

/// What a kit's bus is called: a kit's name is its bus's label (KIT-1).
pub const BUS: &str = "Drums";

/// A kit an instrument names (KIT-3), as + Track's Drum kit lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KitInfo {
    /// Its name: "808 kit".
    pub name: String,
    /// The instrument its tracks play, by ComponentId (`@m2/drums:drum`), and what it is called.
    pub component: String,
    pub instrument: String,
    /// Its tracks' names, in order.
    pub tracks: Vec<String>,
    /// The pack it is from, with its version; none for std's and the project's (PACK-11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<crate::packs::From>,
}

/// Every kit the instruments + Track lists name, in the list's order, each in the order its
/// instrument names them.
pub fn kits(s: &Session) -> Vec<KitInfo> {
    let catalog = s.catalog();
    tracks::instruments(s)
        .into_iter()
        .filter_map(|i| catalog.components.get(&i.component).map(|d| (i, d)))
        .flat_map(|(i, d)| {
            d.editor.kits.iter().map(move |k| KitInfo {
                name: k.name.clone(),
                component: i.component.clone(),
                instrument: i.label.clone(),
                tracks: k.tracks.iter().map(|t| t.name.clone()).collect(),
                pack: i.pack.clone(),
            })
        })
        .collect()
}

/// Add kit `kit` of instrument `component` (KIT-5): a Drums bus (a Group, RENDER-34) and the kit's
/// tracks, each playing the instrument with the kit's values from an empty lane of its own and
/// going to the bus, after track `after` (else last), the bus first. One Tx, one undo step.
/// Refused in words when the lists offer no such instrument, or it names no such kit.
pub fn add_kit(s: &mut Session, component: &str, kit: &str, after: Option<&str>) -> Result<Vec<Made>, EditError> {
    let listed = tracks::instruments(s).into_iter().find(|i| i.component == component);
    let inst = listed.ok_or_else(|| EditError::NotFound(format!("{component}: no instrument the lists offer is called that")))?;
    let d = s.catalog().components[component].clone();
    let Some(k) = d.editor.kits.iter().find(|k| k.name == kit) else {
        let names: Vec<String> = d.editor.kits.iter().map(|k| k.name.clone()).collect();
        return Err(EditError::Rejected(match names.len() {
            0 => format!("{} has no kits.", inst.label),
            1 => format!("{} has no kit called {kit}: its kit is {}.", inst.label, names[0]),
            _ => format!("{} has no kit called {kit}: its kits are {}.", inst.label, crate::listed(&names)),
        }));
    };
    let bus = NewTrack { play: Play::Bus(BusKind::Group), label: Some(BUS.into()), ..NewTrack::default() };
    let made = k.tracks.iter().map(|t| NewTrack {
        play: Play::Instrument(component.to_string()),
        label: Some(t.name.clone()),
        values: t.values.clone(),
        to: Some(0),
        ..NewTrack::default()
    });
    tracks::add_tracks(s, &std::iter::once(bus).chain(made).collect::<Vec<_>>(), after, None)
}

#[cfg(test)]
#[path = "drums_tests.rs"]
mod tests;
