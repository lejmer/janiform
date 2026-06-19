use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
    evolve_mean_reversion_boundary_state_with_endogenous_pressure,
    evolve_mean_reversion_pressure_state,
};

use super::failure::observed_pressure_replay_failure;
use crate::stages::mean_reversion::{
    MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure,
    kappa_from_phi_reference,
};

// Held-out prediction has no access to the next observed transition, so the
// causal pressure forecast carries only the current pressure state forward.
const OBSERVED_PRESSURE_REPLAY_FORECAST_CORE_EXPECTED_INCREMENT: f64 = 0.0;

// The replay state is seeded from the first retained observation as a flat
// boundary. Subsequent observed transitions identify any pressure state.
const OBSERVED_PRESSURE_REPLAY_INITIAL_PRESSURE_STATE: f64 = 0.0;
const OBSERVED_PRESSURE_REPLAY_INITIAL_DEVIATION: f64 = 0.0;

pub(super) struct ObservedPressureGainFitTerms {
    pub(super) response: Vec<f64>,
    pub(super) pressure_column: Vec<f64>,
}

pub(super) struct ObservedPressureReplayPath {
    pub(super) pressure_state_path: Vec<f64>,
    pub(super) boundary_anchor_values: Vec<f64>,
    pub(super) deviations: Vec<f64>,
    pub(super) residuals: Vec<f64>,
}

pub(in crate::stages::mean_reversion) fn observed_pressure_replay_boundary_state_at_candidate_end(
    candidate: &MeanReversionStageCandidateEvaluation,
    current_observation: f64,
) -> Result<MeanReversionBoundaryState, MeanReversionStageExecutionFailure> {
    let Some(anchor_value) = candidate.boundary_anchor_values.last().copied() else {
        return observed_pressure_replay_failure(
            "observed pressure replay candidate has no boundary anchor values",
        );
    };

    let Some(pressure_state) = candidate.pressure_state_path.last().copied() else {
        return observed_pressure_replay_failure(
            "observed pressure replay candidate has no pressure state path",
        );
    };

    let deviation = current_observation - anchor_value;

    MeanReversionBoundaryState::try_new(
        current_observation,
        anchor_value,
        pressure_state,
        deviation,
    )
    .map_err(|source| {
        MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
            error_message: format!(
                "observed pressure replay candidate boundary state is invalid: {source}"
            ),
        }
    })
}

pub(in crate::stages::mean_reversion) fn predict_causal_observed_pressure_replay_next_observation(
    boundary_state: &MeanReversionBoundaryState,
    candidate: &MeanReversionStageCandidateEvaluation,
    interval_delta: f64,
) -> Result<f64, MeanReversionStageExecutionFailure> {
    predict_observed_pressure_replay_next_observation(
        boundary_state,
        &candidate.anchor_model,
        candidate.phi_reference,
        interval_delta,
    )
}

pub(in crate::stages::mean_reversion) fn update_causal_observed_pressure_replay_boundary_state(
    boundary_state: MeanReversionBoundaryState,
    candidate: &MeanReversionStageCandidateEvaluation,
    interval_delta: f64,
    next_observation: f64,
) -> Result<MeanReversionBoundaryState, MeanReversionStageExecutionFailure> {
    // Copy the small anchor model into fitted parameters because the kernel contract owns it.
    let fitted_parameters =
        MeanReversionFittedParameters::try_new(candidate.kappa, candidate.anchor_model.clone())
            .map_err(|source| {
                MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                    error_message: format!(
                        "observed pressure replay candidate parameters are invalid: {source}"
                    ),
                }
            })?;

    update_observed_pressure_replay_boundary_state(
        boundary_state,
        &fitted_parameters,
        candidate.phi_reference,
        interval_delta,
        next_observation,
    )
}

