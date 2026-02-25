//! Shared deterministic complex arithmetic helpers.

use crate::nig::ComplexScalar;

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
