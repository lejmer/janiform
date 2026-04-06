from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Optional

from .._errors import CALIBRATION_ERROR_FACTORY, _calibration_error
from ...contracts import (
    ContractModel,
    require_mapping,
    require_non_empty_string,
    require_non_negative_finite_number,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_integer,
    require_string,
)


CALIBRATION_RUN_SPEC_SCHEMA_VERSION = "calibration.orchestration.run_spec.v1"
CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION = "calibration.orchestration.staged_execution_input.v1"
CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION = "calibration.orchestration.staged_run.report.v1"
CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY = (
    "cli_defaults_to_disk_python_importable_api_defaults_to_memory.v1"
)
# Public Python API default, and callers can override it with an explicit execution mode.
DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE = "memory"


class CalibrationExecutionMode(StrEnum):
    MEMORY = "memory"
    DISK = "disk"


class CalibrationStage(StrEnum):
    SEASONALITY = "seasonality"
    HAWKES = "hawkes"
    RETURN_COMPONENTS = "return_components"
    MEAN_REVERSION = "mean_reversion"
    JOINT_REFINEMENT = "joint_refinement"


class CalibrationStageExecutionStatus(StrEnum):
    SUCCESS = "success"
    FAILURE = "failure"


class CalibrationHandoffTransport(StrEnum):
    IN_MEMORY_OUTPUT = "in_memory_output"
    PERSISTED_ARTIFACT_REFERENCE = "persisted_artifact_reference"


class CalibrationResumeSource(StrEnum):
    SAME_PROCESS = "same_process"
    PERSISTED_CHECKPOINT_REFERENCE = "persisted_checkpoint_reference"


@dataclass(frozen=True, slots=True)
class CalibrationRunDataInputs(ContractModel):
    """Dataset references consumed by staged calibration."""

    exchange_code: str
    trade_events_data_reference: str
    option_surface_data_reference: Optional[str] = None

    def __post_init__(self) -> None:
        if not self.exchange_code.strip():
            raise _calibration_error("exchange_code", "must be a non-empty string")

        if not self.trade_events_data_reference.strip():
            raise _calibration_error("trade_events_data_reference", "must be a non-empty string")

        if self.option_surface_data_reference is not None and not self.option_surface_data_reference.strip():
            raise _calibration_error(
                "option_surface_data_reference",
                "must be non-empty when provided",
            )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationRunDataInputs:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        option_surface_data_reference = raw_payload.get("option_surface_data_reference")
        if option_surface_data_reference is not None and not isinstance(option_surface_data_reference, str):
            raise _calibration_error(
                f"{field_path}.option_surface_data_reference",
                "must be a string when provided",
            )

        return CalibrationRunDataInputs(
            exchange_code=require_non_empty_string(
                raw_payload,
                "exchange_code",
                f"{field_path}.exchange_code",
                CALIBRATION_ERROR_FACTORY,
            ),
            trade_events_data_reference=require_non_empty_string(
                raw_payload,
                "trade_events_data_reference",
                f"{field_path}.trade_events_data_reference",
                CALIBRATION_ERROR_FACTORY,
            ),
            option_surface_data_reference=option_surface_data_reference,
        )


