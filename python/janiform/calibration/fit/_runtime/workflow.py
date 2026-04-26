from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from types import ModuleType
from typing import Any, Optional

from ...._native_api import native_exception_matches, native_module
from ...._workflow_outputs import resolve_fit_artifact_paths
from ...._workflow_progress import WorkflowProgressMode
from ...errors import (
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    CalibrationValidationError,
    calibration_execution_error,
    calibration_integration_error,
)
from ...staged import CalibrationResumeSettings
from ...staged.results import StagedCalibrationResponse, validate_staged_calibration_response
from .._preparation.contracts import FitExecutionConfiguration, PreparedFitWorkflow


@dataclass(frozen=True, slots=True)
class NativeFitWorkflowOutput:
    staged_response: StagedCalibrationResponse
    raw_staged_response_payload: dict[str, Any]
    fit_report_payload: dict[str, Any]
    model_payload: Optional[dict[str, Any]]
    event_log_path: str
    latest_state_snapshot_path: str
    execution_duration_seconds: float
    temporary_directory: Optional[TemporaryDirectory[str]]


def run_prepared_fit_workflow(
    configuration: FitExecutionConfiguration,
    *,
    prepared_workflow: PreparedFitWorkflow,
    output_dir: Optional[str | Path],
    resume_settings: Optional[CalibrationResumeSettings],
    progress_mode: WorkflowProgressMode,
) -> NativeFitWorkflowOutput:
    temporary_directory: Optional[TemporaryDirectory[str]] = None
    artifact_output_dir: str | Path

    if output_dir is None:
        temporary_directory = TemporaryDirectory(prefix="janiform-fit-workflow-")
        artifact_output_dir = Path(temporary_directory.name)
    else:
        artifact_output_dir = output_dir

    artifact_paths = resolve_fit_artifact_paths(
        run_id=prepared_workflow.run_id,
        output_dir=artifact_output_dir,
    )
    request_payload = {
        "paths": {
            "output_directory": str(artifact_paths.run_directory),
            "model_path": str(artifact_paths.primary_output_path),
            "report_path": str(artifact_paths.report_path),
            "checkpoint_directory": str(artifact_paths.run_directory / "checkpoints"),
        },
        "input_paths": fit_workflow_input_paths(prepared_workflow),
        "preparation_payload": fit_workflow_preparation_payload(
            configuration,
            prepared_workflow=prepared_workflow,
        ),
        "staged_execution_input_path": prepared_workflow.prepared.staged_execution_input_path,
        "session_definition_json": (
            prepared_workflow
            .prepared
            .session_resolution
            .resolved_session_definition_json
        ),
        "resume_settings": None if resume_settings is None else resume_settings.to_payload(),
        "progress_mode": native_fit_workflow_progress_mode(progress_mode),
        "emit_machine_events": progress_mode == WorkflowProgressMode.JSONL,
    }

    native = _native_module_for_fit_workflow()
    try:
        raw_output = native.run_prepared_fit_workflow(
            prepared_workflow.prepared.prepared_handle,
            request_payload,
        )
    except Exception as error:
        if temporary_directory is not None:
            temporary_directory.cleanup()

        _raise_fit_workflow_error(native, error)

    if not isinstance(raw_output, dict):
        if temporary_directory is not None:
            temporary_directory.cleanup()

        raise calibration_integration_error(
            "native fit workflow returned an invalid output payload",
            entrypoint="fit",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_output).__name__},
        )

    return _validate_native_fit_workflow_output(
        raw_output,
        temporary_directory=temporary_directory,
    )


def fit_workflow_input_paths(prepared_workflow: PreparedFitWorkflow) -> list[str]:
    request = prepared_workflow.prepared.request
    input_paths = [f"ticks: {request.tick_data_source.path}"]

    if request.option_surface_source is not None:
        input_paths.append(f"options: {request.option_surface_source.path}")

    if request.known_news_inputs is not None:
        input_paths.append(f"known news: {request.known_news_inputs.calendar_source.path}")

    return input_paths


