use janiform_market_time::interval_factor_grids::IntervalFactorGrid;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameters,
    HawkesSeasonalityIntervalError,
};

/// Project state to interval intensities for the fine-scale simulation path.
pub fn project_hawkes_intensities_simulation_interval(
    state: HawkesExcitationState,
    parameters: HawkesKernelParameters,
    interval_factor_grid: &IntervalFactorGrid,
    interval_index: usize,
) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
    state.project_intensities_for_interval_factor(
        parameters.lambda_bar,
        &interval_factor_grid.seasonality_factor,
        interval_index,
    )
}
