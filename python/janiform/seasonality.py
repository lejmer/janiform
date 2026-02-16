from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from types import ModuleType
from typing import Optional, Self, cast

from .api import native_module
from .calendar import (
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
)

TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION: str = "seasonality.timestamp_event.v1"
PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION: str = "seasonality.timestamp_event.preprocessed.v1"
INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION: str = "seasonality.intraday.estimate.v1"
DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION: str = "seasonality.day_of_week.estimate.v1"
DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION: str = "seasonality.day_of_year.estimate.v1"
DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION: str = "seasonality.deterministic.composition.v1"


class SeasonalityPreprocessingError(Exception):
    """Base class for seasonality preprocessing failures."""


class SeasonalityInputValidationError(SeasonalityPreprocessingError):
    """Raised when event records violate the timestamp input contract."""


class SessionFilter(StrEnum):
    ALL = 'all'
    OPEN_ONLY = 'open_only'
    CLOSED_ONLY = 'closed_only'


class IntradaySmoothingKernel(StrEnum):
    GAUSSIAN = 'gaussian'
    UNIFORM = 'uniform'


@dataclass(frozen=True, slots=True)
class TimestampEventInputSchema:
    schema_version: str
    required_fields: tuple[str, ...]
    timestamp_field_name: str
    accepted_timestamp_types: tuple[str, ...]
    timezone_requirement: str
    ordering_policy: str


@dataclass(frozen=True, slots=True)
class IntradaySeasonalityEstimatorSettings:
    minutes_per_bin: int = 1
    smoothing_kernel: IntradaySmoothingKernel = IntradaySmoothingKernel.GAUSSIAN
    smoothing_bandwidth_bins: float = 3.0
    minimum_bin_events: int = 1
    pseudocount: float = 1e-6


@dataclass(frozen=True, slots=True)
class DayOfWeekSeasonalityEstimatorSettings:
    minimum_bucket_events: int = 1
    pseudocount: float = 1e-6


@dataclass(frozen=True, slots=True)
class DayOfYearSeasonalityEstimatorSettings:
    minimum_bucket_events: int = 1
    pseudocount: float = 1e-6


@dataclass(frozen=True, slots=True)
class ScheduledEventOverlayDefinition:
    overlay_id: str
    start_timestamp_utc: str | datetime
    end_timestamp_utc: str | datetime
    log_adjustment: float
    precedence: int = 0
    enabled: bool = True
    source_label: str = "configuration"

    @staticmethod
    def from_config_record(
        config_record: Mapping[str, object],
    ) -> Self:
        if "overlay_id" not in config_record:
            raise SeasonalityInputValidationError(
                "scheduled event overlay configuration record is missing required field 'overlay_id'"
            )
        overlay_id_value: object = config_record["overlay_id"]
        if not isinstance(overlay_id_value, str) or not overlay_id_value.strip():
            raise SeasonalityInputValidationError(
                "scheduled event overlay field 'overlay_id' must be a non-empty string"
            )

        if "start_timestamp_utc" not in config_record:
            raise SeasonalityInputValidationError(
                "scheduled event overlay configuration record is missing required field 'start_timestamp_utc'"
            )
        start_timestamp_utc_value: object = config_record["start_timestamp_utc"]
        if not isinstance(start_timestamp_utc_value, str | datetime):
            raise SeasonalityInputValidationError(
                "scheduled event overlay field 'start_timestamp_utc' must be an RFC3339 string or timezone-aware datetime"
            )

        if "end_timestamp_utc" not in config_record:
            raise SeasonalityInputValidationError(
                "scheduled event overlay configuration record is missing required field 'end_timestamp_utc'"
            )
        end_timestamp_utc_value: object = config_record["end_timestamp_utc"]
        if not isinstance(end_timestamp_utc_value, str | datetime):
            raise SeasonalityInputValidationError(
                "scheduled event overlay field 'end_timestamp_utc' must be an RFC3339 string or timezone-aware datetime"
            )

        if "log_adjustment" not in config_record:
            raise SeasonalityInputValidationError(
                "scheduled event overlay configuration record is missing required field 'log_adjustment'"
            )
        log_adjustment_value: object = config_record["log_adjustment"]
        if isinstance(log_adjustment_value, bool) or not isinstance(log_adjustment_value, int | float):
            raise SeasonalityInputValidationError(
                "scheduled event overlay field 'log_adjustment' must be a float"
            )

        precedence: int = 0
        if "precedence" in config_record:
            precedence_value: object = config_record["precedence"]
            if isinstance(precedence_value, bool) or not isinstance(precedence_value, int):
                raise SeasonalityInputValidationError(
                    "scheduled event overlay field 'precedence' must be an integer"
                )
            if precedence_value < 0:
                raise SeasonalityInputValidationError(
                    "scheduled event overlay field 'precedence' must be non-negative"
                )
            precedence = precedence_value

        enabled: bool = True
        if "enabled" in config_record:
            enabled_value: object = config_record["enabled"]
            if not isinstance(enabled_value, bool):
                raise SeasonalityInputValidationError(
                    "scheduled event overlay field 'enabled' must be a boolean"
                )
            enabled = enabled_value

        source_label: str = "configuration"
        if "source_label" in config_record:
            source_label_value: object = config_record["source_label"]
            if not isinstance(source_label_value, str):
                raise SeasonalityInputValidationError(
                    "scheduled event overlay field 'source_label' must be a string"
                )
            source_label = source_label_value

        return ScheduledEventOverlayDefinition(
            overlay_id=overlay_id_value,
            start_timestamp_utc=start_timestamp_utc_value,
            end_timestamp_utc=end_timestamp_utc_value,
            log_adjustment=float(log_adjustment_value),
            precedence=precedence,
            enabled=enabled,
            source_label=source_label,
        )


