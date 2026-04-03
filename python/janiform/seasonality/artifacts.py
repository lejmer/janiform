"""Internal runtime artifacts for seasonality workflows."""

from __future__ import annotations

from typing import Optional, cast


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
