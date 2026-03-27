from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import TYPE_CHECKING, Any, Optional

from .contracts import (
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationExecutionMode,
)
from .._errors import CALIBRATION_ERROR_FACTORY, _calibration_error
from ...contracts import (
    ContractModel,
    parse_json_object,
    require_mapping,
    require_non_empty_string,
    require_string,
    serialize_deterministic_json,
)

if TYPE_CHECKING:
    from .request import StagedCalibrationRequest


class CalibrationPreparedExecutionPayloadFileFormat(StrEnum):
    JSON = "json"


@dataclass(frozen=True, slots=True)
class CalibrationPreparedExecutionPayloadFileReference(ContractModel):
    path: str
    file_format: CalibrationPreparedExecutionPayloadFileFormat = (
        CalibrationPreparedExecutionPayloadFileFormat.JSON
    )

    def __post_init__(self) -> None:
        if not self.path.strip():
            raise _calibration_error("prepared_payload_file.path", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationPreparedExecutionPayloadFileReference:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        try:
            file_format = CalibrationPreparedExecutionPayloadFileFormat(
                require_string(
                    raw_payload,
                    "file_format",
                    f"{field_path}.file_format",
                    CALIBRATION_ERROR_FACTORY,
                )
            )
        except ValueError as e:
            raise _calibration_error(
                f"{field_path}.file_format",
                "must be a CalibrationPreparedExecutionPayloadFileFormat value",
            ) from e

        return CalibrationPreparedExecutionPayloadFileReference(
            path=require_non_empty_string(
                raw_payload,
                "path",
                f"{field_path}.path",
                CALIBRATION_ERROR_FACTORY,
            ),
            file_format=file_format,
        )


@dataclass(frozen=True, slots=True)
class CalibrationStagedExecutionRuntimeResources(ContractModel):
    disk_checkpoint_directory: Optional[str] = None

    def __post_init__(self) -> None:
        if self.disk_checkpoint_directory is not None and not self.disk_checkpoint_directory.strip():
            raise _calibration_error(
                "runtime_resources.disk_checkpoint_directory",
                "must be a non-empty string when provided",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationStagedExecutionRuntimeResources:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_directory = raw_payload.get("disk_checkpoint_directory")
        if raw_directory is not None and not isinstance(raw_directory, str):
            raise _calibration_error(
                f"{field_path}.disk_checkpoint_directory",
                "must be a string when provided",
            )

        return CalibrationStagedExecutionRuntimeResources(
            disk_checkpoint_directory=raw_directory,
        )


@dataclass(frozen=True, slots=True)
class StagedCalibrationExecutionInput(ContractModel):
    schema_version: str
    request: StagedCalibrationRequest
    prepared_payload_file: CalibrationPreparedExecutionPayloadFileReference
    runtime_resources: Optional[CalibrationStagedExecutionRuntimeResources] = None

    def __post_init__(self) -> None:
        if self.schema_version != CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION:
            raise _calibration_error(
                "schema_version",
                f"must be `{CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION}`",
            )

        runtime_resources = self.runtime_resources
        if (
            self.request.execution_mode == CalibrationExecutionMode.MEMORY
            and runtime_resources is not None
            and runtime_resources.disk_checkpoint_directory is not None
        ):
            raise _calibration_error(
                "runtime_resources.disk_checkpoint_directory",
                "is only supported when execution_mode=disk",
            )

        if (
            self.request.execution_mode == CalibrationExecutionMode.DISK
            and (
                runtime_resources is None
                or runtime_resources.disk_checkpoint_directory is None
            )
        ):
            raise _calibration_error(
                "runtime_resources.disk_checkpoint_directory",
                "is required when execution_mode=disk",
            )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> StagedCalibrationExecutionInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error("staged_calibration_execution_input", "must be an object")

        from .request import StagedCalibrationRequest

        return StagedCalibrationExecutionInput(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                "schema_version",
                CALIBRATION_ERROR_FACTORY,
            ),
            request=StagedCalibrationRequest.from_mapping(
                require_mapping(
                    raw_payload,
                    "request",
                    "request",
                    CALIBRATION_ERROR_FACTORY,
                ),
            ),
            prepared_payload_file=CalibrationPreparedExecutionPayloadFileReference.from_mapping(
                require_mapping(
                    raw_payload,
                    "prepared_payload_file",
                    "prepared_payload_file",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path="prepared_payload_file",
            ),
            runtime_resources=(
                CalibrationStagedExecutionRuntimeResources.from_mapping(
                    raw_payload["runtime_resources"],
                    field_path="runtime_resources",
                )
                if "runtime_resources" in raw_payload and raw_payload["runtime_resources"] is not None
                else None
            ),
        )


def validate_staged_calibration_execution_input(
    raw_execution_input: dict[str, Any] | Any,
) -> StagedCalibrationExecutionInput:
    return StagedCalibrationExecutionInput.from_mapping(raw_execution_input)


def serialize_staged_calibration_execution_input(
    execution_input: StagedCalibrationExecutionInput,
) -> str:
    return serialize_deterministic_json(execution_input)


def deserialize_staged_calibration_execution_input(
    payload_json: str,
) -> StagedCalibrationExecutionInput:
    raw_payload = parse_json_object(
        payload_json,
        "staged calibration execution input",
        CALIBRATION_ERROR_FACTORY,
    )
    return validate_staged_calibration_execution_input(dict(raw_payload))


__all__ = [
    "CalibrationPreparedExecutionPayloadFileFormat",
    "CalibrationPreparedExecutionPayloadFileReference",
    "CalibrationStagedExecutionRuntimeResources",
    "StagedCalibrationExecutionInput",
    "deserialize_staged_calibration_execution_input",
    "serialize_staged_calibration_execution_input",
    "validate_staged_calibration_execution_input",
]