@dataclass(frozen=True, slots=True)
class CalibrationStageSeedOverride(ContractModel):
    stage: CalibrationStage
    seed: int

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationStageSeedOverride:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_stage = require_string(raw_payload, "stage", f"{field_path}.stage", CALIBRATION_ERROR_FACTORY)
        try:
            stage = CalibrationStage(raw_stage)
        except ValueError as e:
            raise _calibration_error(
                f"{field_path}.stage",
                "must be one of: seasonality, hawkes, return_components, mean_reversion, joint_refinement",
            ) from e

        return CalibrationStageSeedOverride(
            stage=stage,
            seed=require_non_negative_integer(
                raw_payload,
                "seed",
                f"{field_path}.seed",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationRunSeedSettings(ContractModel):
    global_seed: int
    stage_seed_overrides: tuple[CalibrationStageSeedOverride, ...] = ()

    def __post_init__(self) -> None:
        if self.global_seed < 0:
            raise _calibration_error("global_seed", "must be non-negative")

        seen_stages: set[CalibrationStage] = set()
        for stage_seed_override in self.stage_seed_overrides:
            if stage_seed_override.stage in seen_stages:
                raise _calibration_error(
                    "stage_seed_overrides",
                    f"contains duplicate stage '{stage_seed_override.stage.value}'",
                )
            seen_stages.add(stage_seed_override.stage)

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationRunSeedSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_overrides = raw_payload.get("stage_seed_overrides", [])
        if isinstance(raw_overrides, str) or not isinstance(raw_overrides, list):
            raise _calibration_error(f"{field_path}.stage_seed_overrides", "must be a sequence")

        return CalibrationRunSeedSettings(
            global_seed=require_non_negative_integer(
                raw_payload,
                "global_seed",
                f"{field_path}.global_seed",
                CALIBRATION_ERROR_FACTORY,
            ),
            stage_seed_overrides=tuple(
                CalibrationStageSeedOverride.from_mapping(
                    raw_override,
                    field_path=f"{field_path}.stage_seed_overrides[{override_index}]",
                )
                for override_index, raw_override in enumerate(raw_overrides)
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationOptimizerSettings(ContractModel):
    solver_name: str
    max_iterations: int
    cpu_count: int
    absolute_tolerance: float
    relative_tolerance: float
    constraint_penalty_weight: float

    def __post_init__(self) -> None:
        if not self.solver_name.strip():
            raise _calibration_error("solver_name", "must be a non-empty string")
        if self.max_iterations <= 0:
            raise _calibration_error("max_iterations", "must be strictly positive")
        if self.cpu_count <= 0:
            raise _calibration_error("cpu_count", "must be strictly positive")
        if self.absolute_tolerance <= 0.0:
            raise _calibration_error("absolute_tolerance", "must be strictly positive")
        if self.relative_tolerance <= 0.0:
            raise _calibration_error("relative_tolerance", "must be strictly positive")
        if self.constraint_penalty_weight < 0.0:
            raise _calibration_error("constraint_penalty_weight", "must be non-negative")

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationOptimizerSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationOptimizerSettings(
            solver_name=require_non_empty_string(
                raw_payload,
                "solver_name",
                f"{field_path}.solver_name",
                CALIBRATION_ERROR_FACTORY,
            ),
            max_iterations=require_positive_integer(
                raw_payload,
                "max_iterations",
                f"{field_path}.max_iterations",
                CALIBRATION_ERROR_FACTORY,
            ),
            cpu_count=require_positive_integer(
                raw_payload,
                "cpu_count",
                f"{field_path}.cpu_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            absolute_tolerance=require_positive_finite_number(
                raw_payload,
                "absolute_tolerance",
                f"{field_path}.absolute_tolerance",
                CALIBRATION_ERROR_FACTORY,
            ),
            relative_tolerance=require_positive_finite_number(
                raw_payload,
                "relative_tolerance",
                f"{field_path}.relative_tolerance",
                CALIBRATION_ERROR_FACTORY,
            ),
            constraint_penalty_weight=require_non_negative_finite_number(
                raw_payload,
                "constraint_penalty_weight",
                f"{field_path}.constraint_penalty_weight",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationStageSolverSettings(ContractModel):
    stage: CalibrationStage
    optimizer_settings: CalibrationOptimizerSettings

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationStageSolverSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_stage = require_string(raw_payload, "stage", f"{field_path}.stage", CALIBRATION_ERROR_FACTORY)
        try:
            stage = CalibrationStage(raw_stage)
        except ValueError as e:
            raise _calibration_error(
                f"{field_path}.stage",
                "must be one of: seasonality, hawkes, return_components, mean_reversion, joint_refinement",
            ) from e

        return CalibrationStageSolverSettings(
            stage=stage,
            optimizer_settings=CalibrationOptimizerSettings.from_mapping(
                require_mapping(
                    raw_payload,
                    "optimizer_settings",
                    f"{field_path}.optimizer_settings",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.optimizer_settings",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationStageHandoff(ContractModel):
    from_stage: CalibrationStage
    to_stage: CalibrationStage
    transport: CalibrationHandoffTransport
    artifact_reference: Optional[str] = None

    def __post_init__(self) -> None:
        if self.from_stage == self.to_stage:
            raise _calibration_error("stage_handoff", "must not target the same stage")
        if self.transport == CalibrationHandoffTransport.PERSISTED_ARTIFACT_REFERENCE:
            if self.artifact_reference is None or not self.artifact_reference.strip():
                raise _calibration_error("artifact_reference", "must be non-empty for persisted handoffs")

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationStageHandoff:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        from_stage = CalibrationStage(
            require_string(raw_payload, "from_stage", f"{field_path}.from_stage", CALIBRATION_ERROR_FACTORY)
        )
        to_stage = CalibrationStage(
            require_string(raw_payload, "to_stage", f"{field_path}.to_stage", CALIBRATION_ERROR_FACTORY)
        )
        transport = CalibrationHandoffTransport(
            require_string(raw_payload, "transport", f"{field_path}.transport", CALIBRATION_ERROR_FACTORY)
        )

        artifact_reference = raw_payload.get("artifact_reference")
        if artifact_reference is not None and not isinstance(artifact_reference, str):
            raise _calibration_error(f"{field_path}.artifact_reference", "must be a string when provided")

        return CalibrationStageHandoff(
            from_stage=from_stage,
            to_stage=to_stage,
            transport=transport,
            artifact_reference=artifact_reference,
        )


@dataclass(frozen=True, slots=True)
class CalibrationResumeSettings(ContractModel):
    source: CalibrationResumeSource
    resume_from_stage: Optional[CalibrationStage] = None
    checkpoint_reference: Optional[str] = None

    def __post_init__(self) -> None:
        if self.source == CalibrationResumeSource.PERSISTED_CHECKPOINT_REFERENCE:
            if self.checkpoint_reference is None or not self.checkpoint_reference.strip():
                raise _calibration_error(
                    "checkpoint_reference",
                    "must be non-empty when source=persisted_checkpoint_reference",
                )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationResumeSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        source = CalibrationResumeSource(
            require_string(raw_payload, "source", f"{field_path}.source", CALIBRATION_ERROR_FACTORY)
        )

        raw_resume_from_stage = raw_payload.get("resume_from_stage")
        if raw_resume_from_stage is not None and not isinstance(raw_resume_from_stage, str):
            raise _calibration_error(f"{field_path}.resume_from_stage", "must be a string when provided")

        resume_from_stage = (
            CalibrationStage(raw_resume_from_stage)
            if raw_resume_from_stage is not None
            else None
        )

        raw_checkpoint_reference = raw_payload.get("checkpoint_reference")
        if raw_checkpoint_reference is not None and not isinstance(raw_checkpoint_reference, str):
            raise _calibration_error(f"{field_path}.checkpoint_reference", "must be a string when provided")

        return CalibrationResumeSettings(
            source=source,
            resume_from_stage=resume_from_stage,
            checkpoint_reference=raw_checkpoint_reference,
        )


class CalibrationTradeSign(StrEnum):
    BUY = "buy"
    SELL = "sell"


def _parse_calibration_execution_mode(
    raw_value: object,
    *,
    field_path: str,
) -> CalibrationExecutionMode:
    if not isinstance(raw_value, str):
        raise _calibration_error(field_path, "must be a string")

    try:
        return CalibrationExecutionMode(raw_value)
    except ValueError as e:
        raise _calibration_error(
            field_path,
            f"must be one of: memory, disk, received '{raw_value}'",
        ) from e

__all__ = [
    "CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY",
    "CALIBRATION_RUN_SPEC_SCHEMA_VERSION",
    "CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION",
    "CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION",
    "DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE",
    "CalibrationExecutionMode",
    "CalibrationHandoffTransport",
    "CalibrationOptimizerSettings",
    "CalibrationResumeSettings",
    "CalibrationResumeSource",
    "CalibrationRunDataInputs",
    "CalibrationRunSeedSettings",
    "CalibrationStage",
    "CalibrationStageExecutionStatus",
    "CalibrationStageHandoff",
    "CalibrationStageSeedOverride",
    "CalibrationStageSolverSettings",
    "CalibrationTradeSign",
]
