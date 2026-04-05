"""Internal shared helpers for CLI workflow command implementations."""

import json
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from typing import Any, Optional, cast

from .._constants import (
    CLI_CALIBRATION_STAGED_ENTRYPOINT,
    CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
    CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
    CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
    CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
)
from .._errors import cli_validation_error
from ..._timestamps import datetime_to_utc_nanos
from ...calibration.staged import (
    CalibrationExecutionMode,
    CalibrationStagedExecutionRuntimeResources,
    StagedCalibrationExecutionInput,
    validate_staged_calibration_execution_input,
)
from ...calendar import CalendarRegistry
from ...hourly_factors import (
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    DEFAULT_SEASONALITY_AGGREGATION_METHOD,
    DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
    DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
    HourlyFactorContract,
    HourlyNewsIntensityMaterialization,
    HourlyNewsIntensityMaterializationSettings,
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityMaterialization,
    HourlySeasonalityMaterializationSettings,
    HourlySeasonalityProviderExplicitSource,
    provide_hourly_factor_contract,
)
from ...seasonality import (
    DayOfWeekSeasonalityEstimatorSettings,
    DayOfYearSeasonalityEstimatorSettings,
    IntradaySeasonalityEstimatorSettings,
    IntradaySmoothingKernel,
)
from ...simulation import (
    SeededTickSimulationHourlyFactorSummary,
    SeededTickSimulationRequest,
)
from ...simulation.seeded.contracts import build_endogenous_seeded_mean_reversion_runtime_inputs


