use fixed::traits::ToFixed as _;

use crate::primitives::{
    Point, Size,
    geometry::{PathEl, Rectangle, Shape, ShapePathIter},
    transform::{CoordinateSpaceTransform, LinearTransform, ScaleFactor},
};

/// The radius of each corner of a rounded rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CornerRadii<T = u32> {
    pub top_left: T,
    pub top_right: T,
    pub bottom_right: T,
    pub bottom_left: T,
}

impl<T: Copy> CornerRadii<T> {
    /// Every corner rounded by `radius`.
    #[must_use]
    pub const fn equal(radius: T) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    /// Each radius passed through `f`.
    #[must_use]
    pub fn map<U>(self, f: impl Fn(T) -> U) -> CornerRadii<U> {
        CornerRadii {
            top_left: f(self.top_left),
            top_right: f(self.top_right),
            bottom_right: f(self.bottom_right),
            bottom_left: f(self.bottom_left),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundedRectangle {
    pub origin: Point,
    pub size: Size,
    pub radii: CornerRadii,
}

impl RoundedRectangle {
    /// A rectangle with every corner rounded by `radius`.
    #[must_use]
    pub const fn new(origin: Point, size: Size, radius: u32) -> Self {
        Self::with_radii(origin, size, CornerRadii::equal(radius))
    }

    /// A rectangle with each corner rounded by its own radius.
    #[must_use]
    pub const fn with_radii(origin: Point, size: Size, radii: CornerRadii) -> Self {
        Self {
            origin,
            size,
            radii,
        }
    }

    /// Returns a new shape offset by the given point
    #[must_use]
    pub fn with_offset(mut self, offset: Point) -> Self {
        self.origin += offset;
        self
    }

    /// Offsets the shape by the given point
    pub fn offset(&mut self, offset: Point) {
        self.origin += offset;
    }
}

impl CoordinateSpaceTransform for RoundedRectangle {
    fn applying(&self, transform: &LinearTransform) -> Self {
        Self {
            origin: self.origin.applying(transform),
            size: self.size.applying(transform),
            radii: self.radii.map(|r| (r * transform.scale).to_num()),
        }
    }

    fn applying_inverse(&self, transform: &LinearTransform) -> Self {
        Self {
            origin: self.origin.applying_inverse(transform),
            size: self.size.applying_inverse(transform),
            radii: self
                .radii
                .map(|r| (r.to_fixed::<ScaleFactor>() / transform.scale).to_num()),
        }
    }
}

#[cfg(feature = "embedded-graphics")]
impl From<RoundedRectangle> for embedded_graphics::primitives::RoundedRectangle {
    fn from(value: RoundedRectangle) -> Self {
        use embedded_graphics::prelude::*;
        use embedded_graphics::primitives::CornerRadii;

        use crate::primitives::geometry::Rectangle;

        let radii = value.radii.map(Size::new_equal);
        Self::new(
            Rectangle::new(value.origin, value.size).into(),
            CornerRadii {
                top_left: radii.top_left,
                top_right: radii.top_right,
                bottom_right: radii.bottom_right,
                bottom_left: radii.bottom_left,
            },
        )
    }
}

impl Shape for RoundedRectangle {
    type PathElementsIter<'iter>
        = ShapePathIter<10>
    where
        Self: 'iter;

    fn path_elements(&self, _tolerance: u16) -> Self::PathElementsIter<'_> {
        let r = self.radii.map(|r| r as i32);
        let width = self.size.width as i32;
        let height = self.size.height as i32;
        let x = self.origin.x;
        let y = self.origin.y;

        // FIXME: The quad points used here are suboptimal
        let elements = [
            // Starting point
            PathEl::MoveTo(Point::new(x + width - r.top_right, y)),
            // Top edge and top-left corner
            PathEl::LineTo(Point::new(x + r.top_left, y)),
            PathEl::QuadTo(Point::new(x, y), Point::new(x, y + r.top_left)),
            // Left side
            PathEl::LineTo(Point::new(x, y + height - r.bottom_left)),
            // Bottom-left corner
            PathEl::QuadTo(
                Point::new(x, y + height),
                Point::new(x + r.bottom_left, y + height),
            ),
            // Bottom side
            PathEl::LineTo(Point::new(x + width - r.bottom_right, y + height)),
            // Bottom-right corner
            PathEl::QuadTo(
                Point::new(x + width, y + height),
                Point::new(x + width, y + height - r.bottom_right),
            ),
            // Right side
            PathEl::LineTo(Point::new(x + width, y + r.top_right)),
            // Top-right corner
            PathEl::QuadTo(
                Point::new(x + width, y),
                Point::new(x + width - r.top_right, y),
            ),
            // Close the path
            PathEl::ClosePath,
        ];

        ShapePathIter::new(elements)
    }

    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(self.origin, self.size)
    }

    fn as_rounded_rect(&self) -> Option<RoundedRectangle> {
        Some(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::transform::LinearTransform;

    #[test]
    fn applying_transform() {
        let rounded_rect = RoundedRectangle::new(Point::new(10, 20), Size::new(40, 60), 8);
        let transform = LinearTransform::new(Point::new(5, 10), 2.0);

        let transformed = rounded_rect.applying(&transform);

        assert_eq!(transformed.origin, Point::new(25, 50));
        assert_eq!(transformed.size, Size::new(80, 120));
        assert_eq!(transformed.radii, CornerRadii::equal(16));
    }

    #[test]
    fn applying_inverse_transform() {
        let rounded_rect = RoundedRectangle::new(Point::new(50, 80), Size::new(80, 120), 16);
        let transform = LinearTransform::new(Point::new(5, 10), 2.0);

        let inverse_transformed = rounded_rect.applying_inverse(&transform);

        assert_eq!(inverse_transformed.origin, Point::new(22, 35));
        assert_eq!(inverse_transformed.size, Size::new(40, 60));
        assert_eq!(inverse_transformed.radii, CornerRadii::equal(8));
    }

    #[test]
    fn transform_roundtrip() {
        let original = RoundedRectangle::new(Point::new(16, 24), Size::new(32, 48), 8);
        let transform = LinearTransform::new(Point::new(4, 8), 2.0);

        let transformed = original.applying(&transform).applying_inverse(&transform);

        assert_eq!(transformed.origin, original.origin);
        assert_eq!(transformed.size, original.size);
        assert_eq!(transformed.radii, original.radii);
    }

    #[cfg(feature = "embedded-graphics")]
    #[test]
    fn each_corner_keeps_its_radius_in_embedded_graphics() {
        let radii = CornerRadii {
            top_left: 14,
            top_right: 14,
            bottom_right: 5,
            bottom_left: 14,
        };
        let rounded_rect = RoundedRectangle::with_radii(Point::zero(), Size::new(60, 40), radii);
        let converted: embedded_graphics::primitives::RoundedRectangle = rounded_rect.into();
        assert_eq!(
            converted.corners.bottom_right,
            embedded_graphics::prelude::Size::new(5, 5)
        );
        assert_eq!(
            converted.corners.top_left,
            embedded_graphics::prelude::Size::new(14, 14)
        );
    }

    #[test]
    fn identity_transform() {
        let rounded_rect = RoundedRectangle::new(Point::new(100, 200), Size::new(50, 75), 10);
        let identity = LinearTransform::identity();

        let transformed = rounded_rect.applying(&identity);

        assert_eq!(transformed, rounded_rect);
    }
}
