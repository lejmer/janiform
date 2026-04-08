from __future__ import annotations

from contextlib import nullcontext, redirect_stdout
from datetime import datetime
from io import StringIO
from pathlib import Path
from typing import Any, Callable, Optional

from ..._workflow_outputs import write_fit_artifacts
from ..._workflow_progress import (
    JaniformOutputController,
    JsonLinesProgress,
    SpinnerProgress,
    WorkflowProgressMode,
)
from ...calendar import CalendarRegistry
from ..errors import calibration_error, calibration_execution_error
from ..market_data.contracts import CalibrationFlatCarryInput, CalibrationInputFileFormat
from ..staged import CalibrationStageExecutionStatus
from ..staged.progress import NativeStagedCalibrationProgressTracker, update_cli_progress_from_staged_native_event
from ..staged.results import build_staged_failure_context, format_staged_failure_summary
from ..staged.runtime import run_staged_calibration_with_prepared_handle_and_events
from .model import JaniformModel
from .preparation.contracts import FitExecutionEstimator, PreparedFitWorkflow
from .report import JaniformFitReport
from .runtime.metadata import build_model_metadata, extract_model_parameters, raw_stage_report_payloads


def fit_with_estimator(
    estimator: FitExecutionEstimator,
    *,
    run_id: Optional[str] = None,
    tick_data_path: str | Path,
    tick_data_format: Optional[CalibrationInputFileFormat] = None,
    tick_data_provenance_label: str = "tick_data",
    option_surface_path: Optional[str | Path] = None,
    option_surface_format: CalibrationInputFileFormat = CalibrationInputFileFormat.PARQUET,
    option_surface_provenance_label: str = "option_surface",
    option_surface_valuation_time_utc: Optional[datetime] = None,
    carry: Optional[CalibrationFlatCarryInput] = None,
    output_dir: Optional[str | Path] = None,
    silent: Optional[bool] = None,
    report: bool = False,
    output_controller_type: type[JaniformOutputController],
    run_staged_calibration_with_prepared_handle_and_events_method: Callable[..., tuple[Any, dict[str, Any]]] = run_staged_calibration_with_prepared_handle_and_events,
) -> JaniformModel | tuple[Optional[JaniformModel], JaniformFitReport]:
    if not tick_data_provenance_label.strip():
        raise calibration_error("tick_data_provenance_label", "must be a non-empty string")
    if not option_surface_provenance_label.strip():
        raise calibration_error("option_surface_provenance_label", "must be a non-empty string")

    effective_silent = estimator._silent if silent is None else silent
    output_controller = output_controller_type(
        suppress_info_output=effective_silent,
        default_progress_mode=(WorkflowProgressMode.NONE if effective_silent else WorkflowProgressMode.HUMAN),
    )
    prepared_workflow: Optional[PreparedFitWorkflow] = None

    suppression_context = redirect_stdout(StringIO()) if effective_silent else nullcontext()
    with suppression_context:
        try:
            with output_controller.progress("fit", total_steps=estimator._fit_progress_total_steps()) as progress:
                progress.update(detail="inferring workflow inputs")
                prepared_workflow = estimator._prepare_fit_workflow(
                    run_id=run_id,
                    tick_data_path=tick_data_path,
                    tick_data_format=tick_data_format,
                    tick_data_provenance_label=tick_data_provenance_label,
                    option_surface_path=option_surface_path,
                    option_surface_format=option_surface_format,
                    option_surface_provenance_label=option_surface_provenance_label,
                    option_surface_valuation_time_utc=option_surface_valuation_time_utc,
                    carry=carry,
                    progress=progress,
                )
                if prepared_workflow.fit_warning is not None:
                    output_controller.warning(prepared_workflow.fit_warning)

                progress.update(detail="running calibration")

                calendar_registry = CalendarRegistry(
                    prepared_workflow.prepared.session_resolution.resolved_session_definition_json
                )
                uses_default_prepared_runner = estimator._uses_default_prepared_runner()

                if isinstance(progress, SpinnerProgress | JsonLinesProgress) and uses_default_prepared_runner:
                    staged_progress_tracker = NativeStagedCalibrationProgressTracker()
                    staged_response, raw_response_payload = (
                        run_staged_calibration_with_prepared_handle_and_events_method(
                            prepared_workflow.prepared.prepared_handle,
                            calendar_registry=calendar_registry,
                            event_callback=lambda raw_event: update_cli_progress_from_staged_native_event(
                                progress,
                                staged_progress_tracker,
                                raw_event,
                            ),
                        )
                    )
                else:
                    staged_response, raw_response_payload = estimator._prepared_staged_runner()(
                        prepared_workflow.prepared.prepared_handle,
                        calendar_registry=calendar_registry,
                    )
        finally:
            if prepared_workflow is not None and prepared_workflow.temporary_directory is not None:
                prepared_workflow.temporary_directory.cleanup()

    fit_report = JaniformFitReport(
        run_id=prepared_workflow.run_id,
        workflow_scope=prepared_workflow.workflow_scope,
        success=staged_response.status == CalibrationStageExecutionStatus.SUCCESS,
        resolved_configuration=prepared_workflow.resolved_configuration,
        session_resolution=prepared_workflow.prepared.session_resolution,
        preparation_diagnostics=prepared_workflow.prepared.diagnostics,
        preparation_provenance=prepared_workflow.prepared.provenance,
        staged_request=prepared_workflow.prepared.staged_request,
        prepared_data_reference=prepared_workflow.prepared.prepared_data_reference,
        prepared_payload_path=prepared_workflow.prepared.prepared_payload_path,
        staged_execution_input_path=prepared_workflow.prepared.staged_execution_input_path,
        staged_response=staged_response,
        stage_report_payloads=raw_stage_report_payloads(raw_response_payload),
    )

    if staged_response.status != CalibrationStageExecutionStatus.SUCCESS:
        if report:
            return None, fit_report

        raise calibration_execution_error(
            format_staged_failure_summary(staged_response),
            entrypoint="fit",
            context=build_staged_failure_context(staged_response),
        )

    model = JaniformModel(
        run_id=prepared_workflow.run_id,
        workflow_scope=prepared_workflow.workflow_scope,
        parameters=extract_model_parameters(fit_report.stage_report_payloads),
        metadata=build_model_metadata(
            resolved_configuration=prepared_workflow.resolved_configuration,
            prepared=prepared_workflow.prepared,
            staged_response=staged_response,
            stage_report_payloads=fit_report.stage_report_payloads,
            input_inference=estimator._build_fit_inference_metadata(prepared_workflow),
            reference_price=prepared_workflow.reference_price,
        ),
        resolved_configuration=prepared_workflow.resolved_configuration,
        session_resolution=prepared_workflow.prepared.session_resolution,
        _stage_report_payloads=fit_report.stage_report_payloads,
    )

    if output_dir is not None:
        write_fit_artifacts(model=model, fit_report=fit_report, output_dir=output_dir)

    if report:
        return model, fit_report

    return model
