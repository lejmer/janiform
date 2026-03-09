//! Verification and regression harness domain.

mod affine_oracle_method_comparison;
mod coarse_fine_consistency;
mod coarse_seeded_tick_consistency;
mod contracts;
mod deterministic_regression;
mod open_closed_gap_leakage;
mod performance_baseline;
mod regression_drift_attribution;
mod scale_consistency;
mod stylized_facts;
mod sufficient_statistics;

pub use affine_oracle_method_comparison::*;
pub use coarse_fine_consistency::*;
pub use coarse_seeded_tick_consistency::*;
pub use contracts::*;
pub use deterministic_regression::*;
pub use open_closed_gap_leakage::*;
pub use performance_baseline::*;
pub use regression_drift_attribution::*;
pub use scale_consistency::*;
pub use stylized_facts::*;
pub use sufficient_statistics::*;
