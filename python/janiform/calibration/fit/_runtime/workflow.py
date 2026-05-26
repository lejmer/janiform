from __future__ import annotations

from collections.abc import Callable, Mapping
from dataclasses import asdict
from dataclasses import dataclass
from datetime import datetime
from enum import Enum
from pathlib import Path
from tempfile import TemporaryDirectory
from types import ModuleType
from typing import Any, Optional

from ...._native_api import native_error_parts, native_exception_matches, native_module
from ...._native_progress import native_terminal_progress_mode
from ...._workflow_progress import WorkflowProgressMode
from ...errors import (
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    CalibrationValidationError,
    calibration_error,
    calibration_execution_error,
    calibration_integration_error,
)
from ...market_data.contracts import CalibrationFlatCarryInput, CalibrationInputFileFormat
from ...staged import CalibrationResumeSettings
from ...staged.results import StagedCalibrationResponse, validate_staged_calibration_response
from .._inference import normalization_overrides_from_policy
from .._preparation.contracts import FitExecutionConfiguration


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


def run_fit_workflow(
    configuration: FitExecutionConfiguration,
    *,
    output_dir: Optional[str | Path],
    resume_settings: Optional[CalibrationResumeSettings],
    run_id: Optional[str],
    tick_data_path: str | Path,
    tick_data_format: Optional[CalibrationInputFileFormat],
    tick_data_provenance_label: str,
    option_surface_path: Optional[str | Path],
    option_surface_format: CalibrationInputFileFormat,
    option_surface_provenance_label: str,
    option_surface_valuation_time_utc: Optional[datetime],
    carry: Optional[CalibrationFlatCarryInput],
    progress_mode: WorkflowProgressMode,
    progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
) -> NativeFitWorkflowOutput:
    temporary_directory: Optional[TemporaryDirectory[str]] = None
    workflow_output_dir: Optional[str | Path] = output_dir
    if workflow_output_dir is None:
        temporary_directory = TemporaryDirectory()
        workflow_output_dir = temporary_directory.name

    request_payload = {
        "preparation_request": _fit_preparation_request_payload(
            configuration,
            run_id=run_id,
            tick_data_path=tick_data_path,
            tick_data_format=tick_data_format,
            tick_data_provenance_label=tick_data_provenance_label,
            option_surface_path=option_surface_path,
            option_surface_format=option_surface_format,
            option_surface_provenance_label=option_surface_provenance_label,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
        ),
        "output_dir": str(Path(workflow_output_dir)),
        "resume_settings": None if resume_settings is None else resume_settings.to_payload(),
        "progress_mode": native_terminal_progress_mode(progress_mode),
        "emit_machine_events": progress_mode == WorkflowProgressMode.JSONL,
    }

    native = _native_module_for_fit_workflow()
    try:
        raw_output = native.run_fit_workflow(
            request_payload,
            progress_callback,
        )
    except Exception as error:
        _cleanup_temporary_directory(temporary_directory)

        _raise_fit_workflow_error(native, error)

    if not isinstance(raw_output, dict):
        _cleanup_temporary_directory(temporary_directory)

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


def prepare_fit_workflow(
    configuration: FitExecutionConfiguration,
    *,
    working_directory: str | Path,
    run_id: Optional[str],
    tick_data_path: str | Path,
    tick_data_format: Optional[CalibrationInputFileFormat],
    tick_data_provenance_label: str,
    option_surface_path: Optional[str | Path],
    option_surface_format: CalibrationInputFileFormat,
    option_surface_provenance_label: str,
    option_surface_valuation_time_utc: Optional[datetime],
    carry: Optional[CalibrationFlatCarryInput],
) -> dict[str, Any]:
    request_payload = {
        "preparation_request": _fit_preparation_request_payload(
            configuration,
            run_id=run_id,
            tick_data_path=tick_data_path,
            tick_data_format=tick_data_format,
            tick_data_provenance_label=tick_data_provenance_label,
            option_surface_path=option_surface_path,
            option_surface_format=option_surface_format,
            option_surface_provenance_label=option_surface_provenance_label,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
        ),
        "working_directory": str(Path(working_directory)),
    }

    native = _native_module_for_fit_workflow()
    try:
        raw_output = native.prepare_fit_workflow(request_payload)
    except Exception as error:
        _raise_fit_workflow_error(native, error)

    if not isinstance(raw_output, dict):
        raise calibration_integration_error(
            "native fit preparation returned an invalid output payload",
            entrypoint="fit",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_output).__name__},
        )

    return raw_output


