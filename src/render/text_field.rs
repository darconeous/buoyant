use core::ops::Range;

use crate::{
    font::{Font, FontMetrics, FontRender},
    primitives::{Interpolate, Point, Size, geometry::Rectangle, transform::LinearTransform},
    render::{AnimatedJoin, AnimationDomain, ContentShape, IntrinsicShape, Render},
    render_target::{Glyph, RenderTarget, SolidBrush},
};

/// The most lines a text field shows at once.
pub const MAX_VISIBLE_LINES: usize = 8;

/// Editable text, drawn as the lines its field broke it into, with a caret.
#[derive(Debug, PartialEq)]
pub struct TextField<'a, F: Font, const N: usize> {
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
}

impl<F: Font, const N: usize> Clone for TextField<'_, F, N> {
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
        }
    }
}

impl<F: Font, const N: usize> TextField<'_, F, N> {
    fn draw<C: Copy>(
        &self,
        render_target: &mut impl RenderTarget<ColorFormat = C>,
        style: &C,
        origin: Point,
    ) where
        F: FontRender<C>,
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
                &brush,
                None,
                &Rectangle::new(origin + caret.origin, caret.size),
            );
        }
    }
}

impl<F: Font, const N: usize> AnimatedJoin for TextField<'_, F, N> {
    fn join_from(&mut self, source: &Self, domain: &AnimationDomain) {
        // The text and caret jump; the field moves.
        self.origin = Interpolate::interpolate(source.origin, self.origin, domain.factor);
    }
}

impl<C: Copy, F: FontRender<C>, const N: usize> Render<C> for TextField<'_, F, N> {
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

impl<F: Font, const N: usize> IntrinsicShape for TextField<'_, F, N> {
    fn content_shape(&self) -> ContentShape {
        Rectangle::new(self.origin, self.size).into()
    }
}
