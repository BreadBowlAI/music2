//! The messages between the app and the host process (HOST-3): framed binary on the child's
//! standard input and output. A frame is its length (u32, little-endian) and that many bytes.
//!
//! | From | Frame |
//! |---|---|
//! | the app | `id` (u64), then a [`Req`] |
//! | the host process | `1`, `id`, then the reply: `1` and its value, or `0` and why (a string) |
//! | the host process | `2`, then a [`HostEvent`] of one of its instances, by its handle there |
//!
//! Numbers are little-endian; a string or bytes are their length (u32) and the bytes; an option is
//! `0`, or `1` and the value; a list is its length (u32) and the items. Both sides are one build:
//! [`STAMP`] is checked at the start.

use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::host::{Blob, HostEvent, MainCall, RenderMode, Reply, WindowCall};
use crate::{Found, Handle, Info, Param};

/// What both sides must agree on: a hash of this file, the slot's layout and the types both use
/// (`host.rs`: `Raw`, `MAX_CALL_EVENTS`, the calls), so a host program built from other sources is
/// refused, not misread.
pub(crate) const STAMP: u64 =
    fnv(include_bytes!("wire.rs")) ^ fnv(include_bytes!("slot.rs")).rotate_left(1) ^ fnv(include_bytes!("../host.rs")).rotate_left(2);

