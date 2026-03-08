from dataclasses import dataclass
from enum import StrEnum
from types import ModuleType
from typing import Any, Optional, cast

from ._contracts import (
    ContractModel,
    ValidationErrorFactory,
    parse_json_object,
    require_integer,
    require_finite_number,
    require_mapping,
    require_non_empty_string,
    require_non_negative_finite_number,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_integer,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)
from ._native_api import native_module
from .calendar import CalendarRegistry
from .errors import (
    ContractValidationFailure,
    JaniformErrorCategory,
    JaniformErrorDetails,
    JaniformExecutionError,
    JaniformIntegrationError,
    JaniformValidationError,
)
from .pricing import GammaSeverityMarkConfiguration, NigSharedPlusShiftConfiguration

CALIBRATION_RUN_SPEC_SCHEMA_VERSION = "calibration.orchestration.run_spec.v1"
CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION = "calibration.market_data.request.v1"
CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION = "calibration.orchestration.staged_execution_input.v1"
CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION = "calibration.orchestration.staged_run.report.v1"
CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY = (
    "cli_defaults_to_disk_python_importable_api_defaults_to_memory.v1"
)
DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE = "memory"
CALIBRATION_VALIDATION_ERROR_ID = "calibration.validation.contract"
CALIBRATION_EXECUTION_ERROR_ID = "calibration.execution.run_failed"
CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID = "calibration.integration.native_module_unavailable"
CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID = "calibration.integration.invalid_native_response"

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


def _calibration_execution_error(
    message: str,
    *,
    entrypoint: str,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
) -> JaniformExecutionError:
    return JaniformExecutionError(
        JaniformErrorDetails.from_context(
            error_id=CALIBRATION_EXECUTION_ERROR_ID,
            category=JaniformErrorCategory.EXECUTION,
            workflow="calibration",
            entrypoint=entrypoint,
            message=message,
            context=context,
        )
    )


def _calibration_integration_error(
    message: str,
    *,
    entrypoint: str,
    error_id: str,
    field_path: Optional[str] = None,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
) -> JaniformIntegrationError:
    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=error_id,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="calibration",
            entrypoint=entrypoint,
            message=message,
            field_path=field_path,
            context=context,
        )
    )


CALIBRATION_ERROR_FACTORY: ValidationErrorFactory = _calibration_error


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


def _require_bool(raw_payload: dict[str, Any], field_name: str, field_path: str) -> bool:
    if field_name not in raw_payload:
        raise _calibration_error(field_path, "is required")

    value = raw_payload[field_name]
    if not isinstance(value, bool):
        raise _calibration_error(field_path, "must be a boolean")

    return value


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


class CalibrationInputFileFormat(StrEnum):
    CSV = 'csv'
    PARQUET = 'parquet'
    JSON = 'json'


class CalibrationMarketDataWorkflowScope(StrEnum):
    SEASONALITY_ONLY = 'seasonality_only'
    FULL_WITHOUT_OPTION_SURFACE = 'full_without_option_surface'
    FULL_WITH_OPTION_SURFACE = 'full_with_option_surface'


class CalibrationDuplicateRecordPolicy(StrEnum):
    ERROR = 'error'
    KEEP_FIRST = 'keep_first'
    KEEP_LAST = 'keep_last'


class CalibrationTickPriceInputPolicy(StrEnum):
    EXPLICIT_PRICE_COLUMN = 'explicit_price_column'
    TRADE_PRICE_COLUMN = 'trade_price_column'
    BID_ASK_MIDPOINT = 'bid_ask_midpoint'
    CLOSE_PRICE_COLUMN = 'close_price_column'


class CalibrationOptionQuoteInputPolicy(StrEnum):
    MID_PRICE_COLUMN = 'mid_price_column'
    BID_ASK_MIDPOINT = 'bid_ask_midpoint'
    IMPLIED_VOLATILITY_COLUMN = 'implied_volatility_column'


