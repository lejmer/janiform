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
        run_spec,
        calendar_registry,
        stage_input,
        None,
        None,
        true,
        cancellation_token,
    )
}

pub(crate) fn run_joint_refinement_calibration_stage_with_progress_observer_and_persistence(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
    progress_observer: Option<&mut dyn FnMut(JointRefinementStageProgressUpdate)>,
    disk_checkpoint_directory: Option<&Path>,
    load_current_stage_resume_state: bool,
    cancellation_token: CancellationToken,
) -> JointRefinementStageExecutionReport {
    run_pipeline_stage_with_progress_observer_and_persistence(
        run_spec,
        calendar_registry,
        stage_input,
        progress_observer,
        disk_checkpoint_directory,
        load_current_stage_resume_state,
        cancellation_token,
        super::pipeline::run_joint_refinement_calibration_stage_internal,
    )
}

pub(crate) fn run_option_refinement_calibration_stage_with_progress_observer_and_persistence(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
    upstream_joint_refinement_handoff: Option<&JointRefinementStageRuntimeHandoff>,
    progress_observer: Option<&mut dyn FnMut(JointRefinementStageProgressUpdate)>,
    disk_checkpoint_directory: Option<&Path>,
    load_current_stage_resume_state: bool,
    cancellation_token: CancellationToken,
) -> JointRefinementStageExecutionReport {
    run_pipeline_stage_with_progress_observer_and_persistence(
        run_spec,
        calendar_registry,
        stage_input,
        progress_observer,
        disk_checkpoint_directory,
        load_current_stage_resume_state,
        cancellation_token,
        move |run_spec,
              calendar_registry,
              stage_input,
              progress_sender,
              disk_checkpoint_directory,
              load_current_stage_resume_state,
              cancellation_token| {
            super::pipeline::run_option_refinement_calibration_stage_internal(
                run_spec,
                calendar_registry,
                stage_input,
                upstream_joint_refinement_handoff,
                progress_sender,
                disk_checkpoint_directory,
                load_current_stage_resume_state,
                cancellation_token,
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
        input.run_spec,
        input.calendar_registry,
        input.stage_input,
        progress_observer,
        input.disk_checkpoint_directory,
        input.load_current_stage_resume_state,
        input.cancellation_token,
        move |run_spec,
              calendar_registry,
              stage_input,
              progress_sender,
              disk_checkpoint_directory,
              load_current_stage_resume_state,
              cancellation_token| {
            super::pipeline::run_finalization_calibration_stage_internal(
                super::pipeline::FinalizationPipelineStageRunInput {
                    run_spec,
                    calendar_registry,
                    stage_input,
                    upstream_joint_refinement_handoff: input.upstream_joint_refinement_handoff,
                    upstream_option_refinement_handoff: input.upstream_option_refinement_handoff,
                    progress_sender,
                    disk_checkpoint_directory,
                    load_current_stage_resume_state,
                    cancellation_token,
                },
            )
        },
    )
}

fn run_pipeline_stage_with_progress_observer_and_persistence<RunStage>(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
    progress_observer: Option<&mut dyn FnMut(JointRefinementStageProgressUpdate)>,
    disk_checkpoint_directory: Option<&Path>,
    load_current_stage_resume_state: bool,
    cancellation_token: CancellationToken,
    run_stage: RunStage,
) -> JointRefinementStageExecutionReport
where
    RunStage: FnOnce(
            &CalibrationRunSpec,
            &ExchangeCalendarRegistry,
            JointRefinementStageExecutionInput,
            Option<mpsc::Sender<JointRefinementStageProgressUpdate>>,
            Option<&Path>,
            bool,
            CancellationToken,
        ) -> JointRefinementStageExecutionReport
        + Send,
{
    let Some(progress_observer) = progress_observer else {
        return run_stage(
            run_spec,
            calendar_registry,
            stage_input,
            None,
            disk_checkpoint_directory,
            load_current_stage_resume_state,
            cancellation_token,
        );
    };

    let (progress_sender, progress_receiver) =
        mpsc::channel::<JointRefinementStageProgressUpdate>();

    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            run_stage(
                run_spec,
                calendar_registry,
                stage_input,
                Some(progress_sender),
                disk_checkpoint_directory,
                load_current_stage_resume_state,
                cancellation_token,
            )
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
