from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Optional

from .contracts import (
    CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationStage,
    CalibrationStageExecutionStatus,
    _parse_calibration_execution_mode,
)
from ..errors import calibration_error, calibration_error
from ...contracts import (
    ContractModel,
    parse_json_object,
    require_non_empty_string,
    require_non_negative_integer,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)


@dataclass(frozen=True, slots=True)
class CalibrationCheckpointMetadata(ContractModel):
    checkpoint_reference: str
    completed_stage: CalibrationStage
    completed_stage_position: int
    execution_mode: CalibrationExecutionMode

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationCheckpointMetadata:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        return CalibrationCheckpointMetadata(
            checkpoint_reference=require_non_empty_string(
                raw_payload,
                "checkpoint_reference",
                f"{field_path}.checkpoint_reference",
                calibration_error,
            ),
            completed_stage=CalibrationStage(
                require_string(raw_payload, "completed_stage", f"{field_path}.completed_stage", calibration_error)
            ),
            completed_stage_position=require_non_negative_integer(
                raw_payload,
                "completed_stage_position",
                f"{field_path}.completed_stage_position",
                calibration_error,
            ),
            execution_mode=_parse_calibration_execution_mode(
                require_string(raw_payload, "execution_mode", f"{field_path}.execution_mode", calibration_error),
                field_path=f"{field_path}.execution_mode",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationFailureDetails(ContractModel):
    failure_kind: str
    error_message: Optional[str] = None
    stage: Optional[CalibrationStage] = None
    checkpoint_reference: Optional[str] = None
    artifact_reference: Optional[str] = None
    run_id: Optional[str] = None

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationFailureDetails:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_stage = raw_payload.get("stage")
        if raw_stage is not None and not isinstance(raw_stage, str):
            raise calibration_error(f"{field_path}.stage", "must be a string when provided")

        return CalibrationFailureDetails(
            failure_kind=require_non_empty_string(
                raw_payload,
                "failure_kind",
                f"{field_path}.failure_kind",
                calibration_error,
            ),
            error_message=raw_payload.get("error_message"),
            stage=CalibrationStage(raw_stage) if raw_stage is not None else None,
            checkpoint_reference=raw_payload.get("checkpoint_reference"),
            artifact_reference=raw_payload.get("artifact_reference"),
            run_id=raw_payload.get("run_id"),
        )


@dataclass(frozen=True, slots=True)
class CalibrationStageReport(ContractModel):
    stage_report_kind: CalibrationStage
    stage: CalibrationStage
    status: CalibrationStageExecutionStatus
    run_id: str
    execution_mode: CalibrationExecutionMode
    handoff_policy: str
    failure: Optional[CalibrationFailureDetails] = None

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationStageReport:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        return CalibrationStageReport(
            stage_report_kind=CalibrationStage(
                require_string(
                    raw_payload,
                    "stage_report_kind",
                    f"{field_path}.stage_report_kind",
                    calibration_error,
                )
            ),
            stage=CalibrationStage(
                require_string(raw_payload, "stage", f"{field_path}.stage", calibration_error)
            ),
            status=CalibrationStageExecutionStatus(
                require_string(raw_payload, "status", f"{field_path}.status", calibration_error)
            ),
            run_id=require_non_empty_string(raw_payload, "run_id", f"{field_path}.run_id", calibration_error),
            execution_mode=_parse_calibration_execution_mode(
                require_string(raw_payload, "execution_mode", f"{field_path}.execution_mode", calibration_error),
                field_path=f"{field_path}.execution_mode",
            ),
            handoff_policy=require_non_empty_string(
                raw_payload,
                "handoff_policy",
                f"{field_path}.handoff_policy",
                calibration_error,
            ),
            failure=(
                CalibrationFailureDetails.from_mapping(raw_payload["failure"], field_path=f"{field_path}.failure")
                if "failure" in raw_payload and raw_payload["failure"] is not None
                else None
            ),
        )


@dataclass(frozen=True, slots=True)
class StagedCalibrationResponse(ContractModel):
    """Typed staged-calibration response."""

    schema_version: str
    failure_triage_policy: str
    run_id: str
    execution_mode: CalibrationExecutionMode
    status: CalibrationStageExecutionStatus
    stage_reports: tuple[CalibrationStageReport, ...]
    emitted_checkpoints: tuple[CalibrationCheckpointMetadata, ...]
    diagnostic_artifact_references: tuple[dict[str, Any], ...] = ()
    diagnostic_artifact_bundle: Optional[dict[str, Any]] = None
    resumed_from_checkpoint_reference: Optional[str] = None
    resumed_from_last_successful_stage: Optional[CalibrationStage] = None
    started_from_stage: Optional[CalibrationStage] = None
    failure: Optional[CalibrationFailureDetails] = None

    def to_payload(self) -> dict[str, Any]:
        payload = super().to_payload()
        if not self.diagnostic_artifact_references:
            payload.pop("diagnostic_artifact_references", None)
        if self.diagnostic_artifact_bundle is None:
            payload.pop("diagnostic_artifact_bundle", None)

        return payload

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> StagedCalibrationResponse:
        if not isinstance(raw_payload, dict):
            raise calibration_error("staged_calibration_response", "must be an object")

        raw_stage_reports = require_sequence(
            raw_payload,
            "stage_reports",
            "stage_reports",
            calibration_error,
        )
        raw_emitted_checkpoints = require_sequence(
            raw_payload,
            "emitted_checkpoints",
            "emitted_checkpoints",
            calibration_error,
        )
        raw_diagnostic_artifact_references = raw_payload.get("diagnostic_artifact_references", ())
        if isinstance(raw_diagnostic_artifact_references, str) or not isinstance(
            raw_diagnostic_artifact_references,
            tuple | list,
        ):
            raise calibration_error(
                "diagnostic_artifact_references",
                "must be a sequence when provided",
            )
        raw_diagnostic_artifact_bundle = raw_payload.get("diagnostic_artifact_bundle")
        raw_resumed_stage = raw_payload.get("resumed_from_last_successful_stage")
        raw_started_stage = raw_payload.get("started_from_stage")

        if raw_diagnostic_artifact_bundle is not None and not isinstance(
            raw_diagnostic_artifact_bundle,
            dict,
        ):
            raise calibration_error(
                "diagnostic_artifact_bundle",
                "must be an object when provided",
            )
        normalized_diagnostic_artifact_references: list[dict[str, Any]] = []
        for reference_index, raw_reference in enumerate(raw_diagnostic_artifact_references):
            field_path = f"diagnostic_artifact_references[{reference_index}]"
            if not isinstance(raw_reference, dict):
                raise calibration_error(field_path, "must be an object")

            normalized_diagnostic_artifact_references.append(
                {
                    "artifact_family": require_non_empty_string(
                        raw_reference,
                        "artifact_family",
                        f"{field_path}.artifact_family",
                        calibration_error,
                    ),
                    "relative_path": require_non_empty_string(
                        raw_reference,
                        "relative_path",
                        f"{field_path}.relative_path",
                        calibration_error,
                    ),
                }
            )

        return StagedCalibrationResponse(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                "schema_version",
                calibration_error,
            ),
            failure_triage_policy=require_non_empty_string(
                raw_payload,
                "failure_triage_policy",
                "failure_triage_policy",
                calibration_error,
            ),
            run_id=require_non_empty_string(raw_payload, "run_id", "run_id", calibration_error),
            execution_mode=_parse_calibration_execution_mode(
                require_string(raw_payload, "execution_mode", "execution_mode", calibration_error),
                field_path="execution_mode",
            ),
            status=CalibrationStageExecutionStatus(
                require_string(raw_payload, "status", "status", calibration_error)
            ),
            stage_reports=tuple(
                CalibrationStageReport.from_mapping(
                    raw_stage_report,
                    field_path=f"stage_reports[{stage_report_index}]",
                )
                for stage_report_index, raw_stage_report in enumerate(raw_stage_reports)
            ),
            emitted_checkpoints=tuple(
                CalibrationCheckpointMetadata.from_mapping(
                    raw_checkpoint,
                    field_path=f"emitted_checkpoints[{checkpoint_index}]",
                )
                for checkpoint_index, raw_checkpoint in enumerate(raw_emitted_checkpoints)
            ),
            diagnostic_artifact_references=tuple(normalized_diagnostic_artifact_references),
            diagnostic_artifact_bundle=dict(raw_diagnostic_artifact_bundle)
            if raw_diagnostic_artifact_bundle is not None
            else None,
            resumed_from_checkpoint_reference=raw_payload.get("resumed_from_checkpoint_reference"),
            resumed_from_last_successful_stage=(
                CalibrationStage(raw_resumed_stage)
                if raw_resumed_stage is not None
                else None
            ),
            started_from_stage=CalibrationStage(raw_started_stage) if raw_started_stage is not None else None,
            failure=(
                CalibrationFailureDetails.from_mapping(raw_payload["failure"], field_path="failure")
                if "failure" in raw_payload and raw_payload["failure"] is not None
                else None
            ),
        )

def validate_staged_calibration_response(raw_response: dict[str, Any] | Any) -> StagedCalibrationResponse:
    return StagedCalibrationResponse.from_mapping(raw_response)


def prefer_calibration_failure_details(
    current_failure: Optional[CalibrationFailureDetails],
    candidate_failure: CalibrationFailureDetails,
) -> bool:
    if current_failure is None:
        return True

    current_has_error_message = current_failure.error_message is not None
    candidate_has_error_message = candidate_failure.error_message is not None

    if candidate_has_error_message and not current_has_error_message:
        return True
    if current_has_error_message and not candidate_has_error_message:
        return False

    current_is_generic = current_failure.failure_kind == "stage_execution_failed"
    candidate_is_generic = candidate_failure.failure_kind == "stage_execution_failed"

    if current_is_generic and not candidate_is_generic:
        return True
    if candidate_is_generic and not current_is_generic:
        return False

    return False


def select_staged_failure_details(
    response: StagedCalibrationResponse,
) -> tuple[Optional[CalibrationFailureDetails], Optional[CalibrationStage]]:
    selected_failure = response.failure
    failed_stage = (
        selected_failure.stage
        if selected_failure is not None and selected_failure.stage is not None
        else None
    )

    for stage_report in reversed(response.stage_reports):
        if stage_report.failure is None:
            continue

        if prefer_calibration_failure_details(selected_failure, stage_report.failure):
            selected_failure = stage_report.failure

        if failed_stage is None:
            failed_stage = stage_report.stage

    return selected_failure, failed_stage


def build_staged_failure_context(
    response: StagedCalibrationResponse,
) -> dict[str, str]:
    failure_context: dict[str, str] = {
        "run_id": response.run_id,
        "staged_status": response.status.value,
    }
    selected_failure, failed_stage = select_staged_failure_details(response)
    if selected_failure is None:
        return failure_context

    failure_context["failure_kind"] = selected_failure.failure_kind

    if failed_stage is not None:
        failure_context["failed_stage"] = failed_stage.value
    if selected_failure.error_message is not None:
        failure_context["failure_error_message"] = selected_failure.error_message
    if selected_failure.checkpoint_reference is not None:
        failure_context["checkpoint_reference"] = selected_failure.checkpoint_reference
    if selected_failure.artifact_reference is not None:
        failure_context["artifact_reference"] = selected_failure.artifact_reference

    return failure_context


def format_calibration_failure_summary(
    failure: Optional[CalibrationFailureDetails],
    *,
    failed_stage: Optional[CalibrationStage] = None,
) -> str:
    if failure is None:
        return "Calibration failed"

    stage_label = failed_stage.value if failed_stage is not None else None
    failure_kind = failure.failure_kind
    error_message = failure.error_message

    if stage_label is not None and error_message is not None:
        return f"Calibration failed at {stage_label} with {failure_kind}: {error_message}"
    if stage_label is not None:
        return f"Calibration failed at {stage_label} with {failure_kind}"
    if error_message is not None:
        return f"Calibration failed with {failure_kind}: {error_message}"

    return f"Calibration failed with {failure_kind}"


def format_staged_failure_summary(response: StagedCalibrationResponse) -> str:
    selected_failure, failed_stage = select_staged_failure_details(response)
    return format_calibration_failure_summary(
        selected_failure,
        failed_stage=failed_stage,
    )


def serialize_staged_calibration_response(response: StagedCalibrationResponse) -> str:
    return serialize_deterministic_json(response)


def deserialize_staged_calibration_response(payload_json: str) -> StagedCalibrationResponse:
    raw_payload = parse_json_object(payload_json, 'staged calibration response', calibration_error)
    return validate_staged_calibration_response(dict(raw_payload))


__all__ = [
    'CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION',
    'CalibrationCheckpointMetadata',
    'CalibrationFailureDetails',
    'CalibrationStageReport',
    'StagedCalibrationResponse',
    'build_staged_failure_context',
    'deserialize_staged_calibration_response',
    'format_calibration_failure_summary',
    'format_staged_failure_summary',
    'prefer_calibration_failure_details',
    'select_staged_failure_details',
    'serialize_staged_calibration_response',
    'validate_staged_calibration_response',
]
