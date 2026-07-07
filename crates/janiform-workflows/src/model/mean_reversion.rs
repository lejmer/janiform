use janiform::model::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, materialize_endogenous_anchor_path,
};
use janiform::model::factors::interval_factor_grid::IntervalFactorGrid;
use janiform::model::hawkes::HawkesExcitationState;
use janiform::simulation::{MeanReversionBoundaryState, MeanReversionRuntimeIntervalAnchors};

use crate::error::WorkflowError;

use super::context::{FittedModelContext, validation_error};

const DEFAULT_INITIAL_MEAN_REVERSION_DEVIATION: f64 = 0.0;

pub(crate) fn materialize_mean_reversion_runtime_inputs(
    context: &FittedModelContext,
    interval_factor_grid: &IntervalFactorGrid,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: f64,
) -> Result<
    (
        MeanReversionRuntimeIntervalAnchors,
        MeanReversionBoundaryState,
    ),
    WorkflowError,
> {
    let interval_factor_grids =
        janiform_calibration_runtime::endogenous_interval_factor_grids_from_interval_factor_grid(
            interval_factor_grid,
        )
        .map_err(|source| validation_error("interval_factor_grid", source))?;

    let interval_alignment_policy = interval_factor_grid.metadata.interval_alignment_policy;

    let anchor_materialization = materialize_endogenous_anchor_path(
        interval_alignment_policy,
        start_anchor_value,
        ExponentialAffineTransformEquationSpecification::try_new(
            context
                .fitted_parameters
                .hawkes_fitted_parameters
                .kernel_parameters,
            context
                .fitted_parameters
                .return_components_fitted_parameters
                .news_process_configuration
                .shared_mark_law
                .gamma_severity_marks,
            context
                .fitted_parameters
                .return_components_fitted_parameters
                .long_variance_parameters,
            // Anchor materialization owns the pressure contract through the affine spec.
            context
                .fitted_parameters
                .return_components_fitted_parameters
                .market_pressure_parameters
                .clone(),
        )
        .map_err(|source| validation_error("equation_specification", source.to_string()))?,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        &context
            .fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration,
        initial_excitation_state,
        interval_factor_grids.as_slice(),
    )
    .map_err(|source| validation_error("mean_reversion_runtime", source.to_string()))?;

    let runtime_interval_anchors = anchor_materialization.runtime_interval_anchors;

    let initial_boundary_state = MeanReversionBoundaryState::try_new(
        runtime_interval_anchors.start_anchor_value,
        runtime_interval_anchors.start_anchor_value,
        DEFAULT_INITIAL_MEAN_REVERSION_DEVIATION,
    )
    .map_err(|source| validation_error("mean_reversion_boundary_state", source.to_string()))?;

    Ok((runtime_interval_anchors, initial_boundary_state))
}
