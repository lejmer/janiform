use serde::Deserialize;

use crate::complex_math::ComplexScalar;
use crate::hawkes::{HawkesExcitationState, HawkesKernelParameters};
use crate::news::GammaSeverityMarkConfiguration;

use super::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformEquationSpecification,
    ExponentialAffineTransformInput, ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformOutput,
};

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformEquationSpecificationPayload {
    hawkes_kernel_parameters: HawkesKernelParameters,
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformEquationSpecification {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload =
            ExponentialAffineTransformEquationSpecificationPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.hawkes_kernel_parameters,
            payload.gamma_severity_mark_configuration,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComplexScalarPayload {
    real: f64,
    imaginary: f64,
}

impl From<ComplexScalarPayload> for ComplexScalar {
    fn from(payload: ComplexScalarPayload) -> Self {
        Self::new(payload.real, payload.imaginary)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct HawkesExcitationStatePayload {
    y_plus_plus: f64,
    y_plus_minus: f64,
    y_minus_plus: f64,
    y_minus_minus: f64,
}

impl From<HawkesExcitationStatePayload> for HawkesExcitationState {
    fn from(payload: HawkesExcitationStatePayload) -> Self {
        Self {
            y_plus_plus: payload.y_plus_plus,
            y_plus_minus: payload.y_plus_minus,
            y_minus_plus: payload.y_minus_plus,
            y_minus_minus: payload.y_minus_minus,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformInputPayload {
    equation_specification: ExponentialAffineTransformEquationSpecification,
    u_plus: ComplexScalarPayload,
    u_minus: ComplexScalarPayload,
    z_plus: ComplexScalarPayload,
    z_minus: ComplexScalarPayload,
    s_h: f64,
    news_intensity: ExponentialAffineTransformNewsIntensityInput,
    delta: f64,
    initial_state: HawkesExcitationStatePayload,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = ExponentialAffineTransformInputPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.equation_specification,
            payload.u_plus.into(),
            payload.u_minus.into(),
            payload.z_plus.into(),
            payload.z_minus.into(),
            payload.s_h,
            payload.news_intensity,
            payload.delta,
            payload.initial_state.into(),
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformDVectorPayload {
    plus_plus: ComplexScalarPayload,
    plus_minus: ComplexScalarPayload,
    minus_plus: ComplexScalarPayload,
    minus_minus: ComplexScalarPayload,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformDVector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = ExponentialAffineTransformDVectorPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.plus_plus.into(),
            payload.plus_minus.into(),
            payload.minus_plus.into(),
            payload.minus_minus.into(),
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformOutputPayload {
    c_at_delta: ComplexScalarPayload,
    d_at_delta: ExponentialAffineTransformDVector,
    transform_value: ComplexScalarPayload,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformOutput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = ExponentialAffineTransformOutputPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.c_at_delta.into(),
            payload.d_at_delta,
            payload.transform_value.into(),
        )
        .map_err(serde::de::Error::custom)
    }
}
