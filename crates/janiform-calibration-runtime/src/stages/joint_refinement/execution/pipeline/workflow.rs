use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Instant;

use janiform_calibration_artifacts::CalibrationRunSpec;
use janiform_foundations::cancellation::CancellationToken;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageExecutionReport, JointRefinementStageProgressUpdate,
};

use super::super::JointRefinementResumeStore;
use super::super::state::JointRefinementStageRuntimeHandoff;
use super::baseline::establish_baseline_evaluation;
use super::final_assembly::{assemble_final_stage_outputs, empty_final_assembly_artifacts};
use super::market_pressure::{
    refit_market_pressure_after_candidate_selection,
    should_refit_market_pressure_after_candidate_selection,
};
use super::option_refinement::{
    CandidateSearchArtifactResolutionInput, resolve_candidate_search_artifacts_for_stage,
    run_audit_option_pricing_if_enabled, run_option_refinement_if_enabled,
    should_run_option_pricing_stage,
};
use super::preparation::{prepare_bootstrap_context, resolve_initial_pipeline_state};
use super::reporting::{
    build_failure_report_with_runtime_profile,
    build_stage_failure_report as build_stage_failure_execution_report, build_success_report,
};
use super::types::{
    BaselineEvaluationArtifacts, CandidateSearchArtifacts, FinalAssemblyArtifacts,
    InitialPipelineState, JointRefinementPipelineStage, PreparedBootstrapContext,
    ResolvedStagePayloads,
};

pub(in crate::stages::joint_refinement::execution) fn run_joint_refinement_calibration_stage_internal(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
    progress_sender: Option<Sender<JointRefinementStageProgressUpdate>>,
    disk_checkpoint_directory: Option<&Path>,
    load_current_stage_resume_state: bool,
    cancellation_token: CancellationToken,
) -> JointRefinementStageExecutionReport {
    run_joint_refinement_pipeline_stage_internal(JointRefinementPipelineStageRunInput {
        run_spec,
        calendar_registry,
        stage_input,
        upstream_joint_refinement_handoff: None,
        upstream_option_refinement_handoff: None,
        progress_sender,
        disk_checkpoint_directory,
        load_current_stage_resume_state,
        cancellation_token,
        pipeline_stage: JointRefinementPipelineStage::JointRefinement,
    })
}

pub(crate) struct OptionRefinementPipelineStageRunInput<'a> {
    pub(crate) run_spec: &'a CalibrationRunSpec,
    pub(crate) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(crate) stage_input: JointRefinementStageExecutionInput,
    pub(crate) upstream_joint_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    pub(crate) progress_sender: Option<Sender<JointRefinementStageProgressUpdate>>,
    pub(crate) disk_checkpoint_directory: Option<&'a Path>,
    pub(crate) load_current_stage_resume_state: bool,
    pub(crate) cancellation_token: CancellationToken,
}

pub(in crate::stages::joint_refinement::execution) fn run_option_refinement_calibration_stage_internal(
    input: OptionRefinementPipelineStageRunInput<'_>,
) -> JointRefinementStageExecutionReport {
    run_joint_refinement_pipeline_stage_internal(JointRefinementPipelineStageRunInput {
        run_spec: input.run_spec,
        calendar_registry: input.calendar_registry,
        stage_input: input.stage_input,
        upstream_joint_refinement_handoff: input.upstream_joint_refinement_handoff,
        upstream_option_refinement_handoff: None,
        progress_sender: input.progress_sender,
        disk_checkpoint_directory: input.disk_checkpoint_directory,
        load_current_stage_resume_state: input.load_current_stage_resume_state,
        cancellation_token: input.cancellation_token,
        pipeline_stage: JointRefinementPipelineStage::OptionRefinement,
    })
}

pub(crate) struct FinalizationPipelineStageRunInput<'a> {
    pub(crate) run_spec: &'a CalibrationRunSpec,
    pub(crate) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(crate) stage_input: JointRefinementStageExecutionInput,
    pub(crate) upstream_joint_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    pub(crate) upstream_option_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    pub(crate) progress_sender: Option<Sender<JointRefinementStageProgressUpdate>>,
    pub(crate) disk_checkpoint_directory: Option<&'a Path>,
    pub(crate) load_current_stage_resume_state: bool,
    pub(crate) cancellation_token: CancellationToken,
}

