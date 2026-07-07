from __future__ import annotations

from datetime import UTC, datetime
from typing import Iterable
from zoneinfo import ZoneInfo

from janiform._timestamps import NANOS_PER_HOUR
from janiform.interval_factor_grids import IntervalFactorGridContract, load_interval_factor_grid_payload


def build_interval_factor_grid_contract(
    *,
    request_start_utc_nanos: int,
    interval_duration_nanos: int = NANOS_PER_HOUR,
    exchange_code: str = "XNYS",
    exchange_timezone_name: str = "America/New_York",
    seasonality_factor: Iterable[float],
    known_news_intensity: Iterable[float],
    surprise_news_intensity: Iterable[float],
    seasonality_materialization_method: str = "test.explicit_payload.v1",
    news_intensity_materialization_method: str = "test.explicit_payload.v1",
) -> IntervalFactorGridContract:
    seasonality_factors = tuple(float(value) for value in seasonality_factor)
    known_component_values = tuple(float(value) for value in known_news_intensity)
    surprise_component_values = tuple(float(value) for value in surprise_news_intensity)

    if len(known_component_values) != len(seasonality_factors):
        raise ValueError("known_news_intensity must match seasonality_factor length")
    if len(surprise_component_values) != len(seasonality_factors):
        raise ValueError("surprise_news_intensity must match seasonality_factor length")

    interval_count = len(seasonality_factors)
    interval_indices = list(range(interval_count))
    interval_start_utc_nanos = [
        request_start_utc_nanos + interval_index * interval_duration_nanos
        for interval_index in interval_indices
    ]
    interval_end_utc_nanos = [
        interval_start_utc_nanos[interval_index] + interval_duration_nanos
        for interval_index in interval_indices
    ]
    exchange_timezone = ZoneInfo(exchange_timezone_name)
    total_news_intensity = [
        known_component_values[interval_index] + surprise_component_values[interval_index]
        for interval_index in interval_indices
    ]

    return load_interval_factor_grid_payload(
        {
            "schema_version": "seasonality.interval_factor_grid.contract.v1",
            "exchange_code": exchange_code,
            "exchange_timezone_name": exchange_timezone_name,
            "request_start_utc_nanos": request_start_utc_nanos,
            "request_end_utc_nanos": (
                interval_end_utc_nanos[-1]
                if interval_end_utc_nanos
                else request_start_utc_nanos
            ),
            "interval_duration_nanos": interval_duration_nanos,
            "interval_indices": interval_indices,
            "interval_start_utc_nanos": interval_start_utc_nanos,
            "interval_end_utc_nanos": interval_end_utc_nanos,
            "interval_start_exchange_local": [
                _exchange_local_timestamp(
                    interval_start_utc_nanos[interval_index],
                    exchange_timezone,
                )
                for interval_index in interval_indices
            ],
            "interval_end_exchange_local": [
                _exchange_local_timestamp(
                    interval_end_utc_nanos[interval_index],
                    exchange_timezone,
                )
                for interval_index in interval_indices
            ],
            "seasonality_factor": list(seasonality_factors),
            "known_news_intensity": list(known_component_values),
            "surprise_news_intensity": list(surprise_component_values),
            "news_intensity": total_news_intensity,
            "interval_count": interval_count,
            "first_interval_start_utc_nanos": (
                interval_start_utc_nanos[0]
                if interval_start_utc_nanos
                else request_start_utc_nanos
            ),
            "last_interval_end_utc_nanos": (
                interval_end_utc_nanos[-1]
                if interval_end_utc_nanos
                else request_start_utc_nanos
            ),
            "skipped_exchange_local_interval_count": 0,
            "repeated_exchange_local_interval_count": 0,
            "interval_boundary_policy": "start_inclusive_end_exclusive.v1",
            "interval_alignment_policy": "request_start_anchored_fixed_width_utc_interval_grid.v1",
            "timezone_transition_policy": "fixed_width_utc_interval_projected_to_exchange_local.v1",
            "spring_forward_policy": "skip_nonexistent_exchange_local_interval_labels.v1",
            "fall_back_policy": "preserve_repeated_exchange_local_interval_labels_with_numeric_utc_offset.v1",
            "seasonality_materialization_method": seasonality_materialization_method,
            "news_intensity_materialization_method": news_intensity_materialization_method,
        }
    )


def _exchange_local_timestamp(timestamp_utc_nanos: int, exchange_timezone: ZoneInfo) -> str:
    timestamp_utc = datetime.fromtimestamp(timestamp_utc_nanos / 1_000_000_000, UTC)
    return timestamp_utc.astimezone(exchange_timezone).isoformat()
