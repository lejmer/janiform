use std::path::Path;
use std::sync::mpsc;

use janiform_foundations::cancellation::CancellationToken;

use super::JointRefinementStageRuntimeHandoff;
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageExecutionReport, JointRefinementStageProgressUpdate,
};
use janiform_calibration_artifacts::CalibrationRunSpec;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

pub fn run_joint_refinement_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
    cancellation_token: CancellationToken,
) -> JointRefinementStageExecutionReport {
    run_joint_refinement_calibration_stage_with_progress_observer_and_persistence(
        JointRefinementCalibrationStageEntrypointInput {
            run_spec,
            calendar_registry,
            stage_input,
            disk_checkpoint_directory: None,
            load_current_stage_resume_state: true,
            cancellation_token,
        },
        None,
    )
}

pub(crate) struct JointRefinementCalibrationStageEntrypointInput<'a> {
    pub(crate) run_spec: &'a CalibrationRunSpec,
    pub(crate) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(crate) stage_input: JointRefinementStageExecutionInput,
    pub(crate) disk_checkpoint_directory: Option<&'a Path>,
    pub(crate) load_current_stage_resume_state: bool,
    pub(crate) cancellation_token: CancellationToken,
}

pub(crate) fn run_joint_refinement_calibration_stage_with_progress_observer_and_persistence(
    input: JointRefinementCalibrationStageEntrypointInput<'_>,
    progress_observer: Option<&mut dyn FnMut(JointRefinementStageProgressUpdate)>,
) -> JointRefinementStageExecutionReport {
    run_pipeline_stage_with_progress_observer_and_persistence(
        PipelineStageEntrypointInput {
            run_spec: input.run_spec,
            calendar_registry: input.calendar_registry,
            stage_input: input.stage_input,
            progress_observer,
            disk_checkpoint_directory: input.disk_checkpoint_directory,
            load_current_stage_resume_state: input.load_current_stage_resume_state,
            cancellation_token: input.cancellation_token,
        },
        |stage_run_input| {
            super::pipeline::run_joint_refinement_calibration_stage_internal(
                stage_run_input.run_spec,
                stage_run_input.calendar_registry,
                stage_run_input.stage_input,
                stage_run_input.progress_sender,
                stage_run_input.disk_checkpoint_directory,
                stage_run_input.load_current_stage_resume_state,
                stage_run_input.cancellation_token,
            )
        },
    )
}

pub(crate) struct OptionRefinementCalibrationStageEntrypointInput<'a> {
    pub(crate) run_spec: &'a CalibrationRunSpec,
    pub(crate) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(crate) stage_input: JointRefinementStageExecutionInput,
    pub(crate) upstream_joint_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    pub(crate) disk_checkpoint_directory: Option<&'a Path>,
    pub(crate) load_current_stage_resume_state: bool,
    pub(crate) cancellation_token: CancellationToken,
}

pub(crate) fn run_option_refinement_calibration_stage_with_progress_observer_and_persistence(
    input: OptionRefinementCalibrationStageEntrypointInput<'_>,
    progress_observer: Option<&mut dyn FnMut(JointRefinementStageProgressUpdate)>,
) -> JointRefinementStageExecutionReport {
    run_pipeline_stage_with_progress_observer_and_persistence(
        PipelineStageEntrypointInput {
            run_spec: input.run_spec,
            calendar_registry: input.calendar_registry,
            stage_input: input.stage_input,
            progress_observer,
            disk_checkpoint_directory: input.disk_checkpoint_directory,
            load_current_stage_resume_state: input.load_current_stage_resume_state,
            cancellation_token: input.cancellation_token,
        },
        move |stage_run_input| {
            super::pipeline::run_option_refinement_calibration_stage_internal(
                super::pipeline::OptionRefinementPipelineStageRunInput {
                    run_spec: stage_run_input.run_spec,
                    calendar_registry: stage_run_input.calendar_registry,
                    stage_input: stage_run_input.stage_input,
                    upstream_joint_refinement_handoff: input.upstream_joint_refinement_handoff,
                    progress_sender: stage_run_input.progress_sender,
                    disk_checkpoint_directory: stage_run_input.disk_checkpoint_directory,
                    load_current_stage_resume_state: stage_run_input
                        .load_current_stage_resume_state,
                    cancellation_token: stage_run_input.cancellation_token,
                },
            )
        },
    )
}

