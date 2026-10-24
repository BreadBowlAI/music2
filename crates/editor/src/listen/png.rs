//! `m2 listen --png`'s picture (AGENT-7): the bands over time, a block for the mix and one for each
//! track asked for, one above the other. Each block is 31 third-octave rows, the lowest at its
//! bottom, in columns of 50 ms; a cell's colour is its level, from black (−80 dBFS or quieter)
//! through violet and orange to pale yellow (−10 dBFS or louder).
//!
//! Written as an 8-bit RGB PNG, compressed, by the `png` crate.

use std::io::BufWriter;
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

/// An 8-bit RGB PNG of `rows` (each `3 × width` bytes), compressed.
fn write(path: &Path, width: usize, height: usize, rows: &[Vec<u8>]) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("couldn't write {}", path.display()))?;
    let mut png = png::Encoder::new(BufWriter::new(file), width as u32, height as u32);
    png.set_color(png::ColorType::Rgb);
    png.set_depth(png::BitDepth::Eight);
    png.set_compression(png::Compression::Balanced);
    let written = png.write_header().and_then(|mut w| {
        w.write_image_data(&rows.concat())?;
        w.finish()
    });
    written.with_context(|| format!("couldn't write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // AGENT-7: a PNG decoder reads back every pixel `draw` drew (8-bit RGB: the bands of the first block,
    // the gap's grey, the second block's silence), and the file is compressed: far smaller than its
    // pixels (gaps: it was deflate's stored blocks, a little larger than them).
    #[test]
    fn the_picture_reads_back_as_drawn_and_is_compressed() {
        // A minute of columns, one pixel wide: levels that vary along both ways, and silence.
        let level = |c: usize, b: usize| Some(FLOOR + ((c / 7 + 3 * b) % 75) as f32);
        let bands = Picture { columns: (0..1200).map(|c| std::array::from_fn(|b| level(c, b))).collect() };
        let silent = Picture { columns: vec![[None; 31]; 1200] };
        let path = std::env::temp_dir().join(format!("m2-listen-png-{}.png", std::process::id()));
        let drawn = draw(&[&bands, &silent], &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((drawn.merged, drawn.column_px, drawn.width, drawn.height), (1, 1, 1200, 2 * 31 * ROW_PX + GAP_PX));
        let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        let read = (info.width as usize, info.height as usize, info.color_type, info.bit_depth);
        assert_eq!(read, (drawn.width, drawn.height, png::ColorType::Rgb, png::BitDepth::Eight));
        let block = 31 * ROW_PX;
        for y in 0..drawn.height {
            for x in 0..drawn.width {
                let want = match y {
                    // Band 0 is the lowest, at the block's bottom.
                    y if y < block => colour(level(x, 30 - y / ROW_PX)),
                    y if y < block + GAP_PX => [96; 3],
                    _ => [0; 3],
                };
                let at = 3 * (y * drawn.width + x);
                assert_eq!(pixels[at..at + 3], want, "the pixel at {x}, {y}");
            }
        }
        let raw = drawn.width * drawn.height * 3;
        assert!(bytes.len() * 4 < raw, "compressed: {} bytes for {raw} bytes of pixels", bytes.len());
    }
}
