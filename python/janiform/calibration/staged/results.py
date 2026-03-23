from dataclasses import dataclass
from typing import Any, Optional

from .contracts import (
    CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationStage,
    CalibrationStageExecutionStatus,
    _parse_calibration_execution_mode,
)
from .._errors import CALIBRATION_ERROR_FACTORY, _calibration_error
from ..._contracts import (
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
            raise _calibration_error(field_path, "must be an object")

        return CalibrationCheckpointMetadata(
            checkpoint_reference=require_non_empty_string(
                raw_payload,
                "checkpoint_reference",
                f"{field_path}.checkpoint_reference",
                CALIBRATION_ERROR_FACTORY,
            ),
            completed_stage=CalibrationStage(
                require_string(raw_payload, "completed_stage", f"{field_path}.completed_stage", CALIBRATION_ERROR_FACTORY)
            ),
            completed_stage_position=require_non_negative_integer(
                raw_payload,
                "completed_stage_position",
                f"{field_path}.completed_stage_position",
                CALIBRATION_ERROR_FACTORY,
            ),
            execution_mode=_parse_calibration_execution_mode(
                require_string(raw_payload, "execution_mode", f"{field_path}.execution_mode", CALIBRATION_ERROR_FACTORY),
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
            raise _calibration_error(field_path, "must be an object")

        raw_stage = raw_payload.get("stage")
        if raw_stage is not None and not isinstance(raw_stage, str):
            raise _calibration_error(f"{field_path}.stage", "must be a string when provided")

        return CalibrationFailureDetails(
            failure_kind=require_non_empty_string(
                raw_payload,
                "failure_kind",
                f"{field_path}.failure_kind",
                CALIBRATION_ERROR_FACTORY,
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
            raise _calibration_error(field_path, "must be an object")

        return CalibrationStageReport(
            stage_report_kind=CalibrationStage(
                require_string(
                    raw_payload,
                    "stage_report_kind",
                    f"{field_path}.stage_report_kind",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            stage=CalibrationStage(
                require_string(raw_payload, "stage", f"{field_path}.stage", CALIBRATION_ERROR_FACTORY)
            ),
            status=CalibrationStageExecutionStatus(
                require_string(raw_payload, "status", f"{field_path}.status", CALIBRATION_ERROR_FACTORY)
            ),
            run_id=require_non_empty_string(raw_payload, "run_id", f"{field_path}.run_id", CALIBRATION_ERROR_FACTORY),
            execution_mode=_parse_calibration_execution_mode(
                require_string(raw_payload, "execution_mode", f"{field_path}.execution_mode", CALIBRATION_ERROR_FACTORY),
                field_path=f"{field_path}.execution_mode",
            ),
            handoff_policy=require_non_empty_string(
                raw_payload,
                "handoff_policy",
                f"{field_path}.handoff_policy",
                CALIBRATION_ERROR_FACTORY,
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
            raise _calibration_error("staged_calibration_response", "must be an object")

        raw_stage_reports = require_sequence(
            raw_payload,
            "stage_reports",
            "stage_reports",
            CALIBRATION_ERROR_FACTORY,
        )
        raw_emitted_checkpoints = require_sequence(
            raw_payload,
            "emitted_checkpoints",
            "emitted_checkpoints",
            CALIBRATION_ERROR_FACTORY,
        )
        raw_diagnostic_artifact_references = raw_payload.get("diagnostic_artifact_references", ())
        if isinstance(raw_diagnostic_artifact_references, str) or not isinstance(
            raw_diagnostic_artifact_references,
            tuple | list,
        ):
            raise _calibration_error(
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
            raise _calibration_error(
                "diagnostic_artifact_bundle",
                "must be an object when provided",
            )
        normalized_diagnostic_artifact_references: list[dict[str, Any]] = []
        for reference_index, raw_reference in enumerate(raw_diagnostic_artifact_references):
            field_path = f"diagnostic_artifact_references[{reference_index}]"
            if not isinstance(raw_reference, dict):
                raise _calibration_error(field_path, "must be an object")

            normalized_diagnostic_artifact_references.append(
                {
                    "artifact_family": require_non_empty_string(
                        raw_reference,
                        "artifact_family",
                        f"{field_path}.artifact_family",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    "relative_path": require_non_empty_string(
                        raw_reference,
                        "relative_path",
                        f"{field_path}.relative_path",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                }
            )

        return StagedCalibrationResponse(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                "schema_version",
                CALIBRATION_ERROR_FACTORY,
            ),
            failure_triage_policy=require_non_empty_string(
                raw_payload,
                "failure_triage_policy",
                "failure_triage_policy",
                CALIBRATION_ERROR_FACTORY,
            ),
            run_id=require_non_empty_string(raw_payload, "run_id", "run_id", CALIBRATION_ERROR_FACTORY),
            execution_mode=_parse_calibration_execution_mode(
                require_string(raw_payload, "execution_mode", "execution_mode", CALIBRATION_ERROR_FACTORY),
                field_path="execution_mode",
            ),
            status=CalibrationStageExecutionStatus(
                require_string(raw_payload, "status", "status", CALIBRATION_ERROR_FACTORY)
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


def serialize_staged_calibration_response(response: StagedCalibrationResponse) -> str:
    return serialize_deterministic_json(response)


def deserialize_staged_calibration_response(payload_json: str) -> StagedCalibrationResponse:
    raw_payload = parse_json_object(payload_json, 'staged calibration response', CALIBRATION_ERROR_FACTORY)
    return validate_staged_calibration_response(dict(raw_payload))


__all__ = [
    'CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION',
    'CalibrationCheckpointMetadata',
    'CalibrationFailureDetails',
    'CalibrationStageReport',
    'StagedCalibrationResponse',
    'deserialize_staged_calibration_response',
    'serialize_staged_calibration_response',
    'validate_staged_calibration_response',
]
