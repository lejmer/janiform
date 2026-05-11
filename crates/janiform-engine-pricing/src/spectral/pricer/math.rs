use super::domain::SpectralDomain;
use super::{ComplexScalar, NodePriceSharedPlusShiftComplexSensitivity, complex_multiply};

pub(super) fn evaluate_cosine_series(
    coefficients: &[ComplexScalar],
    spectral_domain: SpectralDomain,
    evaluation_point: f64,
    _frequency_index: usize,
) -> ComplexScalar {
    coefficients.iter().enumerate().fold(
        ComplexScalar::new(0.0, 0.0),
        |accumulator, (basis_index, coefficient)| {
            let angle = spectral_domain.basis_frequency(basis_index)
                * (evaluation_point - spectral_domain.deviation_lower_bound);

            accumulator.plus(coefficient.scale(angle.cos()))
        },
    )
}

pub(super) fn evaluate_cosine_series_sensitivity(
    coefficient_sensitivities: &[NodePriceSharedPlusShiftComplexSensitivity],
    spectral_domain: SpectralDomain,
    evaluation_point: f64,
    _frequency_index: usize,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    coefficient_sensitivities.iter().enumerate().fold(
        NodePriceSharedPlusShiftComplexSensitivity::zero(),
        |accumulator, (basis_index, coefficient_sensitivity)| {
            let angle = spectral_domain.basis_frequency(basis_index)
                * (evaluation_point - spectral_domain.deviation_lower_bound);

            accumulator.plus(coefficient_sensitivity.scale(angle.cos()))
        },
    )
}

pub(super) fn complex_scale_sensitivity(
    value: ComplexScalar,
    sensitivity: NodePriceSharedPlusShiftComplexSensitivity,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    NodePriceSharedPlusShiftComplexSensitivity {
        alpha_derivative: complex_multiply(value, sensitivity.alpha_derivative),
        beta_0_derivative: complex_multiply(value, sensitivity.beta_0_derivative),
        delta_derivative: complex_multiply(value, sensitivity.delta_derivative),
        delta_beta_derivative: complex_multiply(value, sensitivity.delta_beta_derivative),
    }
}

pub(super) struct ComplexExpectationSensitivityInput {
    pub(super) basis_forward_phase: ComplexScalar,
    pub(super) basis_forward_phase_sensitivity: NodePriceSharedPlusShiftComplexSensitivity,
    pub(super) forward_value: ComplexScalar,
    pub(super) forward_value_sensitivity: NodePriceSharedPlusShiftComplexSensitivity,
    pub(super) basis_backward_phase: ComplexScalar,
    pub(super) basis_backward_phase_sensitivity: NodePriceSharedPlusShiftComplexSensitivity,
    pub(super) backward_value: ComplexScalar,
    pub(super) backward_value_sensitivity: NodePriceSharedPlusShiftComplexSensitivity,
}

pub(super) fn complex_expectation_sensitivity(
    input: ComplexExpectationSensitivityInput,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    let ComplexExpectationSensitivityInput {
        basis_forward_phase,
        basis_forward_phase_sensitivity,
        forward_value,
        forward_value_sensitivity,
        basis_backward_phase,
        basis_backward_phase_sensitivity,
        backward_value,
        backward_value_sensitivity,
    } = input;

    complex_scale_sensitivity(forward_value, basis_forward_phase_sensitivity)
        .plus(complex_scale_sensitivity(
            basis_forward_phase,
            forward_value_sensitivity,
        ))
        .plus(complex_scale_sensitivity(
            backward_value,
            basis_backward_phase_sensitivity,
        ))
        .plus(complex_scale_sensitivity(
            basis_backward_phase,
            backward_value_sensitivity,
        ))
        .scale(0.5)
}