def fit_workflow_preparation_payload(
    configuration: FitExecutionConfiguration,
    *,
    prepared_workflow: PreparedFitWorkflow,
) -> dict[str, Any]:
    return {
        "request": prepared_workflow.prepared.request.to_payload(),
        "diagnostics": prepared_workflow.prepared.diagnostics.to_payload(),
        "provenance": prepared_workflow.prepared.provenance.to_payload(),
        "session_resolution": prepared_workflow.prepared.session_resolution.to_payload(),
        "resolved_configuration": prepared_workflow.resolved_configuration.to_payload(),
        "input_inference": configuration.build_fit_inference_metadata(prepared_workflow),
        "reference_price": prepared_workflow.reference_price,
        "fit_warning": prepared_workflow.fit_warning,
    }


def native_fit_workflow_progress_mode(progress_mode: WorkflowProgressMode) -> str:
    match progress_mode:
        case WorkflowProgressMode.HUMAN:
            return "human"
        case WorkflowProgressMode.JSONL | WorkflowProgressMode.NONE:
            return "none"
        case WorkflowProgressMode.AUTO:
            return "none"


def _native_module_for_fit_workflow() -> ModuleType:
    try:
        return native_module()
    except ModuleNotFoundError as error:
        raise calibration_integration_error(
            "native fit workflow module is unavailable",
            entrypoint="fit",
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from error


def _raise_fit_workflow_error(native: ModuleType, error: Exception) -> None:
    error_context = {
        "source_exception_type": type(error).__name__,
        "source_error_message": str(error),
    }
    if native_exception_matches(native, error, "CalibrationExecutionError"):
        raise calibration_execution_error(
            "fit workflow failed during native execution",
            entrypoint="fit",
            context=error_context,
        ) from error

    raise calibration_execution_error(
        "fit workflow failed during native execution",
        entrypoint="fit",
        context=error_context,
    ) from error


def _validate_native_fit_workflow_output(
    raw_output: dict[str, Any],
    *,
    temporary_directory: Optional[TemporaryDirectory[str]],
) -> NativeFitWorkflowOutput:
    raw_staged_response_payload = raw_output.get("raw_staged_response_payload")
    if not isinstance(raw_staged_response_payload, dict):
        _cleanup_temporary_directory(temporary_directory)

        raise calibration_integration_error(
            "native fit workflow returned an invalid staged response payload",
            entrypoint="fit",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"field": "raw_staged_response_payload"},
        )

    try:
        staged_response = validate_staged_calibration_response(raw_staged_response_payload)
    except CalibrationValidationError as error:
        _cleanup_temporary_directory(temporary_directory)

        raise calibration_integration_error(
            "native fit workflow returned an invalid staged response payload",
            entrypoint="fit",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": error.error_id},
        ) from error

    fit_report_payload = _required_mapping(
        raw_output,
        "fit_report_payload",
        temporary_directory=temporary_directory,
    )

    raw_model_payload = raw_output.get("model_payload")
    if raw_model_payload is not None and not isinstance(raw_model_payload, dict):
        _cleanup_temporary_directory(temporary_directory)

        raise calibration_integration_error(
            "native fit workflow returned an invalid model payload",
            entrypoint="fit",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"field": "model_payload"},
        )

    model_payload = raw_model_payload

    return NativeFitWorkflowOutput(
        staged_response=staged_response,
        raw_staged_response_payload=raw_staged_response_payload,
        fit_report_payload=fit_report_payload,
        model_payload=model_payload,
        event_log_path=str(raw_output.get("event_log_path", "")),
        latest_state_snapshot_path=str(raw_output.get("latest_state_snapshot_path", "")),
        execution_duration_seconds=float(raw_output.get("execution_duration_seconds", 0.0)),
        temporary_directory=temporary_directory,
    )


def _required_mapping(
    raw_output: dict[str, Any],
    field_name: str,
    *,
    temporary_directory: Optional[TemporaryDirectory[str]],
) -> dict[str, Any]:
    raw_value = raw_output.get(field_name)
    if not isinstance(raw_value, dict):
        _cleanup_temporary_directory(temporary_directory)

        raise calibration_integration_error(
            "native fit workflow returned an invalid output payload",
            entrypoint="fit",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"field": field_name},
        )

    return raw_value


def _cleanup_temporary_directory(
    temporary_directory: Optional[TemporaryDirectory[str]],
) -> None:
    if temporary_directory is not None:
        temporary_directory.cleanup()


__all__ = ["NativeFitWorkflowOutput", "run_prepared_fit_workflow"]
