from __future__ import annotations

from typing import Iterable

from janiform._timestamps import ONE_HOUR_NANOS
from janiform.hourly_factors import HourlyFactorContract, load_hourly_factor_contract_payload


def build_hourly_factor_contract(
    *,
    request_start_utc_nanos: int,
    interval_duration_nanos: int = ONE_HOUR_NANOS,
    exchange_code: str = "XNYS",
    exchange_timezone_name: str = "America/New_York",
    s_h: Iterable[float],
    known_component_nu_h: Iterable[float],
    surprise_component_nu_h: Iterable[float],
    seasonality_materialization_method: str = "test.explicit_payload.v1",
    news_intensity_materialization_method: str = "test.explicit_payload.v1",
) -> HourlyFactorContract:
    seasonality_factors = tuple(float(value) for value in s_h)
    known_component_values = tuple(float(value) for value in known_component_nu_h)
    surprise_component_values = tuple(float(value) for value in surprise_component_nu_h)

    if len(known_component_values) != len(seasonality_factors):
        raise ValueError("known_component_nu_h must match s_h length")
    if len(surprise_component_values) != len(seasonality_factors):
        raise ValueError("surprise_component_nu_h must match s_h length")

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
    total_nu_h = [
        known_component_values[interval_index] + surprise_component_values[interval_index]
        for interval_index in interval_indices
    ]

    return load_hourly_factor_contract_payload(
        {
            "schema_version": "seasonality.hourly_factor.contract.v1",
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
                f"test_interval_start_{interval_index}"
                for interval_index in interval_indices
            ],
            "interval_end_exchange_local": [
                f"test_interval_end_{interval_index}"
                for interval_index in interval_indices
            ],
            "s_h": list(seasonality_factors),
            "known_component_nu_h": list(known_component_values),
            "surprise_component_nu_h": list(surprise_component_values),
            "nu_h": total_nu_h,
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
            "skipped_local_hour_interval_count": 0,
            "repeated_local_hour_interval_count": 0,
            "interval_boundary_policy": "start_inclusive_end_exclusive.v1",
            "interval_alignment_policy": "request_start_anchored_fixed_width_utc_interval_grid.v1",
            "timezone_transition_policy": "fixed_width_utc_interval_projected_to_exchange_local.v1",
            "spring_forward_policy": "skip_nonexistent_local_hour_labels.v1",
            "fall_back_policy": "preserve_repeated_local_hour_labels_with_numeric_utc_offset.v1",
            "seasonality_materialization_method": seasonality_materialization_method,
            "news_intensity_materialization_method": news_intensity_materialization_method,
        }
    )
