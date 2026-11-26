//! Fonts rasterized ahead of time into anti-aliased coverage maps.
//!
//! A [`PrerenderedFont`] is one typeface at one weight, rendered at several
//! pixel sizes. The size attribute picks the closest rendered [`Face`], so a
//! view asks for the size it wants and gets the nearest one that exists.
//!
//! Coverage has 16 levels. Each pixel blends the text color into the
//! layer's background hint by its coverage. Without a hint there is nothing
//! to blend into, so a pixel is drawn in full or skipped at half coverage.
//! A face stores its coverage Huffman coded, a code for each pixel's
//! neighbors: see [`coding`].
//!
//! The tables are meant to be generated, typically by a build script that
//! rasterizes a font file, and placed in static memory.

use crate::font::{CustomSize, Font, FontMetrics, FontRender, VMetrics};
use crate::primitives::{Interpolate, Pixel, Point, Size, geometry::Rectangle};
use crate::render_target::Surface;

/// One rasterized glyph. Its character is not stored: the face's
/// [`CharRun`]s give each character the index of its entry.
///
/// Every field is a byte, so a table of entries has no padding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphEntry {
    /// Horizontal distance to the next glyph's origin.
    pub advance: u8,
    /// Left edge of the coverage map, relative to the glyph origin.
    pub left: i8,
    /// Top edge of the coverage map, relative to the top of the line box.
    pub top: i8,
    pub width: u8,
    pub height: u8,
    /// Where the glyph's coverage starts in [`Face::coverage`], in bits,
    /// little-endian: see [`GlyphEntry::start`].
    pub start: [u8; 3],
}

impl GlyphEntry {
    /// Where the glyph's coverage starts in [`Face::coverage`], in bits.
    #[must_use]
    pub const fn start(&self) -> u32 {
        u32::from_le_bytes([self.start[0], self.start[1], self.start[2], 0])
    }
}

/// Consecutive characters whose glyphs are consecutive entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharRun {
    pub first: char,
    pub len: u16,
    /// The entry of `first` in [`Face::glyphs`].
    pub index: u16,
}

/// One typeface rasterized at one pixel size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    /// The size the face was rasterized at, in pixels per em.
    pub size: u16,
    pub ascent: i16,
    /// Typically negative.
    pub descent: i16,
    pub line_gap: i16,
    /// The characters the face has, sorted. Faces of one typeface share
    /// them.
    pub runs: &'static [CharRun],
    /// One entry per character, in character order.
    pub glyphs: &'static [GlyphEntry],
    /// Every glyph's coverage, coded as [`coding`] describes, the most
    /// significant bit of each byte first.
    pub coverage: &'static [u8],
    /// The decoding table, [`coding::TABLE`] bytes: see [`coding`]. Faces
    /// coded together share it.
    pub codes: &'static [u8],
    /// Drawn for a character the face does not have.
    pub fallback: Option<GlyphEntry>,
}

impl Face {
    /// The entry of `character` in [`Face::glyphs`], if the face has it.
    #[must_use]
    pub fn index(&self, character: char) -> Option<usize> {
        let c = u32::from(character);
        let within = |run: &CharRun| {
            let offset = c.wrapping_sub(u32::from(run.first));
            (offset < u32::from(run.len)).then(|| usize::from(run.index) + offset as usize)
        };
        // Runs usually open with printable ASCII.
        let first = self.runs.first()?;
        within(first).or_else(|| {
            let after = self.runs.partition_point(|run| run.first <= character);
            within(self.runs.get(after.checked_sub(1)?)?)
        })
    }

    /// Whether the face has its own glyph for `character`.
    #[must_use]
    pub fn has(&self, character: char) -> bool {
        self.index(character).is_some()
    }

    /// The glyph for `character`, or the fallback.
    #[must_use]
    pub fn glyph(&self, character: char) -> Option<&GlyphEntry> {
        self.index(character)
            .and_then(|i| self.glyphs.get(i))
            .or(self.fallback.as_ref())
    }

    /// `glyph`'s coverage, a pixel at a time, row by row.
    #[must_use]
    pub fn pixels(&self, glyph: &GlyphEntry) -> Pixels<'_> {
        let start = glyph.start();
        let mut pixels = Pixels {
            face: self,
            next: (start / 8) as usize,
            buffer: 0,
            filled: 0,
            width: glyph.width,
            remaining: u32::from(glyph.width) * u32::from(glyph.height),
            x: 0,
            y: 0,
            left: coding::band(coding::EDGE) as u8,
            above: [coding::band(coding::EDGE) as u8; 256],
        };
        pixels.refill();
        pixels.consume(start % 8);
        pixels
    }
    /// `glyph`'s coverage, a row at a time: see [`Rows`].
    #[must_use]
    pub fn rows(&self, glyph: &GlyphEntry) -> Rows<'_> {
        let start = glyph.start();
        let mut rows = Rows {
            face: self,
            next: (start / 8) as usize,
            buffer: 0,
            filled: 0,
            width: glyph.width,
            left: glyph.height,
            above: [coding::band(coding::EDGE) as u8; 256],
        };
        rows.refill();
        rows.buffer <<= start % 8;
        rows.filled -= start % 8;
        rows
    }
}

