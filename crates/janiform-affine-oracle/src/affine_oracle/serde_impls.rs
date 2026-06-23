use serde::Deserialize;

use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::{HawkesExcitationState, HawkesKernelParameters};
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_model_kernels::market_pressure::{MarketPressureParameters, MarketPressureState};
use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, NewsComponentMarkScaleConfiguration,
};

use super::{
    ExponentialAffineTransformActivityLoadings, ExponentialAffineTransformDVector,
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformInput,
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformOutput,
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformEquationSpecificationPayload {
    hawkes_kernel_parameters: HawkesKernelParameters,
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
    long_variance_parameters: LongVarianceParameters,
    market_pressure_parameters: MarketPressureParameters,
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
            payload.long_variance_parameters,
            payload.market_pressure_parameters,
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
struct ExponentialAffineTransformActivityLoadingsPayload {
    u_plus_baseline: ComplexScalarPayload,
    u_plus_endogenous: ComplexScalarPayload,
    u_minus_baseline: ComplexScalarPayload,
    u_minus_endogenous: ComplexScalarPayload,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformActivityLoadings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = ExponentialAffineTransformActivityLoadingsPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.u_plus_baseline.into(),
            payload.u_plus_endogenous.into(),
            payload.u_minus_baseline.into(),
            payload.u_minus_endogenous.into(),
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformInputPayload {
    equation_specification: ExponentialAffineTransformEquationSpecification,
    activity_loadings: ExponentialAffineTransformActivityLoadings,
    z_plus: ComplexScalarPayload,
    z_minus: ComplexScalarPayload,
    news_return_loading: ComplexScalarPayload,
    market_pressure_return_loading: ComplexScalarPayload,
    pressure_signed_imbalance_exposure_risk_price: f64,
    seasonality_factor: f64,
    news_intensity: ExponentialAffineTransformNewsIntensityInput,
    known_news_component_mark_scales: NewsComponentMarkScaleConfiguration,
    surprise_news_component_mark_scales: NewsComponentMarkScaleConfiguration,
    delta: f64,
    initial_state: HawkesExcitationStatePayload,
    initial_market_pressure_state: MarketPressureState,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = ExponentialAffineTransformInputPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.equation_specification,
            payload.activity_loadings,
            payload.z_plus.into(),
            payload.z_minus.into(),
            payload.news_return_loading.into(),
            payload.market_pressure_return_loading.into(),
            payload.pressure_signed_imbalance_exposure_risk_price,
            payload.seasonality_factor,
            payload.news_intensity,
            payload.known_news_component_mark_scales,
            payload.surprise_news_component_mark_scales,
            payload.delta,
            payload.initial_state.into(),
            payload.initial_market_pressure_state,
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
    market_pressure_loading_at_delta: ComplexScalarPayload,
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
            payload.market_pressure_loading_at_delta.into(),
            payload.transform_value.into(),
        )
        .map_err(serde::de::Error::custom)
    }
}