pub(in crate::stages::joint_refinement::execution) fn run_finalization_calibration_stage_internal(
    input: FinalizationPipelineStageRunInput<'_>,
) -> JointRefinementStageExecutionReport {
    run_joint_refinement_pipeline_stage_internal(JointRefinementPipelineStageRunInput {
        run_spec: input.run_spec,
        calendar_registry: input.calendar_registry,
        stage_input: input.stage_input,
        upstream_joint_refinement_handoff: input.upstream_joint_refinement_handoff,
        upstream_option_refinement_handoff: input.upstream_option_refinement_handoff,
        progress_sender: input.progress_sender,
        disk_checkpoint_directory: input.disk_checkpoint_directory,
        load_current_stage_resume_state: input.load_current_stage_resume_state,
        cancellation_token: input.cancellation_token,
        pipeline_stage: JointRefinementPipelineStage::Finalization,
    })
}

struct JointRefinementPipelineStageRunInput<'a> {
    run_spec: &'a CalibrationRunSpec,
    calendar_registry: &'a ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
    upstream_joint_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    upstream_option_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    progress_sender: Option<Sender<JointRefinementStageProgressUpdate>>,
    disk_checkpoint_directory: Option<&'a Path>,
    load_current_stage_resume_state: bool,
    cancellation_token: CancellationToken,
    pipeline_stage: JointRefinementPipelineStage,
}

struct StagePreparationArtifacts<'a> {
    resolved_stage_payloads: ResolvedStagePayloads<'a>,
    resume_store: JointRefinementResumeStore,
    prepared_bootstrap: PreparedBootstrapContext,
}

struct StagePreparationInput<'a> {
    run_spec: &'a CalibrationRunSpec,
    calendar_registry: &'a ExchangeCalendarRegistry,
    stage_input: &'a JointRefinementStageExecutionInput,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    disk_checkpoint_directory: Option<&'a Path>,
    load_current_stage_resume_state: bool,
    pipeline_stage: JointRefinementPipelineStage,
    cancellation_token: &'a CancellationToken,
}

fn run_joint_refinement_pipeline_stage_internal(
    input: JointRefinementPipelineStageRunInput<'_>,
) -> JointRefinementStageExecutionReport {
    let stage_started_at = Instant::now();
    let run_spec = input.run_spec;
    let calendar_registry = input.calendar_registry;
    let stage_input = input.stage_input;
    let upstream_joint_refinement_handoff = input.upstream_joint_refinement_handoff;
    let upstream_option_refinement_handoff = input.upstream_option_refinement_handoff;
    let progress_sender = input.progress_sender;
    let disk_checkpoint_directory = input.disk_checkpoint_directory;
    let load_current_stage_resume_state = input.load_current_stage_resume_state;
    let cancellation_token = input.cancellation_token;
    let pipeline_stage = input.pipeline_stage;

    if let Err(failure) = validate_pipeline_stage_input(&stage_input, &cancellation_token) {
        return stage_failure_report(run_spec, pipeline_stage, failure);
    }

    let StagePreparationArtifacts {
        resolved_stage_payloads,
        mut resume_store,
        mut prepared_bootstrap,
    } = match prepare_stage_runtime(StagePreparationInput {
        run_spec,
        calendar_registry,
        stage_input: &stage_input,
        progress_sender: progress_sender.as_ref(),
        disk_checkpoint_directory,
        load_current_stage_resume_state,
        pipeline_stage,
        cancellation_token: &cancellation_token,
    }) {
        Ok(artifacts) => artifacts,
        Err(failure) => {
            return stage_failure_report(run_spec, pipeline_stage, failure);
        }
    };

    let baseline_evaluation_artifacts = match establish_baseline_evaluation(
        &resolved_stage_payloads,
        &mut prepared_bootstrap.bootstrap_context,
        prepared_bootstrap.baseline_parameter_vector.as_slice(),
        progress_sender.as_ref(),
    ) {
        Ok(artifacts) => artifacts,
        Err(failure) => {
            return runtime_failure_report(
                run_spec,
                &prepared_bootstrap,
                stage_started_at,
                pipeline_stage,
                failure,
            );
        }
    };

    let candidate_search_artifacts = match run_stage_refinement_steps(StageRefinementStepsInput {
        run_spec,
        stage_input: &stage_input,
        resolved_stage_payloads: &resolved_stage_payloads,
        prepared_bootstrap: &prepared_bootstrap,
        baseline_evaluation_artifacts: &baseline_evaluation_artifacts,
        progress_sender: progress_sender.as_ref(),
        resume_store: &mut resume_store,
        disk_checkpoint_directory,
        pipeline_stage,
        upstream_joint_refinement_handoff,
        upstream_option_refinement_handoff,
    }) {
        Ok(artifacts) => artifacts,
        Err(failure) => {
            return runtime_failure_report(
                run_spec,
                &prepared_bootstrap,
                stage_started_at,
                pipeline_stage,
                failure,
            );
        }
    };

    let final_assembly_artifacts = match final_assembly_artifacts_for_stage(
        pipeline_stage,
        &prepared_bootstrap,
        &baseline_evaluation_artifacts,
        &candidate_search_artifacts,
        &stage_input,
        progress_sender.as_ref(),
    ) {
        Ok(artifacts) => artifacts,
        Err(failure) => {
            return runtime_failure_report(
                run_spec,
                &prepared_bootstrap,
                stage_started_at,
                pipeline_stage,
                failure,
            );
        }
    };

    build_success_report(
        run_spec,
        &resolved_stage_payloads,
        &prepared_bootstrap.bootstrap_context,
        &baseline_evaluation_artifacts,
        &candidate_search_artifacts,
        final_assembly_artifacts,
        pipeline_stage.calibration_stage(),
    )
}

