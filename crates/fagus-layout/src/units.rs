//! Layout units: integers in 1/64 CSS pixel.
//!
//! Integer arithmetic keeps layout deterministic across platforms and free of the drift that
//! summing floating-point values over hundreds of lines would cause.

use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// A length in 1/64 CSS pixel.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Au(pub i32);

impl Au {
    pub const ZERO: Self = Self(0);
    /// The smallest positive length.
    pub const EPSILON: Self = Self(1);
    pub const MAX: Self = Self(i32::MAX / 2);
    const SCALE: f32 = 64.0;

    /// The length `px` cut to whole units toward zero, as browsers do: a length taken from a
    /// style never comes out a unit longer than in Chrome, where the units add up over many
    /// lines and decide the page breaks.
    pub fn from_px(px: f32) -> Self {
        if !px.is_finite() {
            return if px > 0.0 {
                Self::MAX
            } else {
                Self(-(i32::MAX / 2))
            };
        }
        Self(
            (px * Self::SCALE)
                .trunc()
                .clamp(-(i32::MAX / 2) as f32, (i32::MAX / 2) as f32) as i32,
        )
    }

    /// The length `px` rounded up to whole units: the width that content (text) needs, which it
    /// must not lose by rounding.
    pub fn from_px_ceil(px: f32) -> Self {
        if !px.is_finite() {
            return Self::from_px(px);
        }
        Self(
            (px * Self::SCALE)
                .ceil()
                .clamp(-(i32::MAX / 2) as f32, (i32::MAX / 2) as f32) as i32,
        )
    }

    pub fn to_px(self) -> f32 {
        self.0 as f32 / Self::SCALE
    }

    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[must_use]
    pub fn clamp_non_negative(self) -> Self {
        Self(self.0.max(0))
    }
}

impl fmt::Debug for Au {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}px", self.to_px())
    }
}

impl Add for Au {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl AddAssign for Au {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl Sub for Au {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl SubAssign for Au {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl Neg for Au {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl Mul<i32> for Au {
    type Output = Self;
    fn mul(self, factor: i32) -> Self {
        Self(self.0.saturating_mul(factor))
    }
}

impl Div<i32> for Au {
    type Output = Self;
    fn div(self, divisor: i32) -> Self {
        Self(self.0 / divisor)
    }
}

impl std::iter::Sum for Au {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Add::add)
    }
}

/// A rectangle in layout units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: Au,
    pub y: Au,
    pub width: Au,
    pub height: Au,
}

impl Rect {
    pub fn new(x: Au, y: Au, width: Au, height: Au) -> Self {
        Self { x, y, width, height }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_and_rounds() {
        assert_eq!(Au::from_px(1.0), Au(64));
        // 2.5mm: 604.7 units, cut to 604 as in Chrome.
        assert_eq!(Au::from_px(2.5 * 96.0 / 25.4), Au(604));
        assert_eq!(Au::from_px(-0.5 / 64.0), Au(0));
        assert_eq!(Au::from_px(0.5).to_px(), 0.5);
        assert_eq!(Au::from_px(f32::INFINITY), Au::MAX);
        let sum: Au = (0..1000).map(|_| Au::from_px(0.1)).sum();
        assert_eq!(sum, Au(6000), "no drift: 1000 × 0.1px is exactly 6000/64");
    }
}
