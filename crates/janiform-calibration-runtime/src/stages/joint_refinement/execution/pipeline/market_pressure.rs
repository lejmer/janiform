use std::sync::mpsc::Sender;

use crate::market_pressure::{
    MarketPressureFitProgress, MarketPressureNigFitInput, MarketPressureRuntimeState,
    fit_market_pressure_from_nig_configuration, market_pressure_parameters_from_runtime_state,
};
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use janiform_calibration_artifacts::CalibrationRunSpec;
use janiform_model_kernels::market_pressure::MarketPressureParameters;

use super::super::JointRefinementCandidateOutcome;
use super::progress::emit_joint_refinement_subphase_progress;
use super::types::{CandidateSearchArtifacts, JointRefinementPipelineStage, ResolvedStagePayloads};

pub(super) fn refit_market_pressure_after_candidate_selection(
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    candidate_search_artifacts: &mut CandidateSearchArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<(), JointRefinementStageExecutionFailure> {
    let market_pressure_parameters = refit_joint_market_pressure_parameters(
        stage_input,
        resolved_stage_payloads,
        candidate_search_artifacts,
        progress_sender,
    )?;

    apply_market_pressure_parameters_to_candidate_outcome(
        &mut candidate_search_artifacts.best_candidate,
        &market_pressure_parameters,
    );

    let selected_candidate_id = candidate_search_artifacts
        .best_candidate
        .candidate_id
        .as_str();

    for candidate_outcome in &mut candidate_search_artifacts.candidate_outcomes {
        if candidate_outcome.candidate_id == selected_candidate_id {
            apply_market_pressure_parameters_to_candidate_outcome(
                candidate_outcome,
                &market_pressure_parameters,
            );
        }
    }

    for candidate_outcome in &mut candidate_search_artifacts.option_refinement_candidate_frontier {
        if candidate_outcome.candidate_id == selected_candidate_id {
            apply_market_pressure_parameters_to_candidate_outcome(
                candidate_outcome,
                &market_pressure_parameters,
            );
        }
    }

    Ok(())
}

fn refit_joint_market_pressure_parameters(
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    candidate_search_artifacts: &CandidateSearchArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<MarketPressureParameters, JointRefinementStageExecutionFailure> {
    if let Some(runtime_state) = resolved_stage_payloads
        .return_components_payload
        .market_pressure_runtime_state
        .as_ref()
        .filter(|runtime_state| {
            market_pressure_runtime_state_covers_joint_input(
                runtime_state,
                stage_input.coarse_data_records.as_deref(),
            )
        })
    {
        emit_joint_refinement_subphase_progress(
            progress_sender,
            JointRefinementStageProgressPhase::MarketPressureRefit,
            1,
            1,
            "apply fitted pressure state",
        );

        return market_pressure_parameters_from_runtime_state(runtime_state).map_err(|source| {
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!("market pressure candidate refit failed: {source}"),
            }
        });
    }

    emit_joint_refinement_subphase_progress(
        progress_sender,
        JointRefinementStageProgressPhase::MarketPressureRefit,
        0,
        1,
        "fit market pressure parameters",
    );

    let market_pressure_progress_sender = progress_sender.cloned();

    let mut market_pressure_progress_observer = move |progress: MarketPressureFitProgress| {
        emit_joint_refinement_subphase_progress(
            market_pressure_progress_sender.as_ref(),
            JointRefinementStageProgressPhase::MarketPressureRefit,
            progress.completed_units,
            progress.total_units,
            progress.current_item_label,
        );
    };

    fit_market_pressure_from_nig_configuration(MarketPressureNigFitInput {
        observed_open_session_returns: stage_input
            .candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        coarse_data_records: stage_input.coarse_data_records.as_deref(),
        nig_configuration: candidate_search_artifacts
            .best_candidate
            .final_evaluation
            .fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        source_window_open_exposure_hours: resolved_stage_payloads
            .return_components_payload
            .source_window_open_exposure_hours,
        solver_settings: &resolved_stage_payloads.stage_solver_settings,
        progress_observer: Some(&mut market_pressure_progress_observer),
    })
    .map(|output| output.parameters)
    .map_err(
        |source| JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: format!("market pressure candidate refit failed: {source}"),
        },
    )
}

fn market_pressure_runtime_state_covers_joint_input(
    runtime_state: &MarketPressureRuntimeState,
    coarse_data_records: Option<
        &[crate::market_data::loading::MarketDataNormalizedCoarseDataRecord],
    >,
) -> bool {
    runtime_state
        .covers_coarse_return_record_count(coarse_data_records.map_or(0, |records| records.len()))
}

fn apply_market_pressure_parameters_to_candidate_outcome(
    candidate_outcome: &mut JointRefinementCandidateOutcome,
    market_pressure_parameters: &janiform_model_kernels::market_pressure::MarketPressureParameters,
) {
    // Local and final evaluations own separate fitted-parameter snapshots.
    candidate_outcome
        .local_evaluation
        .fitted_parameters
        .return_components_fitted_parameters
        .market_pressure_parameters = market_pressure_parameters.clone();

    candidate_outcome
        .final_evaluation
        .fitted_parameters
        .return_components_fitted_parameters
        .market_pressure_parameters = market_pressure_parameters.clone();
}

pub(super) fn should_refit_market_pressure_after_candidate_selection(
    pipeline_stage: JointRefinementPipelineStage,
    _run_spec: &CalibrationRunSpec,
    _stage_input: &JointRefinementStageExecutionInput,
) -> bool {
    match pipeline_stage {
        JointRefinementPipelineStage::JointRefinement => true,
        JointRefinementPipelineStage::OptionRefinement
        | JointRefinementPipelineStage::Finalization => false,
    }
}
