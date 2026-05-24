use chrono::{DateTime, Utc};
use janiform::pricing::{VanillaOptionImpliedVolatilityInversionStatus, VanillaOptionPayoffType};
use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;

use super::super::super::validation::pricing_validation_error;

pub(super) const PHYSICAL_DISTRIBUTION_OUTPUT_SCHEMA_VERSION: &str =
    "janiform.observed_options_physical_distribution_diagnostics.v1";
pub(super) const PHYSICAL_DISTRIBUTION_REPORT_SCHEMA_VERSION: &str =
    "janiform.observed_options_physical_distribution_report.v1";

/// Default model-P terminal-distribution diagnostic policy for observed-option pricing.
pub struct ObservedOptionsPhysicalDistributionDiagnosticsDefaults;

impl ObservedOptionsPhysicalDistributionDiagnosticsDefaults {
    /// Number of coarse simulation paths used by the default diagnostic request.
    pub const PATH_COUNT: usize = 1024;
    /// Root seed used by the default diagnostic request.
    pub const SEED: u64 = 0;
}

/// Optional model-P terminal-distribution diagnostic request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedOptionsPhysicalDistributionDiagnosticsRequest {
    /// Number of physical-measure terminal paths sampled per valuation state.
    pub path_count: usize,
    /// Root seed for deterministic path sampling.
    pub seed: u64,
}

impl ObservedOptionsPhysicalDistributionDiagnosticsRequest {
    /// Build a request from the documented default diagnostic policy.
    pub const fn default_policy() -> Self {
        Self {
            path_count: ObservedOptionsPhysicalDistributionDiagnosticsDefaults::PATH_COUNT,
            seed: ObservedOptionsPhysicalDistributionDiagnosticsDefaults::SEED,
        }
    }

    pub(in crate::pricing::observed) fn validate(&self) -> Result<(), WorkflowError> {
        if self.path_count == 0 {
            return Err(pricing_validation_error(
                "physical_distribution_diagnostics.path_count",
                "must be strictly positive",
            ));
        }

        Ok(())
    }
}

/// Model-P distribution diagnostics returned by observed-options pricing.
#[derive(Clone, Debug, Serialize)]
pub struct ObservedOptionsPhysicalDistributionDiagnosticsOutput {
    pub schema_version: &'static str,
    pub request: ObservedOptionsPhysicalDistributionDiagnosticsRequest,
    pub report: ObservedOptionsPhysicalDistributionDiagnosticsReport,
    pub nodes: Vec<ObservedOptionsPhysicalDistributionDiagnosticsNode>,
}

/// Aggregate diagnostics for model-P observed-option terminal sampling.
#[derive(Clone, Debug, Serialize)]
pub struct ObservedOptionsPhysicalDistributionDiagnosticsReport {
    pub schema_version: &'static str,
    pub valuation_count: usize,
    pub node_count: usize,
    pub path_count_per_valuation: usize,
    pub path_partition_count: usize,
    pub worker_count: usize,
    pub maximum_horizon_interval_count: usize,
}

/// One observed option node with model-P terminal distribution equivalents.
#[derive(Clone, Debug, Serialize)]
pub struct ObservedOptionsPhysicalDistributionDiagnosticsNode {
    #[serde(serialize_with = "janiform_foundations::time::serialize_utc_datetime")]
    pub valuation_time_utc: DateTime<Utc>,
    #[serde(serialize_with = "janiform_foundations::time::serialize_utc_datetime")]
    pub quote_timestamp_utc: DateTime<Utc>,
    #[serde(serialize_with = "janiform_foundations::time::serialize_utc_datetime")]
    pub expiry_timestamp_utc: DateTime<Utc>,
    pub strike: f64,
    pub option_type: VanillaOptionPayoffType,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
    pub maturity_interval: f64,
    pub q_forward_price: f64,
    pub p_simulated_forward_price: f64,
    pub model_q_implied_volatility: Option<f64>,
    pub observed_q_implied_volatility: Option<f64>,
    pub observed_q_implied_volatility_inversion_status:
        Option<VanillaOptionImpliedVolatilityInversionStatus>,
    pub sample_count: usize,
    pub physical_forward_normalized_price: f64,
    pub physical_forward_normalized_implied_volatility: Option<f64>,
    pub physical_forward_normalized_implied_volatility_inversion_status:
        VanillaOptionImpliedVolatilityInversionStatus,
    pub market_forward_physical_price: f64,
    pub market_forward_physical_implied_volatility: Option<f64>,
    pub market_forward_physical_implied_volatility_inversion_status:
        VanillaOptionImpliedVolatilityInversionStatus,
}

#[cfg(test)]
mod tests {
    use super::{
        ObservedOptionsPhysicalDistributionDiagnosticsDefaults,
        ObservedOptionsPhysicalDistributionDiagnosticsRequest,
    };

    #[test]
    fn default_policy_uses_documented_physical_distribution_defaults() {
        assert_eq!(
            ObservedOptionsPhysicalDistributionDiagnosticsRequest::default_policy(),
            ObservedOptionsPhysicalDistributionDiagnosticsRequest {
                path_count: ObservedOptionsPhysicalDistributionDiagnosticsDefaults::PATH_COUNT,
                seed: ObservedOptionsPhysicalDistributionDiagnosticsDefaults::SEED,
            }
        );
    }

    #[test]
    fn validate_rejects_empty_physical_distribution_path_count() {
        let request = ObservedOptionsPhysicalDistributionDiagnosticsRequest {
            path_count: 0,
            seed: 0,
        };

        assert!(request.validate().is_err());
    }
}
