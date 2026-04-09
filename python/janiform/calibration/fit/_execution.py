"""Internal fit-execution orchestration."""

from __future__ import annotations

from contextlib import nullcontext, redirect_stdout
from datetime import datetime
from io import StringIO
from pathlib import Path
from typing import Any, Optional, Union

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
from .model import JaniformModel
from ._preparation.contracts import FitExecutionConfiguration, PreparedFitWorkflow
from .report import JaniformFitReport
from ._runtime.metadata import build_model_metadata, extract_model_parameters, raw_stage_report_payloads


def fit_with_execution_configuration(
    configuration: FitExecutionConfiguration,
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
    report: bool = False,
) -> Union[JaniformModel, tuple[Optional[JaniformModel], JaniformFitReport]]:
    _validate_fit_input_labels(
        tick_data_provenance_label=tick_data_provenance_label,
        option_surface_provenance_label=option_surface_provenance_label,
    )
    output_controller = _build_fit_output_controller(
        configuration.output_controller_factory,
        silent=configuration.silent,
    )
    prepared_workflow, staged_response, raw_response_payload = _run_fit_with_output_controller(
        configuration,
        output_controller=output_controller,
        run_id=run_id,
        tick_data_path=tick_data_path,
        tick_data_format=tick_data_format,
        tick_data_provenance_label=tick_data_provenance_label,
        option_surface_path=option_surface_path,
        option_surface_format=option_surface_format,
        option_surface_provenance_label=option_surface_provenance_label,
        option_surface_valuation_time_utc=option_surface_valuation_time_utc,
        carry=carry,
    )
    fit_report, model = _build_fit_outputs(
        configuration,
        prepared_workflow=prepared_workflow,
        staged_response=staged_response,
        raw_response_payload=raw_response_payload,
        output_dir=output_dir,
    )

    if model is None:
        if report:
            return None, fit_report

        raise calibration_execution_error(
            format_staged_failure_summary(fit_report.staged_response),
            entrypoint="fit",
            context=build_staged_failure_context(fit_report.staged_response),
        )

    if report:
        return model, fit_report

    return model


def _validate_fit_input_labels(
    *,
    tick_data_provenance_label: str,
    option_surface_provenance_label: str,
) -> None:
    if not tick_data_provenance_label.strip():
        raise calibration_error("tick_data_provenance_label", "must be a non-empty string")
    if not option_surface_provenance_label.strip():
        raise calibration_error("option_surface_provenance_label", "must be a non-empty string")


def _build_fit_output_controller(
    output_controller_factory: Any,
    *,
    silent: bool,
) -> JaniformOutputController:
    return output_controller_factory(
        suppress_info_output=silent,
        default_progress_mode=(WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN),
    )


def _run_fit_with_output_controller(
    configuration: FitExecutionConfiguration,
    *,
    output_controller: JaniformOutputController,
    run_id: Optional[str],
    tick_data_path: str | Path,
    tick_data_format: Optional[CalibrationInputFileFormat],
    tick_data_provenance_label: str,
    option_surface_path: Optional[str | Path],
    option_surface_format: CalibrationInputFileFormat,
    option_surface_provenance_label: str,
    option_surface_valuation_time_utc: Optional[datetime],
    carry: Optional[CalibrationFlatCarryInput],
) -> tuple[PreparedFitWorkflow, Any, dict[str, Any]]:
    prepared_workflow: Optional[PreparedFitWorkflow] = None
    suppression_context = redirect_stdout(StringIO()) if configuration.silent else nullcontext()
    staged_response: Optional[Any] = None
    raw_response_payload: Optional[dict[str, Any]] = None
    with suppression_context:
        try:
            with output_controller.progress("fit", total_steps=configuration.progress_total_steps) as progress:
                progress.update(detail="inferring workflow inputs")
                prepared_workflow = configuration.prepare_fit_workflow(
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
                staged_response, raw_response_payload = _run_prepared_fit_calibration(
                    configuration,
                    prepared_workflow=prepared_workflow,
                    progress=progress,
                )
        finally:
            if prepared_workflow is not None and prepared_workflow.temporary_directory is not None:
                prepared_workflow.temporary_directory.cleanup()

    if prepared_workflow is None:
        raise RuntimeError("Fit execution did not produce a PreparedFitWorkflow")
    if staged_response is None or raw_response_payload is None:
        raise RuntimeError("Fit execution did not produce a staged calibration response")

    return prepared_workflow, staged_response, raw_response_payload


def _run_prepared_fit_calibration(
    configuration: FitExecutionConfiguration,
    *,
    prepared_workflow: PreparedFitWorkflow,
    progress: Any,
) -> tuple[Any, dict[str, Any]]:
    calendar_registry = CalendarRegistry(
        prepared_workflow.prepared.session_resolution.resolved_session_definition_json
    )
    if isinstance(progress, SpinnerProgress | JsonLinesProgress) and configuration.use_default_prepared_runner:
        staged_progress_tracker = NativeStagedCalibrationProgressTracker()
        return configuration.evented_prepared_runner(
            prepared_workflow.prepared.prepared_handle,
            calendar_registry=calendar_registry,
            event_callback=lambda raw_event: update_cli_progress_from_staged_native_event(
                progress,
                staged_progress_tracker,
                raw_event,
            ),
        )

    return configuration.prepared_staged_runner(
        prepared_workflow.prepared.prepared_handle,
        calendar_registry=calendar_registry,
    )


def _build_fit_outputs(
    configuration: FitExecutionConfiguration,
    *,
    prepared_workflow: PreparedFitWorkflow,
    staged_response: Any,
    raw_response_payload: dict[str, Any],
    output_dir: Optional[str | Path],
) -> tuple[JaniformFitReport, Optional[JaniformModel]]:
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
        return fit_report, None

    model = JaniformModel(
        run_id=prepared_workflow.run_id,
        workflow_scope=prepared_workflow.workflow_scope,
        parameters=extract_model_parameters(fit_report.stage_report_payloads),
        metadata=build_model_metadata(
            resolved_configuration=prepared_workflow.resolved_configuration,
            prepared=prepared_workflow.prepared,
            staged_response=staged_response,
            stage_report_payloads=fit_report.stage_report_payloads,
            input_inference=configuration.build_fit_inference_metadata(prepared_workflow),
            reference_price=prepared_workflow.reference_price,
        ),
        resolved_configuration=prepared_workflow.resolved_configuration,
        session_resolution=prepared_workflow.prepared.session_resolution,
        _stage_report_payloads=fit_report.stage_report_payloads,
    )

    if output_dir is not None:
        write_fit_artifacts(model=model, fit_report=fit_report, output_dir=output_dir)

    return fit_report, model
