//! The command registry. Every edit — from the UI, the CLI, or anything scripting the
//! HTTP API — is one of these. Each command has a JSON Schema, so the registry
//! describes itself (`music2-cli commands --json`, `GET /api/commands`).

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use m2_graph::{PluginKind, Registry};
use m2_project::*;
use m2_project::{AutoPoint, AutoTarget, AutomationLane};
use m2_theory::{Key, Scale, note_name, parse_note};
use serde_json::{Value, json};

pub struct CmdCtx<'a> {
    pub song: &'a mut Song,
    pub registry: &'a Registry,
}

pub struct CommandDef {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// Read-only commands never touch history.
    pub mutates: bool,
    pub schema: fn() -> Value,
    pub run: fn(&mut CmdCtx, &Value) -> Result<Value>,
}

pub fn find(id: &str) -> Option<&'static CommandDef> {
    COMMANDS.iter().find(|c| c.id == id || c.id.replace('.', "_") == id)
}

// ---------------------------------------------------------------- param helpers

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

fn track_prop() -> Value {
    json!({"description": "Track id (number) or track name (string)", "type": ["integer", "string"]})
}

fn note_props() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "pitch": {"description": "MIDI number (60 = C4) or name like \"C4\", \"F#3\"", "type": ["integer", "string"]},
                "start": {"type": "number", "description": "Beats from clip start (0-based)"},
                "length": {"type": "number", "description": "Length in beats"},
                "velocity": {"type": "integer", "description": "1–127, default 100"}
            },
            "required": ["pitch", "start", "length"]
        }
    })
}

fn f(p: &Value, k: &str) -> Option<f64> {
    p.get(k).and_then(|v| v.as_f64())
}

fn req_f(p: &Value, k: &str) -> Result<f64> {
    f(p, k).ok_or_else(|| anyhow!("`{k}` (number) is required"))
}

fn s<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k).and_then(|v| v.as_str())
}

fn b(p: &Value, k: &str) -> Option<bool> {
    p.get(k).and_then(|v| v.as_bool())
}

fn id_of(p: &Value, k: &str) -> Result<Id> {
    p.get(k).and_then(|v| v.as_u64()).ok_or_else(|| anyhow!("`{k}` (id) is required"))
}

pub fn track_ref(v: &Value) -> Option<TrackRef> {
    match v {
        Value::Number(n) => n.as_u64().map(TrackRef::Id),
        Value::String(s) => match s.parse::<u64>() {
            Ok(id) => Some(TrackRef::Id(id)),
            Err(_) => Some(TrackRef::Name(s.clone())),
        },
        _ => None,
    }
}

fn track_idx(song: &Song, p: &Value) -> Result<usize> {
    let v = p.get("track").ok_or_else(|| anyhow!("`track` is required"))?;
    let r = track_ref(v).ok_or_else(|| anyhow!("`track` must be an id or name"))?;
    song.track_index(&r).ok_or_else(|| anyhow!("no track {v}"))
}

fn clip_loc(song: &Song, p: &Value) -> Result<(usize, usize)> {
    let id = id_of(p, "clip")?;
    song.clip_location(id).ok_or_else(|| anyhow!("no clip {id}"))
}

fn parse_pitch(v: &Value) -> Result<u8> {
    match v {
        Value::Number(n) => n.as_u64().filter(|n| *n <= 127).map(|n| n as u8).ok_or_else(|| anyhow!("pitch {n} out of range")),
        Value::String(s) => parse_note(s).ok_or_else(|| anyhow!("bad note name `{s}`")),
        _ => bail!("pitch must be a number or note name"),
    }
}

fn parse_notes(song: &Song, v: Option<&Value>, clip_len: Tick) -> Result<Vec<Note>> {
    let tempo = song.tempo();
    let Some(arr) = v.and_then(|v| v.as_array()) else { return Ok(vec![]) };
    arr.iter()
        .map(|n| {
            let pitch = parse_pitch(n.get("pitch").ok_or_else(|| anyhow!("note needs `pitch`"))?)?;
            let start = tempo.beats_to_ticks(req_f(n, "start")?).max(0);
            let length = tempo.beats_to_ticks(req_f(n, "length")?).max(1);
            if start >= clip_len {
                bail!("note at beat {} starts after the clip ends", tempo.ticks_to_beats(start));
            }
            let velocity = n.get("velocity").and_then(|v| v.as_u64()).unwrap_or(100).clamp(1, 127) as u8;
            let id = n.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            Ok(Note { id, pitch, start, length: length.min(clip_len - start), velocity })
        })
        .collect()
}

/// Resolve a track reference that must be a bus.
fn bus_id(song: &Song, v: &Value) -> Result<Id> {
    let r = track_ref(v).ok_or_else(|| anyhow!("expected a bus id or name"))?;
    let i = song.track_index(&r).ok_or_else(|| anyhow!("no track {v}"))?;
    let t = &song.tracks[i];
    if t.kind != TrackKind::Bus {
        bail!("`{}` is not a bus — add one with track.add {{\"instrument\": \"bus\"}}", t.name);
    }
    Ok(t.id)
}

/// Device id from a selector: "instrument" or an effect index.
fn device_id(song: &Song, track: usize, sel: Option<&Value>) -> Result<Id> {
    let t = &song.tracks[track];
    match sel {
        Some(Value::String(x)) if x == "instrument" => {
            t.instrument.as_ref().map(|d| d.id).ok_or_else(|| anyhow!("`{}` has no instrument", t.name))
        }
        Some(v) => {
            let i = v.as_u64().ok_or_else(|| anyhow!("`device` must be \"instrument\" or an effect index"))? as usize;
            t.effects.get(i).map(|d| d.id).ok_or_else(|| anyhow!("`{}` has no effect {i}", t.name))
        }
        None => bail!("`device` is required"),
    }
}

