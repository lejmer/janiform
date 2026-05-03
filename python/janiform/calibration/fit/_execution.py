"""Internal fit-execution orchestration."""

from __future__ import annotations

from collections.abc import Callable, Mapping
from contextlib import nullcontext, redirect_stdout
from datetime import datetime
from io import StringIO
from pathlib import Path
from typing import Any, Optional, Union

from ..._workflow_progress import (
    WorkflowProgressMode,
    resolve_workflow_progress_mode,
)
from ..errors import (
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    calibration_error,
    calibration_execution_error,
    calibration_integration_error,
)
from ..market_data.contracts import (
    CalibrationFlatCarryInput,
    CalibrationInputFileFormat,
    CalibrationMarketDataWorkflowScope,
)
from ..market_data.workflow import (
    MarketDataCalibrationPreparationDiagnostics,
    MarketDataCalibrationPreparationProvenance,
)
from ..presets.resolution import ResolvedPresetWorkflowConfiguration
from ..sessions.contracts import MarketDataSessionResolution
from ..staged import (
    CalibrationResumeSettings,
    CalibrationStageExecutionStatus,
)
from ..staged.request import StagedCalibrationRequest
from ..staged.results import build_staged_failure_context, format_staged_failure_summary
from ._preparation.contracts import FitExecutionConfiguration
from ._runtime.metadata import normalize_model_parameters
from ._runtime.workflow import NativeFitWorkflowOutput, run_fit_workflow
from .report import JaniformFitReport
from ...model import JaniformModel


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
    progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
) -> Union[JaniformModel, tuple[Optional[JaniformModel], JaniformFitReport]]:
    _validate_fit_input_labels(
        tick_data_provenance_label=tick_data_provenance_label,
        option_surface_provenance_label=option_surface_provenance_label,
    )

    native_output = _run_fit_with_native_progress(
        configuration,
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
        progress_callback=progress_callback,
    )

    fit_report, model = _build_fit_outputs_from_native(native_output)

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


def _run_fit_with_native_progress(
    configuration: FitExecutionConfiguration,
    *,
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
    progress_callback: Optional[Callable[[Mapping[str, Any]], None]],
) -> NativeFitWorkflowOutput:
    suppression_context = redirect_stdout(StringIO()) if configuration.silent else nullcontext()
    native_output: Optional[NativeFitWorkflowOutput] = None
    resolved_progress_mode = resolve_workflow_progress_mode(
        WorkflowProgressMode.AUTO,
        default_mode=(
            WorkflowProgressMode.NONE
            if configuration.silent
            else WorkflowProgressMode.HUMAN
        ),
    )

    with suppression_context:
        try:
            native_output = run_fit_workflow(
                configuration,
                output_dir=output_dir,
                resume_settings=resume_settings,
                run_id=run_id,
                tick_data_path=tick_data_path,
                tick_data_format=tick_data_format,
                tick_data_provenance_label=tick_data_provenance_label,
                option_surface_path=option_surface_path,
                option_surface_format=option_surface_format,
                option_surface_provenance_label=option_surface_provenance_label,
                option_surface_valuation_time_utc=option_surface_valuation_time_utc,
                carry=carry,
                progress_mode=resolved_progress_mode,
                progress_callback=progress_callback,
            )
        finally:
            if native_output is not None and native_output.temporary_directory is not None:
                native_output.temporary_directory.cleanup()

    if native_output is None:
        raise RuntimeError("Fit execution did not produce a native workflow response")

    return native_output


def _build_fit_outputs_from_native(
    native_output: NativeFitWorkflowOutput,
) -> tuple[JaniformFitReport, Optional[JaniformModel]]:
    staged_response = native_output.staged_response
    fit_report_payload = native_output.fit_report_payload
    preparation_payload = fit_report_payload["preparation"]
    staged_request_payload = fit_report_payload["staged_request"]

    fit_report = JaniformFitReport(
        run_id=str(fit_report_payload["run_id"]),
        workflow_scope=CalibrationMarketDataWorkflowScope(
            str(fit_report_payload["workflow_scope"])
        ),
        success=bool(fit_report_payload["success"]),
        resolved_configuration=ResolvedPresetWorkflowConfiguration.from_mapping(
            preparation_payload["resolved_configuration"],
            field_path="fit_report.preparation.resolved_configuration",
        ),
        session_resolution=MarketDataSessionResolution.from_mapping(
            preparation_payload["session_resolution"],
            field_path="fit_report.preparation.session_resolution",
        ),
        preparation_diagnostics=(
            MarketDataCalibrationPreparationDiagnostics.from_mapping(
                preparation_payload["diagnostics"]
            )
        ),
        preparation_provenance=(
            MarketDataCalibrationPreparationProvenance.from_mapping(
                preparation_payload["provenance"]
            )
        ),
        staged_request=StagedCalibrationRequest.from_mapping(
            staged_request_payload,
            field_path_prefix="fit_report.staged_request",
        ),
        prepared_data_reference=fit_report_payload.get("prepared_data_reference"),
        prepared_payload_path=fit_report_payload.get("prepared_payload_path"),
        staged_execution_input_path=fit_report_payload.get("staged_execution_input_path"),
        staged_response=staged_response,
    )

    if staged_response.status != CalibrationStageExecutionStatus.SUCCESS:
        return fit_report, None

    model_parameters, model_metadata = _validated_native_model_payload(
        native_output.model_payload
    )

    model = JaniformModel(
        run_id=fit_report.run_id,
        workflow_scope=fit_report.workflow_scope,
        parameters=model_parameters,
        metadata=model_metadata,
        resolved_configuration=fit_report.resolved_configuration,
        session_resolution=fit_report.session_resolution,
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
