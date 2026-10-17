//! The sound outputs there are, and which one a player opens (Settings, gaps #31; lane C of the
//! next track).
//!
//! A [`Choice`] names an output and a buffer size; [`Player::start_on`](crate::Player::start_on)
//! opens it. A chosen output that is gone plays on the system's default instead, and says so
//! ([`Player::fell_back`](crate::Player::fell_back)). A buffer size the device can't take is its default.
//!
//! `M2_AUDIO` picks how this process meets the sound hardware ([`Audio`]): the devices (unset);
//! none (`none`, the tests' and suites' seam: a silent player on a clock, which lists the outputs
//! `M2_AUDIO_OUTPUTS` names); or none that works (`fail`, a test seam: every start fails as it does
//! on a computer with no sound output). So no test ever opens a real device.

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, SupportedBufferSize};

/// An output device: its name, whether it is the system's default now, its own rate, and the
/// buffer sizes it takes, in frames, when it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub name: String,
    pub default: bool,
    pub rate: u32,
    pub frames: Option<(u32, u32)>,
}

/// The output a player opens and its buffer size, in frames: None is the system's default.
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
    /// No device: a silent player, and these outputs listed (the first is the default).
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
                .map(|(i, name)| Output { name: name.clone(), default: i == 0, rate: SILENT_RATE, frames: Some(SILENT_FRAMES) })
                .collect(),
            Audio::Fail => vec![],
        }
    }

    /// The default output's name, if there is one now.
    pub(crate) fn default_output(&self) -> Option<String> {
        match self {
            Audio::Device => cpal::default_host().default_output_device().and_then(|d| name_of(&d)),
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

/// The default output's name, if there is one now: what the server watches while no output
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

/// The device's output devices, each once, the default first.
fn device_outputs() -> Vec<Output> {
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| name_of(&d));
    let mut out: Vec<Output> = vec![];
    for device in host.output_devices().into_iter().flatten() {
        let Some(name) = name_of(&device) else { continue };
        if out.iter().any(|o| o.name == name) {
            continue;
        }
        let Ok(config) = device.default_output_config() else { continue };
        let frames = match *config.buffer_size() {
            SupportedBufferSize::Range { min, max } => Some((min, max)),
            SupportedBufferSize::Unknown => None,
        };
        out.push(Output { default: default.as_ref() == Some(&name), name, rate: config.sample_rate(), frames });
    }
    default_first(out)
}

/// The default first; the rest as the OS lists them.
fn default_first(mut out: Vec<Output>) -> Vec<Output> {
    out.sort_by_key(|o| !o.default);
    out
}

/// The device `choice` names, or the default when it is gone (true: it fell back), or none.
pub(crate) fn pick(choice: &Choice) -> Option<(cpal::Device, bool)> {
    let host = cpal::default_host();
    if let Some(want) = &choice.device {
        let found = host.output_devices().into_iter().flatten().find(|d| name_of(d).as_deref() == Some(want.as_str()));
        if let Some(d) = found {
            return Some((d, false));
        }
        return host.default_output_device().map(|d| (d, true));
    }
    host.default_output_device().map(|d| (d, false))
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

#[cfg(test)]
mod tests;
