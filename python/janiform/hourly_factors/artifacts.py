"""Internal runtime artifacts for hourly-factor workflows."""

from __future__ import annotations

from typing import Any, cast

from .._native_types import (
    HourlyFactorContractResultProtocol,
    HourlyNewsIntensityMaterializationResultProtocol,
    HourlySeasonalityMaterializationResultProtocol,
)


class HourlyFactorContract:
    """Hourly `s_h` and `nu_h` values aligned to deterministic `[start, end)` UTC hour buckets."""

    def __init__(self, native_result: HourlyFactorContractResultProtocol) -> None:
        self._native_result = native_result

    def __eq__(self, other: object) -> bool:
        return isinstance(other, HourlyFactorContract) and self.to_payload() == other.to_payload()

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def exchange_code(self) -> str:
        return cast(str, self._native_result.exchange_code)

    @property
    def exchange_timezone_name(self) -> str:
        return cast(str, self._native_result.exchange_timezone_name)

    @property
    def request_start_utc_nanos(self) -> int:
        return cast(int, self._native_result.request_start_utc_nanos)

    @property
    def request_end_utc_nanos(self) -> int:
        return cast(int, self._native_result.request_end_utc_nanos)

    @property
    def interval_duration_nanos(self) -> int:
        return cast(int, self._native_result.interval_duration_nanos)

    @property
    def interval_indices(self) -> list[int]:
        return cast(list[int], self._native_result.interval_indices)

    @property
    def interval_start_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.interval_start_utc_nanos)

    @property
    def interval_end_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.interval_end_utc_nanos)

    @property
    def interval_start_exchange_local(self) -> list[str]:
        return cast(list[str], self._native_result.interval_start_exchange_local)

    @property
    def interval_end_exchange_local(self) -> list[str]:
        return cast(list[str], self._native_result.interval_end_exchange_local)

    @property
    def s_h(self) -> list[float]:
        return cast(list[float], self._native_result.s_h)

    @property
    def known_component_nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.known_component_nu_h)

    @property
    def surprise_component_nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.surprise_component_nu_h)

    @property
    def nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.nu_h)

    @property
    def interval_count(self) -> int:
        return cast(int, self._native_result.interval_count)

    @property
    def first_interval_start_utc_nanos(self) -> int:
        return cast(int, self._native_result.first_interval_start_utc_nanos)

    @property
    def last_interval_end_utc_nanos(self) -> int:
        return cast(int, self._native_result.last_interval_end_utc_nanos)

    @property
    def skipped_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.skipped_local_hour_interval_count)

    @property
    def repeated_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.repeated_local_hour_interval_count)

    @property
    def interval_boundary_policy(self) -> str:
        return cast(str, self._native_result.interval_boundary_policy)

    @property
    def interval_alignment_policy(self) -> str:
        return cast(str, self._native_result.interval_alignment_policy)

    @property
    def timezone_transition_policy(self) -> str:
        return cast(str, self._native_result.timezone_transition_policy)

    @property
    def spring_forward_policy(self) -> str:
        return cast(str, self._native_result.spring_forward_policy)

    @property
    def fall_back_policy(self) -> str:
        return cast(str, self._native_result.fall_back_policy)

    @property
    def seasonality_materialization_method(self) -> str:
        return cast(str, self._native_result.seasonality_materialization_method)

    @property
    def news_intensity_materialization_method(self) -> str:
        return cast(str, self._native_result.news_intensity_materialization_method)

    def to_payload(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "exchange_code": self.exchange_code,
            "exchange_timezone_name": self.exchange_timezone_name,
            "request_start_utc_nanos": self.request_start_utc_nanos,
            "request_end_utc_nanos": self.request_end_utc_nanos,
            "interval_duration_nanos": self.interval_duration_nanos,
            "interval_indices": list(self.interval_indices),
            "interval_start_utc_nanos": list(self.interval_start_utc_nanos),
            "interval_end_utc_nanos": list(self.interval_end_utc_nanos),
            "interval_start_exchange_local": list(self.interval_start_exchange_local),
            "interval_end_exchange_local": list(self.interval_end_exchange_local),
            "s_h": list(self.s_h),
            "known_component_nu_h": list(self.known_component_nu_h),
            "surprise_component_nu_h": list(self.surprise_component_nu_h),
            "nu_h": list(self.nu_h),
            "interval_count": self.interval_count,
            "first_interval_start_utc_nanos": self.first_interval_start_utc_nanos,
            "last_interval_end_utc_nanos": self.last_interval_end_utc_nanos,
            "skipped_local_hour_interval_count": self.skipped_local_hour_interval_count,
            "repeated_local_hour_interval_count": self.repeated_local_hour_interval_count,
            "interval_boundary_policy": self.interval_boundary_policy,
            "interval_alignment_policy": self.interval_alignment_policy,
            "timezone_transition_policy": self.timezone_transition_policy,
            "spring_forward_policy": self.spring_forward_policy,
            "fall_back_policy": self.fall_back_policy,
            "seasonality_materialization_method": self.seasonality_materialization_method,
            "news_intensity_materialization_method": self.news_intensity_materialization_method,
        }