pub(super) fn observed_pressure_gain_fit_terms(
    observation_levels: &[f64],
    interval_deltas: &[f64],
    zero_gain_anchor_model: &MeanReversionAnchorModel,
    phi_reference: f64,
) -> Result<ObservedPressureGainFitTerms, MeanReversionStageExecutionFailure> {
    let mut boundary_state = initial_observed_pressure_boundary_state(observation_levels)?;
    let mut response = Vec::with_capacity(interval_deltas.len());
    let mut pressure_column = Vec::with_capacity(interval_deltas.len());

    // Copy the zero-gain anchor model into fitted parameters because the kernel contract owns it.
    let fitted_parameters = MeanReversionFittedParameters::try_new(
        kappa_from_phi_reference(
            phi_reference,
            zero_gain_anchor_model.reference_interval_hours,
        ),
        zero_gain_anchor_model.clone(),
    )
    .map_err(|source| {
        MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
            error_message: format!(
                "observed pressure replay zero-gain parameters are invalid: {source}"
            ),
        }
    })?;

    for (&next_observation, &interval_delta) in observation_levels
        .iter()
        .skip(1)
        .zip(interval_deltas.iter())
    {
        let phi = interval_phi(
            phi_reference,
            interval_delta,
            zero_gain_anchor_model.reference_interval_hours,
        );

        let forecast_pressure_state = forecast_observed_pressure_state(
            zero_gain_anchor_model,
            boundary_state.pressure_state,
            interval_delta,
        )?;

        let baseline_prediction = phi.mul_add(
            boundary_state.observed_log_price,
            (1.0 - phi) * boundary_state.anchor_value,
        );

        response.push(next_observation - baseline_prediction);
        pressure_column.push(forecast_pressure_state - phi * boundary_state.pressure_state);

        boundary_state = update_observed_pressure_replay_boundary_state(
            boundary_state,
            &fitted_parameters,
            phi_reference,
            interval_delta,
            next_observation,
        )?;
    }

    Ok(ObservedPressureGainFitTerms {
        response,
        pressure_column,
    })
}

pub(super) fn observed_pressure_replay_path(
    observation_levels: &[f64],
    interval_deltas: &[f64],
    anchor_model: &MeanReversionAnchorModel,
    phi_reference: f64,
) -> Result<ObservedPressureReplayPath, MeanReversionStageExecutionFailure> {
    let mut boundary_state = initial_observed_pressure_boundary_state(observation_levels)?;

    // Copy the anchor model into fitted parameters because the kernel contract owns it.
    let fitted_parameters = MeanReversionFittedParameters::try_new(
        kappa_from_phi_reference(phi_reference, anchor_model.reference_interval_hours),
        anchor_model.clone(),
    )
    .map_err(|source| {
        MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
            error_message: format!("observed pressure replay parameters are invalid: {source}"),
        }
    })?;

    let mut pressure_state_path = Vec::with_capacity(observation_levels.len());
    let mut boundary_anchor_values = Vec::with_capacity(observation_levels.len());
    let mut deviations = Vec::with_capacity(observation_levels.len());
    let mut residuals = Vec::with_capacity(interval_deltas.len());

    pressure_state_path.push(boundary_state.pressure_state);
    boundary_anchor_values.push(boundary_state.anchor_value);
    deviations.push(boundary_state.deviation);

    for (&next_observation, &interval_delta) in observation_levels
        .iter()
        .skip(1)
        .zip(interval_deltas.iter())
    {
        let prediction = predict_observed_pressure_replay_next_observation(
            &boundary_state,
            anchor_model,
            phi_reference,
            interval_delta,
        )?;

        residuals.push(next_observation - prediction);

        boundary_state = update_observed_pressure_replay_boundary_state(
            boundary_state,
            &fitted_parameters,
            phi_reference,
            interval_delta,
            next_observation,
        )?;

        pressure_state_path.push(boundary_state.pressure_state);
        boundary_anchor_values.push(boundary_state.anchor_value);
        deviations.push(boundary_state.deviation);
    }

    Ok(ObservedPressureReplayPath {
        pressure_state_path,
        boundary_anchor_values,
        deviations,
        residuals,
    })
}