def build_intraday_seasonality_settings(
    *,
    intraday_minutes_per_bin: int,
    intraday_smoothing_kernel: str,
    intraday_smoothing_bandwidth_bins: float,
    intraday_minimum_bin_events: int,
    intraday_pseudocount: float,
) -> IntradaySeasonalityEstimatorSettings:
    return IntradaySeasonalityEstimatorSettings(
        minutes_per_bin=require_positive_integer_cli_argument(
            intraday_minutes_per_bin,
            field_path="intraday_minutes_per_bin",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
        smoothing_kernel=IntradaySmoothingKernel(intraday_smoothing_kernel),
        smoothing_bandwidth_bins=require_positive_float_cli_argument(
            intraday_smoothing_bandwidth_bins,
            field_path="intraday_smoothing_bandwidth_bins",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
        minimum_bin_events=require_positive_integer_cli_argument(
            intraday_minimum_bin_events,
            field_path="intraday_minimum_bin_events",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
        pseudocount=require_positive_float_cli_argument(
            intraday_pseudocount,
            field_path="intraday_pseudocount",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
    )


def build_day_of_week_seasonality_settings(
    *,
    day_of_week_minimum_bucket_events: int,
    day_of_week_pseudocount: float,
) -> DayOfWeekSeasonalityEstimatorSettings:
    return DayOfWeekSeasonalityEstimatorSettings(
        minimum_bucket_events=require_positive_integer_cli_argument(
            day_of_week_minimum_bucket_events,
            field_path="day_of_week_minimum_bucket_events",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
        pseudocount=require_positive_float_cli_argument(
            day_of_week_pseudocount,
            field_path="day_of_week_pseudocount",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
    )


def build_day_of_year_seasonality_settings(
    *,
    day_of_year_minimum_bucket_events: int,
    day_of_year_pseudocount: float,
) -> DayOfYearSeasonalityEstimatorSettings:
    return DayOfYearSeasonalityEstimatorSettings(
        minimum_bucket_events=require_positive_integer_cli_argument(
            day_of_year_minimum_bucket_events,
            field_path="day_of_year_minimum_bucket_events",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
        pseudocount=require_positive_float_cli_argument(
            day_of_year_pseudocount,
            field_path="day_of_year_pseudocount",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
    )


def require_non_empty_cli_string(
    value: str,
    *,
    field_path: str,
    error_id: str,
    entrypoint: str,
) -> str:
    if value.strip():
        return value

    raise cli_validation_error(
        field_path,
        "must be a non-empty string",
        error_id=error_id,
        entrypoint=entrypoint,
    )


def require_positive_integer_cli_argument(
    value: int,
    *,
    field_path: str,
    error_id: str,
    entrypoint: str,
) -> int:
    if value > 0:
        return value

    raise cli_validation_error(
        field_path,
        "must be strictly positive",
        error_id=error_id,
        entrypoint=entrypoint,
    )


def require_positive_float_cli_argument(
    value: float,
    *,
    field_path: str,
    error_id: str,
    entrypoint: str,
) -> float:
    if value > 0.0:
        return value

    raise cli_validation_error(
        field_path,
        "must be strictly positive",
        error_id=error_id,
        entrypoint=entrypoint,
    )


def parse_utc_datetime_cli_argument(
    value: str,
    *,
    field_path: str,
    error_id: str,
    entrypoint: str,
) -> datetime:
    try:
        parsed_value = datetime.fromisoformat(value)
    except ValueError as e:
        raise cli_validation_error(
            field_path,
            "must be a valid ISO 8601 timestamp",
            error_id=error_id,
            entrypoint=entrypoint,
        ) from e

    if parsed_value.tzinfo is None:
        raise cli_validation_error(
            field_path,
            "must be timezone-aware",
            error_id=error_id,
            entrypoint=entrypoint,
        )

    return parsed_value.astimezone(UTC)


def parse_date_cli_argument(
    value: str,
    *,
    field_path: str,
    error_id: str,
    entrypoint: str,
) -> date:
    try:
        return date.fromisoformat(value)
    except ValueError as e:
        raise cli_validation_error(
            field_path,
            "must be a valid ISO 8601 date",
            error_id=error_id,
            entrypoint=entrypoint,
        ) from e


def extract_hourly_seasonality_materialization_input(
    materialization_input_document: dict[str, Any],
) -> tuple[list[datetime], list[float], HourlySeasonalityMaterializationSettings]:
    source_timestamps_utc = extract_datetime_sequence_field(
        materialization_input_document,
        field_name="source_timestamps_utc",
        field_prefix="hourly seasonality materialization input",
    )
    source_seasonality_factors = extract_float_sequence_field(
        materialization_input_document,
        field_name="source_seasonality_factors",
        field_prefix="hourly seasonality materialization input",
    )
    sparse_hour_minimum_observation_count = extract_int_field(
        materialization_input_document,
        field_name="sparse_hour_minimum_observation_count",
        field_prefix="hourly seasonality materialization input",
        default=1,
    )

    return (
        source_timestamps_utc,
        source_seasonality_factors,
        HourlySeasonalityMaterializationSettings(
            interpolation_method=extract_string_field(
                materialization_input_document,
                field_name="interpolation_method",
                field_prefix="hourly seasonality materialization input",
                default=DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
            ),
            aggregation_method=extract_string_field(
                materialization_input_document,
                field_name="aggregation_method",
                field_prefix="hourly seasonality materialization input",
                default=DEFAULT_SEASONALITY_AGGREGATION_METHOD,
            ),
            sparse_fallback_policy=extract_string_field(
                materialization_input_document,
                field_name="sparse_fallback_policy",
                field_prefix="hourly seasonality materialization input",
                default=DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
            ),
            sparse_hour_minimum_observation_count=sparse_hour_minimum_observation_count,
        ),
    )


def extract_hourly_news_intensity_materialization_input(
    materialization_input_document: dict[str, Any],
) -> HourlyNewsIntensityMaterializationSettings:
    raw_interval_multipliers = materialization_input_document.get(
        "surprise_deterministic_time_multiplier_by_interval_index"
    )
    if raw_interval_multipliers is None:
        surprise_deterministic_time_multiplier_by_interval_index: Optional[dict[int, float]] = None
    else:
        if not isinstance(raw_interval_multipliers, dict):
            raise ValueError(
                "hourly news-intensity materialization input field "
                "`surprise_deterministic_time_multiplier_by_interval_index` must be an object"
            )

        surprise_deterministic_time_multiplier_by_interval_index = {}
        for raw_interval_index, raw_multiplier in raw_interval_multipliers.items():
            try:
                interval_index = int(raw_interval_index)
            except (TypeError, ValueError) as e:
                raise ValueError(
                    "hourly news-intensity materialization input override keys must be integer strings"
                ) from e

            surprise_deterministic_time_multiplier_by_interval_index[interval_index] = coerce_float(
                raw_multiplier,
                field_name=(
                    "hourly news-intensity materialization input field "
                    f"`surprise_deterministic_time_multiplier_by_interval_index[{raw_interval_index}]`"
                ),
            )

    return HourlyNewsIntensityMaterializationSettings(
        known_scheduled_event_timestamps_utc=extract_datetime_sequence_field(
            materialization_input_document,
            field_name="known_scheduled_event_timestamps_utc",
            field_prefix="hourly news-intensity materialization input",
        ),
        surprise_baseline_intensity=extract_required_float_field(
            materialization_input_document,
            field_name="surprise_baseline_intensity",
            field_prefix="hourly news-intensity materialization input",
        ),
        surprise_deterministic_time_multiplier_by_interval_index=(
            surprise_deterministic_time_multiplier_by_interval_index
        ),
    )


def extract_explicit_hourly_factor_source_document(
    source_document: dict[str, Any],
    *,
    field_prefix: str,
) -> tuple[list[float], str]:
    return (
        extract_float_sequence_field(
            source_document,
            field_name="hourly_factors",
            field_prefix=field_prefix,
        ),
        extract_string_field(
            source_document,
            field_name="materialization_method",
            field_prefix=field_prefix,
            default=DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        ),
    )


def extract_materialized_hourly_seasonality_source_document(
    source_document: dict[str, Any],
    *,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> tuple[list[float], str]:
    validate_hourly_materialization_source_document(
        source_document,
        expected_schema_version=HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        field_prefix="hourly seasonality materialization source",
    )

    return (
        extract_float_sequence_field(
            source_document,
            field_name="s_h",
            field_prefix="hourly seasonality materialization source",
        ),
        HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
    )


def extract_materialized_hourly_news_intensity_source_document(
    source_document: dict[str, Any],
    *,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> tuple[list[float], str]:
    validate_hourly_materialization_source_document(
        source_document,
        expected_schema_version=HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        field_prefix="hourly news-intensity materialization source",
    )

    return (
        extract_float_sequence_field(
            source_document,
            field_name="nu_h",
            field_prefix="hourly news-intensity materialization source",
        ),
        HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    )


def validate_hourly_materialization_source_document(
    source_document: dict[str, Any],
    *,
    expected_schema_version: str,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    field_prefix: str,
) -> None:
    schema_version = extract_string_field(
        source_document,
        field_name="schema_version",
        field_prefix=field_prefix,
    )
    if schema_version != expected_schema_version:
        raise ValueError(f"{field_prefix} must have schema_version {expected_schema_version!r}")

    document_exchange_code = extract_string_field(
        source_document,
        field_name="exchange_code",
        field_prefix=field_prefix,
    )
    if document_exchange_code != exchange_code:
        raise ValueError(
            f"{field_prefix} exchange_code {document_exchange_code!r} does not match {exchange_code!r}"
        )

    range_start_nanos = extract_int_field(
        source_document,
        field_name="request_start_utc_nanos",
        field_prefix=field_prefix,
    )
    if range_start_nanos != datetime_to_utc_nanos(range_start_utc):
        raise ValueError(f"{field_prefix} request_start_utc_nanos does not match the requested range_start_utc")

    range_end_nanos = extract_int_field(
        source_document,
        field_name="request_end_utc_nanos",
        field_prefix=field_prefix,
    )
    if range_end_nanos != datetime_to_utc_nanos(range_end_utc):
        raise ValueError(f"{field_prefix} request_end_utc_nanos does not match the requested range_end_utc")


def extract_datetime_sequence_field(
    document: dict[str, Any],
    *,
    field_name: str,
    field_prefix: str,
) -> list[datetime]:
    raw_values = document.get(field_name)
    if not isinstance(raw_values, list):
        raise ValueError(f"{field_prefix} must include list field `{field_name}`")

    parsed_values: list[datetime] = []
    for raw_value in raw_values:
        if not isinstance(raw_value, str):
            raise ValueError(f"{field_prefix} field `{field_name}` must contain only timestamp strings")

        try:
            parsed_value = datetime.fromisoformat(raw_value)
        except ValueError as e:
            raise ValueError(f"{field_prefix} field `{field_name}` contains invalid timestamp {raw_value!r}") from e

        if parsed_value.tzinfo is None:
            raise ValueError(f"{field_prefix} field `{field_name}` contains naive timestamp {raw_value!r}")

        parsed_values.append(parsed_value.astimezone(UTC))

    return parsed_values


def extract_float_sequence_field(
    document: dict[str, Any],
    *,
    field_name: str,
    field_prefix: str,
) -> list[float]:
    raw_values = document.get(field_name)
    if not isinstance(raw_values, list):
        raise ValueError(f"{field_prefix} must include list field `{field_name}`")

    return [
        coerce_float(
            raw_value,
            field_name=f"{field_prefix} field `{field_name}`",
        )
        for raw_value in raw_values
    ]


def extract_string_field(
    document: dict[str, Any],
    *,
    field_name: str,
    field_prefix: str,
    default: Optional[str] = None,
) -> str:
    raw_value = document.get(field_name, default)
    if not isinstance(raw_value, str) or not raw_value.strip():
        raise ValueError(f"{field_prefix} field `{field_name}` must be a non-empty string")

    return raw_value


def extract_int_field(
    document: dict[str, Any],
    *,
    field_name: str,
    field_prefix: str,
    default: Optional[int] = None,
) -> int:
    raw_value = document.get(field_name, default)
    if isinstance(raw_value, bool) or not isinstance(raw_value, int):
        raise ValueError(f"{field_prefix} field `{field_name}` must be an integer")

    return raw_value


def extract_required_float_field(
    document: dict[str, Any],
    *,
    field_name: str,
    field_prefix: str,
) -> float:
    if field_name not in document:
        raise ValueError(f"{field_prefix} must include field `{field_name}`")

    return coerce_float(document[field_name], field_name=f"{field_prefix} field `{field_name}`")


def coerce_float(raw_value: object, *, field_name: str) -> float:
    if isinstance(raw_value, bool) or not isinstance(raw_value, int | float):
        raise ValueError(f"{field_name} must contain numeric values")

    return float(raw_value)


def build_timestamp_event_preprocessing_payload(preprocessed_events: object) -> dict[str, Any]:
    return {
        "schema_version": cast(str, preprocessed_events.schema_version),
        "input_schema_version": cast(str, preprocessed_events.input_schema_version),
        "exchange_code": cast(str, preprocessed_events.exchange_code),
        "exchange_timezone_name": cast(str, preprocessed_events.exchange_timezone_name),
        "source_record_positions": list(cast(list[int], preprocessed_events.source_record_positions)),
        "timestamp_utc_nanos": list(cast(list[int], preprocessed_events.timestamp_utc_nanos)),
        "exchange_local_minute_of_day": list(cast(list[int], preprocessed_events.exchange_local_minute_of_day)),
        "exchange_local_day_of_week": list(cast(bytes, preprocessed_events.exchange_local_day_of_week)),
        "exchange_local_day_of_year": list(cast(list[int], preprocessed_events.exchange_local_day_of_year)),
        "market_state_codes": list(cast(bytes, preprocessed_events.market_state_codes)),
        "missing_hour_bucket_utc_nanos": list(cast(list[int], preprocessed_events.missing_hour_bucket_utc_nanos)),
        "sparse_hour_bucket_utc_nanos": list(cast(list[int], preprocessed_events.sparse_hour_bucket_utc_nanos)),
        "input_record_count": cast(int, preprocessed_events.input_record_count),
        "validated_record_count": cast(int, preprocessed_events.validated_record_count),
        "accepted_record_count": cast(int, preprocessed_events.accepted_record_count),
        "filtered_record_count": cast(int, preprocessed_events.filtered_record_count),
        "first_event_timestamp_utc_nanos": cast(Optional[int], preprocessed_events.first_event_timestamp_utc_nanos),
        "last_event_timestamp_utc_nanos": cast(Optional[int], preprocessed_events.last_event_timestamp_utc_nanos),
        "hour_bucket_count": cast(int, preprocessed_events.hour_bucket_count),
        "missing_hour_bucket_count": cast(int, preprocessed_events.missing_hour_bucket_count),
        "sparse_hour_bucket_count": cast(int, preprocessed_events.sparse_hour_bucket_count),
    }


def build_hourly_factor_contract_payload(contract: HourlyFactorContract) -> dict[str, Any]:
    return contract.to_payload()


def build_hourly_seasonality_materialization_payload(
    materialization: HourlySeasonalityMaterialization,
) -> dict[str, Any]:
    return {
        "schema_version": materialization.schema_version,
        "exchange_code": materialization.exchange_code,
        "exchange_timezone_name": materialization.exchange_timezone_name,
        "request_start_utc_nanos": materialization.request_start_utc_nanos,
        "request_end_utc_nanos": materialization.request_end_utc_nanos,
        "interval_duration_nanos": materialization.interval_duration_nanos,
        "interval_indices": list(materialization.interval_indices),
        "interval_start_utc_nanos": list(materialization.interval_start_utc_nanos),
        "interval_end_utc_nanos": list(materialization.interval_end_utc_nanos),
        "interval_start_exchange_local": list(materialization.interval_start_exchange_local),
        "interval_end_exchange_local": list(materialization.interval_end_exchange_local),
        "s_h": list(materialization.s_h),
        "source_point_count": materialization.source_point_count,
        "interval_count": materialization.interval_count,
        "sparse_interval_count": materialization.sparse_interval_count,
        "interpolated_interval_count": materialization.interpolated_interval_count,
        "fallback_interval_count": materialization.fallback_interval_count,
        "skipped_local_hour_interval_count": materialization.skipped_local_hour_interval_count,
        "repeated_local_hour_interval_count": materialization.repeated_local_hour_interval_count,
        "interpolation_method": materialization.interpolation_method,
        "aggregation_method": materialization.aggregation_method,
        "sparse_fallback_policy": materialization.sparse_fallback_policy,
        "sparse_hour_minimum_observation_count": materialization.sparse_hour_minimum_observation_count,
        "interval_boundary_policy": materialization.interval_boundary_policy,
        "interval_alignment_policy": materialization.interval_alignment_policy,
        "timezone_transition_policy": materialization.timezone_transition_policy,
        "spring_forward_policy": materialization.spring_forward_policy,
        "fall_back_policy": materialization.fall_back_policy,
    }


def build_hourly_news_intensity_materialization_payload(
    materialization: HourlyNewsIntensityMaterialization,
) -> dict[str, Any]:
    return {
        "schema_version": materialization.schema_version,
        "exchange_code": materialization.exchange_code,
        "exchange_timezone_name": materialization.exchange_timezone_name,
        "request_start_utc_nanos": materialization.request_start_utc_nanos,
        "request_end_utc_nanos": materialization.request_end_utc_nanos,
        "interval_duration_nanos": materialization.interval_duration_nanos,
        "interval_indices": list(materialization.interval_indices),
        "interval_start_utc_nanos": list(materialization.interval_start_utc_nanos),
        "interval_end_utc_nanos": list(materialization.interval_end_utc_nanos),
        "interval_start_exchange_local": list(materialization.interval_start_exchange_local),
        "interval_end_exchange_local": list(materialization.interval_end_exchange_local),
        "known_component_nu_h": list(materialization.known_component_nu_h),
        "surprise_component_nu_h": list(materialization.surprise_component_nu_h),
        "nu_h": list(materialization.nu_h),
        "interval_count": materialization.interval_count,
        "known_component_non_zero_interval_count": materialization.known_component_non_zero_interval_count,
        "surprise_component_non_zero_interval_count": materialization.surprise_component_non_zero_interval_count,
        "combined_component_non_zero_interval_count": materialization.combined_component_non_zero_interval_count,
        "known_scheduled_arrival_count": materialization.known_scheduled_arrival_count,
        "surprise_multiplier_override_interval_count": materialization.surprise_multiplier_override_interval_count,
        "skipped_local_hour_interval_count": materialization.skipped_local_hour_interval_count,
        "repeated_local_hour_interval_count": materialization.repeated_local_hour_interval_count,
        "materialization_method": materialization.materialization_method,
        "component_aggregation_policy": materialization.component_aggregation_policy,
        "arrival_intensity_policy": materialization.arrival_intensity_policy,
        "known_news_calendar_anchor_policy": materialization.known_news_calendar_anchor_policy,
        "surprise_news_intensity_control_policy": materialization.surprise_news_intensity_control_policy,
        "known_news_calendar_anchor_count": materialization.known_news_calendar_anchor_count,
        "surprise_baseline_intensity": materialization.surprise_baseline_intensity,
        "interval_boundary_policy": materialization.interval_boundary_policy,
        "interval_alignment_policy": materialization.interval_alignment_policy,
        "timezone_transition_policy": materialization.timezone_transition_policy,
        "spring_forward_policy": materialization.spring_forward_policy,
        "fall_back_policy": materialization.fall_back_policy,
    }


def build_calendar_registry(calendar_fixture_document: dict[str, Any]) -> CalendarRegistry:
    return build_calendar_registry_from_document(calendar_fixture_document)


def build_calendar_registry_from_document(calendar_registry_document: dict[str, Any]) -> CalendarRegistry:
    calendar_registry_payload: dict[str, Any]
    if "calendar_registry" in calendar_registry_document:
        calendar_registry_payload = extract_calendar_registry_payload(
            calendar_registry_document,
            field_path="calendar_registry",
        )
    else:
        calendar_registry_payload = extract_calendar_registry_payload(
            calendar_registry_document,
            field_path="calendar_registry_document",
        )

    return CalendarRegistry(json.dumps(calendar_registry_payload))


def extract_calendar_registry_payload(
    calendar_registry_document: dict[str, Any],
    *,
    field_path: str,
) -> dict[str, Any]:
    schema_version = calendar_registry_document.get("schema_version")
    if isinstance(schema_version, str) and schema_version == "calendar.registry.v1":
        return calendar_registry_document

    calendar_registry_payload: object = calendar_registry_document.get("calendar_registry")
    if not isinstance(calendar_registry_payload, dict):
        raise ValueError(f"{field_path} must be a calendar.registry.v1 JSON object")

    return cast(dict[str, Any], calendar_registry_payload)


def extract_event_records(event_records_document: dict[str, Any]) -> list[dict[str, Any]]:
    raw_event_records: object = event_records_document.get("event_records")
    if not isinstance(raw_event_records, list):
        raise ValueError("event records document must include list field `event_records`")

    parsed_event_records: list[dict[str, Any]] = []
    for raw_event_record in raw_event_records:
        if not isinstance(raw_event_record, dict):
            raise ValueError("event records document record must be an object")

        parsed_event_records.append(cast(dict[str, Any], raw_event_record))

    return parsed_event_records


def attach_runtime_hourly_factor_contract(
    *,
    request: SeededTickSimulationRequest,
    calendar_registry: CalendarRegistry,
) -> SeededTickSimulationRequest:
    interval_duration_nanos = request.hourly_factor_summary.interval_duration_nanos
    range_end_utc_nanos = datetime_to_utc_nanos(request.start_time_utc) + (
        request.horizon_interval_count * interval_duration_nanos
    )
    whole_seconds, remainder_nanos = divmod(range_end_utc_nanos, 1_000_000_000)
    range_end_utc = datetime.fromtimestamp(whole_seconds, tz=UTC) + timedelta(
        microseconds=remainder_nanos // 1_000,
    )

    hourly_factor_contract = provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=request.hourly_factor_summary.exchange_code,
        range_start_utc=request.start_time_utc,
        range_end_utc=range_end_utc,
        interval=f"{interval_duration_nanos}ns",
        seasonality_source=HourlySeasonalityProviderExplicitSource(
            hourly_factors=request.hourly_factor_summary.s_h,
            materialization_method="cli.request_hourly_factor_summary.v1",
        ),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
            hourly_factors=request.hourly_factor_summary.nu_h,
            materialization_method="cli.request_hourly_factor_summary.v1",
        ),
    )

    derived_summary = SeededTickSimulationHourlyFactorSummary.from_contract(hourly_factor_contract)
    if derived_summary != request.hourly_factor_summary:
        raise cli_validation_error(
            "request_json.hourly_factor_summary",
            "request hourly_factor_summary must match the rehydrated hourly factor contract",
            error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
            context={
                "derived_interval_count": derived_summary.interval_count,
                "request_interval_count": request.hourly_factor_summary.interval_count,
            },
        )

    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_endogenous_seeded_mean_reversion_runtime_inputs(
        hourly_factor_contract=hourly_factor_contract,
    )

    return SeededTickSimulationRequest.from_runtime_inputs(
        seed=request.seed,
        start_time_utc=request.start_time_utc,
        horizon_interval_count=request.horizon_interval_count,
        kernel_parameters=request.kernel_parameters,
        nig_shared_plus_shift_configuration=request.nig_shared_plus_shift_configuration,
        news_process_configuration=request.news_process_configuration,
        initial_excitation_state=request.initial_excitation_state,
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
        hourly_factor_contract=hourly_factor_contract,
        debug_trace_enabled=request.debug_trace_enabled,
        sample_every_nth_transition=request.sample_every_nth_transition,
        max_records_per_chunk=request.max_records_per_chunk,
        state_snapshot_capture=request.state_snapshot_capture,
    )


def override_calibration_execution_mode(
    *,
    execution_input: StagedCalibrationExecutionInput,
    execution_mode: CalibrationExecutionMode,
    disk_checkpoint_directory: Optional[Path],
) -> StagedCalibrationExecutionInput:
    if execution_mode == CalibrationExecutionMode.MEMORY and disk_checkpoint_directory is not None:
        raise cli_validation_error(
            "disk_checkpoint_directory",
            "--disk-checkpoint-directory requires --execution-mode disk",
            error_id=CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
            entrypoint=CLI_CALIBRATION_STAGED_ENTRYPOINT,
        )

    runtime_resources: Optional[CalibrationStagedExecutionRuntimeResources] = execution_input.runtime_resources
    if execution_mode == CalibrationExecutionMode.DISK and disk_checkpoint_directory is not None:
        runtime_resources = CalibrationStagedExecutionRuntimeResources(
            disk_checkpoint_directory=str(disk_checkpoint_directory),
        )
    if execution_mode == CalibrationExecutionMode.MEMORY:
        runtime_resources = None

    return validate_staged_calibration_execution_input(
        {
            **execution_input.to_payload(),
            "request": {
                **execution_input.request.to_payload(),
                "execution_mode": execution_mode.value,
            },
            "runtime_resources": None if runtime_resources is None else runtime_resources.to_payload(),
        }
    )