def load_scheduled_event_overlay_definitions(
    overlay_configuration_records: Sequence[Mapping[str, object]],
) -> list[ScheduledEventOverlayDefinition]:
    return [
        ScheduledEventOverlayDefinition.from_config_record(config_record)
        for config_record in overlay_configuration_records
    ]


TIMESTAMP_EVENT_INPUT_SCHEMA: TimestampEventInputSchema = TimestampEventInputSchema(
    schema_version=TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
    required_fields=("timestamp",),
    timestamp_field_name="timestamp",
    accepted_timestamp_types=("datetime", "RFC3339 / ISO-8601 string"),
    timezone_requirement="timestamp values must include timezone information",
    ordering_policy="output is sorted by normalized UTC timestamp and then input position",
)


class TimestampEventPreprocessingResult:
    """Thin Python wrapper over the Rust preprocessing output object."""

    def __init__(self, native_result: object) -> None:
        self._native_result: object = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def input_schema_version(self) -> str:
        return cast(str, self._native_result.input_schema_version)

    @property
    def exchange_code(self) -> str:
        return cast(str, self._native_result.exchange_code)

    @property
    def exchange_timezone_name(self) -> str:
        return cast(str, self._native_result.exchange_timezone_name)

    @property
    def source_record_positions(self) -> list[int]:
        return cast(list[int], self._native_result.source_record_positions)

    @property
    def timestamp_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.timestamp_utc_nanos)

    @property
    def exchange_local_minute_of_day(self) -> list[int]:
        return cast(list[int], self._native_result.exchange_local_minute_of_day)

    @property
    def exchange_local_day_of_week(self) -> bytes:
        return cast(bytes, self._native_result.exchange_local_day_of_week)

    @property
    def exchange_local_day_of_year(self) -> list[int]:
        return cast(list[int], self._native_result.exchange_local_day_of_year)

    @property
    def market_state_codes(self) -> bytes:
        return cast(bytes, self._native_result.market_state_codes)

    @property
    def missing_hour_bucket_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.missing_hour_bucket_utc_nanos)

    @property
    def sparse_hour_bucket_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.sparse_hour_bucket_utc_nanos)

    @property
    def input_record_count(self) -> int:
        return cast(int, self._native_result.input_record_count)

    @property
    def validated_record_count(self) -> int:
        return cast(int, self._native_result.validated_record_count)

    @property
    def accepted_record_count(self) -> int:
        return cast(int, self._native_result.accepted_record_count)

    @property
    def filtered_record_count(self) -> int:
        return cast(int, self._native_result.filtered_record_count)

    @property
    def first_event_timestamp_utc_nanos(self) -> Optional[int]:
        return cast(Optional[int], self._native_result.first_event_timestamp_utc_nanos)

    @property
    def last_event_timestamp_utc_nanos(self) -> Optional[int]:
        return cast(Optional[int], self._native_result.last_event_timestamp_utc_nanos)

    @property
    def hour_bucket_count(self) -> int:
        return cast(int, self._native_result.hour_bucket_count)

    @property
    def missing_hour_bucket_count(self) -> int:
        return cast(int, self._native_result.missing_hour_bucket_count)

    @property
    def sparse_hour_bucket_count(self) -> int:
        return cast(int, self._native_result.sparse_hour_bucket_count)


