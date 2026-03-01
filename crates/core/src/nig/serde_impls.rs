use serde::Deserialize;

use super::driver::NigSharedPlusShiftConfiguration;
use super::parameters::NigParameters;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigParametersPayload {
    alpha: f64,
    beta: f64,
    delta: f64,
    mu: f64,
}

impl<'de> Deserialize<'de> for NigParameters {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = NigParametersPayload::deserialize(deserializer)?;
        Self::try_new(payload.alpha, payload.beta, payload.delta, payload.mu)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigSharedPlusShiftConfigurationPayload {
    alpha: f64,
    beta_0: f64,
    delta: f64,
    mu: f64,
    delta_beta: f64,
}

impl<'de> Deserialize<'de> for NigSharedPlusShiftConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = NigSharedPlusShiftConfigurationPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.alpha,
            payload.beta_0,
            payload.delta,
            payload.mu,
            payload.delta_beta,
        )
        .map_err(serde::de::Error::custom)
    }
}
