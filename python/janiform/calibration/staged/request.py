from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Optional

from ..errors import calibration_error, calibration_error
from .contracts import (
    DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE,
    CalibrationExecutionMode,
    CalibrationHandoffTransport,
    CalibrationResumeSettings,
    CalibrationResumeSource,
    CalibrationRunDataInputs,
    CalibrationRunSeedSettings,
    CalibrationStage,
    CalibrationStageHandoff,
    CalibrationStageSolverSettings,
    _parse_calibration_execution_mode,
)
from ...contracts import (
    ContractModel,
    parse_json_object,
    require_mapping,
    require_non_empty_string,
    require_sequence,
    serialize_deterministic_json,
)


@dataclass(frozen=True, slots=True)
class StagedCalibrationRequest(ContractModel):
    """Typed staged-calibration request with Python default `execution_mode=memory`."""

    run_id: str
    data_inputs: CalibrationRunDataInputs
    execution_mode: CalibrationExecutionMode = CalibrationExecutionMode.MEMORY
    selected_stages: tuple[CalibrationStage, ...] = ()
    seed_settings: Optional[CalibrationRunSeedSettings] = None
    optimizer_settings_by_stage: tuple[CalibrationStageSolverSettings, ...] = ()
    stage_handoffs: tuple[CalibrationStageHandoff, ...] = ()
    resume_settings: Optional[CalibrationResumeSettings] = None

    def __post_init__(self) -> None:
        if not self.run_id.strip():
            raise calibration_error("run_id", "must be a non-empty string")

        if len(self.selected_stages) == 0:
            raise calibration_error("selected_stages", "must include at least one calibration stage")

        if self.seed_settings is None:
            raise calibration_error("seed_settings", "is required")

        selected_stage_set = set(self.selected_stages)
        if len(selected_stage_set) != len(self.selected_stages):
            raise calibration_error("selected_stages", "must not contain duplicates")

        configured_stage_set = {entry.stage for entry in self.optimizer_settings_by_stage}
        if configured_stage_set != selected_stage_set:
            raise calibration_error(
                "optimizer_settings_by_stage",
                "must include exactly one solver-settings entry for each selected stage",
            )

        for stage_seed_override in self.seed_settings.stage_seed_overrides:
            if stage_seed_override.stage not in selected_stage_set:
                raise calibration_error(
                    "seed_settings.stage_seed_overrides",
                    f"contains stage '{stage_seed_override.stage.value}' that is not selected",
                )

        for stage_handoff in self.stage_handoffs:
            if stage_handoff.from_stage not in selected_stage_set:
                raise calibration_error(
                    "stage_handoffs.from_stage",
                    f"'{stage_handoff.from_stage.value}' must be selected",
                )

            if stage_handoff.to_stage not in selected_stage_set:
                raise calibration_error(
                    "stage_handoffs.to_stage",
                    f"'{stage_handoff.to_stage.value}' must be selected",
                )

            if (
                self.execution_mode == CalibrationExecutionMode.MEMORY
                and stage_handoff.transport == CalibrationHandoffTransport.PERSISTED_ARTIFACT_REFERENCE
            ):
                raise calibration_error(
                    "stage_handoffs.transport",
                    "persisted_artifact_reference requires execution_mode=disk",
                )

        if self.resume_settings is not None:
            if (
                self.execution_mode == CalibrationExecutionMode.MEMORY
                and self.resume_settings.source != CalibrationResumeSource.SAME_PROCESS
            ):
                raise calibration_error(
                    "resume_settings.source",
                    "persisted_checkpoint_reference is only supported when execution_mode=disk",
                )

            if (
                self.execution_mode == CalibrationExecutionMode.DISK
                and self.resume_settings.source == CalibrationResumeSource.SAME_PROCESS
            ):
                raise calibration_error(
                    "resume_settings.source",
                    "same_process is only supported when execution_mode=memory",
                )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path_prefix: Optional[str] = None,
    ) -> StagedCalibrationRequest:
        if not isinstance(raw_payload, dict):
            field_path = (
                f"{field_path_prefix}.staged_calibration_request"
                if field_path_prefix is not None
                else "staged_calibration_request"
            )
            raise calibration_error(field_path, "must be an object")

        def prefixed(field_name: str) -> str:
            if field_path_prefix is None:
                return field_name

            return f"{field_path_prefix}.{field_name}"

        execution_mode = _parse_calibration_execution_mode(
            raw_payload.get("execution_mode", DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE),
            field_path=prefixed("execution_mode"),
        )

        raw_selected_stages = require_sequence(
            raw_payload,
            "selected_stages",
            prefixed("selected_stages"),
            calibration_error,
        )
        raw_solver_settings = require_sequence(
            raw_payload,
            "optimizer_settings_by_stage",
            prefixed("optimizer_settings_by_stage"),
            calibration_error,
        )

        raw_stage_handoffs = raw_payload.get("stage_handoffs", [])
        if isinstance(raw_stage_handoffs, str) or not isinstance(raw_stage_handoffs, list):
            raise calibration_error(prefixed("stage_handoffs"), "must be a sequence")

        return StagedCalibrationRequest(
            run_id=require_non_empty_string(
                raw_payload,
                "run_id",
                prefixed("run_id"),
                calibration_error,
            ),
            data_inputs=CalibrationRunDataInputs.from_mapping(
                require_mapping(
                    raw_payload,
                    "data_inputs",
                    prefixed("data_inputs"),
                    calibration_error,
                ),
                field_path=prefixed("data_inputs"),
            ),
            execution_mode=execution_mode,
            selected_stages=tuple(CalibrationStage(cast_stage) for cast_stage in raw_selected_stages),
            seed_settings=CalibrationRunSeedSettings.from_mapping(
                require_mapping(
                    raw_payload,
                    "seed_settings",
                    prefixed("seed_settings"),
                    calibration_error,
                ),
                field_path=prefixed("seed_settings"),
            ),
            optimizer_settings_by_stage=tuple(
                CalibrationStageSolverSettings.from_mapping(
                    raw_setting,
                    field_path=f"{prefixed('optimizer_settings_by_stage')}[{setting_index}]",
                )
                for setting_index, raw_setting in enumerate(raw_solver_settings)
            ),
            stage_handoffs=tuple(
                CalibrationStageHandoff.from_mapping(
                    raw_handoff,
                    field_path=f"{prefixed('stage_handoffs')}[{handoff_index}]",
                )
                for handoff_index, raw_handoff in enumerate(raw_stage_handoffs)
            ),
            resume_settings=(
                CalibrationResumeSettings.from_mapping(
                    raw_payload["resume_settings"],
                    field_path=prefixed("resume_settings"),
                )
                if "resume_settings" in raw_payload and raw_payload["resume_settings"] is not None
                else None
            ),
        )


def validate_staged_calibration_request(raw_request: dict[str, Any] | Any) -> StagedCalibrationRequest:
    return StagedCalibrationRequest.from_mapping(raw_request)


def serialize_staged_calibration_request(request: StagedCalibrationRequest) -> str:
    return serialize_deterministic_json(request)


def deserialize_staged_calibration_request(payload_json: str) -> StagedCalibrationRequest:
    raw_payload = parse_json_object(payload_json, "staged calibration request", calibration_error)
    return validate_staged_calibration_request(dict(raw_payload))


__all__ = [
    "StagedCalibrationRequest",
    "deserialize_staged_calibration_request",
    "serialize_staged_calibration_request",
    "validate_staged_calibration_request",
]