fn auto_target(song: &Song, track: usize, p: &Value) -> Result<AutoTarget> {
    Ok(match s(p, "param").unwrap_or("volume") {
        "volume" => AutoTarget::Volume,
        "pan" => AutoTarget::Pan,
        "macro" => {
            let device = device_id(song, track, p.get("device"))?;
            let macro_id = s(p, "macro").ok_or_else(|| anyhow!("`macro` is required for param \"macro\""))?.to_string();
            AutoTarget::Macro { device, macro_id }
        }
        other => bail!("unknown automation param `{other}` (volume, pan, macro)"),
    })
}

/// Copy-on-write access to a clip's notes.
fn notes_at(song: &mut Song, t: usize, i: usize) -> Option<&mut Vec<Note>> {
    Arc::make_mut(&mut song.track_at_mut(t).clips[i]).notes_mut()
}

fn sort_notes(notes: &mut [Note]) {
    notes.sort_by_key(|n| (n.start, n.pitch));
}

fn check_plugin(reg: &Registry, id: &str, want: &[PluginKind]) -> Result<()> {
    let def = reg.get(id).ok_or_else(|| anyhow!("unknown plugin `{id}` — see plugins.list"))?;
    if !want.contains(&def.kind) {
        bail!("`{id}` is a {:?} plugin, expected {want:?}", def.kind);
    }
    Ok(())
}

fn macros_from(p: &Value) -> std::collections::BTreeMap<String, f32> {
    p.get("macros")
        .and_then(|m| m.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|x| (k.clone(), x.clamp(0.0, 1.0) as f32))).collect())
        .unwrap_or_default()
}

fn device_chain<'a>(song: &'a mut Song, p: &Value) -> Result<&'a mut Vec<Device>> {
    if p.get("track").and_then(|v| v.as_str()) == Some("master") {
        return Ok(&mut song.master.effects);
    }
    let i = track_idx(song, p)?;
    Ok(&mut song.track_at_mut(i).effects)
}

// ---------------------------------------------------------------- describe

