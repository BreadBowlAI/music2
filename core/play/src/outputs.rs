//! The sound outputs there are, and which one a player opens (Settings, gaps #31; lane C of the
//! next track).
//!
//! A [`Choice`] names an output by its id and a buffer size; [`Player::start_on`](crate::Player::start_on)
//! opens that very device. A chosen output that is gone, or won't open, plays on the system's
//! default instead, and says so ([`Player::fell_back`](crate::Player::fell_back)). A buffer size
//! the device can't take is its default.
//!
//! An output's name is for people, never an identity: ALSA lists many PCMs under one card's name
//! (`hw:`, `plughw:`, `sysdefault:`, `front:`, `dmix:` …). Each name is listed once, from the PCM a
//! person means by it: the default or a sound server's, else the card's shared one, never its raw
//! `hw:` while another opens (NX-S1).
//!
//! `M2_AUDIO` picks how this process meets the sound hardware ([`Audio`]): the devices (unset);
//! none (`none`, the tests' and suites' seam: a silent player on a clock, which lists the outputs
//! `M2_AUDIO_OUTPUTS` names); or none that works (`fail`, a test seam: every start fails as it does
//! on a computer with no sound output). So no test ever opens a real device.

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, SupportedBufferSize};

/// An output device: its id (what a [`Choice`] names), its name as people know it, whether it is
/// the system's default now, its own rate, and the buffer sizes it takes, in frames, when it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub id: String,
    pub name: String,
    pub default: bool,
    pub rate: u32,
    pub frames: Option<(u32, u32)>,
}

/// The output a player opens, by its id ([`Output::id`]), and its buffer size, in frames: None is
/// the system's default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub device: Option<String>,
    pub frames: Option<u32>,
}

/// How this process meets the sound hardware, as `M2_AUDIO` says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Audio {
    /// The OS's devices.
    Device,
    /// No device: a silent player, and these outputs listed (the first is the default), each
    /// its name its id.
    Silent(Vec<String>),
    /// No device starts (a test seam).
    Fail,
}

/// The silent outputs' rate, and the buffer sizes they take.
pub(crate) const SILENT_RATE: u32 = 48_000;
pub(crate) const SILENT_FRAMES: (u32, u32) = (16, 4096);

/// The words a start says when there is no output to open.
pub(crate) const NO_OUTPUT: &str = "no audio output device";

impl Audio {
    pub(crate) fn from_env() -> Audio {
        match std::env::var("M2_AUDIO").as_deref() {
            Ok("none") => {
                let listed = std::env::var("M2_AUDIO_OUTPUTS").unwrap_or_default();
                let names: Vec<String> = listed.split(',').map(str::trim).filter(|n| !n.is_empty()).map(String::from).collect();
                Audio::Silent(if names.is_empty() { vec!["none".into()] } else { names })
            }
            Ok("fail") => Audio::Fail,
            _ => Audio::Device,
        }
    }

    /// The outputs there are, the default first.
    pub(crate) fn outputs(&self) -> Vec<Output> {
        match self {
            Audio::Device => device_outputs(),
            Audio::Silent(names) => names
                .iter()
                .enumerate()
                .map(|(i, name)| Output { id: name.clone(), name: name.clone(), default: i == 0, rate: SILENT_RATE, frames: Some(SILENT_FRAMES) })
                .collect(),
            Audio::Fail => vec![],
        }
    }

    /// The default output's id, if there is one now.
    pub(crate) fn default_output(&self) -> Option<String> {
        match self {
            Audio::Device => cpal::default_host().default_output_device().and_then(|d| id_of(&d)),
            Audio::Silent(names) => names.first().cloned(),
            Audio::Fail => None,
        }
    }

    /// Of the silent outputs, the one `choice` names, and whether it fell back to the default:
    /// the name the silent player takes, and its buffer size.
    pub(crate) fn silent_pick(names: &[String], choice: &Choice) -> (String, bool, Option<u32>) {
        let first = names.first().cloned().unwrap_or_else(|| "none".into());
        let (name, fell_back) = match &choice.device {
            Some(d) if names.contains(d) => (d.clone(), false),
            Some(_) => (first, true),
            None => (first, false),
        };
        (name, fell_back, fixed(choice.frames, Some(SILENT_FRAMES)))
    }
}

/// The outputs there are, the default first: the OS's output devices, or under `M2_AUDIO=none`
/// those `M2_AUDIO_OUTPUTS` names (a comma list; one called "none" when it is unset).
pub fn outputs() -> Vec<Output> {
    Audio::from_env().outputs()
}

/// The default output's id, if there is one now: what the server watches while no output
/// would start (gaps #13).
pub fn default_output() -> Option<String> {
    Audio::from_env().default_output()
}

/// Whether a default output exists now: false under `M2_AUDIO=fail`, true under `none`.
pub fn output_ready() -> bool {
    default_output().is_some()
}

/// A device's name, as the user knows it.
pub(crate) fn name_of(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string())
}

/// A device's id, as a [`Choice`] keeps it: `alsa:sysdefault:CARD=PCH`, `coreaudio:<its UID>`.
pub(crate) fn id_of(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.to_string())
}

/// What a device that opens says of itself: its rate, and the buffer sizes it takes.
type Opens = (u32, Option<(u32, u32)>);

