use std::ops::Add;

use crate::grids::VanillaOptionNodePriceSensitivity;
use janiform_foundations::complex_math::ComplexScalar;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct NodePriceSharedPlusShiftSensitivity {
    pub(crate) alpha_derivative: f64,
    pub(crate) beta_0_derivative: f64,
    pub(crate) delta_derivative: f64,
    pub(crate) delta_beta_derivative: f64,
}

impl NodePriceSharedPlusShiftSensitivity {
    pub(crate) fn zero() -> Self {
        Self::default()
    }

    pub(crate) fn plus(self, other: Self) -> Self {
        self + other
    }

    pub(crate) fn into_output(self) -> VanillaOptionNodePriceSensitivity {
        VanillaOptionNodePriceSensitivity {
            alpha_derivative: self.alpha_derivative,
            beta_0_derivative: self.beta_0_derivative,
            delta_derivative: self.delta_derivative,
            delta_beta_derivative: self.delta_beta_derivative,
        }
    }
}

impl Add for NodePriceSharedPlusShiftSensitivity {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        Self {
            alpha_derivative: self.alpha_derivative + other.alpha_derivative,
            beta_0_derivative: self.beta_0_derivative + other.beta_0_derivative,
            delta_derivative: self.delta_derivative + other.delta_derivative,
            delta_beta_derivative: self.delta_beta_derivative + other.delta_beta_derivative,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NodePriceSharedPlusShiftComplexSensitivity {
    pub(crate) alpha_derivative: ComplexScalar,
    pub(crate) beta_0_derivative: ComplexScalar,
    pub(crate) delta_derivative: ComplexScalar,
    pub(crate) delta_beta_derivative: ComplexScalar,
}

impl NodePriceSharedPlusShiftComplexSensitivity {
    pub(crate) fn zero() -> Self {
        Self {
            alpha_derivative: ComplexScalar::new(0.0, 0.0),
            beta_0_derivative: ComplexScalar::new(0.0, 0.0),
            delta_derivative: ComplexScalar::new(0.0, 0.0),
            delta_beta_derivative: ComplexScalar::new(0.0, 0.0),
        }
    }

    pub(crate) fn scale(self, factor: f64) -> Self {
        Self {
            alpha_derivative: self.alpha_derivative.scale(factor),
            beta_0_derivative: self.beta_0_derivative.scale(factor),
            delta_derivative: self.delta_derivative.scale(factor),
            delta_beta_derivative: self.delta_beta_derivative.scale(factor),
        }
    }

    pub(crate) fn plus(self, other: Self) -> Self {
        self + other
    }

    pub(crate) fn conjugate(self) -> Self {
        Self {
            alpha_derivative: self.alpha_derivative.conjugate(),
            beta_0_derivative: self.beta_0_derivative.conjugate(),
            delta_derivative: self.delta_derivative.conjugate(),
            delta_beta_derivative: self.delta_beta_derivative.conjugate(),
        }
    }
}

impl Add for NodePriceSharedPlusShiftComplexSensitivity {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        Self {
            alpha_derivative: self.alpha_derivative + other.alpha_derivative,
            beta_0_derivative: self.beta_0_derivative + other.beta_0_derivative,
            delta_derivative: self.delta_derivative + other.delta_derivative,
            delta_beta_derivative: self.delta_beta_derivative + other.delta_beta_derivative,
        }
    }
}