class IntradaySeasonalityEstimate:
    """Thin Python wrapper over the Rust intraday seasonality estimator output object."""

    def __init__(self, native_result: object) -> None:
        self._native_result: object = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def minutes_per_bin(self) -> int:
        return cast(int, self._native_result.minutes_per_bin)

    @property
    def smoothing_kernel(self) -> str:
        return cast(str, self._native_result.smoothing_kernel)

    @property
    def smoothing_bandwidth_bins(self) -> float:
        return cast(float, self._native_result.smoothing_bandwidth_bins)

    @property
    def minimum_bin_events(self) -> int:
        return cast(int, self._native_result.minimum_bin_events)

    @property
    def pseudocount(self) -> float:
        return cast(float, self._native_result.pseudocount)

    @property
    def bin_start_minute_of_day(self) -> list[int]:
        return cast(list[int], self._native_result.bin_start_minute_of_day)

    @property
    def event_count_by_bin(self) -> list[int]:
        return cast(list[int], self._native_result.event_count_by_bin)

    @property
    def sparse_bin_flags(self) -> bytes:
        return cast(bytes, self._native_result.sparse_bin_flags)

    @property
    def smoothed_density_by_bin(self) -> list[float]:
        return cast(list[float], self._native_result.smoothed_density_by_bin)

    @property
    def intraday_log_component_by_bin(self) -> list[float]:
        return cast(list[float], self._native_result.intraday_log_component_by_bin)

    @property
    def intraday_factor_by_bin(self) -> list[float]:
        return cast(list[float], self._native_result.intraday_factor_by_bin)

    @property
    def input_event_count(self) -> int:
        return cast(int, self._native_result.input_event_count)

    @property
    def bin_count(self) -> int:
        return cast(int, self._native_result.bin_count)

    @property
    def zero_event_bin_count(self) -> int:
        return cast(int, self._native_result.zero_event_bin_count)

    @property
    def sparse_bin_count(self) -> int:
        return cast(int, self._native_result.sparse_bin_count)


class DayOfWeekSeasonalityEstimate:
    """Thin Python wrapper over the Rust day-of-week seasonality estimator output object."""

    def __init__(self, native_result: object) -> None:
        self._native_result: object = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def minimum_bucket_events(self) -> int:
        return cast(int, self._native_result.minimum_bucket_events)

    @property
    def pseudocount(self) -> float:
        return cast(float, self._native_result.pseudocount)

    @property
    def day_of_week_number(self) -> bytes:
        return cast(bytes, self._native_result.day_of_week_number)

    @property
    def event_count_by_day_of_week(self) -> list[int]:
        return cast(list[int], self._native_result.event_count_by_day_of_week)

    @property
    def sparse_bucket_flags(self) -> bytes:
        return cast(bytes, self._native_result.sparse_bucket_flags)

    @property
    def fallback_bucket_flags(self) -> bytes:
        return cast(bytes, self._native_result.fallback_bucket_flags)

    @property
    def day_of_week_log_component(self) -> list[float]:
        return cast(list[float], self._native_result.day_of_week_log_component)

    @property
    def day_of_week_factor(self) -> list[float]:
        return cast(list[float], self._native_result.day_of_week_factor)

    @property
    def input_event_count(self) -> int:
        return cast(int, self._native_result.input_event_count)

    @property
    def bucket_count(self) -> int:
        return cast(int, self._native_result.bucket_count)

    @property
    def zero_event_bucket_count(self) -> int:
        return cast(int, self._native_result.zero_event_bucket_count)

    @property
    def sparse_bucket_count(self) -> int:
        return cast(int, self._native_result.sparse_bucket_count)

    @property
    def fallback_bucket_count(self) -> int:
        return cast(int, self._native_result.fallback_bucket_count)


