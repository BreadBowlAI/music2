//! These tests need real plugins installed (Surge XT, Airwindows Consolidated);
//! each one skips with a message when its plugin is missing.

use super::*;

const SR: f64 = 48_000.0;
const BLOCK: u32 = 128;

fn plugin(id: &str) -> Option<Found> {
    match find(id) {
        Ok(f) => Some(f),
        Err(_) => {
            eprintln!("skipping: {id} is not installed");
            None
        }
    }
}

/// Render `seconds` in blocks, with `events(block_start)` giving each block's events.
fn render(inst: &mut Instance, seconds: f64, input: Option<&dyn Fn(u64) -> f32>, events: &dyn Fn(u64) -> Vec<Event>) -> Vec<f32> {
    let mut out = vec![];
    let (mut l, mut r) = (vec![0.0; BLOCK as usize], vec![0.0; BLOCK as usize]);
    let mut t = 0u64;
    while (t as f64) < seconds * SR {
        let x: Vec<f32> = (0..BLOCK as u64).map(|i| input.map_or(0.0, |f| f(t + i))).collect();
        inst.process(BLOCK, &[&x, &x], &mut [&mut l, &mut r], &events(t), Transport { beats: 0.0, seconds: t as f64 / SR, bpm: 120.0 }).unwrap();
        out.extend_from_slice(&l);
        t += BLOCK as u64;
    }
    out
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

#[test]
fn finds_installed_plugins() {
    let (found, errors) = scan();
    assert!(errors.is_empty(), "{errors:?}");
    for f in &found {
        assert!(!f.id.is_empty() && !f.name.is_empty());
    }
}

#[test]
fn surge_plays_notes() {
    let Some(f) = plugin("org.surge-synth-team.surge-xt") else { return };
    let mut s = Instance::new(&f, SR, BLOCK).unwrap();
    assert!(s.info.note_in && s.info.audio_out == Some(2));
    let off = (0.3 * SR) as u64;
    let out = render(&mut s, 1.5, None, &|t| {
        let mut e = vec![];
        if t == 0 {
            e.push(Event::NoteOn { time: 5, id: 1, key: 57, velocity: 0.9 });
        }
        if (t..t + BLOCK as u64).contains(&off) {
            e.push(Event::NoteOff { time: (off - t) as u32, id: 1, key: 57 });
        }
        e
    });
    assert!(out.iter().all(|x| x.is_finite()));
    assert!(peak(&out[..(0.3 * SR) as usize]) > 0.01, "the note is audible");
    assert!(peak(&out[(1.3 * SR) as usize..]) < 1e-3, "and released after note-off");
    assert!(peak(&out[..5]) == 0.0, "nothing before the note's sample offset");
}

#[test]
fn state_round_trips() {
    let Some(f) = plugin("org.surge-synth-team.surge-xt") else { return };
    let mut a = Instance::new(&f, SR, BLOCK).unwrap();
    let p = a.info.params.iter().find(|p| !p.stepped && p.max > p.min && p.name.contains("Volume")).expect("a volume parameter").clone();
    let target = p.min + (p.max - p.min) * 0.237;
    render(&mut a, 0.01, None, &|t| if t == 0 { vec![Event::Param { time: 0, id: p.id, value: target }] } else { vec![] });
    assert!((a.param(p.id).unwrap() - target).abs() < 1e-6, "{} is set", p.name);

    let bytes = a.save_state().unwrap();
    assert!(!bytes.is_empty());
    let mut b = Instance::new(&f, SR, BLOCK).unwrap();
    assert!((b.param(p.id).unwrap() - target).abs() > 1e-3, "a fresh instance starts elsewhere");
    b.load_state(&bytes).unwrap();
    assert!((b.param(p.id).unwrap() - target).abs() < 1e-6, "{} survives save and load", p.name);
}

#[test]
fn airwindows_processes_audio() {
    let Some(f) = plugin("com.airwindows.consolidated") else { return };
    let mut fx = Instance::new(&f, SR, BLOCK).unwrap();
    assert_eq!((fx.info.audio_in, fx.info.audio_out), (Some(2), Some(2)));
    let sine = |t: u64| (t as f32 / SR as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.5;
    let out = render(&mut fx, 0.5, Some(&sine), &|_| vec![]);
    assert!(out.iter().all(|x| x.is_finite()));
    assert!(peak(&out[(0.25 * SR) as usize..]) > 0.05, "audio comes through");
    fx.reset().unwrap();
}

#[test]
fn param_names_become_unique_ports() {
    let p = |id, name: &str| Param { id, name: name.into(), module: String::new(), min: 0.0, max: 1.0, default: 0.0, stepped: false };
    let names = param_port_names(&[p(1, "Dry/Wet"), p(2, "-"), p(3, "-"), p(4, "Filter 1 Cutoff"), p(5, "in"), p(6, "2nd")]);
    assert_eq!(names, ["dry_wet", "p_", "p__3", "filter_1_cutoff", "in_", "p_2nd"]);
}

#[test]
fn parameter_values_have_display_text() {
    let Some(f) = plugin("org.surge-synth-team.surge-xt") else { return };
    let s = Instance::new(&f, SR, BLOCK).unwrap();
    let p = s.info.params.iter().find(|p| p.name == "A Filter 1 Type").expect("a filter type parameter");
    let names: Vec<String> = (0..=20).map(|i| s.value_text(p.id, i as f64 / 20.0).unwrap_or_default()).collect();
    println!("A Filter 1 Type: {names:?}");
    assert!(names.iter().any(|n| !n.is_empty()));
}
