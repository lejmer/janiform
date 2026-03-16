//! Shared deterministic complex arithmetic helpers.

use std::fmt::{Display, Formatter};

use serde::Serialize;

const COMPLEX_DIVISION_MIN_DENOMINATOR_NORM: f64 = 1.0e-24;

/// Minimal deterministic complex scalar used by transform-side APIs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ComplexScalar {
    pub real: f64,
    pub imaginary: f64,
}

impl ComplexScalar {
    pub fn new(real: f64, imaginary: f64) -> Self {
        Self { real, imaginary }
    }

    pub fn conjugate(self) -> Self {
        Self::new(self.real, -self.imaginary)
    }

    pub fn square(self) -> Self {
        Self::new(
            self.real
                .mul_add(self.real, -(self.imaginary * self.imaginary)),
            2.0 * self.real * self.imaginary,
        )
    }

    pub fn norm(self) -> f64 {
        self.real.hypot(self.imaginary)
    }

    pub fn add(self, other: Self) -> Self {
        Self::new(self.real + other.real, self.imaginary + other.imaginary)
    }

    pub fn subtract(self, other: Self) -> Self {
        Self::new(self.real - other.real, self.imaginary - other.imaginary)
    }

    pub fn scale(self, value: f64) -> Self {
        Self::new(self.real * value, self.imaginary * value)
    }

    pub fn divide(self, denominator: Self) -> Option<Self> {
        let denominator_norm_squared = denominator.real.mul_add(
            denominator.real,
            denominator.imaginary * denominator.imaginary,
        );
        if !denominator_norm_squared.is_finite()
            || denominator_norm_squared <= COMPLEX_DIVISION_MIN_DENOMINATOR_NORM
        {
            return None;
        }

        let real_numerator = self
            .real
            .mul_add(denominator.real, self.imaginary * denominator.imaginary);
        let imaginary_numerator = self
            .imaginary
            .mul_add(denominator.real, -(self.real * denominator.imaginary));

        let real_component = real_numerator / denominator_norm_squared;
        let imaginary_component = imaginary_numerator / denominator_norm_squared;
        if !real_component.is_finite() || !imaginary_component.is_finite() {
            return None;
        }

        Some(Self::new(real_component, imaginary_component))
    }

    pub fn is_finite(self) -> bool {
        self.real.is_finite() && self.imaginary.is_finite()
    }
}

impl Display for ComplexScalar {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "({}+{}i)", self.real, self.imaginary)
    }
}

pub fn complex_multiply(left: ComplexScalar, right: ComplexScalar) -> ComplexScalar {
    ComplexScalar::new(
        left.real
            .mul_add(right.real, -(left.imaginary * right.imaginary)),
        left.real
            .mul_add(right.imaginary, left.imaginary * right.real),
    )
}

pub fn complex_exponential(value: ComplexScalar) -> ComplexScalar {
    let real_exponential = value.real.exp();
    ComplexScalar::new(
        real_exponential * value.imaginary.cos(),
        real_exponential * value.imaginary.sin(),
    )
}

pub fn complex_log_principal(value: ComplexScalar) -> ComplexScalar {
    ComplexScalar::new(value.norm().ln(), value.imaginary.atan2(value.real))
}

pub fn complex_pow_real_exponent_principal(base: ComplexScalar, exponent: f64) -> ComplexScalar {
    complex_exponential(complex_log_principal(base).scale(exponent))
}
