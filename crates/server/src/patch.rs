//! Incremental state sync. Instead of resending the whole song after every edit, each
//! WebSocket client gets a list of ops that turn its last state into the new one.
//!
//! Ops (paths are arrays of object keys and array indices):
//! - `{"op":"set","path":[…],"value":…}` — replace the value at `path`
//! - `{"op":"del","path":[…]}` — remove an object key
//! - `{"op":"ids","path":[…],"ids":[…],"add":{"<id>":…}}` — rebuild an array of `{id, …}`
//!   objects in a new order, reusing existing elements by id and inserting `add`
//!
//! Arrays whose elements all carry an `id` (tracks, clips, assets) are matched by id,
//! so moving one clip sends only that clip's fields. Anywhere the ops would be bigger
//! than the value itself, a plain `set` is sent instead.

use std::sync::Arc;

use m2_project::Song;
use serde_json::{Map, Value, json};

pub fn diff(old: &Value, new: &Value) -> Vec<Value> {
    let mut ops = Vec::new();
    walk(old, new, &mut Vec::new(), &mut ops);
    ops
}

/// Patch between two songs, skipping every track that is pointer-equal (structural
/// sharing means an edit leaves all untouched tracks as the same `Arc`). Cost is
/// proportional to what changed, not to the song.
pub fn song_diff(old: &Arc<Song>, new: &Arc<Song>, base: &[Value], ops: &mut Vec<Value>) {
    if Arc::ptr_eq(old, new) {
        return;
    }
    let head = |s: &Song| {
        json!({"format": s.format, "title": s.title, "bpm": s.bpm, "time_sig": s.time_sig, "key": s.key,
               "master": s.master, "assets": s.assets, "next_id": s.next_id})
    };
    let mut path = base.to_vec();
    walk(&head(old), &head(new), &mut path, ops);

    path.push("tracks".into());
    let old_ids: Vec<u64> = old.tracks.iter().map(|t| t.id).collect();
    let new_ids: Vec<u64> = new.tracks.iter().map(|t| t.id).collect();
    if old_ids != new_ids {
        let add: Map<String, Value> = new
            .tracks
            .iter()
            .filter(|t| !old_ids.contains(&t.id))
            .map(|t| (t.id.to_string(), serde_json::to_value(&**t).unwrap_or(Value::Null)))
            .collect();
        ops.push(json!({"op": "ids", "path": path, "ids": new_ids, "add": add}));
    }
    for (i, t) in new.tracks.iter().enumerate() {
        let Some(o) = old.tracks.iter().find(|o| o.id == t.id) else { continue };
        if Arc::ptr_eq(o, t) {
            continue;
        }
        path.push(Value::from(i));
        let (a, b) = (serde_json::to_value(&**o).unwrap_or_default(), serde_json::to_value(&**t).unwrap_or_default());
        walk(&a, &b, &mut path, ops);
        path.pop();
    }
}

fn set(path: &[Value], value: &Value) -> Value {
    json!({"op": "set", "path": path, "value": value})
}

fn size(v: &impl serde::Serialize) -> usize {
    serde_json::to_vec(v).map(|b| b.len()).unwrap_or(usize::MAX)
}

fn ids(arr: &[Value]) -> Option<Vec<u64>> {
    arr.iter().map(|x| x.get("id").and_then(Value::as_u64)).collect()
}

