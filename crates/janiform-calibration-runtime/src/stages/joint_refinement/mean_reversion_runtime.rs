use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_affine_oracle::endogenous_anchor::{
    EndogenousAnchorIntervalFactors, materialize_endogenous_anchor_path,
};
use janiform_calibration_artifacts::JointRefinementStageFittedParameters;
use janiform_foundations::time::NANOS_PER_HOUR;
use janiform_market_time::hourly_factors::HourlyFactorContractOutput;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionRuntimeIntervalAnchors,
    materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments,
};

/// Convert an hourly factor contract into endogenous anchor interval factors.
pub fn endogenous_interval_factors_from_hourly_contract(
    hourly_factor_contract: &HourlyFactorContractOutput,
) -> Result<Vec<EndogenousAnchorIntervalFactors>, String> {
    let interval_count = hourly_factor_contract.interval_indices.len();
    if hourly_factor_contract.s_h.len() != interval_count
        || hourly_factor_contract.known_component_nu_h.len() != interval_count
        || hourly_factor_contract.surprise_component_nu_h.len() != interval_count
    {
        return Err(
            "joint refinement hourly factor contract has inconsistent interval lengths".to_string(),
        );
    }

    let interval_delta =
        hourly_factor_contract.interval_duration_nanos as f64 / NANOS_PER_HOUR as f64;

    hourly_factor_contract
        .s_h
        .iter()
        .zip(hourly_factor_contract.known_component_nu_h.iter())
        .zip(hourly_factor_contract.surprise_component_nu_h.iter())
        .map(
            |((seasonality_factor, known_intensity), surprise_intensity)| {
                EndogenousAnchorIntervalFactors::try_new(
                    *seasonality_factor,
                    *known_intensity,
                    *surprise_intensity,
                    interval_delta,
                )
                .map_err(|source| source.to_string())
            },
        )
        .collect()
}

pub(super) fn materialize_joint_refinement_mean_reversion_runtime_inputs(
    interval_alignment_policy: &str,
    fitted_parameters: &JointRefinementStageFittedParameters,
    initial_excitation_state: HawkesExcitationState,
    interval_factors: &[EndogenousAnchorIntervalFactors],
) -> Result<
    (
        MeanReversionRuntimeIntervalAnchors,
        MeanReversionBoundaryState,
    ),
    String,
> {
    let anchor_materialization = materialize_endogenous_anchor_path(
        interval_alignment_policy,
        0.0,
        ExponentialAffineTransformEquationSpecification::try_new(
            fitted_parameters.hawkes_fitted_parameters.kernel_parameters,
            fitted_parameters
                .return_components_fitted_parameters
                .news_process_configuration
                .shared_mark_law
                .gamma_severity_marks,
            fitted_parameters
                .return_components_fitted_parameters
                .long_variance_parameters,
        )
        .map_err(|source| source.to_string())?,
        fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        &fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration,
        initial_excitation_state,
        interval_factors,
    )
    .map_err(|source| source.to_string())?;

    let core_expected_increments = anchor_materialization
        .interval_expectations
        .iter()
        .map(|expectation| expectation.core_expected_increment)
        .collect::<Vec<_>>();

    let runtime_interval_anchors =
        materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments(
            interval_alignment_policy,
            0.0,
            0.0,
            &fitted_parameters
                .return_components_fitted_parameters
                .mean_reversion_fitted_parameters
                .anchor_model,
            core_expected_increments.as_slice(),
            anchor_materialization
                .runtime_interval_anchors
                .interval_deltas
                .as_slice(),
        )
        .map_err(|source| source.to_string())?;

    let initial_boundary_state = MeanReversionBoundaryState::try_new(
        runtime_interval_anchors.start_anchor_value,
        runtime_interval_anchors.start_anchor_value,
        runtime_interval_anchors.start_pressure_state,
        0.0,
    )
    .map_err(|source| source.to_string())?;

    Ok((runtime_interval_anchors, initial_boundary_state))
}