def _fit_preparation_request_payload(
    configuration: FitExecutionConfiguration,
    *,
    run_id: Optional[str],
    tick_data_path: str | Path,
    tick_data_format: Optional[CalibrationInputFileFormat],
    tick_data_provenance_label: str,
    option_surface_path: Optional[str | Path],
    option_surface_format: CalibrationInputFileFormat,
    option_surface_provenance_label: str,
    option_surface_valuation_time_utc: Optional[datetime],
    carry: Optional[CalibrationFlatCarryInput],
) -> dict[str, Any]:
    return {
        "run_id": run_id,
        "tick_data_path": str(Path(tick_data_path)),
        "tick_data_format": _enum_value_or_none(tick_data_format),
        "tick_data_provenance_label": tick_data_provenance_label,
        "option_surface_path": (
            str(Path(option_surface_path)) if option_surface_path is not None else None
        ),
        "option_surface_format": _enum_value(option_surface_format),
        "option_surface_provenance_label": option_surface_provenance_label,
        "option_surface_valuation_time_utc": (
            option_surface_valuation_time_utc.isoformat()
            if option_surface_valuation_time_utc is not None
            else None
        ),
        "carry": None if carry is None else carry.to_payload(),
        "configuration": _fit_preparation_configuration_payload(configuration),
    }


def _fit_preparation_configuration_payload(
    configuration: FitExecutionConfiguration,
) -> dict[str, Any]:
    preparation = configuration.preparation

    return {
        "workflow_scope_override": _enum_value_or_none(preparation.workflow_scope_override),
        "tick_data_column_mapping": (
            None
            if preparation.tick_data_column_mapping is None
            else preparation.tick_data_column_mapping.to_payload()
        ),
        "normalization_overrides": asdict(
            normalization_overrides_from_policy(preparation.normalization_policy)
        ),
        "option_surface_column_mapping": (
            None
            if preparation.option_surface_column_mapping is None
            else preparation.option_surface_column_mapping.to_payload()
        ),
        "output_preferences": preparation.output_preferences.to_payload(),
        "optimizer_settings_by_stage": [
            settings.to_payload()
            for settings in preparation.optimizer_settings_by_stage
        ],
        "preprocessing_settings": (
            None
            if preparation.preprocessing_settings is None
            else preparation.preprocessing_settings.to_payload()
        ),
        "mean_reversion": (
            None
            if preparation.mean_reversion is None
            else preparation.mean_reversion.to_payload()
        ),
        "pricing_interval_schedule_policy": (
            preparation.pricing_interval_schedule_policy.to_payload()
        ),
        "dynamics_search_restart_candidate_budget": (
            preparation.dynamics_search_restart_candidate_budget
        ),
        "calibration_effort": preparation.calibration_effort.to_payload(),
        "treat_calibration_warnings_as_errors": (
            preparation.treat_calibration_warnings_as_errors
        ),
        "joint_refinement_option_refinement_candidate_count_override": (
            preparation.joint_refinement_option_refinement_candidate_count_override
        ),
        "joint_refinement_option_refinement_wall_clock_budget_millis": (
            preparation.joint_refinement_option_refinement_wall_clock_budget_millis
        ),
        "joint_refinement_audit_option_pricing_enabled": (
            preparation.joint_refinement_audit_option_pricing_enabled
        ),
        "stylized_fact_huber_cutoff": preparation.stylized_fact_huber_cutoff,
        "stylized_fact_admissibility_z_threshold": (
            preparation.stylized_fact_admissibility_z_threshold
        ),
        "default_endogenous_mean_reversion_configuration": (
            preparation.default_endogenous_mean_reversion_configuration.to_payload()
        ),
        "market_profile_reference": (
            None
            if preparation.market_profile_reference is None
            else preparation.market_profile_reference.to_payload()
        ),
        "known_news_reference": (
            None
            if preparation.known_news_reference is None
            else preparation.known_news_reference.to_payload()
        ),
        "session_reference": (
            None
            if preparation.effective_session_reference is None
            else preparation.effective_session_reference.to_payload()
        ),
        "requires_session_inference": preparation.requires_session_inference,
        "exchange_code_for_inference": preparation.exchange_code_for_inference,
        "session_timezone": preparation.session_timezone,
        "registry": preparation.registry.native_resolution_payload(),
        "cpu_count": preparation.cpu_count,
    }


def _enum_value(value: Any) -> Any:
    if isinstance(value, Enum):
        return value.value

    return value


def _enum_value_or_none(value: Any) -> Any:
    if value is None:
        return None

    return _enum_value(value)


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
    if native_exception_matches(native, error, "CalibrationValidationError"):
        field_path, message = native_error_parts(str(error), default_field_path="fit")
        raise calibration_error(field_path, message) from error

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


__all__ = ["NativeFitWorkflowOutput", "prepare_fit_workflow", "run_fit_workflow"]
