//! The tables a voice plays: five built in, made by code from their harmonics, and one made from a
//! file. Either way each frame becomes its mip levels the same way ([`make`]), at a voice's birth.

use std::f64::consts::PI;

use m2_kernel::Asset;

use crate::fft::Fft;
use crate::shelf::{FRAME, LEN, LEVELS, MAX_FRAMES, OFFSET, PER_FRAME, TOP};

/// The built-in tables, in the menu's order: their places are what a song's code passes.
pub const BUILT_IN: [&str; 5] = ["basic", "pulse", "vowels", "harmonics", "digital"];

/// A frame's harmonics, `(re, im)` each, by number from 0 (the DC, which a table never holds) to
/// `FRAME / 2 - 1`: the frame is `Σ Re(A_h e^(2πi h t))` over its cycle, `t` from 0 to 1.
pub type Harmonics = [[f64; 2]];

/// The frames of built-in table `t`.
pub fn frames(t: usize) -> usize {
    if t == 0 { 4 } else { 32 }
}

/// A sine of amplitude `a` at harmonic `h`, starting upward.
fn sine(a: &mut Harmonics, h: usize, amplitude: f64) {
    a[h] = [0.0, -amplitude];
}

/// The formants of five vowels (a, e, i, o, u), in Hz: an average man's, after Peterson and Barney.
const VOWELS: [[f64; 3]; 5] =
    [[730.0, 1090.0, 2440.0], [530.0, 1840.0, 2480.0], [270.0, 2290.0, 3010.0], [570.0, 840.0, 2410.0], [300.0, 870.0, 2240.0]];
/// Each formant's width (Hz) and gain.
const WIDTHS: [f64; 3] = [130.0, 150.0, 200.0];
const GAINS: [f64; 3] = [1.0, 0.6, 0.3];
/// The note the vowels are made for: A2. Played higher, they rise with it, as a table does.
const VOICE: f64 = 110.0;

/// Frame `j` of built-in table `t` (`t` from 0 to 4, `j` below its [`frames`]).
///
/// | Table | Its frames, from position 0 to 1 |
/// |---|---|
/// | basic | a sine, a triangle, a saw (falling) and a square |
/// | pulse | a pulse, from a square (half high) to a narrow one (2 % high) |
/// | vowels | a saw through a voice's formants, sliding a, e, i, o, u |
/// | harmonics | a saw's harmonics added one by one: a sine, then 2, then 3, … 284 |
/// | digital | a sine and harmonics 2 to 48 switched on in groups, one group more or fewer each frame (a Gray code) |
///
/// Every frame starts upward from 0 where it can, so frames side by side crossfade without
/// cancelling.
pub fn built_in(t: usize, j: usize, a: &mut Harmonics) {
    let top = FRAME / 2;
    let along = j as f64 / (frames(t) - 1) as f64;
    match t {
        0 => match j {
            0 => sine(a, 1, 1.0),
            1 => (1..top).step_by(2).for_each(|h| sine(a, h, 8.0 / (PI * PI) * if h % 4 == 3 { -1.0 } else { 1.0 } / (h * h) as f64)),
            2 => (1..top).for_each(|h| sine(a, h, 2.0 / (PI * h as f64))),
            _ => (1..top).step_by(2).for_each(|h| sine(a, h, 4.0 / (PI * h as f64))),
        },
        1 => {
            // High (+1) for `w` of the cycle from its start, low (-1) for the rest, its DC taken out.
            let w = 0.5 - 0.48 * along;
            for (h, a) in a.iter_mut().enumerate().skip(1) {
                let phi = 2.0 * PI * h as f64 * w;
                *a = [2.0 / (PI * h as f64) * phi.sin(), -2.0 / (PI * h as f64) * (1.0 - phi.cos())];
            }
        }
        2 => {
            let s = along * (VOWELS.len() - 1) as f64;
            let from = (s as usize).min(VOWELS.len() - 2);
            let m = s - from as f64;
            let formants: [f64; 3] = std::array::from_fn(|i| (VOWELS[from][i].ln() * (1.0 - m) + VOWELS[from + 1][i].ln() * m).exp());
            for h in 1..top {
                let f = h as f64 * VOICE;
                let peaks: f64 = (0..3).map(|i| GAINS[i] / (1.0 + ((f - formants[i]) / (0.5 * WIDTHS[i])).powi(2))).sum();
                sine(a, h, (0.05 + peaks) / h as f64);
            }
        }
        3 => {
            let n = (j + 1).max(1.2f64.powi(j as i32) as usize).min(top - 1);
            (1..=n).for_each(|h| sine(a, h, 2.0 / (PI * h as f64)));
        }
        _ => {
            let gray = j ^ (j >> 1);
            sine(a, 1, 1.0);
            for h in 2..=48 {
                if (gray >> ((h - 2) % 5)) & 1 == 1 {
                    sine(a, h, 1.0 / (h as f64).sqrt());
                }
            }
        }
    }
}