const fn fnv(b: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0;
    while i < b.len() {
        h = (h ^ b[i] as u64).wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    h
}

/// A frame's largest length: a plugin's state may be big, but not this big.
pub(crate) const MAX_FRAME: usize = 1 << 30;

pub(crate) const REPLY: u8 = 1;
pub(crate) const EVENT: u8 = 2;

/// A request, made on the host process's main thread unless said.
#[derive(Debug)]
pub(crate) enum Req {
    /// Answers [`STAMP`] (on the reading thread).
    Hello,
    /// Every installed plugin, and the bundles that failed (on a thread of its own).
    Scan,
    /// Create one (its library loads on a thread of its own first).
    Create {
        found: Found,
        state: Option<Blob>,
    },
    /// Activate, after mapping its slot `shm` when named (a new one, or one of another size).
    /// Answers whether the slot is mapped, then the latency or why it didn't activate.
    Activate {
        h: u64,
        rate: u32,
        max_frames: u32,
        shm: Option<String>,
    },
    Deactivate {
        h: u64,
    },
    LoadState {
        h: u64,
        blob: Blob,
    },
    SetRenderMode {
        h: u64,
        mode: RenderMode,
    },
    MainCall {
        h: u64,
        call: MainCall,
    },
    /// Answered once it is destroyed.
    Destroy {
        h: u64,
    },
    Latency {
        h: u64,
    },
    SaveState {
        h: u64,
    },
}

// ---------------------------------------------------------------- writing

#[derive(Default)]
pub(crate) struct W(pub Vec<u8>);

impl W {
    /// A frame, its length to be filled by [`W::done`].
    pub(crate) fn frame() -> W {
        W(vec![0; 4])
    }

    pub(crate) fn done(mut self) -> Vec<u8> {
        let n = (self.0.len() - 4) as u32;
        self.0[..4].copy_from_slice(&n.to_le_bytes());
        self.0
    }

    pub(crate) fn u8(&mut self, v: u8) -> &mut W {
        self.0.push(v);
        self
    }

    pub(crate) fn u32(&mut self, v: u32) -> &mut W {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub(crate) fn u64(&mut self, v: u64) -> &mut W {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub(crate) fn f64(&mut self, v: f64) -> &mut W {
        self.u64(v.to_bits())
    }

    pub(crate) fn bool(&mut self, v: bool) -> &mut W {
        self.u8(v as u8)
    }

    pub(crate) fn bytes(&mut self, b: &[u8]) -> &mut W {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
        self
    }

    pub(crate) fn str(&mut self, s: &str) -> &mut W {
        self.bytes(s.as_bytes())
    }

    pub(crate) fn opt_u32(&mut self, v: Option<u32>) -> &mut W {
        match v {
            Some(v) => self.u8(1).u32(v),
            None => self.u8(0),
        }
    }

    pub(crate) fn strs(&mut self, v: &[String]) -> &mut W {
        self.u32(v.len() as u32);
        for s in v {
            self.str(s);
        }
        self
    }

    pub(crate) fn blob(&mut self, b: &Blob) -> &mut W {
        self.bytes(&b.bytes);
        self.0.extend_from_slice(&b.sha);
        self
    }

    pub(crate) fn found(&mut self, f: &Found) -> &mut W {
        self.str(&f.id).str(&f.name).str(&f.vendor).str(&f.version).strs(&f.features).str(&f.bundle.to_string_lossy())
    }

    pub(crate) fn info(&mut self, i: &Info) -> &mut W {
        self.found(&i.found).opt_u32(i.audio_in).opt_u32(i.audio_out).bool(i.note_in).u32(i.latency).bool(i.gui).u32(i.params.len() as u32);
        for p in &i.params {
            self.u32(p.id).str(&p.name).str(&p.module).f64(p.min).f64(p.max).f64(p.default).bool(p.stepped);
        }
        self
    }

    pub(crate) fn req(&mut self, id: u64, r: &Req) -> &mut W {
        self.u64(id);
        match r {
            Req::Hello => self.u8(0),
            Req::Scan => self.u8(1),
            Req::Create { found, state } => {
                self.u8(2).found(found);
                match state {
                    Some(b) => self.u8(1).blob(b),
                    None => self.u8(0),
                }
            }
            Req::Activate { h, rate, max_frames, shm } => {
                self.u8(3).u64(*h).u32(*rate).u32(*max_frames);
                match shm {
                    Some(s) => self.u8(1).str(s),
                    None => self.u8(0),
                }
            }
            Req::Deactivate { h } => self.u8(4).u64(*h),
            Req::LoadState { h, blob } => self.u8(5).u64(*h).blob(blob),
            Req::SetRenderMode { h, mode } => self.u8(6).u64(*h).bool(*mode == RenderMode::Offline),
            Req::MainCall { h, call } => {
                self.u8(7).u64(*h);
                match call {
                    MainCall::Value(id) => self.u8(0).u32(*id),
                    MainCall::Text(id, v) => self.u8(1).u32(*id).f64(*v),
                    MainCall::Info => self.u8(2),
                    MainCall::TakeLog => self.u8(3),
                    MainCall::Window(WindowCall::Open { title }) => self.u8(4).str(title),
                    MainCall::Window(WindowCall::Close) => self.u8(5),
                }
            }
            Req::Destroy { h } => self.u8(8).u64(*h),
            Req::Latency { h } => self.u8(9).u64(*h),
            Req::SaveState { h } => self.u8(10).u64(*h),
        }
    }

    pub(crate) fn reply(&mut self, r: &Reply) -> &mut W {
        match r {
            Reply::Value(v) => {
                self.u8(0);
                match v {
                    Some(v) => self.u8(1).f64(*v),
                    None => self.u8(0),
                }
            }
            Reply::Text(t) => {
                self.u8(1);
                match t {
                    Some(t) => self.u8(1).str(t),
                    None => self.u8(0),
                }
            }
            Reply::Info(i) => self.u8(2).info(i),
            Reply::Log(l) => self.u8(3).strs(l),
            Reply::Window(open) => self.u8(4).bool(*open),
        }
    }

    pub(crate) fn event(&mut self, e: &HostEvent) -> &mut W {
        match *e {
            HostEvent::Restart(h) => self.u8(0).u64(h.0),
            HostEvent::Dirty(h) => self.u8(1).u64(h.0),
            HostEvent::Latency(h, f) => self.u8(2).u64(h.0).u32(f),
            HostEvent::Crashed(h) => self.u8(3).u64(h.0),
            HostEvent::Hung(h) => self.u8(4).u64(h.0),
            HostEvent::WindowClosed(h) => self.u8(5).u64(h.0),
        }
    }
}

// ---------------------------------------------------------------- reading

pub(crate) struct R<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> R<'a> {
    pub(crate) fn new(b: &'a [u8]) -> R<'a> {
        R { b, at: 0 }
    }

    /// Where it has read to.
    pub(crate) fn at(&self) -> usize {
        self.at
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.b.len() - self.at < n {
            bail!("a message from the plugin host ended early");
        }
        self.at += n;
        Ok(&self.b[self.at - n..self.at])
    }

    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }

    pub(crate) fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }

    pub(crate) fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }

    pub(crate) fn bytes(&mut self) -> Result<Vec<u8>> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }

    pub(crate) fn str(&mut self) -> Result<String> {
        Ok(String::from_utf8(self.bytes()?)?)
    }

    pub(crate) fn opt_u32(&mut self) -> Result<Option<u32>> {
        Ok(match self.u8()? {
            0 => None,
            _ => Some(self.u32()?),
        })
    }

    pub(crate) fn strs(&mut self) -> Result<Vec<String>> {
        (0..self.u32()?).map(|_| self.str()).collect()
    }

    pub(crate) fn blob(&mut self) -> Result<Blob> {
        let bytes = self.bytes()?;
        Ok(Blob { bytes, sha: self.take(32)?.try_into()? })
    }

    pub(crate) fn found(&mut self) -> Result<Found> {
        Ok(Found {
            id: self.str()?,
            name: self.str()?,
            vendor: self.str()?,
            version: self.str()?,
            features: self.strs()?,
            bundle: PathBuf::from(self.str()?),
        })
    }

    pub(crate) fn info(&mut self) -> Result<Info> {
        let found = self.found()?;
        let (audio_in, audio_out, note_in, latency, gui) = (self.opt_u32()?, self.opt_u32()?, self.bool()?, self.u32()?, self.bool()?);
        let params = (0..self.u32()?)
            .map(|_| {
                Ok(Param {
                    id: self.u32()?,
                    name: self.str()?,
                    module: self.str()?,
                    min: self.f64()?,
                    max: self.f64()?,
                    default: self.f64()?,
                    stepped: self.bool()?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Info { found, audio_in, audio_out, note_in, params, latency, gui })
    }

    pub(crate) fn req(&mut self) -> Result<(u64, Req)> {
        let id = self.u64()?;
        let r = match self.u8()? {
            0 => Req::Hello,
            1 => Req::Scan,
            2 => Req::Create {
                found: self.found()?,
                state: match self.u8()? {
                    0 => None,
                    _ => Some(self.blob()?),
                },
            },
            3 => Req::Activate {
                h: self.u64()?,
                rate: self.u32()?,
                max_frames: self.u32()?,
                shm: match self.u8()? {
                    0 => None,
                    _ => Some(self.str()?),
                },
            },
            4 => Req::Deactivate { h: self.u64()? },
            5 => Req::LoadState { h: self.u64()?, blob: self.blob()? },
            6 => Req::SetRenderMode { h: self.u64()?, mode: if self.bool()? { RenderMode::Offline } else { RenderMode::Realtime } },
            7 => {
                let h = self.u64()?;
                let call = match self.u8()? {
                    0 => MainCall::Value(self.u32()?),
                    1 => MainCall::Text(self.u32()?, self.f64()?),
                    2 => MainCall::Info,
                    3 => MainCall::TakeLog,
                    4 => MainCall::Window(WindowCall::Open { title: self.str()? }),
                    5 => MainCall::Window(WindowCall::Close),
                    t => bail!("no main call {t}"),
                };
                Req::MainCall { h, call }
            }
            8 => Req::Destroy { h: self.u64()? },
            9 => Req::Latency { h: self.u64()? },
            10 => Req::SaveState { h: self.u64()? },
            t => bail!("no request {t}"),
        };
        Ok((id, r))
    }

    pub(crate) fn reply(&mut self) -> Result<Reply> {
        Ok(match self.u8()? {
            0 => Reply::Value(match self.u8()? {
                0 => None,
                _ => Some(self.f64()?),
            }),
            1 => Reply::Text(match self.u8()? {
                0 => None,
                _ => Some(self.str()?),
            }),
            2 => Reply::Info(Box::new(self.info()?)),
            3 => Reply::Log(self.strs()?),
            4 => Reply::Window(self.bool()?),
            t => bail!("no reply {t}"),
        })
    }

    pub(crate) fn event(&mut self) -> Result<HostEvent> {
        Ok(match self.u8()? {
            0 => HostEvent::Restart(Handle(self.u64()?)),
            1 => HostEvent::Dirty(Handle(self.u64()?)),
            2 => HostEvent::Latency(Handle(self.u64()?), self.u32()?),
            3 => HostEvent::Crashed(Handle(self.u64()?)),
            4 => HostEvent::Hung(Handle(self.u64()?)),
            5 => HostEvent::WindowClosed(Handle(self.u64()?)),
            t => bail!("no event {t}"),
        })
    }
}

/// Read one frame from `r`: None at the end of the stream.
pub(crate) fn read_frame(r: &mut impl std::io::Read) -> std::io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "a message longer than any the host sends"));
    }
    let mut b = vec![0; n];
    r.read_exact(&mut b)?;
    Ok(Some(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_request_reads_back_as_written() {
        let found = Found {
            id: "a.b".into(),
            name: "AB".into(),
            vendor: "v".into(),
            version: "1".into(),
            features: vec!["instrument".into()],
            bundle: "/x/AB.clap".into(),
        };
        let blob = Blob::new(vec![1, 2, 3]);
        let reqs = [
            Req::Hello,
            Req::Scan,
            Req::Create { found: found.clone(), state: Some(blob.clone()) },
            Req::Create { found, state: None },
            Req::Activate { h: 5, rate: 48_000, max_frames: 128, shm: Some("/m2-1-2".into()) },
            Req::Activate { h: 5, rate: 44_100, max_frames: 64, shm: None },
            Req::Deactivate { h: 1 },
            Req::LoadState { h: 2, blob },
            Req::SetRenderMode { h: 3, mode: RenderMode::Offline },
            Req::MainCall { h: 4, call: MainCall::Text(7, 0.25) },
            Req::MainCall { h: 4, call: MainCall::TakeLog },
            Req::MainCall { h: 4, call: MainCall::Window(WindowCall::Open { title: "Surge XT (Lead)".into() }) },
            Req::MainCall { h: 4, call: MainCall::Window(WindowCall::Close) },
            Req::Destroy { h: 9 },
            Req::Latency { h: 10 },
            Req::SaveState { h: 11 },
        ];
        for (i, r) in reqs.iter().enumerate() {
            let mut w = W::frame();
            w.req(i as u64, r);
            let b = w.done();
            assert_eq!(u32::from_le_bytes(b[..4].try_into().unwrap()) as usize, b.len() - 4);
            let (id, back) = R::new(&b[4..]).req().unwrap();
            assert_eq!((id, format!("{back:?}")), (i as u64, format!("{r:?}")));
        }
        let info = Info {
            found: Found { id: "p".into(), name: "P".into(), vendor: "".into(), version: "".into(), features: vec![], bundle: "b".into() },
            audio_in: Some(2),
            audio_out: None,
            note_in: true,
            params: vec![Param { id: 1, name: "Gain".into(), module: "m".into(), min: 0.0, max: 2.0, default: 1.0, stepped: false }],
            latency: 64,
            gui: true,
        };
        for r in [Reply::Value(Some(0.5)), Reply::Text(None), Reply::Info(Box::new(info)), Reply::Log(vec!["w".into()]), Reply::Window(true)] {
            let mut w = W::default();
            w.reply(&r);
            assert_eq!(format!("{:?}", R::new(&w.0).reply().unwrap()), format!("{r:?}"));
        }
    }
}
