//! Verification and regression harness domain.

mod activity_clustering;
mod affine_oracle_method_comparison;
mod coarse_fine_consistency;
mod contracts;
mod deterministic_regression;
mod open_closed_gap_leakage;
mod performance_baseline;
mod regression_drift_attribution;
mod stylized_fact_diagnostics_shared;
mod stylized_fact_metric_catalog;
mod stylized_fact_return_series_shared;
mod sufficient_statistics;
mod tail_skew;
mod time_buckets;
mod volatility_persistence;

pub use activity_clustering::*;
pub use affine_oracle_method_comparison::*;
pub use coarse_fine_consistency::*;
pub use contracts::*;
pub use deterministic_regression::*;
pub use open_closed_gap_leakage::*;
pub use performance_baseline::*;
pub use regression_drift_attribution::*;
pub use stylized_fact_metric_catalog::*;
pub use sufficient_statistics::*;
pub use tail_skew::*;
pub use volatility_persistence::*;