/// The frames of the table made from `file`: one from a file shorter than a frame (a single
/// cycle), else its whole frames, at most [`MAX_FRAMES`].
pub fn file_frames(file: &Asset) -> usize {
    let n = file.frames as usize;
    if n < FRAME { 1 } else { (n / FRAME).min(MAX_FRAMES) }
}

/// Reads frames from a file for [`make`]: its channels mixed, through a buffer taken once.
pub struct Reader {
    file: Asset,
    buf: [Vec<f32>; 2],
    re: Vec<f64>,
    im: Vec<f64>,
}

impl Reader {
    pub fn new(file: Asset) -> Self {
        let n = (file.frames as usize).clamp(1, FRAME);
        Reader { file, buf: [vec![0.0; n], vec![0.0; n]], re: vec![0.0; FRAME], im: vec![0.0; FRAME] }
    }

    /// Frame `j`'s harmonics into `a`. A file shorter than a frame is one cycle, whatever its
    /// length: its harmonics below its own Nyquist. Else frame `j` is the file's frame `j`, or,
    /// when the file has more frames than a table holds, the one `j / 63` of the way from its
    /// first to its last.
    pub fn frame(&mut self, j: usize, a: &mut Harmonics, fft: &Fft) {
        let n = self.file.frames as usize;
        let len = n.clamp(1, FRAME);
        let whole = n / FRAME;
        let from = if whole <= MAX_FRAMES { j } else { (j as f64 * (whole - 1) as f64 / (MAX_FRAMES - 1) as f64).round() as usize };
        for (c, buf) in self.buf.iter_mut().enumerate() {
            self.file.read(c, (from * FRAME) as u32, buf);
        }
        let x = |m: usize| 0.5 * (self.buf[0][m] as f64 + self.buf[1][m] as f64);
        if n < FRAME {
            // One cycle of `len` samples: its harmonics by the sum, a turn of `e^(-2πi/len)` each.
            let (cos, sin): (Vec<f64>, Vec<f64>) = (0..len).map(|m| 2.0 * PI * m as f64 / len as f64).map(|t| (t.cos(), t.sin())).unzip();
            for (h, a) in a.iter_mut().enumerate().take(len.div_ceil(2)).skip(1) {
                let (mut re, mut im) = (0.0, 0.0);
                for m in 0..len {
                    let at = h * m % len;
                    re += x(m) * cos[at];
                    im -= x(m) * sin[at];
                }
                *a = [2.0 * re / len as f64, 2.0 * im / len as f64];
            }
            return;
        }
        for m in 0..FRAME {
            self.re[m] = x(m);
        }
        self.im.fill(0.0);
        fft.run(&mut self.re, &mut self.im, false);
        for (h, a) in a.iter_mut().enumerate().skip(1) {
            *a = [2.0 * self.re[h] / FRAME as f64, 2.0 * self.im[h] / FRAME as f64];
        }
    }
}

/// A table of `frames` frames into `out` (`frames * PER_FRAME` floats): each frame's mip levels
/// from its harmonics (`harmonics(j, a, fft)` writes frame `j`'s into `a`, all 0 to start), each
/// level the frame's harmonics up to its top, by an inverse transform of its length. Then the table
/// is scaled so its loudest frame peaks at 1 (a silent one stays silent).
pub fn make(frames: usize, out: &mut [f32], mut harmonics: impl FnMut(usize, &mut Harmonics, &Fft)) {
    let fft = Fft::new();
    let mut a = vec![[0.0; 2]; FRAME / 2];
    let (mut re, mut im) = (vec![0.0; FRAME], vec![0.0; FRAME]);
    for j in 0..frames {
        a.fill([0.0; 2]);
        harmonics(j, &mut a, &fft);
        let frame = &mut out[j * PER_FRAME..(j + 1) * PER_FRAME];
        for k in 0..LEVELS {
            let n = LEN[k];
            let (re, im) = (&mut re[..n], &mut im[..n]);
            re.fill(0.0);
            im.fill(0.0);
            for h in 1..=TOP[k].min(n / 2 - 1) {
                [re[h], im[h]] = a[h];
            }
            fft.run(re, im, true);
            let copy = &mut frame[OFFSET[k]..OFFSET[k] + n + 1];
            for (c, r) in copy.iter_mut().zip(re.iter()) {
                *c = *r as f32;
            }
            copy[n] = copy[0];
        }
    }
    let peak = (0..frames).flat_map(|j| &out[j * PER_FRAME..j * PER_FRAME + LEN[0]]).fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        let gain = 1.0 / peak;
        out[..frames * PER_FRAME].iter_mut().for_each(|v| *v *= gain);
    }
}
