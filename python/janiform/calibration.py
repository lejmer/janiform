from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Optional

from ._contracts import (
    ContractModel,
    ValidationErrorFactory,
    parse_json_object,
    require_finite_number,
    require_mapping,
    require_non_empty_string,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_integer,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)
from .errors import ContractValidationFailure, JaniformValidationError

CALIBRATION_RUN_SPEC_SCHEMA_VERSION = "calibration.orchestration.run_spec.v1"
CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION = "calibration.orchestration.staged_run.report.v1"
CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY = (
    "cli_defaults_to_disk_python_importable_api_defaults_to_memory.v1"
)
DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE = "memory"
CALIBRATION_VALIDATION_ERROR_ID = "calibration.validation.contract"

CalibrationValidationFailure = ContractValidationFailure


class CalibrationValidationError(JaniformValidationError):
    pass


def _calibration_error(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = CALIBRATION_VALIDATION_ERROR_ID,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
) -> CalibrationValidationError:
    return CalibrationValidationError(
        CalibrationValidationFailure.from_context(
            error_id=error_id,
            workflow="calibration",
            entrypoint=entrypoint,
            field_path=field_path,
            message=message,
            context=context,
        )
    )


CALIBRATION_ERROR_FACTORY: ValidationErrorFactory = _calibration_error


class CalibrationExecutionMode(StrEnum):
    MEMORY = 'memory'
    DISK = 'disk'


class CalibrationStage(StrEnum):
    SEASONALITY = 'seasonality'
    HAWKES = 'hawkes'
    RETURN_COMPONENTS = 'return_components'


class CalibrationStageExecutionStatus(StrEnum):
    SUCCESS = 'success'
    FAILURE = 'failure'


class CalibrationHandoffTransport(StrEnum):
    IN_MEMORY_OUTPUT = 'in_memory_output'
    PERSISTED_ARTIFACT_REFERENCE = 'persisted_artifact_reference'


class CalibrationResumeSource(StrEnum):
    SAME_PROCESS = 'same_process'
    PERSISTED_CHECKPOINT_REFERENCE = 'persisted_checkpoint_reference'


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
                "must be one of: seasonality, hawkes, return_components",
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
    absolute_tolerance: float
    relative_tolerance: float
    constraint_penalty_weight: float

    def __post_init__(self) -> None:
        if not self.solver_name.strip():
            raise _calibration_error("solver_name", "must be a non-empty string")
        if self.max_iterations <= 0:
            raise _calibration_error("max_iterations", "must be strictly positive")
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
            constraint_penalty_weight=require_positive_or_zero(
                raw_payload,
                "constraint_penalty_weight",
                f"{field_path}.constraint_penalty_weight",
            ),
        )


