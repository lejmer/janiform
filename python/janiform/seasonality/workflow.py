"""Internal workflow entrypoints for seasonality preprocessing and estimation."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from types import ModuleType
from typing import Any, Optional, cast

import orjson

from .._native_api import native_module, remap_native_exception_by_type
from ..calendar import (
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
)
from .artifacts import (
    DayOfWeekSeasonalityEstimate,
    DayOfYearSeasonalityEstimate,
    DeterministicSeasonalityComposition,
    IntradaySeasonalityEstimate,
    TimestampEventPreprocessingResult,
)
from .contracts import (
    DEFAULT_SEASONALITY_ESTIMATOR_VERSION,
    SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION,
    DayOfWeekSeasonalityEstimatorSettings,
    DayOfYearSeasonalityEstimatorSettings,
    IntradaySeasonalityEstimatorSettings,
    ScheduledEventOverlayDefinition,
    SessionFilter,
)
from .errors import SeasonalityInputValidationError

_SEASONALITY_NATIVE_EXCEPTION_TYPES: dict[str, type[Exception]] = {
    "SeasonalityInputValidationError": SeasonalityInputValidationError,
}
_SEASONALITY_PREPROCESS_NATIVE_EXCEPTION_TYPES: dict[str, type[Exception]] = {
    **_SEASONALITY_NATIVE_EXCEPTION_TYPES,
    "CalendarMissingExchangeError": CalendarMissingExchangeError,
    "CalendarInvalidTimezoneError": CalendarInvalidTimezoneError,
}


@dataclass(frozen=True, slots=True)
class SeasonalityDiagnosticsBatchInput:
    dataset_id: str
    preprocessed_events: TimestampEventPreprocessingResult
    intraday_estimate: IntradaySeasonalityEstimate
    day_of_week_estimate: DayOfWeekSeasonalityEstimate
    day_of_year_estimate: DayOfYearSeasonalityEstimate
    composition: DeterministicSeasonalityComposition


def preprocess_timestamp_events(
    event_records: Sequence[Mapping[str, Any]],
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
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _SEASONALITY_PREPROCESS_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

        raise

    return TimestampEventPreprocessingResult(native_result)


def estimate_intraday_seasonality(
    preprocessed_events: TimestampEventPreprocessingResult,
    *,
    settings: Optional[IntradaySeasonalityEstimatorSettings] = None,
) -> IntradaySeasonalityEstimate:
    native: ModuleType = native_module()
    estimator_settings = IntradaySeasonalityEstimatorSettings() if settings is None else settings

    try:
        native_result: object = native.estimate_intraday_seasonality(
            preprocessed_events.exchange_local_minute_of_day,
            minutes_per_bin=estimator_settings.minutes_per_bin,
            smoothing_kernel=estimator_settings.smoothing_kernel.value,
            smoothing_bandwidth_bins=estimator_settings.smoothing_bandwidth_bins,
            minimum_bin_events=estimator_settings.minimum_bin_events,
            pseudocount=estimator_settings.pseudocount,
        )
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _SEASONALITY_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

        raise

    return IntradaySeasonalityEstimate(native_result)


def estimate_day_of_week_seasonality(
    preprocessed_events: TimestampEventPreprocessingResult,
    *,
    settings: Optional[DayOfWeekSeasonalityEstimatorSettings] = None,
) -> DayOfWeekSeasonalityEstimate:
    native: ModuleType = native_module()
    estimator_settings = DayOfWeekSeasonalityEstimatorSettings() if settings is None else settings

    try:
        native_result: object = native.estimate_day_of_week_seasonality(
            preprocessed_events.exchange_local_day_of_week,
            minimum_bucket_events=estimator_settings.minimum_bucket_events,
            pseudocount=estimator_settings.pseudocount,
        )
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _SEASONALITY_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

        raise

    return DayOfWeekSeasonalityEstimate(native_result)


def estimate_day_of_year_seasonality(
    preprocessed_events: TimestampEventPreprocessingResult,
    *,
    settings: Optional[DayOfYearSeasonalityEstimatorSettings] = None,
) -> DayOfYearSeasonalityEstimate:
    native: ModuleType = native_module()
    estimator_settings = DayOfYearSeasonalityEstimatorSettings() if settings is None else settings

    try:
        native_result: object = native.estimate_day_of_year_seasonality(
            preprocessed_events.exchange_local_day_of_year,
            minimum_bucket_events=estimator_settings.minimum_bucket_events,
            pseudocount=estimator_settings.pseudocount,
        )
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _SEASONALITY_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

        raise

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
    serialized_scheduled_event_overlays = (
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
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _SEASONALITY_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

        raise

    return DeterministicSeasonalityComposition(native_result)


def generate_seasonality_diagnostics_report(
    dataset_id: str,
    preprocessed_events: TimestampEventPreprocessingResult,
    intraday_estimate: IntradaySeasonalityEstimate,
    day_of_week_estimate: DayOfWeekSeasonalityEstimate,
    day_of_year_estimate: DayOfYearSeasonalityEstimate,
    composition: DeterministicSeasonalityComposition,
    *,
    estimator_version: str = DEFAULT_SEASONALITY_ESTIMATOR_VERSION,
) -> dict[str, Any]:
    native: ModuleType = native_module()

    try:
        report_json: object = native.generate_seasonality_diagnostics_report(
            dataset_id,
            preprocessed_events._native_result,
            intraday_estimate._native_result,
            day_of_week_estimate._native_result,
            day_of_year_estimate._native_result,
            composition._native_result,
            estimator_version=estimator_version,
        )
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _SEASONALITY_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

        raise

    return cast(dict[str, Any], orjson.loads(cast(str, report_json)))


def generate_seasonality_diagnostics_reports_batch(
    batch_inputs: Sequence[SeasonalityDiagnosticsBatchInput],
    *,
    estimator_version: str = DEFAULT_SEASONALITY_ESTIMATOR_VERSION,
) -> dict[str, Any]:
    if len(batch_inputs) == 0:
        raise SeasonalityInputValidationError(
            "seasonality diagnostics batch report requires at least one dataset input"
        )

    report_documents: list[dict[str, Any]] = []
    dataset_ids_in_order: list[str] = []
    observed_dataset_ids: set[str] = set()
    for batch_input in batch_inputs:
        if batch_input.dataset_id in observed_dataset_ids:
            raise SeasonalityInputValidationError(
                f"seasonality diagnostics batch report received duplicate dataset id '{batch_input.dataset_id}'"
            )

        observed_dataset_ids.add(batch_input.dataset_id)
        report_documents.append(
            generate_seasonality_diagnostics_report(
                batch_input.dataset_id,
                batch_input.preprocessed_events,
                batch_input.intraday_estimate,
                batch_input.day_of_week_estimate,
                batch_input.day_of_year_estimate,
                batch_input.composition,
                estimator_version=estimator_version,
            )
        )
        dataset_ids_in_order.append(batch_input.dataset_id)

    return {
        "schema_version": SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION,
        "report_count": len(report_documents),
        "dataset_ids_in_order": dataset_ids_in_order,
        "reports": report_documents,
    }


def _serialize_scheduled_event_overlays(
    scheduled_event_overlays: Sequence[ScheduledEventOverlayDefinition],
) -> list[dict[str, Any]]:
    serialized_overlays: list[dict[str, Any]] = []
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