fn predict_observed_pressure_replay_next_observation(
    boundary_state: &MeanReversionBoundaryState,
    anchor_model: &MeanReversionAnchorModel,
    phi_reference: f64,
    interval_delta: f64,
) -> Result<f64, MeanReversionStageExecutionFailure> {
    let phi = interval_phi(
        phi_reference,
        interval_delta,
        anchor_model.reference_interval_hours,
    );

    let forecast_pressure_state = forecast_observed_pressure_state(
        anchor_model,
        boundary_state.pressure_state,
        interval_delta,
    )?;

    Ok(phi.mul_add(
        boundary_state.observed_log_price,
        (1.0 - phi) * boundary_state.anchor_value,
    ) + anchor_model.pressure_level_gain
        * (forecast_pressure_state - phi * boundary_state.pressure_state))
}

fn forecast_observed_pressure_state(
    anchor_model: &MeanReversionAnchorModel,
    pressure_state: f64,
    interval_delta: f64,
) -> Result<f64, MeanReversionStageExecutionFailure> {
    evolve_mean_reversion_pressure_state(
        anchor_model,
        pressure_state,
        OBSERVED_PRESSURE_REPLAY_FORECAST_CORE_EXPECTED_INCREMENT,
        interval_delta,
    )
    .map_err(|source| {
        MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
            error_message: format!("observed pressure replay pressure forecast failed: {source}"),
        }
    })
}

fn update_observed_pressure_replay_boundary_state(
    boundary_state: MeanReversionBoundaryState,
    fitted_parameters: &MeanReversionFittedParameters,
    phi_reference: f64,
    interval_delta: f64,
    next_observation: f64,
) -> Result<MeanReversionBoundaryState, MeanReversionStageExecutionFailure> {
    let realized_core_total_return = infer_realized_core_total_return(
        &boundary_state,
        fitted_parameters.anchor_model.reference_interval_hours,
        phi_reference,
        interval_delta,
        next_observation,
    );

    evolve_mean_reversion_boundary_state_with_endogenous_pressure(
        boundary_state,
        fitted_parameters,
        interval_delta,
        realized_core_total_return,
        realized_core_total_return,
    )
    .map_err(|source| {
        MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
            error_message: format!("observed pressure replay boundary update failed: {source}"),
        }
    })
}

fn infer_realized_core_total_return(
    boundary_state: &MeanReversionBoundaryState,
    reference_interval_hours: f64,
    phi_reference: f64,
    interval_delta: f64,
    next_observation: f64,
) -> f64 {
    let phi = interval_phi(phi_reference, interval_delta, reference_interval_hours);

    next_observation
        - phi * boundary_state.observed_log_price
        - (1.0 - phi) * boundary_state.anchor_value
}

fn initial_observed_pressure_boundary_state(
    observation_levels: &[f64],
) -> Result<MeanReversionBoundaryState, MeanReversionStageExecutionFailure> {
    let Some(initial_observation) = observation_levels.first().copied() else {
        return observed_pressure_replay_failure(
            "observed pressure replay requires an initial observation",
        );
    };

    MeanReversionBoundaryState::try_new(
        initial_observation,
        initial_observation,
        OBSERVED_PRESSURE_REPLAY_INITIAL_PRESSURE_STATE,
        OBSERVED_PRESSURE_REPLAY_INITIAL_DEVIATION,
    )
    .map_err(|source| {
        MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
            error_message: format!(
                "observed pressure replay initial boundary state is invalid: {source}"
            ),
        }
    })
}

fn interval_phi(phi_reference: f64, interval_delta: f64, reference_interval_hours: f64) -> f64 {
    phi_reference.powf(interval_delta / reference_interval_hours)
}