fn walk(old: &Value, new: &Value, path: &mut Vec<Value>, ops: &mut Vec<Value>) {
    if old == new {
        return;
    }
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                path.push(Value::from(k.as_str()));
                match a.get(k) {
                    Some(o) => walk(o, v, path, ops),
                    None => ops.push(set(path, v)),
                }
                path.pop();
            }
            for k in a.keys().filter(|k| !b.contains_key(*k)) {
                path.push(Value::from(k.as_str()));
                ops.push(json!({"op": "del", "path": path}));
                path.pop();
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            let mut sub = Vec::new();
            match (ids(a), ids(b)) {
                (Some(ia), Some(ib)) if !b.is_empty() => {
                    if ia != ib {
                        let add: Map<String, Value> =
                            b.iter().zip(&ib).filter(|(_, id)| !ia.contains(id)).map(|(v, id)| (id.to_string(), v.clone())).collect();
                        sub.push(json!({"op": "ids", "path": path, "ids": ib, "add": add}));
                    }
                    // Element diffs are addressed by index in the *new* array.
                    for (i, (v, id)) in b.iter().zip(&ib).enumerate() {
                        if let Some(j) = ia.iter().position(|x| x == id) {
                            path.push(Value::from(i));
                            walk(&a[j], v, path, &mut sub);
                            path.pop();
                        }
                    }
                }
                _ if a.len() == b.len() => {
                    for (i, (o, v)) in a.iter().zip(b).enumerate() {
                        path.push(Value::from(i));
                        walk(o, v, path, &mut sub);
                        path.pop();
                    }
                }
                _ => {
                    ops.push(set(path, new));
                    return;
                }
            }
            if size(&sub) > size(new) + 32 {
                ops.push(set(path, new));
            } else {
                ops.extend(sub);
            }
        }
        _ => ops.push(set(path, new)),
    }
}