def require_positive_or_zero(raw_payload: dict[str, Any], field_name: str, field_path: str) -> float:
    value = require_finite_number(raw_payload, field_name, field_path, CALIBRATION_ERROR_FACTORY)
    if value < 0.0:
        raise _calibration_error(field_path, "must be non-negative")

    return value


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
                "must be one of: seasonality, hawkes, return_components",
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
            raise _calibration_error("run_id", "must be a non-empty string")
        if len(self.selected_stages) == 0:
            raise _calibration_error("selected_stages", "must include at least one calibration stage")
        if self.seed_settings is None:
            raise _calibration_error("seed_settings", "is required")

        selected_stage_set = set(self.selected_stages)
        if len(selected_stage_set) != len(self.selected_stages):
            raise _calibration_error("selected_stages", "must not contain duplicates")

        configured_stage_set = {entry.stage for entry in self.optimizer_settings_by_stage}
        if configured_stage_set != selected_stage_set:
            raise _calibration_error(
                "optimizer_settings_by_stage",
                "must include exactly one solver-settings entry for each selected stage",
            )

        for stage_seed_override in self.seed_settings.stage_seed_overrides:
            if stage_seed_override.stage not in selected_stage_set:
                raise _calibration_error(
                    "seed_settings.stage_seed_overrides",
                    f"contains stage '{stage_seed_override.stage.value}' that is not selected",
                )

        for stage_handoff in self.stage_handoffs:
            if stage_handoff.from_stage not in selected_stage_set:
                raise _calibration_error(
                    "stage_handoffs.from_stage",
                    f"'{stage_handoff.from_stage.value}' must be selected",
                )

            if stage_handoff.to_stage not in selected_stage_set:
                raise _calibration_error(
                    "stage_handoffs.to_stage",
                    f"'{stage_handoff.to_stage.value}' must be selected",
                )

            if (
                self.execution_mode == CalibrationExecutionMode.MEMORY
                and stage_handoff.transport == CalibrationHandoffTransport.PERSISTED_ARTIFACT_REFERENCE
            ):
                raise _calibration_error(
                    "stage_handoffs.transport",
                    "persisted_artifact_reference requires execution_mode=disk",
                )

        if self.resume_settings is not None:
            if (
                self.execution_mode == CalibrationExecutionMode.MEMORY
                and self.resume_settings.source != CalibrationResumeSource.SAME_PROCESS
            ):
                raise _calibration_error(
                    "resume_settings.source",
                    "persisted_checkpoint_reference is only supported when execution_mode=disk",
                )

            if (
                self.execution_mode == CalibrationExecutionMode.DISK
                and self.resume_settings.source == CalibrationResumeSource.SAME_PROCESS
            ):
                raise _calibration_error(
                    "resume_settings.source",
                    "same_process is only supported when execution_mode=memory",
                )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> StagedCalibrationRequest:
        if not isinstance(raw_payload, dict):
            raise _calibration_error("staged_calibration_request", "must be an object")

        execution_mode = CalibrationExecutionMode(
            raw_payload.get("execution_mode", DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE)
        )

        raw_selected_stages = require_sequence(
            raw_payload,
            "selected_stages",
            "selected_stages",
            CALIBRATION_ERROR_FACTORY,
        )
        raw_solver_settings = require_sequence(
            raw_payload,
            "optimizer_settings_by_stage",
            "optimizer_settings_by_stage",
            CALIBRATION_ERROR_FACTORY,
        )

        raw_stage_handoffs = raw_payload.get("stage_handoffs", [])
        if isinstance(raw_stage_handoffs, str) or not isinstance(raw_stage_handoffs, list):
            raise _calibration_error("stage_handoffs", "must be a sequence")

        return StagedCalibrationRequest(
            run_id=require_non_empty_string(raw_payload, "run_id", "run_id", CALIBRATION_ERROR_FACTORY),
            data_inputs=CalibrationRunDataInputs.from_mapping(
                require_mapping(raw_payload, "data_inputs", "data_inputs", CALIBRATION_ERROR_FACTORY),
                field_path="data_inputs",
            ),
            execution_mode=execution_mode,
            selected_stages=tuple(CalibrationStage(cast_stage) for cast_stage in raw_selected_stages),
            seed_settings=CalibrationRunSeedSettings.from_mapping(
                require_mapping(raw_payload, "seed_settings", "seed_settings", CALIBRATION_ERROR_FACTORY),
                field_path="seed_settings",
            ),
            optimizer_settings_by_stage=tuple(
                CalibrationStageSolverSettings.from_mapping(
                    raw_setting,
                    field_path=f"optimizer_settings_by_stage[{setting_index}]",
                )
                for setting_index, raw_setting in enumerate(raw_solver_settings)
            ),
            stage_handoffs=tuple(
                CalibrationStageHandoff.from_mapping(
                    raw_handoff,
                    field_path=f"stage_handoffs[{handoff_index}]",
                )
                for handoff_index, raw_handoff in enumerate(raw_stage_handoffs)
            ),
            resume_settings=(
                CalibrationResumeSettings.from_mapping(
                    raw_payload["resume_settings"],
                    field_path="resume_settings",
                )
                if "resume_settings" in raw_payload and raw_payload["resume_settings"] is not None
                else None
            ),
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
            execution_mode=CalibrationExecutionMode(
                require_string(raw_payload, "execution_mode", f"{field_path}.execution_mode", CALIBRATION_ERROR_FACTORY)
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
            execution_mode=CalibrationExecutionMode(
                require_string(raw_payload, "execution_mode", f"{field_path}.execution_mode", CALIBRATION_ERROR_FACTORY)
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
    resumed_from_checkpoint_reference: Optional[str] = None
    resumed_from_last_successful_stage: Optional[CalibrationStage] = None
    started_from_stage: Optional[CalibrationStage] = None
    failure: Optional[CalibrationFailureDetails] = None

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
        raw_resumed_stage = raw_payload.get("resumed_from_last_successful_stage")
        raw_started_stage = raw_payload.get("started_from_stage")

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
            execution_mode=CalibrationExecutionMode(
                require_string(raw_payload, "execution_mode", "execution_mode", CALIBRATION_ERROR_FACTORY)
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


def validate_staged_calibration_request(raw_request: dict[str, Any] | Any) -> StagedCalibrationRequest:
    return StagedCalibrationRequest.from_mapping(raw_request)


def validate_staged_calibration_response(raw_response: dict[str, Any] | Any) -> StagedCalibrationResponse:
    return StagedCalibrationResponse.from_mapping(raw_response)


def serialize_staged_calibration_request(request: StagedCalibrationRequest) -> str:
    return serialize_deterministic_json(request)


def deserialize_staged_calibration_request(payload_json: str) -> StagedCalibrationRequest:
    raw_payload = parse_json_object(payload_json, "staged calibration request", CALIBRATION_ERROR_FACTORY)
    return validate_staged_calibration_request(dict(raw_payload))


def serialize_staged_calibration_response(response: StagedCalibrationResponse) -> str:
    return serialize_deterministic_json(response)


def deserialize_staged_calibration_response(payload_json: str) -> StagedCalibrationResponse:
    raw_payload = parse_json_object(payload_json, "staged calibration response", CALIBRATION_ERROR_FACTORY)
    return validate_staged_calibration_response(dict(raw_payload))


__all__ = [
    "CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY",
    "CALIBRATION_RUN_SPEC_SCHEMA_VERSION",
    "CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION",
    "CalibrationCheckpointMetadata",
    "CalibrationExecutionMode",
    "CalibrationFailureDetails",
    "CalibrationHandoffTransport",
    "CalibrationOptimizerSettings",
    "CalibrationResumeSettings",
    "CalibrationResumeSource",
    "CalibrationRunDataInputs",
    "CalibrationRunSeedSettings",
    "CalibrationStage",
    "CalibrationStageExecutionStatus",
    "CalibrationStageHandoff",
    "CalibrationStageReport",
    "CalibrationStageSeedOverride",
    "CalibrationStageSolverSettings",
    "CalibrationValidationError",
    "CalibrationValidationFailure",
    "DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE",
    "StagedCalibrationRequest",
    "StagedCalibrationResponse",
    "deserialize_staged_calibration_request",
    "deserialize_staged_calibration_response",
    "serialize_staged_calibration_request",
    "serialize_staged_calibration_response",
    "validate_staged_calibration_request",
    "validate_staged_calibration_response",
]
