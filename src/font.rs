use crate::{
    primitives::{Interpolate, Point, geometry::Rectangle},
    render_target::Surface,
};

mod character_buffer_font;
pub use character_buffer_font::CharacterBufferFont;

pub mod prerendered;
pub use prerendered::PrerenderedFont;

#[cfg(feature = "embedded-graphics")]
mod embedded_mono_font;
#[cfg(feature = "rusttype-fonts")]
mod rusttype;
#[cfg(feature = "embedded-graphics")]
mod u8g2;

/// The grapheme clusters of `text`, in order.
///
/// Text is laid out, wrapped, truncated, and edited a cluster at a time, so
/// a sequence such as an emoji with a skin tone or a flag is never split.
/// With the `unicode-segmentation` feature these are extended grapheme
/// clusters (UAX #29); without it, each character is its own cluster.
#[cfg(feature = "unicode-segmentation")]
#[must_use]
pub fn clusters(text: &str) -> impl DoubleEndedIterator<Item = &str> + Clone {
    unicode_segmentation::UnicodeSegmentation::graphemes(text, true)
}

/// The grapheme clusters of `text`, in order.
///
/// Text is laid out, wrapped, truncated, and edited a cluster at a time, so
/// a sequence such as an emoji with a skin tone or a flag is never split.
/// With the `unicode-segmentation` feature these are extended grapheme
/// clusters (UAX #29); without it, each character is its own cluster.
#[cfg(not(feature = "unicode-segmentation"))]
#[must_use]
pub fn clusters(text: &str) -> impl DoubleEndedIterator<Item = &str> + Clone {
    text.char_indices().map(|(i, c)| &text[i..i + c.len_utf8()])
}

/// The clusters of `text` with their byte offsets, in order.
#[must_use]
pub fn cluster_indices(text: &str) -> impl DoubleEndedIterator<Item = (usize, &str)> + Clone {
    let start = text.as_ptr() as usize;
    clusters(text).map(move |cluster| (cluster.as_ptr() as usize - start, cluster))
}

/// A font that renders a character, or a grapheme cluster, at a time.
///
/// A font that only knows characters draws a cluster's characters one
/// after another; a font that draws some clusters as a single glyph, such
/// as an emoji, overrides the cluster methods.
pub trait Font {
    type Attributes: Default + Interpolate + Clone;
    fn metrics(&self, attributes: &Self::Attributes) -> impl FontMetrics;
}

// TODO: This could probably accept a draw target instead of a surface?
// As-is, it limits to basically just embedded-graphics capable fonts.
// Open to outside implementations, so an application can draw some
// clusters itself, such as emoji, beside a font's text glyphs.
pub trait FontRender<Color>: Font {
    /// Render the character by drawing to a surface.
    fn draw(
        &self,
        character: char,
        offset: Point,
        color: Color,
        background_color: Option<Color>,
        attributes: &Self::Attributes,
        surface: &mut impl Surface<Color = Color>,
    );

    /// Render a grapheme cluster by drawing to a surface. By default its
    /// characters are drawn one after another.
    fn draw_cluster(
        &self,
        cluster: &str,
        offset: Point,
        color: Color,
        background_color: Option<Color>,
        attributes: &Self::Attributes,
        surface: &mut impl Surface<Color = Color>,
    ) where
        Color: Copy,
    {
        let metrics = self.metrics(attributes);
        let mut x = 0;
        for character in cluster.chars() {
            self.draw(
                character,
                offset + Point::new(x, 0),
                color,
                background_color,
                attributes,
                surface,
            );
            x += metrics.advance(character) as i32;
        }
    }
}

pub trait FontMetrics {
    /// The rendered size and offset of a glyph, relative to the top left corner
    #[must_use]
    fn rendered_size(&self, character: char) -> Option<Rectangle>;

    /// The default spacing between baselines
    #[must_use]
    fn vertical_metrics(&self) -> VMetrics;

    /// The horizontal advance produced by a character
    #[must_use]
    fn advance(&self, character: char) -> u32;

    /// The horizontal advance produced by a grapheme cluster. By default,
    /// the sum of its characters' advances.
    #[must_use]
    fn cluster_advance(&self, cluster: &str) -> u32 {
        cluster.chars().map(|c| self.advance(c)).sum()
    }

    /// The rendered size and offset of a grapheme cluster, relative to the
    /// top left corner. By default, the union of its characters' glyphs
    /// laid one after another.
    #[must_use]
    fn cluster_rendered_size(&self, cluster: &str) -> Option<Rectangle> {
        let mut chars = cluster.chars();
        if let (Some(character), None) = (chars.next(), chars.next()) {
            return self.rendered_size(character);
        }
        let mut x = 0;
        let mut bounds: Option<Rectangle> = None;
        for character in cluster.chars() {
            if let Some(mut glyph) = self.rendered_size(character) {
                glyph.origin.x += x;
                bounds = Some(bounds.map_or(glyph, |b| b.union(&glyph)));
            }
            x += self.advance(character) as i32;
        }
        bounds
    }

    /// The width of `text` laid out on one line.
    #[must_use]
    fn str_width(&self, text: &str) -> u32 {
        clusters(text).map(|c| self.cluster_advance(c)).sum()
    }
}

impl<T: FontMetrics> FontMetrics for &T {
    fn rendered_size(&self, character: char) -> Option<Rectangle> {
        (*self).rendered_size(character)
    }

    fn vertical_metrics(&self) -> VMetrics {
        (*self).vertical_metrics()
    }

    fn advance(&self, character: char) -> u32 {
        (*self).advance(character)
    }

    fn cluster_advance(&self, cluster: &str) -> u32 {
        (*self).cluster_advance(cluster)
    }

    fn cluster_rendered_size(&self, cluster: &str) -> Option<Rectangle> {
        (*self).cluster_rendered_size(cluster)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VMetrics {
    /// The distance from the baseline to the top of the tallest character
    pub ascent: i32,
    /// The distance from the baseline to the bottom of the lowest character, typically negative
    pub descent: i32,
    /// The spacing between lines, i.e. the space between the bottom of one line's descent to the
    /// top of the next line's ascent
    pub line_spacing: i32,
}

impl VMetrics {
    /// The distance between baselines
    #[must_use]
    pub fn line_height(&self) -> u32 {
        (self.ascent - self.descent + self.line_spacing).max(0) as u32
    }
}

/// A Font attribute allowing customization of size.
pub trait CustomSize {
    #[must_use]
    fn with_size(self, size: u32) -> Self;
}