class DayOfYearSeasonalityEstimate:
    """Thin Python wrapper over the Rust day-of-year seasonality estimator output object."""

    def __init__(self, native_result: object) -> None:
        self._native_result: object = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def minimum_bucket_events(self) -> int:
        return cast(int, self._native_result.minimum_bucket_events)

    @property
    def pseudocount(self) -> float:
        return cast(float, self._native_result.pseudocount)

    @property
    def day_of_year_number(self) -> list[int]:
        return cast(list[int], self._native_result.day_of_year_number)

    @property
    def event_count_by_day_of_year(self) -> list[int]:
        return cast(list[int], self._native_result.event_count_by_day_of_year)

    @property
    def sparse_bucket_flags(self) -> bytes:
        return cast(bytes, self._native_result.sparse_bucket_flags)

    @property
    def fallback_bucket_flags(self) -> bytes:
        return cast(bytes, self._native_result.fallback_bucket_flags)

    @property
    def day_of_year_log_component(self) -> list[float]:
        return cast(list[float], self._native_result.day_of_year_log_component)

    @property
    def day_of_year_factor(self) -> list[float]:
        return cast(list[float], self._native_result.day_of_year_factor)

    @property
    def input_event_count(self) -> int:
        return cast(int, self._native_result.input_event_count)

    @property
    def bucket_count(self) -> int:
        return cast(int, self._native_result.bucket_count)

    @property
    def zero_event_bucket_count(self) -> int:
        return cast(int, self._native_result.zero_event_bucket_count)

    @property
    def sparse_bucket_count(self) -> int:
        return cast(int, self._native_result.sparse_bucket_count)

    @property
    def fallback_bucket_count(self) -> int:
        return cast(int, self._native_result.fallback_bucket_count)


class DeterministicSeasonalityComposition:
    """Thin Python wrapper over the Rust deterministic seasonality composition output object."""

    def __init__(self, native_result: object) -> None:
        self._native_result: object = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def intraday_minutes_per_bin(self) -> int:
        return cast(int, self._native_result.intraday_minutes_per_bin)

    @property
    def intraday_bin_index_by_event(self) -> list[int]:
        return cast(list[int], self._native_result.intraday_bin_index_by_event)

    @property
    def intraday_log_component_by_event(self) -> list[float]:
        return cast(list[float], self._native_result.intraday_log_component_by_event)

    @property
    def day_of_week_log_component_by_event(self) -> list[float]:
        return cast(list[float], self._native_result.day_of_week_log_component_by_event)

    @property
    def day_of_year_log_component_by_event(self) -> list[float]:
        return cast(list[float], self._native_result.day_of_year_log_component_by_event)

    @property
    def special_log_component_by_event(self) -> list[float]:
        return cast(list[float], self._native_result.special_log_component_by_event)

    @property
    def special_overlay_active_flags_by_event(self) -> bytes:
        return cast(bytes, self._native_result.special_overlay_active_flags_by_event)

    @property
    def active_overlay_ids_by_event(self) -> list[str]:
        return cast(list[str], self._native_result.active_overlay_ids_by_event)

    @property
    def combined_log_component_by_event(self) -> list[float]:
        return cast(list[float], self._native_result.combined_log_component_by_event)

    @property
    def combined_factor_by_event(self) -> list[float]:
        return cast(list[float], self._native_result.combined_factor_by_event)

    @property
    def input_event_count(self) -> int:
        return cast(int, self._native_result.input_event_count)

    @property
    def normalization_policy(self) -> str:
        return cast(str, self._native_result.normalization_policy)

    @property
    def fit_window_event_count(self) -> int:
        return cast(int, self._native_result.fit_window_event_count)

    @property
    def target_mean_factor(self) -> float:
        return cast(float, self._native_result.target_mean_factor)

    @property
    def pre_normalization_mean_factor(self) -> float:
        return cast(float, self._native_result.pre_normalization_mean_factor)

    @property
    def normalization_constant(self) -> float:
        return cast(float, self._native_result.normalization_constant)

    @property
    def normalization_log_offset(self) -> float:
        return cast(float, self._native_result.normalization_log_offset)

    @property
    def post_normalization_mean_factor(self) -> float:
        return cast(float, self._native_result.post_normalization_mean_factor)

    @property
    def mean_anchor_tolerance(self) -> float:
        return cast(float, self._native_result.mean_anchor_tolerance)

    @property
    def overlays_enabled(self) -> bool:
        return cast(bool, self._native_result.overlays_enabled)

    @property
    def configured_overlay_count(self) -> int:
        return cast(int, self._native_result.configured_overlay_count)

    @property
    def enabled_overlay_count(self) -> int:
        return cast(int, self._native_result.enabled_overlay_count)

    @property
    def active_overlay_event_count(self) -> int:
        return cast(int, self._native_result.active_overlay_event_count)

    @property
    def overlay_precedence_policy(self) -> str:
        return cast(str, self._native_result.overlay_precedence_policy)

    @property
    def overlay_merge_policy(self) -> str:
        return cast(str, self._native_result.overlay_merge_policy)

    @property
    def overlay_window_boundary_policy(self) -> str:
        return cast(str, self._native_result.overlay_window_boundary_policy)

    @property
    def configured_overlay_ids_in_order(self) -> list[str]:
        return cast(list[str], self._native_result.configured_overlay_ids_in_order)

    @property
    def applied_overlay_ids_in_order(self) -> list[str]:
        return cast(list[str], self._native_result.applied_overlay_ids_in_order)

    @property
    def overlay_source_labels_in_order(self) -> list[str]:
        return cast(list[str], self._native_result.overlay_source_labels_in_order)