pub(crate) struct FinalizationCalibrationStageEntrypointInput<'a> {
    pub(crate) run_spec: &'a CalibrationRunSpec,
    pub(crate) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(crate) stage_input: JointRefinementStageExecutionInput,
    pub(crate) upstream_joint_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    pub(crate) upstream_option_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    pub(crate) disk_checkpoint_directory: Option<&'a Path>,
    pub(crate) load_current_stage_resume_state: bool,
    pub(crate) cancellation_token: CancellationToken,
}

pub(crate) fn run_finalization_calibration_stage_with_progress_observer_and_persistence(
    input: FinalizationCalibrationStageEntrypointInput<'_>,
    progress_observer: Option<&mut dyn FnMut(JointRefinementStageProgressUpdate)>,
) -> JointRefinementStageExecutionReport {
    run_pipeline_stage_with_progress_observer_and_persistence(
        PipelineStageEntrypointInput {
            run_spec: input.run_spec,
            calendar_registry: input.calendar_registry,
            stage_input: input.stage_input,
            progress_observer,
            disk_checkpoint_directory: input.disk_checkpoint_directory,
            load_current_stage_resume_state: input.load_current_stage_resume_state,
            cancellation_token: input.cancellation_token,
        },
        move |stage_run_input| {
            super::pipeline::run_finalization_calibration_stage_internal(
                super::pipeline::FinalizationPipelineStageRunInput {
                    run_spec: stage_run_input.run_spec,
                    calendar_registry: stage_run_input.calendar_registry,
                    stage_input: stage_run_input.stage_input,
                    upstream_joint_refinement_handoff: input.upstream_joint_refinement_handoff,
                    upstream_option_refinement_handoff: input.upstream_option_refinement_handoff,
                    progress_sender: stage_run_input.progress_sender,
                    disk_checkpoint_directory: stage_run_input.disk_checkpoint_directory,
                    load_current_stage_resume_state: stage_run_input
                        .load_current_stage_resume_state,
                    cancellation_token: stage_run_input.cancellation_token,
                },
            )
        },
    )
}

struct PipelineStageEntrypointInput<'a, 'observer> {
    run_spec: &'a CalibrationRunSpec,
    calendar_registry: &'a ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
    progress_observer: Option<&'observer mut dyn FnMut(JointRefinementStageProgressUpdate)>,
    disk_checkpoint_directory: Option<&'a Path>,
    load_current_stage_resume_state: bool,
    cancellation_token: CancellationToken,
}

struct PipelineStageRunInput<'a> {
    run_spec: &'a CalibrationRunSpec,
    calendar_registry: &'a ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
    progress_sender: Option<mpsc::Sender<JointRefinementStageProgressUpdate>>,
    disk_checkpoint_directory: Option<&'a Path>,
    load_current_stage_resume_state: bool,
    cancellation_token: CancellationToken,
}

fn run_pipeline_stage_with_progress_observer_and_persistence<'a, 'observer, RunStage>(
    input: PipelineStageEntrypointInput<'a, 'observer>,
    run_stage: RunStage,
) -> JointRefinementStageExecutionReport
where
    RunStage: FnOnce(PipelineStageRunInput<'a>) -> JointRefinementStageExecutionReport + Send,
{
    let PipelineStageEntrypointInput {
        run_spec,
        calendar_registry,
        stage_input,
        progress_observer,
        disk_checkpoint_directory,
        load_current_stage_resume_state,
        cancellation_token,
    } = input;

    let Some(progress_observer) = progress_observer else {
        return run_stage(PipelineStageRunInput {
            run_spec,
            calendar_registry,
            stage_input,
            progress_sender: None,
            disk_checkpoint_directory,
            load_current_stage_resume_state,
            cancellation_token,
        });
    };

    let (progress_sender, progress_receiver) =
        mpsc::channel::<JointRefinementStageProgressUpdate>();

    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            run_stage(PipelineStageRunInput {
                run_spec,
                calendar_registry,
                stage_input,
                progress_sender: Some(progress_sender),
                disk_checkpoint_directory,
                load_current_stage_resume_state,
                cancellation_token,
            })
        });

        for progress_update in progress_receiver {
            progress_observer(progress_update);
        }

        match worker.join() {
            Ok(report) => report,
            Err(_) => super::build_joint_refinement_stage_failure_report(
                run_spec,
                JointRefinementStageExecutionFailure::SimulationInfrastructureUnavailable {
                    error_message: "joint refinement progress worker terminated unexpectedly"
                        .to_string(),
                },
            ),
        }
    })
}