class HourlySeasonalityMaterialization:
    """Hourly `s_h` values aligned to deterministic `[start, end)` UTC hour buckets."""

    def __init__(self, native_result: HourlySeasonalityMaterializationResultProtocol) -> None:
        self._native_result = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def exchange_code(self) -> str:
        return cast(str, self._native_result.exchange_code)

    @property
    def exchange_timezone_name(self) -> str:
        return cast(str, self._native_result.exchange_timezone_name)

    @property
    def request_start_utc_nanos(self) -> int:
        return cast(int, self._native_result.request_start_utc_nanos)

    @property
    def request_end_utc_nanos(self) -> int:
        return cast(int, self._native_result.request_end_utc_nanos)

    @property
    def interval_duration_nanos(self) -> int:
        return cast(int, self._native_result.interval_duration_nanos)

    @property
    def interval_indices(self) -> list[int]:
        return cast(list[int], self._native_result.interval_indices)

    @property
    def interval_start_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.interval_start_utc_nanos)

    @property
    def interval_end_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.interval_end_utc_nanos)

    @property
    def interval_start_exchange_local(self) -> list[str]:
        return cast(list[str], self._native_result.interval_start_exchange_local)

    @property
    def interval_end_exchange_local(self) -> list[str]:
        return cast(list[str], self._native_result.interval_end_exchange_local)

    @property
    def s_h(self) -> list[float]:
        return cast(list[float], self._native_result.s_h)

    @property
    def source_point_count(self) -> int:
        return cast(int, self._native_result.source_point_count)

    @property
    def interval_count(self) -> int:
        return cast(int, self._native_result.interval_count)

    @property
    def sparse_interval_count(self) -> int:
        return cast(int, self._native_result.sparse_interval_count)

    @property
    def interpolated_interval_count(self) -> int:
        return cast(int, self._native_result.interpolated_interval_count)

    @property
    def fallback_interval_count(self) -> int:
        return cast(int, self._native_result.fallback_interval_count)

    @property
    def skipped_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.skipped_local_hour_interval_count)

    @property
    def repeated_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.repeated_local_hour_interval_count)

    @property
    def interpolation_method(self) -> str:
        return cast(str, self._native_result.interpolation_method)

    @property
    def aggregation_method(self) -> str:
        return cast(str, self._native_result.aggregation_method)

    @property
    def sparse_fallback_policy(self) -> str:
        return cast(str, self._native_result.sparse_fallback_policy)

    @property
    def sparse_hour_minimum_observation_count(self) -> int:
        return cast(int, self._native_result.sparse_hour_minimum_observation_count)

    @property
    def interval_boundary_policy(self) -> str:
        return cast(str, self._native_result.interval_boundary_policy)

    @property
    def interval_alignment_policy(self) -> str:
        return cast(str, self._native_result.interval_alignment_policy)

    @property
    def timezone_transition_policy(self) -> str:
        return cast(str, self._native_result.timezone_transition_policy)

    @property
    def spring_forward_policy(self) -> str:
        return cast(str, self._native_result.spring_forward_policy)

    @property
    def fall_back_policy(self) -> str:
        return cast(str, self._native_result.fall_back_policy)


