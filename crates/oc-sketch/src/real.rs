//! Scalars the residual functions are written over: `f64` for values, [`Dual`] for exact
//! first derivatives (forward-mode automatic differentiation).
//!
//! Every residual is written once, generically over [`Real`]. Evaluating it with `f64` gives
//! the residual; evaluating it with `Dual<N>` whose inputs are seeded with unit tangents gives
//! the residual and up to `N` partial derivatives in one pass, exact to rounding. This replaces
//! the TypeScript solver's central differences (`h = 1e-7·max(1, |x|)`, about 1e-9 relative
//! error).
//!
//! Branches (`abs`, `min`, clamping, angle wrapping) are decided on the value, so the
//! derivative is that of the branch taken, as with finite differences away from the kink.

use std::ops::{Add, Div, Mul, Neg, Sub};

pub trait Real:
    Copy
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
    + Add<f64, Output = Self>
    + Sub<f64, Output = Self>
    + Mul<f64, Output = Self>
    + Div<f64, Output = Self>
{
    /// A constant (zero derivative).
    fn cst(v: f64) -> Self;
    fn val(self) -> f64;
    fn sqrt(self) -> Self;
    /// `sqrt(self² + o²)` without overflow, as `Math.hypot`.
    fn hypot(self, o: Self) -> Self;
    /// `atan2(self, x)`: angle of the vector `(x, self)`.
    fn atan2(self, x: Self) -> Self;

    fn abs(self) -> Self {
        if self.val() < 0.0 {
            -self
        } else {
            self
        }
    }
    /// The smaller by value (`Math.min`).
    fn min(self, o: Self) -> Self {
        if o.val() < self.val() {
            o
        } else {
            self
        }
    }
    /// The larger by value (`Math.max`).
    fn max(self, o: Self) -> Self {
        if o.val() > self.val() {
            o
        } else {
            self
        }
    }
    /// JavaScript's `v || fallback` for a number: the fallback when `v` is 0 or NaN.
    fn or(self, fallback: f64) -> Self {
        let v = self.val();
        if v == 0.0 || v.is_nan() {
            Self::cst(fallback)
        } else {
            self
        }
    }
}

impl Real for f64 {
    fn cst(v: f64) -> Self {
        v
    }
    fn val(self) -> f64 {
        self
    }
    fn sqrt(self) -> Self {
        f64::sqrt(self)
    }
    fn hypot(self, o: Self) -> Self {
        f64::hypot(self, o)
    }
    fn atan2(self, x: Self) -> Self {
        f64::atan2(self, x)
    }
}

/// A value with `N` partial derivatives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dual<const N: usize> {
    pub v: f64,
    pub d: [f64; N],
}

impl<const N: usize> Dual<N> {
    /// The `k`-th seeded input: value `v`, derivative 1 in slot `k`.
    pub fn var(v: f64, k: usize) -> Self {
        let mut d = [0.0; N];
        d[k] = 1.0;
        Self { v, d }
    }

    #[inline]
    fn map(self, v: f64, f: impl Fn(f64) -> f64) -> Self {
        let mut d = self.d;
        for x in &mut d {
            *x = f(*x);
        }
        Self { v, d }
    }

    #[inline]
    fn zip(self, o: Self, v: f64, f: impl Fn(f64, f64) -> f64) -> Self {
        let mut d = self.d;
        for (x, y) in d.iter_mut().zip(o.d) {
            *x = f(*x, y);
        }
        Self { v, d }
    }
}

impl<const N: usize> Add for Dual<N> {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        self.zip(o, self.v + o.v, |a, b| a + b)
    }
}
impl<const N: usize> Sub for Dual<N> {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        self.zip(o, self.v - o.v, |a, b| a - b)
    }
}
// The product rule adds; clippy flags `+` inside `Mul` as suspicious.
#[allow(clippy::suspicious_arithmetic_impl)]
impl<const N: usize> Mul for Dual<N> {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        let (a, b) = (self.v, o.v);
        self.zip(o, a * b, |da, db| da * b + a * db)
    }
}
impl<const N: usize> Div for Dual<N> {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        let (a, b) = (self.v, o.v);
        let b2 = b * b;
        self.zip(o, a / b, |da, db| (da * b - a * db) / b2)
    }
}
impl<const N: usize> Neg for Dual<N> {
    type Output = Self;
    fn neg(self) -> Self {
        self.map(-self.v, |x| -x)
    }
}
impl<const N: usize> Add<f64> for Dual<N> {
    type Output = Self;
    fn add(self, o: f64) -> Self {
        Self {
            v: self.v + o,
            d: self.d,
        }
    }
}
impl<const N: usize> Sub<f64> for Dual<N> {
    type Output = Self;
    fn sub(self, o: f64) -> Self {
        Self {
            v: self.v - o,
            d: self.d,
        }
    }
}
impl<const N: usize> Mul<f64> for Dual<N> {
    type Output = Self;
    fn mul(self, o: f64) -> Self {
        self.map(self.v * o, |x| x * o)
    }
}
impl<const N: usize> Div<f64> for Dual<N> {
    type Output = Self;
    fn div(self, o: f64) -> Self {
        self.map(self.v / o, |x| x / o)
    }
}

impl<const N: usize> Real for Dual<N> {
    fn cst(v: f64) -> Self {
        Self { v, d: [0.0; N] }
    }
    fn val(self) -> f64 {
        self.v
    }
    fn sqrt(self) -> Self {
        let s = self.v.sqrt();
        if s == 0.0 {
            return Self::cst(0.0);
        }
        self.map(s, |x| x / (2.0 * s))
    }
    fn hypot(self, o: Self) -> Self {
        let h = self.v.hypot(o.v);
        if h == 0.0 {
            return Self::cst(0.0);
        }
        let (a, b) = (self.v / h, o.v / h);
        self.zip(o, h, |da, db| a * da + b * db)
    }
    fn atan2(self, x: Self) -> Self {
        let (yv, xv) = (self.v, x.v);
        let r2 = xv * xv + yv * yv;
        if r2 == 0.0 {
            return Self::cst(yv.atan2(xv));
        }
        self.zip(x, yv.atan2(xv), |dy, dx| (xv * dy - yv * dx) / r2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Derivatives agree with central differences on a function using every operation.
    #[test]
    fn dual_matches_finite_differences() {
        fn f<T: Real>(a: T, b: T) -> T {
            let h = a.hypot(b * 2.0 - 1.0);
            let t = (b - 0.3).atan2(a + 0.1);
            (h * h + t * t + 1.0).sqrt() * t + a / (b + 3.0)
                - (a * b).abs().min(T::cst(0.7))
                - (-a).max(b * 0.5)
        }
        for &(a, b) in &[(0.7, 1.3), (-2.1, 0.4), (3.0, -1.7)] {
            let g = f(Dual::<2>::var(a, 0), Dual::<2>::var(b, 1));
            assert_eq!(g.v, f(a, b));
            let h = 1e-6;
            let da = (f(a + h, b) - f(a - h, b)) / (2.0 * h);
            let db = (f(a, b + h) - f(a, b - h)) / (2.0 * h);
            assert!((g.d[0] - da).abs() < 1e-7, "{} vs {}", g.d[0], da);
            assert!((g.d[1] - db).abs() < 1e-7, "{} vs {}", g.d[1], db);
        }
    }
}