pub fn describe(song: &Song, registry: &Registry, notes_for: Option<Id>) -> Value {
    let tempo = song.tempo();
    let bars = |t: Tick| (tempo.ticks_to_bars(t) * 1000.0).round() / 1000.0;
    let tracks: Vec<Value> = song
        .tracks
        .iter()
        .map(|t| {
            let clips: Vec<Value> = t
                .clips
                .iter()
                .map(|c| {
                    let mut v = json!({
                        "id": c.id, "name": c.name,
                        "start_bar": bars(c.start) + 1.0, "length_bars": bars(c.length),
                    });
                    match &c.content {
                        ClipContent::Midi { notes } => {
                            v["notes"] = json!(notes.len());
                            if notes_for == Some(c.id) {
                                v["note_list"] = notes_json(song, notes);
                            }
                        }
                        ClipContent::Audio { asset, .. } => {
                            v["audio"] = json!(song.asset(*asset).map(|a| a.name.clone()).unwrap_or_default());
                        }
                    }
                    v
                })
                .collect();
            let track_name = |id: Id| song.track(id).map(|t| t.name.clone()).unwrap_or_default();
            let dev = |d: &Device| {
                let name = registry.get(&d.plugin).map(|p| p.name.clone()).unwrap_or_else(|| "(missing)".into());
                let mut v = json!({"plugin": d.plugin, "name": name, "macros": d.macros});
                if let Some(src) = d.sidechain {
                    v["sidechain"] = json!(track_name(src));
                }
                if d.bypass {
                    v["bypass"] = json!(true);
                }
                v
            };
            let lane_name = |l: &m2_project::AutomationLane| match &l.target {
                AutoTarget::Volume => "volume".to_string(),
                AutoTarget::Pan => "pan".to_string(),
                AutoTarget::Macro { device, macro_id } => {
                    let which = if t.instrument.as_ref().is_some_and(|d| d.id == *device) {
                        "instrument".to_string()
                    } else {
                        t.effects.iter().position(|d| d.id == *device).map(|i| format!("effect {i}")).unwrap_or_default()
                    };
                    format!("{which} macro {macro_id}")
                }
            };
            json!({
                "id": t.id, "name": t.name, "kind": t.kind,
                "output": t.output.map(track_name).unwrap_or_else(|| "master".into()),
                "sends": t.sends.iter().map(|x| json!({"to": track_name(x.to), "gain_db": x.gain_db, "pre_fader": x.pre_fader})).collect::<Vec<_>>(),
                "instrument": t.instrument.as_ref().map(dev),
                "effects": t.effects.iter().map(dev).collect::<Vec<_>>(),
                "automation": t.automation.iter().map(|l| json!({
                    "param": lane_name(l),
                    "points": l.points.iter().map(|p| json!({"bar": bars(p.tick) + 1.0, "value": p.value})).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "volume_db": t.volume_db, "pan": t.pan, "mute": t.mute, "solo": t.solo,
                "clips": clips,
            })
        })
        .collect();
    json!({
        "title": song.title,
        "bpm": song.bpm,
        "key": song.key.name(),
        "time_sig": format!("{}/{}", song.time_sig.num, song.time_sig.den),
        "length_bars": bars(song.end_tick()),
        "tracks": tracks,
        "master": {"volume_db": song.master.volume_db, "effects": song.master.effects.iter().map(|d| &d.plugin).collect::<Vec<_>>()},
        "assets": song.assets.iter().map(|a| json!({"id": a.id, "name": a.name, "prompt": a.prompt})).collect::<Vec<_>>(),
    })
}

fn notes_json(song: &Song, notes: &[Note]) -> Value {
    let tempo = song.tempo();
    let r = |x: f64| (x * 1000.0).round() / 1000.0;
    json!(
        notes
            .iter()
            .map(|n| json!({
                "id": n.id, "pitch": n.pitch, "name": note_name(n.pitch),
                "start": r(tempo.ticks_to_beats(n.start)), "length": r(tempo.ticks_to_beats(n.length)),
                "velocity": n.velocity
            }))
            .collect::<Vec<_>>()
    )
}

pub fn plugin_list(registry: &Registry, kind: Option<&str>) -> Value {
    json!(
        registry
            .list()
            .filter(|p| kind.is_none_or(|k| format!("{:?}", p.kind).eq_ignore_ascii_case(k)))
            .map(|p| json!({
                "id": p.id, "name": p.name, "kind": p.kind, "description": p.description,
                "macros": p.macros.iter().map(|m| json!({"id": m.id, "name": m.name, "default": m.default})).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>()
    )
}

// ---------------------------------------------------------------- the registry

pub static COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "song.describe",
        label: "Describe song",
        description: "Summary of the song: tempo, key, tracks, devices, clips (ids, bars). Pass `clip` to include that clip's notes.",
        mutates: false,
        schema: || obj(json!({"clip": {"type": "integer", "description": "Clip id whose notes to include"}}), &[]),
        run: |c, p| Ok(describe(c.song, c.registry, p.get("clip").and_then(|v| v.as_u64()))),
    },
    CommandDef {
        id: "plugins.list",
        label: "List plugins",
        description: "Installed instruments, kits and effects with their macro knobs.",
        mutates: false,
        schema: || obj(json!({"kind": {"type": "string", "enum": ["instrument", "kit", "effect"]}}), &[]),
        run: |c, p| Ok(plugin_list(c.registry, s(p, "kind"))),
    },
    CommandDef {
        id: "song.set",
        label: "Song settings",
        description: "Set title, tempo (bpm), key (e.g. \"A minor\", \"F# major\") and/or time signature (\"4/4\").",
        mutates: true,
        schema: || {
            obj(
                json!({"title": {"type": "string"}, "bpm": {"type": "number"}, "key": {"type": "string"}, "time_sig": {"type": "string"}}),
                &[],
            )
        },
        run: |c, p| {
            if let Some(t) = s(p, "title") {
                c.song.title = t.to_string();
            }
            if let Some(bpm) = f(p, "bpm") {
                c.song.bpm = bpm.clamp(40.0, 300.0);
            }
            if let Some(k) = s(p, "key") {
                c.song.key = Key::parse(k).ok_or_else(|| anyhow!("can't read key `{k}`"))?;
            }
            if let Some(ts) = s(p, "time_sig") {
                let (n, d) = ts.split_once('/').ok_or_else(|| anyhow!("time_sig looks like \"4/4\""))?;
                c.song.time_sig = TimeSig { num: n.trim().parse()?, den: d.trim().parse()? };
            }
            Ok(json!({"bpm": c.song.bpm, "key": c.song.key.name()}))
        },
    },
    CommandDef {
        id: "track.add",
        label: "Add track",
        description: "Add a track. `instrument` is an instrument/kit plugin id (default m2/keys), \"audio\" for an audio track, or \"bus\" for a bus (a track other tracks route into via `output` or sends). Returns track_id.",
        mutates: true,
        schema: || {
            obj(
                json!({"name": {"type": "string"}, "instrument": {"type": "string"}, "macros": {"type": "object", "description": "Macro values 0–1"}}),
                &[],
            )
        },
        run: |c, p| {
            let plugin = s(p, "instrument").unwrap_or("m2/keys");
            let (kind, instrument) = if plugin == "audio" {
                (TrackKind::Audio, None)
            } else if plugin == "bus" {
                (TrackKind::Bus, None)
            } else {
                check_plugin(c.registry, plugin, &[PluginKind::Instrument, PluginKind::Kit])?;
                let mut d = Device::new(plugin);
                d.macros = macros_from(p);
                (TrackKind::Instrument, Some(d))
            };
            let default_name = match &instrument {
                Some(d) => c.registry.get(&d.plugin).map(|p| p.name.clone()).unwrap_or_else(|| "Track".into()),
                None if kind == TrackKind::Bus => "Bus".into(),
                None => "Audio".into(),
            };
            let id = c.song.alloc_id();
            let color = c.song.next_color();
            c.song.tracks.push(Arc::new(Track {
                id,
                name: s(p, "name").map(str::to_string).unwrap_or(default_name),
                color,
                kind,
                instrument,
                effects: vec![],
                clips: vec![],
                volume_db: 0.0,
                pan: 0.0,
                mute: false,
                solo: false,
                output: None,
                sends: vec![],
                automation: vec![],
            }));
            Ok(json!({"track_id": id}))
        },
    },
    CommandDef {
        id: "track.remove",
        label: "Delete track",
        description: "Delete a track and its clips.",
        mutates: true,
        schema: || obj(json!({"track": track_prop()}), &["track"]),
        run: |c, p| {
            let i = track_idx(c.song, p)?;
            let t = c.song.tracks.remove(i);
            // Anything routed into or keyed from it falls back to master / no sidechain.
            for k in 0..c.song.tracks.len() {
                let refs = {
                    let o = &c.song.tracks[k];
                    o.output == Some(t.id)
                        || o.sends.iter().any(|x| x.to == t.id)
                        || o.instrument.iter().chain(o.effects.iter()).any(|d| d.sidechain == Some(t.id))
                };
                if refs {
                    let o = c.song.track_at_mut(k);
                    if o.output == Some(t.id) {
                        o.output = None;
                    }
                    o.sends.retain(|x| x.to != t.id);
                    for d in o.instrument.iter_mut().chain(o.effects.iter_mut()) {
                        if d.sidechain == Some(t.id) {
                            d.sidechain = None;
                        }
                    }
                }
            }
            Ok(json!({"removed": t.name}))
        },
    },
    CommandDef {
        id: "track.set",
        label: "Track settings",
        description: "Rename, set volume (dB, 0 = unity, -6 = half as loud), pan (-1 left … 1 right), mute, solo, move to `index`, or set `output` (a bus track, or \"master\").",
        mutates: true,
        schema: || {
            obj(
                json!({"track": track_prop(), "name": {"type": "string"}, "volume_db": {"type": "number"}, "pan": {"type": "number"},
                       "mute": {"type": "boolean"}, "solo": {"type": "boolean"}, "index": {"type": "integer"},
                       "output": {"description": "Bus track id/name, or \"master\"", "type": ["integer", "string"]}}),
                &["track"],
            )
        },
        run: |c, p| {
            let i = track_idx(c.song, p)?;
            let output = match p.get("output") {
                None => None,
                Some(Value::String(m)) if m == "master" => Some(None),
                Some(Value::Null) => Some(None),
                Some(v) => Some(Some(bus_id(c.song, v)?)),
            };
            let t = c.song.track_at_mut(i);
            if let Some(o) = output {
                t.output = o;
            }
            if let Some(n) = s(p, "name") {
                t.name = n.to_string();
            }
            if let Some(v) = f(p, "volume_db") {
                t.volume_db = v.clamp(-60.0, 12.0) as f32;
            }
            if let Some(v) = f(p, "pan") {
                t.pan = v.clamp(-1.0, 1.0) as f32;
            }
            if let Some(v) = b(p, "mute") {
                t.mute = v;
            }
            if let Some(v) = b(p, "solo") {
                t.solo = v;
            }
            if let Some(to) = p.get("index").and_then(|v| v.as_u64()) {
                let t = c.song.tracks.remove(i);
                let to = (to as usize).min(c.song.tracks.len());
                c.song.tracks.insert(to, t);
            }
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "track.setInstrument",
        label: "Change instrument",
        description: "Replace a track's instrument with another instrument or kit plugin.",
        mutates: true,
        schema: || obj(json!({"track": track_prop(), "plugin": {"type": "string"}, "macros": {"type": "object"}}), &["track", "plugin"]),
        run: |c, p| {
            let plugin = s(p, "plugin").ok_or_else(|| anyhow!("`plugin` is required"))?;
            check_plugin(c.registry, plugin, &[PluginKind::Instrument, PluginKind::Kit])?;
            let i = track_idx(c.song, p)?;
            let mut d = Device::new(plugin);
            d.macros = macros_from(p);
            let t = c.song.track_at_mut(i);
            t.kind = TrackKind::Instrument;
            t.instrument = Some(d);
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "device.setMacros",
        label: "Adjust sound",
        description: "Set macro knobs (0–1) on a device. `track` may be \"master\". `device` is \"instrument\" or an effect index (0-based).",
        mutates: true,
        schema: || {
            obj(
                json!({"track": track_prop(), "device": {"type": ["string", "integer"]}, "macros": {"type": "object"}}),
                &["track", "device", "macros"],
            )
        },
        run: |c, p| {
            let m = macros_from(p);
            let dev = p.get("device").ok_or_else(|| anyhow!("`device` is required"))?;
            let reg = c.registry;
            let d: &mut Device = if dev.as_str() == Some("instrument") {
                let i = track_idx(c.song, p)?;
                c.song.track_at_mut(i).instrument.as_mut().ok_or_else(|| anyhow!("track has no instrument"))?
            } else {
                let idx = dev.as_u64().ok_or_else(|| anyhow!("`device` must be \"instrument\" or an effect index"))? as usize;
                let chain = device_chain(c.song, p)?;
                let len = chain.len();
                chain.get_mut(idx).ok_or_else(|| anyhow!("no effect {idx} (chain has {len})"))?
            };
            if let Some(def) = reg.get(&d.plugin) {
                for k in m.keys() {
                    if !def.macros.iter().any(|x| &x.id == k) {
                        let ids: Vec<&str> = def.macros.iter().map(|x| x.id.as_str()).collect();
                        bail!("`{}` has no macro `{k}` (macros: {})", d.plugin, ids.join(", "));
                    }
                }
            }
            d.macros.extend(m);
            Ok(json!({"macros": d.macros}))
        },
    },
    CommandDef {
        id: "effect.add",
        label: "Add effect",
        description: "Append an effect plugin to a track's chain (or \"master\"). Returns its index.",
        mutates: true,
        schema: || {
            obj(
                json!({"track": track_prop(), "plugin": {"type": "string"}, "macros": {"type": "object"}, "index": {"type": "integer"}}),
                &["track", "plugin"],
            )
        },
        run: |c, p| {
            let plugin = s(p, "plugin").ok_or_else(|| anyhow!("`plugin` is required"))?;
            check_plugin(c.registry, plugin, &[PluginKind::Effect])?;
            let mut d = Device::new(plugin);
            d.macros = macros_from(p);
            let chain = device_chain(c.song, p)?;
            let at = p.get("index").and_then(|v| v.as_u64()).map(|i| (i as usize).min(chain.len())).unwrap_or(chain.len());
            chain.insert(at, d);
            Ok(json!({"index": at}))
        },
    },
    CommandDef {
        id: "effect.remove",
        label: "Remove effect",
        description: "Remove the effect at `index` from a track's chain (or \"master\").",
        mutates: true,
        schema: || obj(json!({"track": track_prop(), "index": {"type": "integer"}}), &["track", "index"]),
        run: |c, p| {
            let idx = p.get("index").and_then(|v| v.as_u64()).ok_or_else(|| anyhow!("`index` is required"))? as usize;
            let chain = device_chain(c.song, p)?;
            if idx >= chain.len() {
                bail!("no effect {idx}");
            }
            let d = chain.remove(idx);
            Ok(json!({"removed": d.plugin}))
        },
    },
    CommandDef {
        id: "effect.bypass",
        label: "Bypass effect",
        description: "Turn an effect off/on without removing it.",
        mutates: true,
        schema: || {
            obj(json!({"track": track_prop(), "index": {"type": "integer"}, "bypass": {"type": "boolean"}}), &["track", "index", "bypass"])
        },
        run: |c, p| {
            let idx = p.get("index").and_then(|v| v.as_u64()).ok_or_else(|| anyhow!("`index` is required"))? as usize;
            let by = b(p, "bypass").unwrap_or(true);
            let chain = device_chain(c.song, p)?;
            chain.get_mut(idx).ok_or_else(|| anyhow!("no effect {idx}"))?.bypass = by;
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "clip.add",
        label: "Add clip",
        description: "Create a MIDI clip on a track. Bars are 1-based (bar 1 is the start of the song). Optionally include notes. Returns clip_id.",
        mutates: true,
        schema: || {
            obj(
                json!({"track": track_prop(), "start_bar": {"type": "number"}, "length_bars": {"type": "number"},
                       "name": {"type": "string"}, "notes": note_props()}),
                &["track", "start_bar", "length_bars"],
            )
        },
        run: |c, p| {
            let i = track_idx(c.song, p)?;
            if c.song.tracks[i].kind == TrackKind::Bus {
                bail!("`{}` is a bus — buses have no clips", c.song.tracks[i].name);
            }
            let tempo = c.song.tempo();
            let start = tempo.bar_to_tick(req_f(p, "start_bar")?.max(1.0));
            let length = tempo.beats_to_ticks(req_f(p, "length_bars")? * c.song.time_sig.num as f64).max(PPQ / 4);
            let mut notes = parse_notes(c.song, p.get("notes"), length)?;
            notes.iter_mut().for_each(|n| n.id = 0);
            c.song.assign_note_ids(&mut notes);
            sort_notes(&mut notes);
            let id = c.song.alloc_id();
            let t = c.song.track_at_mut(i);
            let name = s(p, "name").map(str::to_string).unwrap_or_else(|| t.name.clone());
            t.clips.push(Arc::new(Clip { id, name, start, length, content: ClipContent::Midi { notes } }));
            t.clips.sort_by_key(|c| c.start);
            Ok(json!({"clip_id": id}))
        },
    },
    CommandDef {
        id: "clip.remove",
        label: "Delete clip",
        description: "Delete a clip.",
        mutates: true,
        schema: || obj(json!({"clip": {"type": "integer"}}), &["clip"]),
        run: |c, p| {
            let (t, i) = clip_loc(c.song, p)?;
            c.song.track_at_mut(t).clips.remove(i);
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "clip.set",
        label: "Edit clip",
        description: "Move (start_bar, 1-based), resize (length_bars), rename, or move to another `track`.",
        mutates: true,
        schema: || {
            obj(
                json!({"clip": {"type": "integer"}, "start_bar": {"type": "number"}, "length_bars": {"type": "number"},
                       "name": {"type": "string"}, "track": track_prop()}),
                &["clip"],
            )
        },
        run: |c, p| {
            let (mut t, i) = clip_loc(c.song, p)?;
            let tempo = c.song.tempo();
            let bpb = c.song.time_sig.num as f64;
            if p.get("track").is_some() {
                let to = track_idx(c.song, p)?;
                if c.song.tracks[to].kind == TrackKind::Bus {
                    bail!("`{}` is a bus — buses have no clips", c.song.tracks[to].name);
                }
                if to != t {
                    let clip = c.song.track_at_mut(t).clips.remove(i);
                    c.song.track_at_mut(to).clips.push(clip);
                    t = to;
                }
            }
            let id = id_of(p, "clip")?;
            let clip = Arc::make_mut(c.song.track_at_mut(t).clips.iter_mut().find(|c| c.id == id).unwrap());
            if let Some(bar) = f(p, "start_bar") {
                clip.start = tempo.bar_to_tick(bar.max(1.0));
            }
            if let Some(len) = f(p, "length_bars") {
                clip.length = tempo.beats_to_ticks(len * bpb).max(PPQ / 4);
            }
            if let Some(n) = s(p, "name") {
                clip.name = n.to_string();
            }
            c.song.track_at_mut(t).clips.sort_by_key(|c| c.start);
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "clip.duplicate",
        label: "Duplicate clip",
        description: "Copy a clip `count` times (default 1), each placed right after the previous copy, or at `to_bar` if given. Returns new clip ids.",
        mutates: true,
        schema: || obj(json!({"clip": {"type": "integer"}, "count": {"type": "integer"}, "to_bar": {"type": "number"}}), &["clip"]),
        run: |c, p| {
            let (t, i) = clip_loc(c.song, p)?;
            let src: Clip = (*c.song.tracks[t].clips[i]).clone();
            let count = p.get("count").and_then(|v| v.as_u64()).unwrap_or(1).clamp(1, 64);
            let mut at = match f(p, "to_bar") {
                Some(bar) => c.song.tempo().bar_to_tick(bar.max(1.0)),
                None => src.end(),
            };
            let mut ids = Vec::new();
            for _ in 0..count {
                let id = c.song.alloc_id();
                let mut copy = Clip { id, start: at, ..src.clone() };
                if let Some(notes) = copy.notes_mut() {
                    // Copies are new notes: fresh ids.
                    notes.iter_mut().for_each(|n| n.id = 0);
                    c.song.assign_note_ids(notes);
                }
                c.song.track_at_mut(t).clips.push(Arc::new(copy));
                ids.push(id);
                at += src.length;
            }
            c.song.track_at_mut(t).clips.sort_by_key(|c| c.start);
            Ok(json!({"clip_ids": ids}))
        },
    },
    CommandDef {
        id: "clip.notes",
        label: "Read notes",
        description: "List a clip's notes (pitch, name, start/length in beats, velocity).",
        mutates: false,
        schema: || obj(json!({"clip": {"type": "integer"}}), &["clip"]),
        run: |c, p| {
            let (t, i) = clip_loc(c.song, p)?;
            Ok(notes_json(c.song, c.song.tracks[t].clips[i].notes()))
        },
    },
    CommandDef {
        id: "notes.add",
        label: "Add notes",
        description: "Add notes to a MIDI clip. start/length are in beats from the clip start. Returns the new note ids.",
        mutates: true,
        schema: || obj(json!({"clip": {"type": "integer"}, "notes": note_props()}), &["clip", "notes"]),
        run: |c, p| {
            let (t, i) = clip_loc(c.song, p)?;
            let len = c.song.tracks[t].clips[i].length;
            let mut new = parse_notes(c.song, p.get("notes"), len)?;
            new.iter_mut().for_each(|n| n.id = 0);
            c.song.assign_note_ids(&mut new);
            let ids: Vec<Id> = new.iter().map(|n| n.id).collect();
            let notes = notes_at(c.song, t, i).ok_or_else(|| anyhow!("not a MIDI clip"))?;
            notes.extend(new);
            sort_notes(notes);
            Ok(json!({"ids": ids, "note_count": notes.len()}))
        },
    },
    CommandDef {
        id: "notes.set",
        label: "Replace notes",
        description: "Replace all notes in a MIDI clip. Notes may carry the `id` of an existing note in this clip to keep its identity. Returns ids in input order.",
        mutates: true,
        schema: || obj(json!({"clip": {"type": "integer"}, "notes": note_props()}), &["clip", "notes"]),
        run: |c, p| {
            let (t, i) = clip_loc(c.song, p)?;
            let len = c.song.tracks[t].clips[i].length;
            let mut new = parse_notes(c.song, p.get("notes"), len)?;
            // Only ids already in this clip are kept; anything else is a new note.
            let existing: std::collections::HashSet<Id> = c.song.tracks[t].clips[i].notes().iter().map(|n| n.id).collect();
            new.iter_mut().filter(|n| !existing.contains(&n.id)).for_each(|n| n.id = 0);
            c.song.assign_note_ids(&mut new);
            let ids: Vec<Id> = new.iter().map(|n| n.id).collect();
            sort_notes(&mut new);
            let notes = notes_at(c.song, t, i).ok_or_else(|| anyhow!("not a MIDI clip"))?;
            *notes = new;
            Ok(json!({"ids": ids, "note_count": notes.len()}))
        },
    },
    CommandDef {
        id: "notes.remove",
        label: "Delete notes",
        description: "Remove notes from a clip matching all given filters: `ids`, `indices`, `pitch`, beat range [`from_beat`, `to_beat`). No filters removes all.",
        mutates: true,
        schema: || {
            obj(
                json!({"clip": {"type": "integer"}, "ids": {"type": "array", "items": {"type": "integer"}}, "indices": {"type": "array", "items": {"type": "integer"}},
                       "pitch": {"type": ["integer", "string"]}, "from_beat": {"type": "number"}, "to_beat": {"type": "number"}}),
                &["clip"],
            )
        },
        run: |c, p| {
            let (t, i) = clip_loc(c.song, p)?;
            let tempo = c.song.tempo();
            let pitch = p.get("pitch").map(parse_pitch).transpose()?;
            let from = f(p, "from_beat").map(|x| tempo.beats_to_ticks(x));
            let to = f(p, "to_beat").map(|x| tempo.beats_to_ticks(x));
            let indices: Option<Vec<usize>> =
                p.get("indices").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().map(|x| x as usize)).collect());
            let ids: Option<Vec<Id>> = p.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64()).collect());
            let notes = notes_at(c.song, t, i).ok_or_else(|| anyhow!("not a MIDI clip"))?;
            let before = notes.len();
            let mut k = 0;
            notes.retain(|n| {
                let idx = k;
                k += 1;
                let hit = indices.as_ref().is_none_or(|ix| ix.contains(&idx))
                    && ids.as_ref().is_none_or(|ids| ids.contains(&n.id))
                    && pitch.is_none_or(|pp| n.pitch == pp)
                    && from.is_none_or(|f| n.start >= f)
                    && to.is_none_or(|t| n.start < t);
                !hit
            });
            Ok(json!({"removed": before - notes.len()}))
        },
    },
    CommandDef {
        id: "notes.transpose",
        label: "Transpose",
        description: "Shift notes by semitones, or by scale steps if `in_key` is true (stays in key).",
        mutates: true,
        schema: || {
            obj(
                json!({"clip": {"type": "integer"}, "semitones": {"type": "integer"}, "in_key": {"type": "boolean"}}),
                &["clip", "semitones"],
            )
        },
        run: |c, p| {
            let (t, i) = clip_loc(c.song, p)?;
            let amount = p.get("semitones").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let key = c.song.key;
            let in_key = b(p, "in_key").unwrap_or(false);
            let notes = notes_at(c.song, t, i).ok_or_else(|| anyhow!("not a MIDI clip"))?;
            for n in notes.iter_mut() {
                let target = if in_key { scale_step(&key, n.pitch, amount) } else { n.pitch as i32 + amount };
                n.pitch = target.clamp(0, 127) as u8;
            }
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "notes.quantize",
        label: "Quantize",
        description: "Snap note starts to a grid in beats (0.25 = 16ths). `strength` 0–1 (default 1).",
        mutates: true,
        schema: || obj(json!({"clip": {"type": "integer"}, "grid": {"type": "number"}, "strength": {"type": "number"}}), &["clip", "grid"]),
        run: |c, p| {
            let (t, i) = clip_loc(c.song, p)?;
            let tempo = c.song.tempo();
            let grid = tempo.beats_to_ticks(req_f(p, "grid")?).max(1);
            let strength = f(p, "strength").unwrap_or(1.0).clamp(0.0, 1.0);
            let notes = notes_at(c.song, t, i).ok_or_else(|| anyhow!("not a MIDI clip"))?;
            for n in notes.iter_mut() {
                let q = m2_time_snap(n.start, grid);
                n.start += ((q - n.start) as f64 * strength).round() as Tick;
            }
            sort_notes(notes);
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "notes.velocity",
        label: "Note dynamics",
        description: "Change velocities: `set` to a value, `scale` by a factor, and/or `humanize` (0–1 random variation).",
        mutates: true,
        schema: || {
            obj(
                json!({"clip": {"type": "integer"}, "set": {"type": "integer"}, "scale": {"type": "number"}, "humanize": {"type": "number"}}),
                &["clip"],
            )
        },
        run: |c, p| {
            let (t, i) = clip_loc(c.song, p)?;
            let set = p.get("set").and_then(|v| v.as_u64());
            let scale = f(p, "scale");
            let human = f(p, "humanize").unwrap_or(0.0).clamp(0.0, 1.0);
            let notes = notes_at(c.song, t, i).ok_or_else(|| anyhow!("not a MIDI clip"))?;
            let mut rng = m2_theory::Rng::new(notes.len() as u64 + 17);
            for n in notes.iter_mut() {
                let mut v = set.map(|s| s as f64).unwrap_or(n.velocity as f64);
                if let Some(sc) = scale {
                    v *= sc;
                }
                v += (rng.f() * 2.0 - 1.0) * human * 25.0;
                n.velocity = v.round().clamp(1.0, 127.0) as u8;
            }
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "audio.place",
        label: "Place audio",
        description: "Put an imported/generated audio asset on the timeline at `start_bar`. Uses `track` if given (must be audio-capable), else creates an audio track. Returns clip_id and track_id.",
        mutates: true,
        schema: || {
            obj(
                json!({"asset": {"type": "integer"}, "start_bar": {"type": "number"}, "track": track_prop(), "gain_db": {"type": "number"}}),
                &["asset", "start_bar"],
            )
        },
        run: |c, p| {
            let asset_id = id_of(p, "asset")?;
            let asset = c.song.asset(asset_id).cloned().ok_or_else(|| anyhow!("no asset {asset_id}"))?;
            let tempo = c.song.tempo();
            let ti = match p.get("track") {
                Some(_) => track_idx(c.song, p)?,
                None => {
                    let id = c.song.alloc_id();
                    let color = c.song.next_color();
                    c.song.tracks.push(Arc::new(Track {
                        id,
                        name: asset.name.clone(),
                        color,
                        kind: TrackKind::Audio,
                        instrument: None,
                        effects: vec![],
                        clips: vec![],
                        volume_db: 0.0,
                        pan: 0.0,
                        mute: false,
                        solo: false,
                        output: None,
                        sends: vec![],
                        automation: vec![],
                    }));
                    c.song.tracks.len() - 1
                }
            };
            let start = tempo.bar_to_tick(req_f(p, "start_bar")?.max(1.0));
            let length = tempo.seconds_to_tick(asset.duration as f64).max(PPQ / 4);
            let id = c.song.alloc_id();
            let gain_db = f(p, "gain_db").unwrap_or(0.0) as f32;
            let t = c.song.track_at_mut(ti);
            t.clips.push(Arc::new(Clip {
                id,
                name: asset.name.clone(),
                start,
                length,
                content: ClipContent::Audio { asset: asset_id, offset: 0.0, gain_db },
            }));
            t.clips.sort_by_key(|c| c.start);
            Ok(json!({"clip_id": id, "track_id": t.id}))
        },
    },
    CommandDef {
        id: "sampler.load",
        label: "Make instrument from sound",
        description: "Turn an audio asset into a playable instrument (m2/sampler) on a new or existing track. Returns track_id.",
        mutates: true,
        schema: || obj(json!({"asset": {"type": "integer"}, "track": track_prop(), "name": {"type": "string"}}), &["asset"]),
        run: |c, p| {
            let asset_id = id_of(p, "asset")?;
            let asset = c.song.asset(asset_id).cloned().ok_or_else(|| anyhow!("no asset {asset_id}"))?;
            let mut d = Device::new("m2/sampler");
            d.assets.insert("sample".into(), asset_id);
            let ti = match p.get("track") {
                Some(_) => track_idx(c.song, p)?,
                None => {
                    let id = c.song.alloc_id();
                    let color = c.song.next_color();
                    c.song.tracks.push(Arc::new(Track {
                        id,
                        name: s(p, "name").map(str::to_string).unwrap_or(asset.name.clone()),
                        color,
                        kind: TrackKind::Instrument,
                        instrument: None,
                        effects: vec![],
                        clips: vec![],
                        volume_db: 0.0,
                        pan: 0.0,
                        mute: false,
                        solo: false,
                        output: None,
                        sends: vec![],
                        automation: vec![],
                    }));
                    c.song.tracks.len() - 1
                }
            };
            let t = c.song.track_at_mut(ti);
            t.kind = TrackKind::Instrument;
            t.instrument = Some(d);
            Ok(json!({"track_id": t.id}))
        },
    },
    CommandDef {
        id: "asset.setRoot",
        label: "Set sample pitch",
        description: "Set which MIDI pitch an audio asset plays at unshifted when used as an instrument.",
        mutates: true,
        schema: || obj(json!({"asset": {"type": "integer"}, "root": {"type": ["integer", "string"]}}), &["asset", "root"]),
        run: |c, p| {
            let id = id_of(p, "asset")?;
            let root = parse_pitch(p.get("root").unwrap())?;
            let a = c.song.assets.iter_mut().find(|a| a.id == id).ok_or_else(|| anyhow!("no asset {id}"))?;
            a.root_pitch = root;
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "send.set",
        label: "Send to bus",
        description: "Send a copy of a track into a bus (e.g. a shared reverb or a parallel-compression bus). Creates or updates the send. gain_db 0 = full level.",
        mutates: true,
        schema: || {
            obj(
                json!({"track": track_prop(), "to": {"description": "Bus id or name", "type": ["integer", "string"]},
                       "gain_db": {"type": "number"}, "pre_fader": {"type": "boolean", "description": "Tap before the track's volume"}}),
                &["track", "to"],
            )
        },
        run: |c, p| {
            let i = track_idx(c.song, p)?;
            let to = bus_id(c.song, p.get("to").unwrap())?;
            let gain_db = f(p, "gain_db").map(|g| g.clamp(-60.0, 6.0) as f32);
            let pre = b(p, "pre_fader");
            let t = c.song.track_at_mut(i);
            match t.sends.iter_mut().find(|x| x.to == to) {
                Some(x) => {
                    if let Some(g) = gain_db {
                        x.gain_db = g;
                    }
                    if let Some(pre) = pre {
                        x.pre_fader = pre;
                    }
                }
                None => t.sends.push(m2_project::Send { to, gain_db: gain_db.unwrap_or(-6.0), pre_fader: pre.unwrap_or(false) }),
            }
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "send.remove",
        label: "Remove send",
        description: "Stop sending a track into a bus.",
        mutates: true,
        schema: || obj(json!({"track": track_prop(), "to": {"type": ["integer", "string"]}}), &["track", "to"]),
        run: |c, p| {
            let i = track_idx(c.song, p)?;
            let to = bus_id(c.song, p.get("to").unwrap())?;
            c.song.track_at_mut(i).sends.retain(|x| x.to != to);
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "device.setSidechain",
        label: "Set sidechain",
        description: "Key a device (usually m2/duck) from another track: e.g. duck the bass every time the kick hits. The source can be muted (a 'ghost kick'). `source` null removes it.",
        mutates: true,
        schema: || {
            obj(
                json!({"track": track_prop(), "device": {"type": ["string", "integer"], "description": "\"instrument\" or effect index"},
                       "source": {"type": ["integer", "string", "null"], "description": "Track id/name"}}),
                &["track", "device", "source"],
            )
        },
        run: |c, p| {
            let i = track_idx(c.song, p)?;
            let dev = device_id(c.song, i, p.get("device"))?;
            let source = match p.get("source") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    let r = track_ref(v).ok_or_else(|| anyhow!("`source` must be a track"))?;
                    Some(c.song.tracks[c.song.track_index(&r).ok_or_else(|| anyhow!("no track {v}"))?].id)
                }
            };
            let t = c.song.track_at_mut(i);
            if let Some(d) = t.instrument.iter_mut().chain(t.effects.iter_mut()).find(|d| d.id == dev) {
                d.sidechain = source;
            }
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "automation.set",
        label: "Automate",
        description: "Draw automation on a track: `param` volume (dB), pan (-1..1) or macro (0..1, with `device` and `macro`). Points are {bar (1-based, fractional ok), value}; values are interpolated linearly. Replaces that lane.",
        mutates: true,
        schema: || {
            obj(
                json!({"track": track_prop(), "param": {"type": "string", "enum": ["volume", "pan", "macro"]},
                       "device": {"type": ["string", "integer"], "description": "For macro: \"instrument\" or effect index"},
                       "macro": {"type": "string", "description": "For macro: the macro id"},
                       "points": {"type": "array", "items": {"type": "object", "properties": {"bar": {"type": "number"}, "value": {"type": "number"}}, "required": ["bar", "value"]}}}),
                &["track", "param", "points"],
            )
        },
        run: |c, p| {
            let i = track_idx(c.song, p)?;
            let target = auto_target(c.song, i, p)?;
            let tempo = c.song.tempo();
            let (lo, hi) = match target {
                AutoTarget::Volume => (-60.0, 6.0),
                AutoTarget::Pan => (-1.0, 1.0),
                AutoTarget::Macro { .. } => (0.0, 1.0),
            };
            let mut points: Vec<AutoPoint> = p
                .get("points")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow!("`points` is required"))?
                .iter()
                .map(|pt| {
                    let bar = req_f(pt, "bar")?.max(1.0);
                    let value = req_f(pt, "value")?.clamp(lo, hi) as f32;
                    Ok(AutoPoint { tick: tempo.bar_to_tick(bar), value })
                })
                .collect::<Result<_>>()?;
            points.sort_by_key(|x| x.tick);
            points.dedup_by_key(|x| x.tick);
            let id = c.song.alloc_id();
            let t = c.song.track_at_mut(i);
            t.automation.retain(|l| l.target != target);
            if !points.is_empty() {
                t.automation.push(AutomationLane { id, target, points });
            }
            Ok(json!({"lane_id": id}))
        },
    },
    CommandDef {
        id: "automation.clear",
        label: "Clear automation",
        description: "Remove a track's automation for `param` (and `device`/`macro` for macros). Without `param`, clears all of the track's automation.",
        mutates: true,
        schema: || {
            obj(
                json!({"track": track_prop(), "param": {"type": "string", "enum": ["volume", "pan", "macro"]},
                       "device": {"type": ["string", "integer"]}, "macro": {"type": "string"}}),
                &["track"],
            )
        },
        run: |c, p| {
            let i = track_idx(c.song, p)?;
            let target = if p.get("param").is_some() { Some(auto_target(c.song, i, p)?) } else { None };
            let t = c.song.track_at_mut(i);
            t.automation.retain(|l| target.as_ref().is_some_and(|x| *x != l.target));
            Ok(json!({}))
        },
    },
    CommandDef {
        id: "master.set",
        label: "Master volume",
        description: "Set the master volume in dB.",
        mutates: true,
        schema: || obj(json!({"volume_db": {"type": "number"}}), &["volume_db"]),
        run: |c, p| {
            c.song.master.volume_db = req_f(p, "volume_db")?.clamp(-60.0, 6.0) as f32;
            Ok(json!({}))
        },
    },
];

fn m2_time_snap(t: Tick, grid: Tick) -> Tick {
    if grid <= 0 { t } else { ((t as f64 / grid as f64).round() as Tick) * grid }
}

/// Move `pitch` by `steps` scale degrees (snapping it into the key first).
fn scale_step(key: &Key, pitch: u8, steps: i32) -> i32 {
    let scale = if key.scale == Scale::Chromatic { Scale::Major } else { key.scale };
    let key = Key::new(key.root, scale);
    let pitches = key.pitches_in_range(0, 127);
    let snapped = key.snap(pitch);
    let idx = pitches.iter().position(|p| *p == snapped).unwrap_or(0) as i32;
    pitches.get((idx + steps).clamp(0, pitches.len() as i32 - 1) as usize).map(|p| *p as i32).unwrap_or(pitch as i32)
}