def preprocess_timestamp_events(
    event_records: Sequence[Mapping[str, object]],
    *,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
    timestamp_field_name: str = "timestamp",
    session_filter: SessionFilter = SessionFilter.OPEN_ONLY,
    sparse_hour_event_threshold: int = 1,
) -> TimestampEventPreprocessingResult:
    native: ModuleType = native_module()

    try:
        native_result: object = calendar_registry._preprocess_timestamp_events_native(
            exchange_code=exchange_code,
            event_records=event_records,
            timestamp_field_name=timestamp_field_name,
            session_filter=session_filter.value,
            sparse_hour_event_threshold=sparse_hour_event_threshold,
        )
    except native.SeasonalityInputValidationError as e:
        raise SeasonalityInputValidationError(str(e)) from e
    except native.CalendarMissingExchangeError as e:
        raise CalendarMissingExchangeError(str(e)) from e
    except native.CalendarInvalidTimezoneError as e:
        raise CalendarInvalidTimezoneError(str(e)) from e

    return TimestampEventPreprocessingResult(native_result)


def estimate_intraday_seasonality(
    preprocessed_events: TimestampEventPreprocessingResult,
    *,
    settings: Optional[IntradaySeasonalityEstimatorSettings] = None,
) -> IntradaySeasonalityEstimate:
    native: ModuleType = native_module()
    estimator_settings: IntradaySeasonalityEstimatorSettings = (
        IntradaySeasonalityEstimatorSettings() if settings is None else settings
    )

    try:
        native_result: object = native.estimate_intraday_seasonality(
            preprocessed_events.exchange_local_minute_of_day,
            minutes_per_bin=estimator_settings.minutes_per_bin,
            smoothing_kernel=estimator_settings.smoothing_kernel.value,
            smoothing_bandwidth_bins=estimator_settings.smoothing_bandwidth_bins,
            minimum_bin_events=estimator_settings.minimum_bin_events,
            pseudocount=estimator_settings.pseudocount,
        )
    except native.SeasonalityInputValidationError as e:
        raise SeasonalityInputValidationError(str(e)) from e

    return IntradaySeasonalityEstimate(native_result)


def estimate_day_of_week_seasonality(
    preprocessed_events: TimestampEventPreprocessingResult,
    *,
    settings: Optional[DayOfWeekSeasonalityEstimatorSettings] = None,
) -> DayOfWeekSeasonalityEstimate:
    native: ModuleType = native_module()
    estimator_settings: DayOfWeekSeasonalityEstimatorSettings = (
        DayOfWeekSeasonalityEstimatorSettings() if settings is None else settings
    )

    try:
        native_result: object = native.estimate_day_of_week_seasonality(
            preprocessed_events.exchange_local_day_of_week,
            minimum_bucket_events=estimator_settings.minimum_bucket_events,
            pseudocount=estimator_settings.pseudocount,
        )
    except native.SeasonalityInputValidationError as e:
        raise SeasonalityInputValidationError(str(e)) from e

    return DayOfWeekSeasonalityEstimate(native_result)