/// The device's output devices, each name once, the default first.
fn device_outputs() -> Vec<Output> {
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| id_of(&d));
    let devices: Vec<(String, String, cpal::Device)> =
        host.output_devices().into_iter().flatten().filter_map(|d| Some((id_of(&d)?, name_of(&d)?, d))).collect();
    let seen: Vec<(String, String)> = devices.iter().map(|(id, name, _)| (id.clone(), name.clone())).collect();
    listed(&seen, default.as_deref(), |i| {
        let config = devices[i].2.default_output_config().ok()?;
        let frames = match *config.buffer_size() {
            SupportedBufferSize::Range { min, max } => Some((min, max)),
            SupportedBufferSize::Unknown => None,
        };
        Some((config.sample_rate(), frames))
    })
}

/// The outputs of the devices `seen` (each `(id, name)`, as the OS lists them), each name once,
/// the default (`default`, an id) first: of the devices a name has, the one a person means
/// ([`rank`]) whose config reads (`opens`, which opens the device: tried best first, and no
/// further than the first that opens). Devices nobody means as an output aren't listed.
fn listed(seen: &[(String, String)], default: Option<&str>, mut opens: impl FnMut(usize) -> Option<Opens>) -> Vec<Output> {
    let mut names: Vec<&str> = vec![];
    for (id, name) in seen {
        if rank(id).is_some() && !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    let mut out = vec![];
    for name in names {
        let mut ways: Vec<(u8, usize)> = seen.iter().enumerate().filter(|(_, (_, n))| n == name).filter_map(|(i, (id, _))| Some((rank(id)?, i))).collect();
        ways.sort_by_key(|&(r, _)| r);
        if let Some((i, (rate, frames))) = ways.into_iter().find_map(|(_, i)| Some((i, opens(i)?))) {
            let id = seen[i].0.clone();
            out.push(Output { default: default == Some(id.as_str()), id, name: name.to_string(), rate, frames });
        }
    }
    default_first(out)
}

/// How much a device is what a person means by its name, best first; None: not an output a person
/// means. Only ALSA lists more than one device a name: its default and sound servers' PCMs, then
/// a card's shared ones, its other plugins, and last its raw `plughw:` and `hw:` (which one stream
/// holds alone). Its `null` opens and discards all it is sent, and `usbstream` isn't for playing:
/// neither is listed (NX-S2).
fn rank(id: &str) -> Option<u8> {
    let Some(pcm) = id.strip_prefix("alsa:") else { return Some(0) };
    match pcm.split(':').next().unwrap_or(pcm) {
        "null" | "usbstream" => None,
        "default" | "sysdefault" | "pipewire" | "pulse" | "jack" => Some(0),
        "plughw" => Some(2),
        "hw" => Some(3),
        _ => Some(1),
    }
}

/// The default first; the rest as the OS lists them.
fn default_first(mut out: Vec<Output>) -> Vec<Output> {
    out.sort_by_key(|o| !o.default);
    out
}

/// The devices a start tries, in order, each with whether it falls back: the one `choice` names
/// (found by its id), then the default, when that one is gone or won't open; only the default
/// when it names none.
pub(crate) fn tries(choice: &Choice) -> Vec<(cpal::Device, bool)> {
    let host = cpal::default_host();
    let chosen = choice.device.as_deref().and_then(|id| host.device_by_id(&id.parse().ok()?));
    in_order(choice.device.is_some(), chosen, host.default_output_device())
}

/// [`tries`]' order: `chosen` (when one is `named`), then `default`, falling back.
fn in_order<D>(named: bool, chosen: Option<D>, default: Option<D>) -> Vec<(D, bool)> {
    chosen.map(|d| (d, false)).into_iter().chain(default.map(|d| (d, named))).collect()
}

/// `frames` when the device takes it (`range`, when it says), else its default.
pub(crate) fn fixed(frames: Option<u32>, range: Option<(u32, u32)>) -> Option<u32> {
    let n = frames?;
    match range {
        Some((min, max)) => (min..=max).contains(&n).then_some(n),
        // A device that doesn't say: its default, as a size it may refuse would fail the start.
        None => None,
    }
}

/// The buffer size a stream opens with: `frames` when `supported` takes it.
pub(crate) fn buffer(frames: Option<u32>, supported: &SupportedBufferSize) -> BufferSize {
    let range = match *supported {
        SupportedBufferSize::Range { min, max } => Some((min, max)),
        SupportedBufferSize::Unknown => None,
    };
    match fixed(frames, range) {
        Some(n) => BufferSize::Fixed(n),
        None => BufferSize::Default,
    }
}

/// When a player let go has freed its device ([`Player::freed`](crate::Player::freed)).
pub struct Freed(std::sync::Arc<crate::Shared>);

impl Freed {
    /// Whether the device is free now.
    pub fn is_free(&self) -> bool {
        self.0.freed.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Wait until the device is free, at most `up_to`: whether it is.
    pub fn wait(&self, up_to: std::time::Duration) -> bool {
        let t = std::time::Instant::now();
        while !self.is_free() {
            if t.elapsed() >= up_to {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        true
    }
}

impl crate::Player {
    /// What says when this player's device is free to open again (NX-S5): once every clone is let
    /// go, its stream stops after playing the unload's fade out. An output only one stream may
    /// hold at once (an ALSA `hw:` device) refuses a new one until then. A silent player's clock
    /// stops as soon as it is let go; a driven one has no device.
    pub fn freed(&self) -> Freed {
        Freed(self.shared.clone())
    }
}

#[cfg(test)]
mod tests;
