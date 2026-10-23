//! Fixed-point numbers. Sim state never holds floats: f32/f64 results differ across
//! compilers, targets and optimisation levels, and replays must reproduce bit for bit.

use core::fmt;
use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// Signed 48.16 fixed point.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Fx(pub i64);

impl Fx {
    pub const FRAC_BITS: u32 = 16;
    pub const ONE: Fx = Fx(1 << Self::FRAC_BITS);
    pub const ZERO: Fx = Fx(0);

    pub const fn from_int(v: i64) -> Fx {
        Fx(v << Self::FRAC_BITS)
    }

    /// `num / den`, rounded toward zero. `den` must not be zero.
    pub const fn from_ratio(num: i64, den: i64) -> Fx {
        Fx((num << Self::FRAC_BITS) / den)
    }

    /// Integer part, rounded toward negative infinity.
    pub const fn floor_int(self) -> i64 {
        self.0 >> Self::FRAC_BITS
    }

    pub const fn abs(self) -> Fx {
        Fx(self.0.abs())
    }

    pub fn min(self, other: Fx) -> Fx {
        if self <= other { self } else { other }
    }

    pub fn max(self, other: Fx) -> Fx {
        if self >= other { self } else { other }
    }

    /// Square root of a non-negative value; negative input returns zero.
    pub fn sqrt(self) -> Fx {
        if self.0 <= 0 {
            return Fx::ZERO;
        }
        // sqrt(v * 2^16) * 2^8 == sqrt(v) * 2^16, kept exact by working on v << 16 in u128.
        Fx(isqrt_u128((self.0 as u128) << Self::FRAC_BITS) as i64)
    }

    /// Lossy conversion for rendering only. Never feed the result back into the sim.
    pub fn to_f32(self) -> f32 {
        self.0 as f32 / (1u64 << Self::FRAC_BITS) as f32
    }
}

fn isqrt_u128(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let mut x = 1u128 << ((128 - n.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

impl Add for Fx {
    type Output = Fx;
    fn add(self, rhs: Fx) -> Fx {
        Fx(self.0 + rhs.0)
    }
}

impl AddAssign for Fx {
    fn add_assign(&mut self, rhs: Fx) {
        self.0 += rhs.0;
    }
}

impl Sub for Fx {
    type Output = Fx;
    fn sub(self, rhs: Fx) -> Fx {
        Fx(self.0 - rhs.0)
    }
}

impl SubAssign for Fx {
    fn sub_assign(&mut self, rhs: Fx) {
        self.0 -= rhs.0;
    }
}

impl Neg for Fx {
    type Output = Fx;
    fn neg(self) -> Fx {
        Fx(-self.0)
    }
}

impl Mul for Fx {
    type Output = Fx;
    fn mul(self, rhs: Fx) -> Fx {
        Fx(((self.0 as i128 * rhs.0 as i128) >> Self::FRAC_BITS) as i64)
    }
}

impl Div for Fx {
    type Output = Fx;
    fn div(self, rhs: Fx) -> Fx {
        Fx((((self.0 as i128) << Self::FRAC_BITS) / rhs.0 as i128) as i64)
    }
}

impl fmt::Debug for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_f32())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct Vec2 {
    pub x: Fx,
    pub y: Fx,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: Fx::ZERO, y: Fx::ZERO };

    pub const fn new(x: Fx, y: Fx) -> Vec2 {
        Vec2 { x, y }
    }

    pub const fn from_int(x: i64, y: i64) -> Vec2 {
        Vec2 { x: Fx::from_int(x), y: Fx::from_int(y) }
    }

    pub fn length_sq(self) -> Fx {
        self.x * self.x + self.y * self.y
    }

    pub fn length(self) -> Fx {
        self.length_sq().sqrt()
    }

    pub fn distance(self, other: Vec2) -> Fx {
        (other - self).length()
    }

    pub fn scale(self, s: Fx) -> Vec2 {
        Vec2 { x: self.x * s, y: self.y * s }
    }

    /// Moves `self` toward `target` by at most `step`; lands exactly on `target` if within reach.
    pub fn step_toward(self, target: Vec2, step: Fx) -> Vec2 {
        let delta = target - self;
        let dist = delta.length();
        if dist <= step || dist == Fx::ZERO {
            return target;
        }
        self + delta.scale(step / dist)
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2 { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, rhs: Vec2) -> Vec2 {
        Vec2 { x: self.x - rhs.x, y: self.y - rhs.y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul_div_round_trip_and_sign() {
        let a = Fx::from_ratio(7, 2);
        let b = Fx::from_int(-3);
        assert_eq!(a * b, Fx::from_ratio(-21, 2));
        assert_eq!((a * b) / b, a);
        assert_eq!(Fx::from_int(-3).floor_int(), -3);
        assert_eq!(Fx::from_ratio(-1, 2).floor_int(), -1);
    }

    #[test]
    fn sqrt_exact_and_monotonic() {
        assert_eq!(Fx::from_int(144).sqrt(), Fx::from_int(12));
        assert_eq!(Fx::from_ratio(1, 4).sqrt(), Fx::from_ratio(1, 2));
        assert_eq!(Fx::from_int(-4).sqrt(), Fx::ZERO);
        let big = Fx::from_int(1 << 40);
        assert_eq!(big.sqrt(), Fx::from_int(1 << 20));
        let mut prev = Fx::ZERO;
        for i in 0..2000 {
            let s = Fx(i * 977).sqrt();
            assert!(s >= prev);
            prev = s;
        }
    }

    #[test]
    fn step_toward_lands_exactly_and_never_overshoots() {
        let start = Vec2::from_int(0, 0);
        let goal = Vec2::from_int(3, 4);
        let one = start.step_toward(goal, Fx::from_int(1));
        // (0.6, 0.8) is not representable in 16 fractional bits; truncation may only shorten the step.
        assert!(one.length() <= Fx::from_int(1) && Fx::from_int(1) - one.length() <= Fx(4));
        let mut p = start;
        for _ in 0..10 {
            p = p.step_toward(goal, Fx::from_ratio(7, 10));
            assert!(p.distance(start) <= Fx::from_int(5));
        }
        assert_eq!(p, goal);
        assert_eq!(start.step_toward(goal, Fx::from_int(5)), goal);
        assert_eq!(start.step_toward(goal, Fx::from_int(50)), goal);
    }
}
