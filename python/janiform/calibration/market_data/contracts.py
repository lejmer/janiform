from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from math import isfinite
from typing import Any, Optional, cast

from ...seasonality import IntradaySmoothingKernel, SessionFilter
from .._errors import (
    CALIBRATION_ERROR_FACTORY,
    _calibration_error,
    _require_bool,
)
from ..staged.contracts import (
    DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE,
    CalibrationExecutionMode,
    _parse_calibration_execution_mode,
)
from ...contracts import (
    ContractModel,
    require_finite_number,
    require_mapping,
    require_non_empty_string,
    require_positive_finite_number,
    require_positive_integer,
    require_string,
)
from ...contracts.base import require_only_supported_fields


CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION = "calibration.market_data.request.v1"
CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION = "calibration.market_data.preparation.v1"


class CalibrationInputFileFormat(StrEnum):
    CSV = "csv"
    PARQUET = "parquet"
    JSON = "json"


class CalibrationMarketDataWorkflowScope(StrEnum):
    SEASONALITY_ONLY = "seasonality_only"
    FULL_WITHOUT_OPTION_SURFACE = "full_without_option_surface"
    FULL_WITH_OPTION_SURFACE = "full_with_option_surface"


class CalibrationDuplicateRecordPolicy(StrEnum):
    KEEP_FIRST = "keep_first"
    KEEP_LAST = "keep_last"


class CalibrationTickPriceInputPolicy(StrEnum):
    EXPLICIT_PRICE_COLUMN = "explicit_price_column"
    BID_ASK_MIDPOINT = "bid_ask_midpoint"


class CalibrationTradeSign(StrEnum):
    BUY = "buy"
    SELL = "sell"


class CalibrationOptionQuoteInputPolicy(StrEnum):
    MID_PRICE_COLUMN = "mid_price_column"
    BID_ASK_MIDPOINT = "bid_ask_midpoint"
    IMPLIED_VOLATILITY_COLUMN = "implied_volatility_column"


@dataclass(frozen=True, slots=True)
class CalibrationMeanReversionConfiguration(ContractModel):
    calibration_grid_interval_hours: Optional[float] = None

    def __post_init__(self) -> None:
        if self.calibration_grid_interval_hours is not None and self.calibration_grid_interval_hours <= 0.0:
            raise _calibration_error(
                "calibration_grid_interval_hours",
                "must be strictly positive when provided",
            )

    @property
    def enabled(self) -> bool:
        return True

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationMeanReversionConfiguration:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=("calibration_grid_interval_hours",),
            field_path=field_path,
            error_factory=_calibration_error,
        )

        calibration_grid_interval_hours = (
            require_positive_finite_number(
                raw_payload,
                "calibration_grid_interval_hours",
                f"{field_path}.calibration_grid_interval_hours",
                CALIBRATION_ERROR_FACTORY,
            )
            if "calibration_grid_interval_hours" in raw_payload
            and raw_payload["calibration_grid_interval_hours"] is not None
            else None
        )

        return CalibrationMeanReversionConfiguration(
            calibration_grid_interval_hours=calibration_grid_interval_hours,
        )


