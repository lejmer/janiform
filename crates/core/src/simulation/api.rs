//! Public simulation adapters bridging shared factor, Hawkes, and NIG APIs.

use crate::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameters,
    HawkesSeasonalityIntervalError,
};
use crate::hourly_factors::HourlyFactorContractOutput;
use crate::nig::{
    NigBuySellDriverParameters, NigTradingTimeIncrementContributions,
    NigTradingTimeIncrementSamplingError,
};
use crate::randomness::SimulationRandomnessStream;
use crate::return_decomposition::{ReturnDecomposition, ReturnDecompositionEdgeMode};

/// Resolve interval-aligned hourly factors for the simulation path.
pub use crate::hourly_factors::provide_hourly_factor_contract as load_hourly_factor_contract;

/// Project state to interval intensities for the fine-scale simulation path.
pub fn project_hawkes_intensities_simulation_interval(
    state: HawkesExcitationState,
    parameters: HawkesKernelParameters,
    hourly_factor_contract: &HourlyFactorContractOutput,
    interval_index: usize,
) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
    state.project_intensities_for_hourly_interval(
        parameters.lambda_bar,
        hourly_factor_contract,
        interval_index,
    )
}

/// Sample one trading-time return increment for simulation through the shared NIG driver API.
pub fn sample_nig_trading_time_increment<R>(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    randomness_stream: &mut R,
) -> Result<NigTradingTimeIncrementContributions, NigTradingTimeIncrementSamplingError>
where
    R: SimulationRandomnessStream,
{
    crate::return_decomposition::sample_nig_trading_time_increment_with_edge_mode(
        driver_parameters,
        return_decomposition,
        ReturnDecompositionEdgeMode::Disabled,
        randomness_stream,
    )
}

/// Sample one simulation trading-time increment with optional decomposition edge-mode gating.
pub use crate::return_decomposition::sample_nig_trading_time_increment_with_edge_mode;

/// Apply one simulation-path Hawkes state transition using canonical call order.
pub use crate::hawkes::evolve_hawkes_state;