fn stage_failure_report(
    run_spec: &CalibrationRunSpec,
    pipeline_stage: JointRefinementPipelineStage,
    failure: JointRefinementStageExecutionFailure,
) -> JointRefinementStageExecutionReport {
    build_stage_failure_execution_report(run_spec, pipeline_stage.calibration_stage(), failure)
}

fn runtime_failure_report(
    run_spec: &CalibrationRunSpec,
    prepared_bootstrap: &PreparedBootstrapContext,
    stage_started_at: Instant,
    pipeline_stage: JointRefinementPipelineStage,
    failure: JointRefinementStageExecutionFailure,
) -> JointRefinementStageExecutionReport {
    build_failure_report_with_runtime_profile(
        run_spec,
        &prepared_bootstrap.bootstrap_context,
        stage_started_at,
        pipeline_stage.calibration_stage(),
        failure,
    )
}

fn prepare_stage_runtime<'a>(
    input: StagePreparationInput<'a>,
) -> Result<StagePreparationArtifacts<'a>, JointRefinementStageExecutionFailure> {
    let StagePreparationInput {
        run_spec,
        calendar_registry,
        stage_input,
        progress_sender,
        disk_checkpoint_directory,
        load_current_stage_resume_state,
        pipeline_stage,
        cancellation_token,
    } = input;

    let InitialPipelineState {
        resolved_stage_payloads,
        resume_store,
    } = resolve_initial_pipeline_state(
        run_spec,
        calendar_registry,
        stage_input,
        progress_sender,
        disk_checkpoint_directory,
        load_current_stage_resume_state,
        pipeline_stage,
    )?;

    let prepared_bootstrap = prepare_bootstrap_context(
        run_spec,
        calendar_registry,
        stage_input,
        &resolved_stage_payloads,
        progress_sender,
        cancellation_token.shared_handle(),
    )?;

    Ok(StagePreparationArtifacts {
        resolved_stage_payloads,
        resume_store,
        prepared_bootstrap,
    })
}

fn validate_pipeline_stage_input(
    stage_input: &JointRefinementStageExecutionInput,
    cancellation_token: &CancellationToken,
) -> Result<(), JointRefinementStageExecutionFailure> {
    if cancellation_token.is_cancelled() {
        return Err(
            JointRefinementStageExecutionFailure::SimulationInfrastructureUnavailable {
                error_message: "operation cancelled".to_string(),
            },
        );
    }

    validate_pipeline_stage_configuration(stage_input)
}

