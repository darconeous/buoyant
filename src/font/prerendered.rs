//! Fonts rasterized ahead of time into anti-aliased coverage maps.
//!
//! A [`PrerenderedFont`] is one typeface at one weight, rendered at several
//! pixel sizes. The size attribute picks the closest rendered [`Face`], so a
//! view asks for the size it wants and gets the nearest one that exists.
//!
//! Coverage is stored at 4 bits per pixel. Each pixel blends the text color
//! into the layer's background hint by its coverage. Without a hint there is
//! nothing to blend into, so a pixel is drawn in full or skipped at half
//! coverage.
//!
//! The tables are meant to be generated, typically by a build script that
//! rasterizes a font file, and placed in static memory.

use crate::font::{CustomSize, Font, FontMetrics, FontRender, VMetrics};
use crate::primitives::{Interpolate, Pixel, Point, Size, geometry::Rectangle};
use crate::render_target::Surface;

/// One rasterized glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphEntry {
    pub character: char,
    /// Horizontal distance to the next glyph's origin.
    pub advance: u16,
    /// Left edge of the coverage map, relative to the glyph origin.
    pub left: i16,
    /// Top edge of the coverage map, relative to the top of the line box.
    pub top: i16,
    pub width: u16,
    pub height: u16,
    /// Index of the glyph's first pixel in [`Face::coverage`], counted in
    /// pixels (two per byte, high nibble first).
    pub start: u32,
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
    /// Sorted by character.
    pub glyphs: &'static [GlyphEntry],
    /// 4-bit coverage, two pixels per byte, high nibble first.
    pub coverage: &'static [u8],
    /// Drawn for a character the face does not have.
    pub fallback: Option<GlyphEntry>,
}

impl Face {
    #[must_use]
    pub fn glyph(&self, character: char) -> Option<&GlyphEntry> {
        // Glyphs are sorted, and tables usually open with a run of
        // consecutive characters (printable ASCII): within that run a
        // character's index is its distance from the first.
        // Below the first character this wraps to an index past the end.
        let first = self.glyphs.first().map_or(0, |g| u32::from(g.character));
        let index = u32::from(character).wrapping_sub(first) as usize;
        self.glyphs
            .get(index)
            .filter(|g| g.character == character)
            .or_else(|| {
                self.glyphs
                    .binary_search_by_key(&character, |g| g.character)
                    .ok()
                    .and_then(|i| self.glyphs.get(i))
            })
            .or(self.fallback.as_ref())
    }

    fn coverage_at(&self, index: u32) -> u8 {
        let byte = self
            .coverage
            .get((index / 2) as usize)
            .copied()
            .unwrap_or(0);
        if index.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 0x0f
        }
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
        let width = u32::from(glyph.width);
        // Each of the 16 coverage levels blended once, rather than once a
        // pixel. `None` is not drawn.
        let shades: [Option<C>; 16] = core::array::from_fn(|c| match (c, background_color) {
            (0, _) => None,
            (15, _) => Some(color),
            (c, Some(background)) => Some(C::interpolate(background, color, c as u8 * 17)),
            (c, None) => (c >= 8).then_some(color),
        });
        let pixels = (0..u32::from(glyph.height)).flat_map(move |y| {
            (0..width).filter_map(move |x| {
                shades[usize::from(face.coverage_at(glyph.start + y * width + x))].map(|color| {
                    Pixel {
                        point: origin + Point::new(x as i32, y as i32),
                        color,
                    }
                })
            })
        });
        surface.draw_iter(pixels);
    }
}