/// A glyph's coverage decoded a row at a time, as [`Pixels`] decodes it
/// but without a call and its bookkeeping for each pixel: a glyph drawn
/// row by row decodes much faster.
#[derive(Clone, Debug)]
pub struct Rows<'a> {
    face: &'a Face,
    /// The coverage byte to read next.
    next: usize,
    /// Bits read and not yet used, from the most significant.
    buffer: u32,
    filled: u32,
    width: u8,
    /// Rows not yet decoded.
    left: u8,
    /// The bands of the row above, by column.
    above: [u8; 256],
}

impl Rows<'_> {
    /// Decodes the next row into `levels`, one level from 0 (clear) to 15
    /// (covered) for each of the glyph's columns; `levels` past the glyph's
    /// width are left alone, and a shorter `levels` takes what fits.
    /// Returns `false`, decoding nothing, past the last row.
    pub fn next_row(&mut self, levels: &mut [u8]) -> bool {
        if self.left == 0 {
            return false;
        }
        self.left -= 1;
        let codes = self.face.codes;
        let coverage = self.face.coverage;
        let (mut buffer, mut filled, mut next) = (self.buffer, self.filled, self.next);
        let mut left = BANDS[usize::from(coding::EDGE)];
        let width = usize::from(self.width);
        for (column, above) in self.above[..width].iter_mut().enumerate() {
            let context = usize::from(left) * coding::LEVELS + usize::from(*above);
            if filled < coding::PEEK as u32 {
                while filled <= 24 {
                    let byte = coverage.get(next).copied().unwrap_or(0);
                    buffer |= u32::from(byte) << (24 - filled);
                    filled += 8;
                    next += 1;
                }
            }
            let window = (buffer >> (32 - coding::PEEK)) as usize;
            let entry = codes
                .get(context << coding::PEEK | window)
                .copied()
                .unwrap_or(0);
            let level = entry & 0x0f;
            // At most 15 bits: the shift stays inside the word.
            let bits = u32::from(entry >> 4);
            buffer <<= bits;
            filled = filled.saturating_sub(bits);
            let band = BANDS[usize::from(level)];
            *above = band;
            left = band;
            if let Some(out) = levels.get_mut(column) {
                *out = level;
            }
        }
        (self.buffer, self.filled, self.next) = (buffer, filled, next);
        true
    }

    /// Tops the buffer up with whole bytes, past the end reading zeros.
    fn refill(&mut self) {
        while self.filled <= 24 {
            let byte = self.face.coverage.get(self.next).copied().unwrap_or(0);
            self.buffer |= u32::from(byte) << (24 - self.filled);
            self.filled += 8;
            self.next += 1;
        }
    }
}

/// A glyph's coverage, decoded a pixel at a time: its column, its row, and
/// its level, from 0 (clear) to 15 (covered).
#[derive(Clone, Debug)]
pub struct Pixels<'a> {
    face: &'a Face,
    /// The coverage byte to read next.
    next: usize,
    /// Bits read and not yet used, from the most significant.
    buffer: u32,
    filled: u32,
    width: u8,
    remaining: u32,
    x: u8,
    y: u8,
    /// The band of the level to the left of the next pixel.
    left: u8,
    /// The bands of the row above, by column: the row being decoded
    /// replaces it as it goes.
    above: [u8; 256],
}

impl Iterator for Pixels<'_> {
    type Item = (u8, u8, u8);

    fn next(&mut self) -> Option<Self::Item> {
        self.remaining = self.remaining.checked_sub(1)?;
        let (x, y) = (self.x, self.y);
        let column = usize::from(x);
        let context = usize::from(self.left) * coding::LEVELS + usize::from(self.above[column]);
        if self.filled < coding::PEEK as u32 {
            self.refill();
        }
        let window = (self.buffer >> (32 - coding::PEEK)) as usize;
        let entry = self
            .face
            .codes
            .get(context << coding::PEEK | window)
            .copied()
            .unwrap_or(0);
        let level = entry & 0x0f;
        self.consume(u32::from(entry >> 4));
        let band = BANDS[usize::from(level)];
        self.above[column] = band;
        self.left = band;
        self.x += 1;
        if self.x == self.width {
            self.x = 0;
            self.y = self.y.wrapping_add(1);
            self.left = BANDS[usize::from(coding::EDGE)];
        }
        Some((x, y, level))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining as usize;
        (remaining, Some(remaining))
    }
}

impl Pixels<'_> {
    /// Tops the buffer up with whole bytes, past the end reading zeros.
    fn refill(&mut self) {
        while self.filled <= 24 {
            let byte = self.face.coverage.get(self.next).copied().unwrap_or(0);
            self.buffer |= u32::from(byte) << (24 - self.filled);
            self.filled += 8;
            self.next += 1;
        }
    }

    fn consume(&mut self, bits: u32) {
        self.buffer = self.buffer.checked_shl(bits).unwrap_or(0);
        self.filled = self.filled.saturating_sub(bits);
    }
}

