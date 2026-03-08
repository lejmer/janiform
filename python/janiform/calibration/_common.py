from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Optional, cast

from .._contracts import (
    ContractModel,
    ValidationErrorFactory,
    require_finite_number,
    require_mapping,
    require_non_empty_string,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_integer,
    require_string,
)
from ..errors import (
    ContractValidationFailure,
    JaniformErrorCategory,
    JaniformErrorDetails,
    JaniformExecutionError,
    JaniformIntegrationError,
    JaniformValidationError,
)

CALIBRATION_RUN_SPEC_SCHEMA_VERSION = "calibration.orchestration.run_spec.v1"
CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION = "calibration.market_data.request.v1"
CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION = "calibration.market_data.preparation.v1"
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