class HourlyNewsIntensityMaterialization:
    """Hourly `nu_h` values aligned to deterministic `[start, end)` UTC hour buckets."""

    def __init__(self, native_result: HourlyNewsIntensityMaterializationResultProtocol) -> None:
        self._native_result = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def exchange_code(self) -> str:
        return cast(str, self._native_result.exchange_code)

    @property
    def exchange_timezone_name(self) -> str:
        return cast(str, self._native_result.exchange_timezone_name)

    @property
    def request_start_utc_nanos(self) -> int:
        return cast(int, self._native_result.request_start_utc_nanos)

    @property
    def request_end_utc_nanos(self) -> int:
        return cast(int, self._native_result.request_end_utc_nanos)

    @property
    def interval_duration_nanos(self) -> int:
        return cast(int, self._native_result.interval_duration_nanos)

    @property
    def interval_indices(self) -> list[int]:
        return cast(list[int], self._native_result.interval_indices)

    @property
    def interval_start_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.interval_start_utc_nanos)

    @property
    def interval_end_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.interval_end_utc_nanos)

    @property
    def interval_start_exchange_local(self) -> list[str]:
        return cast(list[str], self._native_result.interval_start_exchange_local)

    @property
    def interval_end_exchange_local(self) -> list[str]:
        return cast(list[str], self._native_result.interval_end_exchange_local)

    @property
    def known_component_nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.known_component_nu_h)

    @property
    def surprise_component_nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.surprise_component_nu_h)

    @property
    def nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.nu_h)

    @property
    def interval_count(self) -> int:
        return cast(int, self._native_result.interval_count)

    @property
    def known_component_non_zero_interval_count(self) -> int:
        return cast(int, self._native_result.known_component_non_zero_interval_count)

    @property
    def surprise_component_non_zero_interval_count(self) -> int:
        return cast(int, self._native_result.surprise_component_non_zero_interval_count)

    @property
    def combined_component_non_zero_interval_count(self) -> int:
        return cast(int, self._native_result.combined_component_non_zero_interval_count)

    @property
    def known_scheduled_arrival_count(self) -> int:
        return cast(int, self._native_result.known_scheduled_arrival_count)

    @property
    def surprise_multiplier_override_interval_count(self) -> int:
        return cast(int, self._native_result.surprise_multiplier_override_interval_count)

    @property
    def skipped_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.skipped_local_hour_interval_count)

    @property
    def repeated_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.repeated_local_hour_interval_count)

    @property
    def materialization_method(self) -> str:
        return cast(str, self._native_result.materialization_method)

    @property
    def component_aggregation_policy(self) -> str:
        return cast(str, self._native_result.component_aggregation_policy)

    @property
    def arrival_intensity_policy(self) -> str:
        return cast(str, self._native_result.arrival_intensity_policy)

    @property
    def known_news_calendar_anchor_policy(self) -> str:
        return cast(str, self._native_result.known_news_calendar_anchor_policy)

    @property
    def surprise_news_intensity_control_policy(self) -> str:
        return cast(str, self._native_result.surprise_news_intensity_control_policy)

    @property
    def known_news_calendar_anchor_count(self) -> int:
        return cast(int, self._native_result.known_news_calendar_anchor_count)

    @property
    def surprise_baseline_intensity(self) -> float:
        return cast(float, self._native_result.surprise_baseline_intensity)

    @property
    def interval_boundary_policy(self) -> str:
        return cast(str, self._native_result.interval_boundary_policy)

    @property
    def interval_alignment_policy(self) -> str:
        return cast(str, self._native_result.interval_alignment_policy)

    @property
    def timezone_transition_policy(self) -> str:
        return cast(str, self._native_result.timezone_transition_policy)

    @property
    def spring_forward_policy(self) -> str:
        return cast(str, self._native_result.spring_forward_policy)

    @property
    def fall_back_policy(self) -> str:
        return cast(str, self._native_result.fall_back_policy)
