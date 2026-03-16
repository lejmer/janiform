//! Validation and regression harness domains for Janiform.

#![forbid(unsafe_code)]

mod affine_oracle_method_comparison;
mod coarse_fine_consistency;
mod coarse_seeded_tick_consistency;
mod contracts;
mod open_closed_gap_leakage;
mod scale_consistency;
mod sufficient_statistics;

pub use affine_oracle_method_comparison::*;
pub use coarse_fine_consistency::*;
pub use coarse_seeded_tick_consistency::*;
pub use contracts::*;
pub use janiform_analytics_stylized_facts::{
    NewsSemanticsScenarioClass, OpenClosedGapDeterministicReplayMetadata,
    OpenClosedGapIssueTraceabilityMetadata,
};
pub use open_closed_gap_leakage::*;
pub use scale_consistency::*;
pub use sufficient_statistics::*;