fn validate_pipeline_stage_configuration(
    stage_input: &JointRefinementStageExecutionInput,
) -> Result<(), JointRefinementStageExecutionFailure> {
    if stage_input.candidate_budget_override == Some(0) {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint refinement candidate_budget_override must be greater than zero"
                        .to_string(),
            },
        );
    }

    if let Some(calibration_effort) = stage_input.calibration_effort
        && let Err(source) = calibration_effort.validate()
    {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!("joint refinement calibration_effort {source}"),
            },
        );
    }

    if stage_input.option_refinement_candidate_count_override == Some(0) {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint refinement option_refinement_candidate_count_override must be greater than zero"
                        .to_string(),
            },
        );
    }

    if stage_input.option_refinement_wall_clock_budget_millis == Some(0) {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint refinement option_refinement_wall_clock_budget_millis must be greater than zero"
                        .to_string(),
            },
        );
    }

    if stage_input.audit_option_pricing_enabled && stage_input.option_pricing_input.is_none() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint refinement audit_option_pricing_enabled requires option_pricing_input"
                        .to_string(),
            },
        );
    }

    Ok(())
}

struct StageRefinementStepsInput<'a> {
    run_spec: &'a CalibrationRunSpec,
    stage_input: &'a JointRefinementStageExecutionInput,
    resolved_stage_payloads: &'a ResolvedStagePayloads<'a>,
    prepared_bootstrap: &'a PreparedBootstrapContext,
    baseline_evaluation_artifacts: &'a BaselineEvaluationArtifacts,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    resume_store: &'a mut JointRefinementResumeStore,
    disk_checkpoint_directory: Option<&'a Path>,
    pipeline_stage: JointRefinementPipelineStage,
    upstream_joint_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    upstream_option_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
}

fn run_stage_refinement_steps(
    input: StageRefinementStepsInput<'_>,
) -> Result<CandidateSearchArtifacts, JointRefinementStageExecutionFailure> {
    let StageRefinementStepsInput {
        run_spec,
        stage_input,
        resolved_stage_payloads,
        prepared_bootstrap,
        baseline_evaluation_artifacts,
        progress_sender,
        resume_store,
        disk_checkpoint_directory,
        pipeline_stage,
        upstream_joint_refinement_handoff,
        upstream_option_refinement_handoff,
    } = input;

    let mut candidate_search_artifacts =
        resolve_candidate_search_artifacts_for_stage(CandidateSearchArtifactResolutionInput {
            run_spec,
            stage_input,
            resolved_stage_payloads,
            prepared_bootstrap,
            baseline_evaluation_artifacts,
            progress_sender,
            resume_store,
            disk_checkpoint_directory,
            pipeline_stage,
            upstream_joint_refinement_handoff,
            upstream_option_refinement_handoff,
        })?;

    if should_refit_market_pressure_after_candidate_selection(pipeline_stage, run_spec, stage_input)
        && let Err(failure) = refit_market_pressure_after_candidate_selection(
            stage_input,
            resolved_stage_payloads,
            &mut candidate_search_artifacts,
            progress_sender,
        )
    {
        return Err(failure);
    }

    if should_run_option_pricing_stage(pipeline_stage) {
        run_option_refinement_if_enabled(
            stage_input,
            &prepared_bootstrap.bootstrap_context,
            &mut candidate_search_artifacts,
            progress_sender,
            resume_store,
        )?;

        run_audit_option_pricing_if_enabled(
            stage_input,
            &prepared_bootstrap.bootstrap_context,
            &mut candidate_search_artifacts,
            progress_sender,
            resume_store,
        )?;
    }

    Ok(candidate_search_artifacts)
}

fn final_assembly_artifacts_for_stage(
    pipeline_stage: JointRefinementPipelineStage,
    prepared_bootstrap: &PreparedBootstrapContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    candidate_search_artifacts: &CandidateSearchArtifacts,
    stage_input: &JointRefinementStageExecutionInput,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<FinalAssemblyArtifacts, JointRefinementStageExecutionFailure> {
    if pipeline_stage != JointRefinementPipelineStage::Finalization {
        return Ok(empty_final_assembly_artifacts());
    }

    assemble_final_stage_outputs(
        &prepared_bootstrap.bootstrap_context,
        baseline_evaluation_artifacts,
        candidate_search_artifacts,
        &stage_input.diagnostics,
        progress_sender,
    )
}
