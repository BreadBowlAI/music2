//! Pads and keys play a kit by sound (KIT-25).

use super::Pads;

// KIT-25, KIT-28 (review C7): a key goes to the kit's track whose sound's own note it is before
// one whose sound only answers to it: in a kit with an Open hat, A#2 (46) plays the Open hat, and
// the pedal hat (44) still the Hats; in a kit with Hats alone, 46 is the Hats'.
#[test]
fn a_key_goes_to_the_track_whose_own_note_it_is_first() {
    let row = |t: &str, s: &str| (t.to_string(), "hits".to_string(), Some(s.to_string()));
    let kit = |rows| Pads { bus: "drums".into(), label: "Drums".into(), rows };
    let both = kit(vec![row("kick", "kick"), row("hats", "hats"), row("open-hat", "open hat")]);
    let at = |p: &Pads, k: u8| p.answers(k).map(|(m, _)| m);
    assert_eq!(at(&both, 46).as_deref(), Some("open-hat"));
    assert_eq!(at(&both, 44).as_deref(), Some("hats"));
    assert_eq!(at(&both, 42).as_deref(), Some("hats"));
    let hats = kit(vec![row("kick", "kick"), row("hats", "hats")]);
    assert_eq!(at(&hats, 46).as_deref(), Some("hats"));
}
