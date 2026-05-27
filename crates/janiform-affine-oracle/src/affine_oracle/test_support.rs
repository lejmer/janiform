use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_model_kernels::news::GammaSeverityMarkConfiguration;

use super::contracts::ExponentialAffineTransformEquationSpecification;

pub(crate) fn valid_equation_specification() -> ExponentialAffineTransformEquationSpecification {
    ExponentialAffineTransformEquationSpecification::try_new(
        HawkesKernelParameters::try_new(
            0.8,
            1.2,
            HawkesKernelLoadings {
                plus_plus: 0.20,
                plus_minus: 0.08,
                minus_plus: 0.10,
                minus_minus: 0.24,
            },
            HawkesKernelLoadings {
                plus_plus: 0.03,
                plus_minus: 0.02,
                minus_plus: 0.01,
                minus_minus: 0.04,
            },
        )
        .expect("valid kernel payload should construct"),
        GammaSeverityMarkConfiguration::try_new(1.8, 0.9)
            .expect("valid gamma configuration should construct"),
        LongVarianceParameters::neutral(),
    )
    .expect("valid equation specification should construct")
}

pub(crate) const fn standard_initial_state() -> HawkesExcitationState {
    HawkesExcitationState {
        y_plus_plus: 0.4,
        y_plus_minus: 0.2,
        y_minus_plus: 0.3,
        y_minus_minus: 0.5,
    }
}