@dataclass(frozen=True, slots=True)
class CalibrationMarketDataSeasonalityPreprocessingSettings(ContractModel):
    session_filter: SessionFilter = SessionFilter.OPEN_ONLY
    sparse_hour_event_threshold: int = 1
    known_news_overlay_log_adjustment: float = 0.0
    intraday_minutes_per_bin: int = 1
    intraday_smoothing_kernel: IntradaySmoothingKernel = IntradaySmoothingKernel.GAUSSIAN
    intraday_smoothing_bandwidth_bins: float = 3.0
    intraday_minimum_bin_events: int = 1
    day_of_week_minimum_bucket_events: int = 1
    day_of_year_minimum_bucket_events: int = 1

    def __post_init__(self) -> None:
        if self.sparse_hour_event_threshold < 1:
            raise _calibration_error("sparse_hour_event_threshold", "must be at least 1")
        if self.intraday_minutes_per_bin < 1:
            raise _calibration_error("intraday_minutes_per_bin", "must be at least 1")
        if not isfinite(self.known_news_overlay_log_adjustment):
            raise _calibration_error(
                "known_news_overlay_log_adjustment",
                "must be finite",
            )
        if (
            not isfinite(self.intraday_smoothing_bandwidth_bins)
            or self.intraday_smoothing_bandwidth_bins <= 0.0
        ):
            raise _calibration_error(
                "intraday_smoothing_bandwidth_bins",
                "must be finite and strictly positive",
            )
        if self.intraday_minimum_bin_events < 1:
            raise _calibration_error("intraday_minimum_bin_events", "must be at least 1")
        if self.day_of_week_minimum_bucket_events < 1:
            raise _calibration_error(
                "day_of_week_minimum_bucket_events",
                "must be at least 1",
            )
        if self.day_of_year_minimum_bucket_events < 1:
            raise _calibration_error(
                "day_of_year_minimum_bucket_events",
                "must be at least 1",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationMarketDataSeasonalityPreprocessingSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationMarketDataSeasonalityPreprocessingSettings(
            session_filter=SessionFilter(
                require_string(
                    raw_payload,
                    "session_filter",
                    f"{field_path}.session_filter",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            sparse_hour_event_threshold=require_positive_integer(
                raw_payload,
                "sparse_hour_event_threshold",
                f"{field_path}.sparse_hour_event_threshold",
                CALIBRATION_ERROR_FACTORY,
            ),
            known_news_overlay_log_adjustment=(
                require_finite_number(
                    raw_payload,
                    "known_news_overlay_log_adjustment",
                    f"{field_path}.known_news_overlay_log_adjustment",
                    CALIBRATION_ERROR_FACTORY,
                )
                if "known_news_overlay_log_adjustment" in raw_payload
                and raw_payload["known_news_overlay_log_adjustment"] is not None
                else 0.0
            ),
            intraday_minutes_per_bin=require_positive_integer(
                raw_payload,
                "intraday_minutes_per_bin",
                f"{field_path}.intraday_minutes_per_bin",
                CALIBRATION_ERROR_FACTORY,
            ),
            intraday_smoothing_kernel=IntradaySmoothingKernel(
                require_string(
                    raw_payload,
                    "intraday_smoothing_kernel",
                    f"{field_path}.intraday_smoothing_kernel",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            intraday_smoothing_bandwidth_bins=require_positive_finite_number(
                raw_payload,
                "intraday_smoothing_bandwidth_bins",
                f"{field_path}.intraday_smoothing_bandwidth_bins",
                CALIBRATION_ERROR_FACTORY,
            ),
            intraday_minimum_bin_events=require_positive_integer(
                raw_payload,
                "intraday_minimum_bin_events",
                f"{field_path}.intraday_minimum_bin_events",
                CALIBRATION_ERROR_FACTORY,
            ),
            day_of_week_minimum_bucket_events=require_positive_integer(
                raw_payload,
                "day_of_week_minimum_bucket_events",
                f"{field_path}.day_of_week_minimum_bucket_events",
                CALIBRATION_ERROR_FACTORY,
            ),
            day_of_year_minimum_bucket_events=require_positive_integer(
                raw_payload,
                "day_of_year_minimum_bucket_events",
                f"{field_path}.day_of_year_minimum_bucket_events",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationMarketDataReturnComponentsPreprocessingSettings(ContractModel):
    intraday_jump_threshold_multiplier: float = 4.0
    close_open_gap_tolerance_multiplier: float = 3.0

    def __post_init__(self) -> None:
        if (
            not isfinite(self.intraday_jump_threshold_multiplier)
            or self.intraday_jump_threshold_multiplier <= 0.0
        ):
            raise _calibration_error(
                "intraday_jump_threshold_multiplier",
                "must be finite and strictly positive",
            )
        if (
            not isfinite(self.close_open_gap_tolerance_multiplier)
            or self.close_open_gap_tolerance_multiplier <= 0.0
        ):
            raise _calibration_error(
                "close_open_gap_tolerance_multiplier",
                "must be finite and strictly positive",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationMarketDataReturnComponentsPreprocessingSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationMarketDataReturnComponentsPreprocessingSettings(
            intraday_jump_threshold_multiplier=require_positive_finite_number(
                raw_payload,
                "intraday_jump_threshold_multiplier",
                f"{field_path}.intraday_jump_threshold_multiplier",
                CALIBRATION_ERROR_FACTORY,
            ),
            close_open_gap_tolerance_multiplier=require_positive_finite_number(
                raw_payload,
                "close_open_gap_tolerance_multiplier",
                f"{field_path}.close_open_gap_tolerance_multiplier",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationMarketDataPreprocessingSettings(ContractModel):
    seasonality: Optional[CalibrationMarketDataSeasonalityPreprocessingSettings] = None
    return_components: Optional[CalibrationMarketDataReturnComponentsPreprocessingSettings] = None

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationMarketDataPreprocessingSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationMarketDataPreprocessingSettings(
            seasonality=(
                CalibrationMarketDataSeasonalityPreprocessingSettings.from_mapping(
                    require_mapping(
                        raw_payload,
                        "seasonality",
                        f"{field_path}.seasonality",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.seasonality",
                )
                if "seasonality" in raw_payload and raw_payload["seasonality"] is not None
                else None
            ),
            return_components=(
                CalibrationMarketDataReturnComponentsPreprocessingSettings.from_mapping(
                    require_mapping(
                        raw_payload,
                        "return_components",
                        f"{field_path}.return_components",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.return_components",
                )
                if "return_components" in raw_payload and raw_payload["return_components"] is not None
                else None
            ),
        )


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
    session_definition_source: Optional[CalibrationInputFileReference] = None
    session_timezone: Optional[str] = None

    def __post_init__(self) -> None:
        if not self.exchange_code.strip():
            raise _calibration_error("exchange_code", "must be a non-empty string")
        if self.session_timezone is not None and not self.session_timezone.strip():
            raise _calibration_error("session_timezone", "must be non-empty when provided")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationSessionConfigurationInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_session_definition_source = raw_payload.get("session_definition_source")
        if raw_session_definition_source is not None and not isinstance(raw_session_definition_source, dict):
            raise _calibration_error(
                f"{field_path}.session_definition_source",
                "must be an object when provided",
            )

        raw_session_timezone = raw_payload.get("session_timezone")
        if raw_session_timezone is not None and not isinstance(raw_session_timezone, str):
            raise _calibration_error(
                f"{field_path}.session_timezone",
                "must be a string when provided",
            )

        return CalibrationSessionConfigurationInput(
            exchange_code=require_non_empty_string(
                raw_payload,
                "exchange_code",
                f"{field_path}.exchange_code",
                CALIBRATION_ERROR_FACTORY,
            ),
            session_definition_source=(
                CalibrationInputFileReference.from_mapping(
                    cast(dict[str, Any], raw_session_definition_source),
                    field_path=f"{field_path}.session_definition_source",
                )
                if raw_session_definition_source is not None
                else None
            ),
            session_timezone=cast(Optional[str], raw_session_timezone),
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
    trade_sign_column: Optional[str] = None
    bid_column: Optional[str] = None
    ask_column: Optional[str] = None

    def __post_init__(self) -> None:
        if not self.timestamp_column.strip():
            raise _calibration_error("timestamp_column", "must be a non-empty string")

        for field_name, field_value in (
            ("price_column", self.price_column),
            ("trade_sign_column", self.trade_sign_column),
            ("bid_column", self.bid_column),
            ("ask_column", self.ask_column),
        ):
            if field_value is not None and not field_value.strip():
                raise _calibration_error(field_name, "must be non-empty when provided")

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationTickDataColumnMapping:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_optional_fields = {
            "price_column": raw_payload.get("price_column"),
            "trade_sign_column": raw_payload.get("trade_sign_column"),
            "bid_column": raw_payload.get("bid_column"),
            "ask_column": raw_payload.get("ask_column"),
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
            trade_sign_column=cast(Optional[str], raw_optional_fields["trade_sign_column"]),
            bid_column=cast(Optional[str], raw_optional_fields["bid_column"]),
            ask_column=cast(Optional[str], raw_optional_fields["ask_column"]),
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
            raise _calibration_error(f"{field_path}.event_identifier_column", "must be a string when provided")

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
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationMarketDataColumnMapping:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping.from_mapping(
                require_mapping(raw_payload, "tick_data", f"{field_path}.tick_data", CALIBRATION_ERROR_FACTORY),
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
    duplicate_record_policy: Optional[CalibrationDuplicateRecordPolicy] = None
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

        raw_duplicate_record_policy = raw_payload.get("duplicate_record_policy")
        if raw_duplicate_record_policy is not None and not isinstance(raw_duplicate_record_policy, str):
            raise _calibration_error(f"{field_path}.duplicate_record_policy", "must be a string when provided")

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
            duplicate_record_policy=(
                CalibrationDuplicateRecordPolicy(raw_duplicate_record_policy)
                if raw_duplicate_record_policy is not None
                else None
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
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationMarketDataOutputPreferences:
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


def validate_tick_price_input_policy(
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
        case CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT:
            if column_mapping.bid_column is None or column_mapping.ask_column is None:
                raise _calibration_error(
                    "column_mapping.tick_data",
                    "bid_column and ask_column are required when tick_price_input_policy=bid_ask_midpoint",
                )


def validate_option_surface_scope(
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


def validate_known_news_inputs(
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
        known_news_inputs.calendar_source.file_format
        in (CalibrationInputFileFormat.CSV, CalibrationInputFileFormat.PARQUET)
        and known_news_column_mapping is None
    ):
        raise _calibration_error(
            "column_mapping.known_news",
            "is required when known_news_inputs.calendar_source uses csv or parquet",
        )


__all__ = [
    "CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION",
    "CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION",
    "CalibrationDuplicateRecordPolicy",
    "CalibrationExecutionMode",
    "CalibrationInputFileFormat",
    "CalibrationInputFileReference",
    "CalibrationKnownNewsColumnMapping",
    "CalibrationKnownNewsInput",
    "CalibrationMarketDataColumnMapping",
    "CalibrationMarketDataPreprocessingSettings",
    "CalibrationMarketDataNormalizationPolicy",
    "CalibrationMarketDataOutputPreferences",
    "CalibrationMarketDataReturnComponentsPreprocessingSettings",
    "CalibrationMarketDataSeasonalityPreprocessingSettings",
    "CalibrationMarketDataWorkflowScope",
    "CalibrationMeanReversionConfiguration",
    "CalibrationOptionQuoteInputPolicy",
    "CalibrationOptionSurfaceColumnMapping",
    "CalibrationSessionConfigurationInput",
    "CalibrationTickDataColumnMapping",
    "CalibrationTickPriceInputPolicy",
    "CalibrationTradeSign",
    "validate_known_news_inputs",
    "validate_option_surface_scope",
    "validate_tick_price_input_policy",
]
