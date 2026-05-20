mod bounds;
mod calendar_exposure;
mod components;
mod materialization;

#[cfg(test)]
mod tests;

pub(crate) use bounds::maximum_runtime_hourly_seasonality_factor_from_payload;
pub(crate) use materialization::{
    RuntimeHourlySeasonalityFactorsBuildInput,
    build_runtime_hourly_seasonality_factors_from_components,
};
