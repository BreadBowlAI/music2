//! `m2 listen --png`'s picture (AGENT-7): the bands over time, a block for the mix and one for each
//! track asked for, one above the other. Each block is 31 third-octave rows, the lowest at its
//! bottom, in columns of 50 ms; a cell's colour is its level, from black (−80 dBFS or quieter)
//! through violet and orange to pale yellow (−10 dBFS or louder).
//!
//! Written as an 8-bit RGB PNG by a writer of its own: deflate's stored blocks, so no compression
//! and no crate (the `png` crate isn't a dependency of the workspace's own crates).

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use super::Picture;

/// Pixels a row is tall, and between two blocks.
pub const ROW_PX: usize = 4;
pub const GAP_PX: usize = 6;
/// The widest picture, in columns of pixels: more columns are merged, each pixel their loudest.
const MOST: usize = 2400;
/// About how wide a short picture is drawn: each column a few pixels wide, up to 8.
const WIDE: usize = 600;
/// The levels the colours span, in dBFS.
const FLOOR: f32 = -80.0;
const TOP: f32 = -10.0;

/// How a picture was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drawn {
    /// Its 50 ms columns.
    pub columns: usize,
    /// How many of them each pixel's column holds (1 unless the range is long).
    pub merged: usize,
    /// Pixels each drawn column is wide.
    pub column_px: usize,
    pub width: usize,
    pub height: usize,
}

/// Draw `blocks` one above the other into a PNG at `path`.
pub fn draw(blocks: &[&Picture], path: &Path) -> Result<Drawn> {
    let columns = blocks.iter().map(|p| p.columns.len()).max().unwrap_or(0).max(1);
    let merged = columns.div_ceil(MOST);
    let shown = columns.div_ceil(merged);
    let column_px = (WIDE / shown).clamp(1, 8);
    let (width, height) = (shown * column_px, blocks.len() * 31 * ROW_PX + blocks.len().saturating_sub(1) * GAP_PX);
    let mut rows: Vec<Vec<u8>> = Vec::with_capacity(height);
    for (b, p) in blocks.iter().enumerate() {
        if b > 0 {
            rows.extend(std::iter::repeat_n(vec![96u8; 3 * width], GAP_PX));
        }
        for band in (0..31).rev() {
            let mut row = Vec::with_capacity(3 * width);
            for c in 0..shown {
                let cells = p.columns.iter().skip(c * merged).take(merged);
                let level = cells.filter_map(|col| col[band]).fold(None, |m: Option<f32>, l| Some(m.map_or(l, |m| m.max(l))));
                let rgb = colour(level);
                for _ in 0..column_px {
                    row.extend_from_slice(&rgb);
                }
            }
            rows.extend(std::iter::repeat_n(row, ROW_PX));
        }
    }
    write(path, width, height, &rows)?;
    Ok(Drawn { columns, merged, column_px, width, height })
}

/// A level's colour: black, then violet, magenta, orange and pale yellow as it rises.
fn colour(level: Option<f32>) -> [u8; 3] {
    const STOPS: [[f32; 3]; 5] = [[0.0, 0.0, 0.0], [48.0, 18.0, 110.0], [170.0, 32.0, 110.0], [245.0, 125.0, 30.0], [255.0, 245.0, 160.0]];
    let Some(l) = level else { return [0, 0, 0] };
    let t = ((l - FLOOR) / (TOP - FLOOR)).clamp(0.0, 1.0) * (STOPS.len() - 1) as f32;
    let i = (t.floor() as usize).min(STOPS.len() - 2);
    let f = t - i as f32;
    std::array::from_fn(|c| (STOPS[i][c] + (STOPS[i + 1][c] - STOPS[i][c]) * f).round() as u8)
}

/// An 8-bit RGB PNG of `rows` (each `3 × width` bytes), each row unfiltered, in stored blocks.
fn write(path: &Path, width: usize, height: usize, rows: &[Vec<u8>]) -> Result<()> {
    let mut raw = Vec::with_capacity(height * (1 + 3 * width));
    for r in rows {
        raw.push(0);
        raw.extend_from_slice(r);
    }
    // zlib: its header, deflate's stored blocks of at most 65,535 bytes, and Adler-32.
    let mut z = vec![0x78, 0x01];
    let pieces: Vec<&[u8]> = if raw.is_empty() { vec![&[][..]] } else { raw.chunks(65_535).collect() };
    for (i, piece) in pieces.iter().enumerate() {
        z.push(u8::from(i + 1 == pieces.len()));
        let n = piece.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(piece);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [(b"IHDR", &ihdr[..]), (b"IDAT", &z[..]), (b"IEND", &[][..])] {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = png.len();
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        let crc = crc32(&png[start..]);
        png.extend_from_slice(&crc.to_be_bytes());
    }
    let mut f = std::fs::File::create(path).with_context(|| format!("couldn't write {}", path.display()))?;
    f.write_all(&png).with_context(|| format!("couldn't write {}", path.display()))
}

/// CRC-32 as PNG checks its chunks (ISO 3309).
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// Adler-32, as zlib checks what it holds (RFC 1950).
fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u64, 0u64);
    for piece in bytes.chunks(5552) {
        for &x in piece {
            a += x as u64;
            b += a;
        }
        (a, b) = (a % 65_521, b % 65_521);
    }
    ((b << 16) | a) as u32
}

#[cfg(test)]
mod tests {
    // RFC 1950's and ISO 3309's own check values.
    #[test]
    fn the_checks_are_the_standards() {
        assert_eq!(super::adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(super::crc32(b"123456789"), 0xCBF4_3926);
    }
}
