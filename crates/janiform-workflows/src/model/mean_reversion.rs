use janiform::model::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, materialize_endogenous_anchor_path,
};
use janiform::model::factors::hourly::HourlyFactorContractOutput;
use janiform::model::hawkes::HawkesExcitationState;
use janiform::simulation::{
    MeanReversionBoundaryState, MeanReversionRuntimeIntervalAnchors,
    materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments,
};

use crate::error::WorkflowError;

use super::context::{FittedModelContext, validation_error};

const DEFAULT_MEAN_REVERSION_START_PRESSURE_STATE: f64 = 0.0;
const DEFAULT_INITIAL_MEAN_REVERSION_DEVIATION: f64 = 0.0;

pub(crate) fn materialize_mean_reversion_runtime_inputs(
    context: &FittedModelContext,
    hourly_factor_contract: &HourlyFactorContractOutput,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: f64,
    interval_delta: f64,
    interval_count: usize,
) -> Result<
    (
        MeanReversionRuntimeIntervalAnchors,
        MeanReversionBoundaryState,
    ),
    WorkflowError,
> {
    let interval_factors =
        janiform_calibration_runtime::endogenous_interval_factors_from_hourly_contract(
            hourly_factor_contract,
        )
        .map_err(|source| validation_error("hourly_factor_contract", source))?;

    let interval_alignment_policy = hourly_factor_contract.metadata.interval_alignment_policy;

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
        interval_factors.as_slice(),
    )
    .map_err(|source| validation_error("mean_reversion_runtime", source.to_string()))?;

    let core_expected_increments = anchor_materialization
        .interval_expectations
        .iter()
        .map(|expectation| expectation.core_expected_increment)
        .collect::<Vec<_>>();

    let interval_deltas = vec![interval_delta; interval_count];

    let runtime_interval_anchors =
        materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments(
            interval_alignment_policy,
            start_anchor_value,
            DEFAULT_MEAN_REVERSION_START_PRESSURE_STATE,
            &context
                .fitted_parameters
                .return_components_fitted_parameters
                .mean_reversion_fitted_parameters
                .anchor_model,
            core_expected_increments.as_slice(),
            interval_deltas.as_slice(),
        )
        .map_err(|source| validation_error("mean_reversion_runtime", source.to_string()))?;

    let initial_boundary_state = MeanReversionBoundaryState::try_new(
        runtime_interval_anchors.start_anchor_value,
        runtime_interval_anchors.start_anchor_value,
        runtime_interval_anchors.start_pressure_state,
        DEFAULT_INITIAL_MEAN_REVERSION_DEVIATION,
    )
    .map_err(|source| validation_error("mean_reversion_boundary_state", source.to_string()))?;

    Ok((runtime_interval_anchors, initial_boundary_state))
}
