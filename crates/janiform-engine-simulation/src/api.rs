//! Public simulation adapters bridging shared factor, Hawkes, and NIG APIs.

use janiform_foundations::randomness::SimulationRandomnessStream;
use janiform_market_time::interval_factor_grids::IntervalFactorGrid;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameters,
    HawkesSeasonalityIntervalError,
};
use janiform_model_kernels::nig::{
    NigBuySellDriverParameters, NigTradingTimeIncrementContributions,
    NigTradingTimeIncrementSamplingError,
};
use janiform_model_kernels::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode,
};

/// Resolve interval-aligned interval factor grids for the simulation path.
pub use janiform_market_time::interval_factor_grids::provide_interval_factor_grid as load_interval_factor_grid;

/// Project state to interval intensities for the fine-scale simulation path.
pub fn project_hawkes_intensities_simulation_interval(
    state: HawkesExcitationState,
    parameters: HawkesKernelParameters,
    interval_factor_grid: &IntervalFactorGrid,
    interval_index: usize,
) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
    state.project_intensities_for_hourly_interval(
        parameters.lambda_bar,
        &interval_factor_grid.seasonality_factor,
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
    sample_nig_trading_time_increment_with_edge_mode(
        driver_parameters,
        return_decomposition,
        ReturnDecompositionEdgeMode::Disabled,
        randomness_stream,
    )
}

/// Sample one simulation trading-time increment with optional decomposition edge-mode gating.
pub use janiform_model_kernels::return_decomposition::sample_nig_trading_time_increment_with_edge_mode;

/// Apply one simulation-path Hawkes state transition using canonical call order.
pub use janiform_model_kernels::hawkes::evolve_hawkes_state;
