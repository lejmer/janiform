"""Public seasonality contracts."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from typing import Any, cast

from ..contracts import ContractModel
from ..contracts.base import require_non_empty_string
from ..contracts.base import (
    require_boolean,
    require_datetime_utc,
    require_finite_number,
    require_mapping_value,
    require_optional_string_value,
)
from .errors import SeasonalityInputValidationError

TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION: str = "seasonality.timestamp_event.v1"
PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION: str = "seasonality.timestamp_event.preprocessed.v1"
INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION: str = "seasonality.intraday.estimate.v1"
DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION: str = "seasonality.day_of_week.estimate.v1"
DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION: str = "seasonality.day_of_year.estimate.v1"
DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION: str = (
    "seasonality.deterministic.composition.v1"
)
SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION: str = "seasonality.diagnostics.report.v1"
SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION: str = "seasonality.diagnostics.batch.v1"
# Public workflow default for the estimator identifier, overrideable by callers.
DEFAULT_SEASONALITY_ESTIMATOR_VERSION: str = "seasonality_estimator.v1"


def _seasonality_error(field_path: str, message: str) -> SeasonalityInputValidationError:
    return SeasonalityInputValidationError(f"{field_path} {message}")


class SessionFilter(StrEnum):
    ALL = 'all'
    OPEN_ONLY = 'open_only'
    CLOSED_ONLY = 'closed_only'


class IntradaySmoothingKernel(StrEnum):
    GAUSSIAN = 'gaussian'
    UNIFORM = 'uniform'


@dataclass(frozen=True, slots=True)
class TimestampEventInputSchema(ContractModel):
    schema_version: str
    required_fields: tuple[str, ...]
    timestamp_field_name: str
    accepted_timestamp_types: tuple[str, ...]
    timezone_requirement: str
    ordering_policy: str


@dataclass(frozen=True, slots=True)
class IntradaySeasonalityEstimatorSettings(ContractModel):
    minutes_per_bin: int = 1
    smoothing_kernel: IntradaySmoothingKernel = IntradaySmoothingKernel.GAUSSIAN
    smoothing_bandwidth_bins: float = 3.0
    minimum_bin_events: int = 1
    pseudocount: float = 1e-6


@dataclass(frozen=True, slots=True)
class DayOfWeekSeasonalityEstimatorSettings(ContractModel):
    minimum_bucket_events: int = 1
    pseudocount: float = 1e-6


@dataclass(frozen=True, slots=True)
class DayOfYearSeasonalityEstimatorSettings(ContractModel):
    minimum_bucket_events: int = 1
    pseudocount: float = 1e-6


@dataclass(frozen=True, slots=True)
class ScheduledEventOverlayDefinition(ContractModel):
    overlay_id: str
    start_timestamp_utc: str | datetime
    end_timestamp_utc: str | datetime
    log_adjustment: float
    precedence: int = 0
    enabled: bool = True
    source_label: str = "configuration"

    @staticmethod
    def from_config_record(
        config_record: Mapping[str, Any] | Any,
    ) -> ScheduledEventOverlayDefinition:
        mapping = require_mapping_value(
            config_record,
            "scheduled_event_overlay",
            _seasonality_error,
        )
        overlay_id_value = require_non_empty_string(
            mapping,
            "overlay_id",
            "scheduled_event_overlay.overlay_id",
            _seasonality_error,
        )
        start_timestamp_utc_value = mapping.get("start_timestamp_utc")
        if start_timestamp_utc_value is None:
            raise SeasonalityInputValidationError(
                "scheduled event overlay configuration record is missing required field 'start_timestamp_utc'"
            )
        require_datetime_utc(
            start_timestamp_utc_value,
            "scheduled_event_overlay.start_timestamp_utc",
            _seasonality_error,
        )

        end_timestamp_utc_value = mapping.get("end_timestamp_utc")
        if end_timestamp_utc_value is None:
            raise SeasonalityInputValidationError(
                "scheduled event overlay configuration record is missing required field 'end_timestamp_utc'"
            )
        require_datetime_utc(
            end_timestamp_utc_value,
            "scheduled_event_overlay.end_timestamp_utc",
            _seasonality_error,
        )

        log_adjustment_value = require_finite_number(
            mapping,
            "log_adjustment",
            "scheduled_event_overlay.log_adjustment",
            _seasonality_error,
        )

        precedence = 0
        raw_precedence_value = mapping.get("precedence")
        if raw_precedence_value is not None:
            if not isinstance(raw_precedence_value, int) or isinstance(raw_precedence_value, bool):
                raise _seasonality_error(
                    "scheduled_event_overlay.precedence",
                    "must be an integer",
                )
            if raw_precedence_value < 0:
                raise _seasonality_error(
                    "scheduled_event_overlay.precedence",
                    "must be non-negative",
                )
            precedence = raw_precedence_value

        enabled = True
        if "enabled" in mapping:
            enabled = require_boolean(
                mapping,
                "enabled",
                "scheduled_event_overlay.enabled",
                _seasonality_error,
            )

        source_label = "configuration"
        raw_source_label_value = require_optional_string_value(
            mapping.get("source_label"),
            "scheduled_event_overlay.source_label",
            _seasonality_error,
        )
        if raw_source_label_value is not None:
            source_label = raw_source_label_value

        return ScheduledEventOverlayDefinition(
            overlay_id=overlay_id_value,
            start_timestamp_utc=cast(str | datetime, start_timestamp_utc_value),
            end_timestamp_utc=cast(str | datetime, end_timestamp_utc_value),
            log_adjustment=log_adjustment_value,
            precedence=precedence,
            enabled=enabled,
            source_label=source_label,
        )


def load_scheduled_event_overlay_definitions(
    overlay_configuration_records: Sequence[Mapping[str, Any]],
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


__all__ = [
    "DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION",
    "DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION",
    "DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION",
    "DEFAULT_SEASONALITY_ESTIMATOR_VERSION",
    "DayOfWeekSeasonalityEstimatorSettings",
    "DayOfYearSeasonalityEstimatorSettings",
    "SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION",
    "SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION",
    "ScheduledEventOverlayDefinition",
    "INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION",
    "IntradaySeasonalityEstimatorSettings",
    "IntradaySmoothingKernel",
    "load_scheduled_event_overlay_definitions",
    "PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION",
    "SessionFilter",
    "TIMESTAMP_EVENT_INPUT_SCHEMA",
    "TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION",
    "TimestampEventInputSchema",
]