/// Apply ops (mirror of the TypeScript client; used to test the round trip).
#[cfg(test)]
pub fn apply(root: &mut Value, ops: &[Value]) {
    for op in ops {
        let path = op["path"].as_array().cloned().unwrap_or_default();
        let kind = op["op"].as_str().unwrap_or_default();
        let (last, parent_path) = match path.split_last() {
            Some((l, p)) => (Some(l), p),
            None => (None, &path[..]),
        };
        let target = if kind == "del" {
            let mut node = &mut *root;
            for seg in parent_path {
                node = index_mut(node, seg);
            }
            if let (Value::Object(m), Some(Value::String(k))) = (node, last) {
                m.remove(k);
            }
            continue;
        } else {
            let mut node = &mut *root;
            for seg in &path {
                node = index_mut(node, seg);
            }
            node
        };
        match kind {
            "set" => *target = op["value"].clone(),
            "ids" => {
                let old = std::mem::take(target);
                let mut by_id: std::collections::HashMap<u64, Value> = old
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|v| v.get("id").and_then(Value::as_u64).map(|id| (id, v)))
                    .collect();
                let list = op["ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|id| {
                        let id = id.as_u64()?;
                        op["add"].get(id.to_string()).cloned().or_else(|| by_id.remove(&id))
                    })
                    .collect();
                *target = Value::Array(list);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
fn index_mut<'a>(node: &'a mut Value, seg: &Value) -> &'a mut Value {
    match seg {
        Value::String(k) => {
            if !node.is_object() {
                *node = Value::Object(Map::new());
            }
            node.as_object_mut().unwrap().entry(k.clone()).or_insert(Value::Null)
        }
        Value::Number(n) => &mut node.as_array_mut().expect("array")[n.as_u64().unwrap() as usize],
        _ => panic!("bad path segment"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use m2_engine::Session;
    use m2_graph::Registry;
    use std::sync::Arc;

    fn big_session() -> Session {
        let mut s = Session::new(Arc::new(Registry::with_builtins()));
        for t in 0..24 {
            let tid = s.execute("track.add", json!({"name": format!("T{t}"), "instrument": "m2/keys"})).unwrap()["track_id"].clone();
            for c in 0..8 {
                let notes: Vec<Value> =
                    (0..64).map(|i| json!({"pitch": 48 + (i * 7 + t) % 36, "start": i as f64 * 0.25, "length": 0.25})).collect();
                s.execute("clip.add", json!({"track": tid, "start_bar": c * 4 + 1, "length_bars": 4, "notes": notes})).unwrap();
            }
        }
        s
    }

    /// Structural diff must produce the same result as diffing the full JSON.
    fn check_song(before: &Arc<Song>, after: &Arc<Song>) -> usize {
        let mut ops = Vec::new();
        song_diff(before, after, &[], &mut ops);
        let mut patched = serde_json::to_value(&**before).unwrap();
        apply(&mut patched, &ops);
        assert_eq!(patched, serde_json::to_value(&**after).unwrap(), "structural patch round-trips");
        size(&ops)
    }

    fn check(before: &Value, after: &Value) -> usize {
        let ops = diff(before, after);
        let mut patched = before.clone();
        apply(&mut patched, &ops);
        assert_eq!(&patched, after, "patch round-trips");
        size(&ops)
    }

    #[test]
    fn edits_send_small_patches() {
        let mut s = big_session();
        let snap = |s: &Session| serde_json::to_value(&**s.song()).unwrap();
        let full = size(&snap(&s));
        let clip = s.song().tracks[5].clips[3].id;

        let a = snap(&s);
        s.execute("notes.velocity", json!({"clip": clip, "set": 80})).unwrap();
        let velocity = check(&a, &snap(&s));

        let a = snap(&s);
        s.execute("clip.set", json!({"clip": clip, "start_bar": 40})).unwrap();
        let moved = check(&a, &snap(&s));

        let a = snap(&s);
        s.execute("clip.set", json!({"clip": clip, "track": "T9"})).unwrap();
        let other_track = check(&a, &snap(&s));

        let a = snap(&s);
        s.execute("track.add", json!({"instrument": "m2/bass"})).unwrap();
        let added = check(&a, &snap(&s));

        let a = snap(&s);
        s.execute("track.remove", json!({"track": "T0"})).unwrap();
        let removed = check(&a, &snap(&s));

        let a = snap(&s);
        s.undo();
        s.undo();
        let undone = check(&a, &snap(&s));

        eprintln!(
            "full song {full} B | velocity {velocity} B | move clip {moved} B | clip to other track {other_track} B | add track {added} B | remove track {removed} B | undo x2 {undone} B"
        );
        assert!(moved < 200, "moving a clip: {moved} B");
        assert!(velocity < full / 50, "one clip's velocities: {velocity} B");
        assert!(other_track < full / 50 && added < 800 && removed < 300);

        // Same edits through the structural (pointer-skipping) differ.
        let a = s.song().clone();
        s.execute("clip.set", json!({"clip": clip, "start_bar": 12})).unwrap();
        let t = std::time::Instant::now();
        let moved = check_song(&a, &s.song().clone());
        // Drag one note up a semitone: notes keep their ids, so only that note changes.
        let a = s.song().clone();
        let mut notes = s.execute("clip.notes", json!({"clip": clip})).unwrap();
        notes[10]["pitch"] = json!(notes[10]["pitch"].as_u64().unwrap() + 1);
        s.execute("notes.set", json!({"clip": clip, "notes": notes})).unwrap();
        let one_note = check_song(&a, &s.song().clone());
        let a = s.song().clone();
        s.execute("track.add", json!({"instrument": "m2/pad"})).unwrap();
        s.execute("track.remove", json!({"track": "T3"})).unwrap();
        let reshaped = check_song(&a, &s.song().clone());
        eprintln!("structural: move clip {moved} B | edit one note {one_note} B | add+remove track {reshaped} B ({:?})", t.elapsed());
        assert!(moved < 200 && one_note < 200, "move {moved} B, one note {one_note} B");
    }

    #[test]
    fn handles_shape_changes() {
        let cases = [
            (json!({"a": 1, "b": [1, 2]}), json!({"a": 2, "c": null, "b": [1, 2, 3]})),
            (json!({"x": [{"id": 1, "v": 1}, {"id": 2, "v": 2}]}), json!({"x": [{"id": 2, "v": 3}, {"id": 3, "v": 0}]})),
            (json!({"x": [{"id": 1}]}), json!({"x": []})),
            (json!({"x": null}), json!({"x": {"deep": [1]}})),
        ];
        for (a, b) in cases {
            check(&a, &b);
        }
    }
}
