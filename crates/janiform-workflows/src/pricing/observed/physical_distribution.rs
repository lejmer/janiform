mod collector;
mod compute;
mod nodes;
mod sampling;
mod types;

pub(super) use compute::compute_physical_distribution_diagnostics_if_requested;
pub use types::{
    ObservedOptionsPhysicalDistributionDiagnosticsDefaults,
    ObservedOptionsPhysicalDistributionDiagnosticsNode,
    ObservedOptionsPhysicalDistributionDiagnosticsOutput,
    ObservedOptionsPhysicalDistributionDiagnosticsReport,
    ObservedOptionsPhysicalDistributionDiagnosticsRequest,
};