/// [`coding::band`] of each level, and of [`coding::EDGE`].
const BANDS: [u8; coding::EDGE as usize + 1] = {
    let mut bands = [0; coding::EDGE as usize + 1];
    let mut level = 0;
    while level <= coding::EDGE {
        bands[level as usize] = coding::band(level) as u8;
        level += 1;
    }
    bands
};

/// How coverage is coded.
///
/// Each pixel's level is coded with a prefix code chosen by its context:
/// the levels of its left and upper neighbors, each sorted into one of
/// [`LEVELS`] bands by [`band`], a pixel at a glyph's edge counting as
/// [`EDGE`]. No code is longer than [`PEEK`] bits, so the decoding table
/// gives each context `2^PEEK` entries, indexed by the next `PEEK` bits:
/// each entry is a level in its low four bits and its code's length above
/// them.
pub mod coding {
    /// The level standing for the outside of a glyph.
    pub const EDGE: u8 = 16;
    /// Bands of neighboring levels.
    pub const LEVELS: usize = 6;
    /// The longest code, in bits.
    pub const PEEK: usize = 6;
    /// The number of contexts.
    pub const CONTEXTS: usize = LEVELS * LEVELS;
    /// The length of a decoding table.
    pub const TABLE: usize = CONTEXTS << PEEK;

    /// The band of a neighboring level: clear, faint, half, strong,
    /// covered, or outside the glyph.
    #[must_use]
    pub const fn band(level: u8) -> usize {
        match level {
            0 => 0,
            1..=4 => 1,
            5..=10 => 2,
            11..=14 => 3,
            15 => 4,
            _ => 5,
        }
    }

    /// The context of a pixel whose left and upper neighbors have these
    /// levels.
    #[must_use]
    pub const fn context(left: u8, above: u8) -> usize {
        band(left) * LEVELS + band(above)
    }
}

/// A typeface at one weight, rendered at one or more sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrerenderedFont {
    /// Sorted by size, and never empty.
    pub faces: &'static [Face],
}

impl PrerenderedFont {
    /// The rendered face closest to `size`, preferring the larger on a tie.
    #[must_use]
    pub fn face(&self, size: u32) -> &'static Face {
        let mut best = &self.faces[0];
        for face in self.faces {
            if u32::from(face.size).abs_diff(size) <= u32::from(best.size).abs_diff(size) {
                best = face;
            }
        }
        best
    }
}

/// The requested size in pixels. Zero selects the font's first face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrerenderedAttributes {
    pub size: u32,
}

impl CustomSize for PrerenderedAttributes {
    fn with_size(self, size: u32) -> Self {
        Self { size }
    }
}

impl Interpolate for PrerenderedAttributes {
    fn interpolate(from: Self, to: Self, amount: u8) -> Self {
        Self {
            size: u32::interpolate(from.size, to.size, amount),
        }
    }
}

impl Font for PrerenderedFont {
    type Attributes = PrerenderedAttributes;

    fn metrics(&self, attributes: &Self::Attributes) -> impl FontMetrics {
        self.face(attributes.size)
    }
}

impl FontMetrics for &'static Face {
    fn rendered_size(&self, character: char) -> Option<Rectangle> {
        self.glyph(character).map(|g| {
            Rectangle::new(
                Point::new(i32::from(g.left), i32::from(g.top)),
                Size::new(u32::from(g.width), u32::from(g.height)),
            )
        })
    }

    fn vertical_metrics(&self) -> VMetrics {
        VMetrics {
            ascent: i32::from(self.ascent),
            descent: i32::from(self.descent),
            line_spacing: i32::from(self.line_gap),
        }
    }

    fn advance(&self, character: char) -> u32 {
        self.glyph(character).map_or(0, |g| u32::from(g.advance))
    }
}

impl<C: Interpolate + Copy> FontRender<C> for PrerenderedFont {
    fn draw(
        &self,
        character: char,
        offset: Point,
        color: C,
        background_color: Option<C>,
        attributes: &Self::Attributes,
        surface: &mut impl Surface<Color = C>,
    ) {
        let face = self.face(attributes.size);
        let Some(glyph) = face.glyph(character) else {
            return;
        };
        let origin = offset + Point::new(i32::from(glyph.left), i32::from(glyph.top));
        // Each of the 16 coverage levels blended once, rather than once a
        // pixel. `None` is not drawn.
        let shades: [Option<C>; 16] = core::array::from_fn(|c| match (c, background_color) {
            (0, _) => None,
            (15, _) => Some(color),
            (c, Some(background)) => Some(C::interpolate(background, color, c as u8 * 17)),
            (c, None) => (c >= 8).then_some(color),
        });
        let pixels = face.pixels(glyph).filter_map(move |(x, y, level)| {
            shades[usize::from(level)].map(|color| Pixel {
                point: origin + Point::new(i32::from(x), i32::from(y)),
                color,
            })
        });
        surface.draw_iter(pixels);
    }
}
