use super::super::{
    SpectralPricingIntervalDiagnostics, TerminalAffineCumulantStatus,
    TerminalAffineMeasureChangeDiagnostics, TerminalAffineOptionSmileContributionDiagnostics,
    VanillaOptionPricingRequest, VanillaOptionPricingRunError,
};
use super::core::{TerminalAffineMaturityContext, TerminalAffineThetaSolution};
use super::mean_reversion::TerminalAffineMeanReversionPath;

pub(super) fn build_terminal_affine_measure_change_diagnostics(
    context: &TerminalAffineMaturityContext<'_>,
    theta_solution: TerminalAffineThetaSolution,
) -> Result<TerminalAffineMeasureChangeDiagnostics, VanillaOptionPricingRunError> {
    let long_variance_parameters = context
        .pricing_request
        .model_parameters
        .long_variance_parameters;

    let kernel_parameters = context
        .pricing_request
        .model_parameters
        .hawkes_kernel_parameters;

    let stationary_level = long_variance_parameters
        .stationary_mean_reversion_level_for_hawkes_kernel(kernel_parameters)
        .map_err(|source| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "terminal affine diagnostics could not derive long-variance stationary level: {source}"
            ),
        })?;

    let boundary_state = context
        .pricing_request
        .boundary_input
        .long_variance_boundary_state;

    let option_smile_contribution = TerminalAffineOptionSmileContributionDiagnostics::try_new(
        boundary_state.level / stationary_level,
        long_variance_parameters.vol_of_vol,
        long_variance_parameters.buy_event_loading,
        long_variance_parameters.sell_event_loading,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InvalidSpectralDiagnostics {
            source: Box::new(source),
        },
    )?;

    TerminalAffineMeasureChangeDiagnostics::try_new(
        TerminalAffineCumulantStatus::Succeeded,
        theta_solution.theta,
        theta_solution.target_log_moment_exponent,
        theta_solution.observed_log_moment_exponent,
        theta_solution.martingale_residual,
        theta_solution.martingale_tolerance,
        boundary_state,
        stationary_level,
        option_smile_contribution,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InvalidSpectralDiagnostics {
            source: Box::new(source),
        },
    )
}

pub(super) fn build_terminal_affine_interval_summaries(
    pricing_request: &VanillaOptionPricingRequest,
    interval_deltas: &[f64],
    maturity_count: usize,
    theta: f64,
    mean_reversion_path: &TerminalAffineMeanReversionPath,
) -> Vec<SpectralPricingIntervalDiagnostics> {
    (0..maturity_count)
        .map(|interval_offset| SpectralPricingIntervalDiagnostics {
            interval_index: pricing_request.boundary_input.interval_index + interval_offset,
            current_anchor_value: mean_reversion_path.interval_anchor_values_before
                [interval_offset],
            next_anchor_value: mean_reversion_path.interval_anchor_values_after[interval_offset],
            interval_delta: interval_deltas[interval_offset],
            expected_hawkes_boundary_state_before: mean_reversion_path
                .interval_hawkes_states_before[interval_offset],
            expected_hawkes_boundary_state_after: mean_reversion_path.interval_hawkes_states_after
                [interval_offset],
            minimum_solved_theta: theta,
            maximum_solved_theta: theta,
        })
        .collect()
}