def estimate_day_of_year_seasonality(
    preprocessed_events: TimestampEventPreprocessingResult,
    *,
    settings: Optional[DayOfYearSeasonalityEstimatorSettings] = None,
) -> DayOfYearSeasonalityEstimate:
    native: ModuleType = native_module()
    estimator_settings: DayOfYearSeasonalityEstimatorSettings = (
        DayOfYearSeasonalityEstimatorSettings() if settings is None else settings
    )

    try:
        native_result: object = native.estimate_day_of_year_seasonality(
            preprocessed_events.exchange_local_day_of_year,
            minimum_bucket_events=estimator_settings.minimum_bucket_events,
            pseudocount=estimator_settings.pseudocount,
        )
    except native.SeasonalityInputValidationError as e:
        raise SeasonalityInputValidationError(str(e)) from e

    return DayOfYearSeasonalityEstimate(native_result)


def compose_deterministic_seasonality_signal(
    preprocessed_events: TimestampEventPreprocessingResult,
    intraday_estimate: IntradaySeasonalityEstimate,
    day_of_week_estimate: DayOfWeekSeasonalityEstimate,
    day_of_year_estimate: DayOfYearSeasonalityEstimate,
    *,
    scheduled_event_overlays: Optional[Sequence[ScheduledEventOverlayDefinition]] = None,
) -> DeterministicSeasonalityComposition:
    native: ModuleType = native_module()
    serialized_scheduled_event_overlays: Optional[list[dict[str, object]]] = (
        _serialize_scheduled_event_overlays(scheduled_event_overlays)
        if scheduled_event_overlays is not None
        else None
    )

    try:
        native_result: object = native.compose_deterministic_seasonality_signal(
            preprocessed_events.timestamp_utc_nanos,
            preprocessed_events.exchange_local_minute_of_day,
            preprocessed_events.exchange_local_day_of_week,
            preprocessed_events.exchange_local_day_of_year,
            intraday_estimate.intraday_log_component_by_bin,
            day_of_week_estimate.day_of_week_log_component,
            day_of_year_estimate.day_of_year_log_component,
            intraday_minutes_per_bin=intraday_estimate.minutes_per_bin,
            scheduled_event_overlays=serialized_scheduled_event_overlays,
        )
    except native.SeasonalityInputValidationError as e:
        raise SeasonalityInputValidationError(str(e)) from e

    return DeterministicSeasonalityComposition(native_result)


def _serialize_scheduled_event_overlays(
    scheduled_event_overlays: Sequence[ScheduledEventOverlayDefinition],
) -> list[dict[str, object]]:
    serialized_overlays: list[dict[str, object]] = []
    for scheduled_event_overlay in scheduled_event_overlays:
        if scheduled_event_overlay.precedence > 65_535:
            raise SeasonalityInputValidationError(
                "scheduled event overlay field 'precedence' must be in the range 0..65535"
            )
        serialized_overlays.append(
            {
                "overlay_id": scheduled_event_overlay.overlay_id,
                "start_timestamp_utc": scheduled_event_overlay.start_timestamp_utc,
                "end_timestamp_utc": scheduled_event_overlay.end_timestamp_utc,
                "log_adjustment": scheduled_event_overlay.log_adjustment,
                "precedence": scheduled_event_overlay.precedence,
                "enabled": scheduled_event_overlay.enabled,
                "source_label": scheduled_event_overlay.source_label,
            }
        )

    return serialized_overlays


__all__ = [
    "DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION",
    "DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION",
    "DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION",
    "DayOfWeekSeasonalityEstimate",
    "DayOfWeekSeasonalityEstimatorSettings",
    "DayOfYearSeasonalityEstimate",
    "DayOfYearSeasonalityEstimatorSettings",
    "DeterministicSeasonalityComposition",
    "ScheduledEventOverlayDefinition",
    "INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION",
    "IntradaySeasonalityEstimate",
    "IntradaySeasonalityEstimatorSettings",
    "IntradaySmoothingKernel",
    "load_scheduled_event_overlay_definitions",
    "PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION",
    "SessionFilter",
    "SeasonalityInputValidationError",
    "SeasonalityPreprocessingError",
    "TIMESTAMP_EVENT_INPUT_SCHEMA",
    "TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION",
    "TimestampEventInputSchema",
    "TimestampEventPreprocessingResult",
    "compose_deterministic_seasonality_signal",
    "estimate_day_of_week_seasonality",
    "estimate_day_of_year_seasonality",
    "estimate_intraday_seasonality",
    "preprocess_timestamp_events",
]
