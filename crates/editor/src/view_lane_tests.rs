//! 8b.5: an input a lane drives says which lane, for "Follows the Cutoff lane" and Show lane.

use crate::Gestures;
use crate::testing::fixture;

#[test]
fn an_input_a_lane_drives_names_the_lane() {
    let f = fixture("examples/first-song");
    let s = f.open();
    let view = s.view();
    let bass = view.modules.iter().find(|m| m.key == "bass").unwrap();
    let inputs: Vec<_> = bass.nodes.iter().flat_map(|n| &n.inputs).collect();
    let driven: Vec<(String, &str)> =
        inputs.iter().filter_map(|p| p.lane.as_ref().map(|l| (format!("{}/{}", l.module, l.lane), p.kind))).collect();
    assert!(driven.contains(&("bass/cutoff".to_string(), "control")), "{driven:?}");
    assert!(driven.contains(&("bass/drop".to_string(), "control")), "a mask is an automation lane too: {driven:?}");
    assert!(!driven.iter().any(|(l, _)| l == "bass/notes"), "a notes lane drives no knob: {driven:?}");
    // Sent only where there is one.
    let json = serde_json::to_value(&view).unwrap();
    let ports: Vec<&serde_json::Value> = json["modules"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["nodes"].as_array().unwrap())
        .flat_map(|n| n["inputs"].as_array().unwrap())
        .collect();
    assert!(ports.iter().any(|p| p["lane"] == serde_json::json!({"module": "bass", "lane": "cutoff"})));
    assert!(ports.iter().filter(|p| p.get("lane").is_none()).count() > 0);
}
