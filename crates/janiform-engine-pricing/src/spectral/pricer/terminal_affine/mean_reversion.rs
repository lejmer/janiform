use janiform_affine_oracle::endogenous_anchor::{
    EndogenousAnchorIntervalFactors, derive_expected_interval_projection_from_first_moment_dynamics,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::evolve_mean_reversion_deviation_with_decay_on_interval_grid;

use crate::characteristic::build_pricing_equation_specification;

use super::super::{
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingRequest, VanillaOptionPricingRunError,
};

#[derive(Debug, Clone)]
pub(super) struct TerminalAffineMeanReversionPath {
    pub(super) terminal_anchor_value: f64,
    pub(super) terminal_deterministic_log_price: f64,
    pub(super) terminal_deterministic_log_return: f64,
    pub(super) anchor_span: f64,
    pub(super) interval_return_weights: Vec<f64>,
    pub(super) interval_anchor_values_before: Vec<f64>,
    pub(super) interval_anchor_values_after: Vec<f64>,
    pub(super) interval_hawkes_states_before: Vec<HawkesExcitationState>,
    pub(super) interval_hawkes_states_after: Vec<HawkesExcitationState>,
}

#[derive(Debug, Clone, Copy)]
struct TerminalAffinePhysicalProjection {
    structural_state_after: HawkesExcitationState,
}

pub(super) fn build_terminal_affine_mean_reversion_path(
    pricing_request: &VanillaOptionPricingRequest,
    maturity_count: usize,
    interval_deltas: &[f64],
    interval_factor_grids: &[VanillaOptionPricingIntervalFactors],
) -> Result<TerminalAffineMeanReversionPath, VanillaOptionPricingRunError> {
    let boundary_input = &pricing_request.boundary_input;
    let observed_boundary_state = &boundary_input.observed_boundary_state;
    let mean_reversion_parameters = &boundary_input.mean_reversion_fitted_parameters;

    let mut current_anchor_value = observed_boundary_state.anchor_value;
    let mut current_deviation = observed_boundary_state.deviation;
    let mut current_hawkes_state = boundary_input.hawkes_boundary_state;

    let mut interval_decay_factors = Vec::with_capacity(maturity_count);
    let mut interval_anchor_values_before = Vec::with_capacity(maturity_count);
    let mut interval_anchor_values_after = Vec::with_capacity(maturity_count);
    let mut interval_hawkes_states_before = Vec::with_capacity(maturity_count);
    let mut interval_hawkes_states_after = Vec::with_capacity(maturity_count);
    let mut zero_return_log_price_points = Vec::with_capacity(maturity_count + 1);

    zero_return_log_price_points.push(observed_boundary_state.observed_log_price);

    for interval_offset in 0..maturity_count {
        let interval_delta = interval_deltas[interval_offset];
        let interval_decay_factor = (-mean_reversion_parameters.kappa * interval_delta).exp();

        if !interval_decay_factor.is_finite() || interval_decay_factor < 0.0 {
            return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
                message: format!(
                    "terminal affine mean-reversion decay is invalid for interval_offset={interval_offset}"
                ),
            });
        }

        let physical_projection = derive_terminal_affine_physical_projection(
            pricing_request,
            interval_factor_grids[interval_offset],
            boundary_input.interval_index + interval_offset,
            current_hawkes_state,
            interval_delta,
        )?;

        let next_anchor_value = current_anchor_value;

        let next_zero_return_deviation =
            evolve_mean_reversion_deviation_with_decay_on_interval_grid(
                current_deviation,
                interval_decay_factor,
                0.0,
                current_anchor_value,
                next_anchor_value,
            )
            .map_err(|source| {
                VanillaOptionPricingRunError::InternalNumericsFailure {
                    message: format!(
                        "terminal affine zero-return deviation evolution failed: {source}"
                    ),
                }
            })?;

        interval_decay_factors.push(interval_decay_factor);
        interval_anchor_values_before.push(current_anchor_value);
        interval_anchor_values_after.push(next_anchor_value);
        interval_hawkes_states_before.push(current_hawkes_state);
        interval_hawkes_states_after.push(physical_projection.structural_state_after);

        current_anchor_value = next_anchor_value;
        current_deviation = next_zero_return_deviation;
        current_hawkes_state = physical_projection.structural_state_after;

        zero_return_log_price_points.push(current_anchor_value + current_deviation);
    }

    let interval_return_weights =
        terminal_return_weights_from_decay_factors(interval_decay_factors.as_slice())?;

    let terminal_deterministic_log_price = current_anchor_value + current_deviation;
    let terminal_deterministic_log_return =
        terminal_deterministic_log_price - observed_boundary_state.observed_log_price;

    let anchor_span = log_price_path_span_from_reference(
        observed_boundary_state.observed_log_price,
        zero_return_log_price_points.as_slice(),
    )?;

    Ok(TerminalAffineMeanReversionPath {
        terminal_anchor_value: current_anchor_value,
        terminal_deterministic_log_price,
        terminal_deterministic_log_return,
        anchor_span,
        interval_return_weights,
        interval_anchor_values_before,
        interval_anchor_values_after,
        interval_hawkes_states_before,
        interval_hawkes_states_after,
    })
}

