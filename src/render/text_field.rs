use core::ops::Range;

use crate::{
    font::{Font, FontMetrics, FontRender},
    primitives::{
        Interpolate, Point, Size,
        geometry::{Rectangle, RoundedRectangle},
        transform::LinearTransform,
    },
    render::{AnimatedJoin, AnimationDomain, ContentShape, IntrinsicShape, Render},
    render_target::{Glyph, RenderTarget, SolidBrush},
};

/// The most lines a text field shows at once.
pub const MAX_VISIBLE_LINES: usize = 8;

/// How wide a text field's scroll bar is.
pub const SCROLL_BAR_WIDTH: u32 = 3;

/// The colors a text field draws in besides its text's.
pub trait AreaStyle<C> {
    /// The caret's color, given the text's.
    fn caret_color(&self, text: C) -> C;
    /// The track and thumb of the bar showing which lines are shown, when
    /// there is one.
    fn scroll_bar(&self) -> Option<(C, C)>;
}

/// The caret takes the text's color, and there is no scroll bar.
impl<C> AreaStyle<C> for () {
    fn caret_color(&self, text: C) -> C {
        text
    }

    fn scroll_bar(&self) -> Option<(C, C)> {
        None
    }
}

/// Colors of a text field's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AreaColors<C> {
    /// The caret, or the text's color when `None`.
    pub caret: Option<C>,
    /// The scroll bar's track and thumb. Without them, a draft longer than
    /// the field shows no bar.
    pub scroll_bar: Option<(C, C)>,
}

impl<C> Default for AreaColors<C> {
    fn default() -> Self {
        Self {
            caret: None,
            scroll_bar: None,
        }
    }
}

impl<C: Copy> AreaStyle<C> for AreaColors<C> {
    fn caret_color(&self, text: C) -> C {
        self.caret.unwrap_or(text)
    }

    fn scroll_bar(&self) -> Option<(C, C)> {
        self.scroll_bar
    }
}

/// Which of a field's lines are shown, when it has more than it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrolled {
    pub first: u16,
    pub shown: u16,
    pub total: u16,
}

/// Editable text, drawn as the lines its field broke it into, with a caret.
#[derive(Debug, PartialEq)]
pub struct TextField<'a, F: Font, const N: usize, K = ()> {
    pub origin: Point,
    pub size: Size,
    pub font: &'a F,
    pub attributes: F::Attributes,
    pub text: heapless::String<N>,
    /// The byte ranges of the lines shown, top to bottom.
    pub lines: heapless::Vec<Range<u16>, MAX_VISIBLE_LINES>,
    pub line_height: u32,
    /// The caret, relative to the origin.
    pub caret: Option<Rectangle>,
    /// Which lines are shown, when not all of them are.
    pub scrolled: Option<Scrolled>,
    /// The colors besides the text's.
    pub style: K,
}

impl<F: Font, const N: usize, K: Clone> Clone for TextField<'_, F, N, K> {
    fn clone(&self) -> Self {
        Self {
            origin: self.origin,
            size: self.size,
            font: self.font,
            attributes: self.attributes.clone(),
            text: self.text.clone(),
            lines: self.lines.clone(),
            line_height: self.line_height,
            caret: self.caret.clone(),
            scrolled: self.scrolled,
            style: self.style.clone(),
        }
    }
}