@dataclass(frozen=True, slots=True)
class CalibrationInputFileReference(ContractModel):
    path: str
    file_format: CalibrationInputFileFormat
    provenance_label: str

    def __post_init__(self) -> None:
        if not self.path.strip():
            raise _calibration_error("path", "must be a non-empty string")
        if not self.provenance_label.strip():
            raise _calibration_error("provenance_label", "must be a non-empty string")

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationInputFileReference:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationInputFileReference(
            path=require_non_empty_string(
                raw_payload,
                "path",
                f"{field_path}.path",
                CALIBRATION_ERROR_FACTORY,
            ),
            file_format=CalibrationInputFileFormat(
                require_string(
                    raw_payload,
                    "file_format",
                    f"{field_path}.file_format",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            provenance_label=require_non_empty_string(
                raw_payload,
                "provenance_label",
                f"{field_path}.provenance_label",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationSessionConfigurationInput(ContractModel):
    exchange_code: str
    session_definition_source: CalibrationInputFileReference
    session_timezone: str

    def __post_init__(self) -> None:
        if not self.exchange_code.strip():
            raise _calibration_error("exchange_code", "must be a non-empty string")
        if not self.session_timezone.strip():
            raise _calibration_error("session_timezone", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationSessionConfigurationInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationSessionConfigurationInput(
            exchange_code=require_non_empty_string(
                raw_payload,
                "exchange_code",
                f"{field_path}.exchange_code",
                CALIBRATION_ERROR_FACTORY,
            ),
            session_definition_source=CalibrationInputFileReference.from_mapping(
                require_mapping(
                    raw_payload,
                    "session_definition_source",
                    f"{field_path}.session_definition_source",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.session_definition_source",
            ),
            session_timezone=require_non_empty_string(
                raw_payload,
                "session_timezone",
                f"{field_path}.session_timezone",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationKnownNewsInput(ContractModel):
    calendar_source: CalibrationInputFileReference
    calendar_timezone: Optional[str] = None

    def __post_init__(self) -> None:
        if self.calendar_timezone is not None and not self.calendar_timezone.strip():
            raise _calibration_error("calendar_timezone", "must be non-empty when provided")

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationKnownNewsInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_calendar_timezone = raw_payload.get("calendar_timezone")
        if raw_calendar_timezone is not None and not isinstance(raw_calendar_timezone, str):
            raise _calibration_error(f"{field_path}.calendar_timezone", "must be a string when provided")

        return CalibrationKnownNewsInput(
            calendar_source=CalibrationInputFileReference.from_mapping(
                require_mapping(
                    raw_payload,
                    "calendar_source",
                    f"{field_path}.calendar_source",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.calendar_source",
            ),
            calendar_timezone=raw_calendar_timezone,
        )


@dataclass(frozen=True, slots=True)
class CalibrationTickDataColumnMapping(ContractModel):
    timestamp_column: str
    price_column: Optional[str] = None
    trade_price_column: Optional[str] = None
    bid_column: Optional[str] = None
    ask_column: Optional[str] = None
    close_price_column: Optional[str] = None

    def __post_init__(self) -> None:
        if not self.timestamp_column.strip():
            raise _calibration_error("timestamp_column", "must be a non-empty string")

        for field_name, field_value in (
            ("price_column", self.price_column),
            ("trade_price_column", self.trade_price_column),
            ("bid_column", self.bid_column),
            ("ask_column", self.ask_column),
            ("close_price_column", self.close_price_column),
        ):
            if field_value is not None and not field_value.strip():
                raise _calibration_error(field_name, "must be non-empty when provided")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationTickDataColumnMapping:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_optional_fields = {
            "price_column": raw_payload.get("price_column"),
            "trade_price_column": raw_payload.get("trade_price_column"),
            "bid_column": raw_payload.get("bid_column"),
            "ask_column": raw_payload.get("ask_column"),
            "close_price_column": raw_payload.get("close_price_column"),
        }
        for field_name, field_value in raw_optional_fields.items():
            if field_value is not None and not isinstance(field_value, str):
                raise _calibration_error(f"{field_path}.{field_name}", "must be a string when provided")

        return CalibrationTickDataColumnMapping(
            timestamp_column=require_non_empty_string(
                raw_payload,
                "timestamp_column",
                f"{field_path}.timestamp_column",
                CALIBRATION_ERROR_FACTORY,
            ),
            price_column=cast(Optional[str], raw_optional_fields["price_column"]),
            trade_price_column=cast(Optional[str], raw_optional_fields["trade_price_column"]),
            bid_column=cast(Optional[str], raw_optional_fields["bid_column"]),
            ask_column=cast(Optional[str], raw_optional_fields["ask_column"]),
            close_price_column=cast(Optional[str], raw_optional_fields["close_price_column"]),
        )


@dataclass(frozen=True, slots=True)
class CalibrationOptionSurfaceColumnMapping(ContractModel):
    quote_timestamp_column: str
    expiry_timestamp_column: str
    strike_column: str
    option_type_column: str
    mid_price_column: Optional[str] = None
    bid_column: Optional[str] = None
    ask_column: Optional[str] = None
    implied_volatility_column: Optional[str] = None

    def __post_init__(self) -> None:
        for field_name, field_value in (
            ("quote_timestamp_column", self.quote_timestamp_column),
            ("expiry_timestamp_column", self.expiry_timestamp_column),
            ("strike_column", self.strike_column),
            ("option_type_column", self.option_type_column),
        ):
            if not field_value.strip():
                raise _calibration_error(field_name, "must be a non-empty string")

        for field_name, field_value in (
            ("mid_price_column", self.mid_price_column),
            ("bid_column", self.bid_column),
            ("ask_column", self.ask_column),
            ("implied_volatility_column", self.implied_volatility_column),
        ):
            if field_value is not None and not field_value.strip():
                raise _calibration_error(field_name, "must be non-empty when provided")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationOptionSurfaceColumnMapping:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_optional_fields = {
            "mid_price_column": raw_payload.get("mid_price_column"),
            "bid_column": raw_payload.get("bid_column"),
            "ask_column": raw_payload.get("ask_column"),
            "implied_volatility_column": raw_payload.get("implied_volatility_column"),
        }
        for field_name, field_value in raw_optional_fields.items():
            if field_value is not None and not isinstance(field_value, str):
                raise _calibration_error(f"{field_path}.{field_name}", "must be a string when provided")

        return CalibrationOptionSurfaceColumnMapping(
            quote_timestamp_column=require_non_empty_string(
                raw_payload,
                "quote_timestamp_column",
                f"{field_path}.quote_timestamp_column",
                CALIBRATION_ERROR_FACTORY,
            ),
            expiry_timestamp_column=require_non_empty_string(
                raw_payload,
                "expiry_timestamp_column",
                f"{field_path}.expiry_timestamp_column",
                CALIBRATION_ERROR_FACTORY,
            ),
            strike_column=require_non_empty_string(
                raw_payload,
                "strike_column",
                f"{field_path}.strike_column",
                CALIBRATION_ERROR_FACTORY,
            ),
            option_type_column=require_non_empty_string(
                raw_payload,
                "option_type_column",
                f"{field_path}.option_type_column",
                CALIBRATION_ERROR_FACTORY,
            ),
            mid_price_column=cast(Optional[str], raw_optional_fields["mid_price_column"]),
            bid_column=cast(Optional[str], raw_optional_fields["bid_column"]),
            ask_column=cast(Optional[str], raw_optional_fields["ask_column"]),
            implied_volatility_column=cast(Optional[str], raw_optional_fields["implied_volatility_column"]),
        )


@dataclass(frozen=True, slots=True)
class CalibrationKnownNewsColumnMapping(ContractModel):
    event_timestamp_column: str
    event_identifier_column: Optional[str] = None

    def __post_init__(self) -> None:
        if not self.event_timestamp_column.strip():
            raise _calibration_error("event_timestamp_column", "must be a non-empty string")
        if self.event_identifier_column is not None and not self.event_identifier_column.strip():
            raise _calibration_error("event_identifier_column", "must be non-empty when provided")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationKnownNewsColumnMapping:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_event_identifier_column = raw_payload.get("event_identifier_column")
        if raw_event_identifier_column is not None and not isinstance(raw_event_identifier_column, str):
            raise _calibration_error(
                f"{field_path}.event_identifier_column",
                "must be a string when provided",
            )

        return CalibrationKnownNewsColumnMapping(
            event_timestamp_column=require_non_empty_string(
                raw_payload,
                "event_timestamp_column",
                f"{field_path}.event_timestamp_column",
                CALIBRATION_ERROR_FACTORY,
            ),
            event_identifier_column=cast(Optional[str], raw_event_identifier_column),
        )


@dataclass(frozen=True, slots=True)
class CalibrationMarketDataColumnMapping(ContractModel):
    tick_data: CalibrationTickDataColumnMapping
    option_surface: Optional[CalibrationOptionSurfaceColumnMapping] = None
    known_news: Optional[CalibrationKnownNewsColumnMapping] = None

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationMarketDataColumnMapping:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping.from_mapping(
                require_mapping(
                    raw_payload,
                    "tick_data",
                    f"{field_path}.tick_data",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.tick_data",
            ),
            option_surface=(
                CalibrationOptionSurfaceColumnMapping.from_mapping(
                    require_mapping(
                        raw_payload,
                        "option_surface",
                        f"{field_path}.option_surface",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.option_surface",
                )
                if "option_surface" in raw_payload and raw_payload["option_surface"] is not None
                else None
            ),
            known_news=(
                CalibrationKnownNewsColumnMapping.from_mapping(
                    require_mapping(
                        raw_payload,
                        "known_news",
                        f"{field_path}.known_news",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.known_news",
                )
                if "known_news" in raw_payload and raw_payload["known_news"] is not None
                else None
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationMarketDataNormalizationPolicy(ContractModel):
    timestamp_timezone: str
    sort_records_by_timestamp: bool = True
    duplicate_record_policy: CalibrationDuplicateRecordPolicy = CalibrationDuplicateRecordPolicy.ERROR
    tick_price_input_policy: Optional[CalibrationTickPriceInputPolicy] = None
    option_quote_input_policy: Optional[CalibrationOptionQuoteInputPolicy] = None

    def __post_init__(self) -> None:
        if not self.timestamp_timezone.strip():
            raise _calibration_error("timestamp_timezone", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationMarketDataNormalizationPolicy:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_tick_price_input_policy = raw_payload.get("tick_price_input_policy")
        if raw_tick_price_input_policy is not None and not isinstance(raw_tick_price_input_policy, str):
            raise _calibration_error(f"{field_path}.tick_price_input_policy", "must be a string when provided")

        raw_option_quote_input_policy = raw_payload.get("option_quote_input_policy")
        if raw_option_quote_input_policy is not None and not isinstance(raw_option_quote_input_policy, str):
            raise _calibration_error(f"{field_path}.option_quote_input_policy", "must be a string when provided")

        return CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone=require_non_empty_string(
                raw_payload,
                "timestamp_timezone",
                f"{field_path}.timestamp_timezone",
                CALIBRATION_ERROR_FACTORY,
            ),
            sort_records_by_timestamp=_require_bool(
                raw_payload,
                "sort_records_by_timestamp",
                f"{field_path}.sort_records_by_timestamp",
            ),
            duplicate_record_policy=CalibrationDuplicateRecordPolicy(
                require_string(
                    raw_payload,
                    "duplicate_record_policy",
                    f"{field_path}.duplicate_record_policy",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            tick_price_input_policy=(
                CalibrationTickPriceInputPolicy(raw_tick_price_input_policy)
                if raw_tick_price_input_policy is not None
                else None
            ),
            option_quote_input_policy=(
                CalibrationOptionQuoteInputPolicy(raw_option_quote_input_policy)
                if raw_option_quote_input_policy is not None
                else None
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationMarketDataOutputPreferences(ContractModel):
    execution_mode: CalibrationExecutionMode = CalibrationExecutionMode.MEMORY
    disk_checkpoint_directory: Optional[str] = None
    emit_derived_staged_execution_input: bool = False
    emit_input_provenance_report: bool = True

    def __post_init__(self) -> None:
        if self.disk_checkpoint_directory is not None and not self.disk_checkpoint_directory.strip():
            raise _calibration_error("disk_checkpoint_directory", "must be non-empty when provided")

        if self.execution_mode == CalibrationExecutionMode.MEMORY and self.disk_checkpoint_directory is not None:
            raise _calibration_error(
                "disk_checkpoint_directory",
                "is only supported when execution_mode=disk",
            )

        if self.execution_mode == CalibrationExecutionMode.DISK and self.disk_checkpoint_directory is None:
            raise _calibration_error(
                "disk_checkpoint_directory",
                "is required when execution_mode=disk",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationMarketDataOutputPreferences:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_disk_checkpoint_directory = raw_payload.get("disk_checkpoint_directory")
        if raw_disk_checkpoint_directory is not None and not isinstance(raw_disk_checkpoint_directory, str):
            raise _calibration_error(
                f"{field_path}.disk_checkpoint_directory",
                "must be a string when provided",
            )

        return CalibrationMarketDataOutputPreferences(
            execution_mode=_parse_calibration_execution_mode(
                raw_payload.get("execution_mode", DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE),
                field_path=f"{field_path}.execution_mode",
            ),
            disk_checkpoint_directory=cast(Optional[str], raw_disk_checkpoint_directory),
            emit_derived_staged_execution_input=_require_bool(
                raw_payload,
                "emit_derived_staged_execution_input",
                f"{field_path}.emit_derived_staged_execution_input",
            ),
            emit_input_provenance_report=_require_bool(
                raw_payload,
                "emit_input_provenance_report",
                f"{field_path}.emit_input_provenance_report",
            ),
        )


def _validate_tick_price_input_policy(
    *,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    column_mapping: CalibrationTickDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
) -> None:
    tick_price_input_policy = normalization_policy.tick_price_input_policy
    if workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY:
        if tick_price_input_policy is not None:
            raise _calibration_error(
                "normalization_policy.tick_price_input_policy",
                "must not be provided when workflow_scope=seasonality_only",
            )
        return

    if tick_price_input_policy is None:
        raise _calibration_error(
            "normalization_policy.tick_price_input_policy",
            "is required when workflow_scope includes return calibration",
        )

    match tick_price_input_policy:
        case CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN:
            if column_mapping.price_column is None:
                raise _calibration_error(
                    "column_mapping.tick_data.price_column",
                    "is required when tick_price_input_policy=explicit_price_column",
                )
        case CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN:
            if column_mapping.trade_price_column is None:
                raise _calibration_error(
                    "column_mapping.tick_data.trade_price_column",
                    "is required when tick_price_input_policy=trade_price_column",
                )
        case CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT:
            if column_mapping.bid_column is None or column_mapping.ask_column is None:
                raise _calibration_error(
                    "column_mapping.tick_data",
                    "bid_column and ask_column are required when tick_price_input_policy=bid_ask_midpoint",
                )
        case CalibrationTickPriceInputPolicy.CLOSE_PRICE_COLUMN:
            if column_mapping.close_price_column is None:
                raise _calibration_error(
                    "column_mapping.tick_data.close_price_column",
                    "is required when tick_price_input_policy=close_price_column",
                )


def _validate_option_surface_scope(
    *,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    option_surface_source: Optional[CalibrationInputFileReference],
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping],
    option_quote_input_policy: Optional[CalibrationOptionQuoteInputPolicy],
) -> None:
    if workflow_scope == CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE:
        if option_surface_source is None:
            raise _calibration_error(
                "option_surface_source",
                "is required when workflow_scope=full_with_option_surface",
            )
        if option_surface_column_mapping is None:
            raise _calibration_error(
                "column_mapping.option_surface",
                "is required when workflow_scope=full_with_option_surface",
            )
        if option_quote_input_policy is None:
            raise _calibration_error(
                "normalization_policy.option_quote_input_policy",
                "is required when workflow_scope=full_with_option_surface",
            )
    else:
        if option_surface_source is not None:
            raise _calibration_error(
                "option_surface_source",
                "must not be provided unless workflow_scope=full_with_option_surface",
            )
        if option_surface_column_mapping is not None:
            raise _calibration_error(
                "column_mapping.option_surface",
                "must not be provided unless workflow_scope=full_with_option_surface",
            )
        if option_quote_input_policy is not None:
            raise _calibration_error(
                "normalization_policy.option_quote_input_policy",
                "must not be provided unless workflow_scope=full_with_option_surface",
            )
        return

    if option_surface_column_mapping is None or option_quote_input_policy is None:
        return

    match option_quote_input_policy:
        case CalibrationOptionQuoteInputPolicy.MID_PRICE_COLUMN:
            if option_surface_column_mapping.mid_price_column is None:
                raise _calibration_error(
                    "column_mapping.option_surface.mid_price_column",
                    "is required when option_quote_input_policy=mid_price_column",
                )
        case CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT:
            if option_surface_column_mapping.bid_column is None or option_surface_column_mapping.ask_column is None:
                raise _calibration_error(
                    "column_mapping.option_surface",
                    "bid_column and ask_column are required when option_quote_input_policy=bid_ask_midpoint",
                )
        case CalibrationOptionQuoteInputPolicy.IMPLIED_VOLATILITY_COLUMN:
            if option_surface_column_mapping.implied_volatility_column is None:
                raise _calibration_error(
                    "column_mapping.option_surface.implied_volatility_column",
                    "is required when option_quote_input_policy=implied_volatility_column",
                )


def _validate_known_news_inputs(
    *,
    known_news_inputs: Optional[CalibrationKnownNewsInput],
    known_news_column_mapping: Optional[CalibrationKnownNewsColumnMapping],
) -> None:
    if known_news_inputs is None and known_news_column_mapping is not None:
        raise _calibration_error(
            "column_mapping.known_news",
            "must not be provided when known_news_inputs is omitted",
        )

    if known_news_inputs is None:
        return

    if (
        known_news_inputs.calendar_source.file_format in (CalibrationInputFileFormat.CSV, CalibrationInputFileFormat.PARQUET)
        and known_news_column_mapping is None
    ):
        raise _calibration_error(
            "column_mapping.known_news",
            "is required when known_news_inputs.calendar_source uses csv or parquet",
        )


@dataclass(frozen=True, slots=True)
class MarketDataCalibrationRequest(ContractModel):
    """User-facing market-data-first calibration request."""

    schema_version: str
    run_id: str
    tick_data_source: CalibrationInputFileReference
    session_configuration: CalibrationSessionConfigurationInput
    column_mapping: CalibrationMarketDataColumnMapping
    workflow_scope: CalibrationMarketDataWorkflowScope
    normalization_policy: CalibrationMarketDataNormalizationPolicy
    output_preferences: CalibrationMarketDataOutputPreferences
    option_surface_source: Optional[CalibrationInputFileReference] = None
    known_news_inputs: Optional[CalibrationKnownNewsInput] = None

    def __post_init__(self) -> None:
        if self.schema_version != CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION:
            raise _calibration_error(
                "schema_version",
                f"must be `{CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION}`",
            )

        if not self.run_id.strip():
            raise _calibration_error("run_id", "must be a non-empty string")

        if self.tick_data_source.file_format not in (CalibrationInputFileFormat.CSV, CalibrationInputFileFormat.PARQUET):
            raise _calibration_error(
                "tick_data_source.file_format",
                "must be one of: csv, parquet",
            )

        if (
            self.option_surface_source is not None
            and self.option_surface_source.file_format not in (CalibrationInputFileFormat.CSV, CalibrationInputFileFormat.PARQUET)
        ):
            raise _calibration_error(
                "option_surface_source.file_format",
                "must be one of: csv, parquet",
            )

        _validate_tick_price_input_policy(
            workflow_scope=self.workflow_scope,
            column_mapping=self.column_mapping.tick_data,
            normalization_policy=self.normalization_policy,
        )
        _validate_option_surface_scope(
            workflow_scope=self.workflow_scope,
            option_surface_source=self.option_surface_source,
            option_surface_column_mapping=self.column_mapping.option_surface,
            option_quote_input_policy=self.normalization_policy.option_quote_input_policy,
        )
        _validate_known_news_inputs(
            known_news_inputs=self.known_news_inputs,
            known_news_column_mapping=self.column_mapping.known_news,
        )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> MarketDataCalibrationRequest:
        if not isinstance(raw_payload, dict):
            raise _calibration_error("market_data_calibration_request", "must be an object")

        return MarketDataCalibrationRequest(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                "schema_version",
                CALIBRATION_ERROR_FACTORY,
            ),
            run_id=require_non_empty_string(
                raw_payload,
                "run_id",
                "run_id",
                CALIBRATION_ERROR_FACTORY,
            ),
            tick_data_source=CalibrationInputFileReference.from_mapping(
                require_mapping(
                    raw_payload,
                    "tick_data_source",
                    "tick_data_source",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path="tick_data_source",
            ),
            session_configuration=CalibrationSessionConfigurationInput.from_mapping(
                require_mapping(
                    raw_payload,
                    "session_configuration",
                    "session_configuration",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path="session_configuration",
            ),
            column_mapping=CalibrationMarketDataColumnMapping.from_mapping(
                require_mapping(
                    raw_payload,
                    "column_mapping",
                    "column_mapping",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path="column_mapping",
            ),
            workflow_scope=CalibrationMarketDataWorkflowScope(
                require_string(
                    raw_payload,
                    "workflow_scope",
                    "workflow_scope",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            normalization_policy=CalibrationMarketDataNormalizationPolicy.from_mapping(
                require_mapping(
                    raw_payload,
                    "normalization_policy",
                    "normalization_policy",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path="normalization_policy",
            ),
            output_preferences=CalibrationMarketDataOutputPreferences.from_mapping(
                require_mapping(
                    raw_payload,
                    "output_preferences",
                    "output_preferences",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path="output_preferences",
            ),
            option_surface_source=(
                CalibrationInputFileReference.from_mapping(
                    require_mapping(
                        raw_payload,
                        "option_surface_source",
                        "option_surface_source",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path="option_surface_source",
                )
                if "option_surface_source" in raw_payload and raw_payload["option_surface_source"] is not None
                else None
            ),
            known_news_inputs=(
                CalibrationKnownNewsInput.from_mapping(
                    require_mapping(
                        raw_payload,
                        "known_news_inputs",
                        "known_news_inputs",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path="known_news_inputs",
                )
                if "known_news_inputs" in raw_payload and raw_payload["known_news_inputs"] is not None
                else None
            ),
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
            raise _calibration_error(field_path, "must be an object")

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
            CALIBRATION_ERROR_FACTORY,
        )
        raw_solver_settings = require_sequence(
            raw_payload,
            "optimizer_settings_by_stage",
            prefixed("optimizer_settings_by_stage"),
            CALIBRATION_ERROR_FACTORY,
        )

        raw_stage_handoffs = raw_payload.get("stage_handoffs", [])
        if isinstance(raw_stage_handoffs, str) or not isinstance(raw_stage_handoffs, list):
            raise _calibration_error(prefixed("stage_handoffs"), "must be a sequence")

        return StagedCalibrationRequest(
            run_id=require_non_empty_string(
                raw_payload,
                "run_id",
                prefixed("run_id"),
                CALIBRATION_ERROR_FACTORY,
            ),
            data_inputs=CalibrationRunDataInputs.from_mapping(
                require_mapping(
                    raw_payload,
                    "data_inputs",
                    prefixed("data_inputs"),
                    CALIBRATION_ERROR_FACTORY,
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
                    CALIBRATION_ERROR_FACTORY,
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


class CalibrationTimestampEventSessionFilter(StrEnum):
    ALL = 'all'
    OPEN_ONLY = 'open_only'
    CLOSED_ONLY = 'closed_only'


class CalibrationIntradaySmoothingKernel(StrEnum):
    GAUSSIAN = 'gaussian'
    UNIFORM = 'uniform'


def _require_timestamp_utc_nanos(raw_payload: dict[str, Any], field_name: str, field_path: str) -> int:
    return require_integer(raw_payload, field_name, field_path, CALIBRATION_ERROR_FACTORY)


@dataclass(frozen=True, slots=True)
class CalibrationTimestampEventRecord(ContractModel):
    source_record_position: int
    timestamp_utc_nanos: int

    def __post_init__(self) -> None:
        if self.source_record_position < 0:
            raise _calibration_error("source_record_position", "must be non-negative")

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationTimestampEventRecord:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationTimestampEventRecord(
            source_record_position=require_non_negative_integer(
                raw_payload,
                "source_record_position",
                f"{field_path}.source_record_position",
                CALIBRATION_ERROR_FACTORY,
            ),
            timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "timestamp_utc_nanos",
                f"{field_path}.timestamp_utc_nanos",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationScheduledEventOverlay(ContractModel):
    overlay_id: str
    start_timestamp_utc_nanos: int
    end_timestamp_utc_nanos: int
    log_adjustment: float
    precedence: int
    enabled: bool
    source_label: str

    def __post_init__(self) -> None:
        if not self.overlay_id.strip():
            raise _calibration_error("overlay_id", "must be a non-empty string")
        if not self.source_label.strip():
            raise _calibration_error("source_label", "must be a non-empty string")
        if self.precedence < 0:
            raise _calibration_error("precedence", "must be non-negative")

        require_finite_number(
            {"log_adjustment": self.log_adjustment},
            "log_adjustment",
            "log_adjustment",
            CALIBRATION_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationScheduledEventOverlay:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationScheduledEventOverlay(
            overlay_id=require_non_empty_string(
                raw_payload,
                "overlay_id",
                f"{field_path}.overlay_id",
                CALIBRATION_ERROR_FACTORY,
            ),
            start_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "start_timestamp_utc_nanos",
                f"{field_path}.start_timestamp_utc_nanos",
            ),
            end_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "end_timestamp_utc_nanos",
                f"{field_path}.end_timestamp_utc_nanos",
            ),
            log_adjustment=require_finite_number(
                raw_payload,
                "log_adjustment",
                f"{field_path}.log_adjustment",
                CALIBRATION_ERROR_FACTORY,
            ),
            precedence=require_non_negative_integer(
                raw_payload,
                "precedence",
                f"{field_path}.precedence",
                CALIBRATION_ERROR_FACTORY,
            ),
            enabled=_require_bool(raw_payload, "enabled", f"{field_path}.enabled"),
            source_label=require_non_empty_string(
                raw_payload,
                "source_label",
                f"{field_path}.source_label",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationIntradaySeasonalityEstimatorSettings(ContractModel):
    minutes_per_bin: int
    smoothing_kernel: CalibrationIntradaySmoothingKernel
    smoothing_bandwidth_bins: float
    minimum_bin_events: int
    pseudocount: float

    def __post_init__(self) -> None:
        if self.minutes_per_bin <= 0:
            raise _calibration_error("minutes_per_bin", "must be strictly positive")
        if self.minimum_bin_events <= 0:
            raise _calibration_error("minimum_bin_events", "must be strictly positive")
        if self.smoothing_bandwidth_bins <= 0.0:
            raise _calibration_error("smoothing_bandwidth_bins", "must be strictly positive")
        if self.pseudocount <= 0.0:
            raise _calibration_error("pseudocount", "must be strictly positive")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationIntradaySeasonalityEstimatorSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationIntradaySeasonalityEstimatorSettings(
            minutes_per_bin=require_positive_integer(
                raw_payload,
                "minutes_per_bin",
                f"{field_path}.minutes_per_bin",
                CALIBRATION_ERROR_FACTORY,
            ),
            smoothing_kernel=CalibrationIntradaySmoothingKernel(
                require_string(
                    raw_payload,
                    "smoothing_kernel",
                    f"{field_path}.smoothing_kernel",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            smoothing_bandwidth_bins=require_positive_finite_number(
                raw_payload,
                "smoothing_bandwidth_bins",
                f"{field_path}.smoothing_bandwidth_bins",
                CALIBRATION_ERROR_FACTORY,
            ),
            minimum_bin_events=require_positive_integer(
                raw_payload,
                "minimum_bin_events",
                f"{field_path}.minimum_bin_events",
                CALIBRATION_ERROR_FACTORY,
            ),
            pseudocount=require_positive_finite_number(
                raw_payload,
                "pseudocount",
                f"{field_path}.pseudocount",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationDayOfWeekSeasonalityEstimatorSettings(ContractModel):
    minimum_bucket_events: int
    pseudocount: float

    def __post_init__(self) -> None:
        if self.minimum_bucket_events <= 0:
            raise _calibration_error("minimum_bucket_events", "must be strictly positive")
        if self.pseudocount <= 0.0:
            raise _calibration_error("pseudocount", "must be strictly positive")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationDayOfWeekSeasonalityEstimatorSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationDayOfWeekSeasonalityEstimatorSettings(
            minimum_bucket_events=require_positive_integer(
                raw_payload,
                "minimum_bucket_events",
                f"{field_path}.minimum_bucket_events",
                CALIBRATION_ERROR_FACTORY,
            ),
            pseudocount=require_positive_finite_number(
                raw_payload,
                "pseudocount",
                f"{field_path}.pseudocount",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationDayOfYearSeasonalityEstimatorSettings(ContractModel):
    minimum_bucket_events: int
    pseudocount: float

    def __post_init__(self) -> None:
        if self.minimum_bucket_events <= 0:
            raise _calibration_error("minimum_bucket_events", "must be strictly positive")
        if self.pseudocount <= 0.0:
            raise _calibration_error("pseudocount", "must be strictly positive")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationDayOfYearSeasonalityEstimatorSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationDayOfYearSeasonalityEstimatorSettings(
            minimum_bucket_events=require_positive_integer(
                raw_payload,
                "minimum_bucket_events",
                f"{field_path}.minimum_bucket_events",
                CALIBRATION_ERROR_FACTORY,
            ),
            pseudocount=require_positive_finite_number(
                raw_payload,
                "pseudocount",
                f"{field_path}.pseudocount",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationSeasonalityStageExecutionSettings(ContractModel):
    session_filter: CalibrationTimestampEventSessionFilter
    sparse_hour_event_threshold: int
    intraday_settings: CalibrationIntradaySeasonalityEstimatorSettings
    day_of_week_settings: CalibrationDayOfWeekSeasonalityEstimatorSettings
    day_of_year_settings: CalibrationDayOfYearSeasonalityEstimatorSettings

    def __post_init__(self) -> None:
        if self.sparse_hour_event_threshold < 0:
            raise _calibration_error("sparse_hour_event_threshold", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationSeasonalityStageExecutionSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationSeasonalityStageExecutionSettings(
            session_filter=CalibrationTimestampEventSessionFilter(
                require_string(
                    raw_payload,
                    "session_filter",
                    f"{field_path}.session_filter",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            sparse_hour_event_threshold=require_non_negative_integer(
                raw_payload,
                "sparse_hour_event_threshold",
                f"{field_path}.sparse_hour_event_threshold",
                CALIBRATION_ERROR_FACTORY,
            ),
            intraday_settings=CalibrationIntradaySeasonalityEstimatorSettings.from_mapping(
                require_mapping(
                    raw_payload,
                    "intraday_settings",
                    f"{field_path}.intraday_settings",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.intraday_settings",
            ),
            day_of_week_settings=CalibrationDayOfWeekSeasonalityEstimatorSettings.from_mapping(
                require_mapping(
                    raw_payload,
                    "day_of_week_settings",
                    f"{field_path}.day_of_week_settings",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.day_of_week_settings",
            ),
            day_of_year_settings=CalibrationDayOfYearSeasonalityEstimatorSettings.from_mapping(
                require_mapping(
                    raw_payload,
                    "day_of_year_settings",
                    f"{field_path}.day_of_year_settings",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.day_of_year_settings",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationSeasonalityStageExecutionInput(ContractModel):
    timestamp_event_records: tuple[CalibrationTimestampEventRecord, ...]
    scheduled_event_overlays: tuple[CalibrationScheduledEventOverlay, ...] = ()
    settings: Optional[CalibrationSeasonalityStageExecutionSettings] = None
    dataset_id_override: Optional[str] = None
    estimator_version_override: Optional[str] = None

    def __post_init__(self) -> None:
        if len(self.timestamp_event_records) == 0:
            raise _calibration_error("timestamp_event_records", "must include at least one event record")
        if self.dataset_id_override is not None and not self.dataset_id_override.strip():
            raise _calibration_error("dataset_id_override", "must be non-empty when provided")
        if self.estimator_version_override is not None and not self.estimator_version_override.strip():
            raise _calibration_error("estimator_version_override", "must be non-empty when provided")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationSeasonalityStageExecutionInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_records = require_sequence(
            raw_payload,
            "timestamp_event_records",
            f"{field_path}.timestamp_event_records",
            CALIBRATION_ERROR_FACTORY,
        )
        raw_overlays = raw_payload.get("scheduled_event_overlays", [])
        if isinstance(raw_overlays, str) or not isinstance(raw_overlays, list):
            raise _calibration_error(f"{field_path}.scheduled_event_overlays", "must be a sequence")

        dataset_id_override = raw_payload.get("dataset_id_override")
        estimator_version_override = raw_payload.get("estimator_version_override")
        if dataset_id_override is not None and not isinstance(dataset_id_override, str):
            raise _calibration_error(f"{field_path}.dataset_id_override", "must be a string when provided")
        if estimator_version_override is not None and not isinstance(estimator_version_override, str):
            raise _calibration_error(
                f"{field_path}.estimator_version_override",
                "must be a string when provided",
            )

        return CalibrationSeasonalityStageExecutionInput(
            timestamp_event_records=tuple(
                CalibrationTimestampEventRecord.from_mapping(
                    raw_record,
                    field_path=f"{field_path}.timestamp_event_records[{record_index}]",
                )
                for record_index, raw_record in enumerate(raw_records)
            ),
            scheduled_event_overlays=tuple(
                CalibrationScheduledEventOverlay.from_mapping(
                    raw_overlay,
                    field_path=f"{field_path}.scheduled_event_overlays[{overlay_index}]",
                )
                for overlay_index, raw_overlay in enumerate(raw_overlays)
            ),
            settings=(
                CalibrationSeasonalityStageExecutionSettings.from_mapping(
                    require_mapping(
                        raw_payload,
                        "settings",
                        f"{field_path}.settings",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.settings",
                )
                if "settings" in raw_payload and raw_payload["settings"] is not None
                else None
            ),
            dataset_id_override=dataset_id_override,
            estimator_version_override=estimator_version_override,
        )


@dataclass(frozen=True, slots=True)
class CalibrationHawkesStructuredAlphaParameterization(ContractModel):
    a_self: float
    a_cross: float
    delta_self: float
    delta_cross: float

    def __post_init__(self) -> None:
        if self.a_self <= 0.0:
            raise _calibration_error("a_self", "must be strictly positive")
        if self.a_cross < 0.0:
            raise _calibration_error("a_cross", "must be non-negative")
        if self.delta_self < 0.0:
            raise _calibration_error("delta_self", "must be non-negative")
        if self.delta_cross < 0.0:
            raise _calibration_error("delta_cross", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationHawkesStructuredAlphaParameterization:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationHawkesStructuredAlphaParameterization(
            a_self=require_positive_finite_number(
                raw_payload,
                "a_self",
                f"{field_path}.a_self",
                CALIBRATION_ERROR_FACTORY,
            ),
            a_cross=require_non_negative_finite_number(
                raw_payload,
                "a_cross",
                f"{field_path}.a_cross",
                CALIBRATION_ERROR_FACTORY,
            ),
            delta_self=require_non_negative_finite_number(
                raw_payload,
                "delta_self",
                f"{field_path}.delta_self",
                CALIBRATION_ERROR_FACTORY,
            ),
            delta_cross=require_non_negative_finite_number(
                raw_payload,
                "delta_cross",
                f"{field_path}.delta_cross",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationHawkesStructuredRhoParameterization(ContractModel):
    r_self: float
    r_cross: float
    delta_self_r: float
    delta_cross_r: float

    def __post_init__(self) -> None:
        if self.r_self <= 0.0:
            raise _calibration_error("r_self", "must be strictly positive")
        if self.r_cross < 0.0:
            raise _calibration_error("r_cross", "must be non-negative")
        if self.delta_self_r < 0.0:
            raise _calibration_error("delta_self_r", "must be non-negative")
        if self.delta_cross_r < 0.0:
            raise _calibration_error("delta_cross_r", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationHawkesStructuredRhoParameterization:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationHawkesStructuredRhoParameterization(
            r_self=require_positive_finite_number(
                raw_payload,
                "r_self",
                f"{field_path}.r_self",
                CALIBRATION_ERROR_FACTORY,
            ),
            r_cross=require_non_negative_finite_number(
                raw_payload,
                "r_cross",
                f"{field_path}.r_cross",
                CALIBRATION_ERROR_FACTORY,
            ),
            delta_self_r=require_non_negative_finite_number(
                raw_payload,
                "delta_self_r",
                f"{field_path}.delta_self_r",
                CALIBRATION_ERROR_FACTORY,
            ),
            delta_cross_r=require_non_negative_finite_number(
                raw_payload,
                "delta_cross_r",
                f"{field_path}.delta_cross_r",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationHawkesStructuredRhoSelfDominancePolicy(ContractModel):
    minimum_self_to_cross_ratio: float

    def __post_init__(self) -> None:
        if self.minimum_self_to_cross_ratio < 0.0:
            raise _calibration_error("minimum_self_to_cross_ratio", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationHawkesStructuredRhoSelfDominancePolicy:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationHawkesStructuredRhoSelfDominancePolicy(
            minimum_self_to_cross_ratio=require_non_negative_finite_number(
                raw_payload,
                "minimum_self_to_cross_ratio",
                f"{field_path}.minimum_self_to_cross_ratio",
                CALIBRATION_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class CalibrationObjectiveTermInput(ContractModel):
    term_id: str
    weight: float
    observed_score: float

    def __post_init__(self) -> None:
        if not self.term_id.strip():
            raise _calibration_error("term_id", "must be a non-empty string")
        if self.weight < 0.0:
            raise _calibration_error("weight", "must be non-negative")

        require_finite_number(
            {"observed_score": self.observed_score},
            "observed_score",
            "observed_score",
            CALIBRATION_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationObjectiveTermInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationObjectiveTermInput(
            term_id=require_non_empty_string(
                raw_payload,
                "term_id",
                f"{field_path}.term_id",
                CALIBRATION_ERROR_FACTORY,
            ),
            weight=require_non_negative_finite_number(
                raw_payload,
                "weight",
                f"{field_path}.weight",
                CALIBRATION_ERROR_FACTORY,
            ),
            observed_score=require_finite_number(
                raw_payload,
                "observed_score",
                f"{field_path}.observed_score",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationHawkesStageCandidateInput(ContractModel):
    candidate_id: str
    lambda_bar: float
    beta: float
    structured_alpha: CalibrationHawkesStructuredAlphaParameterization
    structured_rho: CalibrationHawkesStructuredRhoParameterization
    self_dominance_policy: Optional[CalibrationHawkesStructuredRhoSelfDominancePolicy] = None
    stability_numeric_tolerance: float = 0.0
    objective_terms: tuple[CalibrationObjectiveTermInput, ...] = ()

    def __post_init__(self) -> None:
        if not self.candidate_id.strip():
            raise _calibration_error("candidate_id", "must be a non-empty string")

        require_finite_number({"lambda_bar": self.lambda_bar}, "lambda_bar", "lambda_bar", CALIBRATION_ERROR_FACTORY)
        require_finite_number({"beta": self.beta}, "beta", "beta", CALIBRATION_ERROR_FACTORY)
        require_non_negative_finite_number(
            {"stability_numeric_tolerance": self.stability_numeric_tolerance},
            "stability_numeric_tolerance",
            "stability_numeric_tolerance",
            CALIBRATION_ERROR_FACTORY,
        )

        if len(self.objective_terms) == 0:
            raise _calibration_error("objective_terms", "must include at least one objective term")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationHawkesStageCandidateInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_objective_terms = require_sequence(
            raw_payload,
            "objective_terms",
            f"{field_path}.objective_terms",
            CALIBRATION_ERROR_FACTORY,
        )

        return CalibrationHawkesStageCandidateInput(
            candidate_id=require_non_empty_string(
                raw_payload,
                "candidate_id",
                f"{field_path}.candidate_id",
                CALIBRATION_ERROR_FACTORY,
            ),
            lambda_bar=require_finite_number(
                raw_payload,
                "lambda_bar",
                f"{field_path}.lambda_bar",
                CALIBRATION_ERROR_FACTORY,
            ),
            beta=require_finite_number(
                raw_payload,
                "beta",
                f"{field_path}.beta",
                CALIBRATION_ERROR_FACTORY,
            ),
            structured_alpha=CalibrationHawkesStructuredAlphaParameterization.from_mapping(
                require_mapping(
                    raw_payload,
                    "structured_alpha",
                    f"{field_path}.structured_alpha",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.structured_alpha",
            ),
            structured_rho=CalibrationHawkesStructuredRhoParameterization.from_mapping(
                require_mapping(
                    raw_payload,
                    "structured_rho",
                    f"{field_path}.structured_rho",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.structured_rho",
            ),
            self_dominance_policy=(
                CalibrationHawkesStructuredRhoSelfDominancePolicy.from_mapping(
                    require_mapping(
                        raw_payload,
                        "self_dominance_policy",
                        f"{field_path}.self_dominance_policy",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.self_dominance_policy",
                )
                if "self_dominance_policy" in raw_payload and raw_payload["self_dominance_policy"] is not None
                else None
            ),
            stability_numeric_tolerance=require_non_negative_finite_number(
                raw_payload,
                "stability_numeric_tolerance",
                f"{field_path}.stability_numeric_tolerance",
                CALIBRATION_ERROR_FACTORY,
            ),
            objective_terms=tuple(
                CalibrationObjectiveTermInput.from_mapping(
                    raw_term,
                    field_path=f"{field_path}.objective_terms[{term_index}]",
                )
                for term_index, raw_term in enumerate(raw_objective_terms)
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsSignalSourceWindow(ContractModel):
    window_start_utc_nanos: int
    window_end_utc_nanos: int

    def __post_init__(self) -> None:
        if self.window_end_utc_nanos <= self.window_start_utc_nanos:
            raise _calibration_error("window_end_utc_nanos", "must be strictly greater than window_start_utc_nanos")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsSignalSourceWindow:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsSignalSourceWindow(
            window_start_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "window_start_utc_nanos",
                f"{field_path}.window_start_utc_nanos",
            ),
            window_end_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "window_end_utc_nanos",
                f"{field_path}.window_end_utc_nanos",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsKnownNewsCalendarInput(ContractModel):
    source_window: CalibrationReturnComponentsSignalSourceWindow
    scheduled_event_timestamps_utc_nanos: tuple[int, ...]

    def __post_init__(self) -> None:
        previous_timestamp: Optional[int] = None
        for timestamp_utc_nanos in self.scheduled_event_timestamps_utc_nanos:
            if previous_timestamp is not None and timestamp_utc_nanos <= previous_timestamp:
                raise _calibration_error(
                    "scheduled_event_timestamps_utc_nanos",
                    "must be strictly increasing",
                )
            previous_timestamp = timestamp_utc_nanos

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsKnownNewsCalendarInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_timestamps = require_sequence(
            raw_payload,
            "scheduled_event_timestamps_utc_nanos",
            f"{field_path}.scheduled_event_timestamps_utc_nanos",
            CALIBRATION_ERROR_FACTORY,
        )

        return CalibrationReturnComponentsKnownNewsCalendarInput(
            source_window=CalibrationReturnComponentsSignalSourceWindow.from_mapping(
                require_mapping(
                    raw_payload,
                    "source_window",
                    f"{field_path}.source_window",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.source_window",
            ),
            scheduled_event_timestamps_utc_nanos=tuple(
                require_integer(
                    {"value": raw_timestamp},
                    "value",
                    f"{field_path}.scheduled_event_timestamps_utc_nanos[{timestamp_index}]",
                    CALIBRATION_ERROR_FACTORY,
                )
                for timestamp_index, raw_timestamp in enumerate(raw_timestamps)
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsKnownNewsReturnSample(ContractModel):
    event_timestamp_utc_nanos: int
    open_window_jump_return: float
    closed_window_gap_return: float

    def __post_init__(self) -> None:
        require_finite_number(
            {"open_window_jump_return": self.open_window_jump_return},
            "open_window_jump_return",
            "open_window_jump_return",
            CALIBRATION_ERROR_FACTORY,
        )
        require_finite_number(
            {"closed_window_gap_return": self.closed_window_gap_return},
            "closed_window_gap_return",
            "closed_window_gap_return",
            CALIBRATION_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsKnownNewsReturnSample:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsKnownNewsReturnSample(
            event_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "event_timestamp_utc_nanos",
                f"{field_path}.event_timestamp_utc_nanos",
            ),
            open_window_jump_return=require_finite_number(
                raw_payload,
                "open_window_jump_return",
                f"{field_path}.open_window_jump_return",
                CALIBRATION_ERROR_FACTORY,
            ),
            closed_window_gap_return=require_finite_number(
                raw_payload,
                "closed_window_gap_return",
                f"{field_path}.closed_window_gap_return",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsDetectedIntradayJumpCandidate(ContractModel):
    jump_timestamp_utc_nanos: int
    jump_return: float
    post_shock_activity_lift: float

    def __post_init__(self) -> None:
        require_finite_number({"jump_return": self.jump_return}, "jump_return", "jump_return", CALIBRATION_ERROR_FACTORY)
        if self.post_shock_activity_lift < 0.0:
            raise _calibration_error("post_shock_activity_lift", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsDetectedIntradayJumpCandidate:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsDetectedIntradayJumpCandidate(
            jump_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "jump_timestamp_utc_nanos",
                f"{field_path}.jump_timestamp_utc_nanos",
            ),
            jump_return=require_finite_number(
                raw_payload,
                "jump_return",
                f"{field_path}.jump_return",
                CALIBRATION_ERROR_FACTORY,
            ),
            post_shock_activity_lift=require_non_negative_finite_number(
                raw_payload,
                "post_shock_activity_lift",
                f"{field_path}.post_shock_activity_lift",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsObservedCloseOpenBoundaryObservation(ContractModel):
    close_timestamp_utc_nanos: int
    close_price: float
    open_timestamp_utc_nanos: int
    open_price: float
    post_shock_activity_lift: float

    def __post_init__(self) -> None:
        if self.open_timestamp_utc_nanos <= self.close_timestamp_utc_nanos:
            raise _calibration_error("open_timestamp_utc_nanos", "must be strictly greater than close_timestamp_utc_nanos")

        require_positive_finite_number({"close_price": self.close_price}, "close_price", "close_price", CALIBRATION_ERROR_FACTORY)
        require_positive_finite_number({"open_price": self.open_price}, "open_price", "open_price", CALIBRATION_ERROR_FACTORY)
        if self.post_shock_activity_lift < 0.0:
            raise _calibration_error("post_shock_activity_lift", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsObservedCloseOpenBoundaryObservation:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsObservedCloseOpenBoundaryObservation(
            close_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "close_timestamp_utc_nanos",
                f"{field_path}.close_timestamp_utc_nanos",
            ),
            close_price=require_positive_finite_number(
                raw_payload,
                "close_price",
                f"{field_path}.close_price",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "open_timestamp_utc_nanos",
                f"{field_path}.open_timestamp_utc_nanos",
            ),
            open_price=require_positive_finite_number(
                raw_payload,
                "open_price",
                f"{field_path}.open_price",
                CALIBRATION_ERROR_FACTORY,
            ),
            post_shock_activity_lift=require_non_negative_finite_number(
                raw_payload,
                "post_shock_activity_lift",
                f"{field_path}.post_shock_activity_lift",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsCloseOpenGapDetectionPolicy(ContractModel):
    material_gap_absolute_return_tolerance: float

    def __post_init__(self) -> None:
        if self.material_gap_absolute_return_tolerance < 0.0:
            raise _calibration_error("material_gap_absolute_return_tolerance", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsCloseOpenGapDetectionPolicy:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsCloseOpenGapDetectionPolicy(
            material_gap_absolute_return_tolerance=require_non_negative_finite_number(
                raw_payload,
                "material_gap_absolute_return_tolerance",
                f"{field_path}.material_gap_absolute_return_tolerance",
                CALIBRATION_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsObjectiveMetricWeights(ContractModel):
    open_jump_fit_weight: float
    closed_gap_fit_weight: float
    activity_coupling_fit_weight: float

    def __post_init__(self) -> None:
        if self.open_jump_fit_weight < 0.0:
            raise _calibration_error("open_jump_fit_weight", "must be non-negative")
        if self.closed_gap_fit_weight < 0.0:
            raise _calibration_error("closed_gap_fit_weight", "must be non-negative")
        if self.activity_coupling_fit_weight < 0.0:
            raise _calibration_error("activity_coupling_fit_weight", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsObjectiveMetricWeights:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsObjectiveMetricWeights(
            open_jump_fit_weight=require_non_negative_finite_number(
                raw_payload,
                "open_jump_fit_weight",
                f"{field_path}.open_jump_fit_weight",
                CALIBRATION_ERROR_FACTORY,
            ),
            closed_gap_fit_weight=require_non_negative_finite_number(
                raw_payload,
                "closed_gap_fit_weight",
                f"{field_path}.closed_gap_fit_weight",
                CALIBRATION_ERROR_FACTORY,
            ),
            activity_coupling_fit_weight=require_non_negative_finite_number(
                raw_payload,
                "activity_coupling_fit_weight",
                f"{field_path}.activity_coupling_fit_weight",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationDoubleExponentialBaseShockConfiguration(ContractModel):
    probability_positive: float
    eta_plus: float
    eta_minus: float

    def __post_init__(self) -> None:
        if self.probability_positive <= 0.0 or self.probability_positive >= 1.0:
            raise _calibration_error("probability_positive", "must be strictly between 0 and 1")
        if self.eta_plus <= 0.0:
            raise _calibration_error("eta_plus", "must be strictly positive")
        if self.eta_minus <= 0.0:
            raise _calibration_error("eta_minus", "must be strictly positive")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationDoubleExponentialBaseShockConfiguration:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationDoubleExponentialBaseShockConfiguration(
            probability_positive=require_finite_number(
                raw_payload,
                "probability_positive",
                f"{field_path}.probability_positive",
                CALIBRATION_ERROR_FACTORY,
            ),
            eta_plus=require_positive_finite_number(
                raw_payload,
                "eta_plus",
                f"{field_path}.eta_plus",
                CALIBRATION_ERROR_FACTORY,
            ),
            eta_minus=require_positive_finite_number(
                raw_payload,
                "eta_minus",
                f"{field_path}.eta_minus",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationCoupledGapMarkConfiguration(ContractModel):
    sigma_g: float

    def __post_init__(self) -> None:
        if self.sigma_g < 0.0:
            raise _calibration_error("sigma_g", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationCoupledGapMarkConfiguration:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationCoupledGapMarkConfiguration(
            sigma_g=require_non_negative_finite_number(
                raw_payload,
                "sigma_g",
                f"{field_path}.sigma_g",
                CALIBRATION_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class CalibrationSharedNewsMarkLawConfiguration(ContractModel):
    gamma_severity_marks: GammaSeverityMarkConfiguration
    double_exponential_base_shock: CalibrationDoubleExponentialBaseShockConfiguration
    coupled_gap_marks: CalibrationCoupledGapMarkConfiguration

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationSharedNewsMarkLawConfiguration:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationSharedNewsMarkLawConfiguration(
            gamma_severity_marks=GammaSeverityMarkConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "gamma_severity_marks",
                    f"{field_path}.gamma_severity_marks",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.gamma_severity_marks",
            ),
            double_exponential_base_shock=CalibrationDoubleExponentialBaseShockConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "double_exponential_base_shock",
                    f"{field_path}.double_exponential_base_shock",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.double_exponential_base_shock",
            ),
            coupled_gap_marks=CalibrationCoupledGapMarkConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "coupled_gap_marks",
                    f"{field_path}.coupled_gap_marks",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.coupled_gap_marks",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsStageCandidateGenerationInput(ContractModel):
    known_news_calendar: CalibrationReturnComponentsKnownNewsCalendarInput
    realized_known_news_return_samples: tuple[CalibrationReturnComponentsKnownNewsReturnSample, ...]
    detected_intraday_jump_candidates: tuple[CalibrationReturnComponentsDetectedIntradayJumpCandidate, ...]
    observed_close_open_boundary_observations: tuple[CalibrationReturnComponentsObservedCloseOpenBoundaryObservation, ...]
    close_open_gap_detection_policy: CalibrationReturnComponentsCloseOpenGapDetectionPolicy
    nig_shared_plus_shift_candidates: tuple[NigSharedPlusShiftConfiguration, ...]
    shared_mark_law_template: CalibrationSharedNewsMarkLawConfiguration
    objective_metric_weights: CalibrationReturnComponentsObjectiveMetricWeights

    def __post_init__(self) -> None:
        if len(self.detected_intraday_jump_candidates) == 0:
            raise _calibration_error(
                "detected_intraday_jump_candidates",
                "must include at least one intraday jump candidate",
            )
        if len(self.nig_shared_plus_shift_candidates) == 0:
            raise _calibration_error(
                "nig_shared_plus_shift_candidates",
                "must include at least one NIG candidate",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsStageCandidateGenerationInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        known_news_return_samples = require_sequence(
            raw_payload,
            "realized_known_news_return_samples",
            f"{field_path}.realized_known_news_return_samples",
            CALIBRATION_ERROR_FACTORY,
        )
        intraday_jump_candidates = require_sequence(
            raw_payload,
            "detected_intraday_jump_candidates",
            f"{field_path}.detected_intraday_jump_candidates",
            CALIBRATION_ERROR_FACTORY,
        )
        boundary_observations = require_sequence(
            raw_payload,
            "observed_close_open_boundary_observations",
            f"{field_path}.observed_close_open_boundary_observations",
            CALIBRATION_ERROR_FACTORY,
        )
        nig_candidates = require_sequence(
            raw_payload,
            "nig_shared_plus_shift_candidates",
            f"{field_path}.nig_shared_plus_shift_candidates",
            CALIBRATION_ERROR_FACTORY,
        )

        return CalibrationReturnComponentsStageCandidateGenerationInput(
            known_news_calendar=CalibrationReturnComponentsKnownNewsCalendarInput.from_mapping(
                require_mapping(
                    raw_payload,
                    "known_news_calendar",
                    f"{field_path}.known_news_calendar",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.known_news_calendar",
            ),
            realized_known_news_return_samples=tuple(
                CalibrationReturnComponentsKnownNewsReturnSample.from_mapping(
                    raw_sample,
                    field_path=f"{field_path}.realized_known_news_return_samples[{sample_index}]",
                )
                for sample_index, raw_sample in enumerate(known_news_return_samples)
            ),
            detected_intraday_jump_candidates=tuple(
                CalibrationReturnComponentsDetectedIntradayJumpCandidate.from_mapping(
                    raw_candidate,
                    field_path=f"{field_path}.detected_intraday_jump_candidates[{candidate_index}]",
                )
                for candidate_index, raw_candidate in enumerate(intraday_jump_candidates)
            ),
            observed_close_open_boundary_observations=tuple(
                CalibrationReturnComponentsObservedCloseOpenBoundaryObservation.from_mapping(
                    raw_candidate,
                    field_path=f"{field_path}.observed_close_open_boundary_observations[{candidate_index}]",
                )
                for candidate_index, raw_candidate in enumerate(boundary_observations)
            ),
            close_open_gap_detection_policy=CalibrationReturnComponentsCloseOpenGapDetectionPolicy.from_mapping(
                require_mapping(
                    raw_payload,
                    "close_open_gap_detection_policy",
                    f"{field_path}.close_open_gap_detection_policy",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.close_open_gap_detection_policy",
            ),
            nig_shared_plus_shift_candidates=tuple(
                NigSharedPlusShiftConfiguration.from_mapping(
                    raw_candidate,
                    field_path=f"{field_path}.nig_shared_plus_shift_candidates[{candidate_index}]",
                )
                for candidate_index, raw_candidate in enumerate(nig_candidates)
            ),
            shared_mark_law_template=CalibrationSharedNewsMarkLawConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "shared_mark_law_template",
                    f"{field_path}.shared_mark_law_template",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.shared_mark_law_template",
            ),
            objective_metric_weights=CalibrationReturnComponentsObjectiveMetricWeights.from_mapping(
                require_mapping(
                    raw_payload,
                    "objective_metric_weights",
                    f"{field_path}.objective_metric_weights",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.objective_metric_weights",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationStagedExecutionRuntimeResources(ContractModel):
    disk_checkpoint_directory: Optional[str] = None

    def __post_init__(self) -> None:
        if self.disk_checkpoint_directory is not None and not self.disk_checkpoint_directory.strip():
            raise _calibration_error("disk_checkpoint_directory", "must be non-empty when provided")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationStagedExecutionRuntimeResources:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        disk_checkpoint_directory = raw_payload.get("disk_checkpoint_directory")
        if disk_checkpoint_directory is not None and not isinstance(disk_checkpoint_directory, str):
            raise _calibration_error(
                f"{field_path}.disk_checkpoint_directory",
                "must be a string when provided",
            )

        return CalibrationStagedExecutionRuntimeResources(
            disk_checkpoint_directory=disk_checkpoint_directory,
        )


@dataclass(frozen=True, slots=True)
class StagedCalibrationExecutionInput(ContractModel):
    schema_version: str
    request: StagedCalibrationRequest
    seasonality_stage_input: Optional[CalibrationSeasonalityStageExecutionInput] = None
    hawkes_candidate_inputs: Optional[tuple[CalibrationHawkesStageCandidateInput, ...]] = None
    return_components_candidate_generation_input: Optional[
        CalibrationReturnComponentsStageCandidateGenerationInput
    ] = None
    runtime_resources: Optional[CalibrationStagedExecutionRuntimeResources] = None

    def __post_init__(self) -> None:
        if self.schema_version != CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION:
            raise _calibration_error(
                "schema_version",
                f"must be `{CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION}`",
            )

        selected_stage_set = set(self.request.selected_stages)

        if CalibrationStage.SEASONALITY in selected_stage_set and self.seasonality_stage_input is None:
            raise _calibration_error(
                "seasonality_stage_input",
                "is required when selected_stages contains seasonality",
            )
        if CalibrationStage.SEASONALITY not in selected_stage_set and self.seasonality_stage_input is not None:
            raise _calibration_error(
                "seasonality_stage_input",
                "must not be provided when seasonality is not selected",
            )

        if CalibrationStage.HAWKES in selected_stage_set and self.hawkes_candidate_inputs is None:
            raise _calibration_error(
                "hawkes_candidate_inputs",
                "is required when selected_stages contains hawkes",
            )
        if CalibrationStage.HAWKES not in selected_stage_set and self.hawkes_candidate_inputs is not None:
            raise _calibration_error(
                "hawkes_candidate_inputs",
                "must not be provided when hawkes is not selected",
            )
        if self.hawkes_candidate_inputs is not None and len(self.hawkes_candidate_inputs) == 0:
            raise _calibration_error(
                "hawkes_candidate_inputs",
                "must include at least one Hawkes candidate when provided",
            )

        if (
            CalibrationStage.RETURN_COMPONENTS in selected_stage_set
            and self.return_components_candidate_generation_input is None
        ):
            raise _calibration_error(
                "return_components_candidate_generation_input",
                "is required when selected_stages contains return_components",
            )
        if (
            CalibrationStage.RETURN_COMPONENTS not in selected_stage_set
            and self.return_components_candidate_generation_input is not None
        ):
            raise _calibration_error(
                "return_components_candidate_generation_input",
                "must not be provided when return_components is not selected",
            )

        match self.request.execution_mode:
            case CalibrationExecutionMode.MEMORY:
                if self.runtime_resources is not None and self.runtime_resources.disk_checkpoint_directory is not None:
                    raise _calibration_error(
                        "runtime_resources.disk_checkpoint_directory",
                        "is only supported when execution_mode=disk",
                    )
            case CalibrationExecutionMode.DISK:
                if (
                    self.runtime_resources is None
                    or self.runtime_resources.disk_checkpoint_directory is None
                ):
                    raise _calibration_error(
                        "runtime_resources.disk_checkpoint_directory",
                        "is required when execution_mode=disk",
                    )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> StagedCalibrationExecutionInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error("staged_calibration_execution_input", "must be an object")

        raw_hawkes_candidate_inputs = raw_payload.get("hawkes_candidate_inputs")
        if raw_hawkes_candidate_inputs is not None and (
            isinstance(raw_hawkes_candidate_inputs, str) or not isinstance(raw_hawkes_candidate_inputs, list)
        ):
            raise _calibration_error("hawkes_candidate_inputs", "must be a sequence when provided")

        return StagedCalibrationExecutionInput(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                "schema_version",
                CALIBRATION_ERROR_FACTORY,
            ),
            request=StagedCalibrationRequest.from_mapping(
                require_mapping(raw_payload, "request", "request", CALIBRATION_ERROR_FACTORY),
                field_path_prefix="request",
            ),
            seasonality_stage_input=(
                CalibrationSeasonalityStageExecutionInput.from_mapping(
                    require_mapping(
                        raw_payload,
                        "seasonality_stage_input",
                        "seasonality_stage_input",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path="seasonality_stage_input",
                )
                if "seasonality_stage_input" in raw_payload and raw_payload["seasonality_stage_input"] is not None
                else None
            ),
            hawkes_candidate_inputs=(
                tuple(
                    CalibrationHawkesStageCandidateInput.from_mapping(
                        raw_candidate,
                        field_path=f"hawkes_candidate_inputs[{candidate_index}]",
                    )
                    for candidate_index, raw_candidate in enumerate(raw_hawkes_candidate_inputs)
                )
                if raw_hawkes_candidate_inputs is not None
                else None
            ),
            return_components_candidate_generation_input=(
                CalibrationReturnComponentsStageCandidateGenerationInput.from_mapping(
                    require_mapping(
                        raw_payload,
                        "return_components_candidate_generation_input",
                        "return_components_candidate_generation_input",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path="return_components_candidate_generation_input",
                )
                if (
                    "return_components_candidate_generation_input" in raw_payload
                    and raw_payload["return_components_candidate_generation_input"] is not None
                )
                else None
            ),
            runtime_resources=(
                CalibrationStagedExecutionRuntimeResources.from_mapping(
                    require_mapping(
                        raw_payload,
                        "runtime_resources",
                        "runtime_resources",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path="runtime_resources",
                )
                if "runtime_resources" in raw_payload and raw_payload["runtime_resources"] is not None
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


def validate_market_data_calibration_request(raw_request: dict[str, Any] | Any) -> MarketDataCalibrationRequest:
    return MarketDataCalibrationRequest.from_mapping(raw_request)


def validate_staged_calibration_execution_input(
    raw_execution_input: dict[str, Any] | Any,
) -> StagedCalibrationExecutionInput:
    return StagedCalibrationExecutionInput.from_mapping(raw_execution_input)


def validate_staged_calibration_response(raw_response: dict[str, Any] | Any) -> StagedCalibrationResponse:
    return StagedCalibrationResponse.from_mapping(raw_response)


def serialize_staged_calibration_request(request: StagedCalibrationRequest) -> str:
    return serialize_deterministic_json(request)


def serialize_market_data_calibration_request(request: MarketDataCalibrationRequest) -> str:
    return serialize_deterministic_json(request)


def serialize_staged_calibration_execution_input(execution_input: StagedCalibrationExecutionInput) -> str:
    return serialize_deterministic_json(execution_input)


def deserialize_staged_calibration_request(payload_json: str) -> StagedCalibrationRequest:
    raw_payload = parse_json_object(payload_json, "staged calibration request", CALIBRATION_ERROR_FACTORY)
    return validate_staged_calibration_request(dict(raw_payload))


def deserialize_market_data_calibration_request(payload_json: str) -> MarketDataCalibrationRequest:
    raw_payload = parse_json_object(payload_json, "market-data calibration request", CALIBRATION_ERROR_FACTORY)
    return validate_market_data_calibration_request(dict(raw_payload))


def deserialize_staged_calibration_execution_input(payload_json: str) -> StagedCalibrationExecutionInput:
    raw_payload = parse_json_object(
        payload_json,
        "staged calibration execution input",
        CALIBRATION_ERROR_FACTORY,
    )
    return validate_staged_calibration_execution_input(dict(raw_payload))


def serialize_staged_calibration_response(response: StagedCalibrationResponse) -> str:
    return serialize_deterministic_json(response)


def deserialize_staged_calibration_response(payload_json: str) -> StagedCalibrationResponse:
    raw_payload = parse_json_object(payload_json, "staged calibration response", CALIBRATION_ERROR_FACTORY)
    return validate_staged_calibration_response(dict(raw_payload))


def run_staged_calibration(
    execution_input: StagedCalibrationExecutionInput,
    *,
    calendar_registry: CalendarRegistry,
) -> StagedCalibrationResponse:
    """Run staged calibration from the explicit execution contract.

    Example:
        response = run_staged_calibration(
            execution_input,
            calendar_registry=calendar_registry,
        )

    Return-components stage inputs keep known-news open-window jumps and
    closed-window gaps explicit while deriving surprise close-open gaps
    from raw boundary observations inside Janiform.
    """

    if not isinstance(execution_input, StagedCalibrationExecutionInput):
        raise _calibration_error("execution_input", "must be a StagedCalibrationExecutionInput")
    if not isinstance(calendar_registry, CalendarRegistry):
        raise _calibration_error("calendar_registry", "must be a CalendarRegistry")

    normalized_execution_input = validate_staged_calibration_execution_input(
        execution_input.to_payload()
    )

    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as e:
        raise _calibration_integration_error(
            "native calibration module is unavailable",
            entrypoint="run_staged_calibration",
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    try:
        response_json: str = native.run_staged_calibration(
            serialize_staged_calibration_execution_input(normalized_execution_input),
            calendar_registry._native_registry,
        )
    except native.CalibrationExecutionError as e:
        raise _calibration_execution_error(
            "staged calibration failed during native execution",
            entrypoint="run_staged_calibration",
            context={"source_exception_type": type(e).__name__},
        ) from e
    except Exception as e:
        raise _calibration_execution_error(
            "staged calibration failed during native execution",
            entrypoint="run_staged_calibration",
            context={"source_exception_type": type(e).__name__},
        ) from e

    try:
        return deserialize_staged_calibration_response(response_json)
    except CalibrationValidationError as e:
        raise _calibration_integration_error(
            "native calibration returned an invalid response payload",
            entrypoint="run_staged_calibration",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


__all__ = [
    "CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY",
    "CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION",
    "CALIBRATION_RUN_SPEC_SCHEMA_VERSION",
    "CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION",
    "CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION",
    "CalibrationCoupledGapMarkConfiguration",
    "CalibrationCheckpointMetadata",
    "CalibrationDayOfWeekSeasonalityEstimatorSettings",
    "CalibrationDayOfYearSeasonalityEstimatorSettings",
    "CalibrationDoubleExponentialBaseShockConfiguration",
    "CalibrationExecutionMode",
    "CalibrationFailureDetails",
    "CalibrationDuplicateRecordPolicy",
    "CalibrationHawkesStageCandidateInput",
    "CalibrationHawkesStructuredAlphaParameterization",
    "CalibrationHawkesStructuredRhoParameterization",
    "CalibrationHawkesStructuredRhoSelfDominancePolicy",
    "CalibrationHandoffTransport",
    "CalibrationInputFileFormat",
    "CalibrationInputFileReference",
    "CalibrationIntradaySeasonalityEstimatorSettings",
    "CalibrationIntradaySmoothingKernel",
    "CalibrationKnownNewsColumnMapping",
    "CalibrationKnownNewsInput",
    "CalibrationMarketDataColumnMapping",
    "CalibrationMarketDataNormalizationPolicy",
    "CalibrationMarketDataOutputPreferences",
    "CalibrationMarketDataWorkflowScope",
    "CalibrationOptimizerSettings",
    "CalibrationOptionQuoteInputPolicy",
    "CalibrationOptionSurfaceColumnMapping",
    "CalibrationObjectiveTermInput",
    "CalibrationReturnComponentsCloseOpenGapDetectionPolicy",
    "CalibrationReturnComponentsDetectedIntradayJumpCandidate",
    "CalibrationReturnComponentsKnownNewsCalendarInput",
    "CalibrationReturnComponentsKnownNewsReturnSample",
    "CalibrationReturnComponentsObservedCloseOpenBoundaryObservation",
    "CalibrationReturnComponentsObjectiveMetricWeights",
    "CalibrationReturnComponentsSignalSourceWindow",
    "CalibrationReturnComponentsStageCandidateGenerationInput",
    "CalibrationResumeSettings",
    "CalibrationResumeSource",
    "CalibrationRunDataInputs",
    "CalibrationRunSeedSettings",
    "CalibrationScheduledEventOverlay",
    "CalibrationSeasonalityStageExecutionInput",
    "CalibrationSeasonalityStageExecutionSettings",
    "CalibrationSessionConfigurationInput",
    "CalibrationSharedNewsMarkLawConfiguration",
    "CalibrationStage",
    "CalibrationStageExecutionStatus",
    "CalibrationStageHandoff",
    "CalibrationStageReport",
    "CalibrationStageSeedOverride",
    "CalibrationStageSolverSettings",
    "CalibrationStagedExecutionRuntimeResources",
    "CalibrationTimestampEventRecord",
    "CalibrationTimestampEventSessionFilter",
    "CalibrationTickDataColumnMapping",
    "CalibrationTickPriceInputPolicy",
    "CalibrationValidationError",
    "CalibrationValidationFailure",
    "DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE",
    "MarketDataCalibrationRequest",
    "StagedCalibrationExecutionInput",
    "StagedCalibrationRequest",
    "StagedCalibrationResponse",
    "deserialize_market_data_calibration_request",
    "deserialize_staged_calibration_execution_input",
    "deserialize_staged_calibration_request",
    "deserialize_staged_calibration_response",
    "run_staged_calibration",
    "serialize_market_data_calibration_request",
    "serialize_staged_calibration_execution_input",
    "serialize_staged_calibration_request",
    "serialize_staged_calibration_response",
    "validate_market_data_calibration_request",
    "validate_staged_calibration_execution_input",
    "validate_staged_calibration_request",
    "validate_staged_calibration_response",
]
