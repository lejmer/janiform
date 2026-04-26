"""Internal fit-execution orchestration."""

from __future__ import annotations

from contextlib import nullcontext, redirect_stdout
from datetime import datetime
from io import StringIO
from pathlib import Path
from typing import Any, Optional, Union

from ..._workflow_progress import (
    JaniformOutputController,
    WorkflowProgressMode,
    resolve_workflow_progress_mode,
)
from ..errors import (
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    calibration_error,
    calibration_execution_error,
    calibration_integration_error,
)
from ..market_data.contracts import CalibrationFlatCarryInput, CalibrationInputFileFormat
from ..staged import (
    CalibrationResumeSettings,
    CalibrationStageExecutionStatus,
)
from ..staged.results import build_staged_failure_context, format_staged_failure_summary
from ._preparation.contracts import FitExecutionConfiguration, PreparedFitWorkflow
from ._runtime.metadata import normalize_model_parameters
from ._runtime.stage_reports import resolved_stage_report_payloads
from ._runtime.workflow import NativeFitWorkflowOutput, run_prepared_fit_workflow
from .model import JaniformModel
from .report import JaniformFitReport


def fit_with_execution_configuration(
    configuration: FitExecutionConfiguration,
    *,
    run_id: Optional[str] = None,
    resume_settings: Optional[CalibrationResumeSettings] = None,
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

    (
        prepared_workflow,
        staged_response,
        raw_response_payload,
        native_model_payload,
    ) = _run_fit_with_output_controller(
        configuration,
        output_controller=output_controller,
        output_dir=output_dir,
        run_id=run_id,
        resume_settings=resume_settings,
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
        prepared_workflow=prepared_workflow,
        staged_response=staged_response,
        raw_response_payload=raw_response_payload,
        native_model_payload=native_model_payload,
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
    output_dir: Optional[str | Path],
    run_id: Optional[str],
    resume_settings: Optional[CalibrationResumeSettings],
    tick_data_path: str | Path,
    tick_data_format: Optional[CalibrationInputFileFormat],
    tick_data_provenance_label: str,
    option_surface_path: Optional[str | Path],
    option_surface_format: CalibrationInputFileFormat,
    option_surface_provenance_label: str,
    option_surface_valuation_time_utc: Optional[datetime],
    carry: Optional[CalibrationFlatCarryInput],
) -> tuple[PreparedFitWorkflow, Any, dict[str, Any], Optional[dict[str, Any]]]:
    prepared_workflow: Optional[PreparedFitWorkflow] = None
    suppression_context = redirect_stdout(StringIO()) if configuration.silent else nullcontext()
    native_output: Optional[NativeFitWorkflowOutput] = None
    staged_response: Optional[Any] = None
    raw_response_payload: Optional[dict[str, Any]] = None
    resolved_progress_mode = resolve_workflow_progress_mode(
        output_controller.progress_mode,
        default_mode=output_controller.default_progress_mode,
    )

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

                native_output = run_prepared_fit_workflow(
                    configuration,
                    prepared_workflow=prepared_workflow,
                    output_dir=output_dir,
                    resume_settings=resume_settings,
                    progress_mode=resolved_progress_mode,
                )
                staged_response = native_output.staged_response
                raw_response_payload = native_output.raw_staged_response_payload
        finally:
            if prepared_workflow is not None and prepared_workflow.temporary_directory is not None:
                prepared_workflow.temporary_directory.cleanup()

            if native_output is not None and native_output.temporary_directory is not None:
                native_output.temporary_directory.cleanup()

    if prepared_workflow is None:
        raise RuntimeError("Fit execution did not produce a PreparedFitWorkflow")

    if staged_response is None or raw_response_payload is None:
        raise RuntimeError("Fit execution did not produce a staged calibration response")

    return prepared_workflow, staged_response, raw_response_payload, native_output.model_payload


def _build_fit_outputs(
    prepared_workflow: PreparedFitWorkflow,
    staged_response: Any,
    raw_response_payload: dict[str, Any],
    native_model_payload: Optional[dict[str, Any]],
    output_dir: Optional[str | Path],
) -> tuple[JaniformFitReport, Optional[JaniformModel]]:
    _ = output_dir

    stage_report_payloads = resolved_stage_report_payloads(
        raw_response_payload,
        selected_stage_kinds=prepared_workflow.prepared.staged_request.selected_stages,
        run_id=prepared_workflow.run_id,
        disk_checkpoint_directory=prepared_workflow.prepared.request.output_preferences.disk_checkpoint_directory,
        require_complete_selection=staged_response.status == CalibrationStageExecutionStatus.SUCCESS,
    )

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
        stage_report_payloads=stage_report_payloads,
    )

    if staged_response.status != CalibrationStageExecutionStatus.SUCCESS:
        return fit_report, None

    model_parameters, model_metadata = _validated_native_model_payload(native_model_payload)

    model = JaniformModel(
        run_id=prepared_workflow.run_id,
        workflow_scope=prepared_workflow.workflow_scope,
        parameters=model_parameters,
        metadata=model_metadata,
        resolved_configuration=prepared_workflow.resolved_configuration,
        session_resolution=prepared_workflow.prepared.session_resolution,
        _stage_report_payloads=fit_report.stage_report_payloads,
    )

    return fit_report, model


def _validated_native_model_payload(
    native_model_payload: Optional[dict[str, Any]],
) -> tuple[dict[str, Any], dict[str, Any]]:
    if native_model_payload is None:
        raise calibration_integration_error(
            "native fit workflow did not return a model payload for a successful fit",
            entrypoint="fit",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"field": "model_payload"},
        )

    raw_parameters = native_model_payload.get("parameters")
    if not isinstance(raw_parameters, dict):
        raise calibration_integration_error(
            "native fit workflow returned an invalid model parameters payload",
            entrypoint="fit",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"field": "model_payload.parameters"},
        )

    raw_metadata = native_model_payload.get("metadata")
    if not isinstance(raw_metadata, dict):
        raise calibration_integration_error(
            "native fit workflow returned an invalid model metadata payload",
            entrypoint="fit",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"field": "model_payload.metadata"},
        )

    return normalize_model_parameters(raw_parameters), raw_metadata