impl<F: Font, const N: usize, K> TextField<'_, F, N, K> {
    fn draw<C: Copy>(
        &self,
        render_target: &mut impl RenderTarget<ColorFormat = C>,
        style: &C,
        origin: Point,
    ) where
        F: FontRender<C>,
        K: AreaStyle<C>,
    {
        let bounds = Rectangle::new(origin, self.size);
        if self.size.area() == 0 || !bounds.intersects(&render_target.clip_rect()) {
            return;
        }
        let metrics = self.font.metrics(&self.attributes);
        let brush = SolidBrush::new(*style);
        let mut y = origin.y;
        for range in &self.lines {
            let line = self
                .text
                .get(usize::from(range.start)..usize::from(range.end))
                .unwrap_or_default();
            // Spaces hanging at the end of a line, and its newline, take no
            // room on screen.
            let content = line.trim_end();
            let width = content.chars().map(|c| metrics.advance(c)).sum::<u32>();
            let line_box =
                Rectangle::new(Point::new(origin.x, y), Size::new(width, self.line_height));
            let mut x = 0;
            render_target.draw_glyphs(
                line_box.origin,
                &brush,
                content.chars().map(|c| {
                    let glyph = Glyph {
                        character: c,
                        offset: Point::new(x, 0),
                    };
                    x += metrics.advance(c) as i32;
                    glyph
                }),
                self.font,
                &self.attributes,
                &line_box,
            );
            y += self.line_height as i32;
        }
        if let Some(caret) = &self.caret {
            render_target.fill(
                LinearTransform::default(),
                &SolidBrush::new(self.style.caret_color(*style)),
                None,
                &Rectangle::new(origin + caret.origin, caret.size),
            );
        }
        if let (Some(scrolled), Some((track, thumb))) = (self.scrolled, self.style.scroll_bar()) {
            // Along the trailing edge, the thumb's share of the track the
            // share of the lines shown.
            let x = origin.x + self.size.width.saturating_sub(SCROLL_BAR_WIDTH) as i32;
            let height = self.size.height;
            let total = u32::from(scrolled.total.max(1));
            let thumb_top = height * u32::from(scrolled.first) / total;
            let thumb_length = (height * u32::from(scrolled.shown) / total).max(SCROLL_BAR_WIDTH);
            for (top, length, color) in [(0, height, track), (thumb_top, thumb_length, thumb)] {
                render_target.fill(
                    LinearTransform::default(),
                    &SolidBrush::new(color),
                    None,
                    &RoundedRectangle::new(
                        Point::new(x, origin.y + top as i32),
                        Size::new(SCROLL_BAR_WIDTH, length),
                        SCROLL_BAR_WIDTH / 2,
                    ),
                );
            }
        }
    }
}

impl<F: Font, const N: usize, K> AnimatedJoin for TextField<'_, F, N, K> {
    fn join_from(&mut self, source: &Self, domain: &AnimationDomain) {
        // The text and caret jump; the field moves.
        self.origin = Interpolate::interpolate(source.origin, self.origin, domain.factor);
    }
}

impl<C: Copy, F: FontRender<C>, const N: usize, K: AreaStyle<C>> Render<C>
    for TextField<'_, F, N, K>
{
    fn render(&self, render_target: &mut impl RenderTarget<ColorFormat = C>, style: &C) {
        self.draw(render_target, style, self.origin);
    }

    fn render_animated(
        render_target: &mut impl RenderTarget<ColorFormat = C>,
        source: &Self,
        target: &Self,
        style: &C,
        domain: &AnimationDomain,
    ) {
        let origin = Interpolate::interpolate(source.origin, target.origin, domain.factor);
        target.draw(render_target, style, origin);
    }
}

impl<F: Font, const N: usize, K> IntrinsicShape for TextField<'_, F, N, K> {
    fn content_shape(&self) -> ContentShape {
        Rectangle::new(self.origin, self.size).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::CharacterBufferFont;
    use crate::render_target::FixedTextBuffer;

    fn field<K>(caret_style: K) -> TextField<'static, CharacterBufferFont, 8, K> {
        TextField {
            origin: Point::zero(),
            size: Size::new(4, 1),
            font: &CharacterBufferFont,
            attributes: Default::default(),
            text: heapless::String::try_from("ab").unwrap(),
            lines: core::iter::once(0..2).collect(),
            line_height: 1,
            caret: Some(Rectangle::new(Point::new(2, 0), Size::new(1, 1))),
            scrolled: None,
            style: caret_style,
        }
    }

    #[test]
    fn the_caret_takes_the_text_color_unless_given_its_own() {
        let mut buffer = FixedTextBuffer::<4, 1>::default();
        field(()).render(&mut buffer, &'x');
        assert_eq!(buffer.text[0][..3], ['a', 'b', 'x']);

        let mut buffer = FixedTextBuffer::<4, 1>::default();
        field(AreaColors {
            caret: Some('|'),
            scroll_bar: None,
        })
        .render(&mut buffer, &'x');
        assert_eq!(buffer.text[0][..3], ['a', 'b', '|']);
    }
}