fn terminal_return_weights_from_decay_factors(
    interval_decay_factors: &[f64],
) -> Result<Vec<f64>, VanillaOptionPricingRunError> {
    let mut interval_return_weights = vec![1.0; interval_decay_factors.len()];
    let mut future_decay_weight = 1.0_f64;

    for (interval_offset, interval_decay_factor) in interval_decay_factors.iter().enumerate().rev()
    {
        if !future_decay_weight.is_finite() {
            return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
                message: format!(
                    "terminal affine return-weight product is non-finite at interval_offset={interval_offset}"
                ),
            });
        }

        interval_return_weights[interval_offset] = future_decay_weight;
        future_decay_weight *= *interval_decay_factor;
    }

    Ok(interval_return_weights)
}

fn log_price_path_span_from_reference(
    reference_log_price: f64,
    log_price_points: &[f64],
) -> Result<f64, VanillaOptionPricingRunError> {
    let mut span = 0.0;

    for log_price_point in log_price_points {
        if !log_price_point.is_finite() {
            return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
                message: "terminal affine deterministic log-price path is non-finite".to_string(),
            });
        }

        let distance = (*log_price_point - reference_log_price).abs();

        span = f64::max(span, distance);
    }

    Ok(span)
}

fn derive_terminal_affine_physical_projection(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factor_grids: VanillaOptionPricingIntervalFactors,
    interval_index: usize,
    hawkes_boundary_state: HawkesExcitationState,
    interval_delta: f64,
) -> Result<TerminalAffinePhysicalProjection, VanillaOptionPricingRunError> {
    let endogenous_factors = EndogenousAnchorIntervalFactors::try_new(
        interval_factor_grids.seasonality_factor,
        interval_factor_grids.known_news_intensity,
        interval_factor_grids.surprise_news_intensity,
        interval_delta,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!("terminal affine endogenous interval factors are invalid: {source}"),
        },
    )?;

    let expected_severity_mark = pricing_request
        .model_parameters
        .news_process_configuration
        .shared_mark_law
        .gamma_severity_marks
        .expected_severity_mark()
        .map_err(
            |source| VanillaOptionPricingRunError::InternalNumericsFailure {
                message: format!(
                    "terminal affine expected news severity mark is invalid: {source}"
                ),
            },
        )?;

    let projection = derive_expected_interval_projection_from_first_moment_dynamics(
        interval_index,
        &build_pricing_equation_specification(pricing_request),
        expected_severity_mark,
        endogenous_factors,
        hawkes_boundary_state,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!("terminal affine physical interval projection failed: {source}"),
        },
    )?;

    Ok(TerminalAffinePhysicalProjection {
        structural_state_after: projection.structural_state_after,
    })
}

#[cfg(test)]
mod tests {
    use super::terminal_return_weights_from_decay_factors;

    #[test]
    fn terminal_return_weights_apply_future_mean_reversion_decay() {
        let observed = terminal_return_weights_from_decay_factors(&[0.5, 0.25, 0.125])
            .expect("finite decay factors should resolve");

        assert_eq!(observed, vec![0.25 * 0.125, 0.125, 1.0]);
    }
}
